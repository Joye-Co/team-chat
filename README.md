# team-chat

A macOS work chat for one small team, where people and bots are equal members. Every message
belongs to a topic (话题) in a group (群); bots take work when they are @mentioned, stream their
progress and deliver results (产出) into the topic. Client: Rust, GPUI and gpui-component.
Server: Cloudflare Workers, Durable Objects, D1 and R2. The product name is a variable in
`product.json`; this repository keeps the neutral codename `team-chat`.

- What we build, interface words and the V1 bar: [docs/PRODUCT.md](docs/PRODUCT.md)
- Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); wire protocol: [docs/protocol.md](docs/protocol.md)
- Visual design: [docs/DESIGN.md](docs/DESIGN.md)
- Rules for contributors and coding agents: [AGENTS.md](AGENTS.md)
- Changes per release: [CHANGELOG.md](CHANGELOG.md)

## Install the app

Latest release: <https://github.com/Joye-Co/team-chat/releases/latest> (private repository).
Interns get a download link with their invite code from Joye.

- Interns (Chinese): [docs/ONBOARDING.zh.md](docs/ONBOARDING.zh.md) (download, first open,
  sign in, 群, 话题, 待办, 产出, @ a bot, notifications, diagnostics) and
  [docs/INSTALL.zh.md](docs/INSTALL.zh.md) (install, update, uninstall).
- English: [docs/INSTALL.md](docs/INSTALL.md).

The app needs an Apple silicon Mac with macOS 13 or later. It is not notarized: the first open
goes through System Settings > Privacy & Security > Open Anyway. In-app updates do not ask again.
The default server is production (`product.json` `servers.production`).

## Environments

| Env | API | Used by |
| --- | --- | --- |
| local | `wrangler dev` on `http://127.0.0.1:<port>` | development |
| e2e | `https://chat-api-e2e.huangdeshiou.workers.dev` | `tools/e2e` runs |
| staging | `https://chat-api-staging.huangdeshiou.workers.dev` | checks before production, `--selftest-live` |
| production | `https://chat-api-prod.huangdeshiou.workers.dev` | the team |

Deploy the server with `cd server && bun run deploy:<e2e|staging|production>`
(`scripts/deploy-server.sh`: D1 migrations, then the Worker; `/v1/health` reports
`DEPLOY_VERSION`, else the `v<semver>` tag at HEAD, else the short commit). Secrets, setup and
the deploy steps: [docs/ENVIRONMENTS.md](docs/ENVIRONMENTS.md). Incidents and rollback:
[docs/RUNBOOK.md](docs/RUNBOOK.md). What only Joye can do: [docs/USER-ACTIONS.md](docs/USER-ACTIONS.md).

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

## Release

On the Mac that holds `~/.config/team-chat/production/admin-secret` and the signing files:

```sh
scripts/release.sh 1.2.3 --dry-run                      # checks only; prints the plan
scripts/release.sh 1.2.3 --env production --notes docs/releases/1.2.3.md
scripts/release.sh 1.2.3 --env staging --upload-only    # the same DMG for staging
```

`release.sh` runs in any worktree whose HEAD is `origin/main` and whose tree is clean. It sets
the workspace version (Cargo), builds and checks the DMG (`scripts/build-dmg.sh`,
`scripts/test-dmg.sh`) and the daemon, tags `v<version>`, creates the GitHub Release with the
DMG, the daemon and their `.sha256` files, and uploads the DMG to the server
(`PUT /v1/admin/releases/<version>`), which offers it as the in-app update and on invite
download links. Write the notes in Chinese in `docs/releases/<version>.md` and add a
`CHANGELOG.md` entry. After a release, deploy the server so `/v1/health` shows the version.

**Signing.** `build-dmg.sh` signs the app, the bundled daemon and the DMG with rcodesign
(`cargo install apple-codesign --locked`) and the self-signed certificate in
`~/.config/team-chat/signing/` (`key.pem`, `cert.pem`; directory mode 700, files mode 600;
CN "TeamChat Self-Signed", code signing, valid until 2036). No keychain is used. The designated
requirement is `identifier "co.joye.teamchat" and certificate leaf = H"<sha1 of the
certificate>"`, the same for every build, so the Keychain keeps trusting the app after an
update, and the updater refuses an update that does not meet it. Keep a private backup of the
two files: a new certificate gives a new requirement, and installed apps would then refuse the
in-app update (install that one by hand). Without the files, or with `SIGNING=adhoc`, the build
is signed ad hoc. `scripts/test-dmg.sh <dmg> --same-as <older dmg>` checks that two builds
have the same requirement.
