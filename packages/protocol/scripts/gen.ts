// One command for everything generated from the protocol package: protocol/fixtures and
// protocol/schema. `bun run gen:protocol` writes both; `bun run gen:protocol --check` only
// compares (CI runs the same comparison in test/generated.test.ts).
import { fixtures, fixturesRoot } from "./gen-fixtures.ts";
import { schemaFiles, schemaRoot } from "./gen-schema.ts";
import { run } from "./generated.ts";

run("fixtures", fixturesRoot, fixtures);
run("schema files", schemaRoot, schemaFiles);
