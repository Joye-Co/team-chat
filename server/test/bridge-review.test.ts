// Review fixes of the 1.1 产出 bridge and the chat-run outputs: bot patterns run without
// backtracking (SEC-4), page emit events are resolved like card events (F2), a page event row
// exists before the event is delivered (F3), output keys are scoped to the run (daemon F1), and
// the age of an output is measured on the server clock (daemon F3).
import { SELF, env, runInDurableObject } from "cloudflare:test";
import type { CardV2 } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import type { Actor } from "../src/core/registry";
import type { GroupRoom } from "../src/core/room";
import { ARTIFACT_EVENT_RPC, ARTIFACT_REPLY_RPC, tcSchemaViolation } from "../src/files/bridge";
import { Daemon, downViolations, HEALTH, mention, world, type World } from "./daemon";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

const benv = env as unknown as BotsEnv;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const INTERACT = { features: ["interact/1"] };

afterAll(async () => {
  await sleep(300);
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

async function sha(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function upload(m: TestMember, groupId: string): Promise<string> {
  const bytes = new TextEncoder().encode("<!doctype html><p>hi</p>");
  const c = await api("/v1/files", { token: m.token, body: { groupId, name: "index.html", size: bytes.length, mime: "text/html", sha256: await sha(bytes) } });
  expect(c.status, JSON.stringify(c.body)).toBe(200);
  const put = await SELF.fetch(`${BASE}/v1/files/${c.body.file.id}/content`, { method: "PUT", headers: { authorization: `Bearer ${m.token}` }, body: bytes });
  expect(put.status).toBe(200);
  return c.body.file.id as string;
}

async function makePage(bot: TestMember, groupId: string, topicId: string, events: unknown[], runId?: string) {
  return api("/v1/artifacts", {
    token: bot.token,
    body: { groupId, topicId, title: "看板", kind: "html", entry: "index.html", files: [{ path: "index.html", fileId: await upload(bot, groupId) }], events, ...(runId ? { runId } : {}) },
  });
}

const EVENTS = [
  { name: "report.problem", label: "数据有问题" },
  { name: "ask.total", kind: "ask" },
];

/** A group with a person and an SDK bot, a topic, and a page of the bot. */
async function pageWorld() {
  const owner = await createMember({ level: "owner", name: "Joye" });
  const bot = await createMember({ kind: "bot", name: "ledger" });
  const gid = await createGroup(owner, [bot.id]);
  const { c: co } = await Client.connect(owner.token, INTERACT);
  const { c: cb } = await Client.connect(bot.token, INTERACT);
  const topicId = (await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "做个看板" })).topic.id as string;
  const made = await makePage(bot, gid, topicId, EVENTS);
  expect(made.status, JSON.stringify(made.body)).toBe(200);
  return { owner, bot, gid, co, cb, topicId, artifactId: made.body.artifact.id as string };
}

describe("TcSchema pattern (SEC-4)", () => {
  it("match patterns without backtracking, so a nested quantifier cannot stall the room", () => {
    const schema = { type: "string", pattern: "^(a+)+$" };
    const evil = `${"a".repeat(30)}b`;
    const t0 = Date.now();
    expect(tcSchemaViolation(schema, evil)).toBe("payload does not match the pattern");
    expect(tcSchemaViolation({ type: "string", pattern: "^(a|a)*$" }, `${"a".repeat(4000)}b`)).toBe("payload does not match the pattern");
    expect(Date.now() - t0).toBeLessThan(500);
    expect(tcSchemaViolation(schema, "aaaa")).toBeUndefined();
    // The usual patterns keep their meaning.
    const month = { type: "string", pattern: "^[0-9]{4}-[0-9]{2}$" };
    expect(tcSchemaViolation(month, "2026-10")).toBeUndefined();
    expect(tcSchemaViolation(month, "oct")).toBe("payload does not match the pattern");
    expect(tcSchemaViolation({ pattern: "x|y\\d{2,3}" }, "ay123")).toBeUndefined();
    expect(tcSchemaViolation({ pattern: "^(?:抖音|小红书)$" }, "抖音")).toBeUndefined();
    expect(tcSchemaViolation({ pattern: "^[^\\s]+$" }, "a b")).toBe("payload does not match the pattern");
    expect(tcSchemaViolation({ pattern: "^\\w+\\.json$" }, "data.json")).toBeUndefined();
    expect(tcSchemaViolation({ pattern: "^.{2}$" }, "抖音")).toBeUndefined();
    expect(tcSchemaViolation({ pattern: "\\bv\\d\\b" }, "a v2 b")).toBeUndefined();
    expect(tcSchemaViolation({ pattern: "\\bv\\d\\b" }, "av2")).toBe("payload does not match the pattern");
    expect(tcSchemaViolation({ pattern: "^(?<y>[0-9]{4})-\\p{Nd}+$" }, "2026-10")).toBeUndefined();
    expect(tcSchemaViolation({ pattern: "^a{2,3}?$" }, "aaaa")).toBe("payload does not match the pattern");
    expect(tcSchemaViolation({ pattern: "^(|x)*y$" }, "xxy")).toBeUndefined();
    // Long strings are refused before matching.
    expect(tcSchemaViolation({ pattern: "a" }, "a".repeat(5000))).toBe("payload is too long for the pattern");
  });

  it("refuse a declared pattern that needs backtracking (backreferences, lookaround) when the version is made", async () => {
    const owner = await createMember({ level: "owner", name: "Joye" });
    const bot = await createMember({ kind: "bot", name: "ledger" });
    const gid = await createGroup(owner, [bot.id]);
    const { c: co } = await Client.connect(owner.token);
    const topicId = (await co.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "做个看板" })).topic.id as string;
    for (const pattern of ["^(a)\\1$", "^(?<y>a)\\k<y>$", "^(?=a)a$", "^a(?!b)", "(?<=a)b", "(?<!a)b", "^(a"]) {
      const r = await makePage(bot, gid, topicId, [{ name: "pick", schema: { type: "string", pattern } }]);
      expect(r.status, pattern).toBe(400);
      expect(r.body.error, JSON.stringify(r.body)).toMatchObject({ code: "invalid", reason: "artifact.bad_schema" });
    }
    const ok = await makePage(bot, gid, topicId, [{ name: "pick", schema: { type: "object", properties: { v: { type: "string", pattern: "^(a+)+$" } } } }]);
    expect(ok.status, JSON.stringify(ok.body)).toBe(200);
    co.close();
  });
});

describe("page emit events are resolved like card events (F2)", () => {
  it("take interaction.resolve, send.causeEventId and artifact.data cause for a page event, once, from its receiver only", async () => {
    const w = await pageWorld();
    const emit = () => w.co.ok({ t: "artifact.event", groupId: w.gid, artifactId: w.artifactId, version: 1, name: "report.problem", payload: { channel: "douyin" } });
    const resolvedFor = (eventId: string) => w.co.next((f) => f.t === "transient" && f.event.type === "interaction.resolved" && f.event.eventId === eventId);

    // interaction.resolve: the receiver only, once.
    const e1 = await emit();
    expect(await w.co.request({ t: "interaction.resolve", groupId: w.gid, eventId: e1.eventId, result: "ignored" })).toMatchObject({ ok: false, error: { code: "forbidden" } });
    expect(await w.cb.request({ t: "interaction.resolve", groupId: w.gid, eventId: e1.eventId, result: "ignored", note: "没有这个渠道" })).toMatchObject({ ok: true });
    expect((await resolvedFor(e1.eventId)).event).toMatchObject({ result: "ignored", note: "没有这个渠道" });
    expect(await w.cb.request({ t: "interaction.resolve", groupId: w.gid, eventId: e1.eventId, result: "failed" })).toMatchObject({ ok: false, error: { code: "conflict" } });

    // send with causeEventId: the message stores the cause, and the emitter's pending clears.
    const e2 = await emit();
    const sent = await w.cb.ok({ t: "send", groupId: w.gid, topicId: w.topicId, clientMsgId: clientMsgId(), body: "看板里抖音的数改好了", causeEventId: e2.eventId });
    expect(sent.message.cause).toMatchObject({ eventId: e2.eventId, actorId: w.owner.id, event: "report.problem" });
    expect((await resolvedFor(e2.eventId)).event).toMatchObject({ result: "posted", messageId: sent.message.id });

    // A card patch may name it too.
    const card: CardV2 = { v: 2, rev: 1, intent: "info", title: "10 月数据月报", fallback: "10 月数据", blocks: [{ type: "text", id: "t", text: "抖音 6" }] } as CardV2;
    const posted = await w.cb.ok({ t: "send", groupId: w.gid, topicId: w.topicId, clientMsgId: clientMsgId(), body: "", cardV2: card });
    const e3 = await emit();
    const patched = await w.cb.ok({ t: "card.patch", groupId: w.gid, messageId: posted.message.id, baseRev: 1, cause: e3.eventId, ops: [{ op: "card", props: { title: "10 月数据月报（改）" } }] });
    expect(patched).toMatchObject({ rev: 2 });
    expect((await resolvedFor(e3.eventId)).event).toMatchObject({ result: "patched", messageId: posted.message.id });

    // A data push that names it clears it with "replied".
    const e4 = await emit();
    expect(await w.cb.ok({ t: "artifact.data", groupId: w.gid, artifactId: w.artifactId, version: 1, data: { douyin: 7 }, cause: e4.eventId })).toMatchObject({ dataRev: 1 });
    expect((await resolvedFor(e4.eventId)).event).toMatchObject({ result: "replied" });
    // The event was resolved by the push.
    expect(await w.cb.request({ t: "interaction.resolve", groupId: w.gid, eventId: e4.eventId, result: "ignored" })).toMatchObject({ ok: false, error: { code: "conflict" } });

    // Another member cannot name the event as its cause.
    const other = await createMember({ kind: "bot", name: "other" });
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: other.id } });
    const { c: cOther } = await Client.connect(other.token, INTERACT);
    const e5 = await emit();
    expect(await cOther.request({ t: "send", groupId: w.gid, topicId: w.topicId, clientMsgId: clientMsgId(), body: "x", causeEventId: e5.eventId })).toMatchObject({
      ok: false,
      error: { code: "invalid", reason: "interact.bad_cause" },
    });
    for (const x of [w.co, w.cb, cOther]) x.close();
  });
});

describe("a page event row exists before delivery (F3)", () => {
  it("take a reply and a repeat that come while the room still waits for the receiver's presence", async () => {
    const w = await pageWorld();
    w.cb.close();
    const owner: Actor = { id: w.owner.id, level: "owner", kind: "human" };
    const bot: Actor = { id: w.bot.id, level: "member", kind: "bot" };
    const args = { artifactId: w.artifactId, version: 1, name: "ask.total", kind: "ask", clientEventId: "fast-1" };
    const seen: { reply?: unknown; repeat?: unknown } = {};
    const rooms = (env as unknown as { GROUP_ROOM: DurableObjectNamespace<GroupRoom> }).GROUP_ROOM;
    let ack: any;
    await runInDurableObject(rooms.get(rooms.idFromName(w.gid)), async (room: GroupRoom): Promise<void> => {
      const instance = room as any;
      const orig = instance.env;
      const frames: any[] = [];
      const ns = orig.MEMBER_GATEWAY;
      // The bot's gateway: it takes the transient, and while the room waits for its presence the
      // bot answers at once and the page sends the same event again.
      const fake = {
        deliver: async (batch: unknown[]) => {
          frames.push(...batch);
        },
        onlineWith: async () => {
          const ev = frames.find((f) => f.event?.type === "artifact.event");
          seen.reply = await instance.rpc(w.gid, ARTIFACT_REPLY_RPC, bot, { eventId: ev?.event.eventId, ok: true, payload: { total: 13 } });
          seen.repeat = await instance.rpc(w.gid, ARTIFACT_EVENT_RPC, owner, args);
          return true;
        },
      };
      instance.env = {
        ...orig,
        MEMBER_GATEWAY: {
          idFromName: (name: string) => ({ name }),
          get: (id: { name: string }, opts: unknown) => (id.name === w.bot.id ? fake : ns.get(ns.idFromName(id.name), opts)),
        },
      };
      try {
        ack = await instance.rpc(w.gid, ARTIFACT_EVENT_RPC, owner, args);
      } finally {
        instance.env = orig;
      }
    });
    expect(ack, JSON.stringify(ack)).toMatchObject({ ok: true, value: { expect: { by: w.bot.id, mode: "live" } } });
    expect(seen.reply, JSON.stringify(seen.reply)).toMatchObject({ ok: true });
    expect(seen.repeat, JSON.stringify(seen.repeat)).toMatchObject({ ok: true, value: { eventId: ack.value.eventId, duplicate: true } });
    const reply = await w.co.next((f) => f.t === "transient" && f.event.type === "artifact.reply" && f.event.eventId === ack.value.eventId);
    expect(reply.event).toMatchObject({ ok: true, payload: { total: 13 } });
    // The stored ack has the receiver's presence.
    const again = await w.co.ok({ t: "artifact.event", groupId: w.gid, ...args });
    expect(again).toMatchObject({ eventId: ack.value.eventId, duplicate: true, expect: { by: w.bot.id, mode: "live" } });
    w.co.close();
  });
});

