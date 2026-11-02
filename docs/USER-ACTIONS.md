# What only Joye can do

Production is set up (see [ENVIRONMENTS.md](ENVIRONMENTS.md#production-setup-2026-11-03)). These
steps need Joye: his Mac, his Mac mini, his say-so, or people only he talks to. Do them in this
order. Run the commands from the repository root on the Mac that has
`~/.config/team-chat/production/` (the Mac where production was set up), unless a step says
Mac mini. Never paste a code, token or secret into a chat, a command line or a log: read it from
its file.

Production: `https://chat-api-prod.huangdeshiou.workers.dev`. The codes in steps 1 and 2 expire on
**2026-11-09 23:12 UTC** (7 days after they were made). If a step is later than that, make new
ones first (commands in the step).

## 1. Sign in on your Mac

1. Install the app (the 1.0.0 DMG, when it is released). Its default server is production.
2. Show your code once, in your own terminal: `cat ~/.config/team-chat/production/joye-signin-code`.
3. Type the 8 characters into the code boxes of the app and press 加入. The code is single use;
   it gives your existing member `Joye` (owner) a token for this Mac.
4. Delete the file: `rm ~/.config/team-chat/production/joye-signin-code`.

A new code (expired, used, or for another Mac):

```sh
ID=01M6F8FFH2K1FDRF123W1120YH   # Joye in production
(umask 077; curl -s -H @<(printf 'authorization: Bearer %s' "$(cat ~/.config/team-chat/production/owner-token)") \
  -H 'content-type: application/json' -d "{\"name\":\"Joye\",\"kind\":\"human\",\"level\":\"owner\",\"memberId\":\"$ID\"}" \
  https://chat-api-prod.huangdeshiou.workers.dev/v1/invites | jq -r .code > ~/.config/team-chat/production/joye-signin-code)
```

## 2. Send each intern their invite

There are four invites, 实习生1 to 实习生4 (human, member; each intern changes the name in the app
later). For intern `n`:

- `~/.config/team-chat/production/invites/实习生<n>.code`: the 8-character code.
- `~/.config/team-chat/production/invites/实习生<n>.link`: the download link
  `https://chat-api-prod.huangdeshiou.workers.dev/download#code=<code>`. The code is in the
  fragment, so it is not sent in the URL to the server or stored in logs.

1. Wait until the 1.0.0 release is uploaded to production (`GET /v1/updates/latest` answers 200
   for a member token). Before that, the link opens the page but has no DMG to give.
2. Send each intern their own link and code in a private message (not in a group). One code per
   person; a code works once.
3. The intern opens the link, downloads the DMG, installs the app, and types the code.
4. Check: `bun server/scripts/admin.ts members --env production` lists the intern as `human member`.
5. Delete the files you sent.

A new invite: `bun server/scripts/admin.ts invite --env production --name 实习生<n>` (writes the
code to `~/.config/team-chat/production/invite-*.txt`; the link is
`https://chat-api-prod.huangdeshiou.workers.dev/download#code=<code>`).

## 3. Install the daemon on the Mac mini

The daemon runs Jojo's coding-CLI runs. Jojo is set up as runner-backed with the label `mac-mini`
but **disabled**, because its runtime is not chosen yet. Step 3.6 chooses it and enables it.

1. On the Mac mini: install the coding CLI for Jojo and sign in to it: `claude` (Claude Code) or
   `codex` (Codex CLI). Check that it runs in a Terminal (`claude --version` or `codex --version`).
2. On this Mac, build the daemon (release, signed ad hoc):

   ```sh
   . "$HOME/.cargo/env"; scripts/build-daemon.sh        # prints dist/team-chat-daemon-<version>-aarch64-apple-darwin
   ```

3. Register the machine (owner token; writes the machine token, mode 600, and prints its path):

   ```sh
   bun server/scripts/admin.ts machine --env production --name mac-mini --labels mac-mini
   # machine 01... (mac-mini)
   # machine token: ~/.config/team-chat/production/machine-mac-mini-<id>.token
   ```

4. Copy the binary and the token to the Mac mini, and put the config there:

   ```sh
   scp dist/team-chat-daemon-*-aarch64-apple-darwin mini:~/bin/team-chat-daemon
   ssh mini 'mkdir -p -m 700 ~/.config/team-chat/production'
   scp -p ~/.config/team-chat/production/machine-mac-mini-*.token mini:~/.config/team-chat/production/machine-token
   ssh mini 'chmod 600 ~/.config/team-chat/production/machine-token'
   ```

   On the Mac mini, write `~/.config/team-chat/daemon.toml` (`~/bin/team-chat-daemon example-config`
   prints a full example):

   ```toml
   server_url = "https://chat-api-prod.huangdeshiou.workers.dev"
   token_file = "~/.config/team-chat/production/machine-token"
   name = "mac-mini"
   labels = ["mac-mini"]
   work_root = "~/team-chat-runs"
   max_parallel_runs = 2
   keep_runs_hours = 72
   min_free_gb = 10
   stall_minutes = 5
   repo_roots = []
   env_passthrough = []

   [runtimes.claude]          # keep the one you chose in step 1 (or both)
   kind = "claude"
   permission_mode = "acceptEdits"

   [runtimes.codex]
   kind = "codex"
   sandbox = "workspace-write"
   approval_policy = "never"
   ```

5. On the Mac mini, check, then install the launchd service (it starts at login and restarts by
   itself; logs in `~/Library/Logs`):

   ```sh
   ~/bin/team-chat-daemon doctor            # every line ok; exit 0. It never prints the token.
   ~/bin/team-chat-daemon install-service   # installed ~/Library/LaunchAgents/co.joye.teamchat.daemon.plist
   ```

   The config path is `~/.config/team-chat/daemon.toml` (or `--config <file>`). Install the
   service from a Terminal where `claude` or `codex` is on `PATH`: the service keeps the `PATH` of
   the install. Logs: `~/Library/Logs/team-chat-daemon/`.
6. On this Mac, choose Jojo's runtime and enable the runner (`claude` or `codex`, the name in
   `[runtimes.<name>]`):

   ```sh
   bun server/scripts/admin.ts runner --env production --bot 01M6F8JH9Q1QTCHXW3H746P1WE --runtime claude --labels mac-mini
   bun server/scripts/admin.ts machines --env production   # mac-mini: healthy
   ```

   Add `--repo-url <git url>` or `--repo-path <path on the Mac mini>` when Jojo must work in a
   repository. Then write `@Jojo` in a topic of 全员: Jojo's reply streams in.
7. Change `runner.runtime` in `assets/bots/roster.json` from `null` to the runtime you chose.

## 4. Connect the other roster bots

Every roster bot has a member and a token in production (`~/.config/team-chat/production/bots/<name>.token`)
and in staging (`~/.config/team-chat/staging/bots/<name>.token`). A bot is offline until a
process connects with its token. For each of Tata, Pocket, Forge, Watch, Scout, Ledger and Taro,
choose one:

- **Runner-backed** (runs a coding CLI on the Mac mini, like Jojo): after step 3,
  `bun server/scripts/admin.ts runner --env production --bot <id> --runtime <claude|codex> --labels mac-mini`.
  The ids are in `bun server/scripts/admin.ts members --env production`.
- **An existing bot process** (for example a bot that already does this work for qqbot): give it
  the production URL and its token file and use the bot SDK (`packages/bot-sdk`, see
  [bots.md](bots.md)):

  ```sh
  CHAT_URL=https://chat-api-prod.huangdeshiou.workers.dev \
  CHAT_TOKEN_FILE=~/.config/team-chat/production/bots/Forge.token \
  bun examples/echo-bot.ts            # the SDK echo example; your bot uses Bot from @chat/bot-sdk
  ```

  Copy the token file to the computer that runs the bot with `scp -p` and keep it mode 600.

Try a bot in staging first with its staging token and `CHAT_URL=https://chat-api-staging.huangdeshiou.workers.dev`.

## 5. Add the GitHub webhook (after you say yes)

The production GitHub hook posts pull requests, reviews, check suites and workflow runs of
`Joye-Co/team-chat` into the `开发` group. The hook exists in production; the webhook on GitHub
is not added yet. When you agree:

```sh
scripts/github-webhook.sh
# webhook <id> added on Joye-Co/team-chat (events: pull_request, pull_request_review, check_suite, workflow_run)
# ping delivery: ok (200 OK)
```

It reads `~/.config/team-chat/production/github-hook.json`, never prints the secret, and fails
if GitHub's ping delivery did not get 200. Running it again does not add a second webhook.

## 6. Adjust the roster (optional)

[ROSTER.md](ROSTER.md) and `assets/bots/roster.json` are a first version. To change a bot's
character, 岗位 or level, change both files and run `set-member` in each env (commands in
ROSTER.md). Group membership is changed in the app or with `POST /v1/groups/:id/members`.

## 7. Optional

- **Custom domain**: add a route such as `chat.<your domain>` to `chat-api-prod` in the
  Cloudflare dashboard (Workers, `chat-api-prod`, Settings, Domains and Routes), then change
  `product.json` `servers.production`, the monitor's `TARGETS` `baseUrl` and these docs, and
  release the app again (the URL is built into the app). Make new invite links: old links keep
  the old host, which still works while the workers.dev route is on.
- **R2 location**: `chat-files-prod` was made before production; its location was not checked
  (the wrangler login has no R2 scope; look in the dashboard, R2, `chat-files-prod`, Settings).
  If it is far from the team: a bucket cannot move, to move it, make a new bucket with a location hint
  near the team (for example `apac`) in the dashboard, copy every object, change
  `server/wrangler.jsonc` `env.production.r2_buckets` and deploy. Do it before files pile up.
- **Email alerts**: the monitor only posts to `运维`. For mail too, add a Cloudflare Notification
  (dashboard, Notifications) for Workers errors of `chat-api-prod` and `chat-monitor`.
- **GitHub Actions minutes**: the repository is private, so CI uses the org's included minutes.
  Check usage in the Joye-Co org settings (Billing) and set a spending limit.
