// Writes JSON Schema files for the main unions into protocol/schema/. Run with
// `bun run gen:protocol` at the repo root (or `bun run gen:schema`); `--check` only compares.
// The daemon builds CardV2.schema.json in (`chat-run card schema`); the rest documents the wire
// for readers outside TypeScript. test/generated.test.ts fails when the folder is out of date.
import { join } from "node:path";
import { z } from "zod";
import { CardV2Shape } from "../src/card2.ts";
import { schemas } from "../src/index.ts";
import { run, type Tree } from "./generated.ts";

export const schemaRoot = join(import.meta.dir, "../../../protocol/schema");
export const schemaFiles: Tree = new Map();

const files: Record<string, z.ZodType> = {
  ClientFrame: schemas.ClientFrame,
  ServerFrame: schemas.ServerFrame,
  Event: schemas.Event,
  TransientEvent: schemas.TransientEvent,
};
for (const [name, schema] of Object.entries(files)) {
  const json = z.toJSONSchema(schema, { io: "input", unrepresentable: "any" });
  schemaFiles.set(`${name}.schema.json`, `${JSON.stringify({ title: name, ...json }, null, 2)}\n`);
}

// The Card v2 catalog that `chat-run card schema` prints (the daemon embeds this file). The
// shape only: the write rules (ids, limits, one decision surface) are checked by the server.
const card = z.toJSONSchema(CardV2Shape, { io: "input", unrepresentable: "any" });
schemaFiles.set("CardV2.schema.json", `${JSON.stringify({ title: "CardV2", ...card }, null, 2)}\n`);

// HTTP bodies and entities, one file with a definition per schema name.
const defs: Record<string, unknown> = {};
for (const [name, schema] of Object.entries(schemas)) {
  if (name in files) continue;
  const { $schema: _, ...json } = z.toJSONSchema(schema, { io: "input", unrepresentable: "any" }) as Record<string, unknown>;
  defs[name] = json;
}
schemaFiles.set(
  "http.schema.json",
  `${JSON.stringify({ $schema: "https://json-schema.org/draft/2020-12/schema", title: "Entities and HTTP bodies", $defs: defs }, null, 2)}\n`,
);

if (import.meta.main) run("schema files", schemaRoot, schemaFiles);
