// Bots on the server: BotDesk (one per runner-backed bot), MachineLink (one per machine), their
// HTTP routes and the group room hooks. See docs/protocol.md "Bot runs" and
// docs/machine-protocol.md.
import { roomStub } from "../core/env";
import { unwrap } from "../core/errors";
import { registerMemberDeactivated } from "../core/registry";
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

export { BotDesk } from "./desk";
export { MachineLink } from "./link";
