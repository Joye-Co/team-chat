// HTTP API client: one-shot sends for scripts, files and 产出. Uses the same bearer token
// as the WebSocket connection.
import type {
  AcceptanceItem,
  AddArtifactVersionRequest,
  ArtifactDataAck,
  ArtifactDataResponse,
  ArtifactEventDecl,
  ArtifactEventsResponse,
  ArtifactPreview,
  BotRun,
  DeskNotesResponse,
  EndRunRequest,
  RunPlanItem,
  StartRunRequest,
  Artifact,
  ArtifactResponse,
  ArtifactVersionResponse,
  CreateArtifactRequest,
  CreateFileRequest,
  CreateFileResponse,
  FileMeta,
  FileRef,
  GroupSummary,
  Member,
  PostMessageRequest,
  PutArtifactMediaResponse,
  PutDeskNoteResponse,
  SendResult,
} from "@chat/protocol";
import { ArtifactPreview as ArtifactPreviewSchema, MAX_ARTIFACT_DATA_BYTES, MAX_FILE_BYTES, PutArtifactMediaRequest } from "@chat/protocol";
import { ChatError, isRetryable } from "./errors.ts";
import { checkArtifactEvents } from "./pattern.ts";

export interface HttpClientOptions {
  /** Base URL of the server, for example `http://127.0.0.1:8789`. */
  url: string;
  token: string;
  fetch?: typeof fetch;
  /** Per-request timeout. Default 30 s; uploads get 10 minutes. */
  timeoutMs?: number;
}

/** `POST /v1/groups/:id/messages` body; `clientMsgId` is made when absent. */
export type PostMessageInput = Omit<PostMessageRequest, "clientMsgId"> & { clientMsgId?: string };

export interface UploadInput {
  groupId: string;
  topicId?: string;
  name: string;
  mime: string;
  data: Uint8Array | ArrayBuffer | Blob;
  width?: number;
  height?: number;
}

/** `HttpClient.publishHtml`: one HTML page as a 产出 of kind `html`, or a new version of one. */
export interface PublishHtmlInput {
  groupId: string;
  topicId: string;
  title: string;
  html: string;
  note?: string;
  /** Adds a version to this 产出 instead of making a new one. */
  artifactId?: string;
  extraFiles?: { path: string; mime: string; data: Uint8Array | ArrayBuffer | Blob }[];
  /**
   * 1.1: events the page may send back (`teamchat.emit`, `teamchat.ask`); they reach
   * `Bot.onArtifactEvent`. Only a bot's version keeps them.
   */
  events?: ArtifactEventDecl[];
  /**
   * 1.1: the first data snapshot of the version (the page's `teamchat.getData` and `onData`), at
   * most 256 KiB. Without it the page opens with `null` until the first `pushArtifactData`.
   * Only a bot's version keeps it.
   */
  data?: unknown;
  /** 1.1: the title, summary and up to 6 figures of the version's preview card. */
  preview?: ArtifactPreview;
  /** 1.1: this bot's run that made the version (`Bot.currentRun(topicId)`). */
  runId?: string;
}

type Body = string | Uint8Array | ArrayBuffer | Blob;

export function normalizeBaseUrl(url: string): string {
  const u = new URL(url);
  if (u.protocol !== "http:" && u.protocol !== "https:") throw new Error("server url must be http:// or https://");
  return u.toString().replace(/\/+$/, "");
}

async function toBytes(data: Uint8Array | ArrayBuffer | Blob): Promise<Uint8Array> {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  return new Uint8Array(await data.arrayBuffer());
}

/**
 * Checks a data snapshot of a 产出 version as the server does: JSON of at most 256 KiB
 * (`MAX_ARTIFACT_DATA_BYTES`). Throws `ChatError` `too_large` or `invalid` before anything is sent.
 */
export function checkArtifactData(data: unknown): void {
  if (data === undefined) return;
  let text: string | undefined;
  try {
    text = JSON.stringify(data);
  } catch {
    text = undefined;
  }
  if (text === undefined) throw new ChatError("invalid", "data is not JSON");
  const bytes = new TextEncoder().encode(text).byteLength;
  if (bytes > MAX_ARTIFACT_DATA_BYTES) throw new ChatError("too_large", `data is ${bytes} bytes; a snapshot is at most ${MAX_ARTIFACT_DATA_BYTES}`);
}

export async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes as Uint8Array<ArrayBuffer>);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

export class HttpClient {
  readonly url: string;
  private readonly origin: string;
  private readonly token: string;
  private readonly fetchImpl: typeof fetch;
  private readonly timeoutMs: number;

  constructor(opts: HttpClientOptions) {
    this.url = normalizeBaseUrl(opts.url);
    this.origin = new URL(this.url).origin;
    this.token = opts.token;
    this.fetchImpl = opts.fetch ?? fetch;
    this.timeoutMs = opts.timeoutMs ?? 30_000;
  }

  /** One request. Throws ChatError with the server's code on a non-2xx answer. */
  async request<T>(
    method: string,
    path: string,
    opts: { json?: unknown; body?: Body; headers?: Record<string, string>; timeoutMs?: number; absolute?: boolean } = {},
  ): Promise<T> {
    const target = opts.absolute ? new URL(path) : new URL(`${this.url}${path}`);
    const headers: Record<string, string> = { ...(opts.headers ?? {}) };
    // The token goes only to the configured server, never to another origin.
    if (target.origin === this.origin) headers.authorization = `Bearer ${this.token}`;
    let body: Body | undefined = opts.body;
    if (opts.json !== undefined) {
      headers["content-type"] = "application/json";
      body = JSON.stringify(opts.json);
    }
    let res: Response;
    try {
      res = await this.fetchImpl(target, {
        method,
        headers,
        body: body as BodyInit | undefined,
        signal: AbortSignal.timeout(opts.timeoutMs ?? this.timeoutMs),
      });
    } catch (err) {
      const name = (err as Error)?.name;
      if (name === "TimeoutError" || name === "AbortError") throw new ChatError("timeout", `${method} ${target.pathname} timed out`);
      throw new ChatError("network", `${method} ${target.pathname} failed: ${(err as Error)?.message ?? err}`);
    }
    const text = await res.text();
    let data: unknown = undefined;
    if (text) {
      try {
        data = JSON.parse(text);
      } catch {
        data = text;
      }
    }
    if (!res.ok) {
      const e = (data as { error?: { code?: string; message?: string; reason?: string } } | undefined)?.error;
      const code = (e?.code as ChatError["code"]) ?? (res.status === 401 ? "unauthorized" : res.status >= 500 ? "internal" : "invalid");
      throw new ChatError(code, e?.message ?? `HTTP ${res.status}`, res.status, typeof e?.reason === "string" ? e.reason : undefined);
    }
    return data as T;
  }

  health(): Promise<{ ok: true; version: string }> {
    return this.request("GET", "/v1/health");
  }
  me(): Promise<Member> {
    return this.request("GET", "/v1/me");
  }
  members(): Promise<Member[]> {
    return this.request("GET", "/v1/members");
  }
  groups(): Promise<GroupSummary[]> {
    return this.request("GET", "/v1/groups");
  }
  /** Finds or makes the DM with one member. */
  dm(memberId: string): Promise<GroupSummary> {
    return this.request("POST", "/v1/dms", { json: { memberId } });
  }

  /**
   * `POST /v1/groups/:id/messages`: the `send` path over HTTP, for one-shot scripts.
   * Network errors, 5xx and rate limits are retried with the same clientMsgId, so the
   * message is posted at most once.
   */
  async postMessage(groupId: string, input: PostMessageInput, opts: { attempts?: number } = {}): Promise<SendResult> {
    const body = { ...input, clientMsgId: input.clientMsgId ?? crypto.randomUUID() };
    const attempts = opts.attempts ?? 4;
    for (let i = 1; ; i++) {
      try {
        return await this.request<SendResult>("POST", `/v1/groups/${encodeURIComponent(groupId)}/messages`, { json: body });
      } catch (err) {
        if (i >= attempts || !isRetryable(err)) throw err;
        await new Promise((r) => setTimeout(r, Math.min(8000, 500 * 2 ** (i - 1))));
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Runs and run plans (docs/protocol.md, "Run plans")
  // ---------------------------------------------------------------------------

  /** Retries network errors, 5xx and rate limits: these calls are idempotent. */
  private async retried<T>(fn: () => Promise<T>, attempts = 4): Promise<T> {
    for (let i = 1; ; i++) {
      try {
        return await fn();
      } catch (err) {
        if (i >= attempts || !isRetryable(err)) throw err;
        await new Promise((r) => setTimeout(r, Math.min(8000, 500 * 2 ** (i - 1))));
      }
    }
  }

  /**
   * `POST /v1/runs`: starts a run of this bot in the topic, or returns its active run there.
   * `plan`, when given, replaces the run's plan. Runner-backed bots get `conflict`.
   */
  startRun(req: StartRunRequest): Promise<BotRun> {
    return this.retried(() => this.request<BotRun>("POST", "/v1/runs", { json: req }));
  }

  /** `PUT /v1/runs/:id/plan`: replaces the whole plan of an active run; `[]` clears it. */
  setRunPlan(runId: string, plan: RunPlanItem[]): Promise<BotRun> {
    return this.retried(() => this.request<BotRun>("PUT", `/v1/runs/${encodeURIComponent(runId)}/plan`, { json: { plan } }));
  }

  /**
   * `PUT /v1/runs/:id/acceptance` (1.1): replaces the 验收标准 of an active run of this bot;
   * `[]` clears them. A server without the route answers `not_found`.
   */
  setRunAcceptance(runId: string, items: AcceptanceItem[]): Promise<BotRun> {
    return this.retried(() => this.request<BotRun>("PUT", `/v1/runs/${encodeURIComponent(runId)}/acceptance`, { json: { items } }));
  }

  /** `POST /v1/runs/:id/end`: ends a run this bot started with `startRun`. */
  endRun(runId: string, req: EndRunRequest): Promise<BotRun> {
    return this.request<BotRun>("POST", `/v1/runs/${encodeURIComponent(runId)}/end`, { json: req });
  }

  getRun(runId: string): Promise<BotRun> {
    return this.request<BotRun>("GET", `/v1/runs/${encodeURIComponent(runId)}`);
  }

  /**
   * `PUT /v1/desk-note` (1.1): this bot's one-line priority for the 首页 desk in the group
   * (at most 140 characters after trimming; `""` clears it). `seq` is null when the text did not
   * change. People get `forbidden`; a group the bot is not in, `not_found`.
   */
  setDeskNote(groupId: string, text: string): Promise<PutDeskNoteResponse> {
    return this.retried(() => this.request<PutDeskNoteResponse>("PUT", "/v1/desk-note", { json: { groupId, text } }));
  }

  /** `GET /v1/groups/:id/desk-notes` (1.1): the desk notes of the group's bots, newest first. */
  deskNotes(groupId: string): Promise<DeskNotesResponse> {
    return this.request<DeskNotesResponse>("GET", `/v1/groups/${encodeURIComponent(groupId)}/desk-notes`);
  }

  // ---------------------------------------------------------------------------
  // Files and 产出
  // ---------------------------------------------------------------------------

  createFile(req: CreateFileRequest): Promise<CreateFileResponse> {
    return this.request("POST", "/v1/files", { json: req });
  }
  getFile(fileId: string): Promise<FileMeta> {
    return this.request("GET", `/v1/files/${encodeURIComponent(fileId)}`);
  }

  /** Creates the file record, uploads the bytes and returns a FileRef for `attachments`. */
  async uploadFile(input: UploadInput): Promise<FileRef> {
    const bytes = await toBytes(input.data);
    if (bytes.byteLength > MAX_FILE_BYTES) throw new ChatError("too_large", `file is larger than ${MAX_FILE_BYTES} bytes`);
    const sha256 = await sha256Hex(bytes);
    const created = await this.createFile({
      groupId: input.groupId,
      topicId: input.topicId,
      name: input.name,
      size: bytes.byteLength,
      mime: input.mime,
      sha256,
      width: input.width,
      height: input.height,
    });
    const meta = await this.request<FileMeta>("PUT", created.uploadUrl, {
      absolute: true,
      body: bytes,
      headers: { "content-type": "application/octet-stream" },
      timeoutMs: 600_000,
    });
    return {
      fileId: meta.id,
      name: meta.name,
      size: meta.size,
      mime: meta.mime,
      sha256: meta.sha256,
      ...(meta.width ? { width: meta.width } : {}),
      ...(meta.height ? { height: meta.height } : {}),
    };
  }

  /** Throws `ChatError` before the request when `events` has a schema the server refuses (`invalid`) or `data` is over 256 KiB (`too_large`). */
  async createArtifact(req: CreateArtifactRequest): Promise<ArtifactVersionResponse> {
    checkArtifactEvents(req.events);
    checkArtifactData(req.data);
    return this.request("POST", "/v1/artifacts", { json: req });
  }
  /** Throws `ChatError` before the request, as `createArtifact` does. */
  async addArtifactVersion(artifactId: string, req: AddArtifactVersionRequest): Promise<ArtifactVersionResponse> {
    checkArtifactEvents(req.events);
    checkArtifactData(req.data);
    return this.request("POST", `/v1/artifacts/${encodeURIComponent(artifactId)}/versions`, { json: req });
  }
  /**
   * Attaches the media of a version with its preview after the upload
   * (`PUT /v1/artifacts/:id/versions/:v/media`, the version's creator only).
   */
  async putArtifactMedia(artifactId: string, version: number, media: PutArtifactMediaRequest["media"]): Promise<PutArtifactMediaResponse> {
    const req = PutArtifactMediaRequest.safeParse({ media });
    if (!req.success) throw new ChatError("invalid", `bad media: ${req.error.issues[0]?.message ?? ""}`.trim());
    return this.request("PUT", `/v1/artifacts/${encodeURIComponent(artifactId)}/versions/${version}/media`, { json: req.data });
  }
  listArtifacts(groupId: string, topicId?: string): Promise<Artifact[]> {
    const q = new URLSearchParams({ groupId });
    if (topicId) q.set("topicId", topicId);
    return this.request("GET", `/v1/artifacts?${q}`);
  }
  getArtifact(artifactId: string): Promise<ArtifactResponse> {
    return this.request("GET", `/v1/artifacts/${encodeURIComponent(artifactId)}`);
  }

  /**
   * `GET /v1/artifacts/:id/versions/:v/data` (1.1): the version's latest data snapshot, as every
   * open copy of the page has it. `data` is null and `dataRev` 0 when the version has none. Read
   * it after a restart instead of keeping the page's state only in process memory.
   */
  getArtifactData(artifactId: string, version: number): Promise<ArtifactDataResponse> {
    return this.request("GET", `/v1/artifacts/${encodeURIComponent(artifactId)}/versions/${version}/data`);
  }
  /**
   * `PUT /v1/artifacts/:id/versions/:v/data` (1.1): replaces the snapshot over HTTP, for a script
   * without a connection (`Bot.pushArtifactData` is the same write over the WebSocket). Only the
   * bot that made the version. `baseDataRev` makes the server refuse the write with `conflict`
   * when the snapshot changed since that rev; `cause` names the page event it answers.
   */
  async putArtifactData(artifactId: string, version: number, data: unknown, opts: { baseDataRev?: number; cause?: string } = {}): Promise<ArtifactDataAck> {
    if (data === undefined) throw new ChatError("invalid", "data is required (JSON; null is a value)");
    checkArtifactData(data);
    const json: Record<string, unknown> = { data };
    if (opts.baseDataRev !== undefined) json.baseDataRev = opts.baseDataRev;
    if (opts.cause) json.cause = opts.cause;
    return this.request("PUT", `/v1/artifacts/${encodeURIComponent(artifactId)}/versions/${version}/data`, { json });
  }
  /**
   * `GET /v1/artifacts/:id/events` (1.1): the stored page events of a 产出 (all its versions),
   * newest first. `before` is an event id (lists older ones); `limit` is 1..200, default 100.
   * `replied` is true on an ask its bot answered. The bot gets page events only while it is
   * connected: read here what it missed.
   */
  artifactEvents(artifactId: string, opts: { before?: string; limit?: number } = {}): Promise<ArtifactEventsResponse> {
    const q = new URLSearchParams();
    if (opts.before) q.set("before", opts.before);
    if (opts.limit !== undefined) q.set("limit", String(opts.limit));
    const qs = q.toString();
    return this.request("GET", `/v1/artifacts/${encodeURIComponent(artifactId)}/events${qs ? `?${qs}` : ""}`);
  }

  /**
   * Uploads one HTML page (and optional extra files) and makes a 产出 of kind `html`, or
   * adds a version when `artifactId` is given.
   */
  async publishHtml(input: PublishHtmlInput): Promise<ArtifactVersionResponse> {
    // Before the uploads: a declaration, a snapshot or a preview the server refuses fails here.
    checkArtifactEvents(input.events);
    checkArtifactData(input.data);
    if (input.preview !== undefined) {
      const p = ArtifactPreviewSchema.safeParse(input.preview);
      if (!p.success) throw new ChatError("invalid", `preview: ${p.error.issues[0]?.path.join(".") ?? ""} ${p.error.issues[0]?.message ?? ""}`.trim());
    }
    const entry = "index.html";
    const files = [
      { path: entry, fileId: (await this.uploadFile({ groupId: input.groupId, topicId: input.topicId, name: entry, mime: "text/html", data: new TextEncoder().encode(input.html) })).fileId },
    ];
    for (const f of input.extraFiles ?? []) {
      const name = f.path.split("/").pop() || f.path;
      const ref = await this.uploadFile({ groupId: input.groupId, topicId: input.topicId, name, mime: f.mime, data: f.data });
      files.push({ path: f.path, fileId: ref.fileId });
    }
    const extra = {
      ...(input.events ? { events: input.events } : {}),
      ...(input.data !== undefined ? { data: input.data as CreateArtifactRequest["data"] } : {}),
      ...(input.preview ? { preview: input.preview } : {}),
      ...(input.runId ? { runId: input.runId } : {}),
    };
    if (input.artifactId) {
      return this.addArtifactVersion(input.artifactId, { entry, files, note: input.note, title: input.title, ...extra });
    }
    return this.createArtifact({
      groupId: input.groupId,
      topicId: input.topicId,
      title: input.title,
      kind: "html",
      entry,
      files,
      note: input.note,
      ...extra,
    });
  }
}
