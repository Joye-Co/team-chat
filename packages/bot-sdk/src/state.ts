// What a bot remembers across restarts: the last handled seq per group, the topics it
// follows, its own card messages and 产出 (so card presses, answers and page events reach it
// after a restart), and its side chat topics with the topic each one is about.
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

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

  constructor(path: string) {
    this.path = path;
  }

  load(): BotStateData | null {
    if (!existsSync(this.path)) return null;
    const raw = JSON.parse(readFileSync(this.path, "utf8")) as Partial<BotStateData>;
    if (raw.v !== 1) throw new Error(`unsupported bot state version in ${this.path}`);
    return {
      v: 1,
      memberId: typeof raw.memberId === "string" ? raw.memberId : undefined,
      cursors: raw.cursors && typeof raw.cursors === "object" ? raw.cursors : {},
      following: raw.following && typeof raw.following === "object" ? raw.following : {},
      cards: raw.cards && typeof raw.cards === "object" ? raw.cards : {},
      artifacts: raw.artifacts && typeof raw.artifacts === "object" ? raw.artifacts : {},
      asides: raw.asides && typeof raw.asides === "object" ? raw.asides : {},
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
