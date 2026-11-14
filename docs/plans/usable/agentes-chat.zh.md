# 对标 agentes.chat：差距和先补的三件事

日期 2026-11-13。只读研究，没有改代码。

- 对方读的是 GitHub 上的 `agentes-chat/agentes-chat`（main，最近一次推送 2026-11-12，桌面版 0.8.3）：README、CLAUDE.md、`docs/adr/`（172 篇架构决策）、目录和代码量，外加 GitHub API 的 star、issue、release、贡献者。
- 许可是 AGPL-3.0，和我们一样；下面只写做法，不抄代码。
- 路径不带前缀的是对方仓库；`我们：` 后面的是本仓库。
- 「Beta」是对方 README 自己标的状态，不是我们的判断。

## 0. 一句话

对方是「自部署的 Slack + AI 同事」，功能面铺得很宽（连接器、定时任务、Wiki、网盘、看板、手机、网页、14 种语言、云托管），起步早我们约三周、人多一倍；我们在「AI 成员把一件活做完」这条主线上的设计更细（前台、盯活、审批拦在执行那一步、接管、Card v2、产出页面），但主线还没在真模型上跑通，外围几乎都还没有。

## 1. 对方是什么（有据可查）

| 项 | 内容 | 出处 |
| --- | --- | --- |
| 定位 | 开源、自部署的 Slack 替代品，人和 AI agent 在频道、私聊里一起干活 | README 第一段；仓库 description |
| 技术栈 | Bun + Hono + Drizzle + PostgreSQL（唯一依赖，作业走 Postgres outbox）；Web：React + Vite + TanStack；手机：Expo（iOS、Android）；桌面：Tauri（Mac、Windows、Linux） | CLAUDE.md "Stack"、"Non-negotiables"；README "Contributing" |
| 部署 | 云托管（agentes.chat Cloud）或 `curl -fsSL https://agentes.chat/install \| sh` 一条命令装到 Linux，Docker Compose + Postgres + pgvector，Tailscale 私网 HTTPS，约 10 分钟；也有不带 Docker 的单程序版 | README "Quick start"；SELF_HOSTING.md |
| 代码 | 约 127 万行 TS/TSX（engine 38 万、web 28 万、server 17 万、mobile 16 万），1392 个测试文件，254 个数据库迁移 | 本地统计 |
| 节奏 | ADR 0001 日期 2026-10-14；两位主力提交者 5533 次、2293 次；近一周每天约 270–470 次提交；11-12 一天发了 5 个桌面版本 | `docs/adr/0001-*`；GitHub contributors、releases API |
| 热度 | 2026-11-09 公开，4 天 848 star、114 fork、78 个开着的 issue（不少是外部用户报的 bug） | GitHub API |

核心能力（README "Features" 表，括号里是 ADR 编号）：

- **Agent 是工作区成员**：角色、说明、模型、月预算（每次调用前检查）、负责人（收审批和预算提醒）。
- **路由**（0074）：没 @ 任何人的消息由一条链决定谁回答（小模型 → 作者自己的订阅 → 代码规则 → 没人回答并说明原因），最多 4 个 agent，回答里写明是谁选的。
- **审批**：合并、部署、回滚和连接器标为敏感的动作，变成聊天里的审批卡，批准后只按批准的内容执行一次（0126、0139、0173）。
- **连接器（MCP）**：组织接一次 MCP 服务器（Linear、Sentry、Notion、Stripe 一键接），每个 agent 只拿被授予的 scope；配 Composio key 后再接 20 多个 SaaS（Google 全家、Slack、Teams、HubSpot、Salesforce、Figma……）；还有只读 Postgres 连接器、agent 邮箱。
- **技能**：`SKILL.md`，从 zip、GitHub 链接或编辑器导入，带版本，按需加载。
- **定时任务（Routines）**：按时间或事件（GitHub PR、Sentry issue、新任务、来信、webhook）触发，交付到频道、私聊或网盘（0053、0106、0130）。
- **记忆**：决定、事实、偏好、经验；从纠正和被拒的审批里学；agent 可以提议改自己的说明，人确认（0120、0167）。
- **Wiki、网盘、任务看板、仪表盘**（0115、0128、0135、0171）。
- **Agent 的电脑和浏览器**：命令和代码跑在连接的电脑上；每个 agent 一个 VNC 桌面，浏览器可在聊天里、网页上、手机上实时看并接管（0058、0110；浏览器 Beta）。
- **运行方式**：API 模型（Claude、OpenAI、Gemini、DeepSeek、Kimi、Z.ai、MiniMax、OpenRouter、自定义端点）或电脑上的 CLI：Claude Code、Codex、Cursor、OpenCode、Grok、Antigravity，Copilot 在做（0114、0132、0142、0143；issue #227）。
- **团队的右手**（0137）：每个工作区一个内置 agent，用聊天来建 agent、改设置、查哪里没配好。
- **从别的产品导入**：OpenBot、Hermes Agent、OpenClaw 等的 agent、技能、定时任务和记忆（Beta）。
- **14 种语言**，含简体中文。

对方的弱点（同样有据）：桌面、手机、浏览器、邮箱、导入都还是 Beta；Mac 版没公证；open issue 里有「agent 上存的模型根本没用到这一轮」（#221）、「接了供应商 bot 仍然没有 AI」（#217）这类主流程 bug。功能铺得宽、发版极快，稳定性一般。

## 2. 我们现在是什么

- 原生 Mac（Windows 同一套）客户端，Rust + GPUI；服务端 Cloudflare Workers + Durable Objects + D1 + R2；AI 成员由 daemon 在电脑上跑 Claude Code、Codex、ACP（`我们：docs/ARCHITECTURE.md`）。
- 约 41 万行 Rust/TS/Swift（desktop 21.6 万、server 6 万、daemon 4.7 万），1309 个 Rust 测试、86 个 TS 测试文件，35 个迁移；2026-11-02 建仓，2516 次提交，最新 1.3.0。
- 主线设计：前台四选一、执行者、盯活（由代码）、审批拦在执行那一步（hook、权限请求、`git`/`gh` 垫片）、转交/分出去/按顺序做、四层记忆和夜间整理、接管、Card v2.1、双向的产出页面、引用提问（`我们：docs/PRODUCT.md` "AI members at work"）。
- 自己的判断：前台、转交、记忆整理没用真模型跑过；11-12 审计约 200 条问题，十几条主流程走不通（`我们：docs/plans/usable.zh.md` "为什么要有这一版"）。
- 明确不在这一版：定时任务、技能、市场、MCP 团队工具、手机端、托管、成本统计（同上第 1、7 节）。

## 3. 差距（按影响排序）

| # | 维度 | 对方 | 我们 | 差距 |
| --- | --- | --- | --- | --- |
| 1 | **成熟度** | 有真实外部用户在报 bug、在修；1392 个测试文件；172 篇 ADR 把每个机制的边界写清 | 主线在真模型上没跑通，靠两轮真机验证在补；仓库 0 star，README 还没准备好开源 | 最大的差距不是功能，是「装上就能把活做完」还没成立 |
| 2 | **上手和分发** | 云托管 + 一条命令自部署；网页、Mac/Win/Linux 桌面、iOS/Android | 必须自己有 Cloudflare 账号部署，再配 Linux 主机；只有 Mac（Windows 在收尾）；iOS 做到 M5 在等开发者账号；没有网页、没有安卓、没有托管 | 新人试用的门槛高一个量级 |
| 3 | **接外部工具** | 组织级 MCP + 每个 agent 的 scope；Composio 20 多个 SaaS；技能带版本；Postgres、邮箱 | 只有 `chat-run`；进来的 webhook 和 GitHub 适配器；CLI 自己在电脑上配的 MCP 能用，但团队管不了、也没有 scope | AI 成员只能做「电脑上能做的事」，碰不到团队的业务系统 |
| 4 | **主动干活** | 定时和事件触发（PR、Sentry、来信、webhook），结果交到频道 | 只有人 @ 才开工；「机器人的消息不会启动运行」；webhook 只发卡片不启动运行 | 巡检、日报、盯 CI 这类活没法交给 AI 成员 |
| 5 | **模型和成本** | 统一的模型网关，每个 agent 月预算、调用前检查、按人按 agent 计费；7 种 CLI + 十来家 API 供应商 | Claude Code、Codex、ACP；走订阅的不计入预算；成本页「即将推出」 | 团队放心放权的前提（花多少、谁花的）还没有 |
| 6 | **谁来回答** | 没 @ 的消息有路由链，也有内置「右手」用聊天来配团队 | 必须 @；不带 @ 的跟进给最近开工的成员；主 bot 只是默认对象 | 一个人刚装好说「早」没人理的情况我们也会有 |
| 7 | **知识和工作面** | Wiki（双链、图谱、Obsidian 导入）、网盘（版本、导出 Word/PDF）、任务看板、仪表盘 | 产出（带版本）、群里的决定、记忆；没有 Wiki、看板 | 团队的长期知识没有落点 |
| 8 | **AI 的电脑** | 每个 agent 一个可看可接管的桌面和浏览器，手机上也能看 | 实时查看是由运行步骤重建的终端，接管是命令框；Linux 主机上每次运行一台虚拟机；没有浏览器 | 网页类、要登录的活做不了，也看不到 |

我们比对方好的地方（别丢）：原生客户端的性能目标（p95 300 ms、万条消息 100 ms 打开）；Card v2.1 的交互回路和引用提问；产出页面双向；审批由代码拦在执行那一步；盯活由代码不靠模型；Linux 上按运行隔离的虚拟机；中文优先、产品用词收得很紧。

## 4. 先补的三件事

前提不变：先按 `usable.zh.md` 把十条路径在真电脑、真模型上走通（差距 1）。不先做这个，下面三件加上去也只是又一批没验证的代码。

1. **定时和事件触发的运行（差距 4）**。我们已经有进来的 webhook、GitHub 适配器和 Cron Trigger，缺的只是「事件 → 让某个 AI 成员在某个话题里开工」这一步，加上「每天 9 点」这种时间规则。成本小，价值直接：日报、CI 失败自动看、PR 自动审。要同时带上对方 0131 那种刹车（告警不能变成十个 PR）。
2. **团队级的 MCP 和技能，由 daemon 发到每次运行（差距 3）**。不用推翻「团队工具走 `chat-run` 不做 MCP」：我们的执行者本来就是 Claude Code/Codex，原生支持 MCP 和 `SKILL.md`。只要在团队管理里登记 MCP 服务器和技能、按成员授予，daemon 在开工时写进这次运行的 CLI 配置（凭证照现在的做法按电脑加密）。这是用最少的代码换最大的能力面。
3. **一条命令装好 + 个人本机模式（差距 2）**。把 `scripts/setup-server.sh` 做成真的一条命令（含 Linux 主机），并评估「用这台电脑」时由应用在本机起一个 workerd（wrangler 本地模式已能跑 D1、R2、DO），让一个人不用 Cloudflare 账号也能先用起来。托管和网页版先不做。

下一档：每个 AI 成员的月预算和用量页（差距 5）；没 @ 的消息由主 bot 判断交给谁（差距 6）。

## 5. 没核实的

- 对方的云托管价格和实际用户数；功能实际好不好用（没装起来试）。
- 对方提交数只看了贡献者统计和浅克隆，没算精确总数。
