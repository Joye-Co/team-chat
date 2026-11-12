import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { issueReason, schemas, type SchemaName } from "../src/index.ts";

const root = join(import.meta.dir, "../../../protocol/fixtures");

type Fixture = { kind: string; valid: boolean; reason?: string; value: unknown; normalized?: unknown };

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(join(dir, e.name)) : e.name.endsWith(".json") ? [join(dir, e.name)] : [],
  );
}

const files = walk(root);

describe("fixtures", () => {
  for (const file of files) {
    const name = relative(root, file);
    const fixture = JSON.parse(readFileSync(file, "utf8")) as Fixture;
    test(name, () => {
      expect(Object.keys(schemas)).toContain(fixture.kind);
      const schema = schemas[fixture.kind as SchemaName];
      const result = schema.safeParse(fixture.value);
      if (!fixture.valid) {
        expect(result.success).toBe(false);
        // The server's reason: the first issue that names one (core/errors.ts schemaReason).
        if (fixture.reason !== undefined && !result.success) {
          const named = result.error.issues.map((i) => issueReason(i.message)).find((r) => r !== undefined);
          expect(named).toBe(fixture.reason);
        }
        return;
      }
      if (!result.success) throw result.error;
      const roundTrip = JSON.parse(JSON.stringify(result.data));
      expect(roundTrip).toStrictEqual(fixture.normalized ?? fixture.value);
    });
  }
});

describe("coverage", () => {
  const parsed = files.map((f) => JSON.parse(readFileSync(f, "utf8")) as Fixture);
  const valid = (kind: string) => parsed.filter((f) => f.valid && f.kind === kind).map((f) => f.value as Record<string, unknown>);
  const options = (kind: "ClientFrame" | "ServerFrame" | "Event" | "TransientEvent", key: string) =>
    new Set(schemas[kind].options.map((o) => ("shape" in o ? o.shape[key as never] : o.options[0].shape[key as never])).map((l: { value: string }) => l.value));

  test("every frame and event type has a valid fixture", () => {
    for (const [kind, key] of [["ClientFrame", "t"], ["ServerFrame", "t"], ["Event", "type"], ["TransientEvent", "type"]] as const) {
      const seen = new Set(valid(kind).map((v) => v[key]));
      expect([...seen].sort()).toEqual([...options(kind, key)].sort());
    }
  });

  test("every schema has a valid fixture", () => {
    const kinds = new Set(parsed.filter((f) => f.valid).map((f) => f.kind));
    for (const name of Object.keys(schemas)) expect(kinds.has(name)).toBe(true);
  });
});
