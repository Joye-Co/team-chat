# V1 checklist

The V1 bar of [PRODUCT.md](PRODUCT.md#v1-production-use-by-5-people-and-5-bots), checked on
2026-11-03 by the final-check track against `main` at `6ec0691` (server deployed to e2e and
staging from that commit). Each line is **pass**, **fail** or **not checked**, with its evidence.

Evidence folders (not in git):

- `R` = `~/.cache/team-chat/reports/final/` (JSON reports of this check, no secrets)
- `S` = `~/.cache/team-chat/screens/final/` (screenshot self-test captures, contact sheets, boards)
- e2e run ids: `kthm42` and `sz8xgx` (all scenarios, before and after the last deploy), `p24gpf`
  (30-minute soak), all against
  the e2e env (`https://chat-api-e2e.<your-subdomain>.workers.dev`)

## 1. Chat

| Item | Result | Evidence |
| --- | --- | --- |
| Groups, DMs, topics with status and `waitingOn` | pass | `cd server && bun run test` (17 files, 158 tests); e2e `chat` (`R/kthm42-chat.json`); screens `S/topic-*.png`, `S/start-dm-*.png`, `S/new-group-*.png` |
| @mentions, quotes, edit and delete | pass | server tests; desktop `cargo test -p desktop` (186 pass); e2e `streaming` (edit) |
| Per-topic unread and read positions | pass | server tests (topic reads); `views::topic_list::tests::filters_follow_unread_and_todo`; restore drill keeps read cursors (`R/drill-report.json`) |
| Presence, typing | pass | screens `S/topic-worst-*-worst.png` (typing line), settings 成员 status; server tests |
| 待办 | pass | `S/todo-*.png`; topic list filter test |
| Quiet notifications (mention, waits on me, followed result; mutes) | pass (unit level) | desktop tests; no live notification run in this check |
| No lost or duplicated messages across reconnects | pass | e2e `catchup`, `idempotent`, `gap`, `pipeline` (`R/kthm42-*.json`); 30-min soak: 4,477 messages, 125 network blips, 107 restarts, all 5 caches equal the server log, no errors (`R/p24gpf-soak.json`) |
| Sleep and network changes on a real Mac | not checked | the soak simulates blips and restarts, not macOS sleep |

## 2. Performance

| Item | Result | Evidence |
| --- | --- | --- |
| p95 delivery under 300 ms (same region) | pass | e2e `load` on e2e: 9,000 deliveries, p50 60.8 ms, p95 152.0 ms, p99 491.3 ms (`R/kthm42-load.json`) |
| 10,000-message topic opens under 100 ms from cache, scrolls at 60 fps | pass | release binary `--selftest-timeline 10000`: first frame 12.9 ms, frame p95 10.1 ms (target 16) (`R/release-timeline.json`); DMG binary: 11.1 ms, p95 10.1 ms (`R/dmg-timeline.json`) |
| Cold start to usable under 1 s | pass | DMG `--selftest-smoke`: first frame 636 ms including process start (`R/dmg-smoke.json`) |
| Catch-up after a day offline under 2 s | not checked | only a 50-message catch-up was measured: 725 ms (`R/kthm42-catchup.json`) |
| Search under 300 ms | not checked | search works end to end (`R/search-e2e.log`), latency was not measured |

## 3. Bots

| Item | Result | Evidence |
| --- | --- | --- |
| External bots with a bot token and the TypeScript SDK | pass | `examples/echo-bot.ts` on e2e: reply in 196 ms, streamed reply (`R/remote-smoke-e2e.json`); `packages/bot-sdk` tests |
| Runner-backed bot through the daemon: ack once, stream, result, topic done | pass | `apps/daemon/e2e/local-e2e.ts` on e2e with the fake Claude Code CLI (`R/daemon-e2e-report.json`) |
| Run captured (normalized events and raw session in R2), visible in the process view | pass | same report (run log and gzip session checked); `apps/desktop/e2e/search-process-e2e.ts` (`R/search-e2e.log`); screens `S/process-*.png`, `S/panel-*.png` |
| Stall detection | pass (unit level) | server bot tests; not run live |
| Follow-ups in the topic | not checked live | server tests only |

## 4. 产出

| Item | Result | Evidence |
| --- | --- | --- |
| Files and images up to 100 MB, inline previews | pass (unit level) | chat-core upload test (`upload_streams_with_hash_and_download_is_verified_and_cached`), server file tests; `S/files-*.png`; a 100 MB upload was not run |
| HTML pages in the isolated window with versions | pass | artifact-view tests (7), `file_artifacts_that_are_pages_open_in_the_isolated_window`, restore drill rebuilds 产出 versions |

## 5. Search

| Item | Result | Evidence |
| --- | --- | --- |
| Chinese two-character words, inside one 群 and across mine | pass | `packages/search` tests (410 pass), chat-core `tests/search.rs`, `search_live` hit (`R/search-e2e.log`), `S/search-*.png` |

## 6. Integrations

| Item | Result | Evidence |
| --- | --- | --- |
| Incoming webhook API | pass | server integration tests |
| GitHub adapter: PR, review and CI events into cards | pass | signed `pull_request` opened, `check_suite` completed and `closed` (merged) on e2e: card posted and edited to 已合并; bad signature 401 (`R/remote-smoke-e2e.json`) |

## 7. Operations

| Item | Result | Evidence |
| --- | --- | --- |
| Separate staging and production | fail | staging runs; production is not set up (no `chat-prod` D1, no Worker) |
| Workers logs with secrets redacted | pass | server test "logs carry no secrets"; daemon `redact` tests |
| Monitor every 5 minutes with alerts into an ops 群 | fail | `ops/monitor` tests pass, but the `chat-monitor` Worker is not deployed |
| Daily R2 archive of every group log and of D1 | pass on e2e | `POST /v1/admin/archive/run` on e2e; staging reports `lastRun: null` (no archive pass yet after this deploy) |
| Restore tool and runbook tested | pass | local drill `bun ops/restore/drill.ts --port 8807`: ok, 84 events, 3 groups, 产出 versions, no difference (`R/drill-report.json`); one e2e group restored into a fresh e2e group: 335 events, last seq 335, same seqs and types (`R/restore-e2e-report.json`) |

## 8. Distribution

| Item | Result | Evidence |
| --- | --- | --- |
| DMG builds and passes its checks | pass | `scripts/build-dmg.sh`, `scripts/test-dmg.sh` (19 checks ok, `R/test-dmg.log`) |
| DMG on GitHub Releases | fail | `gh release list` shows no release |
| DMG served by the server | fail | staging `GET /v1/updates/latest` has no release |
| In-app update | pass (unit level) | `updater::install::tests::install_from_dmg_and_relaunch` and the other updater tests; no live update from a server |
| Chinese install and onboarding guides | pass | `docs/INSTALL.zh.md` |
| DMG binary against staging | pass | `--selftest-live` with the test bot `selftest-final`: ack 63 ms, event 63 ms, restore from cache (`R/dmg-live.json`) |

## 9. Security

| Item | Result | Evidence |
| --- | --- | --- |
| Hashed tokens and invite codes | pass | server tests; D1 dump keeps hashes only (ops test) |
| Authorization on every frame and route | pass | e2e `permissions`, server tests |
| Sandboxed 产出 | pass with gap | artifact-view CSP and content rule tests; WebRTC/STUN and DNS prefetch are not covered (finding security-8) |
| Rate limits | pass with gaps | server limits for writes and invite redeem; hello frames and file creation have no limit (findings security-9, security-10) |
| Audited admin actions | pass | server tests |
| Webhook signatures checked | pass | GitHub bad signature answers 401 on e2e (`R/remote-smoke-e2e.json`) |
| Earlier confirmed findings (security-1 to -10, server-data-*, client-*, product-*) | not checked | this check did not re-verify each finding one by one |

## 10. Design

| Item | Result | Evidence |
| --- | --- | --- |
| Every scene in light, dark and worst case | pass | 208 captures in `S/`; contact sheets `S/contact-light.png`, `S/contact-dark.png`, `S/contact-worst.png`; the app was never frontmost during the runs (checked with `lsappinfo front` every 0.3 s) |
| Matches the mockups | pass with small gaps | boards in `S/boards/` (main, short-topic, worst-case at 1100 × 680, switcher, switcher-query, settings-members, login at 960 × 640, each in light and dark) |
| Every control from gpui-component, per DESIGN.md | pass | `cargo test -p desktop` (catalog and UI rules); review of the captures |
| Checked with the four Emil Kowalski skills | not checked in this pass | earlier review captures in `~/.cache/team-chat/screens/review-design-*` |

Small design gaps seen in the boards: at 1100 × 680 with worst-case data, a faint stub of the cut
start block shows under the topic header, and the last 私聊 row is cut by the sidebar footer.
