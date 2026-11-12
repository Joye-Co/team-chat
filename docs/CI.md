# Continuous integration

CI tests the repository's own code on pull requests and pushes to `main`. It also supports
manual runs and GitHub's merge queue. Manual runs, merge queue candidates and changes to the
CI implementation run every check. Ordinary changes select jobs through
[.github/ci-paths.yml](../.github/ci-paths.yml). The selection and result policy have regression
tests: `bun run test:ci` (Bun only; no dependency install required).

## Checks and selection

| Check | What it runs | Selected by |
| --- | --- | --- |
| Workflow lint | CI regression tests, actionlint, ShellCheck for the CI shell runner | Workflows and CI support files |
| Packages | TypeScript checks, Bun tests, generated protocol/schema consistency | JS configuration, packages, protocol fixtures, machine golden frames |
| Server | TypeScript checks; Vitest in three independent shards; fresh sandbox migrations and seed in shard 1 | JS configuration, server, packages, sandbox scripts |
| Rust lint | Workspace formatting; Linux Clippy for protocol, core, daemon, update helper, e2e | Shared Rust inputs, daemon or desktop inputs |
| Rust tests | Protocol, core, daemon and update helper | Shared Rust inputs, daemon, update helper, e2e |
| Desktop macOS / Windows | Desktop tests; macOS also tests artifact-view isolation on changes to that crate, shared Rust inputs, or a full run | Shared Rust inputs, desktop, artifact-view, vendored UI, assets, embedded release notes |
| Daemon Windows | Daemon library, binaries, process tree tests; update helper | Shared Rust inputs, daemon, update helper, e2e |
| Ops | Monitor/restore TypeScript checks and tests; release/setup script guards | JS configuration, packages, ops, their server inputs, release scripts/workflows and notes |

Shared Rust inputs include `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`,
chat-protocol, chat-core and protocol fixtures. General documentation changes need no build.
Release notes are an exception: desktop `build.rs` embeds them in the app.

The Windows installer and updater are tested in Release on the files it ships. Real coding CLIs
are tested locally with `scripts/test-real-cli.sh`. CI does not deploy or use operator secrets.

## Required check

Use **CI result** as the required status check in a branch rule or ruleset. This stable name
aggregates every shard/platform without requiring conditionally skipped jobs individually.
It runs even when a dependency fails. Change detection must succeed and produce valid outputs;
every selected job must succeed. Missing jobs, unexpected skips, cancellations and failures all
fail the result. Jobs not selected may be skipped. A summary table shows the selection and
result of each job. The workflow provides this check; enabling branch protection is a separate
repository setting.

When adding a job, update `scripts/ci/check-results.mjs` and its tests along with the workflow.
The config test requires every job to be in the gate and its selection condition to agree with
the gate. Keep path filters to literals and `*` / `**` globs, which the tests exercise without
installing packages.

## Speed and reliability

- PR updates cancel obsolete runs of that PR. Each main push has its own concurrency group,
  so another push cannot cancel its pending run or force unrelated changes to wait for it.
- Bun is pinned in `.bun-version`. The shared setup action caches only downloaded packages,
  keyed by OS, architecture, Bun version and lockfile, and always performs a frozen install.
  Only pushes to main save that cache; PRs can restore it.
- Rust keeps dependency caches per platform, with separate Linux test and Clippy caches so
  the two jobs do not race to save incompatible build outputs under one key. Only main saves
  Rust caches. Debug information and incremental compilation are disabled for CI builds.
- Every job has a timeout. Server shards do not cancel one another on failure. Sandbox cleanup
  runs even when seeding fails. The package runner continues after a failed package to report
  the other results, and fails if no package defines the requested script.
- Actions use immutable commit references and Dependabot proposes grouped weekly updates.
  The actionlint executable is version pinned and verified against its recorded SHA-256 before
  execution. CI checkouts do not persist Git credentials. Release keeps its separate credential
  handling because it must publish and push.

Server shards upload JUnit XML for seven days, including on test failure. Only the explicit
report file is uploaded: sandbox credentials, `.dev.vars`, databases and the rest of `.tmp/`
are never included. For a failed seed, the job prints the last 200 lines of its server log.

## Local verification

```sh
bun install --frozen-lockfile
bun run test:ci
actionlint -shellcheck= -pyflakes=
shellcheck -x scripts/ci-run.sh
scripts/ci-run.sh typecheck packages/* ops/* server
# Use only public repository config, like the hosted runners (no private ops checkout).
mkdir -p .tmp/ci/no-ops
TC_OPS_DIR="$PWD/.tmp/ci/no-ops" scripts/ci-run.sh test packages/* ops/*
# From server/: the same shard and report options as CI.
bun run test --shard=1/3 --reporter=default --reporter=junit --outputFile.junit=../.tmp/test-results/server-1.xml
```

For local Rust builds, follow the target directory, sccache and free-space rules in AGENTS.md.
Measure hosted runner time after caches are populated; a changed cache key starts cold, so its
first run is not a useful speed comparison.
