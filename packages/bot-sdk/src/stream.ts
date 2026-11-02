// A streaming reply: `send` with streaming true, coalesced `delta` frames, and a final
// `edit` with the full body and streaming false.
import { MAX_BODY_CHARS, type Card, type FileRef, type ArtifactRef, type Message } from "@chat/protocol";
import { ChatError } from "./errors.ts";

/** What a StreamWriter needs from the bot. */
export interface StreamHost {
  sendDelta(groupId: string, messageId: string, append: string): boolean;
  edit(
    groupId: string,
    messageId: string,
    edit: { body: string; streaming?: boolean; card?: Card; attachments?: FileRef[]; artifactRefs?: ArtifactRef[] },
  ): Promise<{ message: Message }>;
  streamEnded(writer: StreamWriter): void;
}

export interface StreamOptions {
  /** Delta flush interval. Default 100 ms (the server coalesces at the same rate). */
  flushMs?: number;
  /** Flush early when this many characters are buffered. Default 4096. */
  flushChars?: number;
  /** The message is already final (a retried stream); the writer accepts no more text. */
  finished?: boolean;
}

function codePoints(s: string): number {
  let n = 0;
  for (const _ of s) n++;
  return n;
}

export class StreamWriter {
  /** The message as the server made it (streaming: true). */
  readonly message: Message;
  private readonly host: StreamHost;
  private textSoFar: string;
  private chars: number;
  private pending = "";
  private timer: ReturnType<typeof setTimeout> | null = null;
  private finished = false;
  private readonly flushMs: number;
  private readonly flushChars: number;

  constructor(host: StreamHost, message: Message, opts: StreamOptions = {}) {
    this.host = host;
    this.message = message;
    this.textSoFar = message.body;
    this.chars = codePoints(message.body);
    this.flushMs = opts.flushMs ?? 100;
    this.flushChars = opts.flushChars ?? 4096;
    this.finished = opts.finished ?? false;
  }

  get groupId(): string {
    return this.message.groupId;
  }
  get messageId(): string {
    return this.message.id;
  }
  /** The full body written so far. */
  get text(): string {
    return this.textSoFar;
  }
  get done(): boolean {
    return this.finished;
  }

  /** Adds text. Deltas are batched; throws when the body would pass the size limit. */
  append(text: string): void {
    if (this.finished) throw new ChatError("conflict", "stream is finished");
    if (!text) return;
    const n = codePoints(text);
    if (this.chars + n > MAX_BODY_CHARS) throw new ChatError("too_large", `message body exceeds ${MAX_BODY_CHARS} characters`);
    this.chars += n;
    this.textSoFar += text;
    this.pending += text;
    if (this.pending.length >= this.flushChars) this.flush();
    else if (!this.timer) this.timer = setTimeout(() => this.flush(), this.flushMs);
  }

  /** Sends buffered text now. Text that cannot be sent (no connection) is covered by the final edit. */
  flush(): void {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    if (!this.pending || this.finished) return;
    const chunk = this.pending;
    this.pending = "";
    this.host.sendDelta(this.groupId, this.messageId, chunk);
  }

  /** Called by the bot after a reconnect: deltas may be lost, so readers get the full text again. */
  async resync(): Promise<void> {
    if (this.finished) return;
    this.pending = "";
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    await this.host.edit(this.groupId, this.messageId, { body: this.textSoFar, streaming: true });
  }

  /**
   * Ends the stream: one `edit` with the full body (or `body` when given) and streaming false.
   * `extra` can attach files, 产出 references or a card.
   */
  async finish(
    body?: string,
    extra: { card?: Card; attachments?: FileRef[]; artifactRefs?: ArtifactRef[] } = {},
  ): Promise<Message> {
    if (this.finished) throw new ChatError("conflict", "stream is finished");
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.pending = "";
    const final = body ?? this.textSoFar;
    this.finished = true;
    try {
      const r = await this.host.edit(this.groupId, this.messageId, { body: final, streaming: false, ...extra });
      this.textSoFar = final;
      return r.message;
    } catch (err) {
      this.finished = false;
      throw err;
    } finally {
      if (this.finished) this.host.streamEnded(this);
    }
  }
}
