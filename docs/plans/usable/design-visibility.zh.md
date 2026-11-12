# AI 成员能看到什么

状态：设计已定（2026-11-12），待拆 PR 实现。属于线 1（读取命令）和线 7（记忆），第 1 波就能开始。
不依赖 B（虚拟机里留什么）和 C（GitHub 身份）两份设计；和 B 的关系见 3.6。

## 1. 目标

负责人 2026-11-12 的原话：

> 对于一个团队而言, 产品的内容和开发的内容难道就需要隔离吗？如果我在开发群开了3个thread, 我肯定是希望这三个thread是可以读到开发群里面的内容的

规则只有一条：**AI 成员替谁干活，就能用那个人能看到的东西。**

- 「替谁干活」是运行的 `requested_for`（docs/protocol.md:2527-2541「Who a run works for」）。转交、分出去、接力、重来沿用前一个运行的。
- 读取（话题、产出、群里的决定、搜索、文件、记忆）从「这个群」扩大到「这个人所在的所有群」。
- 私聊和单独问只属于里面的人。这个人不在的群，替他干活的 AI 成员也读不到。
- 读取命令和记忆用同一条规则。写入（发卡片、建产出、出新版本、上传文件）仍然只写运行自己的群。
- 一个人在自己电脑上用：不多任何设置、界面或概念，他所在的群本来就全能读。

## 2. 现状

**读取命令只认运行自己的群。**
- `machineRun`（server/src/bots/routes.ts:162-175）校验运行在这台电脑上、还在跑、AI 成员在群里，返回 wire 上的 `BotRun`。私密卡片回答触发的运行，wire 上没有 `requestedFor`（server/src/bots/desk/core.ts:690）；真正的 `requested_for` 只在 desk 的 `RunRow` 里（core.ts:140）。
- server/src/context/routes.ts 的 9 个读取路由（:98-304）都写死 `roomStub(env, run.group_id)`；`files/:fileId` 用 `row.group_id !== run.group_id` 挡别的群（:171）；产出入口文件的 R2 key 用 `fileKey(run.group_id, …)`（:214）。
- 所有群聊室 RPC 都先过 `RoomDO.rpc` 的 `this.requireMember(actor.id)`（server/src/core/room.ts:1531-1539），再进处理函数。所以 `bots.history`、`bots.message`、`bots.topicExists`、`context.card`、`context.topicState`、`artifacts.list/get`、`search.query`、`cards.decisions` 在 AI 成员不在的群里都会 `forbidden`。`DECISIONS_RPC` 自己还查一次 `room.isMember`（server/src/cards2/decisions.ts:196）。
- 话题和消息只在各群聊室 Durable Object 里，D1 没有索引；`files.group_id`、`artifacts.group_id` 在 D1 里能按 id 查到群。
- 人在客户端不指定群搜索时，在他所在的每个群并行搜（server/src/search/frame.ts:17-58）。「这个人能看到的」现成定义就是 `group_members`。
- 协议写明运行只读自己的群（docs/machine-protocol.md:893-897）；daemon 的 `topic()` 注释写「the server refuses any other」（apps/daemon/src/chat_run_context.rs:126），`scope` 只认 `topic | group`（:165-173）；服务端提示词里的读取说明是 `CONTEXT_NOTE`（server/src/bots/dispatch.ts:42）。
- 单独问只能在提问人和 AI 成员都在来源群时开（server/src/sidechat/index.ts:22-25，protocol.md:1636-1640）。AI 成员不在的群里也不会有它的运行（`machineRun` 的成员检查）。
- **续接**：同一话题的新运行续上一次 CLI 会话，不看交代人是谁（`lastSession`，server/src/bots/desk/runs.ts:645-660；`frame.resumeSessionId`，:838-840）。

**记忆用的是「听众」规则。**
- 进提示词和 `chat-run memory search` 用 `fitsGroupSql`：来源群要包含运行所在群的每一个人（server/src/memory/access.ts:72-74，inject.ts:52-64）。人读记忆用 `readableSql`「这个人在不在来源群」（access.ts:61-63），这已是本设计的规则。
- `readableMemories` 绑定固定位置 `?1..?6`，`ids` 从 `?7` 起（inject.ts:52-64）。
- 交代人取错了：提示词用 `requesterId: m.authorId`（runs.ts:729-732，前台 front.ts:178-179）；搜索和 `person` 记忆用 `info.requestedBy`（server/src/memory/routes.ts:249,272-276）。转交和接力时作者是另一个 AI 成员，偏好层就空了。
- `member` 记录的 `fromGroup` 是运行的群，`group` 记录写 `group:<run.group_id>`、`fromGroup` 为空（routes.ts:265-285）。

## 3. 设计

### 3.1 可读的群

新文件 `server/src/context/reach.ts`：

```ts
/** A run's person for reads: requested_for when it is an active human and the run is not
 *  anonymous (requested_by = ANONYMOUS_ID); else null. */
export function readerOf(row: { requested_by: string; requested_for: string | null }): string | null
/** Own group always; with a person, every group of kind "group" the person is in. Read now from D1. */
export async function readableGroups(db, run: { group_id: string }, forId: string | null): Promise<Set<string>>
export async function requireReadable(db, run, forId, groupId): Promise<void>   // ApiError not_found otherwise
```

- 自己的群总能读，私聊里的运行和单独问的运行也一样。
- `forId` 为空时只读自己的群，和今天一样。为空的情况：钩子和集成触发的运行、AI 成员之间没有人的 @、人已停用或不是人、**私密卡片回答触发的运行**（`requested_by = ANONYMOUS_ID`）。最后一种如果放宽，运行引用了只有部分人在的群，群里的人就能推出回答人是谁；所以它不放宽，记忆也一样（3.5）。
- 别的群：`groups.kind = 'group'` 且 `forId` 在 `group_members` 里。AI 成员自己在不在那个群无所谓。私聊（`kind = 'dm'`，单独问是私聊里的话题）只有运行本身在里面时才读得到。
- 每次请求现查 D1，不缓存。人被移出群后，下一次读取就 `not_found`，正在跑的运行也一样。管理员和所有者没有额外范围。
- 并行查找最多 200 个群，超过时按 `group_members.added_at` 取最新的 200 个，记一条 warn（只有数量）。

`machineRun` 多返回 `forId`：desk 在 `getRun` 旁加 `readerOf(botId, runId): string | null`（core.ts），用上面的 `readerOf` 规则。`forId` 只在服务端用，不进任何响应、提示词或日志。

### 3.2 群聊室的只读入口

`RoomDO.rpc` 的成员检查不动。在它旁边加（server/src/core/room.ts）：

```ts
/** Server-only read on behalf of a run that may read this group (context/routes.ts checked
 *  requireReadable). No member check; only the RPCs in READ_AS_RPCS. */
async readAs(groupId: string, name: string, viewer: Actor, args: unknown): Promise<Result<unknown>>
```

- `READ_AS_RPCS`（server/src/core/registry.ts）：`bots.history`、`bots.message`、`bots.topicExists`、`context.card`、`context.topicState`、`artifacts.list`、`artifacts.get`、`search.query`、`cards.decisions`。其他名字一律 `forbidden`。
- `RoomContext` 加 `readingAs: boolean`（registry.ts:125），`readAs` 里为 true。`DECISIONS_RPC` 改成 `if (!actor || !(room.readingAs || room.isMember(actor.id)))`。
- `viewer` 仍然是 AI 成员（`kind: "bot"`），私密卡片照旧按它过滤，只给 `answerCount`。
- `readAs` 只由 server/src/context/routes.ts 和 memory 路由调用；客户端碰不到 Durable Object。

### 3.3 读取路由怎么扩

每个路由：算出目标群 `gid` → `requireReadable` → `gid === run.group_id` 时照旧 `rpc`，否则 `readAs`。

| 读取 | 目标群 |
| --- | --- |
| `topic history/message/state` | 有 `group` 参数：那个群。没有：先找自己的群；不在时对其余可读群并行 `bots.topicExists`，取第一个找到的 |
| `card get` | 同上，用 `context.card` 找 |
| `artifact get` | D1 `artifacts.group_id`；入口文件 key 改 `fileKey(gid, …)` |
| `file get` | D1 `files.group_id`；:171 改成 `requireReadable`，key 用 `row.group_id` |
| `artifact list`、`decisions list`、`member list` | `group` 参数，缺省自己的群 |
| `search` | `scope=topic`（缺省）；`scope=group`（`group` 参数，缺省自己的群）；新值 `scope=all`：并行搜所有可读群，最新的在前，合并照搬 search/frame.ts:38-58 |

不可读的群、不存在的 id 一律 `not_found`，不透露群是否存在。

新路由 `GET /v1/machine/runs/:id/groups` → `RunGroupsResponse { groups: { id, name, kind: "group" | "dm", purpose?, current: boolean }[] }`：自己的群（`current: true`）加其余可读群。不带「替谁干活」的名字；私聊按 `viewGroup` 规则命名，查看者是 AI 成员。

**记下读过的群**：每次 `gid !== run.group_id` 的读取成功后，路由调 desk 新方法 `noteRead(botId, runId, gid)`，写进 `runs.read_groups`（JSON 数组，desk 命名迁移 `reach-1`：`ALTER TABLE runs ADD COLUMN read_groups TEXT`）。3.5 和 3.6 都用它。

### 3.4 协议（只加，不改含义）

同一个 commit 改 `packages/protocol/src/bots.ts`、`crates/chat-protocol/src/{bots,machine}.rs`、`protocol/fixtures`、docs/protocol.md、docs/machine-protocol.md：
- `ContextHit += { groupId?, groupName? }`、`TopicStateResponse += { groupId?, groupName? }`、`RunCardResponse += { groupId?, groupName? }`：只有不是自己的群时才带。
- 新 `RunGroupsResponse`。查询参数 `group=<id>`（messages、state、artifacts、decisions、members、search），`scope` 多一个值 `all`。
- 新 `chat-run` op `group.list`；读取 op 的 body 加 `groupId?`。
- 新 daemon feature `groups`（machine.rs，挨着 `CONTEXT`）。
- machine-protocol.md:893-897 改写成 3.1 的规则；protocol.md 的 Memory 一节（:1222-1229）改写成 3.5；续接一节（machine-protocol.md:336）补 3.6 的条件。

### 3.5 记忆

**读**：`access.ts` 把 `fitsGroupSql` 换成

```ts
/** from_group IS NULL OR from_group IN (SELECT value FROM json_each(?groupsParam)); 'unknown' never. */
export function runReadableSql(alias: string, groupsParam: number): string
```

`readableMemories` 的绑定改成固定布局：`?1` team、`?2` 本群的 group scope、`?3` member、`?4` person、`?5` botId、`?6` 可读群 JSON、`?7` 额外的 group scope JSON（`scope IN (SELECT value FROM json_each(?7))`，提示词传 `[]`）、`ids` 从 `?8` 起。`MemoryReader += { groups: string[]; searchGroups?: boolean }`。
- **提示词**：层不变（团队、本群群层、成员层、交代人偏好层），成员层里来自别的可读群的经验也进来。
- **`memory search`**：`?7` 填所有可读群的 `group:<id>`，别的群的约定也能搜到。
- `forId` 为空（含私密卡片回答的运行）：可读群只有自己的群，交代人层为空。

**交代人一律用 `requested_for`**（经 `readerOf`）：runs.ts:729-732 的 `requesterId/requesterName`；memory/routes.ts:249 的搜索；:272-276 的 `--scope person` 写 `person:<forId>`，`forId` 为空仍 `invalid`。前台 front.ts:178-179 还没有运行：作者是人时用作者，否则为空。私密卡片回答的运行 `requesterName` 留空。

**写**：一个运行读过别的群（`read_groups` 非空）时：
- `group` 记录：拒绝，`forbidden`，code `memory.read_other_group`；
- `member` 和 `person` 记录：`fromGroup` = 那一个别的群；读过两个以上记 `'unknown'`（只有所有者能看，不进任何运行）。

这样别的群的内容不会变成本群的记忆，被不在那个群的人的运行读到。没读过别的群时写入不变。

夜间整理（`consolidate.ts`）不是替人干活，规则不变；人读记忆的 `readableSql` 不变。

### 3.6 续接会话

一个运行读过别的群后，它的 CLI 会话里就有那些内容。所以：

- 续接时继承：`frame.resumeSessionId` 设上时，新运行的 `read_groups` 先复制前一个运行的。
- 续不续：`lastSession` 找到 `prev` 后，desk 用新运行的 `readerOf` 算 `readableGroups`；`prev.read_groups` 里有任何一个不在其中，就把新运行标成 `fresh = 1`（列已有，core.ts:469），本次不续接，用完整提示词，话题历史照常带上。判断放在 runs.ts:223 调 `topicPrompts` 之前，`topicPrompts` 和 `frame` 都经 `lastSession` 读 `fresh`，两处一致。
- B 设计在虚拟机里按话题保留 CLI 会话目录；会话 id 不续时 CLI 新开会话，旧目录里的历史不被读入，B 不用改。

### 3.7 daemon 和提示词

- `apps/daemon/src/chat_run_context.rs`：`read_path` 给上表的 op 加 `groupId` → `group=`；`scope` 认 `all`，和 `groupId` 同时出现 `invalid`；加 `GROUP_LIST` → `/v1/machine/runs/{run}/groups`；:126 注释改为「a topic the run may read」。
- `apps/daemon/src/chat_run.rs`：`--in <groupId>`（`topic history|message|state`、`artifact list`、`decisions list`、`member list`、`search`）、`search --all`、`chat-run group list`；更新使用说明。
- `apps/daemon/src/daemon.rs:44-48` 上报 `feature::GROUPS`。
- 服务端：dispatch.ts 加常量 `READ_GROUPS_NOTE`（英文，给模型读，不是界面文字），不出现任何人名：

  > Besides this group you may read the other groups of the person you work for: `chat-run group list`, then `--in <group id>`, or `chat-run search --query … --all`. Ids of topics, cards, 产出 and files there work as they are. People here may not be in those groups: bring over only what the request needs and say which group it came from. Other people's direct conversations and side chats are never readable.

  `topicPrompts`（runs.ts:670）加参数 `readGroups = false`；runs.ts:223 传 `feature("groups") && readerOf(r) !== null`。`buildPrompt` 和 `buildResumePrompt`（runs.ts:744）都带这段：功能上线前开的会话续接时也能看到。
- 旧 daemon：没有这段提示，发不出 `--in`、`--all`；按 id 读取（`artifact get`、`file get`、`topic message --topic`、`card get`）服务端已放宽，照样能用。

### 3.8 界面

不加界面，不加文案。读了哪个群，作为 `chat-run …` 命令出现在过程视图里。PRODUCT.md 的 AI 成员一节加一句（文档）：「AI 成员替谁干活，就能读谁所在的群；私聊和单独问只属于里面的人。」

## 4. 兼容和迁移

- D1 无迁移；desk 加命名迁移 `reach-1`。旧运行 `read_groups` 为空、`requested_for` 为空时行为和今天一样。
- 已合并的 PR 不撤，只改：#199 读取从「本群」扩大，`scope=group` 含义不变；#198 运行的记忆过滤从「听众」改成「交代人」，人读记忆不变；接 2.3 方案里「限同一个群」作废。
- 影响：来源群是运行所在群超集的成员经验，今天在钩子运行里读得到，改后读不到（钩子没有人）。这是规则统一带来的结果，是否给钩子一个人见第 8 节。
- 旧客户端不受影响。旧 daemon 见 3.7。
- usable.zh.md：线 1、线 7 各补一行指向本文（A1 里改）。

## 5. 安全

- **范围只由服务端定**：`forId` 来自 desk 的 `RunRow`，客户端和 daemon 传不进来；daemon 的 `groupId` 只是提示，过不了 `requireReadable` 就 `not_found`。`readAs` 只接受白名单里的读 RPC。
- **不透露存在**：不可读的群、话题、产出、文件和不存在的一样。`group list` 只列可读的群。
- **私密卡片**：回答人的 id 和名字不出现在任何响应、提示词、`/v1/runs/:id/context` 和 `group list` 里；它触发的运行只读自己的群。
- **内容会被带进别的群**：负责人已接受（「产品的内容和开发的内容难道就需要隔离吗」）。缓解只有提示词里「只带需要的、说明来源」。持久的泄露口（续接会话、记忆）由 3.5、3.6 堵住。
- **写入不跨群**：`artifact version`、`card patch`、`artifact data` 指向别的群的东西仍 `not_found`，要有测试。
- 日志只记群 id 和数量，不记内容、查询词和 `forId`。

## 6. 拆成哪些 PR

**A1 服务端读取、续接和协议**（先做）
- 做 3.1–3.4、3.6、3.7 的服务端部分、3.8 的 PRODUCT.md。
- 文件：新 `server/src/context/reach.ts`；`server/src/context/routes.ts`、`server/src/core/room.ts`、`server/src/core/registry.ts`、`server/src/cards2/decisions.ts`、`server/src/bots/routes.ts`、`server/src/bots/desk/core.ts`、`server/src/bots/desk/runs.ts`（:223、`topicPrompts`、`lastSession` 一带、`frame` 一带）、`server/src/bots/dispatch.ts`；协议四处；docs/protocol.md、docs/machine-protocol.md、docs/PRODUCT.md、docs/plans/usable.zh.md。
- 测：新 `server/test/context-reach.test.ts`。场景：P 在 G1、G2；Q 只在 G1；**AI 成员 B 只在 G1，不在 G2**；G3 没有 P；私聊 DM(P,B)，里面有一个单独问话题。
  - P 在 G1 交代的运行能读 G2 的话题、消息、状态、卡片、产出（含入口文本）、文件、决定、成员；`search --all` 带 `groupName`；G3、DM(P,B) 和其中单独问话题都 `not_found`。
  - Q 交代的运行读 G2 → `not_found`。P 被移出 G2 后下一次读 `not_found`。钩子运行只读 G1。
  - 私密卡片回答触发的运行：读 G2 `not_found`；它的提示词、`/v1/runs/:id/context`、`group list` 里没有回答人的名字和 id。
  - 往 G2 的产出出新版本 → `not_found`。`readAs` 调白名单外的 RPC → `forbidden`。
  - 续接：P 的运行读了 G2 后，同一话题 Q 的下一个运行不带 `resumeSessionId`、`fresh = 1`；P 的下一个运行照常续接。
  - 两个 fixture 测试、`bun run test` 通过。

**A2 记忆**（A1 合并后开始）
- 做 3.5。文件：`server/src/memory/{access,inject,routes}.ts`、`server/src/bots/desk/runs.ts`（只动 :729-732）、`server/src/bots/desk/front.ts:178-179`、docs/protocol.md 的 Memory 一节。
- 依赖 A1（`reach.ts`、`forId`、`read_groups`）。和 A1 都动 runs.ts，所以串行。
- 测：扩现有记忆测试。`fromGroup=G2` 的成员经验进 P 在 G1 的运行，不进 Q 的；来源是私聊的不进 G1；`memory search` 搜得到 G2 的群层约定；带 `ids` 的查询（引用路径）结果正确；转交后的运行有交代人偏好层，`--scope person` 写进交代人的；P 的运行读过 G2 后记 `member` 经验 → `fromGroup=G2`，Q 在 G1 的运行看不到；记 `group` 约定 → `forbidden`；私密卡片回答的运行拿不到别的群的记忆。

**A3 daemon**（和 A2 并行，A1 合并后合并）
- 做 3.7 daemon 部分。文件：`apps/daemon/src/chat_run_context.rs`、`apps/daemon/src/chat_run.rs`、`apps/daemon/src/daemon.rs`、`apps/daemon/tests/fake/fake_claude.py`、`apps/daemon/e2e/local-e2e.ts`。不碰 A2 的文件。
- 测：`cargo test`（`group=`、`scope=all`、`all`+`groupId` → `invalid`、`group.list` 路径）；本地 e2e 里假 CLI 调 `group list` 和 `search --all` 拿到第二个群的命中；Mac、Windows、Linux 编过。

## 7. 真电脑上的验收步骤

环境：团队服务器，Linux 主机上的「开发」AI 成员，A3 之后的 daemon。群：开发群（负责人、实习生、开发成员）；产品群（负责人、产品经理，开发成员不在）；技术群（负责人、开发成员，实习生不在）。负责人和开发成员有一个私聊。

1. 产品经理在产品群发「需求文档 v2」产出，用一张卡片定下「先做导出」。
2. 负责人在开发群开三个话题，分别 @开发成员：「按产品群的需求文档 v2 做导出」「产品群定了先做什么？」「搜一下产品群里关于导出格式的讨论」。都答对；过程视图里有 `chat-run group list`、`search --all`、`artifact get`；回答说明来源是产品群。
3. 实习生 @开发成员 问同样的问题：(a) 在开发群新开话题；(b) 在第 2 步第二个话题里紧接着问。两次都说读不到，回答里没有产品群的任何内容；(b) 的运行没有续接（管理员查运行的 `fresh`）。
4. 负责人在私聊告诉开发成员一个口令；在开发群问「我私聊你说过什么」：不知道。在私聊里问：知道。
5. 负责人在技术群对一个话题用单独问问开发成员；然后在开发群替负责人问单独问里的内容：读不到。实习生在开发群问：也读不到。
6. 负责人在技术群让开发成员记一条 `member` 经验（运行没读过别的群）。第二天：负责人在开发群交代的运行，`GET /v1/runs/:id/context`（负责人读）的 `prompt` 里有这条；实习生交代的没有。
7. 运行进行中把负责人移出产品群：下一次 `chat-run topic history --in <产品群>` 返回 `not_found`。
8. 开发群发一张私密卡片，负责人回答，触发开发成员的运行：它读产品群 `not_found`；它的 `prompt` 里没有负责人的名字。
9. 个人场景：一个人、自己的 Mac、两个群，不做任何设置，第 2 步照样走通。
10. 旧版 daemon 跑第 2 步：`artifact get --id` 能用，提示词里没有 `group list` 那段。

每一步在 usable/verify-log.zh.md 留记录。

## 8. 这一版不做的

- 界面上标出读了哪些群，或按群关掉「AI 成员可读」。
- 钩子的运行替谁干活：现在只读自己的群。是否改成「建钩子的人」（GitHub 身份已这样取，protocol.md:2707）等负责人决定，不当作已定。
- 管理员读自己不在的群；征得同意后读私聊；跨群写入。
- D1 给话题和消息建索引（目前并行找）；读取审计日志；提示词自动带别的群的群层约定（只能 `memory search`）。
- 读过别的群的会话在交代人变了之后改为「续接但删掉那部分」；现在一律不续。
