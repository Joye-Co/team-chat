// Writes JSON Schema files for the main unions into protocol/schema/. Run with
// `bun run gen:protocol` at the repo root (or `bun run gen:schema`); `--check` only compares.
// The daemon builds CardV2.schema.json and PatchOp.schema.json in (`chat-run card schema`); the
// rest documents the wire for readers outside TypeScript. test/generated.test.ts fails when the
// folder is out of date.
import { join } from "node:path";
import { z } from "zod";
import { CardV2Shape, MAX_PATCH_OPS, PatchOp } from "../src/card2.ts";
import { schemas } from "../src/index.ts";
import { BASE_DOCS, BLOCK_DOCS, CARD_DOCS, OP_DOCS } from "./card-docs.ts";
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

// The Card v2 catalog that `chat-run card schema` reads (the daemon embeds this file) and the
// patch ops of `chat-run card patch`. The shape only: the write rules (ids, limits, one decision
// surface) are checked by the server. Each block type is `$defs.<type>` and `$defs.block` is
// their union; a part that is written more than once is a `$defs` entry too, so the file stays
// small. The descriptions come from card-docs.ts.
type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
type Obj = { [k: string]: Json };
const isObj = (v: Json | undefined): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
/** Zod writes `z.json()` as a definition `__schema<n>` that names itself; here it is `json`. */
const JSON_REF = "#/$defs/json";
let jsonDef: Json | undefined;
/** The schema without `$schema` and `$defs`; `z.json()` is a `$ref` to `JSON_REF`. */
const plain = (schema: z.ZodType): Obj => {
  const { $schema: _, $defs = {}, ...json } = z.toJSONSchema(schema, { io: "input", unrepresentable: "any" }) as Obj;
  const rename = (v: Json): Json =>
    Array.isArray(v)
      ? v.map(rename)
      : isObj(v)
        ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, k === "$ref" && typeof x === "string" && x.startsWith("#/$defs/__schema") ? JSON_REF : rename(x)]))
        : v;
  for (const [name, def] of Object.entries($defs as Obj)) {
    const renamed = rename(def);
    if (!name.startsWith("__schema") || (jsonDef !== undefined && JSON.stringify(jsonDef) !== JSON.stringify(renamed))) throw new Error(`gen-schema.ts: unexpected definition ${name}`);
    jsonDef = renamed;
  }
  return rename(json) as Obj;
};
/** Every sub-schema equal to a key of `known` becomes a `$ref` to its pointer. */
function refer(v: Json, known: Map<string, string>, top = true): Json {
  if (Array.isArray(v)) return v.map((x) => refer(x, known, false));
  if (!isObj(v)) return v;
  const ref = top ? undefined : known.get(JSON.stringify(v));
  if (ref !== undefined) return { $ref: ref };
  return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, refer(x, known, false)]));
}
/** The schema of a field path of card-docs.ts (`a.b`, through arrays). */
function fieldAt(schema: Obj, path: string, where: string): Obj {
  let at = schema;
  for (const key of path.split(".")) {
    while (isObj(at.items)) at = at.items;
    const next = isObj(at.properties) ? at.properties[key] : undefined;
    if (!isObj(next)) throw new Error(`card-docs.ts: ${where} has no field ${path}`);
    at = next;
  }
  return at;
}
function describe(schema: Obj, docs: Record<string, string>, where: string) {
  for (const [path, text] of Object.entries(docs)) fieldAt(schema, path, where).description = text;
}
/**
 * Moves every object schema written more than once (and worth a name) into `defs`, the largest
 * first, named by the field it is the value of.
 */
function share(root: Obj, defs: Obj) {
  const MIN = 90;
  for (;;) {
    const seen = new Map<string, { n: number; key: string }>();
    // The schemas inside `v`; the map of a `properties` is not a schema itself.
    const inside = (v: Obj, key: string) => {
      for (const [k, x] of Object.entries(v)) {
        if (k === "properties" && isObj(x)) for (const [field, y] of Object.entries(x)) walk(y, field);
        else walk(x, k === "items" || k === "oneOf" || k === "anyOf" ? key : k);
      }
    };
    const walk = (v: Json, key: string) => {
      if (Array.isArray(v)) return v.forEach((x) => walk(x, key));
      if (!isObj(v)) return;
      if (key !== "" && !("$ref" in v)) {
        const text = JSON.stringify(v);
        if (text.length >= MIN) seen.set(text, { n: (seen.get(text)?.n ?? 0) + 1, key: seen.get(text)?.key ?? key });
      }
      inside(v, key);
    };
    // A whole definition is not a candidate itself.
    for (const holder of [root, ...Object.values(defs).filter(isObj)]) inside(holder, "");
    const best = [...seen].filter(([, s]) => s.n > 1).sort((a, b) => b[0].length - a[0].length || (a[0] < b[0] ? -1 : 1))[0];
    if (!best) return;
    let name = best[1].key;
    for (let i = 2; name in defs; i++) name = `${best[1].key}${i}`;
    const known = new Map([[best[0], `#/$defs/${name}`]]);
    for (const [k, x] of Object.entries(root)) if (k !== "$defs") root[k] = refer(x, known, false);
    for (const [k, x] of Object.entries(defs)) defs[k] = refer(x, known);
    defs[name] = JSON.parse(best[0]) as Json;
  }
}

const rawCard = plain(CardV2Shape);
const rawBlocks = fieldAt(rawCard, "blocks", "the card").items as Obj;
const blockList = rawBlocks.oneOf as Obj[];
const typeOf = (b: Obj) => String(((b.properties as Obj).type as Obj).const);
// Raw sub-schemas of the card that the ops schema points at, before anything is described.
const cardParts = new Map<string, string>([[JSON.stringify(rawBlocks), "CardV2.schema.json#/$defs/block"]]);
for (const k of ["ask", "actions", "intent", "status"]) cardParts.set(JSON.stringify((rawCard.properties as Obj)[k]), `CardV2.schema.json#/properties/${k}`);

const blockRefs = new Map(blockList.map((b) => [JSON.stringify(b), `#/$defs/${typeOf(b)}`]));
const cardDefs: Obj = {};
for (const b of blockList) cardDefs[typeOf(b)] = refer(structuredClone(b), blockRefs);
cardDefs.json = jsonDef as Json;
cardDefs.block = { oneOf: blockList.map((b) => ({ $ref: `#/$defs/${typeOf(b)}` })) };
const cardSchema = refer(rawCard, new Map([...blockRefs, [JSON.stringify(rawBlocks), "#/$defs/block"]])) as Obj;
describe(cardSchema, CARD_DOCS, "the card");
for (const b of blockList) {
  const type = typeOf(b);
  const docs = BLOCK_DOCS[type];
  if (!docs) throw new Error(`card-docs.ts: no entry for the block type ${type}`);
  const def = cardDefs[type] as Obj;
  def.description = docs.is;
  describe(def, { ...BASE_DOCS, ...docs.fields }, type);
}
for (const type of Object.keys(BLOCK_DOCS)) if (!blockList.some((b) => typeOf(b) === type)) throw new Error(`card-docs.ts: ${type} is not a block type`);
share(cardSchema, cardDefs);
const pretty = (v: unknown) => `${JSON.stringify(v, null, 2)}\n`;
const DRAFT = "https://json-schema.org/draft/2020-12/schema";
schemaFiles.set("CardV2.schema.json", pretty({ $schema: DRAFT, title: "CardV2", ...cardSchema, $defs: cardDefs }));

const opList = refer(plain(z.array(PatchOp).min(1).max(MAX_PATCH_OPS)), cardParts) as Obj;
for (const op of (opList.items as Obj).oneOf as Obj[]) {
  const name = String(((op.properties as Obj).op as Obj).const);
  const text = OP_DOCS[name];
  if (!text) throw new Error(`card-docs.ts: no entry for the patch op ${name}`);
  op.description = text;
}
schemaFiles.set("PatchOp.schema.json", pretty({ $schema: DRAFT, title: "Patch ops of a card (ops.json)", ...opList, $defs: { json: jsonDef } }));

// HTTP bodies and entities, one file with a definition per schema name.
const defs: Record<string, unknown> = {};
for (const [name, schema] of Object.entries(schemas)) {
  if (name in files) continue;
  const { $schema: _, ...json } = z.toJSONSchema(schema, { io: "input", unrepresentable: "any" }) as Record<string, unknown>;
  defs[name] = json;
}
schemaFiles.set(
  "http.schema.json",
  `${JSON.stringify({ $schema: DRAFT, title: "Entities and HTTP bodies", $defs: defs }, null, 2)}\n`,
);

if (import.meta.main) run("schema files", schemaRoot, schemaFiles);
