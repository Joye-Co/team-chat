-- Per-model prices for 用量 (server/src/usage/prices.ts): the last good copy of models.dev, USD
-- per million tokens by kind. The daily cron replaces the rows; an empty table means the built-in
-- fallback list is used.
CREATE TABLE model_prices (
  model TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  input REAL NOT NULL,
  output REAL NOT NULL,
  cache_read REAL,
  cache_write REAL,
  synced_at INTEGER NOT NULL
);
