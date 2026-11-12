import { env, runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { DEFAULT_APPROVAL_POLICY, DEFAULT_NOTIFY_SETTINGS, type NotifySettings } from "@chat/protocol";
import { afterAll, describe, expect, it } from "vitest";
import { buildPrompt } from "../src/bots/dispatch";
import { decideNotifyEvent } from "../src/notify/rules";
import {
  inQuietHours,
  keepNotify,
  applyPolicyUpdate,
  localMinutes,
  nextLocalTime,
  policyPrompt,
  teamPolicy,
  withDefaults,
} from "../src/settings/rules";
import { Daemon, downViolations, mention, world } from "./daemon";
import { Client, api, clientMsgId, createGroup, createMember, protocolViolations, waitUntil } from "./helpers";

afterAll(async () => {
  await new Promise((r) => setTimeout(r, 300));
  expect(protocolViolations).toEqual([]);
  expect(downViolations).toEqual([]);
});

const H = 3_600_000;
// 2026-11-04 00:00 UTC = 08:00 in Asia/Shanghai.
const MIDNIGHT_UTC = Date.UTC(2026, 10, 4);

function gatewayOf(memberId: string) {
  const ns = (env as unknown as { MEMBER_GATEWAY: DurableObjectNamespace }).MEMBER_GATEWAY;
  return ns.get(ns.idFromName(memberId));
}

describe("settings rules", () => {
  it("read local time and quiet hours across midnight", () => {
    expect(localMinutes(MIDNIGHT_UTC, "Asia/Shanghai")).toBe(8 * 60);
    expect(localMinutes(MIDNIGHT_UTC, "UTC")).toBe(0);
    expect(localMinutes(MIDNIGHT_UTC, "Not/AZone")).toBe(0);
    const q = { enabled: true, start: "22:00", end: "08:00" };
    // 23:00 and 07:59 Shanghai are quiet, 08:00 and 21:59 are not.
    expect(inQuietHours(MIDNIGHT_UTC - 9 * H, q, "Asia/Shanghai")).toBe(true);
    expect(inQuietHours(MIDNIGHT_UTC - 60_000, q, "Asia/Shanghai")).toBe(true);
    expect(inQuietHours(MIDNIGHT_UTC, q, "Asia/Shanghai")).toBe(false);
    expect(inQuietHours(MIDNIGHT_UTC + 14 * H - 60_000, q, "Asia/Shanghai")).toBe(false);
    expect(inQuietHours(MIDNIGHT_UTC - 9 * H, { ...q, enabled: false }, "Asia/Shanghai")).toBe(false);
    expect(inQuietHours(MIDNIGHT_UTC, { ...q, start: "08:00", end: "08:00" }, "Asia/Shanghai")).toBe(false);
    expect(inQuietHours(MIDNIGHT_UTC + 2 * H, { ...q, start: "09:00", end: "18:00" }, "Asia/Shanghai")).toBe(true);
  });

  it("find the next digest time in the member's zone", () => {
    // 08:00 Shanghai now: 09:00 is in one hour, 08:00 is tomorrow.
    expect(nextLocalTime(MIDNIGHT_UTC, "09:00", "Asia/Shanghai")).toBe(MIDNIGHT_UTC + H);
    expect(nextLocalTime(MIDNIGHT_UTC, "08:00", "Asia/Shanghai")).toBe(MIDNIGHT_UTC + 24 * H);
    expect(nextLocalTime(MIDNIGHT_UTC, "18:00", "UTC")).toBe(MIDNIGHT_UTC + 18 * H);
    // Daylight saving: 2026-03-08 is the US change; 09:00 New York is 13:00 UTC after it.
    expect(nextLocalTime(Date.UTC(2026, 2, 8, 6), "09:00", "America/New_York")).toBe(Date.UTC(2026, 2, 8, 13));
  });

  it("keep or drop a notification by switch and quiet hours", () => {
    const s = withDefaults(null);
    expect(keepNotify(s, "mention", MIDNIGHT_UTC)).toBe(true);
    expect(keepNotify(s, undefined, MIDNIGHT_UTC)).toBe(true);
    expect(keepNotify({ ...s, events: { ...s.events, waiting: false } }, "waiting", MIDNIGHT_UTC)).toBe(false);
    // An event this server does not switch follows only quiet hours.
    expect(keepNotify({ ...s, events: { ...s.events, waiting: false } }, "someday", MIDNIGHT_UTC)).toBe(true);
    const quiet = { ...s, quietHours: { ...s.quietHours, enabled: true } };
    const night = MIDNIGHT_UTC - 9 * H;
    expect(keepNotify(quiet, "mention", night)).toBe(false);
    expect(keepNotify(quiet, undefined, night)).toBe(false);
    expect(keepNotify(quiet, "waiting", night)).toBe(false);
    expect(keepNotify(quiet, "waiting", MIDNIGHT_UTC)).toBe(true);
    // Stored settings of a partial or older shape get defaults, and unknown fields are dropped.
    expect(withDefaults({ events: { mention: false } } as Partial<NotifySettings>).events).toEqual({ ...DEFAULT_NOTIFY_SETTINGS.events, mention: false });
    const old = { enabled: false, events: { newTopic: true }, quietHours: { allowWaiting: true } } as unknown as Partial<NotifySettings>;
    expect(withDefaults(old)).toEqual(DEFAULT_NOTIFY_SETTINGS);
  });

  it("decide blocked notifications with a plain reason for old clients", () => {
    const base = { memberId: "me", muted: false, level: "mentions" as const, following: true, waitingAdded: [] };
    const blocked = { seq: 3, groupId: "g", at: 1, actorId: "bot", type: "topic.updated", data: { topicId: "t", status: "blocked" } } as never;
    expect(decideNotifyEvent({ ...base, event: blocked })).toEqual({ notify: "message", notifyEvent: "blocked" });
    expect(decideNotifyEvent({ ...base, following: false, event: blocked })).toBeUndefined();
    expect(decideNotifyEvent({ ...base, muted: true, event: blocked })).toBeUndefined();
    expect(decideNotifyEvent({ ...base, level: "none", event: blocked })).toBeUndefined();
    expect(decideNotifyEvent({ ...base, memberId: "bot", event: blocked })).toBeUndefined();
    const done = { seq: 4, groupId: "g", at: 1, actorId: "bot", type: "topic.updated", data: { topicId: "t", status: "done" } } as never;
    expect(decideNotifyEvent({ ...base, event: done })).toEqual({ notify: "result", notifyEvent: "result" });
  });

  it("update approval policies and write the prompt rules", () => {
    const team = teamPolicy(null);
    expect(team).toEqual(DEFAULT_APPROVAL_POLICY);
    expect(teamPolicy({ askBefore: { mergeMain: false } } as never).askBefore).toEqual({ ...DEFAULT_APPROVAL_POLICY.askBefore, mergeMain: false });
    const p = applyPolicyUpdate(team, { askBefore: { mergeMain: false }, maxSpendCny: null, approver: "owners" }, 5);
    expect(p).toMatchObject({ askBefore: { mergeMain: false, deployProd: true }, maxSpendCny: null, approver: "owners", updatedAt: 5 });
    const rules = policyPrompt(team) as string;
    expect(rules).toContain("merge into the main branch");
    expect(rules).toContain("¥50");
    expect(policyPrompt(p)).not.toContain("main branch");
    // The team approver is saved, but gated actions go to the computer's approvers (an admin here).
    expect(policyPrompt(p)).toContain("a team admin");
    expect(policyPrompt(p)).not.toContain("a team owner");
    const none = applyPolicyUpdate(
      team,
      { askBefore: { mergeMain: false, deployProd: false, prodConfig: false, deleteData: false, externalPublish: false }, maxSpendCny: null },
      6,
    );
    expect(policyPrompt(none)).toBeUndefined();
    const prompt = buildPrompt({ botName: "dev", title: "t", messages: [], names: new Map(), request: { authorId: "a", body: "go" }, rules });
    expect(prompt).toContain("Team rule");
  });
});

describe("notification settings", () => {
  it("read defaults, change fields, reject bad values and audit", async () => {
    const m = await createMember();
    const get = await api("/v1/me/notify-settings", { token: m.token });
    expect(get).toMatchObject({ status: 200, body: DEFAULT_NOTIFY_SETTINGS });
    const up = await api("/v1/me/notify-settings", {
      method: "PATCH",
      token: m.token,
      body: { events: { result: false }, quietHours: { enabled: true, start: "23:30" }, digestAt: "18:00" },
    });
    expect(up.status).toBe(200);
    expect(up.body).toEqual({
      events: { waiting: true, mention: true, result: false, blocked: true },
      quietHours: { enabled: true, start: "23:30", end: "08:00" },
      timeZone: "Asia/Shanghai",
      updatedAt: up.body.updatedAt,
      digestAt: "18:00",
    });
    expect(up.body.updatedAt).toBeGreaterThan(0);
    expect((await api("/v1/me/notify-settings", { token: m.token })).body).toEqual(up.body);
    for (const body of [{ timeZone: "Mars/Olympus" }, { digestAt: "25:00" }, { quietHours: { start: "9:00" } }, { events: { mention: "yes" } }]) {
      expect((await api("/v1/me/notify-settings", { method: "PATCH", token: m.token, body })).status).toBe(400);
    }
    expect((await api("/v1/me/notify-settings", { method: "PATCH", body: {} })).status).toBe(401);
    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'settings.notify' AND actor_id = ?").bind(m.id).first<{ details: string }>();
    expect(JSON.parse(audit?.details ?? "{}").changed.sort()).toEqual(["digestAt", "events", "quietHours"]);
    // The digest is planned in the member's gateway.
    await runInDurableObject(gatewayOf(m.id), (_o, state: DurableObjectState) => {
      const row = state.storage.sql.exec<{ next_at: number | null }>("SELECT next_at FROM digest_state").one();
      expect(row.next_at).toBeGreaterThan(Date.now());
    });
  });

  it("drop notify by event switch and in quiet hours, keep the event, and tag blocked topics", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id]);
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token, { features: ["settings/1"] });

    // Default: a mention notifies and says which event it is.
    const t1 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@b", mentions: [b.id] });
    const f1 = await cb.next((f) => f.t === "event" && f.event.type === "message.created" && f.event.data.message.id === t1.message.id);
    expect(f1).toMatchObject({ notify: "mention", notifyEvent: "mention" });

    // Mentions off: the event arrives without notify.
    await api("/v1/me/notify-settings", { method: "PATCH", token: b.token, body: { events: { mention: false } } });
    const t2 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@b again", mentions: [b.id] });
    const f2 = await cb.next((f) => f.t === "event" && f.event.type === "message.created" && f.event.data.message.id === t2.message.id);
    expect(f2.notify).toBeUndefined();
    expect(f2.notifyEvent).toBeUndefined();

    // A followed topic that becomes blocked notifies as `blocked`.
    await cb.ok({ t: "topic.follow", groupId: gid, topicId: t2.topic.id, follow: true });
    await ca.ok({ t: "topic.update", groupId: gid, topicId: t2.topic.id, status: "blocked" });
    const f3 = await cb.next((f) => f.t === "event" && f.event.type === "topic.updated" && f.event.data.status === "blocked");
    expect(f3).toMatchObject({ notify: "message", notifyEvent: "blocked" });

    // Quiet hours around now (UTC): a switched-on mention arrives without notify.
    const hhmm = (m: number) => `${String(Math.floor((((m % 1440) + 1440) % 1440) / 60)).padStart(2, "0")}:${String((((m % 1440) + 1440) % 1440) % 60).padStart(2, "0")}`;
    const now = localMinutes(Date.now(), "UTC");
    await api("/v1/me/notify-settings", {
      method: "PATCH",
      token: b.token,
      body: { events: { mention: true }, timeZone: "UTC", quietHours: { enabled: true, start: hhmm(now - 60), end: hhmm(now + 60) } },
    });
    const t3 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@b x", mentions: [b.id] });
    const f4 = await cb.next((f) => f.t === "event" && f.event.type === "message.created" && f.event.data.message.id === t3.message.id);
    expect(f4.notify).toBeUndefined();
    ca.close();
    cb.close();
  });

  it("send the daily digest to settings/1 sockets only, counting what happened since the last one", async () => {
    const a = await createMember();
    const b = await createMember();
    const gid = await createGroup(a, [b.id]);
    await api("/v1/me/notify-settings", { method: "PATCH", token: b.token, body: { digestAt: "09:00" } });
    const { c: ca } = await Client.connect(a.token);
    const { c: cb } = await Client.connect(b.token, { features: ["settings/1"] });
    const { c: old } = await Client.connect(b.token);
    const t1 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@b one", mentions: [b.id] });
    const t2 = await ca.ok({ t: "send", groupId: gid, clientMsgId: clientMsgId(), body: "@b two", mentions: [b.id] });
    await cb.next((f) => f.t === "event" && f.event.data?.message?.id === t2.message.id);

    // Make the digest due now and fire the alarm.
    const gw = gatewayOf(b.id);
    await runInDurableObject(gw, (_o, state: DurableObjectState) => {
      state.storage.sql.exec("UPDATE digest_state SET next_at = ?", Date.now() - 1);
    });
    expect(await runDurableObjectAlarm(gw)).toBe(true);
    const digest = (await cb.next((f) => f.t === "transient" && f.event.type === "digest")).event;
    expect(digest.counts).toMatchObject({ mention: 2, waiting: 0 });
    expect(digest.items.map((x: any) => x.topicId).sort()).toEqual([t1.topic.id, t2.topic.id].sort());
    expect(old.frames.some((f) => f.t === "transient" && f.event.type === "digest")).toBe(false);
    // The next digest is planned for tomorrow 09:00 and starts empty.
    await runInDurableObject(gw, (_o, state: DurableObjectState) => {
      const row = state.storage.sql.exec<{ last_at: number; next_at: number }>("SELECT last_at, next_at FROM digest_state").one();
      expect(row.next_at).toBeGreaterThan(Date.now());
      expect(row.last_at).toBeLessThanOrEqual(Date.now());
    });
    ca.close();
    cb.close();
    old.close();
  });
});

describe("devices", () => {
  it("list my signed-in devices and sign out one of them", async () => {
    const m = await createMember();
    const other = await createMember();
    // A second device of m, signed in with a sign-in code.
    const owner = await createMember({ level: "owner" });
    const inv = await api("/v1/invites", { token: owner.token, body: { name: "x", kind: "human", level: "member", memberId: m.id } });
    expect(inv.status).toBe(200);
    const red = await api("/v1/auth/redeem", { body: { inviteCode: inv.body.code, deviceName: "Joye 的 iMac" } });
    expect(red.status).toBe(200);
    const list = await api("/v1/me/devices", { token: m.token });
    expect(list.status).toBe(200);
    expect(list.body.devices).toHaveLength(2);
    const here = list.body.devices.find((d: any) => d.current);
    const imac = list.body.devices.find((d: any) => d.name === "Joye 的 iMac");
    expect(here).toMatchObject({ id: m.tokenId, current: true });
    expect(imac).toMatchObject({ current: false });
    expect(JSON.stringify(list.body)).not.toContain(red.body.token);

    const { c: ci } = await Client.connect(red.body.token);
    // Someone else's token, or an unknown id: not found.
    expect((await api(`/v1/me/devices/${other.tokenId}`, { method: "DELETE", token: m.token })).status).toBe(404);
    expect((await api("/v1/me/devices/01NOPE", { method: "DELETE", token: m.token })).status).toBe(404);
    const del = await api(`/v1/me/devices/${imac.id}`, { method: "DELETE", token: m.token });
    expect(del).toMatchObject({ status: 200, body: { revoked: 1 } });
    await waitUntil(() => ci.closed, 5000, "socket closed");
    expect((await api("/v1/me", { token: red.body.token })).status).toBe(401);
    expect((await api("/v1/me/devices", { token: m.token })).body.devices).toHaveLength(1);
    expect((await api(`/v1/me/devices/${imac.id}`, { method: "DELETE", token: m.token })).status).toBe(404);
  });
});

describe("approval policies", () => {
  it("read the team policy, let only owners change it, and audit", async () => {
    const owner = await createMember({ level: "owner" });
    const m = await createMember();
    const team = await api("/v1/settings/approvals", { token: m.token });
    expect(team).toMatchObject({ status: 200, body: DEFAULT_APPROVAL_POLICY });
    expect((await api("/v1/settings/approvals", { method: "PATCH", token: m.token, body: { maxSpendCny: 20 } })).status).toBe(403);
    expect((await api("/v1/settings/approvals", { method: "PATCH", body: { maxSpendCny: 20 } })).status).toBe(401);
    for (const body of [{ maxSpendCny: 0 }, { maxSpendCny: 20.5 }, { approver: "anyone" }, { askBefore: { mergeMain: "no" } }]) {
      expect((await api("/v1/settings/approvals", { method: "PATCH", token: owner.token, body })).status).toBe(400);
    }
    const up = await api("/v1/settings/approvals", { method: "PATCH", token: owner.token, body: { maxSpendCny: 100, askBefore: { externalPublish: false } } });
    expect(up.status).toBe(200);
    expect(up.body).toMatchObject({ maxSpendCny: 100, approver: "requester", askBefore: { externalPublish: false, mergeMain: true } });
    expect(up.body.updatedAt).toBeGreaterThan(0);
    const up2 = await api("/v1/settings/approvals", { method: "PATCH", token: owner.token, body: { maxSpendCny: null, approver: "owners" } });
    expect(up2.body).toMatchObject({ maxSpendCny: null, approver: "owners", askBefore: { externalPublish: false } });
    expect((await api("/v1/settings/approvals", { token: m.token })).body).toEqual(up2.body);
    const audit = await env.DB.prepare("SELECT details FROM audit WHERE action = 'settings.approvals' ORDER BY id DESC LIMIT 1").first<{ details: string }>();
    expect(JSON.parse(audit?.details ?? "{}")).toMatchObject({ before: { maxSpendCny: 100 }, after: { maxSpendCny: null, approver: "owners" } });
    // Back to the defaults for the other tests.
    await env.DB.prepare("DELETE FROM approval_policies").run();
  });

  it("put the rules in the run prompt and let only owners approve when the policy says so", async () => {
    const w = await world();
    const put = await api("/v1/settings/approvals", { method: "PATCH", token: w.owner.token, body: { approver: "owners", maxSpendCny: 20 } });
    expect(put.status).toBe(200);
    await api(`/v1/groups/${w.gid}/members`, { token: w.owner.token, body: { memberId: w.owner.id } });
    const { d } = await Daemon.connect(w.machineToken);
    const { c } = await Client.connect(w.human.token);
    const { c: co } = await Client.connect(w.owner.token);
    const topicId = await mention(c, w);
    const assign = await d.next((f) => f.t === "run.assign");
    expect(assign.prompt).toContain("Team rule");
    expect(assign.prompt).toContain("¥20");
    expect(assign.prompt).toContain("a team owner");
    const runId = assign.runId;
    d.send({ t: "run.accept", runId });
    await c.waitEvent((e) => e.type === "run.updated" && e.data.run.status === "running");
    d.event(runId, { type: "approval.request", approvalId: "a1", title: "git push origin main" });
    const card = await waitUntil(
      () => c.events().find((e) => e.type === "message.created" && e.data.message.topicId === topicId && e.data.message.kind === "card")?.data.message,
      5000,
      "approval card",
    );
    // The owner is asked, not the requester.
    expect(card.mentions).toEqual([w.owner.id]);
    // The requester's press and 同意 do not answer; the owner's press does.
    await c.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:a1" });
    await c.ok({ t: "send", groupId: w.gid, topicId, clientMsgId: clientMsgId(), body: "同意" });
    await d.next((f) => f.t === "run.input" && f.text === "同意");
    expect(d.frames.filter((f) => f.t === "run.approval")).toEqual([]);
    await co.ok({ t: "card.action", groupId: w.gid, messageId: card.id, actionId: "approve:a1" });
    expect(await d.next((f) => f.t === "run.approval")).toMatchObject({ approvalId: "a1", decision: "approve" });
    await env.DB.prepare("DELETE FROM approval_policies").run();
    c.close();
    co.close();
    d.close();
  });
});
