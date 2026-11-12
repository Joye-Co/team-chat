// 机 5.1, 5.4, 5.6, 4.3: computers that need their managers. A computer offline for 10 minutes
// while an AI member depends on it, a CLI whose sign-in went, and a full disk are told to the
// managers in a DM from that AI member and withdrawn when they recover; the monitor shows the
// daemon's last problem lines; `healthy: false` says which check failed; a key from the app can
// ask before each use.
import { runDurableObjectAlarm, runInDurableObject, env } from "cloudflare:test";
import { Machine, MachineMonitor } from "@chat/protocol";
import { describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import { MIN_FREE_DISK_BYTES, toMachine, unhealthyReason } from "../src/bots/dispatch";
import { ALERT_OFFLINE_MS, ALERT_TEXT, DISK_CLEAR_BYTES, heartbeatAlerts } from "../src/monitor/alerts";
import { Daemon, HEALTH, world } from "./daemon";
import { Client, api, createMember, waitUntil } from "./helpers";

const linkOf = (machineId: string) => {
  const ns = (env as unknown as BotsEnv).MACHINE_LINK;
  return ns.get(ns.idFromName(machineId));
};

/** A team computer the world's AI member is placed on, its daemon connected. */
async function placed() {
  const w = await world();
  const r = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: [], machineId: w.machineId } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const { c } = await Client.connect(w.owner.token);
  const { d } = await Daemon.connect(w.machineToken, { labels: [] });
  return { w, c, d };
}

const fromBot = (c: Client, botId: string, type: string, pred: (e: any) => boolean = () => true) =>
  c.next((f) => f.t === "event" && f.event.type === type && f.event.actorId === botId && pred(f.event), 8000);

function beat(d: Daemon, health: Partial<typeof HEALTH>, metrics?: Record<string, unknown>) {
  d.send({ t: "heartbeat", at: Date.now(), health: { ...HEALTH, ...health }, runs: [], ...(metrics ? { metrics: { at: Date.now(), runs: [], ...metrics } } : {}) });
  return d.next((f) => f.t === "heartbeat.ack");
}

describe("alerts to a computer's managers (机 5.1)", () => {
  it("start and end from the heartbeat: a full disk, and a sign-in that went", () => {
    const base = { seen: {}, cliName: (rt: string) => `CLI ${rt}`, now: 1000 };
    const low = { diskFreeBytes: MIN_FREE_DISK_BYTES - 1, diskTotalBytes: 500e9 };
    expect(heartbeatAlerts({ ...base, up: {}, health: low }).raise).toEqual([{ key: "disk", kind: "disk", since: 1000, freeBytes: low.diskFreeBytes }]);
    const up = { disk: { key: "disk", kind: "disk" as const, since: 1 } };
    // Just above the line it stays (no flapping); well above it ends.
    expect(heartbeatAlerts({ ...base, up, health: { ...low, diskFreeBytes: MIN_FREE_DISK_BYTES } })).toMatchObject({ raise: [], clear: [] });
    expect(heartbeatAlerts({ ...base, up, health: { ...low, diskFreeBytes: DISK_CLEAR_BYTES } }).clear).toEqual(["disk"]);

    const ok = { diskFreeBytes: 100e9, diskTotalBytes: 500e9 };
    // A CLI never signed in is not news; one that was and went is.
    const never = heartbeatAlerts({ ...base, up: {}, health: ok, logins: [{ runtime: "claude", state: "logged_out", confirmed: true }] });
    expect(never.raise).toEqual([]);
    const was = heartbeatAlerts({ ...base, up: {}, health: ok, logins: [{ runtime: "claude", state: "logged_in" }] });
    expect(was.seen).toEqual({ claude: "logged_in" });
    // From the credential files only it waits for the CLI's own check.
    expect(heartbeatAlerts({ ...base, up: {}, health: ok, seen: was.seen, logins: [{ runtime: "claude", state: "logged_out" }] }).raise).toEqual([]);
    const went = heartbeatAlerts({ ...base, up: {}, health: ok, seen: was.seen, logins: [{ runtime: "claude", state: "logged_out", confirmed: true }] });
    expect(went.raise).toEqual([{ key: "signed_out:claude", kind: "signed_out", since: 1000, runtime: "claude", cli: "CLI claude" }]);
    const back = heartbeatAlerts({ ...base, up: { "signed_out:claude": went.raise[0] }, health: ok, seen: went.seen, logins: [{ runtime: "claude", state: "logged_in" }] });
    expect(back.clear).toEqual(["signed_out:claude"]);
    // A heartbeat without readable metrics changes nothing about sign-ins.
    expect(heartbeatAlerts({ ...base, up: { "signed_out:claude": went.raise[0] }, health: ok, seen: went.seen })).toMatchObject({ raise: [], clear: [] });
  });

  it("tells the admins about a full disk in a DM from the AI member, and withdraws it when there is room again", async () => {
    const { w, c, d } = await placed();
    const alice = await createMember({ name: "Alice2" });
    await beat(d, { diskFreeBytes: 2 * 1024 ** 3 });
    const msg = await fromBot(c, w.bot.id, "message.created");
    const m = msg.event.data.message;
    expect(msg.notify).toBe("mention");
    expect(m.mentions).toEqual([w.owner.id]);
    expect(m.body).toBe(ALERT_TEXT.disk("mac-mini", 2 * 1024 ** 3));
    const group = await env.DB.prepare("SELECT kind, dm_key FROM groups WHERE id = ?").bind(msg.event.groupId).first<{ kind: string; dm_key: string }>();
    expect(group).toEqual({ kind: "dm", dm_key: [w.bot.id, w.owner.id].sort().join(":") });
    // A member who manages nothing hears nothing.
    expect(await env.DB.prepare("SELECT 1 FROM groups WHERE dm_key = ?").bind([w.bot.id, alice.id].sort().join(":")).first()).toBeNull();

    // Still low: no second notice. Room again: the notice says so and its 话题 is done.
    await beat(d, { diskFreeBytes: 3 * 1024 ** 3 });
    await beat(d, { diskFreeBytes: 50e9 });
    const edited = await fromBot(c, w.bot.id, "message.updated");
    expect(JSON.stringify(edited.event.data)).toContain(ALERT_TEXT.recovered);
    await fromBot(c, w.bot.id, "topic.updated", (e) => e.data.status === "done");
    expect(c.frames.filter((f) => f.t === "event" && f.event.type === "message.created" && f.event.actorId === w.bot.id)).toHaveLength(1);
    c.close();
    d.close();
  });

  it("tells them when a CLI that was signed in is signed out, and withdraws it when it is back", async () => {
    const { w, c, d } = await placed();
    await beat(d, {}, { logins: [{ runtime: "codex", state: "logged_in" }] });
    await beat(d, {}, { logins: [{ runtime: "codex", state: "logged_out", confirmed: true }] });
    const msg = await fromBot(c, w.bot.id, "message.created");
    expect(msg.event.data.message.body).toBe(ALERT_TEXT.signedOut("mac-mini", "Codex", true));
    await beat(d, {}, { logins: [{ runtime: "codex", state: "logged_in" }] });
    await fromBot(c, w.bot.id, "topic.updated", (e) => e.data.status === "done");
    c.close();
    d.close();
  });

  it("tells them a computer an AI member depends on is offline after 10 minutes, and withdraws it on its return", async () => {
    const { w, c, d } = await placed();
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    // Before 10 minutes nothing.
    await runInDurableObject(linkOf(w.machineId), (_o, state) => {
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = 'offline_since'", String(Date.now() - ALERT_OFFLINE_MS + 60_000));
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    await new Promise((r) => setTimeout(r, 200));
    expect(c.frames.some((f) => f.t === "event" && f.event.actorId === w.bot.id)).toBe(false);
    await runInDurableObject(linkOf(w.machineId), (_o, state) => {
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = 'offline_since'", String(Date.now() - ALERT_OFFLINE_MS - 1000));
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    const msg = await fromBot(c, w.bot.id, "message.created");
    expect(msg.event.data.message.body).toBe(ALERT_TEXT.offline("mac-mini", 10, ["dev"]));
    const { d: d2 } = await Daemon.connect(w.machineToken, { labels: [] });
    await fromBot(c, w.bot.id, "topic.updated", (e) => e.data.status === "done");
    c.close();
    d2.close();
  });

  it("says nothing about an offline computer no AI member depends on", async () => {
    const w = await world();
    const { c } = await Client.connect(w.owner.token);
    const { d } = await Daemon.connect(w.machineToken, { labels: [] });
    d.close();
    await waitUntil(() => d.closed, 5000, "closed");
    await runInDurableObject(linkOf(w.machineId), (_o, state) => {
      state.storage.sql.exec("UPDATE meta SET value = ? WHERE key = 'offline_since'", String(Date.now() - ALERT_OFFLINE_MS - 1000));
    });
    await runDurableObjectAlarm(linkOf(w.machineId));
    await new Promise((r) => setTimeout(r, 300));
    expect(c.frames.some((f) => f.t === "event" && f.event.actorId === w.bot.id)).toBe(false);
    c.close();
  });
});

describe("what the computer says about itself (机 5.4, 5.6)", () => {
  it("shows the daemon's last problem lines in the monitor, on the server clock", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { labels: [] });
    const at = Date.now() - 60_000;
    await beat(d, {}, { problems: [{ at, text: "link down: read: connection reset" }] });
    const r = await api(`/v1/machines/${w.machineId}/monitor?range=15m`, { token: w.owner.token });
    const m = MachineMonitor.parse(r.body);
    expect(m.problems).toHaveLength(1);
    expect(m.problems?.[0].text).toBe("link down: read: connection reset");
    expect(Math.abs((m.problems?.[0].at ?? 0) - at)).toBeLessThan(5000);
    d.close();
  });

  it("names the check that failed", async () => {
    const now = 100_000;
    const view = { id: "m", name: "m", labels: [], revoked: false, connected: true, runningRuns: [], lastHeartbeatAt: now, health: { ...HEALTH, diskFreeBytes: 1 } };
    expect(unhealthyReason(view, now)).toBe("disk");
    expect(unhealthyReason({ ...view, health: HEALTH, lastHeartbeatAt: now - 120_000 }, now)).toBe("heartbeat");
    expect(unhealthyReason({ ...view, health: HEALTH }, now)).toBeUndefined();
    expect(unhealthyReason({ ...view, connected: false }, now)).toBeUndefined();
    expect(Machine.parse(toMachine(view, "o", 1, now))).toMatchObject({ healthy: false, unhealthy: "disk" });
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { labels: [], health: { diskFreeBytes: 1 } });
    const list = await api("/v1/machines", { token: w.owner.token });
    expect(list.body.find((x: { id: string }) => x.id === w.machineId)).toMatchObject({ healthy: false, unhealthy: "disk" });
    d.close();
  });
});

describe("a key from the app that asks before each use (机 4.3)", () => {
  const KEY = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
  it("relays ask only to a daemon that keeps it", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["accounts"], publicKey: KEY });
    const old = await api(`/v1/machines/${w.machineId}/secrets/CF_TOKEN`, { method: "PUT", token: w.owner.token, body: { sealed: "c2VhbGVk", members: [], ask: true } });
    expect(old.status).toBe(409);
    expect(d.frames.some((x) => x.t === "secret.set")).toBe(false);
    d.close();
    const { d: d2 } = await Daemon.connect(w.machineToken, { features: ["accounts", "ask-keys"], publicKey: KEY });
    const put = await api(`/v1/machines/${w.machineId}/secrets/CF_TOKEN`, { method: "PUT", token: w.owner.token, body: { sealed: "c2VhbGVk", members: [w.bot.id], ask: true } });
    expect(put.status, JSON.stringify(put.body)).toBe(200);
    expect(await d2.next((x) => x.t === "secret.set")).toEqual({ t: "secret.set", name: "CF_TOKEN", sealed: "c2VhbGVk", members: [w.bot.id], ask: true });
    d2.close();
  });
});

  it("keeps a key's ask when a new value comes without it (an older client), and turns it off only on ask: false", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { features: ["accounts", "ask-keys"], publicKey: KEY });
    const secrets = [{ name: "CF_TOKEN", last4: "abcd", members: [], updatedAt: Date.now(), ask: true }];
    await beat(d, {}, { secrets });
    const put = async (body: Record<string, unknown>) => {
      d.frames.length = 0;
      const r = await api(`/v1/machines/${w.machineId}/secrets/CF_TOKEN`, { method: "PUT", token: w.owner.token, body: { sealed: "c2VhbGVk", members: [], ...body } });
      expect(r.status, JSON.stringify(r.body)).toBe(200);
      return d.next((x) => x.t === "secret.set");
    };
    expect(await put({})).toEqual({ t: "secret.set", name: "CF_TOKEN", sealed: "c2VhbGVk", members: [], ask: true });
    expect(await put({ ask: false })).toEqual({ t: "secret.set", name: "CF_TOKEN", sealed: "c2VhbGVk", members: [] });
    // A key that does not ask stays that way.
    await beat(d, {}, { secrets: [{ ...secrets[0], ask: undefined }] });
    expect(await put({})).toEqual({ t: "secret.set", name: "CF_TOKEN", sealed: "c2VhbGVk", members: [] });
    d.close();
  });
