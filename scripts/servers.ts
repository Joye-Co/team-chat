// Server URLs of the deployed envs. They are the operator's, not the project's, so they live in
// the private ops checkout: TC_OPS_DIR (else ~/.config/team-chat/ops), file deploy/servers.json,
// shaped { "e2e": "https://...", "staging": "...", "production": "..." }.
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export function opsDir(): string | undefined {
  const dir =
    process.env.TC_OPS_DIR ?? join(process.env.TEAM_CHAT_CONFIG_DIR ?? join(homedir(), ".config/team-chat"), "ops");
  return existsSync(dir) ? dir : undefined;
}

/** The base URL of `env` from deploy/servers.json, without a trailing slash; undefined when unknown. */
export function deployedServer(env: string): string | undefined {
  const dir = opsDir();
  const file = dir && join(dir, "deploy/servers.json");
  if (!file || !existsSync(file)) return undefined;
  const url = (JSON.parse(readFileSync(file, "utf8")) as Record<string, unknown>)[env];
  return typeof url === "string" && url ? url.replace(/\/+$/, "") : undefined;
}
