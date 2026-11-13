-- How many runs a machine may do at once, set in the app (docs/protocol.md, `Machine.parallelRuns`).
-- NULL = what the daemon's own config says (`hello.machine.maxParallelRuns`).
ALTER TABLE machines ADD COLUMN parallel_runs INTEGER;
