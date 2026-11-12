// Bots on the server: BotDesk (one per runner-backed bot), MachineLink (one per machine), their
// HTTP routes and the group room hooks. See docs/protocol.md "Bot runs" and
// docs/machine-protocol.md.
import { roomStub } from "../core/env";
import { unwrap } from "../core/errors";
import type { Env } from "../core/env";
import { registerGroupMemberRemoved, registerMemberDeactivated } from "../core/registry";
import { deskStub } from "./env";
import "./deactivate";
import "./interact";
import "./room";
import { registerBotRoutes } from "./routes";

registerBotRoutes();

// A deactivated bot stops its runs and gives up its topics.
registerMemberDeactivated(async (env, memberId) => {
  const m = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(memberId).first<{ kind: string }>();
  if (m?.kind !== "bot") return;
  await deskStub(env, memberId).deactivated(memberId);
});

// design-relay R2: a deactivated person's pieces of work (chains) stop in every group he is in;
// leaving a group stops them there (chains.ts, the `group.memberRemoved` writer).
registerMemberDeactivated(async (env, memberId) => {
  const m = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(memberId).first<{ kind: string }>();
  if (m?.kind !== "human") return;
  const groups = await env.DB.prepare("SELECT group_id FROM group_members WHERE member_id = ?").bind(memberId).all<{ group_id: string }>();
  for (const { group_id } of groups.results) {
    unwrap(await roomStub(env, group_id).rpc(group_id, "bots.requesterLeft", null, { memberId }));
  }
});

// design-takeover 3.2: a person who leaves a group, or the team, hands back the runs he held
// there (paused or taken over), so they do not stay stopped with reminders to nobody.
async function releaseHolds(env: Env, memberId: string, groupId?: string): Promise<void> {
  const bots = groupId
    ? await env.DB.prepare("SELECT DISTINCT bot_id FROM bot_runs WHERE group_id = ?").bind(groupId).all<{ bot_id: string }>()
    : await env.DB.prepare(
        "SELECT DISTINCT r.bot_id FROM bot_runs r JOIN group_members g ON g.group_id = r.group_id AND g.member_id = ?",
      ).bind(memberId).all<{ bot_id: string }>();
  for (const { bot_id } of bots.results) await deskStub(env, bot_id).releaseHolds(bot_id, memberId, groupId);
}

registerGroupMemberRemoved((env, groupId, memberId) => releaseHolds(env, memberId, groupId));

registerMemberDeactivated(async (env, memberId) => {
  const m = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(memberId).first<{ kind: string }>();
  if (m?.kind === "human") await releaseHolds(env, memberId);
});

export { BotDesk } from "./desk";
export { MachineLink } from "./link";
