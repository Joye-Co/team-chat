// 每天整理 (agent-harness.zh.md 7.3, like Claude's dreaming): once a day after 03:00 in the
// team's time zone (the cron fires hourly; a failed scope is tried again the next hour), each
// scope with pending memories is consolidated once. With an AI service one
// forced tool call returns operations (add, merge, retire, keep, promote); without one, pending
// memories become active as they are. Memories a person wrote are never changed, only flagged.
// Every change is a 记忆更新 line of the day and a new version (by null). A memory learned in a
// group (`from_group`) never reaches a wider audience by itself: it merges only with memories of
// the same group, other members' lessons shown to the model are unlimited ones only, and a
// promotion with such a source is only proposed (a flagged line) for a person to decide.
import { MemoryKind, type MemoryUpdate } from "@chat/protocol";
import { z } from "zod";
import { loadModelConfig } from "../aiservice";
import type { CallOptions, ModelConfig } from "../aiservice/providers";
import { type ToolSpec, callTool } from "../aiservice/tools";
import type { Env } from "../core/env";
import type { Logger } from "../core/logger";
import { type MemoryChange, type MemoryRow, changed, cleanNote, insertStatements, runBatch, saveStatements, updateStatement } from "./store";

export const DEFAULT_TEAM_ZONE = "Asia/Shanghai";
/** The consolidation runs at the first (hourly) cron at or after this local hour. */
export const CONSOLIDATE_HOUR = 3;
/** AI-written active memories unused this long are retired. */
export const STALE_MS = 60 * 24 * 60 * 60_000;
/** memory_runs scope of the stale sweep. */
const STALE_RUN = "~stale";

/** Notes the server writes on 记忆更新 lines and memories (content, shown as is). */
export const NOTE = {
  stale: "60 天没用到，停用了",
  mergedInto: (id: string) => `并入了 ${id}`,
  promotedFrom: (ids: string[]) => `从这几条升为团队约定：${ids.join("、")}`,
  promoteHeld: (text: string) => `来自群里或私聊的经验，没有自动升为团队约定，请决定要不要写进团队约定：${text}`,
  undo: "撤回了",
} as const;

/** The team's time zone: `TEAM_TIME_ZONE` when it is a known zone, else Asia/Shanghai. */
export function teamZone(env: Env): string {
  const zone = (env as Env & { TEAM_TIME_ZONE?: string }).TEAM_TIME_ZONE;
  if (zone) {
    try {
      new Intl.DateTimeFormat("en-US", { timeZone: zone });
      return zone;
    } catch {
      // unknown zone: the default
    }
  }
  return DEFAULT_TEAM_ZONE;
}

/** The local day (`YYYY-MM-DD`), hour, and the epoch ms of that day's 03:00. */
export function localClock(now: number, zone: string): { day: string; hour: number; cutoff: number } {
  const parts = Object.fromEntries(
    new Intl.DateTimeFormat("en-CA", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" })
      .formatToParts(new Date(now))
      .map((p) => [p.type, p.value]),
  ) as Record<string, string>;
  const [y, mo, d, h, mi, s] = [parts.year, parts.month, parts.day, parts.hour, parts.minute, parts.second].map(Number);
  const offset = Date.UTC(y, mo - 1, d, h % 24, mi, s) - Math.floor(now / 1000) * 1000;
  return { day: `${parts.year}-${parts.month}-${parts.day}`, hour: h % 24, cutoff: Date.UTC(y, mo - 1, d, CONSOLIDATE_HOUR) - offset };
}

const Op = z.object({
  op: z.enum(["add", "merge", "retire", "keep", "promote"]),
  id: z.string().optional(),
  into: z.string().optional(),
  text: z.string().optional(),
  when: z.string().optional(),
  kind: MemoryKind.optional(),
  reason: z.string().optional(),
  sources: z.array(z.string()).max(20).optional(),
});
type Op = z.infer<typeof Op>;

export const CONSOLIDATE_TOOL: ToolSpec = {
  name: "consolidate",
  description: "The operations that turn today's pending notes into the scope's memory.",
  schema: {
    type: "object",
    properties: {
      ops: {
        type: "array",
        items: {
          type: "object",
          properties: {
            op: { type: "string", enum: ["add", "merge", "retire", "keep", "promote"] },
            id: { type: "string", description: "add, retire, keep, merge: the memory's id" },
            into: { type: "string", description: "merge: the active memory the pending one goes into" },
            text: { type: "string", description: "add (optional rewording), merge (the merged text), promote (the team convention). At most 500 characters." },
            when: { type: "string", description: "When it applies (optional). At most 300 characters." },
            kind: { type: "string", enum: MemoryKind.options, description: "add: the kind, when it should change" },
            reason: { type: "string", description: "retire and merge: why" },
            sources: { type: "array", items: { type: "string" }, description: "promote: the ids of the memories it comes from, of at least two members" },
          },
          required: ["op"],
        },
      },
    },
    required: ["ops"],
  },
};

function describe(r: MemoryRow, label?: string): string {
  const flags = [r.kind, r.status, ...(r.human ? ["written by a person: never change"] : []), ...(label ? [label] : [])];
  return `- [${r.id}] (${flags.join(", ")}) ${r.text.replace(/\s+/g, " ")}${r.when_text ? ` (when: ${r.when_text})` : ""}${r.note ? ` (note: ${r.note.replace(/\s+/g, " ")})` : ""}`;
}

interface Ctx {
  db: D1Database;
  day: string;
  now: number;
  log: Logger | undefined;
  ops: number;
}

/** Saves a change as a new version (by the consolidation) with its 记忆更新 line. */
async function apply(c: Ctx, prev: MemoryRow, change: MemoryChange, line: { change: MemoryUpdate["change"]; note?: string | null } | null): Promise<MemoryRow> {
  const next = changed(prev, change, c.now);
  if (!next) return prev;
  const stmts = saveStatements(c.db, prev, next, null, c.now);
  if (line) {
    stmts.push(
      updateStatement(c.db, { day: c.day, scope: next.scope, memoryId: next.id, change: line.change, before: line.change === "added" ? null : prev, after: next, note: line.note }, c.now),
    );
    c.ops++;
  }
  await runBatch(c.db, stmts);
  return next;
}

/** A person's memory: the consolidation only mentions it. */
async function flag(c: Ctx, row: MemoryRow, note: string | undefined): Promise<void> {
  await c.db.batch([updateStatement(c.db, { day: c.day, scope: row.scope, memoryId: row.id, change: "flagged", before: row, after: row, note: note ?? null }, c.now)]);
  c.ops++;
}

const activate = (c: Ctx, p: MemoryRow, change: MemoryChange = {}) => apply(c, p, { ...change, status: "active" }, { change: "added" });

/** Consolidates one scope with the model; throws when the model gives no usable answer. */
async function withModel(c: Ctx, cfg: ModelConfig, scope: string, pending: MemoryRow[], opts: CallOptions): Promise<void> {
  const active = (await c.db.prepare("SELECT * FROM memories WHERE scope = ? AND status = 'active' ORDER BY human DESC, uses DESC, updated_at DESC LIMIT 200").bind(scope).all<MemoryRow>()).results;
  const memberScope = scope.startsWith("member:");
  const others = memberScope
    ? (
        await c.db
          // Only lessons without a source group: one learned in a group must not reach this scope's text.
          .prepare(
            "SELECT * FROM memories WHERE scope LIKE 'member:%' AND scope != ? AND status IN ('active', 'pending') AND kind IN ('lesson', 'playbook') AND from_group IS NULL ORDER BY updated_at DESC LIMIT 100",
          )
          .bind(scope)
          .all<MemoryRow>()
      ).results
    : [];
  const team = memberScope ? (await c.db.prepare("SELECT * FROM memories WHERE scope = 'team' AND status = 'active' ORDER BY updated_at DESC LIMIT 100").all<MemoryRow>()).results : [];
  const system = [
    "You consolidate the memory of a team chat once a day, like sleep consolidates the day's notes. Memory is reference material the team's AI members read before they work.",
    `Scope: ${scope}. Decide what happens to each pending note with the \`consolidate\` tool:`,
    "- add: keep it as a memory (reword it to be short and general when that helps);",
    "- merge: it repeats or refines an active memory: give the merged text and the id it goes `into`;",
    "- retire: a pending note that is wrong, one-off or not worth keeping, or an active memory a newer note shows is out of date (give the reason);",
    "- keep: an active memory stays as it is;",
    ...(memberScope ? ["- promote: the same lesson shows up for two or more members: a team convention with the ids of its `sources`."] : []),
    "Memories written by a person are never changed: when one looks out of date, retire it with the reason and a person will look. When a person's latest words disagree with an older note, the person wins.",
    "Keep the memory small: fewer, general memories are better than many specific ones. Write in the language of the notes.",
  ].join("\n");
  const prompt = [
    "Active memories of this scope:",
    ...(active.length > 0 ? active.map((r) => describe(r)) : ["(none)"]),
    "",
    "Pending notes of this scope (since the last consolidation):",
    ...pending.map((r) => describe(r)),
    ...(others.length > 0 ? ["", "Lessons of other members (for promote only):", ...others.map((r) => describe(r, r.scope))] : []),
    ...(team.length > 0 ? ["", "The team's conventions (do not promote what is already here):", ...team.map((r) => describe(r))] : []),
  ].join("\n");
  const res = await callTool(cfg, { system, prompt, tool: CONSOLIDATE_TOOL, maxTokens: 4000 }, opts);
  if (!res.ok) throw new Error(`model error: ${res.error}`);
  const raw = (res.input as { ops?: unknown })?.ops;
  if (!Array.isArray(raw)) throw new Error("no ops");
  const ops = raw.flatMap((o) => {
    const p = Op.safeParse(o);
    return p.success ? [p.data] : [];
  });

  const pend = new Map(pending.map((r) => [r.id, r]));
  const act = new Map(active.map((r) => [r.id, r]));
  const all = new Map([...active, ...pending, ...others].map((r) => [r.id, r]));
  const done = new Set<string>();
  for (const op of ops) {
    try {
      await runOp(c, op, scope, pend, act, all, team, done);
    } catch (err) {
      c.log?.warn("memory op failed", { scope, op: op.op, err });
    }
  }
  // A pending note no operation named becomes active as it is.
  for (const p of pending) if (!done.has(p.id)) await activate(c, p);
}

async function runOp(
  c: Ctx,
  op: Op,
  scope: string,
  pend: Map<string, MemoryRow>,
  act: Map<string, MemoryRow>,
  all: Map<string, MemoryRow>,
  team: MemoryRow[],
  done: Set<string>,
): Promise<void> {
  const p = op.id && !done.has(op.id) ? pend.get(op.id) : undefined;
  switch (op.op) {
    case "add":
      if (!p) return;
      done.add(p.id);
      await activate(c, p, { ...(op.text?.trim() ? { text: op.text } : {}), ...(op.when?.trim() ? { when: op.when } : {}), ...(op.kind ? { kind: op.kind } : {}) });
      return;
    case "merge": {
      const into = op.into ? act.get(op.into) : undefined;
      // Only within one audience: a note of one group does not go into a memory others read.
      if (!p || !into || (p.from_group ?? null) !== (into.from_group ?? null)) return;
      done.add(p.id);
      if (into.human) await flag(c, into, op.reason ?? p.text);
      else {
        const next = await apply(c, into, { ...(op.text?.trim() ? { text: op.text } : {}), ...(op.when?.trim() ? { when: op.when } : {}) }, { change: "changed", note: op.reason ?? NOTE.mergedInto(into.id) });
        act.set(next.id, next);
      }
      await apply(c, p, { status: "retired", note: NOTE.mergedInto(into.id) }, null);
      return;
    }
    case "retire": {
      if (p) {
        done.add(p.id);
        await apply(c, p, { status: "retired", note: cleanNote(op.reason) }, null);
        return;
      }
      const a = op.id ? act.get(op.id) : undefined;
      if (!a) return;
      act.delete(a.id);
      if (a.human) await flag(c, a, op.reason);
      else await apply(c, a, { status: "retired", note: cleanNote(op.reason) ?? a.note }, { change: "retired", note: op.reason });
      return;
    }
    case "keep":
      return;
    case "promote": {
      if (!scope.startsWith("member:") || !op.text?.trim()) return;
      const sources = [...new Set(op.sources ?? [])].map((id) => all.get(id)).filter((r): r is MemoryRow => r !== undefined && r.scope.startsWith("member:"));
      // The same lesson on two members, not just one member saying it twice.
      if (new Set(sources.map((r) => r.scope)).size < 2) return;
      const text = op.text.trim();
      if (team.some((t) => t.text.trim() === text)) return;
      // Learned in a group: the whole team would read it. A person decides.
      const limited = sources.find((r) => r.from_group);
      if (limited) {
        await flag(c, limited, NOTE.promoteHeld(text));
        return;
      }
      const srcIds = [...new Set(sources.flatMap((r) => (JSON.parse(r.source) as { ids: string[] }).ids))].slice(0, 20);
      const firstKind = (JSON.parse(sources[0].source) as { kind: "message" | "run" | "card" | "manual" }).kind;
      const { row, stmts } = insertStatements(
        c.db,
        {
          scope: "team",
          kind: "convention",
          text,
          when: op.when ?? null,
          note: NOTE.promotedFrom(sources.map((r) => r.id)),
          source: { kind: firstKind, ids: srcIds },
          authorId: scope.slice("member:".length),
          human: false,
          status: "active",
          confidence: 0.7,
        },
        null,
        c.now,
      );
      stmts.push(updateStatement(c.db, { day: c.day, scope: "team", memoryId: row.id, change: "added", before: null, after: row, note: row.note }, c.now));
      await c.db.batch(stmts);
      team.push(row);
      c.ops++;
      return;
    }
  }
}

/** Retires AI-written memories nobody used for 60 days. */
async function sweepStale(c: Ctx): Promise<void> {
  const stale = (
    await c.db
      .prepare("SELECT * FROM memories WHERE status = 'active' AND human = 0 AND coalesce(last_used_at, updated_at) < ? LIMIT 200")
      .bind(c.now - STALE_MS)
      .all<MemoryRow>()
  ).results;
  for (const r of stale) await apply(c, r, { status: "retired", note: NOTE.stale }, { change: "retired", note: NOTE.stale });
}

async function claim(db: D1Database, day: string, scope: string, now: number): Promise<boolean> {
  const res = await db.prepare("INSERT OR IGNORE INTO memory_runs (day, scope, at) VALUES (?, ?, ?)").bind(day, scope, now).run();
  return res.meta.changes > 0;
}

export interface ConsolidateResult {
  day: string;
  /** Scopes consolidated by this call. */
  scopes: string[];
  /** 记忆更新 lines written. */
  ops: number;
}

/**
 * The daily consolidation. Does nothing before 03:00 local time; each scope runs once a day
 * (a failed model call frees the scope for the next hourly trigger the same day).
 */
export async function consolidateAll(env: Env, opts: { now?: number; log?: Logger; call?: CallOptions } = {}): Promise<ConsolidateResult> {
  const now = opts.now ?? Date.now();
  const { day, hour, cutoff } = localClock(now, teamZone(env));
  const out: ConsolidateResult = { day, scopes: [], ops: 0 };
  if (hour < CONSOLIDATE_HOUR) return out;
  const db = env.DB;
  const scopes = (await db.prepare("SELECT DISTINCT scope FROM memories WHERE status = 'pending' AND created_at < ? ORDER BY scope").bind(cutoff).all<{ scope: string }>()).results.map((r) => r.scope);
  const cfg = scopes.length > 0 ? await loadModelConfig(env).catch(() => null) : null;
  for (const scope of scopes) {
    if (!(await claim(db, day, scope, now))) continue;
    const c: Ctx = { db, day, now, log: opts.log, ops: 0 };
    try {
      const pending = (await db.prepare("SELECT * FROM memories WHERE scope = ? AND status = 'pending' AND created_at < ? ORDER BY created_at LIMIT 100").bind(scope, cutoff).all<MemoryRow>()).results;
      if (cfg) await withModel(c, cfg, scope, pending, opts.call ?? {});
      else for (const p of pending) await activate(c, p);
      await db.prepare("UPDATE memory_runs SET ops = ? WHERE day = ? AND scope = ?").bind(c.ops, day, scope).run();
      out.scopes.push(scope);
      out.ops += c.ops;
    } catch (err) {
      opts.log?.warn("memory consolidation failed", { scope, err });
      out.ops += c.ops;
      // Nothing (or only part) was done: the next hourly trigger today tries the rest again.
      await db.prepare("DELETE FROM memory_runs WHERE day = ? AND scope = ?").bind(day, scope).run();
    }
  }
  if (await claim(db, day, STALE_RUN, now)) {
    const c: Ctx = { db, day, now, log: opts.log, ops: 0 };
    await sweepStale(c);
    out.ops += c.ops;
  }
  return out;
}
