// Bots on the server: BotDesk (one per runner-backed bot), MachineLink (one per machine), their
// HTTP routes and the group room hooks. See docs/protocol.md "Bot runs" and
// docs/machine-protocol.md.
import "./room";
import { registerBotRoutes } from "./routes";

registerBotRoutes();

export { BotDesk } from "./desk";
export { MachineLink } from "./link";
