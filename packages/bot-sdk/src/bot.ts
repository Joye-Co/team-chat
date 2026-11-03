// Bot: one WebSocket connection with reconnect, ordered event delivery per group, catch-up
// after restarts from a cursor file, and handlers for mentions, followed topics and card
// presses. Writes wait for the connection and are resent after a reconnect; `send` is
// idempotent by clientMsgId, so a resend never posts twice.
import { readFileSync, statSync } from "node:fs";
import {
  AcceptanceItem,
  type ArtifactRef,
  type BotRun,
  type Card,
  type CardV2,
  type CardV2Action,
  ClientFrame,
  Event as EventSchema,
  type Event,
  type EventOf,
  type FileRef,
  type GroupSummary,
  type Member,
  type Message,
  PROTOCOL_VERSION,
  RunPlan,
  type RunPlanItem,
  type SendResult,
  type Topic,
  type TopicContext,
  type TopicState,
  type TopicStatus,
  type TransientEvent,
} from "@chat/protocol";
import { answeredCard, checkCardV2, type DoneCardOptions, doneCard } from "./cards.ts";
import { ChatError } from "./errors.ts";
import { HttpClient, normalizeBaseUrl, type UploadInput } from "./http.ts";
import { consoleLogger, type Logger, type LogLevel } from "./log.ts";
import {
  type AsideRef,
  type BotStateData,
  type CardRef,
  emptyState,
  FileStateStore,
  MAX_REMEMBERED_CARDS,
  MAX_REMEMBERED_REFS,
  MemoryStateStore,
  type StateStore,
} from "./state.ts";
import { type StreamOptions, StreamWriter } from "./stream.ts";

export const SDK_VERSION = "1.1.0";

/** The reaction `ack()` adds by default: "I saw it and work on it". */
export const ACK_EMOJI = "👀";

/** A WebSocket constructor that accepts `{ headers }` (Bun, and Node 22+ through undici). */
export type WebSocketCtor = new (url: string, opts?: { headers?: Record<string, string> }) => WebSocket;

export interface BotOptions {
  /** Server base URL, for example `http://127.0.0.1:8789` or `https://chat-api-....workers.dev`. */
  url: string;
  /** The bot token. Prefer `tokenFile`. */
  token?: string;
  /** File that holds the bot token (mode 600). */
  tokenFile?: string;
  /** Cursor and follow state file. Without it, state lives in memory and a restart starts at the newest events. */
  stateFile?: string;
  /** Custom state store (instead of `stateFile`). */
  stateStore?: StateStore;
  /** Client name and version sent in `hello`. */
  client?: { name: string; version: string };
  logger?: Logger;
  logLevel?: LogLevel;
  /**
   * When there is no saved state at all: "latest" (default) skips the history and handles only
   * new events; "all" handles every stored event. Groups the bot joins later are always read
   * from their start, so a mention that came with the invitation is not lost.
   */
  firstStart?: "latest" | "all";
  /** Move the bot's read position after it handles messages. Default true. */
  markRead?: boolean;
  /** How long a request waits (connection included) before it fails with `timeout`. Default 60 s. */
  requestTimeoutMs?: number;
  /** How long `stop()` waits for running handlers. Default 10 s. */
  stopTimeoutMs?: number;
  /**
   * Run `onMention` and `onMessage` for messages from other bots too. Default false: two bots
   * that answer each other would loop until the rate limit stops them. Turn it on only when
   * the handlers cannot answer a bot with a message that reaches that bot again.
   */
  allowBotAuthors?: boolean;
  WebSocket?: WebSocketCtor;
  fetch?: typeof fetch;
}

/** Where to post: a topic, or a group (starts a new topic). */
export interface Target {
  groupId: string;
  topicId?: string;
}

export interface SendInput {
  body?: string;
  /** Title of a new topic (only without topicId). */
  title?: string;
  mentions?: string[];
  /** Quote this message. */
  replyTo?: string;
  attachments?: FileRef[];
  artifactRefs?: ArtifactRef[];
  card?: Card;
  /** Card v2 (1.1). The server derives the v1 `card` for older readers when `card` is absent. */
  cardV2?: CardV2;
  /** Side chat (1.1): the topic this DM topic is about. Only in a DM with a bot. */
  context?: TopicContext;
  streaming?: boolean;
  /** Idempotency key. Made when absent; pass a stable one to make retries safe across restarts. */
  clientMsgId?: string;
}

export interface EditInput {
  body: string;
  streaming?: boolean;
  card?: Card;
  /** Card v2 (1.1): replaces the card; `rev` must be higher than the stored one. */
  cardV2?: CardV2;
  attachments?: FileRef[];
  artifactRefs?: ArtifactRef[];
}

export interface TopicChange {
  title?: string;
  status?: TopicStatus;
  /** Replaces the whole list. */
  waitingOn?: string[];
  /** null clears the owner. */
  ownerId?: string | null;
}

export interface MessageContext {
  bot: Bot;
  event: EventOf<"message.created">;
  message: Message;
  groupId: string;
  topicId: string;
  /** The event came from catch-up (it happened while the bot was away), not live. */
  catchUp: boolean;
  /** Posts in the same topic. Retries after a restart reuse the same clientMsgId, so no duplicate. */
  reply(input: string | SendInput): Promise<SendResult>;
  /** Starts a streaming reply in the same topic. */
  stream(input?: string | (SendInput & StreamOptions)): Promise<StreamWriter>;
  /** Changes this topic's status, waitingOn, owner or title. */
  updateTopic(change: TopicChange): Promise<Topic>;
  /** Shows "typing" in this topic. */
  typing(): void;
  /**
   * Sets the plan of this bot's current run in this topic, and starts the run when there is
   * none. Each call replaces the whole plan; `[]` clears it. See `Bot.setPlan`.
   */
  setPlan(plan: RunPlanItem[]): Promise<BotRun>;
  /** Ends this bot's current run in this topic (see `Bot.endRun`). */
  endRun(status: "done" | "failed" | "cancelled", summary?: string): Promise<BotRun | null>;
  /** Reacts to the message (default 👀) to say "seen, working on it", instead of a text reply. */
  ack(emoji?: string): Promise<boolean>;
  /** Posts a Card v2 (an ask, usually) in this topic. */
  ask(card: CardV2, input?: Omit<SendInput, "card" | "cardV2">): Promise<SendResult>;
  /**
   * Posts the result card (Card v2, intent "done") in this topic and ends this bot's current run
   * there: `failed` ends it as failed. See `Bot.done`.
   */
  done(card: CardV2 | DoneCardOptions): Promise<DoneResult>;
  /** Replaces the 验收标准 of this bot's current run in this topic (see `Bot.setAcceptance`). */
  setAcceptance(items: AcceptanceItem[]): Promise<BotRun>;
  /** Set when the message is in a side chat (单独问): the topic the side chat is about. */
  aside?: TopicContext;
}

/** A message in a side chat (单独问): a DM topic with this bot about another topic. */
export interface AsideContext extends MessageContext {
  aside: TopicContext;
  /**
   * The messages of the topic the side chat is about, oldest first, up to `aside.uptoSeq`:
   * at most `limit` (default 200) of the newest ones. Deleted messages are left out.
   */
  source(opts?: { limit?: number }): Promise<Message[]>;
}

/** What `done` posted, and the run it ended (null when the bot had no run in the topic). */
export interface DoneResult {
  result: SendResult;
  run: BotRun | null;
}

/** JSON values of an answer. */
export type AnswerValue = string | number | boolean | null | AnswerValue[] | { [k: string]: AnswerValue };

/**
 * An answer to one of my v2 cards (`card.answered`). `I` types `inputs` by block id, for
 * example `onCardAnswer<{ choice: string; when: string }>(...)`; the values come from the
 * client, so check them before you act on them.
 */
export interface CardAnswerContext<I extends Record<string, unknown> = Record<string, AnswerValue>> {
  bot: Bot;
  event: EventOf<"card.answered">;
  groupId: string;
  topicId: string;
  messageId: string;
  /** The card revision the answer was made on. */
  rev: number;
  actionId: string;
  /** Values of decision and input blocks, by block id. */
  inputs: Partial<I>;
  /** Edited text of draft blocks, by block id. */
  editedValues: Record<string, string>;
  /** Who answered. */
  actorId: string;
  catchUp: boolean;
  /** The answered card, when this process saw it (sent or caught up). */
  card?: CardV2;
  /** The answered action of the card, when the card is known. */
  action?: CardV2Action;
  /** The answer is a cancel (a `cancel` action, or the ids "cancel" and "deny" when the card is not known). */
  cancelled: boolean;
  /** Chosen option ids of a decision block (default id "choice"); [] when none. */
  choices(blockId?: string): string[];
  /** The first chosen option id of a decision block (default id "choice"). */
  choice(blockId?: string): string | undefined;
  /** An input as text (numbers and booleans are turned into text). */
  text(blockId: string): string | undefined;
  /** An input as a number (text that is a number is read too). */
  number(blockId: string): number | undefined;
  /** A switch input. */
  bool(blockId: string): boolean | undefined;
  /** The draft text (default id "draft"): the edited text, else the card's own text. */
  draft(blockId?: string): string | undefined;
  /**
   * Edits the card to its answered form: a higher rev and no submit or cancel actions, plus
   * `change`. Needs the card (`card`), or pass `base`.
   */
  resolve(change?: Partial<Omit<CardV2, "v" | "rev">>, base?: CardV2): Promise<Message>;
  reply(input: string | SendInput): Promise<SendResult>;
  updateTopic(change: TopicChange): Promise<Topic>;
}

/** A page event from one of my 产出 (`artifact.evented`). Never treat it as an approval. */
export interface ArtifactEventContext {
  bot: Bot;
  event: EventOf<"artifact.evented">;
  groupId: string;
  /** The topic of the 产出, when this process knows it. */
  topicId?: string;
  artifactId: string;
  version: number;
  name: string;
  payload: unknown;
  /** Who used the page. */
  actorId: string;
  catchUp: boolean;
  /** Posts in the 产出's topic. Needs `topicId`. */
  reply(input: string | SendInput): Promise<SendResult>;
}

export interface CardActionContext {
  bot: Bot;
  event: EventOf<"card.actioned">;
  groupId: string;
  topicId: string;
  messageId: string;
  actionId: string;
  value: unknown;
  /** Who pressed. */
  actorId: string;
  catchUp: boolean;
  /** Replaces the card (for example without its actions). */
  updateCard(card: Card, body?: string): Promise<Message>;
  reply(input: string | SendInput): Promise<SendResult>;
  updateTopic(change: TopicChange): Promise<Topic>;
}

export interface EventContext {
  bot: Bot;
  /** The parsed event, or null for an event type this SDK does not know. */
  event: Event | null;
  raw: { seq: number; groupId: string; type: string; actorId: string; at: number; data: unknown };
  catchUp: boolean;
}

type Handler<C> = (ctx: C) => unknown | Promise<unknown>;

interface Pending {
  rid: string;
  frame: Record<string, unknown> & { t: string };
  resolve: (v: unknown) => void;
  reject: (e: unknown) => void;
  deadline: number;
  sent: boolean;
  attempts: number;
  timer: ReturnType<typeof setTimeout>;
}

interface GroupState {
  /** Last seq applied in memory (handlers may still run). */
  applied: number;
  buffer: Map<number, RawEvent>;
  fetching: boolean;
  /** Seqs whose handlers are still running. */
  running: Set<number>;
  member: boolean;
  readTimer: ReturnType<typeof setTimeout> | null;
  lastReadSent: number;
}

interface RawEvent {
  seq: number;
  groupId: string;
  type: string;
  actorId: string;
  at: number;
  data: unknown;
}

interface TopicInfo {
  waitingOn: string[];
  ownerId: string | null;
}

const PING_MS = 30_000;
const SILENCE_MS = 75_000;
const OPEN_TIMEOUT_MS = 15_000;
const BACKOFF_MIN_MS = 500;
const BACKOFF_MAX_MS = 30_000;
const FETCH_LIMIT = 500;
const SAVE_DEBOUNCE_MS = 500;
const READ_DEBOUNCE_MS = 1500;

/** v2 cards kept in memory for answers (`CardAnswerContext.card`). */
const MAX_CACHED_CARDS = 500;

function cutSummary(s: string): string {
  const cps = [...s];
  return cps.length <= 280 ? s : `${cps.slice(0, 279).join("")}…`;
}

/** Drops the oldest keys (ULIDs sort by time) until at most `max` are left. */
function trimOldest(map: Record<string, unknown>, max: number): void {
  const ids = Object.keys(map);
  if (ids.length <= max) return;
  ids.sort();
  for (const id of ids.slice(0, ids.length - max)) delete map[id];
}

function findBlock(card: CardV2 | undefined, id: string): CardV2["blocks"][number] | undefined {
  for (const b of card?.blocks ?? []) {
    if (b.id === id) return b;
    if (b.type === "group") for (const c of b.blocks) if (c.id === id) return c as CardV2["blocks"][number];
  }
  return undefined;
}

function wsUrl(base: string): string {
  const u = new URL(`${base}/v1/ws`);
  u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
  return u.toString();
}

function readToken(opts: BotOptions, log: Logger): string {
  if (opts.token) return opts.token.trim();
  if (!opts.tokenFile) throw new Error("pass token or tokenFile");
  const mode = statSync(opts.tokenFile).mode & 0o077;
  if (mode !== 0) log.warn("token file can be read by other users; run chmod 600 on it", { file: opts.tokenFile });
  const token = readFileSync(opts.tokenFile, "utf8").trim();
  if (!token) throw new Error(`token file is empty: ${opts.tokenFile}`);
  return token;
}

export class Bot {
  readonly url: string;
  readonly http: HttpClient;
  readonly log: Logger;

  private readonly token: string;
  private readonly opts: BotOptions;
  private readonly WS: WebSocketCtor;
  private readonly store: StateStore;
  private state: BotStateData;
  private readonly firstStart: boolean;

  private ws: WebSocket | null = null;
  private ready = false;
  private stopped = true;
  private fatal: ChatError | null = null;
  private attempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private lastFrameAt = 0;
  private everConnected = false;
  private startWaiters: { resolve: () => void; reject: (e: unknown) => void }[] = [];

  private ridSeq = 0;
  private readonly pending = new Map<string, Pending>();
  private readonly groups = new Map<string, GroupState>();
  private readonly topics = new Map<string, TopicInfo>();
  private readonly streams = new Set<StreamWriter>();
  /** This bot's active run per topic, from `startRun` answers and `run.updated` events. */
  private readonly currentRuns = new Map<string, string>();
  private readonly running = new Set<Promise<unknown>>();
  private saveTimer: ReturnType<typeof setTimeout> | null = null;

  private meMember: Member | null = null;
  private readonly memberMap = new Map<string, Member>();
  private readonly summaryMap = new Map<string, GroupSummary>();

  private readonly mentionHandlers: Handler<MessageContext>[] = [];
  private readonly messageHandlers: Handler<MessageContext>[] = [];
  private readonly cardHandlers: Handler<CardActionContext>[] = [];
  private readonly answerHandlers: Handler<CardAnswerContext>[] = [];
  private readonly artifactEventHandlers: Handler<ArtifactEventContext>[] = [];
  private readonly asideHandlers: Handler<AsideContext>[] = [];
  /** v2 cards of my messages that this process saw, newest last (at most MAX_CACHED_CARDS). */
  private readonly cardCache = new Map<string, CardV2>();
  private readonly eventHandlers: Handler<EventContext>[] = [];
  private readonly transientHandlers: ((e: TransientEvent) => void)[] = [];
  private readonly connectionHandlers: ((up: boolean) => void)[] = [];
  private readonly errorHandlers: ((err: unknown, where: string) => void)[] = [];

  constructor(opts: BotOptions) {
    this.opts = opts;
    this.log = opts.logger ?? consoleLogger(opts.logLevel ?? "info");
    this.url = normalizeBaseUrl(opts.url);
    this.token = readToken(opts, this.log);
    this.http = new HttpClient({ url: this.url, token: this.token, fetch: opts.fetch });
    const WS = opts.WebSocket ?? (globalThis.WebSocket as unknown as WebSocketCtor | undefined);
    if (!WS) throw new Error("no WebSocket implementation: use Bun or Node 22+, or pass the WebSocket option");
    this.WS = WS;
    this.store = opts.stateStore ?? (opts.stateFile ? new FileStateStore(opts.stateFile) : new MemoryStateStore());
    const loaded = this.store.load();
    this.firstStart = loaded === null;
    this.state = loaded ?? emptyState();
  }

  // ---------------------------------------------------------------------------
  // Handlers
  // ---------------------------------------------------------------------------

  /** A message (not mine) that mentions me. */
  onMention(h: Handler<MessageContext>): this {
    this.mentionHandlers.push(h);
    return this;
  }
  /** A message (not mine, not a mention of me, not a system message) in a topic I follow. */
  onMessage(h: Handler<MessageContext>): this {
    this.messageHandlers.push(h);
    return this;
  }
  /** A press on a button of one of my cards. */
  onCardAction(h: Handler<CardActionContext>): this {
    this.cardHandlers.push(h);
    return this;
  }
  /**
   * An answer to one of my v2 cards (1.1). A v1 press on a v2 card arrives here too (with no
   * inputs). Older presses on v1 cards still go to `onCardAction`.
   */
  onCardAnswer<I extends Record<string, unknown> = Record<string, AnswerValue>>(h: Handler<CardAnswerContext<I>>): this {
    this.answerHandlers.push(h as unknown as Handler<CardAnswerContext>);
    return this;
  }
  /** A page event from one of my 产出 (1.1): only names the version declared in `events`. */
  onArtifactEvent(h: Handler<ArtifactEventContext>): this {
    this.artifactEventHandlers.push(h);
    return this;
  }
  /**
   * A message (not mine) in a side chat (单独问, 1.1): a member's DM topic with me about another
   * topic, @ or not. Answer in the DM; the member posts to the topic. Without an `onAside`
   * handler these messages go to `onMention`.
   */
  onAside(h: Handler<AsideContext>): this {
    this.asideHandlers.push(h);
    return this;
  }
  /** Every stored group event, in seq order per group. */
  onEvent(h: Handler<EventContext>): this {
    this.eventHandlers.push(h);
    return this;
  }
  /** Typing, presence and streaming deltas. */
  onTransient(h: (e: TransientEvent) => void): this {
    this.transientHandlers.push(h);
    return this;
  }
  /** true after each welcome, false after each disconnect. */
  onConnection(h: (up: boolean) => void): this {
    this.connectionHandlers.push(h);
    return this;
  }
  /** Handler errors and fatal connection errors. Without a handler they are logged. */
  onError(h: (err: unknown, where: string) => void): this {
    this.errorHandlers.push(h);
    return this;
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  /** Connects. Resolves on the first welcome; rejects on a bad token. Keeps reconnecting after that. */
  start(): Promise<void> {
    if (!this.stopped) return this.ready ? Promise.resolve() : new Promise((resolve, reject) => this.startWaiters.push({ resolve, reject }));
    this.stopped = false;
    this.fatal = null;
    const p = new Promise<void>((resolve, reject) => this.startWaiters.push({ resolve, reject }));
    this.connect();
    return p;
  }

  /** Closes the connection, waits for running handlers (up to stopTimeoutMs) and saves state. */
  async stop(): Promise<void> {
    if (this.stopped) return;
    // Let running handlers finish while the connection is still up.
    let waitTimer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      Promise.allSettled([...this.running]),
      new Promise((r) => {
        waitTimer = setTimeout(r, this.opts.stopTimeoutMs ?? 10_000);
      }),
    ]);
    clearTimeout(waitTimer);
    if (this.stopped) return;
    this.stopped = true;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
    this.stopPing();
    const ws = this.ws;
    this.ws = null;
    this.setReady(false);
    try {
      ws?.close(1000, "stop");
    } catch {
      // already closed
    }
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(new ChatError("closed", "bot stopped"));
    }
    this.pending.clear();
    for (const w of this.startWaiters.splice(0)) w.reject(new ChatError("closed", "bot stopped"));
    for (const g of this.groups.values()) if (g.readTimer) clearTimeout(g.readTimer);
    this.saveNow();
  }

  /** Drops the connection and connects again (for example after a network change). */
  reconnect(): void {
    if (this.stopped) return;
    this.dropSocket("reconnect requested");
  }

  get connected(): boolean {
    return this.ready;
  }
  /** My member record (after the first welcome). */
  get me(): Member {
    if (!this.meMember) throw new ChatError("invalid", "not connected yet");
    return this.meMember;
  }
  get members(): Member[] {
    return [...this.memberMap.values()];
  }
  member(id: string): Member | undefined {
    return this.memberMap.get(id);
  }
  /** Group summaries from the last welcome and directory frames. */
  get groupSummaries(): GroupSummary[] {
    return [...this.summaryMap.values()];
  }
  isFollowing(groupId: string, topicId: string): boolean {
    return this.state.following[groupId]?.includes(topicId) ?? false;
  }
  /** The seq up to which every event of a group is handled. */
  cursor(groupId: string): number {
    return this.state.cursors[groupId] ?? 0;
  }

  // ---------------------------------------------------------------------------
  // Writes
  // ---------------------------------------------------------------------------

  /** Posts a message. Without `target.topicId` it starts a new topic. */
  async send(target: Target, input: string | SendInput): Promise<SendResult> {
    const i: SendInput = typeof input === "string" ? { body: input } : input;
    const frame: Record<string, unknown> & { t: string } = {
      t: "send",
      groupId: target.groupId,
      clientMsgId: i.clientMsgId ?? crypto.randomUUID(),
      body: i.body ?? "",
    };
    if (target.topicId) frame.topicId = target.topicId;
    else if (i.title) frame.title = i.title;
    if (i.mentions?.length) frame.mentions = i.mentions;
    if (i.replyTo) frame.replyTo = { messageId: i.replyTo };
    if (i.attachments?.length) frame.attachments = i.attachments;
    if (i.artifactRefs?.length) frame.artifactRefs = i.artifactRefs;
    if (i.card) frame.card = i.card;
    if (i.cardV2) frame.cardV2 = i.cardV2;
    if (i.context) frame.context = i.context;
    if (i.streaming) frame.streaming = true;
    const result = (await this.request(frame)) as SendResult;
    this.noteOwnMessage(result.message);
    if (result.topic) this.follow(target.groupId, result.topic.id);
    else this.follow(target.groupId, result.message.topicId);
    return result;
  }

  /** Starts a new topic whose root is this message. */
  startTopic(groupId: string, input: string | (SendInput & { title?: string })): Promise<SendResult> {
    return this.send({ groupId }, input);
  }

  /** Replies in the topic of `to`, quoting it when `quote` is true. */
  reply(to: Message, input: string | SendInput, opts: { quote?: boolean } = {}): Promise<SendResult> {
    const i: SendInput = typeof input === "string" ? { body: input } : { ...input };
    if (opts.quote && !i.replyTo) i.replyTo = to.id;
    return this.send({ groupId: to.groupId, topicId: to.topicId }, i);
  }

  /** Posts a card. `body` is the plain-text fallback (default: the card title). */
  postCard(target: Target, card: Card, input: Omit<SendInput, "card"> = {}): Promise<SendResult> {
    return this.send(target, { ...input, card, body: input.body ?? "" });
  }

  /** Replaces one of my cards. */
  async updateCard(groupId: string, messageId: string, card: Card, body?: string): Promise<Message> {
    return (await this.edit(groupId, messageId, { body: body ?? card.title, card })).message;
  }

  /**
   * Posts a Card v2 (1.1). The server makes the v1 card for older readers; `body` defaults to
   * the card's fallback. Build cards with `askCard`, `decisionCard`, `approvalCard`, `draftCard`,
   * `formCard` and `doneCard`.
   */
  postCardV2(target: Target, card: CardV2, input: Omit<SendInput, "card" | "cardV2"> = {}): Promise<SendResult> {
    return this.send(target, { ...input, cardV2: checkCardV2(card), body: input.body ?? "" });
  }

  /** Replaces one of my v2 cards. `card.rev` must be higher than the stored rev. */
  async updateCardV2(groupId: string, messageId: string, card: CardV2, body?: string): Promise<Message> {
    const checked = checkCardV2(card);
    const r = await this.edit(groupId, messageId, { body: body ?? (checked.fallback || checked.title), cardV2: checked });
    this.cacheCard(messageId, checked);
    return r.message;
  }

  /** The v2 card of one of my messages, when this process saw it. */
  knownCard(messageId: string): CardV2 | undefined {
    return this.cardCache.get(messageId);
  }

  /** Answers a v2 card (bots may answer other members' asks when `ask.from` names them). */
  async answerCard(
    groupId: string,
    answer: { messageId: string; rev: number; actionId: string; inputs?: Record<string, AnswerValue>; editedValues?: Record<string, string> },
  ): Promise<void> {
    const frame: Record<string, unknown> & { t: string } = {
      t: "card.answer",
      groupId,
      messageId: answer.messageId,
      rev: answer.rev,
      actionId: answer.actionId,
    };
    if (answer.inputs) frame.inputs = answer.inputs;
    if (answer.editedValues) frame.editedValues = answer.editedValues;
    await this.request(frame);
  }

  /**
   * Posts the result of the work in a topic as a Card v2 with intent "done" (a summary with
   * its proof), then ends this bot's current run there (`failed` ends it as failed). Use it
   * instead of a streaming progress message: progress belongs in the plan.
   */
  async done(target: Required<Target>, card: CardV2 | DoneCardOptions, input: Omit<SendInput, "card" | "cardV2"> = {}): Promise<DoneResult> {
    const c = "v" in card ? checkCardV2(card) : doneCard(card);
    const failed = !("v" in card) && card.failed === true;
    const result = await this.postCardV2(target, c, input);
    const run = await this.endRun(target, failed ? "failed" : "done", cutSummary(c.fallback || c.title));
    return { result, run };
  }

  /**
   * Replaces the 验收标准 (acceptance criteria) of this bot's current run in a topic, and
   * starts a run when there is none. At most 20 items, text 1..500 characters, state
   * `pending`, `pass` or `fail`, with optional evidence. `[]` clears them. Members see them
   * in the live view and the topic rail. Needs a 1.1 server (`PUT /v1/runs/:id/acceptance`).
   */
  async setAcceptance(target: Required<Target>, items: AcceptanceItem[]): Promise<BotRun> {
    const checked = AcceptanceItem.array().max(20).safeParse(items);
    if (!checked.success) throw new ChatError("invalid", checked.error.issues[0]?.message ?? "invalid acceptance items");
    let runId = this.currentRuns.get(target.topicId);
    if (!runId) {
      const started = await this.http.startRun({ groupId: target.groupId, topicId: target.topicId });
      this.noteRun(started);
      runId = started.id;
    }
    let run: BotRun;
    try {
      run = await this.http.setRunAcceptance(runId, checked.data);
    } catch (err) {
      if (!(err instanceof ChatError && err.code === "conflict")) throw err;
      // The run ended meanwhile: start a new one.
      this.currentRuns.delete(target.topicId);
      const started = await this.http.startRun({ groupId: target.groupId, topicId: target.topicId });
      this.noteRun(started);
      run = await this.http.setRunAcceptance(started.id, checked.data);
    }
    this.noteRun(run);
    return run;
  }

  /** Reacts to a message with `emoji` (default 👀): a short "seen, working on it". */
  ack(groupId: string, messageId: string, emoji: string = ACK_EMOJI): Promise<boolean> {
    return this.react(groupId, messageId, emoji);
  }

  /** Starts a streaming message; append text to the writer, then call `finish()`. */
  async stream(target: Target, input: string | (SendInput & StreamOptions) = {}): Promise<StreamWriter> {
    const i = typeof input === "string" ? { body: input } : input;
    const r = await this.send(target, { ...i, streaming: true });
    // A retry of a stream that already finished (duplicate, not streaming): nothing more to write.
    const finished = r.duplicate === true && !r.message.streaming;
    const w = new StreamWriter(this.streamHost, r.message, { ...i, finished });
    if (!finished) this.streams.add(w);
    return w;
  }

  edit(groupId: string, messageId: string, input: EditInput): Promise<{ message: Message }> {
    const frame: Record<string, unknown> & { t: string } = { t: "edit", groupId, messageId, body: input.body };
    if (input.streaming !== undefined) frame.streaming = input.streaming;
    if (input.card) frame.card = input.card;
    if (input.cardV2) frame.cardV2 = input.cardV2;
    if (input.attachments) frame.attachments = input.attachments;
    if (input.artifactRefs) frame.artifactRefs = input.artifactRefs;
    return this.request(frame) as Promise<{ message: Message }>;
  }

  async deleteMessage(groupId: string, messageId: string): Promise<void> {
    await this.request({ t: "delete", groupId, messageId });
  }

  /** Sets status, waitingOn, owner or title of a topic. */
  async updateTopic(groupId: string, topicId: string, change: TopicChange): Promise<Topic> {
    const frame: Record<string, unknown> & { t: string } = { t: "topic.update", groupId, topicId };
    if (change.title !== undefined) frame.title = change.title;
    if (change.status !== undefined) frame.status = change.status;
    if (change.waitingOn !== undefined) frame.waitingOn = change.waitingOn;
    if (change.ownerId !== undefined) frame.ownerId = change.ownerId;
    const r = (await this.request(frame)) as { topic: Topic };
    return r.topic;
  }

  /**
   * Sets the plan of this bot's current run in a topic: its ordered to-do list, which members
   * see in the topic's side panel. Starts a run when the bot has no active run there. Each
   * call replaces the whole plan; `[]` clears it. Items: at most 30, text 1..200 characters,
   * state `done`, `current`, `waiting` (waits for a member) or `pending`.
   * Runner-backed bots get `conflict`: their runner sets the plan.
   */
  async setPlan(target: Required<Target>, plan: RunPlanItem[]): Promise<BotRun> {
    const checked = RunPlan.safeParse(plan);
    if (!checked.success) throw new ChatError("invalid", checked.error.issues[0]?.message ?? "invalid plan");
    const known = this.currentRuns.get(target.topicId);
    let run: BotRun;
    if (known) {
      try {
        run = await this.http.setRunPlan(known, checked.data);
      } catch (err) {
        // The run ended meanwhile: start a new one with the plan.
        if (!(err instanceof ChatError && (err.code === "conflict" || err.code === "not_found"))) throw err;
        this.currentRuns.delete(target.topicId);
        run = await this.http.startRun({ groupId: target.groupId, topicId: target.topicId, plan: checked.data });
      }
    } else {
      run = await this.http.startRun({ groupId: target.groupId, topicId: target.topicId, plan: checked.data });
    }
    this.noteRun(run);
    return run;
  }

  /** Ends this bot's current run in a topic. Returns null when it has none. */
  async endRun(target: Required<Target>, status: "done" | "failed" | "cancelled", summary?: string): Promise<BotRun | null> {
    const runId = this.currentRuns.get(target.topicId);
    if (!runId) return null;
    const run = await this.http.endRun(runId, summary === undefined ? { status } : { status, summary });
    this.noteRun(run);
    return run;
  }

  /** This bot's active run in a topic, as far as this process has seen. */
  currentRun(topicId: string): string | undefined {
    return this.currentRuns.get(topicId);
  }

  private noteRun(run: BotRun): void {
    if (run.botId !== this.meMember?.id && this.meMember) return;
    if (run.endedAt === undefined) this.currentRuns.set(run.topicId, run.id);
    else if (this.currentRuns.get(run.topicId) === run.id) this.currentRuns.delete(run.topicId);
  }

  /** Follows or unfollows a topic on the server (V1 `topic.follow`) and locally. */
  async setFollow(groupId: string, topicId: string, follow: boolean): Promise<TopicState> {
    const r = (await this.request({ t: "topic.follow", groupId, topicId, follow })) as TopicState;
    if (follow) this.follow(groupId, topicId);
    else this.unfollow(groupId, topicId);
    return r;
  }

  /** Presses a card button (bots can press other members' cards too). */
  async pressCard(groupId: string, messageId: string, actionId: string): Promise<void> {
    await this.request({ t: "card.action", groupId, messageId, actionId });
  }

  /** Adds (`on` true, the default) or removes this bot's reaction; false when nothing changed. */
  async react(groupId: string, messageId: string, emoji: string, on = true): Promise<boolean> {
    const r = (await this.request({ t: "react", groupId, messageId, emoji, on })) as { changed: boolean };
    return r.changed;
  }

  typing(groupId: string, topicId: string): void {
    this.sendRaw({ t: "typing", groupId, topicId });
  }

  /** Messages of a topic, newest first. */
  async history(
    groupId: string,
    topicId: string,
    opts: { beforeSeq?: number; limit?: number } = {},
  ): Promise<{ messages: Message[]; hasMore: boolean }> {
    const frame: Record<string, unknown> & { t: string } = { t: "history", groupId, topicId };
    if (opts.beforeSeq !== undefined) frame.beforeSeq = opts.beforeSeq;
    if (opts.limit !== undefined) frame.limit = opts.limit;
    const r = (await this.request(frame)) as { messages: Message[]; hasMore: boolean };
    return { messages: r.messages, hasMore: r.hasMore };
  }

  /** Uploads a file over HTTP; put the result in `attachments`. */
  uploadFile(input: UploadInput) {
    return this.http.uploadFile(input);
  }

  // ---------------------------------------------------------------------------
  // Connection
  // ---------------------------------------------------------------------------

  private connect(): void {
    if (this.stopped) return;
    let ws: WebSocket;
    try {
      ws = new this.WS(wsUrl(this.url), {
        headers: { authorization: `Bearer ${this.token}`, "user-agent": `chat-bot-sdk/${SDK_VERSION}` },
      });
    } catch (err) {
      this.log.warn("websocket create failed", { err: String(err) });
      this.scheduleReconnect();
      return;
    }
    this.ws = ws;
    let opened = false;
    const openTimer = setTimeout(() => {
      if (!opened && this.ws === ws) this.dropSocket("open timeout");
    }, OPEN_TIMEOUT_MS);

    ws.onopen = () => {
      if (this.ws !== ws) return;
      opened = true;
      clearTimeout(openTimer);
      this.lastFrameAt = Date.now();
      const cursors: Record<string, number> = {};
      for (const [gid, g] of this.groups) cursors[gid] = g.applied;
      for (const [gid, seq] of Object.entries(this.state.cursors)) if (!(gid in cursors)) cursors[gid] = seq;
      this.sendRaw(
        {
          t: "hello",
          protocol: PROTOCOL_VERSION,
          client: this.opts.client ?? { name: "bot-sdk", version: SDK_VERSION },
          cursors,
        },
        true,
      );
      this.startPing(ws);
    };
    ws.onmessage = (ev: MessageEvent) => {
      if (this.ws !== ws) return;
      this.lastFrameAt = Date.now();
      if (typeof ev.data !== "string") return;
      let frame: Record<string, unknown>;
      try {
        frame = JSON.parse(ev.data);
      } catch {
        this.log.warn("server frame is not JSON");
        return;
      }
      try {
        this.onFrame(frame);
      } catch (err) {
        this.log.error("frame handling failed", { err, t: frame.t });
      }
    };
    ws.onclose = (ev: CloseEvent) => {
      clearTimeout(openTimer);
      if (this.ws !== ws) return;
      this.ws = null;
      this.stopPing();
      const wasReady = this.ready;
      this.setReady(false);
      this.log.info("disconnected", { code: ev.code, reason: ev.reason, wasReady });
      if (ev.code === 4001 || ev.code === 4000) {
        this.fail(new ChatError(ev.code === 4001 ? "unauthorized" : "unsupported_protocol", ev.reason || "closed by server"));
        return;
      }
      if (!opened) {
        // A rejected upgrade looks like a network error; ask over HTTP whether the token is bad.
        void this.checkToken().then((bad) => {
          if (bad) this.fail(new ChatError("unauthorized", "the server rejected the bot token"));
          else this.scheduleReconnect();
        });
        return;
      }
      this.scheduleReconnect();
    };
    ws.onerror = () => {
      // onclose follows and handles it.
    };
  }

  private async checkToken(): Promise<boolean> {
    try {
      await this.http.me();
      return false;
    } catch (err) {
      return err instanceof ChatError && err.code === "unauthorized";
    }
  }

  private fail(err: ChatError): void {
    this.log.error("connection failed for good", { code: err.code, message: err.message });
    this.fatal = err;
    this.stopped = true;
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(err);
    }
    this.pending.clear();
    for (const w of this.startWaiters.splice(0)) w.reject(err);
    this.emitError(err, "connection");
    this.saveNow();
  }

  private dropSocket(why: string): void {
    const ws = this.ws;
    if (!ws) return;
    this.log.info("dropping connection", { why });
    this.ws = null;
    this.stopPing();
    this.setReady(false);
    try {
      ws.close(4100, why.slice(0, 100));
    } catch {
      // ignore
    }
    this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    if (this.stopped || this.reconnectTimer) return;
    const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** this.attempt);
    const delay = Math.round(base / 2 + Math.random() * (base / 2));
    this.attempt++;
    this.log.debug("reconnecting", { inMs: delay, attempt: this.attempt });
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private startPing(ws: WebSocket): void {
    this.stopPing();
    this.pingTimer = setInterval(() => {
      if (this.ws !== ws) return;
      if (Date.now() - this.lastFrameAt > SILENCE_MS) {
        this.dropSocket("no frames from server");
        return;
      }
      this.sendRaw({ t: "ping" }, true);
    }, PING_MS);
  }

  private stopPing(): void {
    if (this.pingTimer) clearInterval(this.pingTimer);
    this.pingTimer = null;
  }

  private setReady(up: boolean): void {
    if (this.ready === up) return;
    this.ready = up;
    for (const h of this.connectionHandlers) {
      try {
        h(up);
      } catch (err) {
        this.emitError(err, "onConnection");
      }
    }
  }

  /** Sends a frame now if the socket is open. Returns false when it could not be sent. */
  private sendRaw(frame: Record<string, unknown>, beforeReady = false): boolean {
    const ws = this.ws;
    if (!ws || ws.readyState !== 1 || (!beforeReady && !this.ready)) return false;
    try {
      ws.send(JSON.stringify(frame));
      return true;
    } catch {
      return false;
    }
  }

  /** A frame with a rid: waits for the connection, is resent after reconnects, resolves with the result. */
  private request(frame: Record<string, unknown> & { t: string }): Promise<unknown> {
    if (this.fatal) return Promise.reject(this.fatal);
    if (this.stopped) return Promise.reject(new ChatError("closed", "bot is not started"));
    const rid = `r${++this.ridSeq}`;
    const full = { ...frame, rid };
    const check = ClientFrame.safeParse(full);
    if (!check.success) {
      const issue = check.error.issues[0];
      return Promise.reject(new ChatError("invalid", `${frame.t}: ${issue?.path.join(".") || "(root)"} ${issue?.message ?? ""}`.trim()));
    }
    const timeoutMs = this.opts.requestTimeoutMs ?? 60_000;
    return new Promise((resolve, reject) => {
      const p: Pending = {
        rid,
        frame: full,
        resolve,
        reject,
        deadline: Date.now() + timeoutMs,
        sent: false,
        attempts: 0,
        timer: setTimeout(() => {
          this.pending.delete(rid);
          reject(new ChatError("timeout", `${frame.t} got no answer in ${timeoutMs} ms`));
        }, timeoutMs),
      };
      this.pending.set(rid, p);
      this.trySend(p);
    });
  }

  private trySend(p: Pending): void {
    if (this.sendRaw(p.frame)) {
      p.sent = true;
      p.attempts++;
    }
  }

  private settle(rid: string, ok: boolean, value: unknown): void {
    const p = this.pending.get(rid);
    if (!p) return;
    if (!ok) {
      const e = value as { code: ChatError["code"]; message: string };
      if (e.code === "rate_limited" && Date.now() + 1000 * p.attempts < p.deadline) {
        p.sent = false;
        setTimeout(() => {
          if (this.pending.get(rid) === p) this.trySend(p);
        }, 1000 * p.attempts);
        return;
      }
      this.pending.delete(rid);
      clearTimeout(p.timer);
      p.reject(new ChatError(e.code, e.message));
      return;
    }
    this.pending.delete(rid);
    clearTimeout(p.timer);
    p.resolve(value);
  }

  // ---------------------------------------------------------------------------
  // Incoming frames
  // ---------------------------------------------------------------------------

  private onFrame(f: Record<string, unknown>): void {
    switch (f.t) {
      case "welcome":
        return this.onWelcome(f as { me: Member; members: Member[]; groups: GroupSummary[] });
      case "event":
        return this.onEventFrame(f.event as RawEvent, false);
      case "transient": {
        const e = f.event as TransientEvent;
        for (const h of this.transientHandlers) {
          try {
            h(e);
          } catch (err) {
            this.emitError(err, "onTransient");
          }
        }
        return;
      }
      case "ack":
        if (typeof f.rid === "string") {
          if (f.ok === true) this.settle(f.rid, true, f.result);
          else this.settle(f.rid, false, f.error);
        }
        return;
      case "page":
      case "history":
        if (typeof f.rid === "string") this.settle(f.rid, true, f);
        return;
      case "directory":
        return this.onDirectory(f as { members?: Member[]; groups?: GroupSummary[]; removedGroupIds?: string[] });
      case "reads":
        return this.onReads(f as { groupId: string; topics: TopicState[] });
      case "error": {
        const e = f.error as { code: string; message: string } | undefined;
        this.log.warn("server error frame", { code: e?.code, message: e?.message });
        return;
      }
      default:
        return; // pong and frames this SDK does not know
    }
  }

  private onWelcome(f: { me: Member; members: Member[]; groups: GroupSummary[] }): void {
    if (this.state.memberId && this.state.memberId !== f.me.id) {
      this.fail(new ChatError("invalid", "the state file belongs to another member; use one state file per bot"));
      return;
    }
    if (!this.state.memberId) {
      this.state.memberId = f.me.id;
      this.scheduleSave();
    }
    this.meMember = f.me;
    this.memberMap.clear();
    for (const m of f.members) this.memberMap.set(m.id, m);
    this.summaryMap.clear();
    const firstWelcome = !this.everConnected;
    this.everConnected = true;
    this.attempt = 0;
    this.setReady(true);
    this.log.info("connected", { memberId: f.me.id, groups: f.groups.length });
    // Resend unanswered requests first; catch-up below sends its own new requests.
    for (const p of this.pending.values()) this.trySend(p);
    for (const g of this.groups.values()) g.member = false;
    for (const s of f.groups) this.addGroup(s, firstWelcome && this.firstStart);
    for (const w of this.streams) void w.resync().catch((err) => this.log.warn("stream resync failed", { err: String(err) }));
    for (const w of this.startWaiters.splice(0)) w.resolve();
  }

  private onDirectory(f: { members?: Member[]; groups?: GroupSummary[]; removedGroupIds?: string[] }): void {
    for (const m of f.members ?? []) {
      this.memberMap.set(m.id, m);
      if (this.meMember && m.id === this.meMember.id) this.meMember = m;
    }
    for (const s of f.groups ?? []) this.addGroup(s, false);
    for (const gid of f.removedGroupIds ?? []) {
      this.summaryMap.delete(gid);
      const g = this.groups.get(gid);
      if (g) {
        g.member = false;
        g.buffer.clear();
      }
    }
  }

  private onReads(f: { groupId: string; topics: TopicState[] }): void {
    for (const t of f.topics ?? []) {
      if (t.following === true) this.follow(f.groupId, t.topicId);
      else if (t.following === false) this.unfollow(f.groupId, t.topicId);
    }
  }

  private addGroup(s: GroupSummary, skipHistory: boolean): void {
    const gid = s.group.id;
    this.summaryMap.set(gid, s);
    for (const t of s.topics ?? []) if (t.following) this.follow(gid, t.topicId);
    let g = this.groups.get(gid);
    if (!g) {
      const saved = this.state.cursors[gid];
      const start = saved ?? (skipHistory && this.opts.firstStart !== "all" ? s.lastSeq : 0);
      g = { applied: start, buffer: new Map(), fetching: false, running: new Set(), member: true, readTimer: null, lastReadSent: s.readSeq };
      this.groups.set(gid, g);
      if (saved === undefined) {
        this.state.cursors[gid] = start;
        this.scheduleSave();
      }
    }
    g.member = true;
    if (s.lastSeq > g.applied) void this.catchUp(gid);
  }

  private onEventFrame(raw: RawEvent, catchUp: boolean): void {
    if (!raw || typeof raw.seq !== "number" || typeof raw.groupId !== "string") return;
    let g = this.groups.get(raw.groupId);
    if (!g) {
      // An event of a group not in the directory yet: start from its beginning.
      g = { applied: this.state.cursors[raw.groupId] ?? 0, buffer: new Map(), fetching: false, running: new Set(), member: true, readTimer: null, lastReadSent: 0 };
      this.groups.set(raw.groupId, g);
    }
    if (raw.seq <= g.applied) return;
    if (raw.seq > g.applied + 1) {
      g.buffer.set(raw.seq, raw);
      void this.catchUp(raw.groupId);
      return;
    }
    this.apply(g, raw, catchUp);
    for (let next = g.buffer.get(g.applied + 1); next; next = g.buffer.get(g.applied + 1)) {
      g.buffer.delete(next.seq);
      this.apply(g, next, catchUp);
    }
    for (const seq of g.buffer.keys()) if (seq <= g.applied) g.buffer.delete(seq);
  }

  /** Fetches pages until the group has no gap. One loop per group at a time. */
  private async catchUp(gid: string): Promise<void> {
    const g = this.groups.get(gid);
    if (!g || g.fetching) return;
    g.fetching = true;
    try {
      for (;;) {
        if (this.stopped || !g.member) return;
        const before = g.applied;
        const page = (await this.request({ t: "fetch", groupId: gid, afterSeq: g.applied, limit: FETCH_LIMIT })) as {
          events: RawEvent[];
          hasMore: boolean;
          lastSeq: number;
        };
        for (const e of page.events) this.onEventFrame({ ...e, groupId: gid }, true);
        const summary = this.summaryMap.get(gid);
        if (summary && page.lastSeq > summary.lastSeq) this.summaryMap.set(gid, { ...summary, lastSeq: page.lastSeq });
        const gap = g.buffer.size > 0;
        if (!page.hasMore && !gap) return;
        if (g.applied === before) {
          // No progress: the server has no event after `applied`, so buffered later seqs cannot
          // be filled. Skip to the oldest buffered event rather than loop.
          if (!gap) return;
          const oldest = Math.min(...g.buffer.keys());
          this.log.warn("event log gap could not be filled", { groupId: gid, from: g.applied + 1, to: oldest - 1 });
          g.applied = oldest - 1;
          this.onEventFrame(g.buffer.get(oldest) as RawEvent, true);
        }
      }
    } catch (err) {
      if (err instanceof ChatError && (err.code === "forbidden" || err.code === "not_found")) {
        g.member = false;
        return;
      }
      if (err instanceof ChatError && err.code === "closed") return;
      this.log.warn("catch-up failed; retrying", { groupId: gid, err: String(err) });
      setTimeout(() => void this.catchUp(gid), 2000);
    } finally {
      g.fetching = false;
    }
  }

  // ---------------------------------------------------------------------------
  // Applying events and running handlers
  // ---------------------------------------------------------------------------

  private apply(g: GroupState, raw: RawEvent, catchUp: boolean): void {
    g.applied = raw.seq;
    const parsed = EventSchema.safeParse(raw);
    const event = parsed.success ? parsed.data : null;
    if (!parsed.success) this.log.debug("unknown or invalid event; skipped", { type: raw.type, seq: raw.seq });
    const me = this.meMember?.id;
    const jobs: (() => unknown)[] = [];

    for (const h of this.eventHandlers) jobs.push(() => h({ bot: this, event, raw, catchUp }));

    if (event && me) {
      switch (event.type) {
        case "topic.created": {
          const t = event.data.topic;
          this.topics.set(t.id, { waitingOn: t.waitingOn ?? [], ownerId: t.ownerId ?? null });
          if (t.contextOf) {
            this.noteAside(event.groupId, t.id, t.contextOf);
            this.follow(event.groupId, t.id);
          }
          if (t.createdBy === me || t.ownerId === me || t.waitingOn?.includes(me)) this.follow(event.groupId, t.id);
          break;
        }
        case "topic.updated": {
          const d = event.data;
          const info = this.topics.get(d.topicId) ?? { waitingOn: [], ownerId: null };
          const wasWaiting = info.waitingOn.includes(me);
          if (d.waitingOn) info.waitingOn = d.waitingOn;
          if (d.ownerId !== undefined) info.ownerId = d.ownerId;
          this.topics.set(d.topicId, info);
          if (d.contextOf) this.noteAside(event.groupId, d.topicId, d.contextOf);
          if ((d.waitingOn?.includes(me) && !wasWaiting) || d.ownerId === me) this.follow(event.groupId, d.topicId);
          break;
        }
        case "message.created": {
          const m = event.data.message;
          if (m.authorId === me) {
            this.noteOwnMessage(m);
            this.follow(event.groupId, m.topicId);
            break;
          }
          if (m.kind === "system" || m.deletedAt) break;
          // No loops: messages of other bots reach no handler unless allowBotAuthors is set.
          if (this.opts.allowBotAuthors !== true && this.memberMap.get(m.authorId)?.kind === "bot") {
            if (m.mentions.includes(me)) this.follow(event.groupId, m.topicId);
            this.scheduleRead(event.groupId);
            break;
          }
          const ctx = () => this.messageContext(event, catchUp);
          const aside = this.state.asides?.[m.topicId];
          if (aside && aside.dmGroupId === event.groupId) {
            // A side chat message is addressed to me, @ or not.
            this.follow(event.groupId, m.topicId);
            if (this.asideHandlers.length > 0) for (const h of this.asideHandlers) jobs.push(() => h(this.asideContext(event, aside, catchUp)));
            else for (const h of this.mentionHandlers) jobs.push(() => h(ctx()));
          } else if (m.mentions.includes(me)) {
            this.follow(event.groupId, m.topicId);
            for (const h of this.mentionHandlers) jobs.push(() => h(ctx()));
          } else if (this.isFollowing(event.groupId, m.topicId)) {
            for (const h of this.messageHandlers) jobs.push(() => h(ctx()));
          }
          this.scheduleRead(event.groupId);
          break;
        }
        case "run.updated":
          if (event.data.run.botId === me) this.noteRun(event.data.run);
          break;
        case "message.updated": {
          const d = event.data;
          if (event.actorId === me && d.cardV2) this.cacheCard(d.messageId, d.cardV2);
          break;
        }
        case "card.answered": {
          const ref = this.state.cards[event.data.messageId];
          if (!ref || event.actorId === me) break;
          for (const h of this.answerHandlers) jobs.push(() => h(this.answerContext(event, ref.topicId, catchUp)));
          break;
        }
        case "artifact.created":
        case "artifact.versionAdded": {
          const { artifact, version } = event.data;
          if (version.createdBy === me || artifact.createdBy === me) this.noteArtifact(artifact.id, { groupId: artifact.groupId, topicId: artifact.topicId });
          break;
        }
        case "artifact.evented": {
          const ref = this.state.artifacts?.[event.data.artifactId];
          if (!ref || event.actorId === me) break;
          for (const h of this.artifactEventHandlers) jobs.push(() => h(this.artifactEventContext(event, ref, catchUp)));
          break;
        }
        case "card.actioned": {
          const ref = this.state.cards[event.data.messageId];
          if (!ref || event.actorId === me) break;
          for (const h of this.cardHandlers) jobs.push(() => h(this.cardContext(event, ref.topicId, catchUp)));
          break;
        }
        default:
          break;
      }
    }

    if (jobs.length === 0) {
      this.commitCursor(g, raw.groupId);
      return;
    }
    g.running.add(raw.seq);
    const run = (async () => {
      for (const job of jobs) {
        try {
          await job();
        } catch (err) {
          this.emitError(err, `handler for ${raw.type} seq ${raw.seq} in ${raw.groupId}`);
        }
      }
    })();
    this.running.add(run);
    void run.finally(() => {
      this.running.delete(run);
      // Cut off by stop(): keep the cursor below this event so the next start runs it again.
      if (this.stopped) return;
      g.running.delete(raw.seq);
      this.commitCursor(g, raw.groupId);
    });
  }

  /** Saved cursor = highest seq with every handler up to it finished. */
  private commitCursor(g: GroupState, gid: string): void {
    let cursor = g.applied;
    for (const seq of g.running) if (seq - 1 < cursor) cursor = seq - 1;
    if ((this.state.cursors[gid] ?? -1) < cursor) {
      this.state.cursors[gid] = cursor;
      this.scheduleSave();
    }
  }

  private messageContext(event: EventOf<"message.created">, catchUp: boolean): MessageContext {
    const m = event.data.message;
    let n = 0;
    // Stable ids: a handler that runs again after a crash posts the same clientMsgIds,
    // and the server answers with the first message instead of a new one.
    const nextId = () => `bot:${m.id}:${n++}`;
    const target = { groupId: m.groupId, topicId: m.topicId };
    const withId = (input: string | SendInput): SendInput => {
      const i = typeof input === "string" ? { body: input } : { ...input };
      if (!i.clientMsgId) i.clientMsgId = nextId();
      return i;
    };
    return {
      bot: this,
      event,
      message: m,
      groupId: m.groupId,
      topicId: m.topicId,
      catchUp,
      reply: (input) => this.send(target, withId(input)),
      stream: (input = {}) => this.stream(target, withId(input) as SendInput & StreamOptions),
      updateTopic: (change) => this.updateTopic(m.groupId, m.topicId, change),
      typing: () => this.typing(m.groupId, m.topicId),
      setPlan: (plan) => this.setPlan({ groupId: m.groupId, topicId: m.topicId }, plan),
      endRun: (status, summary) => this.endRun({ groupId: m.groupId, topicId: m.topicId }, status, summary),
      ack: (emoji) => this.ack(m.groupId, m.id, emoji),
      ask: (card, input = {}) => this.postCardV2(target, card, withId(input)),
      done: (card) => this.done(target, card, withId({})),
      setAcceptance: (items) => this.setAcceptance(target, items),
      aside: this.asideOf(m.groupId, m.topicId),
    };
  }

  private asideOf(groupId: string, topicId: string): TopicContext | undefined {
    const a = this.state.asides?.[topicId];
    return a && a.dmGroupId === groupId ? { groupId: a.groupId, topicId: a.topicId, uptoSeq: a.uptoSeq } : undefined;
  }

  private asideContext(event: EventOf<"message.created">, ref: AsideRef, catchUp: boolean): AsideContext {
    const base = this.messageContext(event, catchUp);
    const aside = { groupId: ref.groupId, topicId: ref.topicId, uptoSeq: ref.uptoSeq };
    return {
      ...base,
      aside,
      source: (opts = {}) => this.sourceMessages(aside, opts.limit ?? 200),
    };
  }

  /** Messages of a side chat's topic up to `uptoSeq`, oldest first, at most `limit` newest. */
  private async sourceMessages(c: TopicContext, limit: number): Promise<Message[]> {
    const out: Message[] = [];
    let before = c.uptoSeq + 1;
    while (out.length < limit) {
      const page = await this.history(c.groupId, c.topicId, { beforeSeq: before, limit: Math.min(100, limit - out.length) });
      for (const m of page.messages) if (m.createdSeq <= c.uptoSeq && !m.deletedAt) out.push(m);
      if (!page.hasMore || page.messages.length === 0) break;
      before = Math.min(...page.messages.map((m) => m.createdSeq));
    }
    return out.slice(0, limit).reverse();
  }

  private answerContext(event: EventOf<"card.answered">, topicId: string, catchUp: boolean): CardAnswerContext {
    const { messageId, rev, actionId } = event.data;
    const inputs = (event.data.inputs ?? {}) as Record<string, AnswerValue>;
    const editedValues = event.data.editedValues ?? {};
    const card = this.cardCache.get(messageId);
    const action = card?.actions?.find((a) => a.id === actionId);
    const cancelled = action ? action.kind === "cancel" : actionId === "cancel" || actionId === "deny";
    const block = (id: string) => findBlock(card, id);
    const choices = (id = "choice"): string[] => {
      const v = inputs[id];
      if (Array.isArray(v)) return v.filter((x): x is string => typeof x === "string");
      return typeof v === "string" ? [v] : [];
    };
    let n = 0;
    const nextId = () => `bot:${event.groupId}:${event.seq}:${n++}`;
    return {
      bot: this,
      event,
      groupId: event.groupId,
      topicId,
      messageId,
      rev,
      actionId,
      inputs,
      editedValues,
      actorId: event.actorId,
      catchUp,
      card,
      action,
      cancelled,
      choices,
      choice: (id) => choices(id)[0],
      text: (id) => {
        const v = inputs[id];
        return typeof v === "string" ? v : typeof v === "number" || typeof v === "boolean" ? String(v) : undefined;
      },
      number: (id) => {
        const v = inputs[id];
        if (typeof v === "number") return Number.isFinite(v) ? v : undefined;
        if (typeof v === "string" && v.trim() !== "" && Number.isFinite(Number(v))) return Number(v);
        return undefined;
      },
      bool: (id) => (typeof inputs[id] === "boolean" ? (inputs[id] as boolean) : undefined),
      draft: (id = "draft") => {
        if (typeof editedValues[id] === "string") return editedValues[id];
        const b = block(id);
        return b?.type === "draft" ? b.text : undefined;
      },
      resolve: async (change = {}, base) => {
        const from = base ?? this.cardCache.get(messageId);
        if (!from) throw new ChatError("invalid", "the answered card is not known; pass it as base");
        return this.updateCardV2(event.groupId, messageId, answeredCard(from, change));
      },
      reply: (input) => {
        const i = typeof input === "string" ? { body: input } : { ...input };
        if (!i.clientMsgId) i.clientMsgId = nextId();
        return this.send({ groupId: event.groupId, topicId }, i);
      },
      updateTopic: (change) => this.updateTopic(event.groupId, topicId, change),
    };
  }

  private artifactEventContext(event: EventOf<"artifact.evented">, ref: CardRef, catchUp: boolean): ArtifactEventContext {
    const { artifactId, version, name, payload } = event.data;
    let n = 0;
    const nextId = () => `bot:${event.groupId}:${event.seq}:${n++}`;
    return {
      bot: this,
      event,
      groupId: event.groupId,
      topicId: ref.topicId,
      artifactId,
      version,
      name,
      payload,
      actorId: event.actorId,
      catchUp,
      reply: (input) => {
        const i = typeof input === "string" ? { body: input } : { ...input };
        if (!i.clientMsgId) i.clientMsgId = nextId();
        return this.send({ groupId: event.groupId, topicId: ref.topicId }, i);
      },
    };
  }

  private cardContext(event: EventOf<"card.actioned">, topicId: string, catchUp: boolean): CardActionContext {
    const { messageId, actionId, value } = event.data;
    let n = 0;
    const nextId = () => `bot:${event.groupId}:${event.seq}:${n++}`;
    return {
      bot: this,
      event,
      groupId: event.groupId,
      topicId,
      messageId,
      actionId,
      value,
      actorId: event.actorId,
      catchUp,
      updateCard: (card, body) => this.updateCard(event.groupId, messageId, card, body),
      reply: (input) => {
        const i = typeof input === "string" ? { body: input } : { ...input };
        if (!i.clientMsgId) i.clientMsgId = nextId();
        return this.send({ groupId: event.groupId, topicId }, i);
      },
      updateTopic: (change) => this.updateTopic(event.groupId, topicId, change),
    };
  }

  private noteOwnMessage(m: Message): void {
    if (m.cardV2) this.cacheCard(m.id, m.cardV2);
    if (m.kind !== "card" && !m.card) return;
    if (this.state.cards[m.id]) return;
    this.state.cards[m.id] = { groupId: m.groupId, topicId: m.topicId };
    const ids = Object.keys(this.state.cards);
    if (ids.length > MAX_REMEMBERED_CARDS) {
      // Message ids are ULIDs: lexical order is time order.
      ids.sort();
      for (const id of ids.slice(0, ids.length - MAX_REMEMBERED_CARDS)) delete this.state.cards[id];
    }
    this.scheduleSave();
  }

  private cacheCard(messageId: string, card: CardV2): void {
    const old = this.cardCache.get(messageId);
    if (old && old.rev > card.rev) return;
    this.cardCache.delete(messageId);
    this.cardCache.set(messageId, card);
    if (this.cardCache.size > MAX_CACHED_CARDS) {
      const first = this.cardCache.keys().next().value;
      if (first !== undefined) this.cardCache.delete(first);
    }
  }

  private noteArtifact(artifactId: string, ref: CardRef): void {
    const map = (this.state.artifacts ??= {});
    if (map[artifactId]) return;
    map[artifactId] = ref;
    trimOldest(map, MAX_REMEMBERED_REFS);
    this.scheduleSave();
  }

  private noteAside(dmGroupId: string, topicId: string, c: TopicContext): void {
    const map = (this.state.asides ??= {});
    const cur = map[topicId];
    if (cur && cur.dmGroupId === dmGroupId && cur.uptoSeq >= c.uptoSeq && cur.topicId === c.topicId) return;
    map[topicId] = { dmGroupId, groupId: c.groupId, topicId: c.topicId, uptoSeq: c.uptoSeq };
    trimOldest(map, MAX_REMEMBERED_REFS);
    this.scheduleSave();
  }

  private follow(groupId: string, topicId: string): void {
    const list = (this.state.following[groupId] ??= []);
    if (list.includes(topicId)) return;
    list.push(topicId);
    this.scheduleSave();
  }

  private unfollow(groupId: string, topicId: string): void {
    const list = this.state.following[groupId];
    if (!list) return;
    const i = list.indexOf(topicId);
    if (i < 0) return;
    list.splice(i, 1);
    this.scheduleSave();
  }

  private scheduleRead(groupId: string): void {
    if (this.opts.markRead === false) return;
    const g = this.groups.get(groupId);
    if (!g || g.readTimer) return;
    g.readTimer = setTimeout(() => {
      g.readTimer = null;
      const seq = this.state.cursors[groupId] ?? 0;
      if (seq > g.lastReadSent && this.sendRaw({ t: "read", groupId, seq })) g.lastReadSent = seq;
    }, READ_DEBOUNCE_MS);
  }

  private emitError(err: unknown, where: string): void {
    if (this.errorHandlers.length === 0) {
      this.log.error("bot error", { where, err: err instanceof Error ? err : String(err) });
      return;
    }
    for (const h of this.errorHandlers) {
      try {
        h(err, where);
      } catch {
        // ignore
      }
    }
  }

  // ---------------------------------------------------------------------------
  // State and streams
  // ---------------------------------------------------------------------------

  private scheduleSave(): void {
    if (this.saveTimer) return;
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      this.saveNow();
    }, SAVE_DEBOUNCE_MS);
  }

  private saveNow(): void {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    try {
      this.store.save(this.state);
    } catch (err) {
      this.log.error("saving bot state failed", { err: String(err) });
    }
  }

  private readonly streamHost = {
    sendDelta: (groupId: string, messageId: string, append: string) => this.sendRaw({ t: "delta", groupId, messageId, append }),
    edit: (groupId: string, messageId: string, e: EditInput) => this.edit(groupId, messageId, e),
    streamEnded: (w: StreamWriter) => {
      this.streams.delete(w);
    },
  };
}
