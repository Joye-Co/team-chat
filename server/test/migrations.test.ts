import { env, runInDurableObject } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import { GATEWAY_CORE_MIGRATIONS } from "../src/core/gateway";
import { appliedMigrations, runMigrations } from "../src/core/migrations";
import { GroupRoom, roomMigrationSteps } from "../src/core/room";

describe("Durable Object migrations", () => {
  it("migrate a GroupRoom from an empty object and re-run idempotently", async () => {
    const stub = env.GROUP_ROOM.get(env.GROUP_ROOM.idFromName(`empty-${crypto.randomUUID()}`));
    await runInDurableObject(stub, async (_room: GroupRoom, state) => {
      const registered = roomMigrationSteps().map((s) => s.name);
      expect(registered[0]).toBe("core-1");
      expect(appliedMigrations(state.storage)).toEqual(registered);
      const tables = state.storage.sql
        .exec<{ name: string }>("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .toArray()
        .map((r) => r.name);
      expect(tables).toEqual(expect.arrayContaining(["events", "topics", "messages", "mentions", "room_members", "migrations"]));
      expect(runMigrations(state.storage, roomMigrationSteps())).toEqual([]);
      // A feature step added later runs once, after the core steps.
      const steps = [
        ...roomMigrationSteps(),
        { name: "search-1", up: "CREATE TABLE search_test (id TEXT PRIMARY KEY)" },
        { name: "search-2", up: (sql: SqlStorage) => void sql.exec("INSERT INTO search_test VALUES ('x')") },
      ];
      expect(runMigrations(state.storage, steps)).toEqual(["search-1", "search-2"]);
      expect(runMigrations(state.storage, steps)).toEqual([]);
      expect(appliedMigrations(state.storage)).toEqual([...registered, "search-1", "search-2"]);
      expect(state.storage.sql.exec("SELECT count(*) AS n FROM search_test").one().n).toBe(1);
      // A failing step rolls back and is not recorded.
      const bad = [...steps, { name: "bad-1", up: "CREATE TABLE ok_t (x); SELECT * FROM missing_table" }];
      expect(() => runMigrations(state.storage, bad)).toThrow();
      expect(appliedMigrations(state.storage)).not.toContain("bad-1");
      expect(() => runMigrations(state.storage, [...steps, steps[0]])).toThrow(/duplicate/);
    });
  });

  it("migrate a MemberGateway from an empty object", async () => {
    const stub = env.MEMBER_GATEWAY.get(env.MEMBER_GATEWAY.idFromName(`empty-${crypto.randomUUID()}`));
    await runInDurableObject(stub, async (_gw, state) => {
      expect(appliedMigrations(state.storage)).toEqual(GATEWAY_CORE_MIGRATIONS.map((s) => s.name));
      expect(runMigrations(state.storage, GATEWAY_CORE_MIGRATIONS)).toEqual([]);
    });
  });
});
