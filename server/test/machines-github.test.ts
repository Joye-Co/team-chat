// design-github-identity 3.6: a computer's GitHub sign-in from its heartbeat. Kept by the link,
// shown in the machine list to admins and the computer's owner (on the host only when signed in),
// and on a computer that runs tasks in VMs an alert when the sign-in went or a run found none.
import { env } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import type { BotsEnv } from "../src/bots/env";
import type { MachineLink } from "../src/bots/link";
import { ALERT_TEXT, githubAlerts } from "../src/monitor/alerts";
import { Daemon, HEALTH, world } from "./daemon";
import { Client, api, createMember } from "./helpers";

const linkOf = (machineId: string) => {
  const ns = (env as unknown as BotsEnv).MACHINE_LINK;
  return ns.get(ns.idFromName(machineId)) as unknown as Pick<MachineLink, "githubMissing">;
};

/** A team computer the world's AI member is placed on, its daemon connected. */
async function placed(runsIn: "vm" | "host") {
  const w = await world();
  const r = await api(`/v1/bots/${w.bot.id}/runner`, { method: "PUT", token: w.owner.token, body: { runtime: "codex", labels: [], machineId: w.machineId } });
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  const { c } = await Client.connect(w.owner.token);
  const { d } = await Daemon.connect(w.machineToken, { labels: [], runsIn });
  return { w, c, d };
}

function beat(d: Daemon, github?: { state: string; login?: string }) {
  d.send({ t: "heartbeat", at: Date.now(), health: HEALTH, runs: [], metrics: { at: Date.now(), runs: [], ...(github ? { github } : {}) } });
  return d.next((f) => f.t === "heartbeat.ack");
}

const fromBot = (c: Client, botId: string, type: string, pred: (e: any) => boolean = () => true) =>
  c.next((f) => f.t === "event" && f.event.type === type && f.event.actorId === botId && pred(f.event), 8000);

const botMessages = (c: Client, botId: string) => c.frames.filter((f) => f.t === "event" && f.event.type === "message.created" && f.event.actorId === botId);

const settle = () => new Promise((r) => setTimeout(r, 300));

describe("the GitHub alert from the heartbeat", () => {
  const base = { up: {}, now: 1000 };
  const IN = { state: "signed_in", login: "ai-bot" } as const;
  it("starts when a sign-in on a VM computer went, ends when it is back or no longer reported", () => {
    const was = githubAlerts({ ...base, runsIn: "vm", github: IN, seen: undefined });
    expect(was).toEqual({ raise: [], clear: [], seen: "signed_in" });
    for (const state of ["signed_out", "no_gh"] as const) {
      expect(githubAlerts({ ...base, runsIn: "vm", github: { state }, seen: "signed_in" })).toEqual({
        raise: [{ key: "github", kind: "github", since: 1000 }],
        clear: [],
        seen: state,
      });
    }
    // "unknown" says nothing either way and keeps the last certain state.
    expect(githubAlerts({ ...base, runsIn: "vm", github: { state: "unknown" }, seen: "signed_in" })).toEqual({ raise: [], clear: [], seen: "signed_in" });
    const up = { github: { key: "github", kind: "github" as const, since: 1 } };
    expect(githubAlerts({ ...base, up, runsIn: "vm", github: { state: "signed_out" }, seen: "signed_out" })).toMatchObject({ raise: [], clear: [] });
    expect(githubAlerts({ ...base, up, runsIn: "vm", github: IN, seen: "signed_out" }).clear).toEqual(["github"]);
    expect(githubAlerts({ ...base, up, runsIn: "vm", github: undefined, seen: "signed_out" }).clear).toEqual(["github"]);
  });

  it("never starts for a computer that never signed in, or one that runs on the host", () => {
    expect(githubAlerts({ ...base, runsIn: "vm", github: { state: "signed_out" }, seen: undefined }).raise).toEqual([]);
    expect(githubAlerts({ ...base, runsIn: "vm", github: { state: "no_gh" }, seen: "signed_out" }).raise).toEqual([]);
    expect(githubAlerts({ ...base, runsIn: "host", github: { state: "signed_out" }, seen: "signed_in" }).raise).toEqual([]);
    expect(githubAlerts({ ...base, runsIn: undefined, github: { state: "signed_out" }, seen: "signed_in" }).raise).toEqual([]);
  });

  it("tells the admins when the VM computer's sign-in went, and withdraws it when it is back", async () => {
    const { w, c, d } = await placed("vm");
    await beat(d, { state: "signed_in", login: "ai-bot" });
    await beat(d, { state: "signed_out" });
    const msg = await fromBot(c, w.bot.id, "message.created");
    expect(msg.event.data.message.mentions).toEqual([w.owner.id]);
    expect(msg.event.data.message.body).toBe(ALERT_TEXT.github("mac-mini", false));
    expect(ALERT_TEXT.githubTitle("mac-mini")).toBe("mac-mini 没有登录 GitHub");
    // Still signed out: no second notice.
    await beat(d, { state: "signed_out" });
    await beat(d, { state: "signed_in", login: "ai-bot" });
    await fromBot(c, w.bot.id, "topic.updated", (e) => e.data.status === "done");
    expect(botMessages(c, w.bot.id)).toHaveLength(1);
    c.close();
    d.close();
  });

  it("says nothing about a computer that never signed in until a run finds no sign-in, and that only once", async () => {
    const { w, c, d } = await placed("vm");
    await beat(d, { state: "signed_out" });
    await beat(d, { state: "no_gh" });
    await settle();
    expect(botMessages(c, w.bot.id)).toHaveLength(0);
    expect(await linkOf(w.machineId).githubMissing("no_gh")).toBe(true);
    const msg = await fromBot(c, w.bot.id, "message.created");
    expect(msg.event.data.message.body).toBe(ALERT_TEXT.github("mac-mini", false));
    expect(await linkOf(w.machineId).githubMissing("no_gh")).toBe(false);
    await beat(d, { state: "no_gh" });
    await settle();
    expect(botMessages(c, w.bot.id)).toHaveLength(1);
    // The heartbeat no longer reports it (an older daemon): the alert ends.
    await beat(d);
    await fromBot(c, w.bot.id, "topic.updated", (e) => e.data.status === "done");
    c.close();
    d.close();
  });

  it("never alerts a computer that runs on the host", async () => {
    const { w, c, d } = await placed("host");
    await beat(d, { state: "signed_in", login: "joye" });
    await beat(d, { state: "signed_out" });
    expect(await linkOf(w.machineId).githubMissing("signed_out")).toBe(false);
    await settle();
    expect(botMessages(c, w.bot.id)).toHaveLength(0);
    c.close();
    d.close();
  });

  it("leaves a run that found no sign-in on a personal computer to the run's own reply", async () => {
    const owner = await createMember({ name: "Owner" });
    const add = await api("/v1/machines", { token: owner.token, body: { name: "own-box", personal: true } });
    expect(add.status, JSON.stringify(add.body)).toBe(200);
    const { d } = await Daemon.connect(add.body.token, { labels: [], runsIn: "vm" });
    expect(await linkOf(add.body.machine.id).githubMissing("signed_out")).toBe(false);
    d.close();
  });
});

describe("the GitHub sign-in in the machine list", () => {
  const listed = async (token: string, id: string) => {
    const r = await api("/v1/machines", { token });
    expect(r.status).toBe(200);
    return (r.body as { id: string; github?: unknown }[]).find((m) => m.id === id);
  };

  it("on the host only when signed in; on a VM computer any state; never for a plain member", async () => {
    const w = await world();
    const { d } = await Daemon.connect(w.machineToken, { labels: [], runsIn: "host" });
    await beat(d, { state: "signed_out" });
    expect((await listed(w.owner.token, w.machineId))?.github).toBeUndefined();
    await beat(d, { state: "signed_in", login: "ai-bot" });
    expect((await listed(w.owner.token, w.machineId))?.github).toEqual({ state: "signed_in", login: "ai-bot" });
    expect((await listed(w.human.token, w.machineId))?.github).toBeUndefined();
    // A heartbeat without it clears it.
    await beat(d);
    expect((await listed(w.owner.token, w.machineId))?.github).toBeUndefined();
    d.close();

    const { d: vm } = await Daemon.connect(w.machineToken, { labels: [], runsIn: "vm" });
    await beat(vm, { state: "no_gh" });
    expect((await listed(w.owner.token, w.machineId))?.github).toEqual({ state: "no_gh" });
    expect((await listed(w.human.token, w.machineId))?.github).toBeUndefined();
    vm.close();
  });

  it("a personal computer's sign-in goes to its owner and the admins, not to other members", async () => {
    const w = await world();
    const add = await api("/v1/machines", { token: w.human.token, body: { name: "alice-box", personal: true } });
    expect(add.status, JSON.stringify(add.body)).toBe(200);
    const id = add.body.machine.id as string;
    const { d } = await Daemon.connect(add.body.token, { labels: [], runsIn: "host" });
    await beat(d, { state: "signed_in", login: "alice" });
    expect((await listed(w.human.token, id))?.github).toEqual({ state: "signed_in", login: "alice" });
    expect((await listed(w.owner.token, id))?.github).toEqual({ state: "signed_in", login: "alice" });
    const bob = await createMember({ name: "Bob" });
    expect((await listed(bob.token, id))?.github).toBeUndefined();
    d.close();
  });
});
