// HTTP API client: one-shot sends for scripts, files and 产出. Uses the same bearer token
// as the WebSocket connection.
import type {
  AcceptanceItem,
  AddArtifactVersionRequest,
  ArtifactEventDecl,
  BotRun,
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
  SendResult,
} from "@chat/protocol";
import { MAX_FILE_BYTES } from "@chat/protocol";
import { ChatError, isRetryable } from "./errors.ts";

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

  createArtifact(req: CreateArtifactRequest): Promise<ArtifactVersionResponse> {
    return this.request("POST", "/v1/artifacts", { json: req });
  }
  addArtifactVersion(artifactId: string, req: AddArtifactVersionRequest): Promise<ArtifactVersionResponse> {
    return this.request("POST", `/v1/artifacts/${encodeURIComponent(artifactId)}/versions`, { json: req });
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
   * Uploads one HTML page (and optional extra files) and makes a 产出 of kind `html`, or
   * adds a version when `artifactId` is given.
   */
  async publishHtml(input: {
    groupId: string;
    topicId: string;
    title: string;
    html: string;
    note?: string;
    artifactId?: string;
    extraFiles?: { path: string; mime: string; data: Uint8Array | ArrayBuffer | Blob }[];
    /** 1.1: events the page may send back (`emitEvent`); they reach `Bot.onArtifactEvent`. */
    events?: ArtifactEventDecl[];
    /** 1.1: this bot's run that made the version (`Bot.currentRun(topicId)`). */
    runId?: string;
  }): Promise<ArtifactVersionResponse> {
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
