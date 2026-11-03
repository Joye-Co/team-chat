# Changelog

Each release is tagged `v<version>` and has a GitHub Release with the DMG, the machine daemon
and their `.sha256` files. Release notes for the team are in Chinese in `docs/releases/`.

## 1.0.0 (2026-11-03)

The first production release, for daily use by 5 people and 5 bots. Notes:
[docs/releases/1.0.0.md](docs/releases/1.0.0.md).

- Chat: groups, DMs, topics with status and `waitingOn`, @mentions, quotes, edit and delete,
  per-topic read positions, presence, typing, 待办, quiet notifications, group rename.
- Bots: bot tokens and the TypeScript bot SDK; runner-backed bots through the machine daemon,
  with streaming replies, captured runs and the process view.
- 产出: files and images up to 100 MB, HTML pages in an isolated window with versions.
- Search with Chinese two-character words, in one group and across all groups.
- Integrations: incoming webhooks and a GitHub adapter that posts PR, review and CI cards.
- Operations: separate staging and production, a monitor with alerts into 运维, a daily R2
  archive and a tested restore tool.
- Distribution: DMG on GitHub Releases and on the server, invite download links, in-app update.
- Signing: the app, its bundled daemon and the DMG are signed with one self-signed certificate,
  so the designated requirement (`identifier "co.joye.teamchat" and certificate leaf = H"…"`)
  stays the same across builds and the Keychain keeps trusting the app after an update. The
  updater refuses an update whose signature does not meet the running app's designated
  requirement (an ad-hoc running app, such as a 0.x build, accepts any valid update).
- `/v1/health` reports the release version.
