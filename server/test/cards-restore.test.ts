// Restore of Card v2 state from the archive (PC-1): private cards, revs, refs, ask timers, the
// topic base of a blocking ask and the state outside the log (card events, requesters, page data).
import { env, runInDurableObject } from "cloudflare:test";
import { afterAll, describe, expect, it } from "vitest";
import type { CardV2 } from "@chat/protocol";
import { ulid } from "../src/core/ids";
import { GROUP_PREFIX, groupStateKey, parseGroupFileKey, runArchive, utcDate } from "../src/ops/archive";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
});

const ADMIN = { "x-admin-secret": "test-admin-secret" };
const FEATURES = ["interact/1", "refs/1"];
const roomOf = (gid: string) => env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(gid));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const base = (extra: Partial<CardV2>): CardV2 => ({
  v: 2,
  catalog: "tc.card/2.1",
  rev: 1,
  intent: "info",
  title: "卡片",
  fallback: "卡片",
  blocks: [],
  ...extra,
});

const TABLES = ["card_private", "card_revs", "block_refs", "card_timers", "card_topic_base", "card_events", "card_requesters", "artifact_data", "recorded_decisions"];

async function cardTables(gid: string) {
  return runInDurableObject(roomOf(gid), (_o, state) => {
    const out: Record<string, unknown[]> = {};
    for (const t of TABLES) out[t] = state.storage.sql.exec(`SELECT * FROM ${t} ORDER BY 1, 2`).toArray();
    return out;
  });
}

async function emptyGroupLike(memberIds: string[]): Promise<string> {
  const id = ulid();
  const now = Date.now();
  await env.DB.batch([
    env.DB.prepare("INSERT INTO groups (id, kind, name, created_by, created_at) VALUES (?, 'group', 'restored', ?, ?)").bind(id, memberIds[0], now),
    ...memberIds.map((m) => env.DB.prepare("INSERT INTO group_members (group_id, member_id, added_at) VALUES (?, ?, ?)").bind(id, m, now)),
  ]);
  return id;
}

async function restore(gid: string, target: string): Promise<void> {
  const list = await env.FILES.list({ prefix: `${GROUP_PREFIX}${gid}/` });
  const keys = list.objects.map((o) => o.key).filter((k) => parseGroupFileKey(k)).sort();
  for (const key of keys) {
    const bytes = new Uint8Array(await (await env.FILES.get(key))!.arrayBuffer());
    const r = await api(`/v1/admin/import/groups/${target}`, { method: "POST", headers: ADMIN, raw: bytes as unknown as string });
    expect(r.status, JSON.stringify(r.body)).toBe(200);
  }
  const state = new Uint8Array(await (await env.FILES.get(groupStateKey(gid, utcDate(Date.now()))))!.arrayBuffer());
  const st = await api(`/v1/admin/import/groups/${target}/state`, { method: "POST", headers: ADMIN, raw: state as unknown as string });
  expect(st.status, JSON.stringify(st.body)).toBe(200);
}

describe("Card v2 state after a restore", () => {
  it("keeps private answers hidden, refs, revs, page data and expires a timed ask", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const alice = await createMember({ name: "Alice" });
    const bob = await createMember({ name: "Bob" });
    const bot = await createMember({ kind: "bot", name: "Ledger" });
    const gid = await createGroup(owner, [alice.id, bob.id, bot.id]);
    const [ca, cb, cbot] = await Promise.all([alice, bob, bot].map(async (m) => (await Client.connect(m.token, { features: FEATURES })).c));

    // An anonymous questionnaire and a private poll, answered by two members.
    const survey = base({
      intent: "ask",
      title: "实习反馈",
      ask: { from: [alice.id, bob.id], blocking: false },
      blocks: [
        { id: "q", type: "questionnaire", children: ["mood", "next"], anonymous: true, fallbackText: "实习反馈" },
        { id: "mood", type: "input", kind: "text", label: "感受" },
        { id: "next", type: "input", kind: "text", label: "建议" },
      ],
    });
    const s = (await cbot.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "", cardV2: survey })).message;
    const topicId = s.topicId as string;
    const ans = await ca.request({ t: "card.event", groupId: gid, messageId: s.id, rev: 1, clientEventId: crypto.randomUUID(), blockId: "q", event: "action.press", inputs: { mood: { value: "还行" } } });
    expect(ans, JSON.stringify(ans)).toMatchObject({ ok: true, result: { stored: true } });
    const poll = base({
      intent: "ask",
      title: "周五去哪？",
      ask: { from: ["owners"], blocking: false },
      blocks: [
        {
          id: "where",
          type: "poll",
          label: "去哪",
          options: [{ id: "a", label: "爬山" }, { id: "b", label: "看电影" }],
          voters: [alice.id, bob.id],
          closesAt: Date.now() + 3600_000,
          fallbackText: "投票",
        },
      ],
    });
    const p = (await cbot.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "", cardV2: poll })).message;
    for (const c of [ca, cb]) {
      const r = await c.request({ t: "card.event", groupId: gid, messageId: p.id, rev: 1, clientEventId: crypto.randomUUID(), blockId: "where", event: "choice.submit", value: { value: "a" } });
      expect(r, JSON.stringify(r)).toMatchObject({ ok: true, result: { stored: true } });
    }
    // A blocking timed ask, and a question that quotes a part of the poll.
    const timed = base({
      intent: "ask",
      title: "今天发版吗？",
      ask: { from: [alice.id], expiresAt: Date.now() + 4000 },
      blocks: [{ id: "pick", type: "decision", deliver: "immediate", options: [{ id: "yes", label: "发" }, { id: "no", label: "不发" }] }],
    });
    const t = (await cbot.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "", cardV2: timed })).message;
    const ref = { messageId: p.id, cardRev: 1, blockId: "where", label: "投票" };
    const q = (await cb.ok({ t: "send", groupId: gid, topicId, clientMsgId: clientMsgId(), body: "这个准吗？", blockRefs: [ref] })).message;
    // Page data of a 产出 (not in the log).
    await runInDurableObject(roomOf(gid), (_o, state) => {
      state.storage.sql.exec(
        "INSERT INTO artifact_data (artifact_id, version, data, data_rev, push_id, updated_at) VALUES (?, 1, ?, 3, NULL, ?)",
        ulid(),
        JSON.stringify({ rows: [1, 2] }),
        Date.now(),
      );
    });

    // A decision recorded by hand (记一条) is in the log.
    const rec = await api(`/v1/groups/${gid}/decisions`, { token: alice.token, body: { text: "周五停止合并", topicId } });
    expect(rec.status, JSON.stringify(rec.body)).toBe(200);
    await runArchive(env, { skipD1: true });
    const original = await cardTables(gid);
    expect(original.recorded_decisions).toHaveLength(1);
    expect(original.card_private).toHaveLength(2);
    expect(original.card_timers.length).toBeGreaterThan(0);
    expect(original.card_topic_base).toHaveLength(1);
    const target = await emptyGroupLike([owner.id, alice.id, bob.id, bot.id]);
    await restore(gid, target);
    expect(await cardTables(target)).toEqual(original);

    // Bob's history of the restored group names no voter of the private cards.
    const history = await cb.request({ t: "history", groupId: target, topicId });
    const sv = history.messages.find((m: any) => m.id === s.id);
    const pv = history.messages.find((m: any) => m.id === p.id);
    expect(sv.cardState.answers).toEqual([]);
    expect(pv.cardState.tallyBy).toBeUndefined();
    expect(pv.cardState.answers.map((a: any) => a.actorId)).toEqual([bob.id]);
    expect(JSON.stringify(history.messages)).not.toContain("还行");
    expect(history.messages.find((m: any) => m.id === q.id).blockRefs[0]).toMatchObject({ messageId: p.id, label: "投票" });

    // The timed ask still expires in the restored room.
    let expired: any;
    for (let i = 0; i < 100 && !expired; i++) {
      const r = await cb.request({ t: "history", groupId: target, topicId });
      const m = r.messages.find((x: any) => x.id === t.id);
      if (m?.cardState.ask.status === "expired") expired = m;
      else await sleep(100);
    }
    expect(expired.cardState.ask.status).toBe("expired");
    await sleep(100);
    for (const c of [ca, cb, cbot]) c.close();
  }, 30_000);
});
