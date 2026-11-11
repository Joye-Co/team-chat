# 对标 OpenBot：模型层、harness 层、机器层

日期 2026-11-12。只读研究，没有改代码。

- OpenBot 读的是本地检出的源码（HEAD `ac99196e6`，2026-11-01，版本 0.58.0）和官网 openbot.run（首页、/pricing、/teams、/compare）。
- 许可是 PolyForm Noncommercial：下面只写做法，不抄代码。
- 路径不带前缀的都是 OpenBot 仓库里的路径；`我们：` 后面的路径是本仓库。
- 「未核实」= 源码或官网里没找到证据。

## 0. 两个产品的形态差别（先看这个）

| | OpenBot | 我们 |
| --- | --- | --- |
| 是什么 | 本地优先的桌面应用。一台「server」= 一个 OpenBot 进程 = 一台电脑，上面跑若干 agent | 团队聊天（服务端在 Cloudflare）+ 分配给 AI 成员的电脑（daemon） |
| 一个人能不能直接用 | 能。下载、连一个模型供应商、一句话描述 agent、开始用。个人免费（官网 /pricing） | 不能。先要有人部署服务端并发邀请码（`docs/INSTALL.zh.md`、`docs/ENVIRONMENTS.md`） |
| 协调者在哪 | 和执行者在同一台电脑上，用 agent 自己的 CLI 和订阅 | 在服务端，要另配一把 API key（`server/src/bots/desk/front.ts:44`） |
| 成熟度 | 版本 0.58.0，架构文档约 1.2 万行，很多机制有专门的 e2e 脚本（README "Commands"） | 前台、盯活、记忆在真模型下没跑过 |

## A. 模型层

| 项 | OpenBot 怎么做 | 我们现在 | 差距 | 值得借的 |
| --- | --- | --- | --- | --- |
| 支持哪些 CLI | Codex、Claude Code、Grok、OpenCode、Gemini（Antigravity）、Cursor、Cline、Pi、Muse，外加任意 ACP agent（`src/backend/provider-drivers.ts`，每家一个 driver）。自定义 ACP agent 可以保存、检测、从 ACP registry 安装（`docs/architecture/providers.md` "Custom agents"、"ACP registry"） | `codex`、`claude` 两种是一等的；`acp`、`pty` 要手写 daemon.toml（`docs/machine-protocol.md` Runtimes） | 我们对外说「harness 可选」，实际只有两家能在界面里选 | 每家一个 driver，差异（模型列表、登录、用量、能不能并发）都写在 driver 里，不散在各处 |
| 支持哪些模型供应商 | 订阅登录：ChatGPT、Claude、Google AI Pro、Grok、Cursor。只要 key 的：DeepSeek、OpenRouter、Inception（都跑在 Pi CLI 上，`providers.md` "DeepSeek"、"OpenRouter"）。任意 OpenAI 兼容地址；自动发现本机 Ollama、LM Studio（`providers.md` "Local detection"）。OpenCode 的免费模型不用账号 | 执行者：跟着 CLI 的订阅登录。服务端：anthropic、deepseek、openai 兼容（`server/src/aiservice/providers.ts`） | 没有「不用订阅也能跑」的路；没有本地模型 | key 型供应商借一个通用 CLI（Pi）来跑，不用给每家写执行器 |
| CLI 谁来装 | OpenBot 自己下载并固定版本，启动时和每小时检查更新，不动用户系统里的 CLI（README "Agent setup"；`native-runtime.lock.json`） | 要人先在电脑上装好并登录（`docs/bots.md` 第 6 节第 1 步）；daemon 只更新自己（`apps/daemon/src/selfupdate.rs`） | 新电脑第一步就要开终端 | daemon 托管 CLI 的下载和更新，带校验 |
| 模型在哪选 | 每个 agent 一个模型 + 思考强度，在聊天头部和 agent 设置里选（`packages/ui/src/features/conversation/ConversationHeader.tsx`、`AgentSettingsPanel.tsx`）。agent 可以用工具改自己的模型（`src/backend/agent/model-tools.ts`）。频道没有模型设置，每个成员用自己的（`channels-and-messaging.md` 末段）。Claude 换模型和强度下一轮生效，不重开会话（`providers.md:85`） | 成员级模型和思考强度（#87）；#103 再加团队级；前台另一处。三处并存 | 我们的入口比他们多，但没有「换了下一轮就生效」和「排队中的切换」 | 模型是 agent 的属性，一处设置；运行中的改动记成 `pendingRuntime`，下一条消息开始前才生效（`agents.md` "Agent threads" 末段） |
| 模型列表从哪来 | 统一问每个 CLI 的 `model/list`，差异写在 driver 的 `catalog`（`src/backend/agent/model-catalog.ts`）。Codex 报 Fast 档 | Codex 用 `model/list`；Claude Code 只有四个别名（`docs/machine-protocol.md` "Models and effort"） | 基本持平 | — |
| 登录和凭证 | 界面里登录：设备码、粘贴码、打开页面三种，远程服务器上也能在自己设备上完成（`docs/hosted-servers.md` "Providers"；`src/backend/agent/cli-login-flow.ts`、`codex-login.ts`）。可以断开登录（`providers.md` "Disconnect"）。API key 存在加密存储里，agent 要 key 时用 `request_api_key` 工具弹表单，值不进对话（`src/backend/agent-api-keys.ts`、`agent/tool-catalog.ts`） | 能从应用里给电脑登录 CLI、下发密钥（`docs/machine-protocol.md` "Accounts and keys"）；密钥按成员限定 | 基本持平；我们没有「agent 自己向人要 key」的工具 | `request_api_key`：agent 缺凭证时发一张表单，人填完直接进密钥库 |
| 多账号 | 每个供应商最多 10 个账号（`packages/contracts/src/ipc-agent-status.ts:97`），每个账号一个独立目录，agent 指定用哪个（`src/backend/agent/provider-accounts.ts`）。Claude 还能加一个 API key 账号（`claude-api-key-v1`） | 一台电脑一份登录，所有成员共用 | 一个订阅撞线，这台电脑上所有成员一起停 | 账号是一等对象：每个账号单独目录、单独用量、成员绑定账号 |
| 额度用完 | CLI 报「计划拒绝了这一轮」后，`UsageLimitGate` 把这个账号 + 模型上所有 agent 的队列挂起到重置时间；没给重置时间就每 15 分钟读一次用量；重置后过 30 秒再放行（`src/backend/agent/usage-limit-gate.ts:9-20`）。有备用模型的 agent 自动挪过去，原模型恢复后挪回来（`providers.md` "Backup models and failover"）。Codex 的「重置额度券」可以在界面里用（"Usage reset credits"） | 没有处理。daemon 只把百分比报给监控（`apps/daemon/src/usage.rs`） | 撞线后任务怎么死的我们自己都不知道 | 撞线 = 挂起队列等重置，不是失败；界面显示「等到几点」 |
| 出错换模型 | 每个 agent 最多 8 个备用模型，有顺序。错误分六类：`billing`、`provider_unavailable`、`rate_limit`、`authentication`、`network`、`timeout`。这一轮还没动手（没写字、没跑工具）才重排到备用模型，原模型冷却 10 分钟；已经动过手就照常报错，不重跑（`providers.md` "Backup models and failover"） | 没有。#103 只补了 Codex 找不到模型时用默认模型 | 供应商抖一下，任务就失败 | 「没动过手才自动重试」这条线，避免重复对外动作 |
| 用量和成本 | 侧栏每个供应商一个环：5 小时、每周（OpenCode 还有每月）。读用量有节流：一次一个，4 分钟内复用，失败后 5、10、20、40 分钟退避（`providers.md` "Usage read pacing"）。每个 agent 一份用量报告，按 API 等价价格算，并写明「不是订阅扣费」（`src/backend/agent/usage-pricing.ts:3-5`、`packages/ui/src/features/usage/AgentUsageReport.tsx`） | 用量页按成员、提出人、电脑、运行方式汇总，按美元设每周预算（`server/src/usage`、`server/src/budget`）；订阅百分比只在机器监控里 | 我们的美元预算和订阅额度是两套，互不相干 | 把订阅窗口百分比放到成员能看见的地方；美元数标明是「等价价格」 |
| 便宜模型干杂活 | 记忆复盘用供应商的默认模型（比 agent 的模型便宜时），仍走 agent 自己的订阅（`providers.md:113-117`）。频道路由用 lead 自己的模型开一个无工具会话（`channels-and-messaging.md:163`） | 前台、整理要另配 API key，没配就整块不工作 | 他们的「前台」不需要第二套凭证 | 协调和整理先走成员自己 CLI 的便宜模型；API key 只是可选的加速 |

## B. harness 层

| 项 | OpenBot 怎么做 | 我们现在 | 差距 | 值得借的 |
| --- | --- | --- | --- | --- |
| 路由：谁来接 | 先规则，不调模型：指名的人、回复所属的任务、对唯一在做任务的明显跟进、只有一个成员的频道、开头的 @A @B、@everyone、「continue」。都不中才让 lead 判断，用 lead 的模型、无工具会话，输入是频道摘要不是全文，输出是固定格式（`src/backend/channel-service.ts`；`channels-and-messaging.md:160-180`）。答案解不出来就暂停任务并提供「重新指派」，不会广播 | 规则 + 前台一次模型调用四种决定（`server/src/agent/frontdesk.ts`）；没配 AI 服务就 @ 一下派一次 | 设计相近；我们的在真模型下没验证过，他们有 e2e | lead 选了谁，频道里留一行，人能改；规则选的不留 |
| 问几个人 | 一条请求可以给多个成员各开一个根任务，每人在写完自己的回答前看不到别人的；写完后跟进才能看到（`channels-and-messaging.md:167-176`） | 没有 | 「各说各的看法」做不了 | 这条和 Joye 定的「bot 之间不讨论」相容：并行各答，互不可见 |
| 委派和上限 | `assign`（子任务，父任务保留负责人）和 `transfer`（换负责人）。每个根请求最多自动派 8 次，每个频道同时 2 个在做的指派（`CHANNEL_PARALLEL_LIMIT`，`packages/contracts/src/ipc-chat-channels.ts:20`）。不能把任务派回给派它的人；派出去的可以查状态、打断（`agents.md` "Agent communication policy"） | `chat-run handoff/assign` 和服务端 `delegate.ts` 已上线，和 11-09 定的「bot 不给 bot 派活」冲突 | 我们的方向和代码打架；他们至少有硬上限 | 如果保留任何形式的接力：硬上限 + 修订号防止旧指派完成新请求 |
| 一个 agent 多件事 | 主对话之外可以开「side thread」，每个有自己的会话、队列、结果；默认同时跑 2 个，最多 4 个；最多 50 个打开的（`packages/contracts/src/ipc-agent-tasks.ts:8-10`；`agents.md:270-290`）。能从任一条消息「分叉」出新线程（"Branch from here"） | 一个话题一个会话；群聊里新请求开新会话 | 我们没有并发上限的设置，也没有分叉 | 分叉就是 Joye 要的「从某个节点走另一条路」：复制到那一轮为止的消息，新会话拿交接包 |
| 上下文怎么拼 | 稳定规则在前，资料和工作区设置居中，记忆在最后（`src/backend/agent/developer-instructions.ts`）。工具说明不全塞：目录只列名字，用 `tool_search`、`tool_describe` 按需取（`providers.md:6-40`）。CI 里量指令和工具定义的字节数，变大就挂（`bun run check:agent-context`） | 每条消息压成一行，预算 24k 字，话题第一条必带（#85） | 我们没有量过固定前缀有多大 | 上下文预算进 CI |
| 频道上下文 | 每轮一个自包含的包：目的、职责、当前请求、来源消息、共享决定、最近消息、附件引用；更早的用带版本的摘要；全文可翻页取（`channels-and-messaging.md:256-262`） | 上下文服务按 ID 取、搜索（`server/src/context`）；旧的 30 行路径还在（`server/src/bots/dispatch.ts:25`） | 持平，但我们两条路并存 | — |
| 压缩 | 用到 80% 就压缩（`src/backend/agent/context-compaction.ts:23`）。各家不同：Codex、Muse 走事件，Pi 走请求，Claude 自己管，不能压缩的就换会话带交接（`providers.md` "Compaction ownership"）。界面有上下文环，可以手动压缩 | 没有，靠 CLI 自己 | 长任务会撞窗口，我们看不见 | 上下文占用是界面上的一个数 |
| 换会话的交接 | 最多 32,000 字：最近消息 24k、更早摘录 4k、工作步骤 2k、说明 2k。不调模型做摘要。丢掉的原文用 `history_search`、`history_read` 取回（`providers.md:54-70`；`src/backend/agent/handoff-history.ts`） | 续接只发增量；跨电脑还原会话没做 | 换电脑、换 CLI 后上下文断 | 交接包 + 按需取回，两个都不花模型调用 |
| 崩溃恢复 | CLI 进程退出：按 0.5 s、1 s、2 s 退避重启 3 次（`src/backend/agent/provider-runtime.ts:3676-3707`）。应用重启：对没结束的投递去查供应商的历史，确认做没做过再决定重试，拿不准的标「需要处理」（`src/backend/agent/boot-recovery.ts`；`channels-and-messaging.md:284`）。消息队列落盘，可暂停、恢复、取消 | daemon 有租约和重放（`docs/machine-protocol.md`）；机器 2 分钟没心跳就换电脑重来（`server/src/agent/supervise.ts`） | 我们「换电脑重来」会不会重复对外动作，没验证 | 重启后先查 CLI 的历史，不盲目重跑 |
| 卡住和打转 | 只有超时：ACP 5 分钟、Pi 10 分钟没输出就判这一轮失败，有工具在跑、在等人、在压缩时不算（`src/backend/acp-client.ts:118`、`src/backend/pi-client.ts:70`）。Codex、Claude 没找到看门狗。计划里还有没做完的步骤时自动续一轮，有次数上限（`src/backend/agent/plan-continuation.ts`） | 5 分钟催一次、同一步 4 次算打转、最多自己处理 3 次再告诉人（`server/src/agent/supervise.ts`） | 这块我们的设计更全 | 「计划还有未完成步骤就续跑」比我们「只说不做就续跑」的判断更可靠 |
| 权限档位 | 每个 agent 三档：只在工作区、先问、全开（`packages/contracts/src/ipc-agent-admin.ts:44`）。「只在工作区」是真隔离：Codex 用它的 `workspace-write` 沙箱；Claude 走 SDK 的 `canUseTool` 回调拦工作区外的写（`src/backend/claude-client.ts:825`）；其余 CLI 整个进程放进 macOS 沙箱，Linux 和 Windows 上不支持就拒绝启动（`src/backend/process-confinement.ts:11-18`） | 默认全开，只拦五类动作 | 我们没有「收紧一档」的选项（这是 Joye 定的，不是缺口） | — |
| 审批怎么强制 | 全部走 CLI 自己的审批通道，由代码回答，不靠提示词。Codex 每个线程都用 `approvalPolicy: "on-request"`（`src/backend/agent/thread-lifecycle.ts:616`），审批请求进 `AttentionRegistry`；全开档由代码自动同意（`src/backend/agent/approval-automation.ts`），个别工具标 `alwaysAsk`，全开也问（`attention-registry.ts:152`、`:786`） | Claude Code 用 hook 拦；**Codex 只在提示词里写规则**（`apps/daemon/src/gates.rs:41-45`） | Codex 这半边我们没有强制 | Codex 开 `on-request`，daemon 收到审批请求后按规则回答：命中五类的发卡，其余自动同意。OpenBot 已经证明这条路跑得通 |
| 拦哪些动作 | 没有「合并到 main、上生产」这种按动作类型的规则。市场里的 agent 写着「发送、花钱、合并、发布前先问」，是写在 agent 指令里的（CHANGELOG 0.58.0）。工作流里有显式的审批节点（`agents.md` "Workflows"） | 五类动作，团队可勾选，批准人按类型设 | 这块我们更细 | — |
| 记忆分层 | 两层：agent 的记忆和频道的记忆（`src/backend/agent-memory-store.ts`、`channel-memory-store.ts`）。每条记忆是「必带」或「可搜」；必带的有字节预算，超了自动降成可搜（`src/backend/agent/agent-memories.ts:373-376`）。agent 用 `remember`、`search_memories`、`forget_memory` | 团队、群、成员、个人四层；每天整理一次 | 我们层数多，但有已复现的跨群泄露 | 必带和可搜分开，必带有硬预算 |
| 记忆复盘 | 每 20 轮（可设 10、20、40 或关），或者闲 15 分钟且有 3 轮新对话，或者清空对话时。一次无工具的生成，输出 `add`、`update`、`forget` 的 JSON。人写的、人选过的记忆不改；复盘期间变过的不改；密文先过脱敏。改动在对话里显示一行（`providers.md:94-137`；`src/backend/agent/memory-review.ts`） | 每天夜里整理，界面有「记忆更新」可撤回 | 我们的没用真模型跑过 | 「人写的不动」和「读之后变过的不动」两条保护 |
| 多 bot 协作形态 | agent 之间能发消息、回复、传文件（mailbox）；频道里是「一个任务一个负责人 + 显式委派」。指令里要求不寒暄、不来回确认、结果四行格式（状态、结果、证据、解除阻塞）（`agents.md` "Agent communication policy"）。另有画布式工作流：把 agent、模型、条件、审批、输出连成图，支持 GitHub 触发（`agents.md` "Workflows"；CHANGELOG 0.58.0） | 方向定了（不讨论、靠产出和状态接力），代码还是旧的 | 他们是「有约束的消息 + 显式流程图」，我们是「痕迹协作」，路线不同 | 工作流画布 = 把接力顺序交给人画，不让 bot 自己决定派给谁 |
| 工具 | 所有 CLI 拿到同一套内置工具，Claude 走 SDK MCP，其余走本地 MCP 桥（`providers.md:36-38`）。MCP 服务器是整台 server 共用的，带 OAuth 登录和续期；agent 可以在人同意后自己加 MCP 服务器（`src/backend/agent/mcp-tools.ts`） | 团队工具是 `chat-run` 命令行，不是 MCP | 换一家 CLI 就要重新教它用 `chat-run` | 团队工具做成 daemon 起的 MCP 服务，各家 CLI 都认 |
| skills 和市场 | 遵循 Agent Skills 规范；安装时同时写 `.agents/skills` 和 `.claude/skills`，让各家都能读到（`docs/architecture/skills-and-plugins.md`）。agent 能把一段做法存成 skill 给别的 agent 用（README "Local skills"）。市场有应用（MCP + skills）、技能、成品 agent（0.58.0 一次加了 17 个 agent、15 个 skill） | 没有 skills 管理，没有市场 | 「团队 know-how 给到 agent」我们只有记忆这一条路 | skill 是比记忆更合适的 know-how 载体：有版本、可安装、可分享 |
| 接外部 | 内置连接：GitHub、Slack、Discord、Telegram、1Password、Bitwarden（`docs/architecture/integrations.md` "Add a connector"）。定时任务、webhook 触发（`docs/automation.md`、`docs/webhooks.md`）。内嵌浏览器可被 agent 操作，登录页会请人接管；密码库自动填表，agent 看不到密码（`src/backend/password-vault.ts`）。可选 Computer Use | 进来的 webhook 和 GitHub 事件卡片 | 定时任务、浏览器、密码库我们都没有 | 浏览器 + 请人接管登录，是非编程类工作的基础 |

## C. 机器层

| 项 | OpenBot 怎么做 | 我们现在 | 差距 | 值得借的 |
| --- | --- | --- | --- | --- |
| 跑在哪 | 五种，同一个程序：① 本机桌面应用（默认）；② 同一台电脑上再开最多 5 个独立 server（`docs/architecture/servers.md:851-890`）；③ 自己的 Linux 服务器：一条安装命令、Docker 或 ECS（`docs/self-hosted-server.md`、`docs/docker.md`）；④ 官方托管：每个 server 一个云沙箱，15 分钟不用就停、有人用或定时任务到点就唤醒，每月 20、50、100 欧元（`docs/hosted-servers.md`；官网 /pricing）；⑤ 一台 Mac 上每个客户一个系统用户（`docs/multi-tenant-hosting.md`） | macOS、Linux（Firecracker）、Windows 上的 daemon，接到团队服务端 | 我们没有「不用自己的电脑」的现成选项；VM 主机可能还没上线 | 托管的「空闲停机、按需唤醒」；一条命令装服务器 |
| 多台电脑 | 一个账号可以连多个 server，客户端在它们之间切换。agent 属于某个 server；没找到在电脑之间调度的代码，搬家靠「实例迁移」导出导入（`agents.md` "Instance transfer"，本次没细读） | 成员可以固定电脑，也可以按标签选；电脑没了能换一台 | 这块我们更强，但固定的电脑离线时直接失败 | — |
| 每个 agent 的目录 | `~/OpenBot/Agents/<id>` 一个 agent 一个工作目录，长期保留；`~/OpenBot/Shared` 是 agent 之间共享的；可以给 agent 追加文件夹（README "Local data"；`src/backend/agent/workspace-sandbox.ts`） | 每次运行一个目录；VM 里每个成员一个长期家目录，不限大小 | 「成员在电脑上有没有长期状态」我们没定，他们明确是有 | 工作目录跟着 agent 走，是长期身份的一部分 |
| 隔离 | 默认不隔离：同一台 server 上的 agent 是同一个系统用户，共用 `~/.claude`、`~/.codex` 登录（README "Local data"）。要隔离就用「只在工作区」档，或者分 server、分系统用户。README 明说「全开是产品决定，不是安全边界」 | 本机不隔离；Linux 上每次运行一台 VM，但共享宿主机的登录目录 | 两边默认都不隔离。他们把话说明白了，我们没有 | 把「这台电脑上的成员互相看得见」写进界面和文档，隔离做成可选的一档 |
| 仓库和分支 | 没有。没找到 worktree 或分支管理的代码；agent 在自己的工作目录里自己 clone | 每次运行一个 `git worktree` 和 `bot/<runId>` 分支，有清理和磁盘下限（`docs/machine-protocol.md` "Run folders"） | 这块我们更强 | — |
| GitHub 和 git 身份 | 每台电脑一次登录：GitHub App 的设备码流程，只用公开的 client ID（`src/main/github-device-flow.ts`、`github-connector-service.ts:1-2`）。`gh` 用登录人的 token，GitHub 上显示「某人 with OpenBotGit」；`git` 和 MCP 工具在这个人能推的仓库上换成 App 的安装 token（1 小时有效），显示为 App；App 没装的仓库就用人的 token，所以 bot 的权限不会超过这个人（`src/main/github-bot-tokens.ts:43-48`）。token 放在 agent 写不到的目录里，git 通过凭证助手读，不进对话（`github-connector-service.ts:81-110`） | 谁点了授权就一直用谁的身份，凭证明文放在成员家目录 | 明文落盘；多人时借权 | **这套同时适合个人和团队**：个人就是自己登录一次；团队里是「在这台电脑上登录的人」的权限上限。不需要先建团队 App |
| 其他凭证 | 密码库（自带、1Password、Bitwarden），浏览器填表时 agent 只看到条目名；API key 加密存储，按 agent 或全体可见 | 机器密钥：加密下发，可按成员限定（`docs/machine-protocol.md` "Accounts and keys"） | 持平 | — |
| 实时查看和接管 | 每个 agent 一个内嵌浏览器，人随时能看、能接手，agent 也能主动请人接管（`docs/architecture/browser.md`）。能远程看和操作 server 的桌面（README "Remote desktop"；Linux 要 X11）。手机有通知和 Live Activity（`channels-and-messaging.md` 开头两节）。没找到「接管 CLI 终端」 | 实时查看、接管、交还；VM 不报接管地址 | 我们能接管终端，他们能接管浏览器和桌面 | — |
| 资源保护 | 托管 server 上：内存低时暂缓新的一轮，并限制同时跑几轮（`src/backend/agent/memory-hold.ts`、`turn-slots.ts`） | 磁盘低于下限拒绝新任务；没有内存保护 | 小 | 等待原因写明白：`task-limit`、`usage-limit`、`server-busy` 等（`agents.md:286`），界面直接显示为什么在排队 |

## D. 上手

| 步骤 | OpenBot | 我们 |
| --- | --- | --- |
| 1 | 下载安装（macOS、Windows 已签名，Linux AppImage） | 有人先部署服务端（Cloudflare Worker、D1、Durable Objects），建团队，发邀请码 |
| 2 | 首次启动：语言、选「本机」还是「服务器」、选一个模型供应商，应用自己下载 CLI 并带你登录（`src/renderer/src/features/onboarding/`） | 下载安装，macOS 上手动放行未公证的应用，输邀请码 |
| 3 | 一句话描述想要的 agent，应用生成名字、头像、指令，人看一眼保存（`agents.md` "Prompt-driven agent profiles"） | 在要跑 bot 的电脑上开终端装 CLI 并登录 |
| 4 | 发第一条消息 | 设置 › 机器 › 用这台电脑 |
| 5 | — | 设置里添加 AI 成员，选电脑、运行方式、模型 |
| 6 | — | 在群里 @ 它 |
| 7 | — | 想要前台、盯活、记忆：再去配 AI 服务的 API key |

官网写的是四步：下载并连供应商、一句话描述、检查指令、派活。

他们开箱就有、我们没有的：

- 不用服务端、不用邀请码，一个人能直接用；个人免费。
- 应用托管 CLI 的下载、登录、更新。
- 一句话生成 agent；市场里有成品 agent、技能、应用（Notion、Figma、Linear、Canva 等）。
- 从 Grok Bot 导入 agent（指令、技能、定时任务、记忆）；agent 模板可以发成链接分享。
- OpenCode 免费模型：没有任何订阅也能跑起来。
- 手机端（iPhone、Android）和网页端连到自己的电脑，不用 VPN。
- 定时任务、webhook、工作流画布。
- 内嵌浏览器和密码库。
- 备用模型、多账号、额度用完自动等待。
- 英、法、日、葡、土五种界面语言。

## E. 他们明显不如我们的地方

| 项 | 依据 |
| --- | --- |
| 不是以人和人聊天为主体的软件。它是 agent 工作台：以「我和某个 agent 的对话」为中心；频道的定义是「多个 agent 共用的聊天」，人在里面给 agent 派任务。成员之间只有一对一私聊（纯文字），没找到人和人的群聊 | `docs/glossary.md` channel 条；`packages/contracts/src/ipc-team-host.ts:221`；`src/backend/team-chat-store.ts` |
| 任务对象是 agent 的任务（在做、需要你、已停、失败、已取消），没找到人和人之间的话题、「在等谁」、待办 | `agents.md:366-372`（Tasks 页）；人的话题未找到，标未核实 |
| 没找到我们这种「产出」：结果是聊天里的消息、文件、`html_render` 的一块页面，以及可发布的 hosted site；没找到带版本、能追来龙去脉的产出对象 | `channels-and-messaging.md:240`、`agents.md:584`；`servers.md` "Hosted sites per server"（本次没细读） |
| 数据在一台电脑的 SQLite 里。那台电脑关机，别人就连不上（托管的除外）；公开 webhook 要求主机在线 | README "Local data"；`docs/webhooks.md` |
| agent 不能在电脑之间调度，一台 server 就是一台电脑 | `agents.md` "Instance transfer" |
| 没有每次任务的 worktree 和分支；多个任务共用工作目录，不锁文件（文档自己写明） | `agents.md` "Agent threads" 的 "Shared writes" 段 |
| 没有按动作类型的审批（合并到 main、上生产），只有三档整体权限 | `ipc-agent-admin.ts:44` |
| 卡住检测只有超时；没找到打转检测，Codex 和 Claude 上连超时都没找到 | `src/backend/acp-client.ts:118`、`src/backend/pi-client.ts:70` |
| 默认全开且同机不隔离，README 首屏就挂着警告 | README:13-17、"Security" |
| 商用要买许可，源码不能商用 | LICENSE；官网 /pricing |
| 团队版官网页面没写隔离、角色、SSO、合规的任何细节 | 官网 /teams |

## 和 `docs/design/agent-harness.zh.md` 第 14 节相比，要更正和补充的

- 第 14 节写「三档权限，默认全开；agent 只能收紧，不能放宽」。源码核对：三档是 `workspace`、`ask`、`full`，README 警告里的 `approvalPolicy: never` 已经过时，现在每个 Codex 线程都是 `on-request`，由代码回答。「agent 只能收紧」这句本次没找到证据，标未核实。
- 第 14 节没写的：多账号、额度撞线挂起、出错按六类原因分流、托管 CLI、GitHub 身份的做法、side thread 和分叉、工具按需发现、上下文预算进 CI、工作流画布、托管和自托管服务器。
- 第 14 节说「它们大多是单机桌面应用」。OpenBot 现在也能当常驻服务器跑（自托管、Docker、官方托管），并有手机和网页客户端。「前台放服务端所以换电脑更容易」这条优势比写的时候小了。

## 未核实

- OpenBot 上 Codex、Claude 的一轮卡死会怎样：只确认了没找到看门狗代码，没跑过。
- 官方托管沙箱（boat）里 agent 之间的隔离程度。
- 官网 /pricing 上团队许可的价格前后不一致（2 人 70 欧元 + 每人 10.8，订单示例却是 5 人 70），以实际下单为准。
- 我们这边 Codex 会不会自己压缩上下文。
