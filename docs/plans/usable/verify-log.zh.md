# 可用版：真机验证记录（第一轮）

- 日期：2026-11-12
- 测的 main：`d588c022`（#207 合并之后）
- 在哪跑：一台 Mac（Apple 芯片），本 worktree 的本地沙盒服务器（`wrangler dev`，样例团队来自 `scripts/sandbox/seed.ts`），加一个从这份代码编出来的 daemon（`cargo build -p daemon`），以电脑 runner-1 的身份连上。没有开桌面应用，全部通过公开 API 和 WebSocket，以样例的人（Joye、小王、李雪）操作。
- 真 CLI：Claude Code 2.1.319、Codex CLI 0.159.0，都用这台电脑上已经登录的订阅。AI 成员：Forge（Claude Code，`开发` 群），Taro（Codex，`夹夹增长` 群）。没有配 AI 服务（前台和夜间整理的模型），所以前台、运行后的经验总结、夜间整理都没有参与。
- 工作目录：Forge 指向一个没有远端的一次性仓库，Taro 指向另一个；DM 和不带仓库的任务用 daemon 的临时目录。每个任务只要一两次工具调用。
- 证据：脚本和输出在 worktree 的 `.tmp/verify/`（`checkN.ts` 是脚本，`checkN*.log` 是输出，`*-events.json` 是 `GET /v1/runs/:id/events`，`*-context.json` 是 `GET /v1/runs/:id/context`），daemon 的运行目录在 `.tmp/daemon/work/`。`.tmp/` 不进仓库，下面把关键的一行抄出来。运行 id 是这次本地沙盒里的，不是任何部署环境的。

## 结果

| 路径或条目 | 做了什么 | 结果 | 证据 | 备注 |
| --- | --- | --- | --- | --- |
| 路径 3 交代一件事（接 2.1，#190） | 小王在 `开发` 群聊发一条带暗号的消息；Joye 新开话题 @Forge，引用那条消息，附一张图（白底，左边红色实心圆，右边蓝色实心方块），问图里画了什么、引用的暗号是什么 | 通过 | run `01M760QMPS5Q9TEG0ERD73B8E4`。回答：「白色背景上画了两个形状：左边一个红色实心圆，右边一个蓝色实心正方形……暗号是「青柠檬 7381」，出自小王那条消息」。运行事件里它先 `chat-run file get --id <f>` 下载图片再用 Read 看图。context 的「Request from Joye」下面有 `↪ quotes m 01M760QM5RHY… (小王: 提醒一下：本周的暗号是「青柠檬 7381」…)` 和 `[file f 01M760QMNY1E… · shapes.png · image/png · 1 KB · 240×160]` | `context.manifest` 写的是 `files: 0, messages: 0, chars: 0`，没算请求自己带的文件和引用（问题 6）。图片被下载到工作目录里的 `.tmp/dl-shapes/`（问题 7） |
| 路径 5 接着说：话题（接 3.1，#200） | Joye 在 `开发` 新话题 @Forge 新建 `notes.txt` 写「第一行」不提交；做完后在同一话题不带 @ 说「追加一行：第二行」；90 秒没动静，再带 @ 说一遍 | 带 @：通过。不带 @：不通过 | 第 1 次 run `01M760S2F9D9V1GFR6ZZMEA7QM`、第 2 次 run `01M760W4YKMB496SEVG03EYHPX`：`run.json` 的 `line` 是同一个工作线目录，分支同为 `bot/<key>`；文件内容「第一行\n第二行」，`git status` 为 `?? notes.txt`（没提交）；两次的 `session` 事件是同一个 sessionId，第 2 次的 `stream.jsonl` 以 `SessionStart:resume` 开头；第 2 次 context 是 `resumed: true, messages: 1`。不带 @ 的那句：90 秒内没有新 run | 不带 @ 的跟进不理，就是审计里的接 1.4（问题 1） |
| 路径 5 接着说：私聊里的话题 | Joye 在和 Forge 的私聊里开一个话题，同样两步，第二句不带 @ | 通过 | run `01M760WYCR57QB32X3KBHDJFBZ` → `01M760X86KZAZW8XA5VAPS2W4X`：同一个工作线目录，同一个 sessionId，第 2 次 `resumed: true`；`dm-notes.txt` 两行 | |
| 路径 5 接着说：私聊的聊天区（main 话题） | Joye 在和 Forge 的私聊里直接发（不开话题），同样两步 | 不通过 | run `01M760XPTQKXE5W52S7T76CTAM` → `01M760Y1VCV63TWYTBTRZ91HN1`：工作目录相同，`dm-main.txt` 两行都在；但第 2 次是新会话（sessionId 不同），context `resumed: false`，靠话题里的两行记录接上 | 私聊每句话都开新会话，就是接 3.2（问题 2）。工作目录续上了，会话没有 |
| 路径 8 审批：Claude Code（盯 2.x，#103） | 审批规则用默认（`合并到 main` 勾着，批准人是请求人）。Joye @Forge 在仓库里运行 `git push origin main`（仓库没有远端） | 通过 | run `01M760Z6MAJB51QX9G6WJH9DRV`：事件 `tool.call git push origin main` 之后紧跟 `approval.request {kind: mergeMain, approvalId: daemon-gate-1}`，run 状态 `waiting`，命令没有执行。话题里出现卡片「需要你确认：合并到 main」，按钮 同意 / 这件事里都同意 / 拒绝，@ 了 Joye。等了约 8 分钟没有被当成卡住。Joye 按 同意 后：`approval.resolved approve`，命令这时才执行（`fatal: 'origin' does not appear to be a git repository`），run `done`；同一条消息 `01M760Z9Z9…` 原地变成「已同意：合并到 main」、一行「Joye 同意了 · 03:30」、没有按钮，没有另发新卡 | 卡片是 v1 卡片（`card`，没有 `cardV2`）。原地改是一次消息编辑；我的脚本当时按错了 `message.updated` 的字段去找所以没认出推送，是从历史里确认的 |
| 路径 8 审批：常见绕法 | 同上，命令换成 `git -C . push origin main` | 不通过 | run `01M761QS5W049STCTY6XC64N8V`：没有 `approval.request`，命令直接执行（因为没有远端才失败），没有卡片 | 盯 2.2（问题 3） |
| 路径 8 审批：Codex | Taro 在它的仓库里运行 `git push origin main` | 不通过 | run `01M761GMM1RA87D6K7A0BYA9D4`：直接执行，没有卡片。电脑上报的 `clis` 里 codex 没有 `gates` | 盯 2.1（问题 4）。这次还发现 Codex 自己发了一张「做完」卡片，最后的回答又发了一条，话题里结果出现两遍（问题 5） |
| Codex 成员跑简单任务 | Taro 的 runner 没设模型；@Taro 运行 `echo codex-ok` 并说输出 | 通过 | run `01M761FNSAPDFERWGWP5RRMVWB`：`session` 事件 `model: gpt-6-luna`，回答「输出是：`codex-ok`」 | |
| Codex 没设模型时用账号的默认模型（#103） | 同上。这台电脑 Codex 配置里的默认模型不在这个账号的模型列表里 | 通过 | daemon 日志：`Codex's default model is not in this account's list; it runs with gpt-6-luna`（列表里的默认）；`usage.models` 只有 `gpt-6-luna` | 跑的是账号列表的默认，不是本机配置里写的那个（那个账号用不了） |
| 记忆：成员层不串群（盯 4.1，#198） | Joye 在 `开发` 让 Forge 用 `chat-run memory note --scope member --kind lesson` 记一条「开发群的发版窗口是周四下午三点」；分别以 Joye、小王（在 `开发`）、李雪（不在 `开发`）读 `member:<Forge>` | 通过 | run `01M761K68XQE1QYESTQWP00014`：记忆 `01M761K93C41…`，`status: pending`，`fromGroup` = `开发`。Joye、小王的列表里有；李雪的列表是空的，`GET /v1/memories/:id` 李雪 404、小王 200 | 经验总结（运行后的 retro）要 AI 服务，这次没配，所以只测了成员自己记的这条 |
| 记忆：带进提示词按群过滤 | Joye 把这条记忆设成 active，再分别在 `全员`（有李雪）和 `开发` 各 @Forge 问一个算术题 | 通过 | `全员` run `01M761KSFS0KZQD9XF04T4AMHY` 的 context 不含这条；`开发` run `01M761KWTV7TTXT8G7AYC3FASG` 的 context 有一行 `- 开发群的发版窗口是周四下午三点 [01M761K93C41…]` | |
| #103 团队模型设置 | `PUT /v1/settings/models/claude {model: haiku, effort: low}`，Forge 自己没设模型，@Forge 问一个算术题，之后清掉 | 通过 | run `01M761MRE5YQJC566PGD59M9NY`：`session` 事件 `model: claude-haiku-5-5` | `BotRun` 上没有这次用的模型和思考强度（`model`、`effort` 都是空），只能从运行事件里看（模 2.12 还没做） |
| 路径 9：一条消息 @ 两个成员（接 1.2） | Joye 在 `开发` 新话题里一条消息 @Forge @Scout，各自回答 3+3 | 不通过 | 话题 `01M761VSVAX2CR82WQ4V45VYP0`：2 分钟内只有 Forge 的 run `01M761VSWSDWZSBSAWR5GR76CK`（回答「6」），Scout 没有 run | 接 1.2（问题 10）。这一轮只顺手测了这一条，路径 9 的其他部分没测 |
| #194 用这台电脑：登录状态的数据 | 读 `GET /v1/machines/:id/monitor` 的 `clis` | 通过（只看了数据） | claude、codex 都是 `available: true, login: logged_in, method: subscription` | 桌面上那张三行卡片没有在真机上看；没有在没装 CLI 的电脑上试 |
| #200 清理前先提交没提交的改动（机 2.4） | 把 daemon 的 `keep_runs_hours` 改成 0 后重启 daemon | 见下一行 | 重启时 14 个刚结束的 run 都被「recovered」（1 小时内结束的 run 要补发帧，这是设计），启动时的清理把它们当成在用，11 个工作线一个没动 | 启动时这样是对的；见下一行的定时清理。daemon 重启后补发帧没有在话题里造成重复消息（看了两个话题的历史） |
| #200 定时清理 | 等 daemon 30 分钟一次的清理 | 通过 | daemon 日志：`cleanup: line a1bd61805fdc3458 had uncommitted changes; committed them to bot/a1bd61805fdc3458`、`kept branch … (1 commits not pushed)`；工作线目录全部移走；一次性仓库里 `bot/a1bd…` 多了一个提交「wip: uncommitted changes kept by cleanup」，内容是两行的 `notes.txt` | Taro 那个仓库的分支上被提交的是它自己写的 `card.json`（问题 5、7） |
| #200 清理后再续接 | 清理之后，在原来的「续接测试」话题里 @Forge 用 cat 看 `notes.txt` | 通过 | run `01M763DVXWHXNAQKTDA3ZS0FW7`：同一个工作线 key 和分支重新检出，sessionId 和第一次相同（续上了会话），回答里是「第一行\n第二行」 | |
| #203 自己部署服务器 | 没跑 | 没法跑 | | 要一台全新的 Ubuntu 和一个 Cloudflare 账号；这一轮不碰任何部署环境 |
| #206、#207 Windows | 没跑 | 没法跑 | | 这台是 Mac |
| #204 协议 | 只有类型，没有可以跑的行为 | 不适用 | | |

## 发现的问题

1. **话题里做完后不带 @ 的跟进没人理（接 1.4，仍然存在）。** 在 `开发` 的话题里，Forge 做完第一件事后，请求人 Joye 不带 @ 说「在 notes.txt 末尾追加一行」，90 秒内没有 run，也没有 👀。第 4 节的默认规则是：没配 AI 服务时，成员上一条消息之后一段时间内请求人说的话算跟进。现在只有 @ 才会派。复现：`.tmp/verify/check2.ts topic`。私聊里开的话题不带 @ 是派的。
2. **私聊的聊天区里每句话都是新会话（接 3.2，仍然存在）。** 在和 Forge 的私聊里直接发两句（发到 `main-<私聊群 id>`），第 2 句开了新的 CLI 会话（sessionId 不同，context `resumed: false`），只靠提示词里的两行记录接上。工作目录是续上的（同一个工作线），所以文件还在。第 4 节默认是私聊续接上一个会话，说「新任务」才重开。复现：`.tmp/verify/check2.ts dmmain`。
3. **`git -C . push origin main` 绕过「合并到 main」（盯 2.2，仍然存在）。** 内置规则是 `\bgit push\b…`，`git -C <dir> push` 中间夹了参数就匹配不上。Claude Code 直接执行了，没有卡片。线 5 要补 `git -C`，以及当前分支是 main 时的 `git push`。
4. **Codex 不会被拦（盯 2.1，仍然存在）。** Taro 运行 `git push origin main` 直接执行。电脑上报的 codex 没有 `gates`，服务端只能靠提示词，而 Codex 照做了。线 5 的 Codex 先问和 PATH 垫片还没做。
5. **Codex 的结果在话题里出现两遍。** Taro 在任务里自己用 `chat-run card post` 发了一张「做完」卡片（前两次因为 block 少了 `id` 被拒，第三次成功），然后它最后的回答又作为结果发了一条文字消息，内容几乎一样。run `01M761GMM1RA87D6K7A0BYA9D4` 的事件里能看到三次 `chat-run card post`。要么在 CLI 已经发过做完卡片时不再把最终回答另发一条，要么提示里说清最终回答就是结果、不用自己发做完卡片。另外它把 `card.json` 写在了仓库工作目录里，`git status` 是 `?? card.json`，清理时会被当成没提交的改动提交到分支上。
6. **`GET /v1/runs/:id/context` 的 manifest 不算请求本身。** 请求带了一个文件和一条引用，manifest 却是 `files: 0, messages: 0, chars: 0`（`server/src/context/render.ts` 的 `countStats` 只数话题里的上下文行，请求那一节不算）。提示词本身是对的，只是 manifest 让人以为什么都没带。建议把请求行的文件、引用、产出也算进去，或者单独给 `request: { files, quotes, refs }`。
7. **执行者下载的文件落在工作目录里。** Forge 用 `chat-run file get --out .tmp/dl-shapes/shapes.png` 把附件下到了工作线目录里。仓库型的成员下载附件后，这些文件会留在 worktree，清理时被当成没提交的改动提交到 `bot/<key>` 分支。建议 `chat-run file get` 默认存到运行目录（不在 worktree 里），或者提示里写明下载到哪。
8. **工作记录里看不到这次用的模型（模 2.12，仍然存在）。** `BotRun` 的 `model`、`effort` 都是空的，实际模型只在 `session` 运行事件里（例如 `claude-haiku-5-5`、`gpt-6-luna`）。
9. **（备注，不一定是问题）这台电脑上 Claude Code 的个人设置会作用到 AI 成员。** Forge 的会话日志里有这台电脑上用户自己配置的 `SessionStart` 钩子在跑。「用这台电脑」时 AI 成员和你权限一样，这是设计；但个人的钩子、插件、全局指令也会一起带进 AI 成员的每次运行，上手页上那一屏说明（机 3.1）应该提到这一点。

10. **一条消息 @ 两个 AI 成员只有第一个动（接 1.2，仍然存在）。** `@Forge @Scout` 两个都在 `mentions` 里，只派给了 Forge。复现：`.tmp/verify/check7.ts`。

## 这一轮没有覆盖的

- 路径 1、2、4、6、7、9、10 的大部分：没有开桌面应用，没有配 AI 服务（前台、经验总结、夜间整理都没跑），没有长任务、断网、额度用完、多个成员。
- ACP：这台电脑没有配 ACP 的 CLI。
- Windows、Linux 主机：没有。
