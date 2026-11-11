// Bots on the server: BotDesk (one per runner-backed bot), MachineLink (one per machine), their
// HTTP routes and the group room hooks. See docs/protocol.md "Bot runs" and
// docs/machine-protocol.md.
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

export { BotDesk } from "./desk";
export { MachineLink } from "./link";
