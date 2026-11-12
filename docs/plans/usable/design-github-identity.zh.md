# GitHub 身份按电脑

状态：定稿，待拆 PR（2026-11-12）。属于 [usable.zh.md](../usable.zh.md) 线 8，替换「已定 3」（每个人连接自己的 GitHub）。本文完整替换旧版。审批部分接 [design-roles.zh.md](design-roles.zh.md) 留下的「团队电脑上受控操作只由管理员批准」。

## 1. 目标

负责人原话（2026-11-12）：

> 对于跑在vps的负责开发的bot就给它一个独立身份, 所有人跟他对话他都是用这个独立身份

此前他否掉了「自己注册一个 OAuth App」（#210 的设备码登录就是 OAuth App）。已定，不再讨论：

1. **个人电脑**：用电脑主人自己在这台电脑上的 gh / git 登录（daemon 现在的 `machine` 模式）。
2. **团队电脑**（VPS）：管理员在主机上用一个机器账号登录一次；这台电脑上的每个 AI 成员替任何人做事都用它。虚拟机里的 git 和 gh 通过 daemon 的凭证助手和 gh 包装拿主机的登录，虚拟机里不存。
3. 提交记在 AI 成员名下；PR 正文写明谁交代的。
4. 产品不再保存任何人的 GitHub 凭证：撤掉或关掉 #210、#233、#238 加的东西和 #214 里「向服务器取令牌」的一半；daemon 的助手和包装保留，令牌改从电脑自己的登录取。
5. 团队电脑上的受控操作（合并到 main、发布到生产、改生产配置、删除数据、对外发布）由管理员批准，交代人不能批；个人电脑上由电脑主人批准。批准人算法、谁能点卡片、「谁来批准」设置一起改。
6. 验收例子：实习生 @ 开发用的 AI 成员改一个他在 GitHub 上没权限的仓库；AI 成员用机器账号干完、开 PR，合并到 main 时停下等管理员。

约束：一个人用自己的电脑不多出任何只有团队才要的步骤；只求能用；协议只做加法；任务读不到的密钥不进日志；界面文字简体中文、取自文案目录、叫「电脑」。

## 2. 现状（origin/main，716cd316）

**服务端按人存 GitHub（#210、#233）**
- `server/src/github/`：设备码登录 `routes.ts:48-190`；令牌加密存 D1（`store.ts:52`，迁移 `0017_github_accounts.sql`）；`loadAccount` 直接 `SELECT * FROM github_accounts`，没有 catch（`store.ts:39-40`）；`features.ts:28` 注册。
- 派工：`bots/desk/runs.ts:874` 调 `githubFor`（`github/assign.ts:105`），团队电脑和虚拟机走 `app`（交代人的令牌）；`committerOf` 在 `assign.ts:87`，`authorOf` 用人的 GitHub 身份（:92）。`runs.ts:893` 写 `bot_runs.github_member_id`。
- 机器路由 `github/machine.ts`（发令牌、`/wait`、`/rejected`）；等人连接 `bots/desk/github.ts`（meta `github_wait:`/`github_resume:`），`events.ts:576,706-718` 把已结束的运行的话题置 blocked 且 waitingOn=[那个人]，`stall.ts:381`。
- 部署顺序：`scripts/deploy-server.sh:112-117` 先 `d1 migrations apply`，再 `wrangler deploy`。

**daemon（#214）**：`apps/daemon/src/github.rs` 的 `app` 模式设 `!chat-run git-credential` 和 gh 包装（`/opt/runner/bin/gh`），每用一次 `Run::fetch`（:258）问服务器要令牌并加进脱敏表；`run.rs:165,221` 自己克隆时放 HTTP 头；`run.rs:546` `clear_home`。`daemon.rs:632` 报 `runsIn`。VM 主机的 daemon 以 `runner` 用户运行（`scripts/runner/host-setup.sh:8,64`），主机没装 gh，基础镜像装了。脱敏覆盖 `gh[pousr]_`、`github_pat_`（`redact.rs:41-42`）。

**桌面端（#238）**：`views/settings/github.rs`（`machine_hint` :467，被 `views/settings/machines.rs:379` 用）、`views/github_banner.rs`（`app.rs:916`）、`provider/github.rs`、`data/github.rs`、`fake/github.rs`、`crates/chat-core/src/github.rs`、`prefs.rs:44 github_prompted`、`copy/settings.rs` 的 `GITHUB_*`、`docs/PRODUCT.md:87-90`。电脑列表的 `MachineInfo.github` 只是「daemon 有没有 github feature」（`provider/settings.rs:244`，读 owners-only 的 `features`）。

**审批**
- `approvalKind`（`desk/core.ts:880`）：开着的审批属于哪类受控操作（`GATE_KINDS`，`packages/protocol/src/settings.ts:118`），普通的「先问一下」没有类别，返回 null。
- 通知谁：`core.ts:937 approversOrRequester`，按团队规则 `approver`/每类批准人；规则是 owners 而群里没有管理员就退回交代人，卡片写 `approvalNoOwner`（`dispatch.ts:110`，`events.ts:359`）。话题的 waitingOn 也用它（`events.ts:123 approvers`）。
- 谁能批：`input.ts:153 mayApprove`：个人电脑上主人和 `requested_for`/`requested_by`；团队电脑上管理员，规则不是 owners 时交代人也行。卡片 `input.ts:299 cardAction`、文字 `input.ts:207`，都经 `approverOfOpen`（`core.ts:972`）。
- 告诉 CLI 找谁：`settings/rules.ts:234 policyPrompt`（`who()` :239）。
- 桌面：设置 › 默认规则的「谁来批准」（`views/settings/members.rs:706-752`，`copy/settings.rs:319-321`），成员页说明 `members_page/mod.rs:1597`。
- 测试：`server/test/gates.test.ts`、`server/test/settings.test.ts`。

**可以直接用的**：电脑提醒 `server/src/monitor/alerts.ts`（`AlertKind` 只在 :23，不是协议类型；`notifyAlert` :151 在 AI 成员和管理者的私聊里发并 @；团队电脑通知管理员，个人电脑通知主人）；`heartbeatAlerts`（`bots/link.ts:508`）；电脑列表 `bots/store.ts:99-112` 从 `linkStub.status()` 拼 `Machine`（`packages/protocol/src/bots.ts:383`）。

## 3. 设计

### 3.1 一条规则：用电脑自己的登录

服务端不再决定用谁的 GitHub，只告诉 daemon「谁交代的」和「提交写谁」。

| 运行方式 | git 和 gh 用什么 | 谁登录 |
| --- | --- | --- |
| 直接在电脑上（`runsIn: "host"`：Mac、Windows、没开虚拟机的 Linux） | 这个系统用户本来的 gh、git 凭证、SSH key，和今天一样，daemon 什么都不碰 | 个人电脑：主人。团队电脑：管理员用机器账号登录这个系统用户 |
| 虚拟机里（`runsIn: "vm"`） | 每用一次，经 daemon 取主机上 **daemon 所在用户（`runner`）** 的 `gh auth token` | 不论个人还是团队，都是有主机 root 的人以 `runner` 用户登录一次 |

个人电脑和团队电脑同一套代码。一个人在 Mac 上用，什么都不用做，也看不到任何 GitHub 设置。

### 3.2 服务端发给 daemon（`run.assign.github`）

`githubFor`（`github/assign.ts`）改成同步拼装，不查账号：
- `source` 一律 `"machine"`，不再发 `"app"`、`login`、`rewriteSsh`。
- `memberId`：按现有 `identityPerson`（`assign.ts:39`）。
- `committer` 和 `author` 都填 `committerOf(botId, botName)`（`ai-<botId>@users.noreply.invalid`）；旧 daemon 拿到 `author` 就设 `GIT_AUTHOR_*`（`github.rs:113-116`），不用改 daemon。
- 新可选字段 `askedBy?: string`（≤100 字）：交代人在团队里的显示名；匿名的私密卡片作答运行（`requested_by = anonymous`）不发。
- 下发条件不变（`runs.ts:874`）。`bot_runs.github_member_id` 不再写。

### 3.3 daemon：虚拟机里用主机的登录（新 feature `github-host`）

- `Run::fetch()` 改为在主机上以 daemon 自己的用户执行 `gh auth token --hostname github.com`（daemon 的环境，10 秒超时）：成功得 `Token` 并加进本次脱敏表；找不到 gh → `Missing("no_gh")`；没登录 → `Missing("signed_out")`；其他 → `Failed`。每用一次取一次，不缓存、不落盘、不进 CLI 环境变量、不作为事件上报。
- 何时装助手：只看 `cfg.vm.is_some()`。虚拟机里的运行设 `!chat-run git-credential`、gh 包装、`CHAT_GITHUB_AS`（机器账号 login，取不到写 `this computer`），SSH 地址固定改写成 HTTPS。直接在电脑上跑：和今天 `machine` 模式一样。收到 `"app"` 当 `"machine"`。
- daemon 自己的克隆和 fetch（`run.rs:165,221`）：虚拟机模式用同一个 `fetch()` 放 HTTP 头；直接在电脑上用电脑的 git。
- 取不到令牌：每次运行只发一次运行事件 `{ type: "github", state: "missing", reason: "no_gh" | "signed_out" }`（`reason` 新可选字段）；助手对 git 返回 `quit=1`，给 CLI 的话：`This computer is not signed in to GitHub. Stop and report it; do not try to sign in.` 推送认证失败（`erase`）发一次 `state: "invalid"`，并马上重查状态。删掉 `/wait`、`/rejected` 调用。
- gh 包装多拒绝 `auth token`、`auth status --show-token`/`-t`（已拒 `login|logout|refresh|setup-git`）。
- 系统说明（`github::note`，`run.rs:724`）：提交人是谁；虚拟机里加「git 和 gh 以这台电脑的 GitHub 账号工作，不要登录」；有 `askedBy` 时加 `When you open a pull request, end its body with the line "交代人：{askedBy}".`
- 心跳 `metrics.github?: { state: "signed_in" | "signed_out" | "no_gh" | "unknown", login? }`：`gh auth status --hostname github.com` 的退出码，login 用 `gh api user --jq .login`；启动时、每 10 分钟、遇到 `invalid` 时查。两种运行方式都报（服务端决定给谁看）。
- `hello.features` 加 `github-host`。`clear_home` 保留，等 B 部分删家目录镜像时一起删。

### 3.4 服务端：撤掉按人存的 GitHub

- 删除 `server/src/github/{api,routes,store,hooks,machine}.ts`、`bots/desk/github.ts`、`core.ts:1140-1146 githubWaitingOn`、`events.ts` 的 `resumeAfterGithub`/`githubFor` 分支（:576、:706-718）、`stall.ts:381`。`external.ts:56` 直接继承 `DeskInteract`。
- 兼容桩（`server/src/github/index.ts`）：`GET /v1/me/github` → `{ available: false, unavailableReason: "off", waits: [] }`；`POST …/device`、`…/poll` → `conflict`（`github.off`）；`DELETE` → `{ ok: true }`；不再推 `github.status`；`/v1/machine/runs/:id/github*` 不注册（404，旧 daemon 只在 `app` 模式调）。
- D1 迁移 `server/migrations/0019_github_off.sql` **只删行不删表**：`DELETE FROM github_device; DELETE FROM github_waits; DELETE FROM github_accounts;`。理由：部署脚本先迁移后发 Worker，间隙里旧 Worker 还在查这些表；回滚到旧版也要能跑。表在下一个版本的迁移 `00NN_github_drop.sql` 里 `DROP TABLE`（间隙里旧 Worker 新写的等待行随之删掉，里面没有密钥）。
- desk DO 命名迁移 `github-off-1`：把 `github_wait:%`、`github_resume:%` 里的 runId 存进 meta `github_off_pending`（JSON 数组），再删这些键。DO 下次 `bind(botId)` 后异步跑 `clearGithubWaits()`：对每个 runId 读出运行，`this.topic(r, r.status === "done" ? "done" : "failed", [], { outcome: r.status, result: r.summary ?? "" })`，清掉话题上「等某人连接 GitHub」的 blocked；做完删键。
- `product.json` 删 `githubClientId`。
- 运行事件 `github`（`desk/events.ts` 新 case），`missing`/`invalid` 每次运行在话题里只说一次（消息 id `run:<id>:github`），不 @ 人、不置 blocked，运行照「失败归类」结束。文案在 `server/src/github/text.ts`：
  - 团队电脑：`这台电脑还没有登录 GitHub，推不了代码。已通知管理员，登录好后再 @我。`（invalid：`这台电脑的 GitHub 登录失效了。已通知管理员，重新登录后再 @我。`），并调 `linkStub(env, machineId).githubMissing(reason)`（见 3.6）。
  - 个人电脑（只会是 Linux 虚拟机主机）：`这台电脑还没有登录 GitHub，推不了代码。请电脑主人 {owner} 在电脑上登录后再 @我。` 不发提醒。

### 3.5 审批：受控操作按电脑定批准人

只改**受控操作**（`approvalKind(r)` 属于 `GATE_KINDS`）。没有类别的普通审批完全照旧（`approversOrRequester`、`mayApprove` 现在的逻辑、`approver` 设置）。

新函数 `approversFor(r, kind)`（`desk/core.ts`）：kind 为 null 时返回 `approversOrRequester(r, kind)`；否则按电脑（`machineOwnerOf`，`input.ts:168`）：

| 电脑 | 通知谁（mentions、话题 waitingOn） | 谁能点卡片、回「同意」 | 群里没有这个人时 |
| --- | --- | --- | --- |
| 团队电脑（`owner_id` 为空或电脑已删） | 群里在职的真人管理员（≤20） | 群里在职的真人管理员；交代人不行，除非他是管理员 | 卡片加 `approvalNoAdmin`，@ 交代人；没人能批，直到管理员进群 |
| 个人电脑 | 电脑主人 | 只有电脑主人 | 卡片加 `approvalNoMachineOwner`，@ 交代人；同上 |

- 文案（`dispatch.ts` 的 `TEXT`，替换 `approvalNoOwner` 用于受控操作的那条路）：`approvalNoAdmin = "这类操作要管理员批准，可这个群里没有管理员。请把一位管理员拉进群来批准。"`；`approvalNoMachineOwner = "这类操作要这台电脑的主人批准，可他不在这个群里。请把他拉进群来批准。"`。卡片 meta 记 `noApprover: true`（不再为受控操作记 `noOwner`）。运行一直等，等待不计时限（现有规则）。
- 批准人必须在群里：卡片只有群成员能看、能按（`cardAction` 从群房间来），按可见范围（A 部分）群外的人看不到这个群。所以不发到群外管理员的私聊，这一版要求管理员进群（第 8 节）。
- `mayApprove(r, actorId, kind, level, approver, machineOwner, gated)` 加 `gated`：`gated` 时只看上表；否则原逻辑不变。
- 改调用处：`core.ts approvers()`（`events.ts:123` 话题 waitingOn）、`events.ts:359` 发卡片、`input.ts:207` 文字批准、`input.ts:299 cardAction`、「这件事里都同意」。`approverOfOpen` 只给普通审批用。
- `policyPrompt(p, gated, gateWho: "admin" | "owner")`：受控操作那几行写 `a team admin` 或 `the owner of this computer`；`runs.ts:723` 按电脑传。花费等其他行仍用 `who()`。
- 团队规则：每类批准人 `approvers` 读写照旧，不再生效；`approver` 只管普通审批。
- 桌面：「谁来批准」下拉保留，标签改 `RULES_APPROVER = "其他要确认的事谁来批准"`；它上面加一行只读说明 `RULES_GATED_NOTE = "上面这几件事：团队的电脑上由管理员批准，个人的电脑上由电脑主人批准。"`。成员页 `members_page/mod.rs:1597` 的说明同步。一个人用：他既是管理员又是主人，界面上只多一行说明。

### 3.6 电脑的 GitHub 状态和提醒

- **服务端存值**：`MachineLink` 每次心跳把 `metrics.github` 存 meta `github`（`bots/link.ts`），`status()` 带出去。
- **电脑列表**（`Machine.github?: MachineGithub`，`packages/protocol/src/bots.ts:383`；`bots/store.ts:99-112` 填）：只发给管理员和这台电脑的主人；`runsIn: "vm"` 时任何状态都发，`"host"` 时只在 `signed_in` 时发（主机模式下 git 可能用 SSH 或钥匙串，没登录 gh 不代表推不了）。
- **监控**（`MachineMonitor.github?` 和 `runsIn?`，`packages/protocol/src/monitor.ts:119`，管理员看）：原样转发。
- **提醒**（`server/src/monitor/alerts.ts`）：`AlertKind` 加 `"github"`，key `github`，只对 `runsIn: "vm"` 的电脑。开始：心跳从 `signed_in` 变成 `signed_out`/`no_gh`；或团队电脑上 desk 调了 `githubMissing()` 而当前没有这条提醒（第一天管理员还没登录的情况）。结束：心跳 `signed_in`，或电脑不再报这一项。每台电脑同时只有一条（现有 key 去重）。通知谁照旧（团队→管理员，个人→主人）。文案 `ALERT_TEXT`：
  - 标题 `githubTitle(machine) = "{machine} 没有登录 GitHub"`；
  - 正文给管理员：`AI 成员在这台电脑上推代码、开 PR 要用它。请到 团队管理 › 电脑，选 {machine}，照说明在电脑上登录。`；给个人电脑主人：`AI 成员在这台电脑上推代码、开 PR 要用它。请在这台电脑上为运行 AI 成员的系统账户登录 GitHub。`

### 3.7 桌面端

- 删除：`views/settings/github.rs`、`views/github_banner.rs`、`provider/github.rs`、`data/github.rs`、`fake/github.rs`、`crates/chat-core/src/github.rs` 和 `crates/chat-core/tests/github.rs`，`app.rs:916` 的安装，`account.rs` 的 GitHub 区块，`topic_page/mod.rs` 和 `selftest/*` 的相关场景，`copy/settings.rs` 的 `GITHUB_*`，只给横条用的 token。不再声明 `github/1`。`prefs.rs` 的 `github_prompted` 读到就忽略，不再写。
- 设置 › 电脑（`views/settings/machines.rs:379`，`machine_hint` 换成读 `Machine.github`）：说明前加 `MACHINE_GITHUB_ON = "GitHub @{login}"` 或 `MACHINE_GITHUB_OFF = "未登录 GitHub"`（`signed_out`、`no_gh`、`unknown` 都是它）；没有字段不显示。`copy/settings.rs` 受目录测试约束（`copy/mod.rs:546` 禁 cli、token…），这里不出现命令、gh、runner。
- 团队管理 › 电脑监控（`views/admin`，`copy/admin.rs:1` 标为开发者视图，允许命令）：一行「GitHub」：`已登录 @{login}` / `未登录` / `没装 gh`；未登录和没装时副文案：`runsIn: "vm"` 写 `在主机上运行 sudo -iu runner gh auth login`，`"host"` 写 `用运行 AI 成员的系统用户运行 gh auth login`。
- 不提供在应用里登录 GitHub 的按钮。

### 3.8 主机和文档

- `scripts/runner/host-setup.sh` 的 apt 列表加 `gh`。
- `docs/SELF-HOSTING(.zh).md` 加「给 AI 成员一个 GitHub 账号」：建机器账号，只给需要的仓库写权限；`sudo -iu runner gh auth login`，或细粒度令牌 `gh auth login --with-token`（只开 Contents、Pull requests 读写，设过期）；main 开分支保护。不用 `gh auth setup-git`。非虚拟机的团队电脑：用运行 daemon 的系统用户登录。

## 4. 兼容和迁移

| 已有的东西 | 处理 |
| --- | --- |
| D1 里的账号和令牌（#210） | 本版 `0019_github_off.sql` 删行（含加密令牌），表留着；下一版 `00NN_github_drop.sql` 删表。顺序写进发版说明。运维在 GitHub 上删掉那个 OAuth App（所有令牌一次作废），删 Worker 变量 `GITHUB_CLIENT_ID`（记在私有 ops 仓库）；发版说明提醒个人可去 GitHub 设置撤销 |
| 等人连接 GitHub 的运行（D1 `github_waits`、desk meta） | 行删掉；desk 迁移 `github-off-1` 把这些话题的 blocked/waitingOn 清掉（3.4） |
| 旧客户端（#238） | `GET /v1/me/github` 返回 `available: false`：成员看不到 GitHub 区块（`settings/github.rs:508`），管理员看到一行「不可用」，升级后消失 |
| 旧 daemon（没有 `github-host`） | 收到 `machine`：主机上照旧；虚拟机里只设提交人，没有 GitHub（和 #214 之前一样）。团队主机必须先升级 daemon（验收第 0 步）。不报 `metrics.github`，电脑页不显示这一行 |
| `bot_runs.github_member_id` | 不读不删 |
| 团队规则里的 `approver`、`approvers` | 保留；`approvers` 不再生效，`approver` 只管普通审批。上线时还开着的受控操作卡片，按新规则判断谁能批 |
| 协议里的 `source: "app"`、`login`、`rewriteSsh`、`MachineGithubTokenResponse`、`MachineGithubWaitRequest`、`github.status` | 标注「不再发送」，类型和 fixtures 保留，含义不变 |
| 成员家目录里留下的 gh 登录 | `clear_home` 继续清，直到 B 部分删家目录 |

## 5. 安全

- 产品里不再有 GitHub 凭证：服务器、App、协议都没有。机器账号令牌只在主机 `runner` 的 gh 配置（`~runner/.config/gh/hosts.yml`，600）；主机的 `~` 不挂进虚拟机。
- 令牌每次用时取、用完丢：不写虚拟机磁盘、不进 CLI 环境变量、不作事件上报、不进 daemon 日志，加进本次脱敏表（帧、文件、会话上传、`stderr.log`）。
- **已知限制（写进 SELF-HOSTING）**：git 推送必须拿到令牌，任务里的进程可以自己调凭证助手（`git credential fill`）在运行期间读到它；gh 包装挡住 `auth token`/`--show-token` 只是挡最直接的用法。缓解：机器账号最小仓库和权限、细粒度令牌带过期、main 分支保护、虚拟机用完丢（B 部分）。主机侧注入认证头的代理这一版不做。
- 接管时人执行的命令里没有 `chat-run`（design-takeover），拿不到令牌。
- 团队电脑上交代人批不了受控操作；真正的底线仍是 GitHub 分支保护（设置页那句话保留）。
- 个人电脑上运行的权限和主人一样（机 3.1 已说明），无新增风险。

## 6. 拆成哪些 PR

顺序：G1 → G4、G5、G6 并行 → G2（要 G4 的 `githubMissing`）→ G3 → G7。G4/G5/G6 改的文件互不重叠；G2、G3 都改 desk 文件，按顺序。

| PR | 做什么 | 改哪些文件 | 依赖 | 怎么测 |
| --- | --- | --- | --- | --- |
| G1 协议 | `RunGithub.askedBy?`；`MachineGithub`；`MachineMetrics.github?`；`Machine.github?`；`MachineMonitor.github?`、`runsIn?`；运行事件 `github` 加 `reason?`；feature `github-host`；`unavailableReason` 加 `"off"`；废弃字段标注；protocol.md「GitHub sign-in」改为停用，machine-protocol.md「GitHub」重写 | `docs/protocol.md`、`docs/machine-protocol.md`、`packages/protocol/src/{github,machine,monitor,bots}.ts`、`crates/chat-protocol/src/{github,machine,monitor,bots}.rs`、`protocol/fixtures/**` | 无 | 两套 fixture 测试；新 fixtures：带 `askedBy` 的 assign、带 `github` 的心跳、带 `github` 的电脑列表和监控 |
| G4 服务端：电脑 GitHub 状态 | 3.6：心跳存值、`status()`、电脑列表按 runsIn 过滤、监控转发、提醒 `github`、RPC `githubMissing(reason)` | `server/src/bots/link.ts`、`server/src/bots/store.ts`、`server/src/monitor/{alerts,index}.ts`、`server/test/monitor.test.ts`、`server/test/machines*.test.ts` | G1 | vitest：signed_in→signed_out 发提醒、回来结束；从没登录且没人调 `githubMissing` 不提醒；团队电脑 `githubMissing` 发一次、再调不重复；host 电脑不提醒；列表里 host 只在 signed_in 时有 `github`，成员（非主人）拿不到 |
| G5 daemon | 3.3 | `apps/daemon/src/{github,run,daemon,metrics,chat_run}.rs`、`apps/daemon/tests/github.rs`、`apps/daemon/tests/redact.rs` | G1；和 B 部分的 daemon PR 都改 `run.rs`，G5 先合 | 测试里主机 PATH 放假 `gh` 打印 `ghp_…`：VM 模式助手拿到、各处打码；假 gh 退出非 0：`quit=1`、`missing` 带 reason 只发一次；host 模式没有助手；`gh auth token` 被拒；心跳带 `github` |
| G6 桌面端 | 3.7 和 3.5 的界面 | `apps/desktop/src/{app,data,fake}.rs`、`{data,fake,provider}/github.rs`（删）、`provider/{mod,settings,snapshot}.rs`、`views/github_banner.rs`（删）、`views/settings/{github(删),account,machines,members,prefs}.rs`、`views/admin/*` 监控、`views/members_page/mod.rs`、`views/topic_page/mod.rs`、`copy/{settings,admin}.rs`、`ui/native/tokens.rs`、`selftest/*`、`crates/chat-core/src/{github(删),engine,client,lib}.rs`、`crates/chat-core/tests/github.rs`（删） | G1 | `cargo test`（文案目录、自检截图）；`bun run dev:desktop` 亮暗截图：电脑页两种状态、监控三种状态、默认规则说明行；上手和话题无 GitHub 提示 |
| G2 服务端：撤掉按人存 | 3.2、3.4 | `server/src/github/*`、`server/src/bots/desk/{github,external,core,events,stall,runs}.ts`、`server/src/features.ts`、`server/migrations/0019_github_off.sql`、`product.json`、`server/test/github-*.test.ts` | G1、G4 | vitest：assign 永远 `machine`，author=committer=AI 成员，`askedBy` 正确、匿名不发；`/v1/me/github` 返回 off；机器路由 404；迁移后三表为空但存在；`github-off-1` 后原 blocked 话题 waitingOn 为空；`missing` 团队/个人各说一次、团队调了 `githubMissing`、重复事件不再说 |
| G3 服务端：受控操作批准人 | 3.5 服务端 | `server/src/bots/desk/{core,input,events,runs}.ts`、`server/src/settings/rules.ts`、`server/src/bots/dispatch.ts`、`server/test/gates.test.ts`、`server/test/settings.test.ts` | G2 | 见下 |
| G7 文档和主机 | 3.8；PRODUCT.md GitHub 用词换成新词表；usable.zh.md 已定 3 和线 8；machine.zh.md 机 4.1 指向本文 | `scripts/runner/host-setup.sh`、`docs/SELF-HOSTING.md`、`docs/SELF-HOSTING.zh.md`、`docs/PRODUCT.md`、`docs/plans/usable.zh.md`、`docs/plans/usable/machine.zh.md` | G2、G5；`host-setup.sh` 等 B 部分改它的 PR 合并后 | 新 Ubuntu 上照文档走一遍（第 7 节） |

G3 测试：`gates.test.ts` 里「交代人批准受控操作」「个人电脑上交代人批准」两类用例翻转为被忽略，「approver=owners」用例改为只对普通审批断言；新增：团队电脑上实习生能批普通审批、不能批合并到 main，管理员能；群里没管理员时卡片带 `approvalNoAdmin` 并 @ 交代人，交代人仍不能批；个人电脑上受控操作只有主人能批（管理员、交代人都不行），主人不在群时带 `approvalNoMachineOwner` 并 @ 交代人；话题 waitingOn 用新批准人；团队规则 `approvers.mergeMain = "requester"` 不影响；`policyPrompt` 两种写法。`settings.test.ts` 里断言 `approvers` 生效的用例改为只断言能存能读。

G7 的 PRODUCT.md 新词表（替换 `docs/PRODUCT.md:87-90`）：「GitHub @{login}」「未登录 GitHub」「这台电脑还没有登录 GitHub」「这台电脑的 GitHub 登录失效了」「其他要确认的事谁来批准」以及 3.5、3.6 的卡片和提醒原文；删去旧词「连接 GitHub」「验证码」「{name} 在等你连接 GitHub」等 #238 的说法。

## 7. 真电脑上的验收

准备：团队 Linux 主机已装带 G5 的 daemon；服务器部署到 G3；Mac 装带 G6 的应用。管理员 A、实习生 I（GitHub 账号对 `<org>/<repo>` 无权限），开发用 AI 成员 D 在团队主机上，群「开发」有 A 和 I。

0. 第一天：主机还没登录。I @D 改 README。话题出现「这台电脑还没有登录 GitHub…已通知管理员…」只一次；A 在私聊收到「<主机> 没有登录 GitHub」。A 在主机运行 `sudo -iu runner gh auth login` 登录 `<org>-bot`；10 分钟内提醒变为已恢复，团队管理 › 电脑显示「已登录 @<org>-bot」，设置 › 电脑显示「GitHub @<org>-bot」。
1. I 说：`@D 在 <org>/<repo> 把 README 的错别字改了，开个 PR`。检查：PR 作者 `<org>-bot`；`git log -1 --format='%an <%ae> / %cn'` 两处都是 D；PR 正文最后一行 `交代人：<I>`；运行中虚拟机里 `ls ~/.config/gh` 没有 `hosts.yml`、`env | grep -i token` 无输出；daemon 日志、`stderr.log`、运行事件里搜 `gh[pousr]_`、`github_pat_` 搜不到。
2. I 说 `@D 合并到 main`：`gh pr merge` 被拦，卡片 @A。I 点「同意」、回「同意」都无效；A 点后合并成功，审计记 A。另让 D 做一件普通需确认的事，I 能批。
3. 把 A 移出「开发」，让 D 合并另一个 PR：卡片显示「这类操作要管理员批准，可这个群里没有管理员…」并 @I，I 批不了；A 回群后能批。
4. 主机上 `sudo -iu runner gh auth logout`：10 分钟内两处电脑页变「未登录」，A 收到提醒。D 再推送：话题出现登录提示只一次。重新登录后提醒恢复。
5. 一个人：新 Mac 从头安装「用这台电脑」，Mac 上已 `gh auth login` 自己的账号（再试一次只用 SSH key、没装 gh）。交代 D2 开 PR：由他的账号推送，提交作者 D2；应用里没有任何 GitHub 设置、横条、提示（只用 SSH 时电脑页也不显示 GitHub）；合并到 main 的卡片发给他，他能批。默认规则页只多一行说明。
6. 个人电脑主人不在群：成员 M 的个人 Mac，D3 的活跑在上面，在一个 M 不在的群里让 D3 合并到 main：卡片显示「…电脑的主人…不在这个群里…」并 @ 交代人，交代人批不了；把 M 拉进群后 M 能批。
7. 旧数据：staging 上先保留 #210 的账号数据和一个等连接的话题再部署。部署后三表为空、表还在；那个话题不再 blocked；旧版应用打开不报错。部署期间旧 Worker 派工不报错。
8. Windows 当个人电脑走一遍第 5 步。

每一步结果原样记进 [verify-log.zh.md](verify-log.zh.md)。

## 8. 这一版不做

- 令牌完全不进虚拟机（主机侧注入认证头的代理，或 daemon 代为推送）。
- GitHub App、按人的 GitHub 身份、应用里登录 GitHub。
- 一台电脑多个 GitHub 账号（按仓库、按 AI 成员）；GitHub Enterprise 和其他代码托管平台。
- PR 自动打标签、指派审阅人；在主机上强制 PR 正文写交代人（靠系统说明，验收检查）。
- 批准人不在群时改发私聊、在群外处理审批：这一版要求批准人进群。
- 每类操作单独设批准人（数据保留，界面不再提供）。
- 删 D1 旧表：下一版。
