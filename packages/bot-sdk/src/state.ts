// What a bot remembers across restarts: the last handled seq per group, the topics it
// follows, its own card messages and 产出 (so card presses, answers and page events reach it
// after a restart), and its side chat topics with the topic each one is about.
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { ChatError } from "./errors.ts";

/** A path with a leading `~` (alone, or before `/` or `\`) replaced by the home folder. */
export function expandHome(path: string): string {
  if (path === "~") return homedir();
  return path.startsWith("~/") || path.startsWith("~\\") ? join(homedir(), path.slice(2)) : path;
}

/** Where one of the bot's card messages (or 产出) is. */
export interface CardRef {
  groupId: string;
  topicId: string;
  /** The card message's `createdSeq`, so its card can be loaded again after a restart. */
  seq?: number;
}

export interface BotStateData {
  v: 1;
  memberId?: string;
  cursors: Record<string, number>;
  following: Record<string, string[]>;
  cards: Record<string, CardRef>;
  /** 1.1: my 产出 by artifact id. */
  artifacts?: Record<string, CardRef>;
  /** 1.1: side chat topics (单独问) by topic id: the topic each one reads, up to which seq. */
  asides?: Record<string, AsideRef>;
  /**
   * 1.1: DM groups by id, read for side chat topics from their start up to this seq. A side chat
   * opened before the cursor (a first start that skipped history, or state of an older SDK) is
   * found this way; later ones arrive as events.
   */
  asideScan?: Record<string, number>;
}

/** A side chat topic in a DM: `groupId`/`topicId`/`uptoSeq` name the topic it is about. */
export interface AsideRef {
  dmGroupId: string;
  groupId: string;
  topicId: string;
  uptoSeq: number;
}

/** At most this many own card messages are remembered (oldest are dropped). */
export const MAX_REMEMBERED_CARDS = 2000;
/** At most this many own 产出 and side chat topics are remembered (oldest are dropped). */
export const MAX_REMEMBERED_REFS = 2000;

export function emptyState(): BotStateData {
  return { v: 1, cursors: {}, following: {}, cards: {} };
}

/** Where state lives. `load` returns null when nothing was saved yet. */
export interface StateStore {
  load(): BotStateData | null;
  save(state: BotStateData): void;
}

export class MemoryStateStore implements StateStore {
  private data: BotStateData | null = null;
  load(): BotStateData | null {
    return this.data ? structuredClone(this.data) : null;
  }
  save(state: BotStateData): void {
    this.data = structuredClone(state);
  }
}

/** JSON file, written atomically (temp file + rename), mode 600. */
export class FileStateStore implements StateStore {
  readonly path: string;

  /** A leading `~` in `path` is the home folder. */
  constructor(path: string) {
    this.path = expandHome(path);
  }

  load(): BotStateData | null {
    if (!existsSync(this.path)) return null;
    // Deleting the file also drops the cursors, the followed topics and the remembered cards.
    const hint = "delete it to start fresh (events from while the bot was away are skipped)";
    let raw: Partial<BotStateData>;
    try {
      raw = JSON.parse(readFileSync(this.path, "utf8")) as Partial<BotStateData>;
    } catch (err) {
      throw new ChatError("invalid", `bot state file is not valid JSON: ${this.path} (${hint}): ${(err as Error).message}`);
    }
    if (!raw || typeof raw !== "object") throw new ChatError("invalid", `bot state file is not a JSON object: ${this.path} (${hint})`);
    if (raw.v !== 1) throw new ChatError("invalid", `unsupported bot state version in ${this.path}`);
    return {
      v: 1,
      memberId: typeof raw.memberId === "string" ? raw.memberId : undefined,
      cursors: raw.cursors && typeof raw.cursors === "object" ? raw.cursors : {},
      following: raw.following && typeof raw.following === "object" ? raw.following : {},
      cards: raw.cards && typeof raw.cards === "object" ? raw.cards : {},
      artifacts: raw.artifacts && typeof raw.artifacts === "object" ? raw.artifacts : {},
      asides: raw.asides && typeof raw.asides === "object" ? raw.asides : {},
      asideScan: raw.asideScan && typeof raw.asideScan === "object" ? raw.asideScan : {},
    };
  }

  save(state: BotStateData): void {
    mkdirSync(dirname(this.path), { recursive: true, mode: 0o700 });
    const tmp = `${this.path}.${process.pid}.tmp`;
    writeFileSync(tmp, `${JSON.stringify(state)}\n`, { mode: 0o600 });
    chmodSync(tmp, 0o600);
    renameSync(tmp, this.path);
  }
}
