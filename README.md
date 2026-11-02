# team-chat

A desktop work chat for macOS where people and bots are equal members. The product name is a
variable in `product.json`; this repository keeps the neutral codename `team-chat`.

- Architecture and milestones: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Wire protocol: [docs/protocol.md](docs/protocol.md)
- Rules for contributors and coding agents: [AGENTS.md](AGENTS.md)

## Quick start

Needs rustup (the toolchain in `rust-toolchain.toml`), bun 1.3 and the Xcode Command Line Tools.

```sh
bun install --frozen-lockfile

# Server: local Worker with local D1, R2 and Durable Objects
cd server
bunx wrangler d1 migrations apply DB --local
bun scripts/admin.ts local-secret --env local        # writes server/.dev.vars (ADMIN_SECRET)
bunx wrangler dev --port 8789 --inspector-port 9789
cd ..
bun server/scripts/admin.ts bootstrap --env local --url http://127.0.0.1:8789 --name <owner>

# Desktop app (fake data; --signed-out starts at the login view)
cargo run -p desktop --release
# Release DMG: dist/<name>-<version>.dmg
scripts/build-dmg.sh && scripts/test-dmg.sh
```

Tests:

| What | Command |
| --- | --- |
| Server | `cd server && bun run typecheck && bun run test` |
| Packages | `scripts/ci-run.sh typecheck packages/*` and `scripts/ci-run.sh test packages/*` |
| Monitor | `cd ops/monitor && bunx vitest run && bunx tsc --noEmit` |
| Rust | `cargo test -p chat-protocol -p chat-core -p daemon -p desktop -p artifact-view -p e2e` and `cargo clippy --workspace --all-targets -- -D warnings` |
| Desktop self-tests | `desktop --selftest-smoke`, `--selftest-timeline 10000`, `--selftest-cards`, `--screenshot <dir> --theme both [--worst-case]` (see [apps/desktop/README.md](apps/desktop/README.md)) |
| End to end | `cargo run -p e2e -- --base-url <url> --secrets-dir <dir>` (see [docs/ENVIRONMENTS.md](docs/ENVIRONMENTS.md)) |

Deployed environments, secrets and the deploy steps: [docs/ENVIRONMENTS.md](docs/ENVIRONMENTS.md).
Incidents and rollback: [docs/RUNBOOK.md](docs/RUNBOOK.md).
