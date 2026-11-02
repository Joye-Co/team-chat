// What a bot remembers across restarts: the last handled seq per group, the topics it
// follows, and its own card messages (so card presses reach it after a restart).
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export interface CardRef {
  groupId: string;
  topicId: string;
}

export interface BotStateData {
  v: 1;
  memberId?: string;
  cursors: Record<string, number>;
  following: Record<string, string[]>;
  cards: Record<string, CardRef>;
}

/** At most this many own card messages are remembered (oldest are dropped). */
export const MAX_REMEMBERED_CARDS = 2000;

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
