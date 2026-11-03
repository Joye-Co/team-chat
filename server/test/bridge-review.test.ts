// Review fixes of the 1.1 产出 bridge and the chat-run outputs: bot patterns run without
// backtracking (SEC-4), page emit events are resolved like card events (F2), a page event row
// exists before the event is delivered (F3), output keys are scoped to the run (daemon F1), and
// the age of an output is measured on the server clock (daemon F3).
import { SELF, env, runInDurableObject } from "cloudflare:test";
import type { CardV2 } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import type { Actor } from "../src/core/registry";
import { ARTIFACT_EVENT_RPC, ARTIFACT_REPLY_RPC, tcSchemaViolation } from "../src/files/bridge";
import { Daemon, downViolations, HEALTH, mention, world, type World } from "./daemon";
import { BASE, Client, type TestMember, api, clientMsgId, createGroup, createMember, protocolViolations } from "./helpers";

const benv = env as unknown as BotsEnv;
const roomOf = (groupId: string) => benv.GROUP_ROOM.get(benv.GROUP_ROOM.idFromName(groupId));
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

