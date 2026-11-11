# 每个人的 GitHub 身份

状态：设计已定，待拆 PR（2026-11-12）。属于 [usable.zh.md](../usable.zh.md) 线 8，对应「已定 3」和审计条目机 4.1、4.5。

## 1. 目标

负责人原话（2026-11-12）：

> 登录方面，每个人 onboard 时先登录一下自己的 GitHub，以后就用它，所以我们会保存所有人的 GitHub 凭证。

起因（2026-11-10）：

> 假设有 10 个 bot，可能后面会有其他真人用，可能会有不同的 GitHub 登录凭证，这个要怎么保存呢？

团队级的 GitHub App 已被否掉，因为产品也有一个人单独用的场景。

这一版要做到：

1. 每个人上手时在应用里连接一次自己的 GitHub，产品保存凭证。
2. 一个人交代的任务（含转交、分出去、接力、重试）在 GitHub 上用他的身份拉代码、推代码、开 PR。
3. 没连接的人交代了要用 GitHub 的事，AI 成员当场说清楚，他点一下就能连上；连上后任务自己接着做，不用他再说一遍。

上手时的「以后再说」是本设计有意留的软化，不是负责人的话：理由是已定的「一个人用不多一步」，他在自己的电脑上不连也能照今天的样子用。

## 2. 现状

- 产品不管 GitHub 身份。只有 `apps/daemon/src/config.rs:409` 的 `env_passthrough` 示例（`GITHUB_TOKEN`）和脱敏规则 `apps/daemon/src/redact.rs:41`（`gh[pousr]_[A-Za-z0-9]{30,}`）。
- 本机执行（Mac、Windows、不开虚拟机的 Linux）：CLI 就是电脑主人这个系统用户，用电脑上现成的 `gh`、SSH key、`~/.gitconfig`（`docs/machine-protocol.md:828`）。
- Linux 虚拟机：每个 AI 成员一个长期家目录（`apps/daemon/src/executor/firecracker.rs:377`），`docs/machine-protocol.md:816` 写明 `gh auth login` 的结果下次还在。谁输了验证码，之后这个成员所有任务都用谁（机 4.1）；令牌明文在 `~/.config/gh/hosts.yml`，任务能读写。虚拟机里 PATH 被换成 `GUEST_PATH`（`firecracker.rs:29`，`/opt/runner/bin` 打头），`SSH_AUTH_SOCK` 被去掉（`firecracker.rs:143`）。
- 镜像没设 git 的 user.name/email（`scripts/runner/base-image/Dockerfile:34-38`），第一次提交就报错（机 4.5）。
- daemon 自己在 CLI 启动前克隆和 fetch 裸镜像 `<work_root>/repos/`，再 `git worktree add`（`docs/machine-protocol.md:788-793`），用的是电脑用户自己的 git 凭证。
- 已有密钥通道：App 把密钥封给电脑公钥（`apps/daemon/src/accounts.rs:1-14`），服务器只转发密文（`server/src/monitor/accounts.ts:1-4`），电脑存 `secrets.json`；只有 owner 能操作，按电脑存。
- 服务器已有加密存储：`server/src/integrations/secrets.ts:37` `encryptSecret`（AES-GCM，Worker secret `HOOK_SECRET_KEY`），没配时抛 `hooks are not configured`（`secrets.ts:31`）。AI 服务的 key 也用它（`server/src/aiservice/store.ts:40`）。`docs/INSTALL*.md` 没提这个 secret，只有 `docs/ENVIRONMENTS.md:91,113` 和 RUNBOOK 提。
- 「谁交代的」：desk DO 的 `runs.requested_by`（`server/src/bots/desk/runs.ts:82`）；转交、分出去沿用最初交代人（`server/src/bots/desk/delegate.ts:70,97,181`）；重试复用原请求。私密卡片的作答人对外是 `anonymous`（`server/src/cards2/privacy.ts:21,72`），但 desk 拿得到真作答人 `claim.askerId`（`runs.ts:376-382`）。集成事件的 hook 记着 `hooks.created_by`（`server/migrations/0003_integrations.sql:12`）；AI 成员是谁加的记在 `members.created_by`（`0001_core.sql:11`）。
- desk DO 的 `runs.ended_at` 是运行是否结束的唯一准数（`server/src/bots/desk/core.ts:48,124` `ACTIVE`，`takeover.ts:38` 已这样用）。D1 的 `bot_runs`（`0003_bots.sql:29-36`）只是索引，记 `machine_id`。
- 跟进：运行结束后还有人的输入，desk 起一条 `follow_up_of` 的新运行（`core.ts:704` `start(m, followUpOf, ...)`，深度上限 `events.ts:386`）。
- `run.assign` 原样存进 MachineLink 的 SQLite 用于重发（`server/src/bots/link.ts:175`），不能放令牌。
- 电脑用机器令牌走 HTTPS 已有先例：`PUT /v1/machine/runs/:id/session`（`server/src/bots/routes.ts:261-266`，先查 `bot_runs.machine_id`）。
- `chat-run` 是 daemon 程序本身按名字分流；`<运行目录>/bin` 放在 PATH 最前（`chat_run.rs:296` `install_link`）。秘密操作 `secrets::OP` 在 daemon 本地处理、不上传（`chat_run.rs:644`），其他操作都作为运行事件 `output` 上传等服务器（`chat_run.rs:1-7`）。Codex 走文件通道，答案写成工作目录里 mode 600 的 `<id>.res`，`chat-run` 读完即删（`chat_run.rs:13-19`）。客户端等待上限 15 s + 5 s（`chat_run.rs:44,1515-1521`）。
- 每次运行在开始时克隆一份 redactor（`apps/daemon/src/run.rs:148,249,319,462`）；`secret.set` 换的是全局 Arc（`daemon.rs:1005-1017`），进行中的运行看不到新值。
- `github_logins`（`0003_integrations.sql:19-24`）：登录名小写，`updated_at NOT NULL`，owner 手工维护，用来把 PR 卡片对上人。

## 3. 设计

### 3.1 关键决定

| 决定 | 选择 | 理由 |
| --- | --- | --- |
| 登录方式 | 唯一主路径：GitHub OAuth App 设备码登录，服务器发起和轮询，应用只显示验证码 | Mac、Windows 同一套代码；应用不经手令牌；团队主机也能用。 |
| 导入现成 `gh` 登录 | 不做 | 只对本机成立；令牌属于 gh 的 App，`gh auth logout` 就失效。 |
| 粘贴令牌 | 不做 | 要自己生成 PAT、懂 scope，多一步多一个概念。 |
| OAuth App 还是 GitHub App | OAuth App | 权限等于这个人自己的权限，不用在仓库上装 App（负责人已否掉安装步骤）；令牌不过期，不用做刷新。 |
| client id 从哪来 | 默认 `product.json` 的 `githubClientId`（产品公开的 OAuth App，开 Device Flow）；Worker 变量 `GITHUB_CLIENT_ID` 覆盖 | 设备码流程只要公开 client id，不要 secret，自己部署的人不用多注册。代价：授权页显示的是产品账号，组织批准一次对所有部署有效，产品账号理论上能整体吊销；改名的 fork 或不想依赖产品账号的人自己建 App 填变量（G6 写清楚）。 |
| 权限范围 | `repo read:org workflow` | 推代码、开 PR、常用 `gh` 命令；`workflow` 用于改 `.github/workflows`。 |
| 存在哪 | 服务器 D1，AES-GCM 加密，AAD `github:<memberId>`，密钥沿用 `HOOK_SECRET_KEY` | 见下段比较。 |
| 怎么交给任务 | 不进 `run.assign`、不进 CLI 环境变量。git 凭证助手和 `gh` 代理用到时向 daemon 要，daemon 每次都用机器令牌向服务器取，不缓存 | 帧会落盘；环境变量会被打印和继承；不缓存则断开立刻生效，不需要「忘掉令牌」帧。 |
| 没连接时 | 助手立刻失败，不在助手里等；运行以「在等人连 GitHub」结束；他连上后服务器自动起跟进 | CLI 的 shell 工具 2 分钟就杀进程（`machine-protocol.md:386`），`chat-run` 客户端也只等 20 s，原地等做不成。 |
| 署名 | 作者是人，提交者是 AI 成员 | 贡献算在人身上；GitHub 显示「X authored，{成员} committed」；只靠环境变量，不改仓库、不写钩子。 |

**为什么存在服务器，不封给每台电脑**：封给电脑时，令牌在人的应用里，应用要给每台有资格的电脑各封一份；新加电脑要等他打开应用；接力、重试、集成事件常在人不在线时发生；失效检测也没地方做。存在服务器的代价是服务器运营者能解密；但团队里运营者就是管理员，管理员本来就管着团队主机，令牌运行时总要到那台主机上，封给电脑挡不住他；一个人用时服务器就是他自己的。AI 服务的 key 已经这样存（已定 2），负责人原话也是「我们会保存」。

### 3.2 一个任务用谁的身份（身份人）

服务器派活时按顺序算，第一条命中就用：

1. 有真人交代（运行的 `requested_for`，即协议的 `BotRun.requestedFor`，规则见 design-roles 3.3）：用他。私密卡片作答等 `requested_by` 为 `anonymous` 的，`requested_for` 就是 desk 拿到的真作答人 `claim.askerId`（`runs.ts:381`），不落到第 3 条。转交、分出去、接力沿用最初交代人，重试复用原请求，都由 `requested_for` 带过去，走这条。
2. 集成事件（交代人是 hook 对应的成员）：用 `hooks.created_by`。
3. 没有人交代（定时、AI 成员自己起的）：用这个 AI 成员的 `members.created_by`。

算出的人已被移出团队（`members.deactivated = 1`）时，改用团队里最早的在用的人类管理员（`level = 'owner'` 且 `kind = 'human'`；design-roles 里 `level='owner'` 就是管理员，含所有者，所以合并前后是同一条查询）。理由：没人负责的事落给还在的负责人，不 @ 已经走了的人。

跟进是新运行，按新消息的作者重新算：A 等着连 GitHub 时 B 说「继续」，这次就用 B。这是有意的：谁说话谁负责。

**电脑分两类模式**：

| 电脑 | 身份人已连 | 没连 |
| --- | --- | --- |
| 身份人自己的个人电脑，且本机执行（不开虚拟机） | `app`，作者用他本机 gitconfig | `machine`：电脑上现成的 gh、SSH、gitconfig（就是他自己，和今天一样） |
| 别人的个人电脑（只会因规则 2、3 发生），本机执行 | `machine` | `machine` |
| 团队电脑，或任何虚拟机执行 | `app` | `app`：没连，用到时请他连 |

「个人电脑」「团队电脑」按 [design-roles.zh.md](design-roles.zh.md) 的电脑归属字段。它合并前：本机执行且 `machines.created_by` 等于身份人才算个人电脑，其余一律按团队电脑。虚拟机执行永远走 `app`：虚拟机里没有主人的 SSH，家目录里只有以前借来的登录，正是机 4.1。电脑是否虚拟机执行由 daemon 在 `hello.machine.runsIn` 里报（见 3.5）；旧 daemon 没报的不发 `github`，行为不变。

### 3.3 数据和存储（D1）

G2 的迁移 `server/migrations/0017_github_accounts.sql`：

```sql
CREATE TABLE github_accounts (
  member_id TEXT PRIMARY KEY REFERENCES members(id),
  github_user_id INTEGER NOT NULL UNIQUE,  -- 一个 GitHub 账号只连一个成员
  login TEXT NOT NULL,
  name TEXT,                               -- GitHub 资料名，空则用 login
  token_enc TEXT NOT NULL,                 -- v1.<iv>.<ct>，AAD = 'github:' || member_id
  scopes TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('ok', 'invalid')),
  connected_at INTEGER NOT NULL,
  checked_at INTEGER NOT NULL
);
CREATE TABLE github_device (               -- 进行中的设备码登录，15 分钟过期
  id TEXT PRIMARY KEY, member_id TEXT NOT NULL,
  device_enc TEXT NOT NULL,                -- 加密的 device_code，AAD = 'github-device:' || id
  interval_s INTEGER NOT NULL, expires_at INTEGER NOT NULL, created_at INTEGER NOT NULL
);
```

G3 的迁移 `server/migrations/0017_github_runs.sql`（同号允许，见 AGENTS）：

```sql
ALTER TABLE bot_runs ADD COLUMN github_member_id TEXT;  -- 派活时算出的身份人
CREATE TABLE github_waits (                             -- 在等某人连 GitHub 的运行
  run_id TEXT PRIMARY KEY, member_id TEXT NOT NULL,
  group_id TEXT NOT NULL, topic_id TEXT NOT NULL, bot_id TEXT NOT NULL,
  reason TEXT NOT NULL CHECK (reason IN ('not_connected', 'invalid')),
  created_at INTEGER NOT NULL
);
CREATE INDEX github_waits_member ON github_waits(member_id);
```

- 连接成功时写对照表：`INSERT INTO github_logins (login, member_id, updated_at) VALUES (lower(?), ?, ?) ON CONFLICT(login) DO NOTHING`。理由：owner 手工设的不被覆盖。断开不删。
- 成员被停用时：core 已有 feature 钩子 `registerMemberDeactivated`（`server/src/core/registry.ts`，`deactivateMember` 之后由 `afterDeactivate` 调用），github 模块用它删他的 `github_accounts`、`github_device` 行；G3 用同一个钩子删自己的 `github_waits` 行。
- 运行是否还活着不另加列，取令牌时问 desk DO（3.4）。

### 3.4 服务端

新目录 `server/src/github/`，`index.ts` 由 G2 建，暴露 `registerGithub()` 和一个空的 `machineRoutes` 注册点，G3 只往里追加。所有路由用 Zod 校验。外部地址 `GITHUB_BASE`（默认 `https://github.com`）和 `GITHUB_API_BASE`（默认 `https://api.github.com`）可由变量覆盖，供测试指向假 GitHub。

| 路由 | 鉴权 | 做什么 |
| --- | --- | --- |
| `GET /v1/me/github` | 成员 | `{ available, unavailableReason?, account?: { login, name?, status, connectedAt }, waits: { runId, groupId, topicId, botId, reason }[] }` |
| `POST /v1/me/github/device` | 成员 | 调 `login/device/code`，存 device 行，回 `{ deviceId, userCode, verificationUri, interval, expiresAt }` |
| `POST /v1/me/github/device/:id/poll` | 本人 | 调 `login/oauth/access_token`，回 `{ state: "pending" \| "slow_down" \| "done" \| "expired" \| "denied" \| "taken", account? }`。done：`GET /user` 取 id/login/name，加密保存，删 device 行，写对照表，给本人会话推 `github.status`，然后恢复他所有的等待（见下）。 |
| `DELETE /v1/me/github` | 本人 | 删行，推 `github.status`（无 account）。 |
| `GET /v1/machine/runs/:id/github` | 机器令牌 | 取令牌，检查见下。 |
| `POST /v1/machine/runs/:id/github/wait` | 机器令牌 | daemon 报「要用但没连/失效」，body `{ reason }`。 |
| `POST /v1/machine/runs/:id/github/rejected` | 机器令牌 | git 报认证失败后重新验一次令牌。 |

- `available`：`client id` 和 `HOOK_SECRET_KEY` 都有才为 true。缺哪个，`unavailableReason` 给 `no_client_id` 或 `no_secret_key`（github 模块自己检查，不把 `hooks are not configured` 透出去）。
- `GET .../github` 依次检查：`bot_runs.machine_id` 是这台电脑；`github_member_id` 非空；向 desk DO 发新 RPC `run.live(runId)`，确认 `ended_at IS NULL` 且 DO 里的 `machine_id` 也是这台电脑（任何结束路径——完成、取消、超时、租约丢失、转到别的电脑、`external.ts:277`——都落到 DO 的 `ended_at` 或换 `machine_id`，所以只查这一处）；电脑对此人有资格（3.2 表格不是 `machine` 的那几格）；此人已连且 `status = 'ok'`。
  - 通过：`{ token, login }`。没连：404（`not_found`）`github.not_connected`。失效：409（`conflict`）`github.invalid`（`ErrorCode` 是封闭的，没有对应 410 的值，所以用 `conflict`）。没资格或运行已结束：403（`forbidden`）`github.not_allowed`。
  - 每次发放写审计 `github.token.issue`（runId、machineId、memberId，不记值）。
- `/wait`：`INSERT INTO github_waits ... ON CONFLICT(run_id) DO NOTHING`，只有真插入了一行才动作，所以每次运行只处理一次且不怕并发：
  - 由这个 AI 成员在话题里发一条普通消息 @ 身份人：没连时「@{名字} 这一步要用你的 GitHub。连接 GitHub 后我会自己接着做。」；失效时「@{名字} 你的 GitHub 连接失效了，重新连接后我会自己接着做。」
  - 不另发事件；@ 本身带系统通知。
- **恢复**：此人连接成功时，对他每条 `github_waits` 调 desk DO 新 RPC `github.resume(runId)`，删行。desk 以那次运行为 `follow_up_of` 起一条跟进，交代人沿用原运行，提示里写「{名字} 已连接 GitHub，从停下的地方接着做」。原运行还没结束（AI 成员还在汇报）就等它结束再起，和现有「运行结束后有剩余输入」同一条路（`events.ts:386` 的深度上限照样生效）。等待的运行若已被别人跟进过，不重复起。
- `/rejected`：用 `GET /user` 验一次；401 就标 `invalid` 并推 `github.status`。只在这里验，不做定时检查（OAuth App 令牌不过期）。
- 派活：`server/src/bots/desk/runs.ts:397` `assignFrame` 里加一行调用 `githubFor(r, claim, machine)`（新文件 `server/src/github/assign.ts`）。只在电脑报了 `github` 功能时填 `frame.github`，并写 `bot_runs.github_member_id`。
- 令牌不进 `run.assign`、DO 存储、运行事件、日志、审计详情。

### 3.5 协议（只加不改）

`docs/protocol.md`、`packages/protocol`、`crates/chat-protocol`、`protocol/fixtures` 同一提交改。

成员客户端：

```ts
GithubAccount = { login, name?, status: "ok" | "invalid", connectedAt }
// 新的临时事件（TransientEvent），只推给本人那些 hello.features 带 "github/1" 的连接；没有 account 表示已断开
{ type: "github.status", account?: GithubAccount }
```

G1 已确认：Rust 的 `TransientEvent` 遇到不认识的 `type` 会解析失败（`protocol/fixtures/transient/unknown-type.json` 锁着），所以 `github.status` 只发给在 hello 里声明了 `github/1` 的连接（和 `digest` 只发给 `settings/1` 一样）；旧客户端收不到，靠 `GET /v1/me/github` 拿（应用回到前台和打开设置时取）。G5 在桌面端的 hello 里加 `github/1`。

所有新的状态、原因、`runsIn`、`source` 在协议里都是开放字符串（Zod `z.string()`、Rust `String`，已知值导出成常量），新值不会让旧读者解析失败；下面写成联合只是列出已知值。

电脑（`docs/machine-protocol.md`，新功能 `github`）：

```ts
hello.machine += { runsIn?: "host" | "vm" }   // 有 github 功能的 daemon 必报
run.assign += { github?: RunGithub }           // 只发给报了 github 的 daemon
RunGithub = {
  source: "app" | "machine",
  memberId: string,                            // 身份人
  login?: string,                              // 已连时
  author?: { name: string, email: string },    // 见 3.6「署名」；缺省表示用本机 gitconfig
  committer: { name: string, email: string },  // 这个 AI 成员
  rewriteSsh?: boolean                         // 团队电脑和虚拟机为 true
}
// RunEvent 新类型（过程视图里能看到）
{ type: "github", state: "used" | "missing" | "invalid", login? }
```

### 3.6 daemon

新文件 `apps/daemon/src/github.rs`；`daemon.rs:28` 的 `FEATURES` 加 `github`；hello 里报 `runsIn`。

**署名**（`run.rs:98` `build_env` 末尾，`a.github` 有值时）：

- 提交者：两种模式都设 `GIT_COMMITTER_NAME/EMAIL` 为 AI 成员（名字是显示名，邮箱 `ai-<memberId>@users.noreply.invalid`）。
- 作者：`author` 有值才设 `GIT_AUTHOR_NAME/EMAIL`。服务器只在团队电脑和虚拟机上给：已连时是 GitHub 名字和 `<id>+<login>@users.noreply.github.com`；没连时是他的成员显示名和 `<memberId>@users.noreply.invalid`（不然虚拟机里第一次提交就报错，机 4.5）。在自己的个人电脑上不给，沿用本机 gitconfig，公司邮箱这类要求不受影响。

**凭证（`app` 模式）**，都用命令行级 `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n`：

- `credential.https://github.com.helper` = 空串（清掉系统、全局里的助手，包括 Git for Windows 的 `credential.helper=manager`，只对 github.com）。
- 再加 `credential.https://github.com.helper` = `!chat-run git-credential`。
- `rewriteSsh` 时加 `url.https://github.com/.insteadOf` = `git@github.com:` 和 `ssh://git@github.com/`，并去掉 `SSH_AUTH_SOCK`。
- 不把令牌放进任何环境变量。

**daemon 自己的 git**：镜像克隆、fetch、`git worktree add` 在 `app` 模式下用同一套环境（清掉 github.com 的助手、`rewriteSsh` 的改写），令牌由 daemon 进程内直接取（不经 socket），只作为这几条 git 子进程的 `http.https://github.com/.extraHeader`（`GIT_CONFIG_*`，不进镜像配置、不进 CLI 环境）；这时 CLI 还没启动。取不到（没连或失效）时不带凭证试一次，公开仓库照常；私有仓库失败就报 `/wait`，运行以等待结束（发 `github` 事件 `missing`），不启动 CLI。镜像按仓库地址在一台电脑上共用，不按人分：B 的运行能读到 A 的令牌 fetch 下来的分支。这一版接受，写进第 5 节。

**`gh` 代理**（`app` 模式）：

- 本机：`install_link`（`chat_run.rs:296`）在 `bin/` 里多放一个指向 daemon 的 `gh`（Windows 上 `gh.exe`）。
- 虚拟机：工具盘加 `/opt/runner/bin/gh` 指向 agent 程序（`firecracker.rs:4,87`），因为虚拟机里 PATH 是 `GUEST_PATH`，`bin/` 不在上面。工具盘由 daemon 自己做（`firecracker.rs` `stage_tools`），不是 `scripts/runner/` 的脚本。
- 代理只在环境里有 `CHAT_GITHUB_AS`（`app` 模式才设，值是身份人的名字，不是秘密）时取令牌和拦 `gh auth login`；没有时原样运行真 gh，所以旧服务器、`machine` 模式下虚拟机里的 gh 行为不变。
- 代理按启动名分流：向 daemon 要令牌，从 PATH 里跳过 `bin/` 和 `/opt/runner/bin` 找真 `gh`，带 `GH_TOKEN` 运行它，透传输入输出和退出码。真 gh 及它起的子进程（扩展、别名）会继承 `GH_TOKEN`，这一版接受。
- `gh auth login|logout|refresh|setup-git` 直接拦下，输出 `GitHub sign-in is handled by the app for <name>; do not sign in here.`，退出码 1。
- 没装 gh 就照原样报没装。

**chat-run 新操作** `git-credential`、`github.token`：和 `secrets::OP` 一样在 daemon 的 `chat_run::handle` 里提前分支本地处理（`chat_run.rs:644` 旁边），绝不走 `outputs.add` 上传。每次调用都向服务器取一次，不缓存。

- `git-credential get`：`host=github.com` 返回 `username=x-access-token`、`password=<令牌>`；别的 host 返回空。
- `git-credential erase`：调 `/rejected`。
- 404/409：发 `github` 事件 `missing`/`invalid`，调 `/wait`，立刻返回失败。助手在 stderr 写一行：`GitHub is not connected for <name> (they have been asked). Stop and report that you are waiting for them to connect GitHub.` git 随即报认证失败。
- 文件通道（Codex）：答案照现有规则写 mode 600 的 `.res`，`chat-run` 读完即删；测试确认调用后没有 `.res` 留下。

**脱敏**：每个运行的 redactor 改成可追加：`Redactor::for_run` 给运行一个 `Arc<RwLock<Vec<String>>>` 额外值列表，daemon 循环里这次运行的帧、`events.jsonl`、产出文件、会话上传（`run.rs:462`）的脱敏都读它；`stderr.log` 是 CLI 直接写的文件，运行结束后有追加值时整份过一遍脱敏重写；github 模块每取到一次令牌就追加。`redact.rs:41` 的规则继续兜底。chat-run 的操作日志只记 ok 或失败。

**虚拟机家目录**：有 `github` 字段的运行开始前，在宿主机上删掉 `<homes>/<botId>/.config/gh/hosts.yml` 和 `.git-credentials`，并把家目录 `.gitconfig` 里 github.com 的 `credential.*helper` 和带令牌的 `url.*` 段删掉（`firecracker.rs:377` 附近）。日志只写 `removed an old GitHub sign-in`。所有虚拟机运行都走 `app`，所以这一步每次都做。

**给 CLI 的系统说明**（英文，一行或两行）：

> GitHub: git and gh act as <login> (<name>, who asked). Do not run gh auth login. If git says GitHub is not connected, stop and report that you are waiting for <name>. If GitHub refuses because of "OAuth App access restrictions", stop and report that an owner of that organization must approve the app at https://github.com/organizations/<org>/settings/oauth_application_policy.

`machine` 模式：不装代理、不设助手，只设提交者。

### 3.7 桌面端（Mac、Windows 同一套代码）

- **连接 sheet**（新文件 `apps/desktop/src/views/settings/github.rs`，gpui-component 的 sheet、button、spinner）：大字等宽显示 8 位验证码，带「复制验证码」；「打开 GitHub」复制验证码并用浏览器打开 `verificationUri`。按 `interval` 轮询，成功显示「已连接为 @login」，1 秒后关。过期、取消、账号已被占用各一句，都带「再试一次」。底部一行小字提醒组织授权。
- **设置 › 账号**（`views/settings/account.rs`）新增「GitHub」一组：未连接显示「连接…」；已连接显示 `@login` 和「断开…」（确认对话框，带去 GitHub 撤销的链接）；失效显示「需要重新连接」和「重新连接…」。`available` 为 false 时：owner/管理员看到「服务器还没配好，暂时不能连接 GitHub」，其他人不显示这一组。
- **上手**：第一次登录成功后，`available` 为 true 且没连接，弹一次连接 sheet，带「以后再说」。每个账号每台设备只弹一次，记在本地偏好。
- **话题横幅**：打开话题、收到 `github.status`、应用回到前台时取 `GET /v1/me/github`；`waits` 里有这个话题，就在话题顶部显示「{成员}在等你连接 GitHub」或「{成员}在等你重新连接 GitHub」，带「连接 GitHub」按钮，点了直接弹连接 sheet。连上后横幅消失，任务自己继续。
- **电脑页**：daemon 没有 `github` 功能的电脑显示「这台电脑更新后，任务才能用你连接的 GitHub」。

**新增界面文案**（`apps/desktop/src/copy/settings.rs` 新一段 `github`）：

```text
GitHub
连接 GitHub
连接…
重新连接…
断开…
已连接为 @{login}
未连接
需要重新连接
AI 成员替你拉代码、推代码、开 PR 时用这个账号
在 GitHub 页面输入这串验证码
复制验证码
打开 GitHub
正在等你在 GitHub 上确认…
验证码过期了
你在 GitHub 上取消了授权
这个 GitHub 账号已经连在{name}上
再试一次
以后再说
要用组织里的仓库，在 GitHub 授权页上给那个组织点「Grant」或「Request」
断开 GitHub？
断开后，AI 成员不能再用你的 GitHub。要彻底收回，到 GitHub 的设置里撤销授权。
去 GitHub 撤销
服务器还没配好，暂时不能连接 GitHub
{成员}在等你连接 GitHub
{成员}在等你重新连接 GitHub
这台电脑更新后，任务才能用你连接的 GitHub
```

没有新产品名词：GitHub 是专名，验证码是 GitHub 自己的说法，成员、电脑沿用现有用词。AI 成员在话题里发的两句 @ 消息放在服务端文案（`server/src/github/text.ts`），同样受用词约束。

### 3.8 一个人用时

- 自己部署的服务器、自己的 Mac：第一次登录弹一次「连接 GitHub」，不到 1 分钟。
- 点「以后再说」：他的电脑是个人电脑、本机执行，走 `machine`，用他电脑上现成的 gh、SSH、gitconfig。和今天唯一的不同：提交者变成 AI 成员（作者还是他）。
- 连了：作者照旧是他本机 gitconfig；推代码用保存的令牌，换一台自己的 Linux 主机（虚拟机执行）也能直接推，不用在虚拟机里再登录。
- 只有一台 Linux 主机、用虚拟机执行、又没连：任务一用到私有仓库就停下 @ 他；他连上后自己接着做。
- 他看不到任何团队概念：没有「各人」「管理员」字样。

### 3.9 团队用时

- 每人上手时连接自己的 GitHub。A 交代的事用 A 的推送权限，B 的用 B 的。
- 团队主机不用主机上的任何登录，也不用家目录里留下的旧登录。
- 没连的人第一次交代要用 GitHub 的事：被 @ 一次，话题里有横幅；点一下连上，任务自己接着做。
- 公司组织限制 OAuth 应用时，AI 成员在话题里说清要组织 owner 批准，并给出设置页地址。
- 有人离开团队：他的连接删掉；他建的 hook、加的 AI 成员之后的任务落到最早的 owner 身上。
- 管理员能看到也只能看到审计：谁的令牌在什么时候被哪台电脑、哪次运行取过。

## 4. 兼容和迁移

- **旧客户端**：不认识新路由和事件，不会连接（G1 确认未知事件被忽略）。他们被 @ 的是普通文字消息，能看到；更新应用后就能连。在自己的个人电脑上走 `machine`，行为不变。
- **旧 daemon**：没有 `github` 功能，服务器不发 `github` 字段、不发令牌，行为和今天一样。线 8 的自动更新会把它们带上来。
- **旧服务器配新 daemon**：没有 `github` 字段，daemon 什么都不装，也不删家目录文件。
- **旧数据**：`bot_runs.github_member_id` 允许为空，老运行没有身份人，不能取令牌。`github_logins` 原样保留。虚拟机家目录里的旧登录在第一次带 `github` 字段的运行时删除。
- **没配 `HOOK_SECRET_KEY` 的服务器**：G6 让自部署脚本在缺这个 secret 时自动生成并 `wrangler secret put`（AI 服务 key 也要它），INSTALL 写明。仍然缺时 owner/管理员在设置里看到原因，所有电脑按 `machine` 处理（团队电脑和虚拟机不发 `github`），即今天的行为。

## 5. 安全

- 服务器只存密文；AAD 绑定 memberId（device 行绑定 id），行之间不能互换。
- 令牌只从 `GET /v1/machine/runs/:id/github` 流出，且只给：分到这次运行、DO 里还没结束、对这个人有资格的电脑。设备码轮询时令牌留在服务器，不回给应用。
- 电脑上不缓存、不写文件、不进 CLI 环境变量、不进家目录。每次 git 或 gh 用到时现取，交给那个 git 进程，或那个 gh 进程及其子进程。Codex 文件通道的 `.res` 文件只在毫秒级时间内存在，mode 600。
- 执行者权限全开（已定方向），运行期间任务能用 `git credential fill` 拿到令牌。范围是这个人自己的 GitHub 权限，只在他交代的那次运行期间。
- 不是完全按人隔离：AI 成员的家目录和电脑上的镜像被不同交代人的运行共用。A 的运行克隆下来的内容、写进家目录的东西，B 之后的运行能读到；推送权限是按人的，读不是。这一版接受，文档写清；虚拟机家目录里带令牌的 `url.*` 和凭证文件每次运行前清掉。
- 脱敏：取到的值追加进这次运行的精确匹配列表，加上 `gh*_` 规则；运行事件、`stderr.log`、原始会话上传都过脱敏。
- 断开或被停用：删服务器那一行，正在跑的任务下一次 git/gh 就取不到。要在 GitHub 那边彻底失效得本人去撤销（没有 client secret，服务器替不了），界面给链接。
- 失效检测：只在 git 报认证失败时验一次，401 标失效并通知本人。
- 设备码 15 分钟过期，`device_code` 加密保存，只有本人能轮询自己那条。
- 团队电脑和虚拟机去掉 `SSH_AUTH_SOCK`、SSH 地址改写成 HTTPS，不会用到主机自己的 key。

## 6. 拆成哪些 PR

前置操作（不是产品决定）：负责人用产品账号在 GitHub 建 OAuth App，开 Device Flow，把 client id 写进 `product.json` 的 `githubClientId`。在那之前本地和 staging 用 `GITHUB_CLIENT_ID` 变量，不阻塞开发。

| PR | 做什么 | 主要文件 | 依赖 | 怎么测 |
| --- | --- | --- | --- | --- |
| G1 协议（并进四份设计共用的协议 PR，新模块 `github.ts` / `github.rs`） | 3.5 的字段、事件、`runsIn`、`github` RunEvent、fixtures；确认未知事件被忽略 | `docs/protocol.md`、`docs/machine-protocol.md`、`packages/protocol/src/`、`crates/chat-protocol/src/`、`protocol/fixtures/` | — | 两边 fixture 测试；未知事件解码测试 |
| G2 服务端连接 | `0017_github_accounts.sql`、加密存储、设备码路由、`/v1/me/github`（`waits` 先返回空数组）、断开、`available` 和原因、对照表、停用钩子（用现成的 `registerMemberDeactivated`）、`github/index.ts` 注册点 | `server/src/github/{index,routes,store,api,hooks}.ts`、`server/src/features.ts` | G1 | vitest：mock GitHub fetch，覆盖 pending、slow_down、done、expired、denied、taken；库里和日志没有明文；对照表不覆盖手工行；停用后行被删 |
| G3 服务端派活 | 身份人规则（含 anonymous、离开团队）、`frame.github`、`0017_github_runs.sql`、机器取令牌、`/wait`、`/rejected`、恢复跟进、desk RPC `run.live` 和 `github.resume`、`waits` 查询、话题 @ 文案 | `server/src/github/{assign,machine,resume,text}.ts`；`server/src/bots/desk/runs.ts:397` 一行；`server/src/bots/desk/github.ts`（新，两个 RPC）；往 `github/index.ts` 追加 | G1、G2。design-roles 没合并就按 3.2 的临时规则 | vitest：3.2 表格每一格；规则 1–3 和回落；别的电脑取令牌被拒；完成、取消、超时、租约丢失、转电脑后取令牌都被拒；`/wait` 并发只发一次 @；连上后起跟进且只起一次 |
| G4 daemon | `github` 功能和 `runsIn`、署名环境、助手、`gh` 代理（本机和工具盘）、daemon 自己 fetch 的凭证、可追加脱敏、家目录清理、系统说明 | `apps/daemon/src/github.rs`（新）、`run.rs`、`chat_run.rs`（本地分支）、`daemon.rs`（FEATURES、hello、每次运行的 redactor）、`workspace.rs`（daemon 自己 git 的环境）、`redact.rs`、`executor/firecracker.rs`（工具盘由 daemon 做） | G1 | cargo test：mock 服务器，本地 http 远端上跑 `git credential fill`、`git push`；`gh auth login` 被拦；404 立刻失败并报 `/wait`；`stderr.log` 和会话上传里令牌被遮；文件通道调用后无 `.res`；guest PATH 解析 `gh` 到代理；Windows CI 上 `gh.exe` 分流，并验证空助手清掉系统级 `credential.helper=manager` |
| G5 桌面端 | 连接 sheet、账号页一组、上手弹一次、话题横幅、电脑页提示、文案、截图自测数据 | `apps/desktop/src/views/settings/{github,account}.rs`、`copy/settings.rs`、`data.rs`、`provider/` 的 HTTP 调用、`fake/`（只为截图自测） | G1；横幅的数据要 G3，之前按 G2 的空 `waits` 做 | 文案目录测试；截图自测（浅色、深色、Mac、Windows）：未连接、等确认、已连接、失效、不可用、横幅 |
| G6 文档、部署和端到端 | 自部署脚本生成 `HOOK_SECRET_KEY`；INSTALL 写 client id 覆盖和自建 OAuth App；上手指南加连接 GitHub；改 `machine-protocol.md:816` 那段；`tools/e2e` 场景 | `scripts/deploy-server.sh`、`docs/INSTALL*.md`、`docs/ONBOARDING.zh.md`、`docs/machine-protocol.md`、`tools/e2e/` | G3、G4 | 照文档在新 Ubuntu 上部署一遍；e2e：假 GitHub（`GITHUB_API_BASE`）下，没连 → @ → 连接 → 自动跟进推送成功；集成事件用 hook 建立人；重试仍用原交代人 |

- G2、G4、G5 改的文件互不重叠，可以并行。G3 只在 `desk/runs.ts` 加一行、desk 加一个新文件，要和线 1–3 改 desk 的会话错开合并。
- 沙箱：连接要真 GitHub，`scripts/sandbox/seed.ts` 不造连接；本地开发用开发者自己的 OAuth App 走真设备码（不需要回调地址）。

## 7. 真电脑上的验收步骤

每一步留截图或日志。一个测试仓库（私有），A、B 两个 GitHub 账号。

1. **Mac，一个人用**：新部署服务器（照 INSTALL，确认 `HOOK_SECRET_KEY` 自动生成），第一次登录弹「连接 GitHub」，点「以后再说」。让 AI 成员改仓库开 PR：用电脑上现成的 gh，作者是本人 gitconfig，提交者是 AI 成员。
2. **同一台 Mac**：设置 › 账号里连接 A，再交代同样的事。PR 由 A 创建，提交显示「A authored，{成员} committed」；过程视图和原始会话下载里搜 `gho_` 找不到。
3. **Windows**：重复第 2 步，确认 `gh.exe` 代理生效，Git for Windows 的 manager 助手没被用到。
4. **Linux 团队主机（虚拟机）**：A、B 通过邀请码进团队，A 已连，B 没连。A 交代的事以 A 身份拉取私有仓库并推送成功。主机上成员家目录没有 `hosts.yml`、`.git-credentials`；任务里跑 `env | grep -i -e gh -e token` 没有令牌；`which gh` 是 `/opt/runner/bin/gh`。
5. **同一台主机，B 交代**：话题里出现 @B 的消息，B 的应用里话题顶部有横幅；运行以「在等 B 连接 GitHub」结束，话题不标完成。B 点横幅连接，不说话，几秒内自动起跟进并推送成功，作者是 B；A 的令牌没有出现在 B 这次运行的审计里。
6. **另一人说「继续」**：重复第 5 步的等待，这次 A 在话题里说「继续」，跟进用 A 的身份推送（有意的行为）；B 之后再连接不会再起一次。
7. **转交**：A 交代给成员甲，甲转交给乙，乙的推送用 A。
8. **重试**：让运行失败后重来，仍用 A。
9. **集成事件**：hook 由 A 建，事件触发的任务用 A；把 A 停用后再触发，用最早的 owner，@ 的是 owner。
10. **断开和撤销**：A 在运行中断开，下一次 `git push` 取不到令牌，走等待流程。A 重连后到 GitHub 撤销授权，下一次推送被标「需要重新连接」，A 被 @ 并看到横幅；重连后任务自己接着做。
11. **组织限制**：用开了 OAuth 应用限制的组织仓库，AI 成员在话题里说明要组织 owner 批准并给出地址。
12. **旧 daemon**：主机 daemon 保持旧版本，任务行为和现在一样，电脑页显示「这台电脑更新后，任务才能用你连接的 GitHub」。

## 8. 这一版不做的

- GitHub Enterprise Server、粘贴令牌、从 `gh` 导入现成登录；GitLab 等其他平台。
- 按人隔离读取：家目录和镜像按人分开。
- 没连时已做的提交，作者邮箱是占位邮箱；连上后不改写历史。
- 提交签名（GPG、SSH signing）；`Co-authored-by` trailer。
- 给 AI 成员单独配 GitHub 账号（机 4.1 方案 B）。
- 管理员在成员列表里看每人连没连。
- 服务器替人到 GitHub 撤销授权（要 client secret）；令牌轮换；换用会过期的 GitHub App 用户令牌。
- 定时检查令牌是否还有效。
- `GITHUB_TOKEN` 这类环境变量形式给其他工具；其他工具可以用 `gh auth token`（经代理）。
- 拦住 gh 扩展、别名起的子进程继承 `GH_TOKEN`。

## 9. 还要负责人定的

无。唯一要负责人动手的是第 6 节开头的前置操作：建 OAuth App、把 client id 写进 `product.json`。

### 对两份评审的处理

两份评审的意见都已采纳，只有一条没照原样做：评审二建议在助手或 gh 代理里检测组织限制返回的 403。没这么做，因为助手看不到 HTTP 响应，代理要截取 stderr 才能看到。改成在系统说明里写明这种情况，由 AI 成员在话题里报告，效果相同，也更简单。
