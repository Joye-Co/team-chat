// Bindings of the bots feature and stubs for its Durable Objects (location hint `oc`).
import { type Env, LOCATION_HINT, type Rpc } from "../core/env";
import type { BotDesk } from "./desk";
import type { MachineLink } from "./link";

export interface BotsEnv extends Env {
  BOT_DESK: DurableObjectNamespace<BotDesk>;
  MACHINE_LINK: DurableObjectNamespace<MachineLink>;
}

/** The bot's front desk: `idFromName(botMemberId)`. */
export function deskStub(env: Env, botId: string): Rpc<BotDesk> {
  const ns = (env as BotsEnv).BOT_DESK;
  return ns.get(ns.idFromName(botId), { locationHint: LOCATION_HINT }) as unknown as Rpc<BotDesk>;
}

/** The machine's link: `idFromName(machineId)`. */
export function linkStub(env: Env, machineId: string): Rpc<MachineLink> {
  const ns = (env as BotsEnv).MACHINE_LINK;
  return ns.get(ns.idFromName(machineId), { locationHint: LOCATION_HINT }) as unknown as Rpc<MachineLink>;
}
