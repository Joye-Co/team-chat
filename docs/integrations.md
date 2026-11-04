# Integrations

Incoming hooks let scripts, CI and GitHub post into a group. Each hook posts as its own bot
member, so its messages show the hook name as the author. The wire contract is in
[protocol.md](protocol.md#integrations); the code is in `server/src/integrations/`.

## Server setup

Hook secrets are stored AES-GCM encrypted with the Worker secret `HOOK_SECRET_KEY` (GitHub signs
with the raw secret, so a digest is not enough). Set it once per environment, before the first
hook is made. Do not change it later: hooks made with the old key stop working.

```sh
cd server
openssl rand -base64 32 | bunx wrangler secret put HOOK_SECRET_KEY --env staging
```

For `wrangler dev`, add a line `HOOK_SECRET_KEY=<random text, 16 characters or more>` to
`server/.dev.vars`. Without the key, `POST /v1/hooks` answers 500 `internal`.

## Add a hook

Owners manage hooks with a bearer token (`OWNER_TOKEN` below).

```sh
API=https://chat-api-staging.example.workers.dev
curl -sS "$API/v1/hooks" -H "Authorization: Bearer $OWNER_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Deploys","groupId":"<groupId>","kind":"generic"}'
```

The response is `{ hook, url, secret }`. The secret is shown only once: store it in the sender's
secret store (for example a GitHub Actions secret), never in a repository or a log.

- `kind`: `generic` (posts to `url` = `/v1/hooks/:id/in`) or `github` (`/v1/hooks/:id/github`).
- `topicId` (optional, generic only): the default topic for posts that name no topic.
- The server makes a bot member with the hook name and adds it to the group.
- `GET /v1/hooks` lists hooks (never the secrets). `DELETE /v1/hooks/:id` deletes the hook and
  deactivates its member; old messages stay. To rotate a secret, make a new hook and delete the
  old one.

## Generic hook

`POST <url>` with the header `X-Hook-Secret: <secret>` and a JSON body:

| Field | Use |
| --- | --- |
| `topicId` | Post into this topic of the group. |
| `topicKey` | Your own key (1..128 of `A-Z a-z 0-9 _ . : # / -`). The first post with a new key starts a topic; later posts with the key go to that topic. Keys are per hook. Not with `topicId`. |
| `title` | Title of a topic this post starts. Default: the start of the body. |
| `body` | Markdown text. `body` (not empty) or `card` is required. |
| `card` | A card (see protocol.md, "Cards"), at most 64 KiB. With `body: ""` the card title is the text. |
| `clientMsgId` | Dedupe key, 1..64 characters. A retry with the same key returns the first message with `duplicate: true`. |

The header `Idempotency-Key` sets `clientMsgId` when the body has none. With neither, every
request makes a new message. With no `topicId`, no `topicKey` and no default topic, each post
starts a new topic.

The answer is the `SendResult` of a normal send (`{ message, topic?, duplicate? }`). The post goes
through the same checks and the same fan-out as a message sent in the app. Errors use the normal
error body: 401 wrong secret, 404 unknown hook or topic, 400 invalid body, 413 card too large,
429 more than about 20 posts per second.

```sh
curl -sS "$HOOK_URL" \
  -H "X-Hook-Secret: $HOOK_SECRET" \
  -H 'Content-Type: application/json' \
  -H "Idempotency-Key: deploy-$BUILD_ID" \
  -d '{
    "topicKey": "deploy:production",
    "title": "Production deploys",
    "body": "",
    "card": {
      "v": 1,
      "title": "Deploy 1.4.2",
      "tone": "success",
      "status": { "label": "完成" },
      "blocks": [
        { "type": "fields", "fields": [{ "label": "版本", "value": "1.4.2" }] },
        { "type": "link", "label": "日志", "target": { "kind": "url", "url": "https://ci.example.com/b/812" } }
      ]
    }
  }'
```

## GitHub webhook

1. Make a hook with `"kind": "github"` and keep `url` and `secret`.
2. In the repository (or organization): Settings, Webhooks, Add webhook. Payload URL = `url`,
   Content type = `application/json`, Secret = `secret`, SSL verification on.
3. Choose "Let me select individual events": Pull requests, Pull request reviews, Issues, and for
   CI either Check suites or Workflow runs (both work; with both, a GitHub Actions run shows as
   two lines on the card).
4. Map GitHub logins to members, so reviewers and authors can be put in `waitingOn` and as the
   topic owner:

```sh
curl -sS -X PUT "$API/v1/integrations/github/logins" -H "Authorization: Bearer $OWNER_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"logins":{"alice-gh":"<memberId of Alice>","bob-gh":"<memberId of Bob>"}}'
```

`PUT` replaces the whole map (at most 500 logins; logins are not case-sensitive); `GET` on the
same path returns it. The map is shared by all GitHub hooks. A login that is not mapped, or whose
member is not in the hook's group, is left out of `waitingOn`.

Every request must carry a valid `X-Hub-Signature-256` (HMAC-SHA256 of the raw body with the
secret, checked in constant time); otherwise 401. `X-GitHub-Delivery` ids are kept for 14 days,
so a redelivery changes nothing. Events other than the ones below answer `{ ok: true }` and are
dropped (`ping` too).

### What the adapter does

- **One topic per pull request**, key `github:<owner>/<repo>#<number>`, titled
  `<owner>/<repo>#<number> <PR title>` (the title follows PR renames). When the PR body has a
  line `Topic: <topicId>` of a topic in the hook's group, the PR uses that topic instead, and its
  title and owner stay as they are.
- **One status card per PR**, posted once and then edited: state (opened, draft, ready for review,
  review requested, approved, changes requested, merged, closed), reviewers and the CI results of
  the current head commit (`check_suite` and `workflow_run` with action `completed`; `check_run`
  shows a started check as pending until it completes). New commits
  (`synchronize`) clear the CI lines; results for an older commit are ignored. If someone deletes
  the card, the next event posts a new one in the same topic.
- **Topic status follows the PR**. The adapter sends a change only when the computed value
  changes, so a status set by hand stays until GitHub moves the PR.

| PR | Topic status | waitingOn |
| --- | --- | --- |
| merged or closed | `done` | nobody |
| draft | `in_progress` | nobody |
| reviewers requested | `waiting_review` | the requested reviewers |
| changes requested | `in_progress` | the author |
| approved | `open` | the author |
| otherwise | `open` | nobody |

  The topic owner is set once to the author's member, when the author is mapped.
- **Topic link (1.1)**: the adapter also stores `topic.linked` (docs/protocol.md, "Topic
  links") in the PR's topic with the PR's state (`open`, `draft`, `merged`, `closed`), title,
  the CI results of the head commit and its size (additions, deletions, changed files). It stores
  one only when one of these changes, so reviews alone add none. A link that cannot be stored is
  tried again on the next delivery; the card and the topic status are not held back.
- **CI** goes to the PR topic: by the `pull_requests` of the event, else by its head commit. CI
  for a commit with no known PR goes to one topic per branch, `github:<owner>/<repo>@<branch>`,
  with one card that shows the latest commit. CI never changes a topic's status.
- **Issues**: `opened` starts a topic with a card (key `github:<owner>/<repo>#<number>`),
  `closed` sets it to `done`, `reopened` back to `open`.

Notifications come from the normal rules: members in `waitingOn` get `waiting`, followers get
`result` when the topic is `done`. Card edits do not notify.

### Limits

- Deliveries for one hook run one at a time, so events that arrive together never make two topics
  for one PR. About 20 deliveries per second per hook (burst 60); more answer 429, and GitHub
  shows them as failed deliveries that you can redeliver.
- Payloads up to 5 MiB.
- Requested teams are not mapped; only requested users are.
