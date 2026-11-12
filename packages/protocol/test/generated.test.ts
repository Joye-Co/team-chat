// protocol/fixtures and protocol/schema are generated (packages/protocol/scripts/gen.ts). This
// fails when a checked-in file differs from the generator's output, is missing, or was added by
// hand, so the generator can always be run again without losing or changing anything.
import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fixtures, fixturesRoot } from "../scripts/gen-fixtures.ts";
import { schemaFiles, schemaRoot } from "../scripts/gen-schema.ts";
import { diffTree, writeTree } from "../scripts/generated.ts";

const hint = "run `bun run gen:protocol` at the repo root and commit the result";

test("protocol/fixtures is what the generator writes", () => {
  expect(diffTree(fixturesRoot, fixtures), hint).toEqual([]);
});

test("protocol/schema is what the generator writes", () => {
  expect(diffTree(schemaRoot, schemaFiles), hint).toEqual([]);
});

test("diffTree names missing, changed and hand-written files; writeTree fixes all three", () => {
  const dir = mkdtempSync(join(tmpdir(), "gen-tree-"));
  try {
    const tree = new Map([
      ["a/one.json", "1\n"],
      ["two.json", "2\n"],
    ]);
    writeTree(dir, tree);
    expect(diffTree(dir, tree)).toEqual([]);
    writeFileSync(join(dir, "two.json"), "x\n");
    writeFileSync(join(dir, "a/by-hand.json"), "{}\n");
    rmSync(join(dir, "a/one.json"));
    expect(diffTree(dir, tree)).toEqual([
      "changed two.json",
      "missing a/one.json",
      "not generated a/by-hand.json (add it to packages/protocol/scripts or delete it)",
    ]);
    writeTree(dir, tree);
    expect(diffTree(dir, tree)).toEqual([]);
    expect(readFileSync(join(dir, "two.json"), "utf8")).toBe("2\n");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
