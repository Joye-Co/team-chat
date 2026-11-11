# 团队权限和电脑归属

状态：设计已定，待拆 PR 实现。属于线 13（[../usable.zh.md](../usable.zh.md)），第 3 波。

## 1. 目标

负责人 2026-11-12 的原话：

> 夜间跟其他会配另外一把 key，到时候会配 DeepSeek 的 key。这里的 key 应该由团队管理，团队里面要有 owner，包括 admin，只有 admin 可以来改这些模型。

> 加电脑的话，所有人都可以加自己的电脑，但团队服务器会有个人级别和团队级别的区分。团队级别只有 admin 能管，团队版还是要实现一套最基础的 admin 和 member 的权限机制。

「团队服务器」在这里指 AI 成员干活的电脑，比如团队那台共用的 Linux 主机。落到这一版是三件事：

1. 三种身份：所有者（owner）、管理员（admin）、普通成员（member）。所有者也是管理员。
2. 电脑分两种：团队的电脑只有管理员能管；每个人都能加自己的电脑（个人的电脑），它默认只做主人自己交代的事。
3. 一个人自己用（自己部署的服务器、自己的电脑、没有别人）时，不多出任何步骤、概念或界面。

## 2. 现状

**身份只有两级。**
- `members.level` 只能是 `owner` 或 `member`（`server/migrations/0001_core.sql:7`，带 CHECK；邀请同文件 :32）。协议 `MemberLevel = z.enum(["owner","member"])`（`packages/protocol/src/common.ts:57`）；Rust 端是严格枚举（`crates/chat-protocol/src/entities.rs:28-33`），服务端发出第三个值时旧客户端解析 `Member` 会失败。
- 第一个人由 `POST /v1/admin/bootstrap`（admin secret）建成 owner（`server/src/core/routes.ts:216-240`）。owner 可以有几个；最后一位在用的 owner 不能被降级或停用（`server/src/notify/members.ts:56-62`）。PRODUCT.md:180 写明 owner 级别也可以是 AI 成员（运维用的 AI 成员）。
- 路由分 `none`、`member`、`owner` 三档（`server/src/core/registry.ts:42-43`，`core/worker.ts:61` 检查）。`auth:"owner"` 的路由举例：AI 服务 `aiservice/index.ts:33,40,65,81`；审批规则 `settings/approvals.ts:49`；邀请和添加 AI 成员 `core/routes.ts:350,394`；运行配置 `bots/routes.ts:79,130,141,526`；电脑 `bots/routes.ts:160,223`；电脑上的登录和密钥 `monitor/accounts.ts:30-117`；用量 `usage/index.ts:165`；活动 `activity/index.ts:47`；审计 `notify/members.ts:159`；团队名单 `team/index.ts:9`；钩子 `integrations/routes.ts:85-184`；诊断 `files/ops.ts:163,180`；前台决定 `agent/index.ts:16,46,63`；运行上下文 `context/routes.ts:139`；`core/auth.ts:64`；#103 的 `PUT /v1/settings/models/:runtime`。这份列表是例子：**所有 `auth:"owner"` 的路由一律等于「管理员（含所有者）」**。
- handler 里判断 owner 的地方：改别人、改级别、预算、停用 `notify/members.ts:18-25`；停用、撤令牌 `core/routes.ts:192,431`；移出群 `core/routes.ts:564`；删别人消息 `core/room.ts:1053`；团队记忆写入 `memory/access.ts:31,41`；卡片的 owners `cards2/events.ts:115`、`cards2/state.ts:166`；审批 `bots/desk/input.ts:73`，批准人是 owner 时找人 `bots/desk/core.ts:617-627`；预算卡 `bots/desk/external.ts:177`；接管 `bots/desk/takeover.ts:27`。
- 停用一个人后 `afterDeactivate` 依次跑 `memberDeactivatedHooks`（`core/routes.ts:168-176`），钩子出错只记日志。
- 文案不统一：设置里 owner 叫「管理员」、member 叫「普通成员」（`apps/desktop/src/copy/settings.rs:47-48`）；团队管理里叫「负责人 / 成员」，写着「只有负责人能打开」（`copy/admin.rs:8-10,289-290`）；「负责人」又用在话题的负责成员上（`copy/timeline.rs:41`）。桌面端用 `i_am_owner` 判断管理入口（`views/settings/mod.rs:146-151`，共 23 处）。

**电脑不分归属，派工不看交代人。**
- `machines` 只有 `created_by`（`server/migrations/0003_bots.sql:3-13`）。只有 owner 能加、移除电脑、给 AI 成员指定电脑（`bots/routes.ts:160,223,79`；`bot_runners.machine_id`，`0010_runner_machine.sql`）。非 owner 拿到删减过的电脑列表（`bots/routes.ts:190-215`）。
- `PUT /v1/bots/:id/runner` 每次整体替换配置；`machineId` 不传表示保留原来的电脑（`bots/routes.ts:79-100`）；`bot_runners.updated_by` 记最后改的人（`0003_bots.sql:25`）。
- 派工 `candidates` 只看指定的电脑或标签、运行方式、健康和空位（`server/src/bots/dispatch.ts:118-134`）；`pick` 先试话题上一次会话的电脑（`bots/desk/runs.ts:238-247`）。一台电脑加进来，所有人交代的活都可能落上去（机 1.12、3.1）。
- `dispatchQueued` 按时间顺序给排队的运行找电脑，某一条找不到就 `break`，后面的全部等着（`bots/desk/runs.ts:223`）；排队位置按这个 AI 成员的整条队算（`bots/desk/core.ts:476-482`）。
- 运行的交代人 `runs.requested_by` 是触发消息的作者（`bots/desk/runs.ts:82-87`）：钩子消息的作者是钩子自己的 AI 成员（`integrations/deliver.ts:122`）；AI 成员在话题里 @ 另一个，作者是前一个 AI 成员。转交和分出去把 `r.requested_by` 原样带过去（`bots/desk/delegate.ts:70,97`），对方拒绝时变成 `delegation.refused` 错误回给前一个运行（:106-110）。
- 跑着的运行：群里任何人的消息都进 `toActive` 当输入（`bots/desk/input.ts:35,89`）；`mayApprove` 对 owner 总是 true（:73）；`mayTakeOver` 对 owner 总是 true（`takeover.ts:27`）。
- 密钥按电脑存，客户端封好再发，服务器看不到明文；每个密钥带 `members`，daemon 只给名单里的 AI 成员（`monitor/accounts.ts:95-112`；`docs/machine-protocol.md:253-266`）。
- 「用这台电脑」走 `POST /v1/machines`（`apps/desktop/src/provider/settings.rs:626-668`）；添加 AI 成员的表单可以选「这台电脑」（`views/settings/members.rs:1613,2149`）。
- 活动页由审计触发器 `activity_from_audit` 抄过去，只认一张固定的动作表（`0013_activity.sql:21-45`）；0016 用另加一个触发器的办法扩表（`0016_account_sign_in.sql:27-32`）。

## 3. 设计

### 3.1 身份怎么存

不给 `level` 加第三个值（旧 Rust 客户端会解析失败），加一个标记：

| 身份 | 存储 | 线上 |
| --- | --- | --- |
| 所有者 | `level='owner'`，`team_owner=1` | `level:"owner"`，`teamOwner:true` |
| 管理员 | `level='owner'`，`team_owner=0` | `level:"owner"` |
| 普通成员 | `level='member'` | `level:"member"` |

- `level:"owner"` 继续表示「能管理团队」，这是它一直的实际含义，所以服务端几十处 `level === "owner"` 检查原样变成「管理员（含所有者）」。设置里本来就叫它「管理员」。
- 新可选字段 `teamOwner` 标出所有者，旧客户端忽略，把所有者当管理员显示。
- 不变式：`team_owner=1` 必有 `level='owner'` 且 `kind='human'`；团队至少一位在用的所有者。所有者可以有几位，转让就是先设一位再降自己。
- AI 成员的级别规则不变：今天 owner 级别的 AI 成员（运维用）保留，显示为「管理员」，旧路由照常能调。原因：PRODUCT.md 写明的行为，没人要求改，降级会打断现有的运维 AI 成员。只有所有者能把 AI 成员设成管理员（和改人的身份同一条规则），AI 成员永远不能是所有者。

迁移 `server/migrations/0017_roles.sql`：

```sql
ALTER TABLE members ADD COLUMN team_owner INTEGER NOT NULL DEFAULT 0;
UPDATE members SET team_owner = 1 WHERE kind = 'human' AND level = 'owner';
```

- 现在所有人类 owner 都成为所有者。原因：谁都不丢掉现在有的权力，之后由他们自己降级。
- 不重建活动触发器：改 `teamOwner` 时审计的 `details` 同时写 `level: [旧, 新]`（例如 `{"level":["owner","owner"],"teamOwner":[false,true]}`），已有触发器就会把它记成「level」类活动（`0013_activity.sql:31`）。

### 3.2 谁能做什么

所有者拥有管理员的全部权力，此外只多两件事：改任何成员的身份；停用或撤销管理员和所有者。下表「管理员」一列也适用于所有者。

| 事情 | 管理员 | 普通成员 | 位置和改法 |
| --- | --- | --- | --- |
| AI 服务的 key 和模型（前台、夜间整理那把，例如 DeepSeek） | 看、改、测试 | 看不到 key，只看到配没配（线 4 模 1.3 做） | `aiservice/index.ts` 不变 |
| 团队的模型设置（每种运行方式一个） | 改 | 只读 | #103 路由不变 |
| 审批规则 | 改 | 只读 | `settings/approvals.ts:49` 不变 |
| 邀请新成员（普通成员、AI 成员） | 能 | 不能 | `core/routes.ts:350` 不变；新成员邀请码带 `level:"owner"` 只有所有者能发（旧客户端还能发） |
| 给已有成员发重新登录的码（`memberId`） | 能；对象是别的管理员或所有者时只有所有者能（拿码登录就是以对方身份登录） | 不能 | `core/routes.ts` `POST /v1/invites`；不改对方身份 |
| 改别人的名字、岗位、形象、汇报给、每周预算 | 能（对象是管理员或所有者时只有所有者能，和 3.8、protocol.md 一致） | 只能改自己的名字和岗位 | `notify/members.ts` |
| 改身份（设为所有者、管理员、普通成员） | 只有所有者 | 不能 | `notify/members.ts` |
| 停用、启用普通成员和 AI 成员；撤他们的令牌 | 能 | 只能撤自己的 | `notify/members.ts`、`core/routes.ts:192,431` |
| 停用管理员或所有者，撤他们的令牌 | 只有所有者 | 不能 | 同上 |
| 添加、编辑、停用 AI 成员（名字、形象、运行方式、模型、放在哪台团队的电脑上） | 能 | 不能 | `core/routes.ts:394`、`bots/routes.ts:79`；放到个人电脑见 3.4 |
| 团队的电脑：添加、改名、移除、换凭证、登录订阅、管密钥、看监控、指派 AI 成员 | 能 | 只看到名字和在不在线 | `bots/routes.ts`、`monitor/*` |
| 自己的个人电脑：上面全部，加上「我交代的事在这里做」 | 能 | 能 | 新，3.3 |
| 别人的个人电脑 | 只看到名字、主人、在不在线 | 同左 | 新；不能改、不能看监控和密钥 |
| 团队电脑上的密钥 | 设、删 | 不能 | `monitor/accounts.ts` |
| 个人电脑上的密钥和订阅登录 | 只有主人 | 只有主人 | 新 |
| 个人电脑上跑着的运行：插话、批准、暂停、继续 | 只有交代人和电脑主人 | 同左 | 新，3.6 |
| 个人电脑上跑着的运行：接管 | 只有电脑主人 | 同左 | 新，3.6、design-takeover 3.2 |
| 团队电脑上跑着的运行：批准、暂停、替人交还 | 按审批规则；管理员能批、能暂停、能替人交还 | 交代人能批、能暂停 | `input.ts:73`、`takeover.ts:27` |
| 团队电脑上跑着的运行：接管 | 不能接管别人的任务 | 只有交代人，且只在报了 `takeover-isolated` 的电脑上 | design-takeover 3.2 |
| 停止一个运行 | 群里任何一个人（今天的规则，不变） | 同左 | `routes.ts:451` |
| 用量（全团队）、活动、审计、诊断 | 能 | 不能 | 不变 |
| 预算卡的「加额度 / 停止」 | 能 | 不能 | `external.ts:177` 不变 |
| 团队约定（团队层记忆）写入、撤回 | 能 | 只读 | `memory/access.ts:31` 不变 |
| AI 成员层记忆写入 | 能，AI 成员自己也能 | 只读 | `memory/access.ts:41` 不变 |
| 群层、个人层记忆 | 按群成员、按本人（线 7） | 同左 | 不变 |
| 删别人的消息、把人移出群 | 能 | 不能 | 不变 |

批准人设为「管理员」而群里没有管理员时退回交代人，由线 5（盯 2.7）做。服务端要改的只有：身份字段、所有者独有的两件事、电脑归属和派工。

### 3.3 个人的电脑和团队的电脑

**存储**，`server/migrations/0017_machine_owner.sql`（和 0017_roles 无关，按仓库规则可共用编号）：

```sql
ALTER TABLE machines ADD COLUMN owner_id TEXT;     -- NULL = 团队的电脑；否则 = 主人
CREATE TABLE machine_bots (                        -- 「我交代给这个 AI 成员的事，在我这台电脑上做」
  machine_id TEXT NOT NULL REFERENCES machines(id),
  bot_id TEXT NOT NULL REFERENCES members(id),
  repo_path TEXT,                                  -- 只有主人能设的本机代码仓库
  created_at INTEGER NOT NULL,
  PRIMARY KEY (machine_id, bot_id)
);
CREATE TRIGGER activity_from_audit_machine_owner AFTER INSERT ON audit
WHEN NEW.action IN ('machine.update', 'machine.bots.set', 'machine.bots.delete')
BEGIN
  INSERT INTO activity (audit_id, at, kind, action, actor_id, target, machine_id, details)
  VALUES (NEW.id, NEW.at, 'machine', NEW.action, NEW.actor_id, NEW.target, NEW.target, NEW.details);
END;
```

旧电脑全部 `owner_id = NULL`，即团队的电脑，行为和今天一样。新触发器照 0016 的做法另加，不重建旧触发器，所以和别的迁移不冲突。

**谁能加**
- `POST /v1/machines` 从 `auth:"owner"` 改成 `auth:"member"`，请求体加可选 `personal?: boolean`。
- 普通成员加的一律是个人的电脑，传 `personal:false` 得 403 `machine.team_admin_only`。管理员不传时是团队的电脑（旧客户端行为不变）。
- 新客户端：「用这台电脑」总是传 `personal:true`。「添加电脑」（装 daemon 的命令，机 1.8 / 线 10）普通成员也有，一律建个人的电脑；管理员的表单多一项「谁用这台电脑：团队 / 只有我」，默认团队，只在团队里有不止一位在用的人时显示。

**谁算交代人**（新函数 `requesterOf`，放 `server/src/bots/desk/runs.ts`；种类判断 `requesterKind(db, id)` 放 `server/src/bots/store.ts`，返回 `human` / `hook`（在 `hooks` 表里有行的成员）/ `bot`）。每次运行在 desk 的 `runs` 表新列 `requested_for`（DO 迁移 `roles-1`）里存「替哪个人做」：

| 运行怎么来的 | `requested_for` |
| --- | --- |
| 人的消息（包括追问） | 这个人 |
| 重来（`POST /v1/runs/:id/retry`） | 原运行的 `requested_for` |
| 私密卡片的作答（`requested_by` 是 `anonymous`） | 真作答人（desk 的 `claim.askerId`） |
| 转交、分出去（`delegate.ts`） | 前一个运行的 `requested_for`，`DelegateIn.requesterId` 改传它 |
| 接力的一步（design-relay，房间按顺序叫醒） | 发起接力的人（`topic_relays.started_by`） |
| 连上 GitHub 后自动起的跟进（design-github-identity） | 等待的那次运行的 `requested_for` |
| 钩子触发 | NULL |

原因：接力和转交是在替同一个人做事，跟着这个人走；钩子没有人。`requested_by` 含义不变，仍是触发消息的作者。AI 成员的消息永远不起运行（design-relay），所以没有「替 AI 成员做」的运行。

协议上它是 `BotRun.requestedFor`（protocol.md「Who a run works for」），四份设计说的「交代人 / 请求人」都指它：design-github-identity 的身份人第 1 条、design-relay 的链请求人（`work_chains.requester_id`）、design-takeover 的「请求人」。私密卡片作答的运行在线上不带这个字段，作答人对外仍是匿名。规则 2 的「接力来的」：`fromMachine` 取前一步的运行所在的电脑。

**派工规则**（`dispatch.ts` 的 `candidates` 加参数 `ctx: { botId, requestedFor, fromMachine?, updatedBy }`）。团队的电脑和今天一样：指定的或者标签匹配。个人电脑 P（主人 A）成为候选，要满足下面一条：

1. `requested_for = A`，并且这个 AI 成员指定在 P 上、或 A 在 P 上勾了它（`machine_bots`）、或它没指定电脑且标签匹配 P。
2. `requested_for = A`，这次是从 P 上的运行转交、分出去或接力来的（`fromMachine = P`），且 P 有这个 AI 成员的运行方式。原因：活跟着电脑走，A 本机的代码仓库和没提交的改动都在。
3. `requested_for` 为 NULL（钩子），AI 成员指定在 P 上，而且运行配置最后是 A 改的（`bot_runners.updated_by = A`）。原因：只有主人亲手放上去的 AI 成员能在无人交代时用他的电脑。

所有候选都还要满足今天的条件：有这个运行方式、健康、有空位。

规则 1 的第三种情况让一个人用时不会卡：他的 AI 成员没指定电脑也能落到他自己的电脑上。

**优先顺序**
- `requested_for` 的人勾了这个 AI 成员时，只用他勾过的那些电脑。原因：本人明确的选择，多半为了本机仓库或自己的登录。勾选只列出这台电脑有运行方式的 AI 成员，所以不会勾到跑不了的。
- 勾过的电脑都不在线：运行排队，话题里状态写「在等{电脑}上线」（`TEXT.waitPersonal`）。不自动换电脑，原因：悄悄换到没有本机仓库的电脑上做，比看得见地等更糟。要不等，在设置 › 电脑里取消勾选；取消、加回、改归属都会立刻重新派一次排队的运行。
- 否则按 `pick` 今天的顺序：话题上一次会话的电脑（如果它在候选里）、再按负载。转交来的运行 `fromMachine` 排在最前。

**排队不互相挡**：`dispatchQueued` 里找不到电脑的那条 `continue`，不再 `break`；只有所有候选电脑都没空位时才停。`queuePosition` 改成只数「更早排队、而且候选电脑有交集」的运行。原因：A 合上笔记本，不该让团队主机上 B 的活也停下。

**做不了的立刻说**：派工前算 `reachable(...)`，只看归属、指定、勾选、`updated_by` 和运行方式，不看在不在线和忙不忙。一台都不可能时不排队：话题里回 `TEXT.personalOnly`，撤掉 👀，运行结束成「做不了」。转交和分出去在对方 desk 的 `takeDelegation` 里先算 `reachable`，为 false 就拒绝，前一个运行收到 `delegation.not_for_requester`（和 design-relay 同一个 reason），原因写「{成员}只做{人}交代的事」，它可以换一位成员。可能、只是离线或忙，就按线 3 排队。`personalOnly` 会说出别人有台个人电脑，这可以接受：电脑名字和主人本来对所有人可见。

**追问和重来**：`requested_for` 是追问或重来的人自己。B 在 A 的个人电脑做过的话题里追问，B 的运行不能用 A 的电脑，会落到团队的电脑上（从新会话开始），或者 AI 成员只在 A 的电脑上时得到 `personalOnly`。原因：B 交代的事不能跑在 A 的电脑上，这是机 3.1 的底线。

**本机代码仓库**（机 3.8）：在个人电脑上运行时，`run.assign` 的仓库路径只取 `machine_bots.repo_path`；AI 成员上管理员设的 `repo_path` 不用，只按 `repo_url` 克隆。团队的电脑上照旧。daemon 不改。

### 3.4 谁能把 AI 成员放到个人电脑上

- 指定（`PUT /v1/bots/:id/runner` 的 `machineId`）仍只有管理员能做。新规则：只有当 `machineId` **变成**一台个人电脑（和存着的不同）时，调用的人必须是它的主人，否则 403 `machine.not_yours`。原样带回存着的 `machineId` 的编辑照常通过，所以管理员仍能改指定在别人电脑上的 AI 成员的模型、运行方式、标签。
- 管理员改了这样的 AI 成员后 `updated_by` 变成管理员，按规则 3 钩子就不再用那台个人电脑；主人自己交代的事照常在那里做。环境变量名单改了也拿不到主人的密钥：密钥给谁由主人在自己电脑上定（`secret.members`）。表单在这种 AI 成员上显示一行 `BOT_PINNED_OTHER_HINT`。
- 勾选（`machine_bots`）：主人可以勾任何没停用、他电脑上有运行方式的 AI 成员，只影响他自己交代的事，不改 AI 成员设置，所以普通成员也能做。

### 3.5 归属怎么改、移除和停用

`PATCH /v1/machines/:id`，请求体 `{ name?: Name, owner?: "me" | null }`：

| 改法 | 谁能做 | 之后 |
| --- | --- | --- |
| 改名 | 团队电脑：管理员；个人电脑：主人 | — |
| 个人 → 团队（`owner:null`） | 主人本人，而且他是管理员 | 删掉它的 `machine_bots` 行 |
| 团队 → 我的（`owner:"me"`） | 管理员，归他自己 | 指定到它的 AI 成员：`updated_by` 不是他的，解除指定，给他补一行 `machine_bots` |

- 不能把电脑改给别人。原因：「这一版做可用」，没有真实场景要求；以前替别人登记的电脑由那个人重新「用这台电脑」即可。
- 团队 → 我的时，电脑上的密钥和订阅登录原样保留。原因：新主人就是原来能管它们的管理员。确认框写明「团队的其他管理员以后管不了这台电脑」。
- 普通成员想把自己的电脑给团队：请管理员在那台电脑上登记团队的电脑。这一版不做转交流程。
- 移除（`DELETE /v1/machines/:id`）：团队电脑由管理员，个人电脑由主人。做的事和今天一样（设 `revoked_at`、解除指定、吊销连接、审计 `machine.revoke`），另外删 `machine_bots` 行。机 1.5 的改名、移除界面复用这两个路由。

**停用一个人**：R3 在 `server/src/bots/` 里注册一个 `memberDeactivatedHooks` 钩子，不改 `core/routes.ts`。它对这个人的每台个人电脑做和 `DELETE /v1/machines/:id` 一样的事：先一批写库（`revoked_at`、解除指定、删 `machine_bots`、审计 `machine.revoke`，`actor_id` 是停用他的人），再吊销连接。写库成功后令牌已经失效，daemon 下一次请求就被拒；吊销连接失败重试一次，再失败写 warn 日志，下次活动页看得到 `machine.revoke`。这些电脑上跑着的运行按线 3 结束成「中断」，话题里照常有说明。解除指定的 AI 成员回到按标签匹配团队的电脑；活动页的 `machine.revoke` 条目就是给管理员的通知，不另发。

**电脑上的路由按归属判断**：`/v1/machines/:id/logins*`、`/secrets*`、`/monitor`、`PATCH`、`DELETE`、`PUT/DELETE /v1/machines/:id/bots/:botId` 改成 `auth:"member"`，handler 里调新函数 `requireMachineManager(db, auth, machineId)`（`server/src/bots/store.ts`）：团队电脑要管理员，个人电脑要主人。

**`GET /v1/machines` 返回什么**

| 调用的人 | 团队的电脑 | 自己的个人电脑 | 别人的个人电脑 |
| --- | --- | --- | --- |
| 管理员 | 完整 | 完整 | id、名字、`ownerId`、在不在线 |
| 普通成员 | 删减（同今天） | 完整 | id、名字、`ownerId`、在不在线 |

别人的个人电脑也返回最少的一行，原因：运行记录里有它的 id，客户端要能显示名字。

### 3.6 个人电脑上跑着的运行

运行所在电脑是个人电脑、主人不是行动的人时（R4 改 `bots/desk/input.ts`、`bots/desk/takeover.ts`）：

- 插话：`toActive` 只把 `requested_for` 那个人的消息作为 `run.input` 送进去；别人的消息照常留在话题里，下次运行作为上下文读到。
- 批准：`mayApprove` 去掉 `level === "owner"` 的放行，只认交代人（`requested_for`）和电脑主人。
- 暂停、继续、看接管地址：去掉管理员放行，只认交代人（`requested_for`）和电脑主人。
- 接管（能在工作目录里跑命令）：只认电脑主人，交代人也不行。接管状态机本身见 design-takeover 3.2，两份设计的权限按这两条对齐。
- 停止：不变，群里任何一个人都能停（`routes.ts:451`）。

原因：在个人电脑上运行和主人自己动手一样，团队的管理员不该替他批准或动他的电脑。团队电脑上一切不变。

### 3.7 协议新增（只加可选字段和路由）

`docs/protocol.md`、`packages/protocol`、`crates/chat-protocol`、`protocol/fixtures` 同一提交改。

- `Member.teamOwner?: boolean`：为 true 时才发；团队名单条目（`roster.ts`）也加。
- `UpdateMemberRequest.teamOwner?: boolean`：只有所有者能设；设 true 时服务端同时把 `level` 设成 `owner`；`level` 改成 `member` 时同时清掉。对 AI 成员设 true 得 `invalid`。
- `Machine.ownerId?: Id`：不发表示团队的电脑。`Machine.personalBots?: { botId: Id; repoPath?: string }[]`：只发给主人。
- `CreateMachineRequest.personal?: boolean`。
- 新路由：`PATCH /v1/machines/:id`（`UpdateMachineRequest { name?: Name; owner?: "me" | null }`）；`PUT /v1/machines/:id/bots/:botId`（`{ repoPath?: string | null }`）；`DELETE /v1/machines/:id/bots/:botId`。
- 新错误码（`ApiError.code` 的 reason）：`role.owner_only`、`role.last_owner`、`machine.not_yours`、`machine.team_admin_only`、`machine.personal_requester`（运行结束原因；`POST /v1/runs/:id/retry` 因此被拒时也带它）。
- `BotRun.requestedFor?: Id`：即 3.3 的 `requested_for`（私密卡片作答的运行不发）。四份设计合并协议时加的，派工、批准、接管和客户端判断「我能不能接管」都看它。
- 转交、分出去被个人电脑挡下时的 reason 是 `delegation.not_for_requester`（和 design-relay 共用）。
- 机器协议（`docs/machine-protocol.md`）不变，只加一段「电脑归属」说明；daemon 不知道归属。
- 以上都已在协议 PR（四份设计合并的那一个）里加好；R1、R3 只做服务端和迁移。

### 3.8 服务端改动一览

- `core/db.ts`：`MemberRow.team_owner`，`toMember` 输出 `teamOwner`。`core/routes.ts` bootstrap（:235）写 `team_owner=1`。
- `core/routes.ts` 邀请：`!req.memberId && req.level === "owner"` 时要求调用的人是所有者；`kind:"bot"` 且 `level:"owner"` 的新邀请同样只有所有者能发；用 `memberId` 的重新登录码保留对方身份，兑换时不动 `team_owner`。未用的旧 `level='owner'` 邀请码兑换后是管理员（`team_owner=0`）。
- `notify/members.ts`：改 `level`、`teamOwner` 只有所有者；对象是管理员或所有者时只有所有者能停用、撤令牌、改资料；「至少一位在用的 owner」守卫（:56-62）改成「至少一位在用的人类所有者」，写法照旧放在 UPDATE 里，两人同时互相降级不会都成功。
- `bots/routes.ts`：`POST/PATCH/DELETE/GET /v1/machines` 按 3.3、3.5；`PUT /v1/bots/:id/runner` 按 3.4。`monitor/accounts.ts`、`monitor/index.ts` 改用 `requireMachineManager`。
- `bots/store.ts`：`loadMachineViews` 带出 `owner_id` 和 `machine_bots`，组成 `MachineView.ownerId`、`MachineView.personalBots: Map<botId, repoPath | null>`；`loadRunner` 带出 `updatedBy`；`requesterKind`、`requireMachineManager`。
- `bots/dispatch.ts`：`candidates`、`reachable`；`TEXT.personalOnly`、`TEXT.waitPersonal`。
- `bots/desk/core.ts`：DO 迁移 `roles-1`（`runs.requested_for`）；`queuePosition`。`bots/desk/runs.ts`：`requesterOf`、`pick`、`couldTake`、`dispatchQueued`、`assignFrame` 的仓库路径。`bots/desk/delegate.ts`：传 `requested_for` 和 `fromMachine`，`takeDelegation` 先查 `reachable`。`bots/desk/input.ts`、`takeover.ts`：3.6。

### 3.9 桌面端：界面和文案

所有文字进文案目录；身份在 `copy/settings.rs`、`copy/admin.rs`，电脑在 `copy/settings.rs`，派工提示在服务端 `dispatch.ts` 的 `TEXT`。「所有者」「管理员」「普通成员」不是开发者词（PRODUCT.md:62 的名单），文案目录测试允许。

**「只有一个人」的条件**：团队里在用的人类成员只有一位。满足时，下面所有「团队 / 我的」区分一律不出现：没有分组、没有副标题、没有后缀，「任意一台团队的电脑」写成「任意一台电脑」，没有「谁用这台电脑」。判断函数 `solo()` 放在 `apps/desktop/src/views/settings/mod.rs`，所有用到的地方共用。

| 键 | 文案 |
| --- | --- |
| `ROLE_TEAM_OWNER` | 所有者 |
| `LEVEL_OWNER` / `LEVEL_MEMBER`（settings，不变） | 管理员 / 普通成员 |
| admin.rs 的 `LEVEL_OWNER` / `LEVEL_MEMBER` / `ROLE_OWNER` | 管理员 / 普通成员 / 所有者（原来是「负责人」，和话题负责成员撞词） |
| `HEADER_DESC` / `SETTINGS_CARD_DESC`（admin） | 所有者和管理员能打开 / 成员、用量、电脑、订阅账号和密钥都在这里。只有所有者和管理员能看到。 |
| `SET_ROLE_OWNER` / `SET_ROLE_ADMIN` / `SET_ROLE_MEMBER` | 设为所有者 / 设为管理员 / 设为普通成员 |
| `ERR_ADMIN_ONLY` | 只有管理员能改这一项。 |
| `ERR_OWNER_ONLY` | 只有所有者能改身份。 |
| `ERR_LAST_OWNER` | 团队至少要有一位所有者。 |
| `ERR_NOT_YOURS` | 这是别人的电脑，只有它的主人能改。 |
| `MACHINE_TEAM` / `MACHINE_MINE` / `MACHINE_OF` | 团队的电脑 / 我的电脑 / {name}的电脑 |
| `MACHINE_WHO_USES` / `_TEAM` / `_ONLY_ME` | 谁用这台电脑 / 团队 / 只有我 |
| `MACHINE_PERSONAL_NOTE` | 只做你交代的事。 |
| `MACHINE_TO_TEAM` / `MACHINE_TO_MINE` | 交给团队 / 改成我的电脑 |
| `MACHINE_TO_MINE_CONFIRM` | 改成你的电脑后，其他管理员就管不了它了。 |
| `MACHINE_MY_BOTS` | 我交代给这些成员的事，在这台电脑上做 |
| `MACHINE_MY_BOTS_REPO` | 这台电脑上的代码仓库 |
| `BOT_PLACE_ANY` / `BOT_PLACE_ANY_TEAM` | 任意一台电脑 / 任意一台团队的电脑 |
| `BOT_PLACE_TEAM_SUFFIX` / `BOT_PLACE_MINE_SUFFIX` | {name}（团队） / {name}（我的） |
| `BOT_PLACE_MINE_HINT` | 放在你自己的电脑上：只有你交代的事会做。 |
| `BOT_PINNED_OTHER_HINT` | 这位成员在{person}的电脑上，只做{person}交代的事。 |
| `BOT_MY_COMPUTER` / `_DEFAULT` | 我交代的事 / 用团队安排的电脑 |
| `INVITE_SOLO_NOTE` | 你的成员现在只在你的电脑上做你交代的事。想让新成员也能用，把电脑交给团队。 |
| `TEXT.personalOnly`（服务端） | {bot}在{person}的电脑上干活，只做{person}交代的事。可以请管理员把它放到团队的电脑上，或者在你自己的电脑上用它。 |
| `TEXT.waitPersonal`（服务端） | 在等{machine}上线。不想等的话，在设置 › 电脑里取消勾选。 |

**界面改动**
- 判断函数保留 `i_am_owner` 的名字（加注释：表示管理员或所有者），新增 `i_am_team_owner`。原因：23 处改名会和线 8、10 的桌面 PR 互相 rebase，条件又没变。`data::Member` 加 `team_owner`，`provider/convert.rs` 读 `teamOwner`。
- 成员列表（设置和团队管理两处）：身份一列显示所有者 / 管理员 / 普通成员；⋯ 菜单的「设为…」只在我是所有者、对象不是我时出现（对象是 AI 成员时只有设为管理员 / 普通成员）。邀请表单不加身份选项，邀请来的都是普通成员，之后再「设为…」。
- 电脑页（设置 › 电脑；团队管理 › 电脑）：普通成员也能打开设置 › 电脑，分「我的电脑」「团队的电脑」两组，团队那组只有名字和在不在线；每台的副标题写团队的电脑或 {name}的电脑；有「添加电脑」。个人电脑详情里有 `MACHINE_MY_BOTS`：gpui-component 的 list 加 switch，只列这台电脑有运行方式的 AI 成员，每行可展开填代码仓库。⋯ 菜单按 3.5 显示「交给团队」或「改成我的电脑」（后者用 dialog 确认）。
- 「用这台电脑」：请求带 `personal:true`，完成那一屏多一行 `MACHINE_PERSONAL_NOTE`（团队时才显示；那一屏的其余说明是线 8 机 3.1 a）。
- AI 成员的电脑选择（成员页表单，以及线 10 日常界面里改电脑的弹窗）：管理员看到任意一台（团队的）电脑 / 各台团队电脑 / 自己的个人电脑；指定在别人个人电脑上的 AI 成员，这一项只读并显示 `BOT_PINNED_OTHER_HINT`。普通成员只看到「我交代的事：用团队安排的电脑 / 用我的电脑 {name}」，写的是 `machine_bots`。添加 AI 成员时，如果「用这台电脑」已做过，默认选这台电脑。
- 邀请表单：我在 `solo()` 状态、而且有指定在我个人电脑上的 AI 成员时，显示 `INVITE_SOLO_NOTE` 和「交给团队」按钮（我不是管理员时不会走到这里，邀请本来就要管理员）。

### 3.10 一个人用时是什么样

- 他用 bootstrap 建出来，是所有者，能做全部事情。成员列表只有他自己，没有「设为…」。
- 「用这台电脑」登记成他的个人电脑；添加 AI 成员默认放在这台电脑上；没指定电脑的 AI 成员按规则 1 也能落到这里；钩子触发的运行，AI 成员是他亲手指定的，按规则 3 照样跑。
- 加一台远程电脑（例如他的 Ubuntu 服务器）是团队的电脑，因为管理员不选时默认团队；只有他一个人，两种没有区别。
- 界面上看不到任何「团队 / 我的」区分（`solo()`），也没有新步骤。和今天比，唯一的新东西是电脑详情里可以勾「我交代的事在这里做」，不用它也没关系。

### 3.11 团队用时是什么样

- 一个人用了一阵、第一次邀请同事：邀请表单提示 `INVITE_SOLO_NOTE`，点「交给团队」，他的电脑变成团队的电脑，同事也能用他的 AI 成员。不点也行，同事 @ 时会立刻看到 `personalOnly`，不会一直转圈。
- 所有者把一位同事设为管理员，这位管理员去配 DeepSeek 的 key、团队模型和审批规则。
- 普通成员小王用邀请码进来，不加电脑，直接 @ 团队 Linux 主机上的 AI 成员 X。
- 小王想让 X 改他本机的代码：在 Mac 上「用这台电脑」，勾上 X，填代码仓库。之后他交代给 X 的事在他 Mac 上做；X 在那里把活转交给 Y，Y 也在他 Mac 上接着做（规则 2）。别人交代给 X 的事照旧在团队主机上做。小王合上笔记本，团队主机上别人的活不受影响。
- 小李 @ 一个只指定在小王电脑上的 AI 成员：立刻看到 `personalOnly`。小李在小王的话题里插话，话进不了跑在小王电脑上的运行；管理员也批不了那上面的审批。

## 4. 兼容和迁移

- **旧客户端**：不认识 `teamOwner`、`ownerId`、`personalBots`，忽略。管理员和所有者都按 `level:"owner"` 显示管理入口，服务端也允许。旧客户端的管理员改别人身份得 403 `role.owner_only`，现有错误提示照常显示。旧客户端「用这台电脑」不带 `personal`：管理员登记成团队的电脑（今天的行为），普通成员的旧客户端没有这个入口。别人的个人电脑出现在列表里，只有名字。
- **旧 daemon**：机器协议不变，不用升级。
- **旧数据**：人类 owner 成为所有者；AI 成员级别不变。所有电脑是团队的电脑，`bot_runners.machine_id` 原样有效。旧的 desk 运行：迁移 `roles-1` 回填 `requested_for = requested_by`；作者是钩子或 AI 成员时这个 id 不等于任何电脑主人，效果和 NULL 一样，而旧数据里没有个人电脑，所以没有影响。一个人用的旧数据：他的 Mac 是团队的电脑，行为不变；想改成我的电脑就点「改成我的电脑」。

## 5. 安全

- 所有检查在服务端；桌面端藏入口只为界面干净。
- 别人交代的事不会落到你的电脑上：派工按 `requested_for` 筛，在服务端（机 3.1 b）；接力和转交跟着原来的人走，钩子只用你亲手指定的 AI 成员（`updated_by`）。管理员改了配置，钩子就不再用你的电脑。
- 你电脑上跑着的运行，只有交代人和你能插话、批准、接管；管理员也不行（3.6）。
- 个人电脑的代码仓库只认主人设的路径（机 3.8）。
- 个人电脑上的密钥和订阅登录只有主人能动；密钥照旧客户端封好、服务器看不到明文、不进提示词、不写日志，服务端只记名字、`last4` 和给谁用。
- 停用一个人，他的个人电脑在同一个钩子里吊销。
- 只有所有者能给任何成员（包括 AI 成员）管理员身份；AI 成员不能是所有者。至少一位所有者的守卫在 UPDATE 语句里。
- 审计：`member.update`（带 `level` 和 `teamOwner`）、`machine.update`（`{ owner: [旧, 新] }`、`{ name }`）、`machine.bots.set` / `machine.bots.delete`；都进活动页。审计不写本机路径以外的本机信息。

## 6. 拆成哪些 PR

R1、R3 可并行（R1 只碰成员，R3 只碰电脑，都不改对方文件，触发器各自独立）。R4 改 desk，要等线 1–3 和接力（线 6）里改同一批 desk 文件的 PR 先合。R2 在 R1 后；R5 在 R2、R3 后（共用 `views/settings/members.rs`）。

| PR | 做什么 | 主要文件 | 依赖 | 怎么测 |
| --- | --- | --- | --- | --- |
| R1 身份：服务端 | 0017_roles；`teamOwner` 读写；所有者独有的检查；邀请规则；最后一位所有者守卫；bootstrap 写所有者；PRODUCT.md「Permissions」改成三种身份（协议字段和 protocol.md「Members」已在协议 PR 里） | `server/migrations/0017_roles.sql`、`server/src/core/{db,routes}.ts`（bootstrap、邀请）、`server/src/notify/members.ts`、`docs/PRODUCT.md` | 无 | vitest 每条矩阵行：管理员改身份 403；降最后一位所有者 409；管理员给另一位管理员发重新登录码成功、身份不变；管理员发 `level:"owner"` 新邀请 403；AI 成员设 `teamOwner` invalid；迁移后身份；活动页出现「level」 |
| R2 身份：桌面端 | `team_owner` 进数据层；`i_am_team_owner`；身份列和「设为…」；文案统一 | `apps/desktop/src/{data.rs,provider/convert.rs,provider/settings.rs}`、`views/settings/{mod,members}.rs`（成员列表部分）、`views/admin/{team,mod}.rs`、`views/members_page/mod.rs`、`copy/{settings,admin}.rs`（身份段）、`fake/settings*.rs`（只给截图自测补字段） | R1 | `cargo test`（含文案目录测试）；截图自测：所有者、管理员、普通成员三视角，浅色和深色；沙盒两个账号实测 |
| R3 电脑归属：服务端 | 0017_machine_owner（含新触发器）；`POST /v1/machines` 带 `personal`；`PATCH`；`machine_bots` 路由；`requireMachineManager`；`GET` 按身份返回；`PUT runner` 的归属检查；停用钩子（协议字段、protocol.md「Machines」和 machine-protocol.md「电脑归属」已在协议 PR 里） | `server/migrations/0017_machine_owner.sql`、`server/src/bots/{routes,store}.ts`、`server/src/bots/deactivate.ts`（新，注册钩子）、`server/src/monitor/{accounts,index}.ts` | 无 | vitest：普通成员加团队电脑 403、加个人电脑成功；管理员动不了别人个人电脑的密钥和监控；改名、交给团队、改成我的电脑；管理员带原 `machineId` 改别人个人电脑上的 AI 成员成功、改成别人的个人电脑 403；停用后电脑 `revoked_at` 有值、`machine_bots` 清空、审计有 `machine.revoke`；钩子写库独立成批、吊销连接失败时写库仍在 |
| R4 派工按归属 | `requesterKind`、`requesterOf`、`requested_for`（DO 迁移 `roles-1`）；`candidates` 三条规则和优先顺序；`reachable` 和 `personalOnly`、`waitPersonal`；`dispatchQueued` 不互相挡、`queuePosition`；转交带 `requested_for` 和 `fromMachine`、`takeDelegation` 先查；勾选变化重新派；个人电脑上的插话、批准、接管；代码仓库路径 | `server/src/bots/dispatch.ts`、`server/src/bots/desk/{core,runs,delegate,input,takeover}.ts`、`server/src/bots/routes.ts`（只在 `machine_bots` 路由里加一行重新派工）、`server/src/bots/store.ts`（只加 `requesterKind`） | R3；线 1–3、线 6 改 desk 的 PR | `dispatch.ts` 单元测试：一个人、一台个人电脑、没指定电脑的 AI 成员，他的请求落到这台电脑；别人请求不落；钩子只在 `updated_by` 是主人时落；勾选优先；desk 测试：A 的运行等 A 离线的电脑，B 后来的运行拿到团队主机；X 在 A 电脑上转交给 Y，Y 也在 A 电脑上；分出去给只在别人电脑上的成员得到 `delegation.not_for_requester`；B 追问 A 电脑上的话题落到团队电脑；B 的插话不进 A 电脑上的运行；管理员批不了 A 电脑上的审批、不能接管；tools/e2e 加两人两电脑场景 |
| R5 电脑归属：桌面端 | `personal:true`；`solo()`；电脑页分组和副标题、普通成员「添加电脑」；「谁用这台电脑」；交给团队、改成我的电脑；勾选和代码仓库；AI 成员表单的放置和「我交代的事」；默认选这台电脑；邀请表单提示；`seed.ts` 加一位普通成员和他的个人电脑 | `apps/desktop/src/provider/{settings,mod}.rs`、`crates/chat-core/src/runs.rs`（新路由的客户端）、`views/settings/machines.rs`、`views/settings/members.rs`（BotForm 和邀请表单部分）、`views/admin/fleet.rs`、`copy/settings.rs`（电脑段）、`scripts/sandbox/seed.ts` | R2、R3；线 10 改电脑的弹窗 | `cargo test`；截图自测（一个人、普通成员、管理员看电脑页，个人电脑详情），浅色和深色；`bun run dev:desktop` 实测；用 `docs/design/skills/emilkowalski/` 四个技能过一遍 |

## 7. 真电脑上的验收步骤

在 Ubuntu 上自己部署服务器，按线 10 的部署文档：部署脚本跑 bootstrap，用 bootstrap 得到的令牌登录第一位成员。

**A. 一个人用（Mac 一遍，Windows 一遍）**
1. 登录后设置 › 成员里我是「所有者」，没有「设为…」。
2. 先添加一个没指定电脑的 AI 成员 Z，再「用这台电脑」。电脑页没有分组、副标题和「谁用这台电脑」。@ Z 交代一件事，做完（规则 1）。
3. 再添加 AI 成员 W，电脑默认选这台电脑，保存；@ W 做完。
4. 配 AI 服务的 key（DeepSeek），保存、测试成功；改团队模型、审批规则成功。
5. 加一个钩子让 W 做事，能在这台 Mac 上跑完。
6. 旧数据：升级前的服务器上做过 1–3，升级后照样能做事，电脑是团队的电脑，行为不变。

**B. 小团队（Linux 主机加两台 Mac）**
1. 所有者 J 用了一阵 A 段，邀请 A：邀请表单出现 `INVITE_SOLO_NOTE`，点「交给团队」。再登记 Linux 主机（团队的电脑），邀请 M。J 把 A 设为管理员。
2. A 登录：能打开团队管理，能改 AI 服务的 key 和团队模型；J 的行上不能停用、不能「设为…」。
3. M 登录：看不到 AI 服务的 key；团队的电脑只有名字和在不在线。M @ 主机上的 X，在 Linux 主机做完。
4. M 在 Mac 上「用这台电脑」，得到「我的电脑」；勾上 X，填代码仓库。M 再交代 X 一件事，在 M 的 Mac 上用那个代码仓库做；X 转交给 Y，Y 也在 M 的 Mac 上接着做。J 交代 X 的事仍在 Linux 主机上。
5. M 合上 Mac，再交代 X：话题显示「在等…上线」。同时 J 交代 X 的事照常在主机上开始。M 打开 Mac，等着的那件开始。
6. A 打开电脑页，看到「M的电脑」，没有改名、密钥、登录、移除。用 curl 带 A 的令牌调 `PUT /v1/machines/<M的电脑>/secrets/X`，返回 403。M 的电脑上 X 要批准一条命令时，A 回「同意」不生效，M 回「同意」生效。
7. A 把新 AI 成员 V 指定到自己的 Mac。M @ V，话题里立刻出现「V在A的电脑上干活…」，没有转圈。A 把这台电脑「交给团队」，M 再 @ V 做完。
8. J 停用 M：M 的 daemon 断开，电脑页显示已移除，活动页有这条；之后 J 交代 X 的事不会落到那台 Mac。
9. J 把 A 设为普通成员，A 刷新后团队管理入口消失；J 想把自己降成普通成员，提示「团队至少要有一位所有者」。
10. 留记录：每一步只截应用窗口，浅色和深色；审计里对应的条目。

## 8. 这一版不做的

- 自定义身份、按群设管理员、细粒度权限开关；邀请时直接选身份。
- 普通成员自己建 AI 成员；个人电脑「也接别人交代的事」的开关、共享给指定的人。
- 把电脑改给别人、普通成员把电脑交给团队的流程；管理员替别人管个人电脑（停用时自动吊销除外）。
- 勾选的电脑离线一段时间后自动换电脑。
- 密钥归团队或个人、自动发到每台电脑（机 4.4）；同名密钥给不同成员不同值（机 4.2，线 8）。
- 普通成员看自己的用量页；按人设模型权限；转让所有者的专门向导。
- 由别的线做、这里只定身份规则的：成员读到 AI 服务配没配（线 4 模 1.3）；没有管理员时批准退回交代人（线 5 盯 2.7）；「用这台电脑」那一屏的说明（线 8 机 3.1 a）；接管状态机（design-takeover）；接力的机制（design-relay）。

## 9. 还要负责人定的

无。
