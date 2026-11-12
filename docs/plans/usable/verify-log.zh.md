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

# 第二轮

- 日期：2026-11-12
- 测的 main：`230c592a`（#229 合并之后）
- 在哪跑：同一台 Mac。做法和第一轮一样：本 worktree 自己的沙盒服务器（`wrangler dev`，端口在给这一轮的范围里，样例团队来自 `scripts/sandbox/seed.ts`）；种子里的样例电脑脚本开出的 6 个样例任务先取消、脚本停掉，换成从这份代码编出来的 daemon（1.2.7）用同一个电脑 token 以 runner-1 连上。没有开桌面应用，全部通过公开 API 和 WebSocket，以样例的人操作（Joye 是所有者，陈晨是管理员，小王、李雪是成员）。
- 真 CLI：Claude Code 2.1.319、Codex CLI 0.159.0，用这台电脑上已经登录的订阅。Forge、Scout（Claude Code）指向一个没有远端的一次性仓库，Taro（Codex）指向另一个；Ledger 改用 daemon 配置里第二个 Codex 条目 `codexout`，它的 `CODEX_HOME` 指向一个空目录（等于没登录）。没有配 AI 服务。
- 证据：脚本和输出在 worktree 的 `.tmp/verify/`（不进仓库），下面抄关键的一行。运行 id 是这次本地沙盒里的，不是任何部署环境的。

## 结果

| 路径或条目 | 做了什么 | 结果 | 证据 | 备注 |
| --- | --- | --- | --- | --- |
| 第一轮问题 1：话题里不带 @ 的跟进 | Joye 在 `开发` 新话题 @Forge 建 `notes.txt` 写一行；做完后同一话题不带 @ 说「追加一行：第二行」 | 通过 | run `01M76XP5TPX3EE3Y8FPAP9PGQJ` → `01M76XPJ230VT6TXHZFE5JAQ2H`：不带 @ 的那句马上开了 run；同一个工作线目录、同一个 sessionId，第 2 次 context `resumed: true`；文件两行 | |
| 第一轮问题 2：私聊聊天区续会话 | Joye 在和 Forge 的私聊里直接发（`main-<私聊群>`），同样两步 | 通过 | run `01M76XP5XKCN4KQ4AZWXYDN1Y1` → `01M76XPJ51DS67ZMRVCHXM6T5Y`：sessionId 相同，第 2 次 `resumed: true` | |
| 第一轮问题 3：`git -C . push origin main` | 默认审批规则；@Forge 在仓库里运行这一条，卡片上按「拒绝」 | 通过 | run `01M76XSJS97ZZZCSA4JG9RS2DK`：`approval.request {kind: mergeMain, command: "git -C . push origin main"}`，run `waiting`；拒绝后钩子回「a person declined it」，命令没执行；卡片原地变成「已拒绝：合并到 main」「Joye 拒绝了 · 11:46」，没有按钮 | 拒绝之后它用 `chat-run report` 汇报，又被拦了一次，见新问题 1 |
| 第一轮问题 3：main 上不带参数的 `git push` | @Forge 在工作目录里 `git init -b main` 一个子仓库，空提交，再在里面直接 `git push` | 通过 | run `01M76XTXSYBARSRHNPMBPBNPND`：卡片内容是 `git push` 和垫片算出来的 `git push origin HEAD:refs/heads/main`；拒绝后没执行 | 话题标成在等回答（`needs_reply`），没有标完成 |
| 第一轮问题 3：脚本里的推送 | @Scout 写 `push.sh`（一行 `git push origin main`），运行 `bash push.sh` | 通过 | run `01M76XSJXJPAWNN34E84DM5Y2G`：Bash 那一行是 `bash push.sh`，由 PATH 垫片拦下发卡（`git push origin main`、`git push origin main:refs/heads/main`）；拒绝后脚本输出 `git: This command needs a person's approval … a person declined it`，exit 1 | |
| 第一轮问题 4：Codex 被拦并发卡 | Taro 运行 `git push origin main`，按「同意」 | 通过 | 电脑上报的 codex `gates: true`。run `01M76XSJS89SJE1P6ECEDY8Z4Z`：`approval.request` 在命令执行前，同意后才执行（没有远端所以失败）；卡片原地变「已同意」 | Codex 的第一条命令起不来，第二次才跑（新问题 7） |
| 第一轮问题 5：Codex 的结果只出现一次 | 同上一行 | 通过 | 话题里 Taro 的文字结果只有一条，它没有自己发「做完」卡片 | |
| 第一轮问题 6：manifest 算请求带的文件和引用 | 小王在 `开发` 群聊发暗号；Joye 新话题 @Forge，引用那条，附一张图，问图和暗号 | 通过 | run `01M76XP7JQEQKTEH37GG6GJHB9`：manifest `files: 1, refs: 1`（第一轮全是 0）；回答对 | 它想按 id 再读一遍被引用的消息，`chat-run topic message` 回 `not_found`（新问题 3） |
| 第一轮问题 7：下载落在工作目录 | 同上一行 | 不通过 | 它先 `--out` 到系统临时目录，被拒（`file.outside_workspace`）；又在工作目录里建 `./dl.xxxx/` 下到那里，看完自己删了 | 一半好了：不带 `--out` 时默认已经在运行目录（不在 worktree）；但 `--out` 只许写在工作目录里（新问题 2） |
| 第一轮问题 8：工作记录里的模型和思考强度 | 看 `GET /v1/runs/:id`；再把团队的 Claude Code 设成 `haiku` + `low`，@Scout 问一个算术题，之后清掉 | 通过 | 没设时 Claude Code 的 run `model: claude-opus-5-5`，Codex 的 run `model: gpt-6-luna, effort: high`；设了之后 run `model: claude-haiku-5-5, effort: low`，和 `session` 事件一致 | |
| 第一轮问题 10：一条消息 @ 两个成员 | 新话题一条消息 @Forge @Scout，各答 3+3 | 通过 | 话题 `01M76XYP4YCQEZ2SS8XRHFEK47`：Forge、Scout 各一个 run，各答「6」 | |
| 结束汇报：做不到的事不标完成 | @Forge 读一个不存在的文件，不许创建 | 通过 | run `01M76XYP9KMHM6WGT6ZQ3V8SBV`：它 `chat-run report --state blocked`；run `status: done`，`phase: needs_reply, waitingFor: stuck`；话题状态先 `in_progress` 再 `blocked`，`waitingOn` 是 Joye；回答说清读不到 | |
| 失败归类：CLI 没登录 | Ledger 用 `codexout`（空的 `CODEX_HOME`），电脑上报它 `logged_out`。@Ledger 问一个算术题，45 秒后在另一个话题再 @ 一次 | 通过 | 两次 run 都立刻 `failed`，`failure.kind: signed_out`，没有分到电脑（`machineId` 空，电脑上没有这两个 run）；话题里 Ledger 说「这次没有开始：runner-1 上的 Codex 没有登录，或者登录已经过期。请到 团队管理 › 账号与密钥 › 订阅账号，选 runner-1，点「重新登录」，然后再 @我。」 | 同一台电脑上有两个 Codex 条目时，这句话分不出是哪一个（新问题 8） |
| 恢复：daemon 正常停（SIGTERM） | @Forge 三步：`echo step1 >> rec-stop.txt`、`sleep 90`、`echo step2 >> rec-stop.txt`；sleep 时停 daemon，8 秒后再开 | 不通过 | 原 run `01M76YB5EGZT68X53TMYBFHCPF` 变 `cancelled`；新 run `01M76YC5J9Q2MTN40TWYQ2QJSR`（`retryOf` 原 run），同一个 sessionId，提示词里有恢复说明（「This is try 2 … The last 2 steps … Bash: sleep 90」），step1 没有重做。但新 run 的 CLI 结果是 `success, num_turns: 0`、空文字；话题里 Forge 说「完成了。」，run `done`，文件里只有 `step1` | 新问题 4：第 3 步没做，话题却标完成 |
| 恢复：daemon 被杀（SIGKILL） | 同样三步，`sleep 60` 要求在前台；sleep 时 `kill -9` daemon，再开 | 通过 | 原 run `01M76YEGR2TMWZTSPMH1Z90CCF` → 新 run `01M76YF92CNSWMC4T0SK641KX9`（`retryOf`），同一个 sessionId，带恢复说明；它先看文件和 `git status`，没有重做 step1；之后以 `waiting` 汇报：前台的 `sleep 60` 被 Claude Code 自己拦了 | Claude Code 2.1.319 会拦单独的 `sleep N`（「Blocked: standalone sleep …」），AI 成员的等待只能放后台，这和新问题 4 有关 |
| 转交 | Joye @Forge：用 `chat-run handoff` 转交给 Scout，目标里让 Scout 先试着转交回 Forge，再答 5+5 | 通过 | 话题里 Forge 发了卡片「Forge 把这件事转交给 Scout」，带「全部停止」；Forge 的 run 交出去就结束了；Scout 在同一话题开 run，答「5+5 等于 10」 | 回环上限没测到：Scout 按提示词拒绝转交回去，没有真的调，所以 `delegation.cycle` 没被真模型触发；上限只有单元测试 |
| 分出去 + 全部停止：停下来 | Joye @Forge：用 `chat-run assign` 把「sleep 150 后答 4+4」分给 Scout；Scout 开始约 25 秒后，小王（不是请求人）调 `POST /v1/groups/:id/chains/:chainId/stop` | 通过 | Forge 发卡片「Forge 把一部分分给 Scout」；新话题里「@Scout 运行 sleep 150 …」；停止返回 `stoppedRuns: 1`，Scout 的 run `cancelled, phase: stopped`，之后 Forge 没有再被叫起来；再停一次返回 `stoppedRuns: 0` | Scout 一开始被样例预算拦住（新问题 6） |
| 分出去 + 全部停止：话题里看得见停了 | 同上一行，停止后看 Forge 的主话题和 Scout 的子话题 | 不通过 | 主话题的「Forge 把一部分分给 Scout」卡还挂着「全部停止」按钮，没有改成「被 小王 停止」；子话题只有「已停止，没有完成。」，没说是谁停的 | design-relay.zh.md「结束」和「话题里看得见」两条要求分出去的卡原地改；见新问题 5 |
| 读取命令：真模型用 | @Forge 依次 `chat-run member list`、`decisions list`、`topic state`；@Taro（Codex）`artifact list --group`、`artifact get`；@Forge `memory search --query 代码评审` | 通过 | Forge 列出 5 个 AI 成员、决定「登录只用邀请码，不做手机号」、状态卡第一行；Taro 答出「10 月数据月报」、1 个版本、正文第一句；memory search 找到下面那条群约定 | |
| 群和个人记忆（API） | 小王写 `group:<开发>` 的约定；陈晨读、改；李雪（不在 `开发`）读、列、写；小王停用、看版本、恢复版本 1；小王写自己的 `person:` 偏好，所有者读、列，小王自己列、停用 | 通过 | 群成员读写都成；李雪读、列、写都是 `not_found`；改成版本 2 → 停用 → 恢复成版本 1 的文字；别人（包括所有者）读个人层是 `not_found`，本人能列能停用 | 每天的「记忆更新」和撤回要夜间整理，没配 AI 服务，没测 |
| 身份：管理员不是所有者 | 陈晨（管理员）改小王的 `level`、`teamOwner`，降所有者、停用所有者，发管理员邀请码，改小王的职位文字；小王改团队模型设置 | 通过 | 前五个都是 `forbidden, reason: role.owner_only`；改职位文字成功；小王改模型设置 `forbidden`；`GET /v1/members` 里 Joye `teamOwner: true` | 没测「最后一个所有者不能降」：沙盒里只有 Joye 一个所有者，万一没拦住会把沙盒锁死 |
| 个人电脑改名和移除 | 小王建团队电脑；建个人电脑，自己改名；陈晨改名、读监控、删除；李雪看列表；小王删除；小王改团队电脑名 | 通过 | 建团队电脑 `machine.team_admin_only`；个人电脑 `ownerId` 是小王，改名成功；陈晨改名、读监控、删除都是 `machine.not_yours`；李雪看到的只有 `id, name, createdBy, createdAt, ownerId, connected, healthy`（`labels`、`runningRuns` 空）；小王删除后 `revoked: true`；小王改团队电脑 `machine.not_yours` | |
| 电脑离线时交代（#195，顺带） | 停 daemon，@Forge 一个算术题，20 秒后再开 daemon | 通过 | 离线时 run `created`，话题里「runner-1 不在线，等它上线后自动开始；不需要了可以停止。」；上线后自动做完，答「18」 | 等电脑时 run 上没有 `phase`、`waitingFor`（新问题 9） |
| 群聊里同一个成员的第二件事（接 1.5，顺带） | Joye 在 `开发` 群聊连发两条：「@Forge 运行 sleep 40，然后只回答：甲」「@Forge 另一件事：只回答 7+8 等于几」 | 不通过 | 只有一个 run `01M76YHHBEFGZGMDCZAAHXAQ9M`，第二条以 `input.accepted` 插进了第一件；回答只有「7+8 等于 15」，「甲」那件没有答 | 新问题 10 |
| 暂停和继续（顺带） | @Forge 跑一个 60 秒的 python 等待，20 秒时 `POST /v1/runs/:id/pause`，10 秒后 `resume` | 通过 | daemon 日志 `paused`、`resumed`，之后做完 | 接管（`POST /v1/runs/:id/takeover`）在 main 上还没有（404） |
| GitHub 登录（顺带） | `GET /v1/me/github`、`POST /v1/me/github/device` | 没法跑 | `available: false, unavailableReason: no_client_id`；device 回 `conflict, github.no_client_id` | 沙盒没有 GitHub OAuth 应用；这一轮不碰真的 GitHub |

## 第一轮问题现在的状态

| 第一轮问题 | 现在 |
| --- | --- |
| 1 话题里不带 @ 的跟进没人理 | 已解决 |
| 2 私聊聊天区每句话新会话 | 已解决 |
| 3 `git -C` 等绕法 | 已解决：`git -C . push`、main 上不带参数的 `git push`、脚本里的推送都拦住了；带来一个误拦（新问题 1） |
| 4 Codex 不会被拦 | 已解决 |
| 5 Codex 的结果出现两遍 | 已解决 |
| 6 manifest 不算请求本身 | 已解决 |
| 7 下载落在工作目录 | 部分解决：默认位置对了，模型给 `--out` 时仍只能写进 worktree（新问题 2） |
| 8 工作记录看不到模型 | 已解决（模型和思考强度都在 run 上） |
| 9 个人的 Claude Code 设置带进 AI 成员 | 这一轮没复查，上手页的说明还没加 |
| 10 一条消息 @ 两个成员 | 已解决 |

## 新发现的问题

1. **审批规则把命令参数里的文字也当成命令（误拦）。** 被拒绝或同意之后，AI 成员用 `chat-run report --state blocked --text "git -C . push origin main 需要审批，被审批人拒绝，未执行"` 汇报，这一行又被当成「合并到 main」拦下，话题里多出第二张「需要你确认：合并到 main」卡，内容是那条 `chat-run report …`，run 一直 `waiting` 到有人再按。Claude Code（run `01M76XSJS97ZZZCSA4JG9RS2DK`）和 Codex（run `01M76XSJS89SJE1P6ECEDY8Z4Z`）都出现了。原因：钩子和 Codex 的请求按整条 shell 行匹配，按「plain words」去掉引号以后，`--text` 里的字就和规则 `git … push` 对上了（`apps/daemon/src/gates.rs`；machine-protocol.md「Gated actions」的 Matching 一段写的就是这个行为）。垫片那条路径按解析过的参数判断，没有这个问题。建议：整行匹配时，引号里的参数不参与，或者至少 daemon 自己的 `chat-run` 命令整条跳过；改了要同步 machine-protocol.md。复现：让成员被拒后用 `chat-run report` 写一句带原命令的话。
2. **`chat-run file get --out` 只许写在工作目录里，模型一给 `--out` 文件就进 worktree。** run `01M76XP7JQEQKTEH37GG6GJHB9`：模型先 `--out "$(mktemp -d)/shapes.png"`，被拒（`forbidden, file.outside_workspace`），于是在工作目录里 `mktemp -d ./dl.XXXXXX` 再下载，看完自己删掉。这次删了，不删的话清理时会被提交进 `bot/<key>`。`apps/daemon/src/chat_run_files.rs` 的 `inside()` 同时管上传和下载。建议：下载的 `--out` 也允许运行目录（`<run folder>/downloads`）和系统临时目录；或者提示词里 `file get` 那句（`server/src/bots/dispatch.ts` 第 31 行、`apps/daemon/src/chat_run.rs` 的说明）写明「不要给 --out，结果里的 path 就是文件位置」。
3. **引用的消息在别的话题时，按 id 读不到。** 请求引用了群聊里的一条消息，提示词里是 `↪ quotes m <id> (…)`，没有话题 id。模型用 `chat-run topic message --id <id>` 去读，回 `not_found`：不带 `--topic` 时只查 run 自己的话题（`server/src/context/routes.ts` 的 `readTopic`）。建议：引用行带上话题（`server/src/context/render.ts` 第 125 行，例如 `↪ quotes m <id> in t <topicId>`），或者 `topic message` 在本话题找不到时在本群里按 id 找。
4. **daemon 重启后续上的 run 一轮都没跑，就在话题里说「完成了。」。** 经过：Forge 要 `sleep 90`，Claude Code 不许前台单独 sleep，它就放到后台（`run_in_background`），这一轮以「第 1 步已做完，第 2 步 sleep 90 正在运行，结束后再执行第 3 步」结束，进程等后台通知。这时正常停 daemon 再开：新 run `01M76YC5J9Q2MTN40TWYQ2QJSR` 用原会话续上、带了恢复说明，但它的 `stream.jsonl` 只有钩子、一条 `task_notification`（`status: stopped`，后台命令在上一个会话结束前没跑完）、`init`，然后 `result: success, num_turns: 0`，结果文字是空的。daemon 报 `succeeded`，服务端用兜底文字（`server/src/bots/dispatch.ts` 第 61 行的 `done: "完成了。"`）发到话题，run `done`，第 3 步没做。（会话文件里能看到后面又有一轮检查文件、重新放后台 sleep 的记录，但那不在这个 run 的输出里，和这个 run 的结束对不上。）要修：(a) 结果为空、`num_turns: 0` 的 run 不能算完成，按「卡住」处理并说清；最后一句明说在等后台命令的，也不该算完成；(b) 续接时要确认恢复说明真的开了一轮（看起来被会话里排着的后台任务通知占掉了）。相关代码：`apps/daemon/src/runtime/` 里 Claude Code 结果的解析，服务端 run 结束时选兜底文字的地方。复现：`.tmp/verify/recovery.ts stop` 的做法（三步里有一个 90 秒的 sleep，停在 sleep 上）。
5. **「全部停止」之后，主话题里看不到停了。** 分出去的那份被小王停掉以后：Scout 的子话题里有「已停止，没有完成。」（没说是谁停的）；Forge 的主话题里那张「Forge 把一部分分给 Scout」卡还挂着「全部停止」按钮，最后一句仍是「等它自己回来后告诉你答案」，请求人在主话题里看不出这件事已经停了。design-relay.zh.md 说链停后结果不交回，这一点对；但它的「结束」一条说被人停下的也回报、结果写「被 {name} 停止」，「话题里看得见」一条说分出去的卡在结果回来时由 A 的 desk 在 `partDone` 里 `card.patch` 一次（「{B} 做完了，结果已交回 {A}」或「{B} 没做完：{原因}」）。这两处都没有发生：卡片应该原地改成已停止（谁停的）并去掉按钮，主话题的状态也该跟着变。
6. **样例预算会拦住走订阅的任务。** 沙盒种子给 Scout 每周 $0.40；跑了几次订阅的小任务以后，Scout 被分出去和被转交的两件事都停在「Scout 这周花了 $0.47，到了预算。继续吗？」。这是线 4 还没做的「走订阅的任务不计入每周预算」（模 4.1）。另外，两张预算卡各按一次「这周再加 $1」后，转交的那个话题里「Joye 这周给 Scout 加了 $1 预算，接着做。」出现了两遍。
7. **Codex 的第一条命令有时起不来。** run `01M76XSJS89SJE1P6ECEDY8Z4Z`：第一条 `/bin/zsh -lc 'git push origin main'` 返回 `Failed to create unified exec process: No such file or directory (os error 2)`，Codex 换个说法再跑就好了。只见过一次，可能和带审批时的 `ZDOTDIR` 垫片目录或工作目录有关，要再看。
8. **一台电脑上同一种 CLI 有两个运行方式时，登录提示分不出是哪一个。** Ledger 用的是 `codexout`，提示说「runner-1 上的 Codex 没有登录」，而同一台电脑上的 `codex` 是登录着的，照着去团队管理找会找错。建议条目名和种类不同时，句子里带上条目名。少见，不急。
9. **排队等电脑时 run 上没有 `phase`。** 电脑离线时交代的 run 是 `status: created`，`phase`、`waitingFor` 都没有；protocol.md「Run phase and take-over」写的是 `waiting_computer` + `offline`。话题里那句话是对的，只是 run 对象没填。另外，所有 run 上都没有 `requestedFor`（protocol.md「Who a run works for」说人发起的 run 要填）。
10. **群聊里同一个成员连着两件事会被并成一件（接 1.5 没做到）。** 在 `开发` 群聊连发「@Forge 运行 sleep 40，然后只回答：甲」和「@Forge 另一件事：只回答 7+8 等于几」：只开了一个 run，第二条以 `input.accepted` 插进了第一件，回答只有「7+8 等于 15」，「甲」那件丢了。第 4 节的默认是「并行，各出各的结果」。两条消息相隔不到 1 秒；间隔长一些时没有测。

## 这一轮没有覆盖的

- 桌面应用（所有界面、单独问、截图），Windows、Linux 主机（虚拟机、家目录上限），自己部署服务器。
- 没有 AI 服务的 key：前台（追问卡、闲聊不出声、由前台判断的跟进）、运行后的经验总结、夜间整理、第二天的「记忆更新」和撤回，都没跑。
- ACP：这台电脑没有配 ACP 的 CLI，ACP 的续接和审批没有真跑。
- 接力（按顺序做）和接管：main 上还没有（接管的路由是 404）。转交的上限（3 层、8 次、回环）真模型没有触发，只有单元测试。
- GitHub 登录：沙盒没有 OAuth 应用。
- 额度用完、模型不可用这两类失败：没法在真账号上安全地造出来。
- 「最后一个所有者不能降级」：万一没拦住会锁死沙盒，没测。

# GitHub 身份按电脑：主机脚本和自部署文档（G7）

- 日期：2026-11-12
- 改了什么：`scripts/runner/host-setup.sh` 的 apt 列表加 `gh`；SELF-HOSTING（中英）加「给 AI 成员一个 GitHub 账号」。
- 查过的：`shellcheck scripts/runner/host-setup.sh` 无输出。在团队的一台 Ubuntu 24.04.5 主机上只读查看：`apt-cache policy gh` 的候选版本是 Ubuntu 自己的 `2.45.0`（universe），所以不用另加 GitHub 的软件源；这个版本有文档里用到的 `gh auth login`（浏览器码和 `--with-token`）、`gh auth status`、`gh auth token --hostname`。主机上没有改任何东西。
- 没走的：没有全新的 Ubuntu 主机，没有重跑 `host-setup.sh`；没有机器账号，所以「在主机上登录」「电脑页显示 `GitHub @<login>`」「提醒来和走」这几步没有真做。这些归 design-github-identity.zh.md 第 7 节的验收。
