import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { checkDiagramSource } from "../src/index.ts";

type Case = { source: string; ok: boolean; type?: string; reason?: string };
const file = join(import.meta.dir, "../../../protocol/checks/diagram-checks.json");
const { cases } = JSON.parse(readFileSync(file, "utf8")) as { cases: Case[] };

test("diagram source checks match protocol/checks/diagram-checks.json", () => {
  expect(cases.length).toBeGreaterThan(10);
  for (const c of cases) {
    const want: unknown = c.ok ? { ok: true, type: c.type } : { ok: false, reason: c.reason };
    expect({ source: c.source.slice(0, 40), got: checkDiagramSource(c.source) as unknown }).toEqual({ source: c.source.slice(0, 40), got: want });
  }
});
