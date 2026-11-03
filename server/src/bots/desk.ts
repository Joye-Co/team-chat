// BotDesk lives in ./desk/: core (state, migrations, room writes), runs, events, stall, input
// and external (the BotDesk class). This file keeps the import path of the Durable Object.
export { BotDesk } from "./desk/external";
export { sanitizePlan } from "./desk/core";
