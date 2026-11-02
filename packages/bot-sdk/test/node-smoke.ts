// Runs the echo example under Node against a local server, mentions it once and checks the
// reply. Run: bun test/node-smoke.ts (needs node 22+ on PATH).
import { spawn } from "node:child_process";
import { chmodSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { Bot } from "../src/index.ts";
import { localServer } from "./server.ts";

const server = localServer(Number(process.env.BOT_SDK_E2E_PORT ?? 8795));
let code = 1;
let child: ReturnType<typeof spawn> | null = null;
try {
  await server.start();
  const post = async (path: string, body: unknown, headers: Record<string, string>) => {
    const r = await fetch(`${server.url}${path}`, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify(body) });
    if (!r.ok) throw new Error(`${path} ${r.status}`);
    return (await r.json()) as any;
  };
  const boot = await post("/v1/admin/bootstrap", { ownerName: "Joye" }, { "x-admin-secret": server.adminSecret });
  const made = await post("/v1/bots", { name: "Echo" }, { authorization: `Bearer ${boot.token}` });
  const tokenFile = join(server.dir, "echo.token");
  writeFileSync(tokenFile, made.token, { mode: 0o600 });
  chmodSync(tokenFile, 0o600);
  child = spawn("node", [resolve(import.meta.dir, "../../../examples/echo-bot.ts")], {
    env: { ...process.env, CHAT_URL: server.url, CHAT_TOKEN_FILE: tokenFile, LOG_LEVEL: "warn" },
    stdio: ["ignore", "inherit", "inherit"],
  });
  const human = new Bot({ url: server.url, token: boot.token, logLevel: "warn" });
  let reply = "";
  human.onEvent(({ event }) => {
    if (event?.type === "message.created" && event.data.message.authorId === made.member.id) reply = event.data.message.body;
  });
  await human.start();
  // Wait until the bot is online (it has a fresh state, so it skips older history).
  const until = Date.now() + 20_000;
  let sent = false;
  while (!reply && Date.now() < until) {
    if (!sent) {
      await new Promise((r) => setTimeout(r, 3000));
      await human.startTopic(boot.group.id, { body: "@Echo from node", mentions: [made.member.id] });
      sent = true;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  await human.stop();
  if (reply !== "echo: from node") throw new Error(`unexpected reply: ${JSON.stringify(reply)}`);
  console.log("node smoke: ok");
  code = 0;
} catch (err) {
  console.error("node smoke failed:", err);
} finally {
  child?.kill("SIGTERM");
  await server.dispose();
}
process.exit(code);
