// Writes JSON Schema files for the main unions into protocol/schema/.
// Run with `bun run gen:schema`.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { schemas } from "../src/index.ts";

const out = join(import.meta.dir, "../../../protocol/schema");
mkdirSync(out, { recursive: true });

const files: Record<string, z.ZodType> = {
  ClientFrame: schemas.ClientFrame,
  ServerFrame: schemas.ServerFrame,
  Event: schemas.Event,
  TransientEvent: schemas.TransientEvent,
};
for (const [name, schema] of Object.entries(files)) {
  const json = z.toJSONSchema(schema, { io: "input", unrepresentable: "any" });
  writeFileSync(join(out, `${name}.schema.json`), `${JSON.stringify({ title: name, ...json }, null, 2)}\n`);
}

// HTTP bodies and entities, one file with a definition per schema name.
const defs: Record<string, unknown> = {};
for (const [name, schema] of Object.entries(schemas)) {
  if (name in files) continue;
  const { $schema: _, ...json } = z.toJSONSchema(schema, { io: "input", unrepresentable: "any" }) as Record<string, unknown>;
  defs[name] = json;
}
writeFileSync(
  join(out, "http.schema.json"),
  `${JSON.stringify({ $schema: "https://json-schema.org/draft/2020-12/schema", title: "Entities and HTTP bodies", $defs: defs }, null, 2)}\n`,
);
console.log(`wrote JSON Schema to ${out}`);
