// Every deployed Worker env has Workers logs on and the crons the ops jobs need.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ROOT } from "./lib";

/** JSONC -> JSON: drops comments outside strings. */
function parseJsonc(text: string): any {
  let out = "";
  let inStr = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (inStr) {
      out += c;
      if (c === "\\") out += text[++i];
      else if (c === '"') inStr = false;
    } else if (c === '"') {
      inStr = true;
      out += c;
    } else if (c === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") i++;
      out += "\n";
    } else if (c === "/" && text[i + 1] === "*") {
      i = text.indexOf("*/", i + 2) + 1;
    } else out += c;
  }
  return JSON.parse(out.replace(/,(\s*[}\]])/g, "$1"));
}

function envs(path: string): [string, any][] {
  const cfg = parseJsonc(readFileSync(join(ROOT, path), "utf8"));
  return [["(top level)", cfg], ...Object.entries<any>(cfg.env ?? {})];
}

test("API Worker: logs on and the daily archive cron in every env", () => {
  const list = envs("server/wrangler.jsonc");
  expect(list.map(([n]) => n)).toEqual(["(top level)", "e2e", "staging", "production"]);
  for (const [name, e] of list) {
    expect(e.observability?.enabled, name).toBe(true);
    expect(e.observability?.logs?.enabled, name).toBe(true);
    expect(e.triggers?.crons, name).toContain("0 3 * * *");
    expect(e.r2_buckets?.[0]?.binding, name).toBe("FILES");
  }
});

test("API Worker: the archive cron matches the code", () => {
  const src = readFileSync(join(ROOT, "server/src/ops/index.ts"), "utf8");
  expect(src).toContain('ARCHIVE_CRON = "0 3 * * *"');
});

test("Monitor Worker: logs on, 5-minute cron, targets with secrets named, never a token in vars", () => {
  for (const [name, e] of envs("ops/monitor/wrangler.jsonc")) {
    expect(e.observability?.logs?.enabled, name).toBe(true);
    expect(e.triggers?.crons, name).toEqual(["*/5 * * * *"]);
    const targets = JSON.parse(e.vars.TARGETS);
    expect(targets.length, name).toBeGreaterThan(0);
    for (const t of targets) {
      expect(t.tokenSecret).toMatch(/^MONITOR_TOKEN_[A-Z]+$/);
      expect(t.baseUrl).toMatch(/^https?:\/\//);
    }
    expect(JSON.stringify(e.vars)).not.toMatch(/tc_[A-Za-z0-9_-]{20,}/);
  }
  const top = parseJsonc(readFileSync(join(ROOT, "ops/monitor/wrangler.jsonc"), "utf8"));
  expect(top.name).toBe("chat-monitor");
  expect(JSON.parse(top.vars.TARGETS).map((t: any) => t.name)).toEqual(["production", "staging"]);
});
