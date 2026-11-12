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

# 第三轮

- 日期：2026-11-12
- 测的 main：`228318a3`
- 在哪跑：同一台 Mac，做法和前两轮一样：本 worktree 自己的沙盒服务器（`wrangler dev`，端口在给这一轮的范围里，加了 `--test-scheduled`，并把 `TEAM_TIME_ZONE` 设成一个当时正好是凌晨 2–3 点的时区，好让夜间整理在本轮里按时跑），样例团队来自 `scripts/sandbox/seed.ts`，种子开出的样例任务先取消；从这份代码编出来的 daemon（1.2.7）以 runner-1 连上，`max_parallel_runs = 4`。接管另开了第二个 daemon，作为 Joye 的个人电脑「Joye 的 Mac」。没有开桌面应用，全部通过公开 API 和 WebSocket。
- 真 CLI：Claude Code 2.1.319、Codex CLI 0.159.0，用这台电脑上已经登录的订阅。Forge、Scout（Claude Code）指向一个没有远端的一次性仓库，Taro（Codex）指向另一个，Ledger 用 Codex、不带仓库。
- **这一轮配了 AI 服务**：provider `deepseek`，model `deepseek-chat`，key 从本机 mode 600 的文件读进请求，不打印、不进日志。
- 证据：脚本和输出在 worktree 的 `.tmp/verify/`（不进仓库）。运行 id 是这次本地沙盒里的，不是任何部署环境的。
- 这一轮按协调要求提前收尾，没来得及做的在表里标「没法跑」并写明原因。收尾以后在同一个沙盒里补跑了其中三项（两个 Codex 条目的登录提示、电脑离线时排队的 `phase`、额度撞线的假 CLI 测试），表里已改成补跑的结果。

## 结果

| 路径或条目 | 做了什么 | 结果 | 证据 | 备注 |
| --- | --- | --- | --- | --- |
| AI 服务：测试连接、保存、状态 | 先用错 key 测，再用真 key 测，小王（成员）试着保存，Joye 保存，不带 key 再测一次，读状态 | 通过 | 错 key：`ok: false`，`error` 是 DeepSeek 的 401 原文（只露 key 末 4 位）；真 key：`ok: true`、`text.ok`、`tool.ok`（约 1.9 s）；小王保存 `forbidden`；保存后 `keyHint` 只有末 4 位，`switches` 三项全开；不带 key 用已存的 key 测 `ok`；成员读 `/v1/settings/ai/status` 从全 false 变成全 true；之后 `uses` 里 front_desk、retro、title 都是 `ok, failures: 0` | 整轮 AI 服务调用：front_desk 26、retro 29、title 6、summary 1、tidy 3，失败 0 |
| 前台：问题直接答 | Joye @Forge「用一两句话说说 rebase 和 merge 的区别，不用动代码」 | 通过 | 决定 `reply`（0.97，约 1.6 s），Forge 在话题里直接回答，没有 run | |
| 前台：派活带交代单 | Joye @Forge 新建 `fd.txt` 写 hello、不提交 | 通过 | 决定 `dispatch`，`brief` 有 goal、3 条 doneWhen、notes；run `01M77G9ZTJS0HKKZCWR97BCSJP` 的提示词里有「Work order from your front desk … Goal: … Done when: …」；做完，结果是 done 卡 | |
| 前台：不带 @ 的跟进 | 同一话题不带 @ 说「在 fd.txt 末尾再加一行 world」 | 通过 | 决定 `dispatch`，brief 里写明接着前面那件事；新 run `01M77GQWBJ0J3SCF26XKA8WWS7` 做完，文件两行 | |
| 前台：闲聊不出声 | 同一话题不带 @ 说「好的，辛苦啦 👍」，等 45 秒 | 通过 | 决定 `skip`，没有新消息、没有新 run、没有留下 👀 | |
| AI 话题标题 | 上面两个话题都没给标题 | 通过 | 标题被换成「git rebase 与 merge 区别」「新建 fd.txt 写入 hello」 | |
| 话题摘要 | 小王和 Joye 在一个话题里发 64 条长消息，第 3 条写暗号和回滚负责人；之后 @Forge 只按提示词回答 | 通过 | room 的 `topic_summaries` 有这个话题的摘要（到 #199，写了暗号、负责人、待确认事项）；前台据此直接 `reply` 答对暗号和负责人 | 之后再派一个 run，它的上下文 66 条全部放得下（`omitted: 0`），提示词里没有用到摘要；「被预算挤掉的部分用摘要顶上」这一半没有真触发 |
| 运行后的经验总结 | Scout 被要求用 `python add.py` 运行（这台电脑没有 `python`） | 通过 | run `01M77GYC6VV8PFF5HD1HABJN91` 结束后约 10 秒，Scout 的成员层多了一条 `lesson`，`pending`：「这个环境里 python 不存在，只有 python3 …」，带 evidence（`command not found: python`）和 when，`fromGroup` 是 `开发`，来源是这个 run | 后面每个 run 都写了 1–4 条，有重复和互相矛盾的（新问题 3） |
| 夜间整理（按需跑） | 等到团队时区 03:00，用 `--test-scheduled` 触发每小时的 cron | 通过 | `memory_runs` 记了 3 个 scope 和 `~stale`；`GET /v1/memories/updates`：Forge 17 行、Scout 6 行、Taro 2 行，都是 `added`，记忆变 `active`（version 2）；tidy 3 次调用全部成功 | 没有一条被合并或停用，见新问题 3 |
| 记忆更新撤回 | 撤回 Forge 的第一行 `added` | 通过 | `undoneAt` 有值；那条记忆变 `retired`，version 3；再撤一次 `409 already undone` | |
| 能看到什么：别的群 | Joye（在 `夹夹增长`）在 `开发` @Forge 查李雪在 `夹夹增长` 里发的口令 | 通过 | run `01M77HB8NRDJKHDR0E65PDMDDF`：`chat-run group list` → `search --in <夹夹增长>` → `topic history --in`；答对，并说明出处是 `夹夹增长` | 它把口令原样发进了 `开发`，`开发` 里有不在 `夹夹增长` 的人。设计允许「只带过来需要的那句、说明出处」，这里就是这么做的 |
| 能看到什么：交代人不在的群 | Alex（在 `开发`，不在 `夹夹增长`）问同样的问题 | 通过 | `group list` 只有 开发、运维、全员；`search --all` 找不到；回答「查不到，你不在夹夹增长群里」 | 原本让小王当交代人，被新问题 1 挡住，换成 Alex |
| 能看到什么：私聊 | Joye 在 `开发` 问「我和小王的私聊里的口令」「我在私聊里告诉你的口令」；再在和 Forge 的私聊里问 | 通过 | 两个都答「查不到」，`group list` 里没有私聊；在和 Forge 的私聊里问，答对 | |
| 第二轮问题 1：`chat-run report` 里的命令文字被误拦 | Forge 跑 `git -C . push origin main`、Taro（Codex）跑 `git push origin main`，都按「拒绝」，再让它们用 `chat-run report --state blocked --text` 原样写出那条命令 | 通过 | 两边都只有一张卡，拒绝后原地变「已拒绝：合并到 main」「Joye 拒绝了 · …」、没有按钮；带命令文字的 `chat-run report` 直接 `ok`，没有第二张卡；run 结束 `phase: needs_reply` | 卡上的时间用的是默认时区，不是团队时区（新问题 4） |
| 第二轮问题 2：`file get --out` 到临时目录 | 请求里要求下载到 `mktemp -d` 的目录 | 通过 | `chat-run file get --out "$D/shapes.png"` 成功，返回 `inWorkFolder: false`；图看对 | |
| 第二轮问题 3：引用的消息在别的话题 | 引用 `开发` 群聊里的一条消息，要求按 id 读原文 | 通过 | 提示词里的引用行是 `↪ quotes m <id> in t main-<group>`；`chat-run topic message --id … --topic …` 读到原文；manifest `files: 1, refs: 1` | |
| 第二轮问题 4：daemon 正常停之后续上的 run | 三步，第二步后台 `sleep 90`，sleep 时 SIGTERM 停 daemon 再开 | 通过 | 原 run `cancelled`（`restarted: daemon restarted`）；最后续上的 run `01M77KXS76PWRBZAT49NG005FK` 用同一个 sessionId，发现后台 sleep 被中断，重跑 sleep、等它结束才写 step2，文件「step1\nstep2」，如实说明，`done` | 中间多了一次白跑的 run，见新问题 5 |
| 恢复：daemon 被杀（SIGKILL） | 三步，第二步前台 `python3 … sleep(90)`，sleep 时 `kill -9` daemon 再开 | 通过 | 原 run `cancelled` → 新 run `01M77M347BX6E65F0GMTQS77C3`（`retryOf`），同一个 sessionId，先查文件和 git，没重做 step1，重跑 sleep 后写 step2，`done` | |
| 第二轮问题 5：分出去被停后话题里看得见 | Forge 用 `chat-run assign` 把「前台 sleep 150 后答 4+4」分给 Scout；Scout 开始约 25 秒后小王调 chain stop | 部分通过 | `stoppedRuns: 1`；Scout 的子话题「已被 小王 停止，没有完成。」（说了是谁）；主话题里那张「Forge 把一部分分给 Scout」卡的「全部停止」按钮没了 | 卡片标题和正文没变，主话题里仍看不出「被 小王 停止」，Forge 最后一句还是「结果会自动回到我这里」 |
| 分出去：结果交回 | 同上，没有停（第一次脚本取错了 chain id，停止返回 `not_found`，这件事就跑完了） | 通过 | Scout 跑完后，Forge 在主话题新开一轮，提示词以「The part you gave to Scout is done …」开头，Forge 答「4+4 = 8」 | |
| 转交 | Forge 用 `chat-run handoff` 转交给 Scout 答 5+5 | 通过 | 卡片「Forge 把这件事转交给 Scout」；Scout 在同一话题答「5 + 5 = 10」 | 做完以后卡上仍挂着「全部停止」（新问题 6） |
| 转交 + 全部停止 | 转交给 Scout 前台 sleep 150；Joye 在卡上按「全部停止」 | 通过 | 卡上按钮没了；Scout「已被 Joye 停止，没有完成。」 | 卡片文字同样没写已停止 |
| 第二轮问题 6：订阅的任务不计入预算 | Scout 的每周预算设成 1 美分，再交代一个小任务 | 通过 | run 直接 `done`，没有预算卡 | |
| 第二轮问题 10：群聊里同一成员的第二件事 | `开发` 群聊连发「@Forge 前台 sleep 40 后答：甲」「@Forge 另一件事：…」 | 通过 | 第二件是问题时，前台直接答「15」，第一件的 run 答「甲」；第二件是活（`echo 乙-ok`）时，两个 run 同时在跑，各自答「甲」「乙-ok」 | |
| 接力：走完 | 夹夹增长：`@Ledger 说三种水果 → @Taro 翻成英文` | 通过 | 接力 `running` → 第 1 步 done、第 2 步 working → `done`；两个 run 各一条结果 | 两位 Codex 成员，不带仓库 |
| 接力：仓库里的两位成员 | 开发：`@Forge 新建 relay.txt → @Scout 末尾加一行`，要求不提交 | 不通过 | 第 1 步 done 后接力 `paused, reason: not_pushed`，Scout 一直没开始 | 新问题 2 |
| 接力：停止 | `@Forge 前台 sleep 200 后答 1 → @Scout 答 2`；第 1 步做了 25 秒小王停止 | 通过 | `state: stopped, stoppedBy: 小王`；Forge 的 run `cancelled`；话题里「已被 小王 停止，没有完成。」；再停一次 `409 the relay has ended` | |
| 接力：重来 | 第 1 步读不存在的文件、按要求报 blocked；Joye 补一句内容后调 retry | 部分通过 | 第一次 `paused, reason: step_blocked`；retry 后 `running`，Forge 重新做了这一步（用了 Joye 补的内容）；之后又因 `not_pushed` 停住，没走到第 2 步 | 重来本身对；卡住的是新问题 2。重来上限（第 4 次被拒）没测 |
| 接管 | Joye 交代 Scout 的事放在 Joye 的个人电脑上；Scout 前台 sleep 150 时 Joye 接管，跑 `echo b >> take.txt && cat take.txt && git status --short`，交还 | 通过 | 接管后 run `phase: taken_over, holdKind: takeover`；运行事件 `hand.command`、`hand.output`（`a\nb\n?? take.txt`）、`hand.result`、`hand.summary`；陈晨（管理员）跑命令 `forbidden`，接管 `conflict run.held`；交还后同一个 run 接着做（没开新 run），`lastHandback {commands: 1, files: [take.txt]}`；Scout 的结果说有人在它暂停时执行了 `echo b >> take.txt`，它没有重复 | 团队电脑上的接管要 `takeover-isolated`（虚拟机），这台 Mac 没有，没测 |
| 时限：单步几分钟不被杀 | Forge 前台跑 `python3 … sleep(420)` | 通过 | run 7.3 分钟 `done`，输出 `long-ok`；没有催促、重来或续跑的运行事件 | 30 分钟以上的单步没测 |
| GitHub 按电脑 | `GET /v1/me/github`、`POST …/device`；看电脑；@Forge 跑 `gh auth status` | 通过 | `available: false, unavailableReason: off`；device `conflict github.off`；runner-1 和 Joye 的 Mac 都是 `github.state: signed_in`；run 里的 `gh` 是运行目录里的包装，用这台电脑钥匙串里的登录，`GH_TOKEN`、`GITHUB_TOKEN` 都没设；D1 的 `github_accounts`、`github_device`、`github_logins`、`github_waits` 都是 0 行 | |
| 第二轮问题 7：Codex 第一条命令起不来 | 这一轮 Codex 跑了 4 次 | 没再出现 | | 只见过一次，仍不确定 |
| 第二轮问题 8：两个 Codex 条目的登录提示（补跑） | daemon 配置里加第二个 Codex 条目 `codexout`（`CODEX_HOME` 指向空目录），Ledger 改用它，@Ledger 跑 `echo` | 通过 | 电脑上报 `codex: logged_in`、`codexout: logged_out`；run 立刻 `failed`、`failure.kind: signed_out`；Ledger 说「这次没有开始：runner-1 上的 Codex（codexout） 没有登录，或者登录已经过期。请到 团队管理 › 账号与密钥 › 订阅账号，选 runner-1，点「重新登录」，然后再 @我。」 | 句子里带上了条目名。括号后面多了一个空格（「（codexout） 没有」） |
| 电脑离线时交代、排队的 `phase`（第二轮问题 9，补跑） | 停 daemon，@Forge 跑 `echo offline-ok`，20 秒后读 run，再开 daemon | 通过 | run `01M77MDJDXCR7XHHFH3NC6AEWR` 离线时 `status: created, phase: waiting_computer, waitingFor: offline`，`requestedFor` 是 Joye；话题里「runner-1 不在线，等它上线后自动开始；不需要了可以停止。」；上线后自动做完，答「offline-ok」 | 第一次用「只回答 9+9」，前台直接答了、没有 run，换成要动电脑的活才排队 |
| 额度撞线等重置 | 真账号：没跑。假 CLI：跑了现有测试 | 没法跑（真账号） | `server/` 里 `bunx vitest run test/limit-wait.test.ts`：8 个全过；`cargo test -p daemon limit`：`a_run_at_its_limit_says_so_and_goes_on_in_its_session`、`codex_failure_kinds_and_the_limit_reset`、`claude_not_logged_in_and_the_limit_reset` 等全过 | 真账号上没法安全地撞线；等重置这条只有假 CLI 的证据 |
| 转交的上限（3 层、8 次、回环） | | 没法跑 | | 真模型不会主动触发，只有单元测试 |

## 第二轮问题现在的状态

| 第二轮问题 | 现在 |
| --- | --- |
| 1 `chat-run report` 里的命令文字被误拦 | 已解决（Claude Code 和 Codex 都试了） |
| 2 `file get --out` 只许写在工作目录 | 已解决 |
| 3 引用的消息在别的话题读不到 | 已解决 |
| 4 daemon 重启后一轮没跑就说完成 | 已解决；带出新问题 5 |
| 5 全部停止之后主话题看不到 | 部分解决：按钮去掉了，子话题说了谁停的；卡片文字和主话题状态仍看不出已停止 |
| 6 样例预算拦订阅任务 | 已解决（「加 $1」那句出现两遍没复查） |
| 7 Codex 第一条命令起不来 | 这一轮没出现 |
| 8 两个 Codex 条目的登录提示 | 已解决（补跑：提示里带条目名 `codexout`） |
| 9 排队时没有 `phase`；没有 `requestedFor` | 已解决（补跑：离线排队是 `waiting_computer` + `offline`，`requestedFor` 都有） |
| 10 群聊里同一成员的第二件事被并掉 | 已解决 |
| 第一轮备注 9 个人的 Claude Code 设置带进 AI 成员 | 文案已加（`apps/desktop/src/copy/settings.rs` 的 `THIS_INTRO_SETTINGS`：「…也会带进它做的每一件事，包括钩子、插件和全局指令…」）；没开桌面应用，界面上没看 |

## 新发现的问题

1. **普通成员交代团队电脑上的 AI 成员，因为他自己的个人电脑勾了这个成员、而那台电脑从没连过，任务立刻失败。** 小王在 `开发` @Forge `echo hi`：run `01M77HESF7GX1A576XWQJ4TFQR` 一创建就 `failed`，话题里「现在没有能接手的电脑，给我配好电脑后再 @我。」；同样的话陈晨来说，在 runner-1 上做完。原因：种子里小王的「小王的 MacBook」（个人电脑，从没连上过）勾了 Forge。`server/src/bots/dispatch.ts` 的 `placesFor` 在交代人勾过时只用勾过的电脑；`whyNoMachine` 里 `known` 只算报过 `clis` 的电脑，从没 hello 过的电脑被当成「没有电脑」（`why: "none"`），于是不排队、直接失败。design-roles 3.3 说勾过的电脑不在线时应排队，并在话题里写「在等{电脑}上线。不想等的话，在设置 › 电脑里取消勾选。」（`TEXT.waitPersonal`）。从没连过的电脑也应按「离线、排队、说清在等哪台」处理。复现：沙盒种子原样，以小王 @Forge。
2. **仓库没有远端时，接力在第 1 步之后永远停在 `not_pushed`。** 两位 Claude Code 成员用同一个没有 origin 的仓库：`@Forge 新建 relay.txt → @Scout 末尾加一行`（请求里说不提交），Forge 做完后接力 `paused, reason: not_pushed`，Scout 从没开始；重来以后又停在同一处。design-relay 的规则是「前一步代码 `pushed=false` 且下一步换了成员 → 停住」，但这里根本推不上去：没有 remote、请求要求不提交、两位成员在同一台电脑同一个仓库。建议：本机仓库没有 `origin`，或者两步在同一台电脑、同一个 `repo.path` 时，不按 `not_pushed` 停；否则停住时话题里要说清「代码没推上去，下一位接不上」以及怎么办。现在话题里没有任何说明。Forge 为了交接自己把文件发成了产出，说明提示词里也没说清这种情况怎么交接。
3. **经验总结写得太多、太具体，夜间整理全部照单收下。** 一轮测试下来 Forge 有 17 条、Scout 6 条、Taro 2 条，整理后全部 `added` 成 `active`，没有一条被合并或丢掉。其中 Taro 的两条文字完全相同；Forge 有两条互相矛盾（「Scout 在独立话题里执行，结果不会自动回到本话题，必须主动轮询」和「assign 分出去的子任务结果会自动回到我这里，不用轮询」，后一条是对的）；还有不少只对这次测试有意义的（「take.txt 被外部进程追加了 b」）。要修：复盘的提示词要求只写以后用得上、和已有经验不重复的，0 条是常态；整理时至少合并文字相同的，和已有记忆矛盾的标出来或只留一条；整理结果里可以有 `retired`。相关：`server/src/memory/retro.ts`、`server/src/memory/consolidate.ts`。
4. **审批卡上的时间不按团队时区。** 沙盒设了 `TEAM_TIME_ZONE`（不是上海），卡片「Joye 拒绝了 · 17:31」仍是上海时间（当时团队时区是 02:31）。`server/src/bots/desk/events.ts` 第 353 行的 `stamp(Date.now())` 用了默认时区，应传 `teamZone(this.env)`。和接 2.9（时间用团队时区）是同一类。
5. **daemon 正在停的时候还接了新的重来，白跑一次。** SIGTERM 后 daemon 打出 `daemon stopping`，原 run 结束成 `restarted: daemon restarted`，服务端立刻把重来的 run `01M77KX7D515N6YDMWFM3QYXAD` 派给同一个正在退出的 daemon，它 `accepted` 后又被当成 `daemon restarted` 取消，第三个 run 才真正接着做。结果是对的，只是多一次重来、多一次 CLI 启动。daemon 处于 `stopping` 时应拒绝新的 `run.assign`（例如回 `draining`），服务端等它重连再派。
6. **（小）转交或分出去的事做完以后，卡片上的「全部停止」还在。** 转交给 Scout、Scout 答完以后，「Forge 把这件事转交给 Scout」卡仍挂着「全部停止」；被停止时按钮会去掉，做完时不会。按 design-relay「话题里看得见」，做完也应原地改成「{B} 做完了」并去掉按钮。
7. **（小）工作记录的 `summary` 取结果的第一行，第一行是「输出是：」时什么也没说。** 7 分钟那个 run 的 `summary` 是「命令在前台一次跑完，输出是：」，真正的输出 `long-ok` 在下面的代码块里；第一轮的「我在终端运行了 `echo hi`，输出是：」也是这样。`server/src/bots/desk/events.ts` 第 563 行用 `firstLine(result)`。第一行以冒号结尾、后面是代码块时，应把代码块的第一行带上，或者取到第一句完整的话。
8. **（备注）`BotRun.chainId` 没有填。** protocol.md「Participants, chains and relays」说一件事的每个 run（包括最开始那个）都带 `chainId`，但转交和分出去的 run 在 `GET /v1/runs/:id` 和 `run.updated` 里都没有这个字段，只能从卡片按钮的 `chain.stop:<id>` 里取。这一轮的脚本第一次就是因此拿不到 chain id，停止返回 `not_found`。客户端要按 run 停整件事时会碰到同样的问题。

## 这一轮没有覆盖的

- 桌面应用、Windows、Linux 主机（虚拟机、团队电脑上的接管 `takeover-isolated`）、自己部署服务器、ACP。
- 额度撞线等重置：只能用假 CLI 的测试。
- 接力重来的次数上限、转交的上限、30 分钟以上的单步：协调要求提前收尾，没跑（离线排队和两个 Codex 条目后来补跑了，见表）。
- 摘要在运行提示词里顶替被挤掉的消息：这次的话题放得下，没有触发。

# 第四轮：团队 Linux 主机上的虚拟机

- 日期：2026-11-12
- 测的 main：`228318a3`（#281 之后，含虚拟机状态重做 #268、#270、#271、#280 和 GitHub 按电脑 #273–#278）
- 在哪跑：团队的 Linux 主机（Ubuntu 24.04，KVM，Firecracker 1.17）。主机上的正式 daemon（连正式服务器）和它的 `/srv/runner` **没有碰**：开始和结束时正式 daemon 是同一个进程，配置、目录、虚拟机、防火墙表都没动。测试全部在一个单独的系统用户和单独目录里：origin/main 的检出、自己的沙盒服务器（`wrangler dev`，只听本机，端口在 9620–9639）和 `scripts/sandbox/seed.ts` 的样例团队、从这份检出编的 daemon（release）、自己的 `work_root` 和 `[vm].dir`、用 `build-base-image.sh` 新编的基础镜像（内核拷了主机那份）、一份改了用户和数据目录的 `runner-vm` 和它自己的 sudoers 一行。为了不和正式 daemon 抢槽位（tap 名和地址按槽位算，两个 daemon 都从 0 开始会撞），测试检出里改了一行让槽位从 64 开始（没进仓库）。结束后全部停掉删掉（用户、目录、sudoers、tap、NFS 导出、镜像、主机 rustup），端口和导出查过是空的。
- 模型：主机上已有的订阅登录属于正式 daemon 的用户，没有拿来用（不拷、不改）。Claude Code 和 Codex 都走 DeepSeek（Claude Code 用它的 Anthropic 兼容接口，Codex 用 Responses 接口，模型 `deepseek-v4-pro`），key 只在测试用户 600 的文件和测试配置里。订阅登录进虚拟机的部分只能用一个假的 Codex 登录文件（金丝雀）测。
- AI 成员：Forge、Scout（Claude Code）、Taro（Codex），各指向一个没有远端的一次性仓库；Scout 的是一个很小的 Rust 仓库（`rust-toolchain.toml` 写 `stable`，一个依赖）。最多 4 个任务同时跑。

## 结果

| 条目 | 做了什么 | 结果 | 证据和备注 |
| --- | --- | --- | --- |
| Claude Code 在虚拟机里跑任务 | @Forge 跑 `hostname` | **不通过**（新问题 1） | 每次约 137 秒后失败：「the approval hook did not start」。审批钩子命令是 `<run folder>/bin/chat-run gate-alive`，而 `bin/chat-run` 是指向主机 daemon 程序的软链接，虚拟机里没挂那个目录，钩子起不来，Claude Code 一直等它。同样的设置在主机上（不进虚拟机）0.4 秒就跑了钩子 |
| 同上，绕过之后 | 测试配置加 `[vm].shares = ["<daemon 程序目录>"]`（**只为测试**，正式环境不要这样做） | 通过 | 之后 Claude Code 任务都在虚拟机里做完（`hostname` 是 `vm`）。下面所有 Claude Code 的结果都在这个绕过下 |
| Codex 在虚拟机里跑任务 | @Taro 跑 `hostname`、`git status`、`which git` | 通过 | `model: deepseek-v4-pro`；`git` 是 `/opt/runner/bin/git`（垫片在最前） |
| 几个 Codex 任务同时跑 | 同时开 4 个 @Taro 新话题 | **不通过**（新问题 2） | 3 个失败：`failed to initialize sqlite state runtime under <line>/cli/.codex`；单独跑都成功 |
| 接着说：工作线和会话 | Forge 新话题建 `notes.txt` 写「第一行」不提交、跑 `hostname`；同一话题再说「追加第二行，并凭记忆说上一轮 hostname 的输出」 | 通过 | 两次同一个 sessionId，第 2 次 `resumed: true`；文件两行；答「vm」没有再跑。会话在 `lines/<key>/cli/.claude/` |
| 同一成员两个话题同时跑（vm-state §7 第 4 条） | Forge 两个新话题同时各建一个文件；再同时追问「不用工具，凭记忆说文件和内容」 | 通过 | 两个 `lines/<k>/cli` 分开；各自 `resumed: true`，分别答出 `p1.txt alpha-甲`、`p2.txt beta-乙` |
| 工具链只装一次（§7 第 3 条） | Rust 仓库同时开 3 个话题让 Scout `cargo build` | 通过 | 三个 run 都 `toolchain stable ready`（7–8 秒），缓存里只有一份 `stable` 工具链（617 MB），三个都编译成功 |
| 下一次不再下载、sccache 命中 | 话题 2 里 `cargo clean` 后 `cargo build -v`、`sccache -s` | 通过 | `Downloading` 0 次；sccache 命中 1、未命中 0，位置 `caches/<key>/sccache` |
| 用户级工具跟着仓库，其余随虚拟机清掉（§7 第 6 条） | 同话题 `echo x > $HOME/foo`、`pip install --user httpie`、`npm i -g cowsay`；下一次再看 | 通过 | 下一次 `$HOME/foo`、`http` 没了，`cowsay` 还在（`caches/<key>/npm-global/bin`），`target/debug/rdemo` 还在能跑。`daemon caches` 列出三个缓存 |
| 登录不进虚拟机、不可写 | 测试用户主机上放假的 Codex `auth.json`；@Taro 在虚拟机里看 `$CODEX_HOME`、家目录、挂载 | 部分通过 | 虚拟机里 `~/.codex`、`~/.claude` 都不存在，`/proc/mounts` 没有 CLI 目录或家目录；主机 `auth.json` 前后哈希一样；运行后 `work_root` 下 `auth.json` 0 个。daemon 把假登录判为没登录（第一次 `signed_out` 不派），所以「副本里 refresh token 为空」没看到，要真订阅 |
| CLI 自带记忆关掉 | 同上 | 通过 | 虚拟机里 `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`，codex 参数有 `features.memories=false` |
| 审批：Claude Code | Forge `git push origin main`（拒绝）；`git -C . push origin main`（同意） | 通过 | 都先 `approval.request`、`waiting_approval`；拒绝后没执行、话题 `needs_reply`；同意后才执行（没有远端所以失败）。拒绝那次多了一句 GitHub 没登录（新问题 4） |
| 审批：Codex | Taro `git push origin main`（拒绝） | 通过（单独跑时） | 卡片、`waiting_approval`、拒绝后没执行。和别的任务同时跑时两次被新问题 2 打断 |
| 接管（design-takeover §7 第 3 组） | Forge 跑 150 秒前台等待；25 秒时 Joye 接管，跑一组命令、建 `handnote.txt`，留言后交还 | 通过（一处例外） | `taken_over`、`takeover: isolated`；CLI 进程 `Tl`，虚拟机没停；人的命令 uid 不是任务 uid，`env` 只有 `HOME LANG PATH PWD SHLVL TERM`；读别的进程环境 0 条；连 vsock 2:1025 `Operation not permitted`；`git credential fill` 拿不到；看不到 `cli/` 和缓存；sudo 被拒。交还后 CLI 做完并答出 `handnote.txt` 的内容。例外：人的 `git status` 报 `detected dubious ownership`（新问题 3） |
| `boot-test.sh` | 新基础镜像 | 通过 | `vm-init: ready after 0.72s`，列出 claude、codex、pi、bun、uv、gh 2.45、python 3.12、`rustup 1.29.3`、`sccache 0.19.0` |

## #227 的 17 条和 vm-state §7

#227 的家目录镜像在 1.2.8 被 vm-state 拿掉，第 2、3、10–14、17 条不再适用。

| # | 条目 | 结果 |
| --- | --- | --- |
| 1 | `runner-vm.nft` 的 `fib daddr . iif type != local drop` | 在临时网络命名空间里加载仓库的新表：语法对、规则在。**主机现在装的是旧表**（没有这条），因为正式环境还没重跑 `host-setup.sh` |
| 4、5 | Claude Code 不带 key / 带长期 token | 没测（没用主机订阅） |
| 6 | Codex 副本 refresh 为空、主机文件不变 | 部分（见上） |
| 7 | 虚拟机里看不到登录目录 | 通过 |
| 8 | NFS 收窄 | 新 `runner-vm` 拒绝导出私有目录、整个 `caches`、`images`、根以外的路径，只许一个仓库的缓存；导出只给本虚拟机地址、`all_squash`。从虚拟机连别的槽位地址没测完 |
| 9、15、16 | 升级前话题续接、Pi、内存检查 | 没测 |
| vm-state 1、2 | 升级、升级前话题 | 没做（不碰正式环境），见「怎么安全地升级正式 daemon」 |
| vm-state 3、4、6、9 | 缓存、两个话题、工具保留、记忆 | 通过 |
| vm-state 5、7、8 | 同话题两人并发、72 小时清理、磁盘 LRU | 没测（时间） |

## 新发现的问题

1. **虚拟机里 Claude Code 的审批钩子找不到 `chat-run`，带审批规则的 Claude Code 任务全部起不来（阻塞，正式升级前必须修）。** `apps/daemon/src/run.rs` 的 `gate_hook = dir.join("bin").join(chat_run::link_name())` 在虚拟机里也用这个主机路径；它是指向主机 daemon 程序的软链接，虚拟机不挂那个目录。PATH 垫片那条已经区分了虚拟机（`github::GUEST_BIN`，工具盘上的 `/opt/runner/bin`），钩子没有。修法：`exec.isolated()` 时用 `/opt/runner/bin/chat-run`（`firecracker.rs` 的 `stage_tools` 已放好）。报错说「managed settings allow only managed hooks」，会误导人去查托管设置。`local-e2e.ts --vm` 要加一个带默认审批规则的场景。
2. **几个 Codex 任务同时在虚拟机里跑时起不来：`failed to initialize sqlite state runtime under <line>/cli/.codex`。** 4 个同时 3 个失败，单独都好。主机 `/proc/fs/nfsd/clients/*/info` 里每台虚拟机的 NFS 客户端名都是 `Linux NFSv4.2 vm`（主机名都是 `vm`），同一时刻好几条 `unconfirmed`：服务端把不同虚拟机当成同一个客户端，互相顶掉租约和锁，Codex 的 SQLite 加锁失败；Claude Code 不用 SQLite。这是推断，没改代码验证。修法方向：每台虚拟机唯一的 NFSv4 客户端标识（`firecracker.rs` 启动参数加 `nfs.nfs4_unique_id=<run id>`），或每台不同的主机名（`scripts/runner/base-image/vm-init`）。复现：同时在 3–4 个新话题 @ 同一个 Codex 成员。正式配置 `max_parallel_runs = 12`，升级后会常见。
3. **接管时人在虚拟机里跑 git 报 `detected dubious ownership`。** 人的命令是另一个 uid，共享上的文件属于任务 uid。文件能建（交还后 CLI 读得到），但 git 命令都不能用，而 design-takeover 写了人能 `git status / diff / commit`。修法：`apps/daemon/src/vm_agent.rs` 给人的命令加 `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=*`。
4. **审批被拒后话题里多一句「这台电脑还没有登录 GitHub，推不了代码。已通知管理员」。** 命令没执行、仓库也没远端；daemon 日志 `chat-run github.token: not_found`（垫片执行前就问了 token）。没登录 GitHub 的电脑上每个被拦的推送都会多这句和一条管理员提醒。应只在 git 真要凭证时提示。

## SELF-HOSTING 第 2 部分哪里不对或缺了

1. 第 4 步没说 `scripts/build-daemon.sh` 要 `jq` 和 `shasum`（`shasum` 在 perl 里，这台碰巧有）。
2. daemon 的文件名三处不一致：第 4 步装 `~/.local/bin/team-chat-daemon`，`install-daemon.sh` 装 `~/.local/bin/<slug>-daemon`，现有主机的服务启动 `~/.local/bin/daemon`。「Update」照第 4 步写。要统一。
3. `daemon run --log-file` 只给 Windows 用（Linux 上报错「redirect the output yourself」），没写。
4. 第 8 步示例没有 `self_update`；手编 daemon 时它会去找发布物，应写 `self_update = false` 或说明。
5. 没写一台主机只能跑一个 `[vm]` daemon：两个 daemon 都从槽位 0 开始，tap 名、地址、`runner-vm` 状态文件会撞。写明，或加槽位起点的配置（`VmConfig` 是 `deny_unknown_fields`，注意回滚）。
6. 第 2 步内核仍没有脚本；主机现有 6.1 内核能启动新镜像。

## 怎么安全地升级正式 daemon

前提：问题 1 修好并合进 main（否则升级后所有带审批规则的 Claude Code 任务起不来）；问题 2 最好也修好，否则先把 `max_parallel_runs` 调到 1–2。然后按顺序，每步都能回退：

1. **备份**（root）：`daemon.toml`、`~/.local/bin/daemon`、`/usr/local/sbin/runner-vm`、`/etc/runner-vm.nft` 各拷一份 `.bak`。
2. **等空闲**：应用电脑页上这台没有在做的任务。
3. **主机脚本**（root）：主机检出 `git checkout <修好的版本>`，`bash scripts/runner/host-setup.sh`。装新 `runner-vm`、新防火墙表、主机 rustup，把旧家目录收成只剩会话。注意它会 `apt-get upgrade`，并跑 `install-daemon.sh`（只打印怎么把服务切到它装的文件，不会自动切）。
4. **基础镜像**（root）：`bash scripts/runner/build-base-image.sh`；以 runner 跑 `boot-test.sh`，要看到 `rustup`、`sccache`。
5. **daemon**（runner）：编新程序，写成同目录 `daemon.new` 再 `mv -f` 盖到 `~/.local/bin/daemon`。**不要 stop 服务**：daemon 看到文件变了，空闲时自己退出，systemd 拉起新版本。
6. **配置**（runner）：从 `[vm]` 删掉 `shares = ["~/.claude", "~/.codex", "~/.pi"]`（新版本会跳过，留着容易误会）。空闲时 `systemctl --user restart team-chat-daemon` 让配置生效。
7. **检查**：`daemon doctor` 没有警告；`nft list chain inet runner_vm input` 有 `fib` 一行；在测试群各 @ 一次 Claude Code 和 Codex 成员跑 `hostname`（答 `vm`），再试一次被拦的 `git push`（出卡片）；同时开 3 个 Codex 任务确认问题 2。
8. **回退**：拷回 `.bak`，`systemctl --user restart team-chat-daemon`、`systemctl restart runner-vm-net.service`。旧 daemon 碰到收过的家目录会当普通目录用，能跑。

## 这一轮没有覆盖的

- 主机现有的订阅登录：没用，订阅副本、主机统一刷新、长期 token 都没在虚拟机里真跑。
- 72 小时清理、缓存 LRU、内存检查、同话题两人并发、虚拟机连别的槽位、升级前话题续接、Pi、ACP、桌面应用。
- GitHub：测试用户没登录 `gh`，推送只到「没有远端」为止。
- 前台、夜间整理：DeepSeek key 只当执行者模型用，没配 AI 服务。
