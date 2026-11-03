// Bot: one WebSocket connection with reconnect, ordered event delivery per group, catch-up
// after restarts from a cursor file, and handlers for mentions, followed topics, card presses
// and (1.1) the interaction loop: live card events, answers, page events, patches. Writes wait
// for the connection and are resent after a reconnect; `send` is idempotent by clientMsgId, so
// a resend never posts twice.
import { readFileSync, statSync } from "node:fs";
import {
  AcceptanceItem,
  type ArtifactRef,
  type ArtifactVersion,
  type BlockRef,
  type BlockRefInput,
  type BotRun,
  type Card,
  type CardV2,
  type CardPatchAck,
  type CardState,
  type CardV2Action,
  CARD_CATALOGS,
  CLIENT_FEATURES,
  ClientFrame,
  Event as EventSchema,
  type Event,
  type EventOf,
  type FileRef,
  type GroupSummary,
  type Member,
  type Message,
  type PartRef,
  type PatchOp,
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
import { answeredCard, checkCardV2, type DoneCardOptions, doneCard, failedV1Card } from "./cards.ts";
import { ChatError } from "./errors.ts";
import { HttpClient, normalizeBaseUrl, type UploadInput } from "./http.ts";
import { opsFit, opsOf, type PatchInput } from "./interact.ts";
import { findBlock as findRefBlock, findPart, stripRefPrefix } from "./refs.ts";
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
  /** 2.1: the event (`eventId`) this message answers; the emitter's waiting state clears. */
  causeEventId?: string;
  /** 1.1: refs to parts (引用提问); the server writes their quote lines in front of the body. */
  blockRefs?: BlockRefInput[];
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
  /** 2.1, with `cardV2`: the rev the edit is based on (`conflict`, reason `card.stale_rev`). */
  baseRev?: number;
  /** 2.1: the event this edit answers. */
  causeEventId?: string;
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
  /** 1.1 (引用提问): the parts the message asks about; [] when none. Labels and values are the sender's data. */
  refs: BlockRef[];
  /** The body without the quote lines the server wrote for `refs` (equals `body` without refs). */
  question: string;
  /** The source message or 产出 of a ref, and the block and part it names (see `Bot.resolveRef`). */
  resolveRef(ref: BlockRef): Promise<ResolvedRef>;
}

/** What a ref points at now. `current` is false when the source changed after the send or is gone. */
export interface ResolvedRef {
  message?: Message;
  artifact?: ArtifactVersion;
  /** The block: a Card v2 or v1 block, the whole card (`card`), or a Markdown block `{kind, text, lang?}`. */
  block?: unknown;
  /** The part the ref's `part` names in the block (the block for an unknown kind). */
  part?: unknown;
  current: boolean;
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
 * An interaction with one of my v2 cards (2.1): a live event (transient `card.event`, only
 * while connected) or a stored answer (`card.answered`, also on catch-up). `value` and
 * `inputs` come from the client: check them before you act on them.
 */
export interface CardEventContext {
  bot: Bot;
  groupId: string;
  topicId: string;
  messageId: string;
  /** The event id; `patch`, `reply` and `postCard` name it as their cause. "" from a 1.0 server. */
  eventId: string;
  /** The rev the person saw. */
  rev: number;
  /** The event name, for example `choice.change`, `chart.select`, `action.press`. */
  name: string;
  blockId?: string;
  actionId?: string;
  value: unknown;
  /** Input values by block id (submit and reject actions). */
  inputs: Record<string, unknown>;
  /** The part the event targets (`chart.select`: point or bar; `metric.press`: metric). */
  part?: PartRef;
  /** Who acted (always a person). */
  actorId: string;
  /** True for a stored answer. */
  stored: boolean;
  answerId?: string;
  askId?: string;
  catchUp: boolean;
  /** The newest rev of the card that the SDK knows. */
  card: CardV2;
  /** Server-owned state of the card (ask rounds, answers, tallies), when known. */
  cardState?: CardState;
  /** False when a newer event of the same block (or action) arrived: latest wins. */
  isLatest(): boolean;
  /**
   * Changes the card in place: ops, or a function that records them on a draft of the newest
   * rev. `baseRev` and `cause` are set for you. On `card.stale_rev` it rebases on the newest rev
   * (at most 3 tries) when every op still targets an existing id, else it throws `conflict`.
   * On `rate_limited` it queues and merges ops for the same message (at most 1 patch per second).
   */
  patch(ops: PatchInput): Promise<Message>;
  /** Posts a message in the card's topic as the answer to this event. */
  reply(input: string | SendInput): Promise<SendResult>;
  /** Posts a new card in the card's topic as the answer to this event. */
  postCard(card: CardV2, input?: Omit<SendInput, "card" | "cardV2">): Promise<SendResult>;
  /**
   * Answers the event without a change. A live-event handler that returns (or throws) without
   * `patch`, `reply`, `postCard` or `resolve` makes the SDK send `ignored` (or `failed`) itself.
   */
  resolve(result: "ignored" | "failed", note?: string): Promise<void>;
  updateTopic(change: TopicChange): Promise<Topic>;
}

/**
 * An answer to one of my v2 cards (`card.answered`, stored, also on catch-up), with readers of
 * the answer. `I` types `inputs` by block id, for example `onCardAnswer<{ choice: string }>`;
 * the values come from the client, so check them before you act on them.
 */
export interface CardAnswerContext<I extends Record<string, unknown> = Record<string, AnswerValue>> extends Omit<CardEventContext, "inputs"> {
  event: EventOf<"card.answered">;
  /** The answered action ("" for the commit of an `immediate` block, which names `blockId`). */
  actionId: string;
  /** Values of input blocks, by block id. 2.1 clients send `{value}`, `{values}`, `{value, other}`, `{order}` or `{rows}`. */
  inputs: Partial<I>;
  /** Edited text of draft blocks, by block id. */
  editedValues: Record<string, string>;
  /** The answered action of the card. */
  action?: CardV2Action;
  /** The answer is a cancel or a reject (or the ids "cancel" and "deny" when the action is not in the card). */
  cancelled: boolean;
  /** Chosen option ids of a choice block (default id "choice"); [] when none. */
  choices(blockId?: string): string[];
  /** The first chosen option id of a choice block (default id "choice"). */
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
   * Edits the card to its answered form with a full replace: a higher rev and no submit or
   * cancel actions, plus `change`.
   */
  markAnswered(change?: Partial<Omit<CardV2, "v" | "rev">>, base?: CardV2): Promise<Message>;
  /** Replaces the card with a newer rev (for example without its actions). */
  updateCard(card: CardV2, body?: string): Promise<Message>;
}

/**
 * A page event from one of my 产出 (transient `artifact.event`, only while connected). Never
 * treat it as an approval. `payload` is untrusted data from the page.
 */
export interface ArtifactEventContext {
  bot: Bot;
  event: Extract<TransientEvent, { type: "artifact.event" }>;
  groupId: string;
  /** The topic of the 产出. */
  topicId: string;
  /** The event id (page events are not stored). */
  eventId: string;
  /** `emit`, or `ask` (the page waits for a reply). */
  kind: "emit" | "ask";
  artifactId: string;
  version: number;
  name: string;
  payload: unknown;
  /** Who used the page. */
  actorId: string;
  catchUp: boolean;
  /** Answers an `ask` (once; private to the asker). */
  reply(payload: unknown): Promise<void>;
  /** Declines an `ask` (once); the page's promise rejects with `declined`. */
  decline(message: string): Promise<void>;
  /** Replaces the version's data snapshot; every viewer's page gets it. `cause` is this event. */
  pushData(data: unknown, opts?: { baseDataRev?: number }): Promise<{ dataRev: number }>;
  /** Posts a message in the 产出's topic as the answer to this event. */
  postMessage(input: string | SendInput): Promise<SendResult>;
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
  /** Do not resend on `rate_limited` (patches queue and merge instead). */
  noRateRetry?: boolean;
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

/** One `patch` call waiting in a message's patch lane. */
interface PatchPart {
  input: PatchInput;
  cause?: string;
  clientPatchId?: string;
  /** Set by `patchCard({ baseRev })`: no rebase. */
  baseRev?: number;
  tries: number;
  /** Set after a merged patch failed: this part goes in a frame of its own. */
  alone?: boolean;
  resolve: (m: Message) => void;
  reject: (e: unknown) => void;
}

/** Patches of one message go one at a time; after `rate_limited` they wait and merge. */
interface PatchLane {
  groupId: string;
  parts: PatchPart[];
  running: boolean;
  notBefore: number;
  timer: ReturnType<typeof setTimeout> | null;
}

const PING_MS = 30_000;
const SILENCE_MS = 75_000;
const OPEN_TIMEOUT_MS = 15_000;
const BACKOFF_MIN_MS = 500;
const BACKOFF_MAX_MS = 30_000;
const FETCH_LIMIT = 500;
const SAVE_DEBOUNCE_MS = 500;
const READ_DEBOUNCE_MS = 1500;

/** v2 cards kept in memory for answers and patches (`CardEventContext.card`). */
const MAX_CACHED_CARDS = 500;
/** Messages kept in memory for `resolveRef`. */
const MAX_CACHED_MESSAGES = 1000;
/** History pages `findMessage` reads at most (100 messages each). */
const MAX_HISTORY_PAGES = 20;
/** Tries of a patch (the first and the rebases after `card.stale_rev`). */
const MAX_PATCH_TRIES = 3;
/** Ops of one `card.patch` frame. */
const MAX_PATCH_OPS = 50;
/** Wait between patches of one message after `rate_limited`. */
const PATCH_INTERVAL_MS = 1000;
/** How long a rebase waits for the newer rev to arrive before it reads the history. */
const REBASE_WAIT_MS = 1500;
/** The server takes a cause only from the last 30 minutes (INTERACT 4.1, `interact.bad_cause`). */
const CAUSE_WINDOW_MS = 30 * 60_000;
/** Older causes are left out a little early: clock skew and the time a write waits in a lane. */
const CAUSE_MARGIN_MS = 2 * 60_000;

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
    if (b.type === "group") for (const c of b.blocks ?? []) if (c.id === id) return c as CardV2["blocks"][number];
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
  /** Server time minus local time (`welcome.serverTime`). */
  private clockSkew = 0;
  private readonly memberMap = new Map<string, Member>();
  private readonly summaryMap = new Map<string, GroupSummary>();

  private readonly mentionHandlers: Handler<MessageContext>[] = [];
  private readonly messageHandlers: Handler<MessageContext>[] = [];
  private readonly cardHandlers: Handler<CardActionContext>[] = [];
  private readonly answerHandlers: Handler<CardAnswerContext>[] = [];
  private readonly artifactEventHandlers: Handler<ArtifactEventContext>[] = [];
  private readonly cardEventHandlers: Handler<CardEventContext>[] = [];
  private readonly asideHandlers: Handler<AsideContext>[] = [];
  /** v2 cards of my messages that this process saw, newest last (at most MAX_CACHED_CARDS). */
  private readonly cardCache = new Map<string, CardV2>();
  /** `cardState` of my card messages, by message id. */
  private readonly cardStates = new Map<string, CardState>();
  /** Waiters for a newer rev of a card (rebase). */
  private readonly revWaiters = new Map<string, (() => void)[]>();
  /** Messages this process saw (for `resolveRef`), newest last. */
  private readonly messageCache = new Map<string, Message>();
  /** Group of each topic this process saw. */
  private readonly topicGroups = new Map<string, string>();
  private readonly patchLanes = new Map<string, PatchLane>();
  /** Latest-wins: the newest event number per message and block (or action). */
  private readonly latestEvents = new Map<string, number>();
  private eventSeq = 0;
  /** Live card events run one at a time per message, in arrival order. */
  private readonly liveChains = new Map<string, Promise<unknown>>();
  private serverFeatureList: string[] = [];
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
  /**
   * A page event from one of my 产出 (1.1): only names the version declared in `events`. A
   * handler that does not answer makes the SDK answer for it: an `emit` gets `ignored`
   * (`failed` when it throws), an `ask` gets a decline.
   */
  onArtifactEvent(h: Handler<ArtifactEventContext>): this {
    this.artifactEventHandlers.push(h);
    return this;
  }
  /**
   * A live event of one of my v2 cards (2.1, transient `card.event`): a pick, a chart point, a
   * row selection, an inline button. It reaches only this handler (not `onEvent`) and only while
   * connected. Respond with `ctx.patch`, `ctx.reply`, `ctx.postCard` or `ctx.resolve`; without
   * one the SDK sends `ignored` (or `failed` when the handler throws).
   */
  onCardEvent(h: Handler<CardEventContext>): this {
    this.cardEventHandlers.push(h);
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
  /** What the server supports (`welcome.features`, 1.1), for example "interact/1", "refs/1". */
  get serverFeatures(): string[] {
    return [...this.serverFeatureList];
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
    if (i.causeEventId) frame.causeEventId = i.causeEventId;
    if (i.blockRefs?.length) frame.blockRefs = i.blockRefs;
    if (i.streaming) frame.streaming = true;
    const result = (await this.request(frame)) as SendResult;
    this.noteOwnMessage(result.message);
    this.noteMessage(result.message);
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
    const checked = checkCardV2(card);
    const v1 = failedV1Card(checked);
    return this.send(target, { ...input, cardV2: checked, ...(v1 ? { card: v1 } : {}), body: input.body ?? "" });
  }

  /**
   * Replaces one of my v2 cards. `card.rev` must be higher than the stored rev. `baseRev`
   * (2.1) makes the server check the rev the edit is based on; `causeEventId` names the event
   * it answers. For small changes prefer `patchCard`.
   */
  async updateCardV2(
    groupId: string,
    messageId: string,
    card: CardV2,
    body?: string,
    opts: { baseRev?: number; causeEventId?: string } = {},
  ): Promise<Message> {
    const checked = checkCardV2(card);
    const v1 = failedV1Card(checked);
    const r = await this.edit(groupId, messageId, {
      body: body ?? (checked.fallback || checked.title),
      cardV2: checked,
      ...(v1 ? { card: v1 } : {}),
      ...opts,
    });
    this.cacheCard(messageId, r.message.cardV2 ?? checked, r.message.cardState);
    this.noteMessage(r.message);
    return r.message;
  }

  /** The v2 card of one of my messages, when this process saw it. */
  knownCard(messageId: string): CardV2 | undefined {
    return this.cardCache.get(messageId);
  }

  /**
   * Changes one of my v2 cards in place (2.1, `card.patch`): ops, or a function that records
   * them on a draft of the newest rev. Without `baseRev` the SDK uses the newest rev it knows
   * and rebases on `card.stale_rev` (at most 3 tries) when every op still targets an existing
   * id; with `baseRev` a stale rev throws `conflict`. On `rate_limited` patches of the same
   * message wait and merge (at most 1 per second). `cause` is the event the patch answers.
   */
  patchCard(
    groupId: string,
    messageId: string,
    ops: PatchInput,
    opts: { baseRev?: number; cause?: string; clientPatchId?: string } = {},
  ): Promise<Message> {
    let lane = this.patchLanes.get(messageId);
    if (!lane) {
      lane = { groupId, parts: [], running: false, notBefore: 0, timer: null };
      this.patchLanes.set(messageId, lane);
    }
    const l = lane;
    return new Promise<Message>((resolve, reject) => {
      l.parts.push({ input: ops, cause: opts.cause, clientPatchId: opts.clientPatchId, baseRev: opts.baseRev, tries: 0, resolve, reject });
      this.kickLane(messageId, l);
    });
  }

  /** Answers an event without changing anything (2.1, `interaction.resolve`); the note shows for 5 s. */
  async resolveEvent(groupId: string, eventId: string, result: "ignored" | "failed", note?: string): Promise<void> {
    const frame: Record<string, unknown> & { t: string } = { t: "interaction.resolve", groupId, eventId, result };
    if (note) frame.note = [...note].slice(0, 120).join("");
    await this.request(frame);
  }

  /**
   * Replaces the data snapshot of one of my 产出 versions (1.1, `artifact.data`, <= 256 KiB).
   * Every viewer's open page gets it. `baseDataRev` makes the server check the current rev.
   */
  async pushArtifactData(
    groupId: string,
    artifactId: string,
    version: number,
    data: unknown,
    opts: { baseDataRev?: number; cause?: string; clientPushId?: string } = {},
  ): Promise<{ dataRev: number }> {
    const frame: Record<string, unknown> & { t: string } = {
      t: "artifact.data",
      groupId,
      artifactId,
      version,
      data,
      clientPushId: opts.clientPushId ?? crypto.randomUUID(),
    };
    if (opts.baseDataRev !== undefined) frame.baseDataRev = opts.baseDataRev;
    if (opts.cause) frame.cause = opts.cause;
    return (await this.request(frame)) as { dataRev: number };
  }

  /** Answers a page `ask` (1.1, `artifact.reply`): once, within 120 s, private to the asker. */
  async replyArtifact(groupId: string, eventId: string, payload: unknown): Promise<void> {
    const frame: Record<string, unknown> & { t: string } = { t: "artifact.reply", groupId, eventId, ok: true };
    if (payload !== undefined) frame.payload = payload;
    await this.request(frame);
  }

  /** Declines a page `ask`; the page's promise rejects with `declined` (or `failed`). */
  async declineArtifact(groupId: string, eventId: string, message: string, code: "declined" | "failed" = "declined"): Promise<void> {
    await this.request({ t: "artifact.reply", groupId, eventId, ok: false, error: { code, message: [...message].slice(0, 200).join("") } });
  }

  /**
   * Reads what a ref (引用提问) points at: the source message (from this process's cache, else
   * the topic history) or the 产出 version, the block and the part (docs/protocol.md "References
   * to a part"). `current` is false when the source is gone, the card has a newer rev than the
   * one the sender saw, or the block or part no longer exists. Markdown blocks (`md:<n>`) are
   * counted best effort; the ref's `value` is always the sender's data.
   */
  async resolveRef(ref: BlockRef, groupId?: string): Promise<ResolvedRef> {
    if (ref.artifact) {
      const { artifactId, version } = ref.artifact;
      let versions: ArtifactVersion[] = [];
      try {
        versions = (await this.http.getArtifact(artifactId)).versions;
      } catch (err) {
        if (!(err instanceof ChatError && (err.code === "not_found" || err.code === "forbidden"))) throw err;
      }
      const v = versions.find((x) => x.version === version);
      return { artifact: v, block: v, part: v ? findPart(v, ref.part) : undefined, current: v !== undefined && ref.deleted !== true };
    }
    if (!ref.messageId) return { current: false };
    const gid = groupId ?? this.topicGroups.get(ref.sourceTopicId);
    const m = this.messageCache.get(ref.messageId) ?? (gid ? await this.findMessage(gid, ref.sourceTopicId, ref.messageId) : undefined);
    if (!m) return { current: false };
    if (m.deletedAt || ref.deleted) return { message: m, current: false };
    const block = findRefBlock(m, ref.blockId, stripRefPrefix(m.body, m.blockRefs));
    if (block === undefined) return { message: m, current: false };
    const part = findPart(block, ref.part);
    const newer = ref.cardRev !== undefined && m.cardV2 !== undefined && m.cardV2.rev > ref.cardRev;
    return { message: m, block, part, current: !newer && (ref.part === undefined || part !== undefined) };
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
    if (input.baseRev !== undefined) frame.baseRev = input.baseRev;
    if (input.causeEventId) frame.causeEventId = input.causeEventId;
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
          // 1.1: the interaction transients (card and page events) and refs to parts.
          features: [...CLIENT_FEATURES],
          supportedCatalogs: [...CARD_CATALOGS],
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
  private request(frame: Record<string, unknown> & { t: string }, opts: { noRateRetry?: boolean } = {}): Promise<unknown> {
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
        noRateRetry: opts.noRateRetry,
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
      const e = value as { code: ChatError["code"]; message: string; reason?: string };
      if (e.code === "rate_limited" && !p.noRateRetry && Date.now() + 1000 * p.attempts < p.deadline) {
        p.sent = false;
        setTimeout(() => {
          if (this.pending.get(rid) === p) this.trySend(p);
        }, 1000 * p.attempts);
        return;
      }
      this.pending.delete(rid);
      clearTimeout(p.timer);
      p.reject(new ChatError(e.code, e.message, undefined, typeof e.reason === "string" ? e.reason : undefined));
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
        return this.onWelcome(f as { me: Member; members: Member[]; groups: GroupSummary[]; features?: string[]; serverTime?: number });
      case "event":
        return this.onEventFrame(f.event as RawEvent, false);
      case "transient": {
        const e = f.event as TransientEvent;
        if (e && e.type === "artifact.event" && e.actorId !== this.meMember?.id) this.onArtifactEventTransient(e);
        if (e && e.type === "card.event" && e.actorId !== this.meMember?.id) this.onCardEventTransient(e);
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

  private onWelcome(f: { me: Member; members: Member[]; groups: GroupSummary[]; features?: string[]; serverTime?: number }): void {
    if (this.state.memberId && this.state.memberId !== f.me.id) {
      this.fail(new ChatError("invalid", "the state file belongs to another member; use one state file per bot"));
      return;
    }
    if (!this.state.memberId) {
      this.state.memberId = f.me.id;
      this.scheduleSave();
    }
    this.meMember = f.me;
    if (typeof f.serverTime === "number" && Number.isFinite(f.serverTime)) this.clockSkew = f.serverTime - Date.now();
    this.serverFeatureList = Array.isArray(f.features) ? f.features.filter((x) => typeof x === "string") : [];
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
          this.topicGroups.set(t.id, event.groupId);
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
          this.noteMessage(m);
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
          const old = this.messageCache.get(d.messageId);
          if (old) {
            const next: Record<string, unknown> = { ...old };
            for (const [k, v] of Object.entries(d)) if (k !== "messageId" && v !== undefined) next[k] = v;
            this.noteMessage(next as Message);
          }
          // My cards: every rev (patches, edits, and the server's own updates of cardState).
          if (d.cardV2 && (event.actorId === me || this.state.cards[d.messageId] || this.cardCache.has(d.messageId))) {
            this.cacheCard(d.messageId, d.cardV2, d.cardState);
          } else if (d.cardState && this.cardCache.has(d.messageId)) this.cardStates.set(d.messageId, d.cardState);
          break;
        }
        case "message.deleted": {
          const old = this.messageCache.get(event.data.messageId);
          if (old) this.noteMessage({ ...old, deletedAt: event.data.deletedAt });
          break;
        }
        case "card.answered": {
          const ref = this.state.cards[event.data.messageId];
          if (!ref || event.actorId === me) break;
          const answer = this.trackEvent(event.data.messageId, event.data.blockId ?? event.data.actionId);
          if (this.answerHandlers.length === 0) break;
          jobs.push(async () => {
            // After a restart or a cache eviction, load the card by its saved seq first.
            await this.loadCard(event.data.messageId, ref);
            const ctx = await this.answerContext(event, ref.topicId, catchUp, answer);
            if (!ctx) return;
            for (const h of this.answerHandlers) {
              try {
                await h(ctx);
              } catch (err) {
                this.emitError(err, `onCardAnswer for ${event.data.messageId}`);
              }
            }
          });
          break;
        }
        case "artifact.created":
        case "artifact.versionAdded": {
          const { artifact, version } = event.data;
          if (version.createdBy === me || artifact.createdBy === me) this.noteArtifact(artifact.id, { groupId: artifact.groupId, topicId: artifact.topicId });
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
      refs: m.blockRefs ?? [],
      question: stripRefPrefix(m.body, m.blockRefs),
      resolveRef: (ref) => this.resolveRef(ref, m.groupId),
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

  /** Counts an event of a block (or action) for latest-wins; returns its number. */
  private trackEvent(messageId: string, part: string | undefined): { key: string; n: number } {
    const key = `${messageId}:${part ?? ""}`;
    const n = ++this.eventSeq;
    this.latestEvents.set(key, n);
    if (this.latestEvents.size > 5000) {
      const first = this.latestEvents.keys().next().value;
      if (first !== undefined) this.latestEvents.delete(first);
    }
    return { key, n };
  }

  /** Whether the server still takes the event made at `at` (server time) as a cause. */
  private causeFresh(at: number | undefined): boolean {
    return at === undefined || Date.now() + this.clockSkew - at < CAUSE_WINDOW_MS - CAUSE_MARGIN_MS;
  }

  /**
   * Runs a write with the cause the SDK added; when the server says `interact.bad_cause` (an
   * older server, or clock skew), runs it again without one.
   */
  private async withCause<T>(cause: string | undefined, write: (cause: string | undefined) => Promise<T>): Promise<T> {
    if (!cause) return write(undefined);
    try {
      return await write(cause);
    } catch (err) {
      if (!(err instanceof ChatError && err.reason === "interact.bad_cause")) throw err;
      this.log.debug("cause not taken; sent without it", { cause });
      return write(undefined);
    }
  }

  /** The context of a live event or an answer; `responded` turns true when the handler answered. */
  private cardEventContext(
    src: {
      groupId: string;
      topicId: string;
      messageId: string;
      eventId: string;
      rev: number;
      name: string;
      blockId?: string;
      actionId?: string;
      value: unknown;
      inputs: Record<string, unknown>;
      part?: PartRef;
      actorId: string;
      stored: boolean;
      answerId?: string;
      askId?: string;
      catchUp: boolean;
      /** When the server made the event (server time); a cause only within the window. */
      at?: number;
    },
    card: CardV2,
    latest: { key: string; n: number },
  ): { ctx: CardEventContext; flags: { responded: boolean } } {
    const flags = { responded: false };
    const { groupId, topicId, messageId, eventId, at, ...rest } = src;
    // An answer read on catch-up can be older than the server's cause window.
    const cause = eventId && this.causeFresh(at) ? eventId : undefined;
    let n = 0;
    const nextId = () => `bot:${messageId}:${eventId || latest.n}:${n++}`;
    const answered = <T>(p: Promise<T>): Promise<T> =>
      p.then((v) => {
        flags.responded = true;
        return v;
      });
    const ctx: CardEventContext = {
      bot: this,
      ...rest,
      groupId,
      topicId,
      messageId,
      eventId,
      card,
      cardState: this.cardStates.get(messageId),
      isLatest: () => this.latestEvents.get(latest.key) === latest.n,
      patch: (ops) => answered(this.withCause(cause, (c) => this.patchCard(groupId, messageId, ops, c ? { cause: c } : {}))),
      reply: (input) => {
        const i = typeof input === "string" ? { body: input } : { ...input };
        if (!i.clientMsgId) i.clientMsgId = nextId();
        const auto = i.causeEventId ? undefined : cause;
        return answered(this.withCause(auto, (c) => this.send({ groupId, topicId }, c ? { ...i, causeEventId: c } : i)));
      },
      postCard: (c, input = {}) => {
        const i = { ...input };
        if (!i.clientMsgId) i.clientMsgId = nextId();
        const auto = i.causeEventId ? undefined : cause;
        return answered(this.withCause(auto, (x) => this.postCardV2({ groupId, topicId }, c, x ? { ...i, causeEventId: x } : i)));
      },
      resolve: async (result, note) => {
        if (!cause) return;
        await answered(this.resolveEvent(groupId, cause, result, note));
      },
      updateTopic: (change) => this.updateTopic(groupId, topicId, change),
    };
    return { ctx, flags };
  }

  private async answerContext(
    event: EventOf<"card.answered">,
    topicId: string,
    catchUp: boolean,
    latest: { key: string; n: number },
  ): Promise<CardAnswerContext | undefined> {
    const d = event.data;
    const { messageId, rev } = d;
    let card: CardV2 | undefined;
    try {
      card = await this.ensureCard(event.groupId, topicId, messageId);
    } catch (err) {
      this.log.warn("could not read the answered card", { messageId, err: String(err) });
    }
    if (!card) {
      this.emitError(new ChatError("not_found", `the answered card ${messageId} is not readable`), "onCardAnswer");
      return undefined;
    }
    // 2.1: the commit of an immediate block has no action id (it names `blockId`).
    const actionId = d.actionId ?? "";
    const inputs = (d.inputs ?? {}) as Record<string, AnswerValue>;
    const editedValues = d.editedValues ?? {};
    const action = card.actions?.find((a) => a.id === actionId);
    const cancelled = action ? action.kind === "cancel" || action.kind === "reject" : actionId === "cancel" || actionId === "deny";
    // 2.1 input values are objects ({value}, {values}, {value, other}); 2.0 values are bare.
    const raw = (id: string): unknown => {
      const v = inputs[id];
      return v && typeof v === "object" && !Array.isArray(v) && "value" in v ? (v as { value: unknown }).value : v;
    };
    const choices = (id = "choice"): string[] => {
      const v = inputs[id];
      const vs = v && typeof v === "object" && !Array.isArray(v) && "values" in v ? (v as { values: unknown }).values : raw(id);
      if (Array.isArray(vs)) return vs.filter((x): x is string => typeof x === "string");
      return typeof vs === "string" ? [vs] : [];
    };
    const { ctx: base } = this.cardEventContext(
      {
        groupId: event.groupId,
        topicId,
        messageId,
        eventId: d.eventId ?? "",
        rev,
        name: d.event ?? "action.press",
        blockId: d.blockId,
        actionId: d.actionId,
        value: d.value,
        inputs,
        actorId: event.actorId,
        stored: true,
        answerId: d.answerId,
        askId: d.askId,
        catchUp,
        at: event.at,
      },
      card,
      latest,
    );
    const answerCause = d.eventId && this.causeFresh(event.at) ? d.eventId : undefined;
    const update = (c: CardV2, body?: string) =>
      this.withCause(answerCause, (x) => this.updateCardV2(event.groupId, messageId, c, body, x ? { causeEventId: x } : {}));
    return {
      ...base,
      event,
      actionId,
      inputs,
      editedValues,
      action,
      cancelled,
      choices,
      choice: (id) => choices(id)[0],
      text: (id) => {
        const v = raw(id);
        return typeof v === "string" ? v : typeof v === "number" || typeof v === "boolean" ? String(v) : undefined;
      },
      number: (id) => {
        const v = raw(id);
        if (typeof v === "number") return Number.isFinite(v) ? v : undefined;
        if (typeof v === "string" && v.trim() !== "" && Number.isFinite(Number(v))) return Number(v);
        return undefined;
      },
      bool: (id) => {
        const v = raw(id);
        return typeof v === "boolean" ? v : undefined;
      },
      draft: (id = "draft") => {
        if (typeof editedValues[id] === "string") return editedValues[id];
        const v = raw(id);
        if (typeof v === "string") return v;
        const b = findBlock(this.cardCache.get(messageId) ?? card, id);
        return b?.type === "draft" ? b.text : undefined;
      },
      markAnswered: async (change = {}, from) => {
        const c = from ?? this.cardCache.get(messageId) ?? card;
        return update(answeredCard(c, change));
      },
      updateCard: (c, body) => update(c, body),
    };
  }

  // ---------------------------------------------------------------------------
  // Live card events and page events (transient)
  // ---------------------------------------------------------------------------

  /** Runs `job` after the earlier live events of the same key; tracked for `stop()`. */
  private chainLive(key: string, job: () => Promise<void>): void {
    const prev = this.liveChains.get(key) ?? Promise.resolve();
    const run = prev.then(job, job);
    this.liveChains.set(key, run);
    this.running.add(run);
    void run.finally(() => {
      this.running.delete(run);
      if (this.liveChains.get(key) === run) this.liveChains.delete(key);
    });
  }

  private onCardEventTransient(e: Extract<TransientEvent, { type: "card.event" }>): void {
    const latest = this.trackEvent(e.messageId, e.blockId ?? e.actionId);
    this.chainLive(`card:${e.messageId}`, async () => {
      let card: CardV2 | undefined;
      try {
        card = await this.ensureCard(e.groupId, e.topicId, e.messageId);
      } catch (err) {
        this.log.warn("could not read the card of a live event", { messageId: e.messageId, err: String(err) });
      }
      if (!card) {
        await this.resolveEvent(e.groupId, e.eventId, "failed").catch((err) => this.log.debug("resolve failed", { err: String(err) }));
        return;
      }
      const { ctx, flags } = this.cardEventContext(
        {
          groupId: e.groupId,
          topicId: e.topicId,
          messageId: e.messageId,
          eventId: e.eventId,
          rev: e.rev,
          name: e.event,
          blockId: e.blockId,
          actionId: e.actionId,
          value: e.value,
          inputs: (e.inputs ?? {}) as Record<string, unknown>,
          part: e.part,
          actorId: e.actorId,
          stored: false,
          catchUp: false,
          at: e.at,
        },
        card,
        latest,
      );
      let threw = false;
      for (const h of this.cardEventHandlers) {
        try {
          await h(ctx);
        } catch (err) {
          threw = true;
          this.emitError(err, `onCardEvent ${e.event} for ${e.messageId}`);
        }
      }
      if (!flags.responded && !this.stopped) {
        // The person never waits for nothing.
        await this.resolveEvent(e.groupId, e.eventId, threw ? "failed" : "ignored").catch((err) =>
          this.log.debug("auto resolve failed", { eventId: e.eventId, err: String(err) }),
        );
      }
    });
  }

  private onArtifactEventTransient(e: Extract<TransientEvent, { type: "artifact.event" }>): void {
    this.chainLive(`artifact:${e.artifactId}:${e.version}`, async () => {
      const { ctx, flags } = this.artifactEventContext(e);
      let threw = false;
      for (const h of this.artifactEventHandlers) {
        try {
          await h(ctx);
        } catch (err) {
          threw = true;
          this.emitError(err, "onArtifactEvent");
        }
      }
      if (flags.responded || this.stopped) return;
      const done =
        e.kind === "ask"
          ? this.declineArtifact(e.groupId, e.eventId, "", threw ? "failed" : "declined")
          : this.resolveEvent(e.groupId, e.eventId, threw ? "failed" : "ignored");
      await done.catch((err) => this.log.debug("auto answer of a page event failed", { eventId: e.eventId, err: String(err) }));
    });
  }

  private artifactEventContext(event: Extract<TransientEvent, { type: "artifact.event" }>): {
    ctx: ArtifactEventContext;
    flags: { responded: boolean };
  } {
    const { artifactId, version, name, payload, eventId, kind, topicId, groupId } = event;
    const flags = { responded: false, replied: false };
    let n = 0;
    const nextId = () => `bot:${groupId}:${eventId}:${n++}`;
    const once = () => {
      if (kind !== "ask") throw new ChatError("invalid", "reply and decline answer an ask; this event is an emit");
      if (flags.replied) throw new ChatError("conflict", "this ask is answered already");
      flags.replied = true;
    };
    const ctx: ArtifactEventContext = {
      bot: this,
      event,
      groupId,
      topicId,
      eventId,
      kind,
      artifactId,
      version,
      name,
      payload,
      actorId: event.actorId,
      catchUp: false,
      reply: async (p) => {
        once();
        await this.replyArtifact(groupId, eventId, p);
        flags.responded = true;
      },
      decline: async (message) => {
        once();
        await this.declineArtifact(groupId, eventId, message);
        flags.responded = true;
      },
      pushData: async (data, opts = {}) => {
        const r = await this.pushArtifactData(groupId, artifactId, version, data, { ...opts, cause: eventId });
        // An ask still needs its reply; an emit is answered by the push.
        if (kind === "emit") flags.responded = true;
        return r;
      },
      postMessage: async (input) => {
        const i = typeof input === "string" ? { body: input } : { ...input };
        if (!i.clientMsgId) i.clientMsgId = nextId();
        // The page event is the cause (INTERACT 4.3); a server without page causes gets none.
        const auto = i.causeEventId ? undefined : eventId;
        const r = await this.withCause(auto, (c) => this.send({ groupId, topicId }, c ? { ...i, causeEventId: c } : i));
        if (kind === "emit") flags.responded = true;
        return r;
      },
    };
    return { ctx, flags };
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
    this.state.cards[m.id] = { groupId: m.groupId, topicId: m.topicId, seq: m.createdSeq };
    const ids = Object.keys(this.state.cards);
    if (ids.length > MAX_REMEMBERED_CARDS) {
      // Message ids are ULIDs: lexical order is time order.
      ids.sort();
      for (const id of ids.slice(0, ids.length - MAX_REMEMBERED_CARDS)) delete this.state.cards[id];
    }
    this.scheduleSave();
  }

  /** Puts the v2 card of one of my card messages in the cache when it is not there. */
  private async loadCard(messageId: string, ref: CardRef): Promise<void> {
    if (this.cardCache.has(messageId)) return;
    try {
      let before = ref.seq === undefined ? undefined : ref.seq + 1;
      // Without a saved seq (state from an older SDK), look back through at most 10 pages.
      for (let page = 0; page < (ref.seq === undefined ? 10 : 1); page++) {
        const r = await this.history(ref.groupId, ref.topicId, { beforeSeq: before, limit: ref.seq === undefined ? 100 : 1 });
        const m = r.messages.find((x) => x.id === messageId);
        if (m) {
          if (m.cardV2) this.cacheCard(m.id, m.cardV2, m.cardState);
          return;
        }
        if (!r.hasMore || r.messages.length === 0) return;
        before = Math.min(...r.messages.map((x) => x.createdSeq));
      }
    } catch (err) {
      this.log.warn("could not load the answered card", { messageId, err: String(err) });
    }
  }

  private cacheCard(messageId: string, card: CardV2, state?: CardState): void {
    const old = this.cardCache.get(messageId);
    if (old && old.rev > card.rev) return;
    this.cardCache.delete(messageId);
    this.cardCache.set(messageId, card);
    if (state) this.cardStates.set(messageId, state);
    if (this.cardCache.size > MAX_CACHED_CARDS) {
      const first = this.cardCache.keys().next().value;
      if (first !== undefined) {
        this.cardCache.delete(first);
        this.cardStates.delete(first);
      }
    }
    if (!old || card.rev > old.rev) for (const w of this.revWaiters.get(messageId)?.splice(0) ?? []) w();
  }

  /** Remembers a message for `resolveRef` (newest last, at most MAX_CACHED_MESSAGES). */
  private noteMessage(m: Message): void {
    this.topicGroups.set(m.topicId, m.groupId);
    this.messageCache.delete(m.id);
    this.messageCache.set(m.id, m);
    if (this.messageCache.size > MAX_CACHED_MESSAGES) {
      const first = this.messageCache.keys().next().value;
      if (first !== undefined) this.messageCache.delete(first);
    }
    if (m.cardV2 && this.cardCache.has(m.id)) this.cacheCard(m.id, m.cardV2, m.cardState);
  }

  /** Reads a message from the topic history, newest first (at most MAX_HISTORY_PAGES pages). */
  private async findMessage(groupId: string, topicId: string, messageId: string): Promise<Message | undefined> {
    let beforeSeq: number | undefined;
    for (let page = 0; page < MAX_HISTORY_PAGES; page++) {
      const r = await this.history(groupId, topicId, beforeSeq === undefined ? { limit: 100 } : { beforeSeq, limit: 100 });
      const m = r.messages.find((x) => x.id === messageId);
      if (m) {
        this.noteMessage(m);
        return m;
      }
      if (!r.hasMore || r.messages.length === 0) return undefined;
      beforeSeq = Math.min(...r.messages.map((x) => x.createdSeq));
    }
    return undefined;
  }

  /** The newest known v2 card of a message: the cache, else the topic history. */
  private async ensureCard(groupId: string, topicId: string, messageId: string): Promise<CardV2 | undefined> {
    const known = this.cardCache.get(messageId);
    if (known) return known;
    const m = await this.findMessage(groupId, topicId, messageId);
    if (m?.cardV2) this.cacheCard(messageId, m.cardV2, m.cardState);
    return this.cardCache.get(messageId);
  }

  /** The newest card after a stale rev: waits a moment for its event, else reads the history. */
  private async freshCard(groupId: string, messageId: string, staleRev: number): Promise<CardV2 | undefined> {
    const newer = () => {
      const c = this.cardCache.get(messageId);
      return c && c.rev > staleRev ? c : undefined;
    };
    if (newer()) return newer();
    await new Promise<void>((resolve) => {
      const list = this.revWaiters.get(messageId) ?? [];
      this.revWaiters.set(messageId, list);
      const done = () => {
        clearTimeout(t);
        resolve();
      };
      const t = setTimeout(() => {
        const i = list.indexOf(done);
        if (i >= 0) list.splice(i, 1);
        resolve();
      }, REBASE_WAIT_MS);
      list.push(done);
    });
    if (newer()) return newer();
    const topicId = this.state.cards[messageId]?.topicId ?? this.messageCache.get(messageId)?.topicId;
    if (!topicId) return undefined;
    const m = await this.findMessage(groupId, topicId, messageId);
    if (m?.cardV2) this.cacheCard(messageId, m.cardV2, m.cardState);
    return newer();
  }

  /** Starts the next patch of a lane when it is free and not waiting after `rate_limited`. */
  private kickLane(messageId: string, lane: PatchLane): void {
    if (lane.running || lane.timer || lane.parts.length === 0) return;
    const wait = lane.notBefore - Date.now();
    if (wait > 0) {
      lane.timer = setTimeout(() => {
        lane.timer = null;
        this.kickLane(messageId, lane);
      }, wait);
      return;
    }
    lane.running = true;
    void this.runLane(messageId, lane).finally(() => {
      lane.running = false;
      if (lane.parts.length > 0) this.kickLane(messageId, lane);
      else if (!lane.timer) this.patchLanes.delete(messageId);
    });
  }

  /** Sends the queued parts of a lane as one patch (merged, at most MAX_PATCH_OPS ops). */
  private async runLane(messageId: string, lane: PatchLane): Promise<void> {
    const first = lane.parts[0];
    if (!first) return;
    let card = this.cardCache.get(messageId);
    if (!card && first.baseRev === undefined) {
      const topicId = this.state.cards[messageId]?.topicId ?? this.messageCache.get(messageId)?.topicId;
      if (topicId) card = await this.ensureCard(lane.groupId, topicId, messageId).catch(() => undefined);
    }
    if (!card && first.baseRev === undefined) {
      lane.parts.shift();
      first.reject(new ChatError("invalid", `the card of ${messageId} is not known; pass baseRev`));
      return;
    }
    // Merge: parts without their own baseRev join the first one while the ops fit in one frame.
    // A frame names one cause and the server resolves only that event, so a part with another
    // cause waits for the next frame.
    const batch: PatchPart[] = [];
    const ops: PatchOp[] = [];
    let cause: string | undefined;
    for (const part of lane.parts) {
      if (batch.length > 0 && (part.baseRev !== undefined || part.alone || first.alone)) break;
      if (batch.length > 0 && part.cause && cause && part.cause !== cause) break;
      let partOps: PatchOp[];
      try {
        partOps = card ? opsOf(card, part.input) : Array.isArray(part.input) ? part.input : [];
      } catch (err) {
        if (batch.length > 0) break;
        lane.parts.shift();
        part.reject(err);
        return;
      }
      if (batch.length > 0 && ops.length + partOps.length > MAX_PATCH_OPS) break;
      batch.push(part);
      ops.push(...partOps);
      cause ??= part.cause;
    }
    lane.parts.splice(0, batch.length);
    const baseRev = first.baseRev ?? (card as CardV2).rev;
    const frame: Record<string, unknown> & { t: string } = {
      t: "card.patch",
      groupId: lane.groupId,
      messageId,
      baseRev,
      ops,
      clientPatchId: batch.length === 1 && first.clientPatchId ? first.clientPatchId : crypto.randomUUID(),
    };
    if (cause) frame.cause = cause;
    try {
      const r = (await this.request(frame, { noRateRetry: true })) as CardPatchAck;
      if (r.message.cardV2) this.cacheCard(messageId, r.message.cardV2, r.message.cardState);
      this.noteMessage(r.message);
      for (const p of batch) p.resolve(r.message);
    } catch (err) {
      const e = err instanceof ChatError ? err : undefined;
      if (e?.code === "rate_limited") {
        lane.notBefore = Date.now() + PATCH_INTERVAL_MS;
        lane.parts.unshift(...batch);
        return;
      }
      const stale = e?.code === "conflict" && (e.reason === "card.stale_rev" || /stale_rev/.test(e.message));
      for (const p of batch) p.tries++;
      if (stale && batch.every((p) => p.baseRev === undefined) && batch.every((p) => p.tries < MAX_PATCH_TRIES)) {
        const fresh = await this.freshCard(lane.groupId, messageId, baseRev).catch(() => undefined);
        let fits = false;
        if (fresh) {
          try {
            fits = opsFit(fresh, batch.flatMap((p) => opsOf(fresh, p.input)));
          } catch {
            fits = false;
          }
        }
        if (fits) {
          lane.parts.unshift(...batch);
          return;
        }
        const conflict = new ChatError("conflict", "the card changed and the patch no longer fits the newest rev", undefined, "card.stale_rev");
        for (const p of batch) p.reject(conflict);
        return;
      }
      if (batch.length > 1 && !stale) {
        // One bad part must not fail the others: each one goes again on its own.
        for (const p of batch) p.alone = true;
        lane.parts.unshift(...batch);
        return;
      }
      for (const p of batch) p.reject(err);
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
