# Server

Cloudflare Worker `chat-api` with the Durable Objects `MemberGateway` and `GroupRoom`.
Contract: [docs/protocol.md](../docs/protocol.md). Core code: `src/core/`; feature modules live in
`src/<feature>/` and plug in through `src/core/registry.ts` (imported from `src/features.ts`).

```sh
bun install                                # at the repo root
bun scripts/admin.ts local-secret          # ADMIN_SECRET for wrangler dev (server/.dev.vars)
bun run db:migrate:local                   # D1 migrations for the local database
bun run dev                                # wrangler dev on http://127.0.0.1:8789
bun scripts/admin.ts bootstrap --env local --name Joye
bun scripts/admin.ts invite --env local --name Intern --role dev
bun run test                               # vitest in the Workers runtime
bun run typecheck
```

`admin.ts` reads `admin-secret` and `owner-token` from `~/.config/team-chat/<env>/` and writes
new tokens and invite codes there (mode 600); it prints only file paths. Base URL: `--url`, or
`product.json` `servers.<env>`, or `http://127.0.0.1:8789` for local.

Extension points (all in `src/core/registry.ts`):

| Function | What it adds |
| --- | --- |
| `registerRoute(def)` | HTTP route with `auth` none/member/owner and its own `bodyLimit` |
| `registerFrameHandler(t, h)` | WebSocket frame type handled in the gateway (schema, rate class, handler) |
| `registerFrameValidator(t \| "*", fn)` | Extra validation of an incoming frame before its handler |
| `registerRoomRpc(name, fn)` | Handler run inside a GroupRoom via `GroupRoom.rpc(groupId, name, actor, args)` |
| `registerRoomMigration(step)` / `registerGatewayMigration(step)` | Named SQLite steps (`search-1`, ...) |
| `registerMessageExtension(fn)` | Extra message fields at send time, stored in `messages.ext` |
| `registerAfterCommit(hook)` | Runs after every committed event (fan-out is not delayed) |
| `registerGatewayRpc(name, fn)` | Handler run inside a member's gateway via `MemberGateway.rpc(name, args)` |
