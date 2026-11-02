// Named-migrations runner for Durable Object SQLite. Each step runs once, in order, inside a
// transaction together with its row in `migrations(name, applied_at)`. Names are unique across
// core and feature modules ("core-1", "search-1", ...), so features add steps without clashes.

export interface MigrationStep {
  /** Unique name, e.g. "core-1" or "search-1". Never rename or change a released step. */
  name: string;
  /** SQL (may hold several statements) or a function that runs synchronously on `sql`. */
  up: string | ((sql: SqlStorage) => void);
}

export function runMigrations(storage: DurableObjectStorage, steps: readonly MigrationStep[]): string[] {
  const sql = storage.sql;
  sql.exec(
    "CREATE TABLE IF NOT EXISTS migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)",
  );
  const seen = new Set<string>();
  for (const s of steps) {
    if (seen.has(s.name)) throw new Error(`duplicate migration step name: ${s.name}`);
    seen.add(s.name);
  }
  const done = new Set(sql.exec<{ name: string }>("SELECT name FROM migrations").toArray().map((r) => r.name));
  const applied: string[] = [];
  for (const step of steps) {
    if (done.has(step.name)) continue;
    storage.transactionSync(() => {
      if (typeof step.up === "string") sql.exec(step.up);
      else step.up(sql);
      sql.exec("INSERT INTO migrations (name, applied_at) VALUES (?, ?)", step.name, Date.now());
    });
    applied.push(step.name);
  }
  return applied;
}

export function appliedMigrations(storage: DurableObjectStorage): string[] {
  return storage.sql
    .exec<{ name: string }>("SELECT name FROM migrations ORDER BY rowid")
    .toArray()
    .map((r) => r.name);
}
