# 设计：转交、分出去和接力

状态：已定，等拆 PR 实现。属于 [usable.zh.md](../usable.zh.md) 线 6；对应审计 盯 3.1–3.7、接 1.1、接 1.2、接 2.6、接 4.4。

## 1. 目标

负责人原话（2026-11-12）：

> 多个 AI 成员现在先做，接力也是要做的。这边要做到完整，接力和分发这些都要做。

2026-11-09 定的方向（不再讨论）：AI 成员之间不做任何形式的文字讨论，靠产出和状态共享上下文；不开圆桌，用接力；读上下文是渐进的（先压缩的状态，要全文再取）；痕迹有版本，现在先存父版本关系，这一版不做分叉时间线界面；「一个话题只有一个负责成员」是错的；成员之间不能无限互相交活，必须有上限。

这一版做到：一个话题里可以有几个 AI 成员同时或先后在做；转交和分出去做完整（知道有谁、对方收到什么、话题里看得见、结果怎么回来、有上限、对方忙/离线/做不了时怎样、人能叫停）；接力由人事先写好顺序，房间按状态推进；产出版本存父版本。

## 2. 现状

- **一个话题一个 AI 成员**：`bot_topics` 主键是 `topic_id`（`server/src/bots/room.ts:26-35`），`bots.claim` 用 `INSERT OR IGNORE`，先来的占住（`room.ts:150`）。投递时有占住的成员就只给它（`room.ts:528`），否则只取第一个能用的 @（`room.ts:533`）。desk 两处也按占住的成员挡：`server/src/bots/desk/runs.ts:74`、`server/src/bots/desk/front.ts:51`。线 1 正在改这段路由（@ 谁给谁，各一份）。
- **两套「负责」**：路由看 `bot_topics`；界面看 `topics.owner_id`，只有 `topic.update` 写它（`server/src/core/room.ts:1134-1138`），desk 每次收尾带 `ownerId: this.botId`（`server/src/bots/desk/core.ts:629-648`）。两个成员都在做时，先做完的会把话题标成完成。
- **卡片按钮送错人**：话题里的卡片按钮送给占住的成员，不送卡片作者（`room.ts:58-65`）。
- **转交、分出去**（`server/src/bots/desk/delegate.ts`，`docs/protocol.md:925-940`），系统提示词教了 CLI（`apps/daemon/src/chat_run.rs:92-95`）。问题：
  - 深度在交接记录里（`delegate.ts:57-59`），「最多分出 8 份」只记在交出方自己的 meta（`delegate.ts:78-80`），B 再往下分从 0 数；
  - 设计稿说「不能交回链上已有的成员」（`docs/design/agent-harness.zh.md` 3.5），代码没做；
  - 转交不带 `parent`（`delegate.ts:59`），被转交的那一份结果回不到交出方；
  - `bots.transfer` 先改归属、再让对方接（`delegate.ts:65`），对方拒绝时归属已经改了；它还只许当前占住的成员转交（`room.ts:305`）；
  - 被停下的那份不回报（`server/src/bots/desk/events.ts:453`）；
  - 回报是 desk 直接调用，失败只记日志（`delegate.ts:173-190`）；交出方有 run 但还没分到电脑时直接丢掉结果（`delegate.ts:165`，`if (active) return`）；
  - 话题里只有一句「@B 转交给你：…」（`delegate.ts:66`），没有叫停入口；
  - 转交后 A 是否停下全靠提示词。
- **房间的出件箱**（`bot_outbox`）不会一直重试：超过 `HANDOFF_GIVE_UP_MS` 就删掉、只记一条错误（`room.ts:632`）；同一话题一行失败会挡住后面所有行，包括人的消息。
- **一条消息 @ 同一个人两次只存一次**：`server/src/core/room.ts:850` 对 `mentions` 去重。
- **没有接力**。**产出没有父版本**（`docs/protocol.md:524`，`packages/protocol/src/files.ts:209`）。
- **run 记录里没有代码位置**：`BotRun`（`packages/protocol/src/bots.ts:16-53`）没有分支和提交；daemon 的分支在 `apps/daemon/src/workspace.rs:243`。
- **房间迁移在构造时跑完**（`server/src/core/room.ts:289`，`blockConcurrencyWhile` 里），所以新表在任何 RPC 之前都已存在，不需要「表不存在」的兜底。
- **人停 run** 走 `POST /v1/runs/:id/cancel`，群里任何一个人都能停（`server/src/bots/routes.ts:445-457`，desk 方法 `server/src/bots/desk/input.ts:268`）。

本设计依赖线 1 的：路由、`chat-run member list`、`chat-run topic state`、请求消息的附件和引用渲染（接 2.1）、提示词里的成员名单。不重复做。

## 3. 设计

### 3.1 概念

不加界面名词。数据里有三样：

| 概念 | 存在哪 | 用来做什么 |
| --- | --- | --- |
| 参与成员：在这个话题跑过 run 的 AI 成员，各自是否在做 | 房间新表 `topic_bots` | 话题状态汇总、详情栏、状态卡 |
| 跟进对象（`ownerId`）：最近在这个话题开始 run 的成员 | `bot_topics` 和 `topics.owner_id`，两处写同一个值 | 不带 @ 的跟进给它（第 4 节默认）；只用于路由，界面不再显示「负责」 |
| 一件事（chain）：一个人的请求和由它转交、分出去、接力出来的所有 run | 房间新表 `work_chains`、`chain_steps` | 上限、叫停、结果回哪 |

理由：负责人说「一个负责成员」是错的，界面只显示谁在做、谁参与过；`ownerId` 留作路由字段，不改协议的取值。

### 3.2 数据和存储

**群房间**，命名迁移 `participants-1`：

```sql
CREATE TABLE topic_bots (
  topic_id TEXT NOT NULL, bot_id TEXT NOT NULL,
  run_id TEXT,                         -- 最近一个 run
  working INTEGER NOT NULL DEFAULT 0,  -- 手上有没结束的 run
  report TEXT,                         -- 最近一次结束时的汇报：done | waiting_review | waiting_answer | blocked | failed | cancelled
  waiting_on TEXT NOT NULL DEFAULT '[]',
  started_at INTEGER NOT NULL, ended_at INTEGER,
  PRIMARY KEY (topic_id, bot_id));
INSERT OR IGNORE INTO topic_bots (topic_id, bot_id, started_at, ended_at)
  SELECT topic_id, bot_id, claimed_at, claimed_at FROM bot_topics;
```

命名迁移 `chains-1`：

```sql
CREATE TABLE work_chains (
  id TEXT PRIMARY KEY,                 -- 房间生成的 ulid
  topic_id TEXT NOT NULL,
  root_kind TEXT NOT NULL,             -- run | message（接力从一条消息开始）
  root_id TEXT NOT NULL,
  requester_id TEXT NOT NULL,          -- 根 run 的 requested_by，或发消息的人
  handoffs INTEGER NOT NULL DEFAULT 0, -- 成员发起的转交和分出去的次数（不含接力的步、不含重试）
  created_at INTEGER NOT NULL, stopped_at INTEGER, stopped_by TEXT);
CREATE TABLE chain_steps (
  chain_id TEXT NOT NULL, seq INTEGER NOT NULL,
  kind TEXT NOT NULL,                  -- handoff | assign | relay
  from_bot TEXT, to_bot TEXT NOT NULL, from_run TEXT, run_id TEXT,
  topic_id TEXT NOT NULL, depth INTEGER NOT NULL,
  path TEXT NOT NULL,                  -- JSON：从根到这一步经过的成员 id
  parent_run TEXT,                     -- assign：结果交回的 run
  state TEXT NOT NULL,                 -- reserved | working | done | failed | cancelled
  result TEXT,                         -- <= 4000 字
  at INTEGER NOT NULL,
  PRIMARY KEY (chain_id, seq));
CREATE INDEX chain_steps_run ON chain_steps(run_id);
CREATE INDEX chain_steps_parent ON chain_steps(parent_run, state);
```

命名迁移 `relay-1`：

```sql
CREATE TABLE topic_relays (
  id TEXT PRIMARY KEY, topic_id TEXT NOT NULL, message_id TEXT NOT NULL,
  chain_id TEXT NOT NULL, started_by TEXT NOT NULL, goal TEXT NOT NULL,
  steps TEXT NOT NULL,                 -- JSON [{ memberId, note?, state, runId? }]
  current INTEGER NOT NULL, state TEXT NOT NULL,  -- running | paused | done | stopped
  reason TEXT, stopped_by TEXT, retries INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL);
CREATE INDEX topic_relays_topic ON topic_relays(topic_id, state);
CREATE UNIQUE INDEX topic_relays_message ON topic_relays(message_id);
```

迁移名按功能起（`participants-1`、`chains-1`、`relay-1`、`files-parent-1`），不占 `bots-N`、`files-N` 的号，避免和并行的线撞名。

`bot_outbox.kind`（`room.ts:419` 的联合类型和 `deliver()`）加三种：`relay`（开始接力的某一步）、`part`（分出去的结果交回）、`cancel`（替一个人停一个 run，调 `input.ts:268` 的 `cancel(botId, runId, actorId)`）。

**desk meta**：`chain:<runId>` = `{ chainId, seq, depth, path }`；`relayStep:<runId>` = `{ relayId, index }`；`handedOver:<runId>` = 接手成员 id；`partsIn:<runId>` = 交回来还没给出去的结果。重试和续跑出来的 run 复制 `chain:`、`relayStep:`，所以还算同一步。旧的 `delegation:<runId>` 照样读：没有 `chain:` 时，第一次用到就向房间补建一条以它为根的链。

**产出**：`files` 房间迁移 `files-parent-1`：`ALTER TABLE artifact_versions ADD COLUMN parent_version INTEGER`。

### 3.3 协议（只加不改）

```ts
// 话题
Topic += {
  participants?: { memberId: Id, working: boolean, lastRunAt: Millis }[],  // 只列 AI 成员
  relay?: TopicRelay,                                                      // 没有 = 没有接力
}
topic.updated data += { participants?, relay?: TopicRelay | null }
TopicRelay = {
  id, startedBy, goal /* <= 1000 */, current /* 0 起 */,
  state: "running" | "paused" | "done" | "stopped",
  reason?: string,      // member_unavailable | step_failed | step_blocked | step_cancelled | limit | undelivered | not_pushed | requester_left；不认识的值按「停住了」显示
  stoppedBy?: Id, updatedAt: Millis,
  steps: { memberId, note? /* <= 100 */, state: "pending" | "queued" | "working" | "done" | "failed" | "stopped", runId? }[] /* 2..6 */
}

// run
BotRun += {
  chainId?: Id,                                   // 属于哪件事；链里的每个 run（含根）都有
  code?: { branch: string, head: string, pushed: boolean },  // daemon 在 run 结束时报的代码位置
}

// 产出
ArtifactVersion += { parentVersion?: number }    // 没有 = version - 1（version 1 没有父）
AddArtifactVersionRequest += { parentVersion?: number }
```

新 HTTP 路由（不加客户端帧，免得动 `ClientFrame` 联合和网关分发；和停 run 用同一种方式）：

| 路由 | 谁能调 | 返回 |
| --- | --- | --- |
| `POST /v1/groups/:id/relays/:relayId/stop` | 群里的人（不是 AI 成员） | `TopicRelay`；已结束回 `conflict` |
| `POST /v1/groups/:id/relays/:relayId/retry` | 同上 | `TopicRelay`；不是 `paused` 回 `conflict` |
| `POST /v1/groups/:id/chains/:chainId/stop` | 同上 | `{ stoppedRuns: number }` |

规则：

- 谁能停：群里任何一个人。理由：今天停一个 run 就是这个规则（`routes.ts:451`），停一串不应比停一个更严；打字说「停」、点 run 的停止、点全部停止，规则一致。
- `ownerId` 的说明改成「不带 @ 的跟进给谁：最近在这个话题开始 run 的 AI 成员」，取值不变。
- `parentVersion` 必须是已有版本且小于新版本，否则 `invalid`，reason `artifact.bad_parent`；不传时服务端存当时的 `currentVersion`，所以新数据都显式带父版本。
- machine-protocol：`run.done` 加可选 `code`；`artifact.version` 的 body 加可选 `parentVersion`；`member.handoff`、`member.assign` 不变，新错误 reason：`delegation.cycle`、`delegation.stopped`、`delegation.not_for_requester`。
- 发消息被拒的新 reason：`relay.member_unavailable`（带 `names`）、`relay.running`、`relay.bad_steps`。

### 3.4 服务端

**核心钩子**（core 的改动，R1 负责）：

- `RoomContext.updateTopic(tx, actorId, topicId, { status?, waitingOn?, ownerId?, extra? })`：把 `topicUpdateNow`（`core/room.ts:1100`）里写 `topics` 行、过 `topicStatusFilters`、追加 `topic.updated` 的那段抽出来共用。`extra` 并进事件的 data（放 `participants`、`relay`）。功能代码不能直接调 `room.topicUpdate`，所以要这个钩子。
- `registerTopicFields((sql, topicId) => Partial<Topic>)`：`toTopic`（`core/room.ts:592`）调用它，把 `participants`、`relay` 放进 `Topic`。

**参与成员和话题状态**（新文件 `server/src/bots/participants.ts`）：

- `bots.claim`（非 peek）：`bot_topics` 改成 upsert（替换 `room.ts:150`）；`topic_bots` 这一行 `working=1, run_id, started_at=now`；在同一个 commit 里用 `updateTopic` 把状态设成 `in_progress`、`ownerId` 设成这个 bot，`participants` 带上。返回的 `botId` 总是调用者。
- `runs.ts:74` 和 `front.ts:51` 不再按占住的成员挡（谁收到消息由线 1 的路由决定）；如果线 1 已经删了这两处，就不再动。
- 新 RPC `bots.workState { topicId, runId, outcome, status, waitingOn, stoppedBy?, handedOver?, result? }`：desk 的 `topic()`（`core.ts:629`）在 run 结束时改调它，不再自己带 `ownerId`。房间：写这一行 `working=0, report, ended_at`；处理链和接力（下面）；再汇总、`updateTopic`。调用失败时 desk 把参数存进 `pendingState:<runId>`，由 desk 的 alarm 重试，不丢。
- 兜底：`run.updated` 的事件写入器（同步，不追加事件）在 run 到终态时，把对应行的 `working` 置 0。这样 desk 出错、电脑丢了 run，行也不会一直卡在「在做」。
- 汇总规则（只算「当前的」行）：
  1. 有 `working=1` 的行 → `in_progress`（卡片提问仍由 cards2 的过滤器叠上去）；
  2. 否则只看 `ended_at >= 话题里最近一次 started_at` 的行，按 `waiting_answer > waiting_review > blocked > failed(按 blocked) > done` 取最高，`waitingOn` 取并集；
  3. 有 `running` 的接力且不在最后一步 → `in_progress`；`paused` → `blocked`，`waitingOn=[发起人]`。
  这样「A 卡住，人再 @B，B 做完」时 A 那一行不再算（B 开始得更晚），话题是完成；接力第 1 步「带 PR 等审」不会在第 2 步时让话题变成「等你审」。人手动改的状态保持到下一个 run 开始或结束。
- `cardOwner`（`room.ts:58`）：卡片作者在 `topic_bots` 里就送给作者，否则送给 `ownerId`。
- `bots.transfer` 删掉，归属的改动并进 `bots.chainStepResult`（下面）。
- `bots.release`（`room.ts:316`）同时删这个 bot 的 `topic_bots` 行；它在接力里还没轮到的步，到那一步时按「接不了」处理。
- 恢复存档的 importer（`room.ts:286`）同时补 `topic_bots`（`working=0`，按 run 时间）。链、接力不恢复，从空开始。

**一件事的链**（新文件 `server/src/bots/chains.ts`）：

- 限额常量都在这里：`MAX_DEPTH = 3`、`MAX_HANDOFFS = 8`、`MAX_RELAY_STEPS = 6`、`MAX_RELAY_RETRIES = 3`；删掉 `delegate.ts` 里的两个常量。
- `bots.chainStep { fromRun, requesterId, kind, to, topicId, parentRun? }`（两段式的第一段）：链不存在就建（`root_kind=run`，`root_id=fromRun`）。按顺序查：链已停 → `delegation.stopped`；`handoffs >= 8` → `delegation.too_many`；深度 `>= 3` → `delegation.too_deep`；`to` 已在 `path` 里 → `delegation.cycle`。通过后 `handoffs+1`，写一行 `state=reserved`，返回 `{ chainId, seq, depth }`。
- `bots.chainStepResult { chainId, seq, runId | null }`（第二段）：有 runId → `working`，转交时把 `bot_topics`、`topics.owner_id`、`topic_bots` 一起改成接手成员；null → 删掉这一行、`handoffs-1`。对方拒绝时，次数和归属都不变。
- 结束：`bots.workState` 里，这个 run 是某一步时更新 `chain_steps.state/result`。是分出去的一份 → 入队一条 `part` 给交出方（`last` = 交出方那个 run 分出去的是否都结束了）。被人停下的也回报，结果写「被 {name} 停止」。
- `bots.chainStop { chainId, byActorId }`：只给 HTTP 路由和 desk 调；房间用 `memberInfo` 再查一遍 `byActorId` 是这个群里的人。链标停，给链里每个没结束的 run 入队一条 `cancel`（actorId = 这个人），链上的接力改成 `stopped`。
- 人停一个 run（`input.ts:268`）：`workState` 带 `stoppedBy`；房间把这个 run 分出去、还没结束的那几份也入队 `cancel`。理由：人停了 A，A 分出去的活回来后不该再把 A 叫起来。
- 请求人离开：`registerMemberDeactivated`（`server/src/bots/index.ts:13` 同一处）和成员移出群的路径，对他所在的每个群调内部 RPC `bots.requesterLeft { memberId }`：他发起的链全部停下，接力 `stopped`、reason `requester_left`，话题 `blocked`、`waitingOn` = 管理员（线 13；合并前用 owner）。

**出件箱**：`relay`、`part`、`cancel` 的投递遇到「忙」「排队」不抛异常（记下状态就返回），所以不会挡住这个话题里人的消息。真投不出去到放弃时间时，`drainOutbox`（`room.ts:620-637`）按 kind 调一个 `onGiveUp`：`relay` → 接力 `paused`、reason `undelivered`；`part` → 这一步 `failed`，交出方的话题 `blocked`、`waitingOn=[请求人]`；`cancel` → 记日志。话题里那一句话用线 3 给交接失败做的同一个出口（接 6.1）；线 3 合并前，靠话题状态和接力条上的原因让人看见。`afterCommit` 的触发集合（`room.ts:649`）加上 `run.updated` 和 `topic.updated`，让这三种行及时投出。

**转交、分出去**（改 `server/src/bots/desk/delegate.ts`）：

- 交给谁：`findMember` 之外再查对方肯不肯接这个请求人的活：对方跑在别人的个人电脑上、请求人又不是主人时，回 `delegation.not_for_requester`（线 13：个人电脑默认只接主人的事；线 13 合并前跳过这一条）。链上所有 run 的 `requested_by` 都是最初那个人，审批、密钥、GitHub 身份都按他走。
- 顺序：`chainStep` 预留 → （分出去：先建新话题）→ `takeDelegation` → `chainStepResult` → 发交接卡。分出去的对方忙不了（新话题里没有它的 run），不可用在建话题前已由 `findMember` 查过；极少数竞争下被拒，新话题里只留那条「@B 目标」。
- 对方收到什么（交代单 `Brief` 之外，代码拼的交接摘要，不调模型，标成 `Work order from {A}, an AI member, on behalf of {requester}`）：
  - 原始请求：根请求消息的 id，desk 用线 1 的请求渲染带上它的正文、附件、引用和提到的产出（接 2.1）；
  - 交出方 run 的计划和进度；它做出的产出（id、标题、版本）；它最后一段话，最多 1500 字；
  - 代码位置：交出方 run 的 `code`（分支、提交、是否推上去），从 run 记录取，不从模型的话里取；
  - 链上的位置（「第 k 次交出，来自 A」）；全文用 `chat-run topic state`、`topic history` 去读。
- 话题里看得见：交出方发一张 Card v2（intent `info`，带 v1 回退），不再发那句纯文字。转交：「{A} 把这件事转交给 {B}」，内容是目标和验收标准；分出去：发在 A 的话题里，「{A} 把一部分分给 {B}」，带新话题链接。卡片底部一个按钮「全部停止」（action `chain.stop`）：按下送到卡片作者 A 的 desk（`cardOwner` 已改），A 调 `bots.chainStop`，房间再查一遍按的人。分出去的卡在结果回来时由 A 的 desk 在 `partDone` 里 `card.patch` 一次：「{B} 做完了，结果已交回 {A}」或「{B} 没做完：{原因}」；转交卡不改，B 的进度看 B 的 run。
- 转交之后 A：desk 写 `handedOver:<runId>`。A 的 run 收尾时不发结果卡、不回报分出去的结果；`workState` 带 `handedOver`，房间不把它当一步结束（不推进接力），只把 A 的行置为不在做。2 分钟后 A 还没停，desk 给电脑发 `run.cancel`，但按 `done` 结束（摘要「已转交给 {B}」），不走 `cancelled`。
- 转交继承 `parent`：A 本来是别人分来的，B 做完结果交回原交出方。在接力的某一步里转交：`relayStep` 复制给 B 的 run，`topic_relays.steps[current]` 的 `memberId`、`runId` 改成 B，接力等 B 结束。
- 结果怎么回来（分出去，经 `part` 行，失败会重投）：交出方的 run 在电脑上跑着 → 作为 `run.input` 给它；交出方的 run 存在但还没分到电脑（排队、等电脑）→ 存进 `partsIn:<runId>`，run 开始时作为第一条输入给；交出方没有 run → 存起来，等 `last` 到了用全部结果只开一次续跑（续跑复制 `chain:`、`relayStep:`）。
- 对方忙、离线、做不了：

  | 情况 | 处理 | 交出方 | 话题里 |
  | --- | --- | --- | --- |
  | 对方在这个话题里正在做 | 拒绝 `delegation.busy` | 错误：等它做完或自己做 | 不发卡 |
  | 对方在别处忙、名额满 | 接下，排队 | 成功 | 卡片照发，B 的 run 显示排队 |
  | 对方的电脑离线 | 接下，按线 3 等电脑上线 | 成功 | B 的 run 显示等电脑 |
  | 对方停用、不在群、没有运行方式 | 拒绝 `delegation.unavailable` | 错误 | 不发卡 |
  | 对方做了但做不了 | 对方以「卡住」结束（线 3 的结构化汇报） | 分出去：结果「没做完：原因」交回 | 卡住了，等请求人 |
  | 链已停、超上限、回环 | 拒绝，对应 reason | 错误 | 不发卡 |

  没有「拒接」命令：做不了是一种状态，不是一段对话。

**接力**（新文件夹 `server/src/bots/relay/`：`parse.ts`、`engine.ts`；desk 侧 `server/src/bots/desk/relay.ts`）：

- **主机制：人事先写好顺序。** 理由：顺序是人定的，房间只按状态推进，不调模型，不会失控；一个人用时打一行字就行。没选的：话题里写规则要模型解析；前台定顺序要另配的 key；「做完的成员决定交给谁」正是负责人担心的成员互相派活（转交仍保留，只用于中途发现该换人）。
- **怎么写**：一条消息的第一行是 `@A 说明 → @B 说明 → @C 说明`，分隔符认 `→`、`->`、`=>`。每段是一个 @ 加可选的这一步说明（≤ 100 字）。有第二行时，第二行起是目标；只有一行时，最后一段 @ 后面的文字是目标，前面各段的文字是各步说明。2 到 6 步；同一个成员可以出现多次，不能紧挨着。例：`@小码 → @小审 → @小码 给设置页加深色模式`，或两行写 `@小码 写 → @小审 审 → @小码 按意见改` + 目标。
- **解析**：在 `registerMessageWriteHook` 的 `send`（`core/registry.ts:220` 起）里做，它拿得到 `actor.kind`：只解析人发的、不是群聊的消息。按正文里 `@名字` 的顺序解析（`mentions` 去过重，不能用来定顺序），每个名字对照这个群的 AI 成员（去重后的 `mentions` 当允许集合）。发之前把每个名字都查一遍：在群里、没停用、有运行方式、肯接这个请求人的活（线 13）；有不行的就拒绝这条消息（`relay.member_unavailable`，输入框显示「{names} 现在接不了这件事」），话题里已有 `running` 或 `paused` 的接力也拒绝（`relay.running`）。通过后在 `inTx` 里建链（`root_kind=message`）、写 `topic_relays`、入队第 0 步的 `relay` 行、用 `updateTopic` 追加带 `relay` 的 `topic.updated`。
- 这条消息不再按 @ 分给每个人：投递（线 1 改过的 `deliver()`）先查 `topic_relays.message_id`，命中就跳过。
- **每一步怎么开始**：`relay` 行投到这一步成员的 desk `takeRelay`：不经过前台，用交代单开工（和转交一样）。它在这个话题正在做别的 → 返回 busy，这一步记 `queued`，显示「等 {name} 手上的做完」。恢复的触发：`run.updated` 写入器看到 X 在话题 T 的 run 到终态，而 T 的接力当前步是 X 且 `queued`，就入队一条 `relay`。电脑离线 → 按线 3 排队；如果等电脑超时失败，按 `step_failed` 停住，原因用线 4 的失败归类那一句。
- **被叫醒的成员收到什么**（先压缩，全文按需）：目标；这一步的说明（没写时用默认：第一步「开始做这件事」；中间步「检查前一步的结果，把意见写进结果或产出，不要重做」；最后一步「按前面的结果收尾」）；「第 k/n 步，前面是 {A}，做完交给 {C}」或「你是最后一步」；根请求消息（线 1 渲染，含附件、引用、产出）；前一步的结果（≤ 1500 字）、产出（id、标题、版本、父版本）和代码位置（`code`）；一句「全文用 `chat-run topic state`」。后面还有别的成员时，提示词要求把分支推上去再结束。同一话题有会话时按线 2 续接。
- **什么时候算这一步做完，叫醒下一步**：这一步的 run（含重试、续跑、转交出去的 run）结束，且它分出去的都已交回、那一次续跑也结束，`workState` 里推进：
  - 汇报 `done` 或 `waiting_review` → 这一步 `done`；前一步代码 `pushed=false` 且下一步换了成员 → 停住 `not_pushed`；否则入队下一步；没有下一步 → 接力 `done`。
  - `blocked` / `failed` → 停住，`step_blocked` / `step_failed`。
  - 被人停（`cancelled`，带 `stoppedBy`）→ 停住 `step_cancelled`。打字说「停」和点 run 的停止都是这条。
  - run 中途等人回答不算结束，接力一直等着。
  - GitHub 没登录等问题由 run 自己以「卡住」结束（见 GitHub 身份的设计），接力按 `step_blocked` 停住。
- **怎么停**：最后一步做完；每步有线 3 的时限和催促；一次最多 6 步；「重试」从停住的那步重来，一次接力最多重试 3 次，重试不算交出次数；成员发的消息永远不会被解析成接力。
- **人怎么插手**：接力条上的「停止」（停当前这步的 run，接力 `stopped`）和「重试」（只在停住时）；在话题里 @ 别人、说「停」照线 1 的规则，说「停」会让接力停住。要换顺序：先停止，再发一条新的。
- **为什么不是成员之间的讨论**：后一步只读得到前一步的结果、产出、代码位置和话题状态；没有给别的成员发话的命令；成员的消息不叫醒别的成员（`room.ts:524` 保持）；审查意见写成结果卡或产出。

**状态卡**（线 1 的 `chat-run topic state`）多两段：「参与成员」每人一行（名字 · 在做/做完了/卡住了 · 最近的 run）；「这件事的交接」每步一行（`k. A →(转交|分出去|接力) B · 状态 · 产出 id@版本`），有接力时加一行当前位置。

### 3.5 daemon

- `chat_run.rs` 的提示词（`chat_run.rs:92-95`）交接那段改为：要知道群里有谁，用 `chat-run member list`；只有确实该换人时才转交，交出去就停；不要交回给交给你的成员；分出去的结果会自己回来，不要轮询；你从来不需要给别的成员留言，要说的写进结果或产出。USAGE 同步。
- `run.done` 带 `code { branch, head, pushed }`：从 `workspace.rs` 已有的分支和 `unpushed_commits`（`workspace.rs:267`）取，取不到就不带。
- `chat_run_files.rs` 的 `artifact version` 加 `--parent <n>`。

### 3.6 桌面端（Mac、Windows 同一份）

- **按顺序做的条**（新文件 `apps/desktop/src/views/topic_page/relay_strip.rs`）：话题标题下、计划上。`topic.relay` 是 `running`、`paused`，或结束不到 1 天（看 `updatedAt`）时显示。一行：「按顺序做：小码 做完了 → 小审 在做 → 小码」+「第 2/3 步」；还没轮到的只显示名字（`ink-3`）。右边：在跑时「停止」；停住时「重试」「停止」，下面一行「停在 {name}：{原因}」。用 gpui-component 的 Button，颜色尺寸取 tokens，按 COMPLETENESS.md 画全每种状态。
- **输入框**：话题里第一行是箭头写法时，上方提示「按顺序做：小码 → 小审」；一条消息 @ 了两个以上 AI 成员时提示「想让他们按顺序做？用 → 连起来」（这是让人发现这个写法的地方）；群聊里写箭头时提示「按顺序做要在话题里用」。客户端不改写消息。发送被拒时按 reason 显示文案。
- **详情栏的成员**：用 `Topic.participants`：在做的标「在做」，其余标「参与过」。不再显示「负责」（DESIGN.md §7.8 那一行同步改）。
- **交接卡**：现有 Card v2 渲染，不写新代码。
- **新界面文字**（加进文案目录，并按以往做法补进 PRODUCT.md 的「允许的界面词」；状态只用封闭集里的 在做、排队、做完了、卡住了、等{name}，停止沿用已有的「停止」）：
  - 按顺序做：{names}、第 {k}/{n} 步、停在 {name}：{reason}、都做完了、{name} 停止了、重试、停止；
  - {a} 把这件事转交给 {b}、{a} 把一部分分给 {b}、全部停止、{b} 做完了，结果已交回 {a}、{b} 没做完：{reason}、被 {name} 停止；
  - 原因：{name} 现在接不了这件事、等 {name} 手上的做完、等 {name} 的电脑上线、交出次数到上限了、{name} 的代码还没推上去、发起的人已经不在这个群、没有交到 {name} 手上；
  - 输入框：想让他们按顺序做？用 → 连起来、按顺序做要在话题里用、这个话题已经在按顺序做，先停止它；
  - 详情栏：参与过。
  - PRODUCT.md:171 的「One responsible member per topic」改成「几个成员可以一起或先后在做」。

### 3.7 一个人用时、团队用时

**一个人**：Joye 自己的 Mac 和服务器，三个 AI 成员：小码（Claude Code）、小审（Codex），小窗（跑在他的 Windows 电脑上）。不开任何设置。

1. 在话题里 @ 了小码和小审，输入框提示可以用 → 连起来。他发两行：`@小码 写 → @小审 审 → @小码 按意见改` / `给设置页加深色模式`，附一张截图。话题下出现「按顺序做」条，小码开工。
2. 小码做完，推上分支，发带 PR 的结果卡。条变成「小码 做完了 → 小审 在做」，话题保持进行中（第 1 步的「等审」不算当前）。
3. 小审被叫醒：知道自己这一步是「审」，拿到目标、截图、小码的结果、PR 和分支提交。它把意见写进结果卡。
4. 小码第三步读到意见改完，条显示「都做完了」，话题完成，Joye 收到做完了。
5. 中途小码要在 Windows 上验证，`chat-run assign --member 小窗`：话题里出现「小码 把一部分分给 小窗」。小码先结束了也不算这一步做完；小窗交回结果后小码续跑一次，之后才轮到下一步。
6. Joye 不满意时点「停止」或交接卡上的「全部停止」，或直接说「停」。

只多了一条条和交接卡，没有新页面、新设置。

**团队**：一样的做法。多出来的只有：交给跑在同事个人电脑上的成员会被拒（转交时交出方收到错误；接力在发消息时就被拒，写明谁接不了）；请求人离开时链停下，等管理员。谁都能停，和停一个 run 一样。

## 4. 兼容和迁移

- **旧客户端**：不认识 `participants`、`relay`、`chainId`、`code`，忽略。交接卡带 v1 回退。看不到条，但看得到 run 状态和结果卡；用 run 的停止停当前步，接力随之停住。旧客户端发的箭头消息照样建接力（解析在服务端）；被拒时显示通用错误。
- **旧 daemon**：`member.handoff`、`member.assign` 不变，新 reason 是新字符串，CLI 原样打印。提示词是旧的，上限由服务端拦。没有 `code` 时代码位置写「不知道」，不按 `not_pushed` 停。没有 `--parent` 时服务端补 `currentVersion`。
- **旧数据**：`bot_topics` 留着，意思是「跟进对象」；`participants-1` 把它补进 `topic_bots`。旧 run 没有 `chainId`；旧 `delegation:` meta 按 3.2 读。旧产出版本没有 `parent_version`，读时按 `version-1`，不回填。旧话题没有接力。
- 房间迁移在构造时跑完（`core/room.ts:289`），不加「表不存在」的兜底。

## 5. 安全

- **不提权**：链上所有 run 的请求人都是最初那个人；审批（线 5）、密钥（线 8）、GitHub 身份都按他。不能把活交到别人的个人电脑上。
- **交代单是不可信数据**：摘要和目标是另一个 AI 成员写的，可能带着网页上的注入内容。提示词标明是谁写的、替谁做；被拦的动作照样由代码拦（线 5）。
- **防失控**：深度 3、成员发起的交出最多 8 次、接力最多 6 步、最多重试 3 次、不能回环，都在房间一处计数；链停后一切交出都被拒；成员的消息不触发 run，也不被解析成接力。
- **只有人能叫停和重试**：三个 HTTP 路由只收人；卡片按钮经 desk 时，`bots.chainStop` 在房间里再查一次按的人是群里的人。`bots.requesterLeft`、`bots.release` 只收内部调用。
- **日志**：交接日志只记 id、深度、序号，不记目标原文和结果原文；结果和摘要有长度上限。

## 6. 拆成哪些 PR

| PR | 做什么 | 主要文件 | 依赖 | 怎么测 |
| --- | --- | --- | --- | --- |
| R0 协议 | 3.3 的所有字段、路由、reason；`run.done.code`、`artifact.version.parentVersion` | `docs/protocol.md`（Topic、Runs、Delegation、新「Relay」节、Artifacts）、`docs/machine-protocol.md`、`packages/protocol/src/{topic,bots,files}.ts`、`crates/chat-protocol/src/{topic,bots,files}.rs`、`protocol/fixtures` | 无 | 两份 fixture 测试；旧 fixture 照样能读 |
| R1 参与成员和话题状态 | 核心钩子 `updateTopic`、`registerTopicFields`；`participants-1`；claim upsert 并写 `owner_id`；`workState` 和汇总；写入器兜底；`cardOwner`；`release`、importer；去掉 `runs.ts:74`、`front.ts:51` 的挡；desk `topic()` 改调；PRODUCT.md:171 | `server/src/core/registry.ts`、`server/src/core/room.ts`、`server/src/bots/room.ts`（claim、cardOwner、release、importer、写入器）、新 `server/src/bots/participants.ts`、`server/src/bots/desk/{core,runs,front}.ts`、`docs/PRODUCT.md` | R0；线 1 的路由 PR | vitest：A 跑完再 B 跑，`Topic.ownerId` 和不带 @ 的投递都是 B；A 卡住后人 @B、B 做完 → 完成；A、B 同时、一个卡住 → 卡住了；B 的卡片按钮送 B；release 后行没了；desk 不调 workState 时行也不卡在在做；旧房间迁移后能读 |
| R2 链、转交、分出去、停止 | `chains-1`；`chainStep` 两段式、`chainStepResult`、`chainStop`、`requesterLeft`；出件箱 `part`、`cancel` 和 `onGiveUp`；`delegate.ts` 重写（计数、回环、继承 parent、请求人检查、交接摘要含根请求和代码位置、交接卡、`partsIn`、转交后关 A）；`events.ts`（被停也回报、转交后不发结果卡）；`chains/:id/stop` 路由；daemon 提示词；状态卡「交接」段 | 新 `server/src/bots/chains.ts`、`server/src/bots/desk/delegate.ts`、新 `server/src/bots/desk/delegate_card.ts`、`server/src/bots/desk/events.ts`、`server/src/bots/room.ts`（只动出件箱的 kind、`deliver()`、give-up）、`server/src/bots/routes.ts`、`server/src/bots/index.ts`、`apps/daemon/src/chat_run.rs` | R1 | vitest：深度、8 次、A→B→A 回环、链停后被拒；对方拒绝时次数和归属不变；被转交的那份结果交回原交出方；被停的那份会回报；交出方排队时结果不丢；交出方空闲时只续跑一次；人停 A 时 A 分出去的也停；`part` 放弃时话题卡住；请求人离开时链停；`cargo test` 提示词和 USAGE |
| R3 接力 | `relay-1`；解析（消息写入钩子）；引擎；出件箱 `relay`；desk `takeRelay`；`queued` 的恢复；接力里的转交和分出去；`Topic.relay` 字段；两个路由；投递跳过；状态卡接力行；tools/e2e 用假 CLI 跑三步 | 新 `server/src/bots/relay/{parse,engine}.ts`、新 `server/src/bots/desk/relay.ts`、`server/src/bots/room.ts`（`deliver()` 跳过、写入器里的恢复）、`server/src/bots/routes.ts`、`tools/e2e` | R2 | vitest：分隔符、2–6 步、A→B→A、正文顺序和 `mentions` 顺序不同、单行和两行写法、bot 消息不解析、群聊不解析、有人接不了时拒发、已有接力时拒发；推进、停住、重试（不占交出次数，第 4 次被拒）；B 正在被人 @ 的 run 结束后排队的步开始；步里转交后等 B；步里分出去的未交回时不推进；`not_pushed`；`relay` 放弃时停住；e2e 三步走完、一步失败停住、重试 |
| R4 产出父版本 | `files-parent-1`、校验、缺省补值；daemon `--parent` | `server/src/files/room.ts`、`apps/daemon/src/chat_run_files.rs` | R0 | vitest：缺省补 currentVersion、非法父版本、旧版本读出 version-1；`cargo test` |
| R5 代码位置 | daemon 在 `run.done` 报 `code`；desk 存进 run 记录 | `apps/daemon/src/workspace.rs`、daemon 的 run 结束上报、`server/src/bots/desk/events.ts` 里 run.done 那一处 | R0；先于 R2 合并（同改 `events.ts`） | `cargo test`：有推、没推、不在 git 仓库；vitest：`BotRun.code` 存下 |
| R6 桌面端 | 条、输入框提示、拒发文案、详情栏在做/参与过、三个路由的调用、数据层字段、文案目录、PRODUCT.md 允许词、DESIGN.md §7.8；`scripts/sandbox/seed.ts` 加两个 AI 成员和一个带接力的话题 | 新 `apps/desktop/src/views/topic_page/relay_strip.rs`、`apps/desktop/src/views/topic_rail/`、`apps/desktop/src/views/composer/`、新 `apps/desktop/src/copy/relay.rs`、`apps/desktop/src/copy/mod.rs`、`apps/desktop/src/data.rs`、provider、`scripts/sandbox/seed.ts`、`docs/DESIGN.md` | R0（真行为等 R3） | 文案目录测试；截图自测加条（在跑、停住、做完了；浅色、深色）；`bun run dev:desktop` 走一遍 |

并行：R0 先合。之后 R1、R4、R5、R6 可以同时开（文件不重叠）；R2 等 R1、R5；R3 等 R2。只有 R0 改 `docs/protocol.md` 和协议包，其余 PR 不再改它们。

和别的线的关系：线 1（路由、读取命令、请求渲染）是 R1 的前提；线 2（会话续接、分支）、线 3（结构化汇报、离线排队、交接失败的那一句话）、线 13（管理员、个人电脑只接主人的活）、GitHub 身份没合并前：续接用新会话；结束状态按现在的 run 状态算（成功 → done，失败 → failed）；离线按现在的排队；那一句话先不发；`not_for_requester` 跳过；管理员用 owner。

## 7. 真电脑上的验收步骤

Mac、Windows、Linux 主机（团队）各走一遍，每步留截图和运行记录。

1. 准备：A（Claude Code）、B（Codex），都在这台电脑上，一个 git 仓库。团队场景再加 C，跑在同事的个人电脑上。
2. 在话题里发两行 `@A 写 → @B 审 → @A 按意见改` / `在 README 里加一节安装说明`，附一张截图：
   - 只有 A 有 👀，条显示第 1/3 步；
   - A 做完后话题仍是进行中，不变成等你审；B 自动开工；B 的工作记录里有交接内容：截图、A 的结果、A 的分支和提交；
   - B 只提意见、不重写（看它的改动）；第三步 A 读到了 B 的意见；
   - 最后话题完成，条显示都做完了。
3. 再来一次，B 在做时点「停止」：B 的 run 停下，条显示「{你} 停止了」。再来一次，B 在做时在话题里说「停」：接力停住，可以重试。
4. B 的电脑离线后发接力：轮到 B 时显示等电脑；上线后接着做。
5. 让 B 故意做不了：接力停在 B 并写原因；话题卡住了、有通知；点「重试」从 B 重来。
6. 在箭头消息里写一个停用的成员：消息发不出去，输入框写明谁接不了。接力在跑时再发一条箭头消息：被拒。
7. 在 A 的 run 里让 A 转交给 B：话题出现转交卡；A 停下、不发结果卡；B 做完；之后不带 @ 的一句跟进送给 B；详情栏 A 是参与过、B 是做完后的参与过。
8. 让 A 分出去两份（给 B 和另一个成员），A 先结束：两份都完成后 A 只续跑一次，分出去的卡显示结果已交回。
9. 上限：让 B 转交回 A → `delegation.cycle`；连续交出到第 9 次被拒；团队场景转交给 C 被拒，话题里不发卡。
10. 点交接卡上的「全部停止」：链上所有在做的 run 停下，之后再交出被拒。
11. `chat-run artifact version --artifact X --parent 1 …`：`GET /v1/artifacts/X` 里这一版 `parentVersion` 是 1；不带时是当时的版本号。
12. 上一版桌面端打开同一个话题：没有条，卡片和结果都在，没报错；上一版 daemon 跑转交照样能用，上限生效。

## 8. 这一版不做的

- 分叉时间线的界面；跨产出的「基于」关系。
- 让前台或成员定接力顺序；保存的接力模板；工作流画布；条上改顺序、跳过一步（要改就停止后重发）。
- 群聊里的接力；跨群的转交。
- 每一步单独的验收标准；按金额计的接力预算；一件事同时在做的份数上限（先靠排队名额）。
- 成员显式「拒接」的命令。
- 前台的 `handoff` 决定（保持四种决定加「停止」）。
- 恢复存档时恢复链和接力。

## 9. 还要负责人定的

无。

评审意见里没有采纳的：

- 「把接力当名词加进允许词表是改仓库规则，要负责人定」：界面不用「接力」这个名词（条上写「按顺序做」），状态只用封闭集里的词；往 PRODUCT.md 的允许界面词里补短语是每个功能都走的做法（1.1 的各段就是这样加的），不是改规则。
- 「停止只许发起人或管理员」：改成群里任何一个人都能停，和今天停一个 run 的规则一致（`routes.ts:451`），也消掉了按钮和打字「停」规则不一的矛盾。
