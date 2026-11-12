# 接管的状态机

状态：设计，待实现。属于线 8（第 2 波，排在线 1→2→3 之后）。范围：人看到的一次任务处在哪种状态；暂停、接管、交还怎么切换；接管时人在应用里能做什么；交还后 AI 成员知道什么。

## 1. 目标

负责人的原话：

- 2026-11-12：「接管的话，可以参考一下 Buzz 那个产品，他们好像是做成状态机之类的，那一版可以拿过来借鉴。」
- 2026-11-10：「实时查看这边有很大问题，感觉体验不好。它现在基本无法支持做稍微复杂点的任务，而且好像不会暂停下来，对暂停和任务完成这部分还有很大问题」
- 已定（usable.zh.md 第 1 节）：人只在应用里用，接管也在应用里。不用 Tailscale，不让人从 shell 进去。

这一版做到四件事：

1. 一次任务只有一套给人看的状态，由服务端算，切换写成表。
2. 「暂停」「等你」「完成」分开，不再混。
3. 接管后，人在应用里能在任务的工作目录里跑命令（Mac、Windows、Linux 虚拟机）；不能跑命令的地方，按钮老实叫「暂停」。
4. 交还后，AI 成员拿到一段说明：人停了多久、跑了什么、工作目录变了什么、留了什么话，然后接着做。

## 2. 现状

### 2.1 代码里

- **状态散在四处**：`RunStatus`（`packages/protocol/src/bots.ts:11`）；暂停是旁挂的 `pausedBy`，「a paused run keeps its status」（`docs/protocol.md:1612-1613`）；话题状态另一套（`packages/protocol/src/index.ts:261-268`）；监控又一套（`packages/protocol/src/monitor.ts:63`）。客户端各自拼。
- **「接管」只是暂停**：`takeover.ts:31-57` 发 `run.pause`，`takeover.ts:60-84` 发 `run.resume` 并带一句话，同时立刻清掉 `paused_by`（`takeover.ts:74`）。谁能做：请求人或 owner（`takeover.ts:26-28`）。已结束的任务拒绝（`takeover.ts:40`）。
- **daemon 怎么停**：本机对 CLI 进程组 SIGSTOP（`executor/local.rs:45-50`）；Windows 挂起线程（`executor/local_windows.rs:96-115`）；Linux 主机把整台虚拟机暂停（`executor/firecracker.rs:254-299`），人进不去。`Executor::stop` 只拿到 `run_dir`（`executor/mod.rs:38`）。交还时恢复，并把留言当 `Control::Input` 交给 CLI（`run.rs:600-633`）；没在暂停时收到 resume 直接忽略（`run.rs:615`）；已暂停时再收到 pause 只再报一次 `paused: true`（`run.rs:600-609`）。
- **离线**：`toRun()` 在链路断开时把帧放进 outbox，重连后补发（`server/src/bots/link.ts:628-638`）。租约 10 分钟（`dispatch.ts:7`），过期后 `expireLeases()` 调 `runLost()`（`link.ts:582-595`），`runLost` 不看暂停，直接换电脑或结束（`desk/runs.ts:473-481`）。所以今天被接管的任务，电脑离线 10 分钟就会被重开或失败。
- **人在哪里动手**：没有地方。唯一出口是 `daemon.toml` 里手写的屏幕共享地址（`apps/daemon/src/config.rs:119-144`；`MachineInfo.takeoverUrl`，示例是 tailnet 地址，`docs/machine-protocol.md:431-436`），和已定方向冲突（机 3.4）。
- **虚拟机里**：vm-agent 用任务的 uid 起 CLI，并放进单独的进程组（`vm_agent.rs:316-318`）；只接受一条连接（`vm_agent.rs:206-209`）；和它说话的唯一通道是 `VmProcess.ctl`（`firecracker.rs:228-250`）。
- **忘了交还**：暂停的任务不参与任何计时（`stall.ts:20`、`37`、`116`），没有提醒（盯 6.5）。
- **「完成」和「等你」混在一起**：CLI 成功退出就算完成（`events.ts:360-365`，接 4.2）；只有提问卡才「等你」（`events.ts:292-293`）。结构化汇报（做完 / 卡住 / 在等谁）由线 3 做，本设计只规定它怎么映射到状态。
- **监控**：只要有 `pausedBy` 就显示 paused（`server/src/monitor/index.ts:59`）。
- **桌面端**：实时查看是用工具事件拼的终端（`apps/desktop/src/views/live/mod.rs:1-6`）；接管和交还按钮在 `live/mod.rs:1202-1207`、`337-342`，交还不带留言（`live/mod.rs:146-166`）；文案在 `copy/live.rs:21-27`、`copy/overlays.rs:56-57`。

### 2.2 Buzz 里有什么（github.com/block/buzz，Apache-2.0，2026-11-12 读）

结论：Buzz **没有**「接管一次任务、再交还」的状态机。`VISION_REMOTE_AGENTS.md`、`VISION_ACTIVITY.md` 里都没有暂停、接管、交还。负责人记得的「状态机」，最接近的是下面几处，本设计借它们的做法，不抄代码：

| 文件 | 它做了什么 | 借什么 |
| --- | --- | --- |
| `desktop/src/features/terminal/terminalState.ts` | 应用内终端用 reducer 管「键盘归谁」：`owner: "buzz" \| "terminal"`，切换是完整事务，失焦作废 | 同一时刻只有一个持有人；接管和交还是显式的所有权切换；不是持有人的输入一律不收 |
| `crates/buzz-acp/src/pool_lifecycle.rs` | 显式枚举状态（Listening、Waking、Ready、Failed{retry_at}），非法切换返回错误 | 服务端是唯一裁判，非法切换回 `conflict` |
| `docs/remote-agents.md`「Deploy State Machine」 | 「观察到的状态 → 动作 → 理由」逐行列，重复执行结果不变 | 3.1 的切换表同样写法；交还可以重复发 |
| `crates/buzz-acp/src/queue.rs` 的 `CancelReason { Interrupt, Steer }` | 被打断的请求在下一轮以「[Previous request — interrupted]」开头重新交给 agent | 交还时用一段固定格式的说明告诉 AI 成员发生了什么 |
| `VISION_ACTIVITY.md`「Never go dark」「Mutate in place」 | 安静、空闲、超时都要画出来；一个动作一行，原地更新 | 每个状态一行字加已持续时间；人的命令一条一行，原地更新 |
| `docs/agent-availability.md` | 「不知道」不等于「离线」 | 电脑不在线单独显示「在等电脑」，不算失败 |

不合用的：Buzz 停 agent 靠发消息、没有控制通道（我们有 daemon 链路，接管必须走它）；agent 闲了自己退出（我们有服务端租约）；Nostr 身份；Buzz 的终端是本机 PTY，不连 agent 的电脑。

## 3. 设计

### 3.1 状态

`BotRun` 新增 `phase`，由服务端纯函数算出，是客户端显示的唯一依据。`RunStatus` 和 `pausedBy` 不变。

| phase | 人看到的词 | CLI | 电脑 |
| --- | --- | --- | --- |
| `working` | 在做 | 在跑 | 正常 |
| `waiting_person` | 等 {who} 回复 / 卡住了，需要你看一下 | 还在：停在提问卡的工具调用里，或盯活判了卡住 | 正常 |
| `needs_reply` | 等 {who} 回复 / 卡住了，需要你看一下 | 已退出，会话保留（线 3 的汇报是「在等人」或「卡住」） | 无 |
| `waiting_approval` | 等 {who} 批准 | 停在被拦下的那一步 | 正常 |
| `waiting_computer` | 在等电脑：排队 / 电脑不在线 / 正在准备 | 没起来，或电脑失联 | 无或失联 |
| `paused` | {who} 暂停了 {member} | 停住 | 正常运行 |
| `taken_over` | 你在操作 / {who} 在操作 | 停住 | 正常运行；持有人在工作目录里跑命令 |
| `done` | 完成 | 已退出 | — |
| `failed` | 没做完 | 已退出 | — |
| `stopped` | 已停止 | 已退出 | — |

不设「交还中」：今天交还时 `paused_by` 立刻清掉，`run.resume` 走 outbox，电脑离线也会在回来后补发；多一个中间状态只多一处卡住的可能。

**`phaseOf(row, ctx)`**（新文件 `server/src/bots/desk/phase.ts`，按顺序取第一条命中的；每条一个 vitest）：

1. 已结束（`ended_at` 不为空）：
   1. `cancelled` → `stopped`。
   2. 正常结束（`done`），汇报是「在等人」或「卡住」（`chat-run report --state waiting|blocked`），并且这是该成员在这个话题里最新的任务、后面没有跟进任务 → `needs_reply`（`waitingFor`: `report` 或 `stuck`）。失败的任务不看汇报。线 3 已实现：`server/src/bots/desk/report.ts` 的 `endedPhase`，结束的任务由服务端填 `phase`。
   3. `done` → `done`；其余（`failed`、`blocked`）→ `failed`。被跟进任务盖过的「在等人」也落到这两条。
2. 还没结束：
   1. `paused_by` 不为空：`hold_kind = 'takeover'` → `taken_over`，否则 `paused`（旧数据 `hold_kind` 为空，算 `paused`）。电脑失联时 phase 不变，另给 `waitingFor: "offline"`。
   2. 线 3 的「在等电脑」标记（排队、指定电脑离线、失联、虚拟机启动中）→ `waiting_computer`，`waitingFor`: `queue` / `offline` / `booting`。
   3. `approval_id` 不为空 → `waiting_approval`。
   4. `status = waiting` → `waiting_person`（`question`）；`status = blocked` → `waiting_person`（`stuck`）。
   5. 其余 → `working`。

于是「完成」只来自：任务结束，且汇报是「做完」（没有汇报时沿用今天的规则）。人点暂停是 `paused`，AI 成员要东西是 `waiting_person` 或 `needs_reply`，三者不再混。

### 3.2 切换表

服务端 desk 是唯一裁判，不合法的切换回 `conflict`。「管理员」是线 13 的管理员（含 owner）；「电脑主人」是线 13 的个人电脑的主人；「请求人」是运行的 `requested_for`（协议 `BotRun.requestedFor`，design-roles 3.3；旧运行没有时用 `requestedBy`）。权限和 design-roles 3.2、3.6 对齐：个人的电脑上管理员不能暂停、继续或替人交还。

| 从 | 动作 | 到 | 谁能做 | 理由 |
| --- | --- | --- | --- | --- |
| working、waiting_person、waiting_approval | 暂停 | paused | 请求人；团队的电脑上加管理员；个人的电脑上加电脑主人，管理员不行（design-roles 3.6） | 先让它停下，说句话再继续 |
| working、waiting_person、waiting_approval、paused | 接管 | taken_over | 见下面「谁能接管」 | 接管等于在这台电脑上跑命令 |
| paused | 继续（可带一句话） | working | 暂停的人、请求人；团队的电脑上加管理员，个人的电脑上加电脑主人 | — |
| taken_over | 交还（可带一句话） | working | 持有人；团队的电脑上管理员可「替 {who} 交还」 | 防止忘了交还 |
| paused、taken_over，电脑不在线 | 继续 / 交还 | waiting_computer | 同上 | `run.resume` 进 outbox，电脑回来后补发 |
| paused、taken_over | 电脑失联 | 不变（命令框不能用） | 系统 | 人持有的状态不被系统改掉，见 3.5 的租约 |
| paused、taken_over | 持有人被移出团队或这个群 | working（自动交还，留言「{who} 已离开，自动交还」） | 系统 | 不然提醒发给不在的人，任务永远停着 |
| 还没结束的任何状态 | 停止 | stopped | 群里任何一个人（已有逻辑，`POST /v1/runs/:id/cancel`，`routes.ts:451`；design-relay 的停止也靠这条） | 正在跑的人的命令一起结束 |
| waiting_computer | 暂停 / 接管 | 拒绝：「电脑还没准备好」 | — | 没有可停的东西 |
| needs_reply | 人在话题里回复 | 新的跟进任务（线 2 同会话续上）；旧任务按 3.1 变成 done 或 failed | 和今天回复一样 | CLI 已退出，没法暂停或接管，界面上不出这两个按钮 |
| done、failed、stopped | 任何持有动作 | 拒绝，只能「重试」 | — | 已有逻辑 |

**谁能接管**（接管 = 能跑命令；跑不了命令的地方只有暂停）：

- 电脑没有 `takeover` 特性（旧 daemon）：谁都不能接管，只能暂停。
- 个人的电脑：只有它的主人，不管任务是谁交代的。A 把电脑开放给 B 用，B 的任务在 A 的电脑上跑：A 能接管，B 只能暂停。
- 团队的电脑：只有请求人，且只在报了 `takeover-isolated` 的电脑上（Linux 主机的虚拟机，人的命令换一个 uid 跑，见 3.4）。团队的 Mac / Windows 上，人的命令只能以 daemon 的用户身份跑，等于拿到这台电脑的 shell，能读别人的凭证，所以不给。管理员在团队电脑上能暂停、能替人交还，不能接管别人的任务（接管就能读到请求人自己的 GitHub 凭证）。
- 线 13 合并之前（还没有电脑主人和个人 / 团队之分）：只有 owner 能接管，所有电脑按 owner 的个人电脑算。一个人用时他就是 owner，不受影响。

### 3.3 计时

- `paused`、`taken_over`：不自动交还（自动恢复会踩坏人改到一半的东西）。满 2 小时在应用内提醒持有人一次；满 24 小时再提醒持有人、请求人和管理员。停住的时间不计入时限和卡住计时（今天已如此，`run.rs:619-621`）。
- 人的一条命令：最长 10 分钟，到点强制结束；同一时刻只跑一条；终端里保留最后 16 KB 输出。
- 交还后 2 分钟内 CLI 失败退出（长时间停住后模型连接断了很常见）：不算「没做完」，按线 3 的「同会话打断重来」重开一次，并把交还说明作为第一条输入；话题里说一句「{member} 交还后出错，已从上次的进度重新开始」。
- `waiting_person`、`waiting_approval`、`waiting_computer`、`needs_reply` 的计时归线 3 和线 5，不改。

### 3.4 接管时人能做什么：在工作目录里跑命令

**决定**：接管后，实时查看的终端下面出现命令框。持有人输入一行命令，daemon 在这次任务的工作目录里执行，输出实时进同一个终端，行首标「你」或人名。不做完整的 PTY，不做看屏幕。

**理由**：负责人选了状态机而不是改名，审计 机 3.4 的建议也是「暂停 + 应用里连到工作目录」；一行命令复用今天的终端视图和运行事件通道，三种电脑一样；每条命令都有记录，交还时能原样告诉 AI 成员。完整 PTY 要在 GPUI 里写终端模拟器，还要低延迟转发字节流，留到下一版。

**底线**：时间不够时，命令框（PR T3、T5、T7）可以整体推迟；状态机、暂停 / 等你 / 完成分开、交还说明（T1、T2、T4、T6）必须做。推迟时界面上没有「接管」，只有「暂停」。

能做：看文件和日志、`git status` / `diff` / `commit`、跑测试、改配置（sed、打补丁）、重装依赖。不能做：要交互输入的程序（vim、密码提示）、图形界面。这些可以写进交还留言让 AI 成员做。

| 电脑 | 接管时停什么 | 人的命令怎么跑 |
| --- | --- | --- |
| 本机 macOS / Linux | 和今天一样，对 CLI 进程组 SIGSTOP | `$SHELL -c '<cmd>'`，没有 SHELL 时 macOS 用 `/bin/zsh`、Linux 用 `/bin/bash`；不加 `-l`（免得 profile 把 token 带回来）；新会话（setsid），不在被停住的组里 |
| Windows | 和今天一样挂起线程 | `powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand <cmd>`（实现时改用 `-EncodedCommand`：命令按 UTF-16LE 的 base64 传，免去引号转义；输出设成 UTF-8），单独的 job |
| Linux 主机的虚拟机 | **改成**：虚拟机继续运行，vm-agent 只对它起的 CLI 进程组 SIGSTOP（和本机同一条规则）。不再整台暂停 | vm-agent 以另一个 uid（任务 uid + 1，组是任务的组）执行 `/bin/bash -c <cmd>`，`HOME` 是每次接管新建的临时目录；命令在自己的 mount namespace 里跑，只留工作目录和任务的 git 目录两个共享，成员家目录和运行目录卸掉（见下面「实现时更正」） |

**实现时更正（T5）**：主机给虚拟机的 NFS 共享是 `all_squash,anonuid=<daemon uid>`（`scripts/runner/runner-vm`），虚拟机里任何 uid 访问共享，在主机上都按 daemon 的 uid 判权限。所以只换 uid 挡不住人的命令读成员家目录（CLI 的订阅登录）。vm-agent 在 fork 之后、exec 之前：`setsid`，`unshare(CLONE_NEWNS)`，先把工作目录和 git 目录 bind 到 `/run/tc-hand/<i>`，再卸掉 `Start.mounts` 里的每个共享（由内到外，`MNT_DETACH`），然后把两个目录 bind 回原路径，最后 `setgroups(0)`、`setgid(任务 gid)`、`setuid(任务 uid + 1)`、`chdir`。`/proc/<CLI pid>/environ` 仍靠 uid 不同读不到；`/run/chat-run.sock` 是 600、属于任务 uid，人的命令连不上。但这个套接字只是 vm-agent 的转发，真正的通路是 vsock（CID 2、端口 1025，Firecracker 转到主机的 chat-run，主机按 daemon 的 uid 认人，会用任务的 GitHub 登录回答 `git-credential`、发放审批过的 secret），虚拟机里任何进程都能开 vsock。所以 vm-agent 在 `setuid` 之后再装一个 seccomp 过滤（先 `PR_SET_NO_NEW_PRIVS`）：`socket(AF_VSOCK, …)` 和 `io_uring_setup` 返回 EPERM，x32 调用返回 EPERM，别的架构的调用直接杀掉（`vm_agent::hand_filter`）。装不上就不启动命令。`chmod -R g+rwX` 只对工作目录做（以任务 uid 去改，root 在 NFS 上被 squash），git 目录是同一仓库共用的镜像，不改它的权限。`Restore` 的 chown 在 `all_squash` 下通常没有可改的文件，保留它是为了不走 NFS 的共享。

- 每条命令都从工作目录开始，不保留上一条的 `cd`（一行里写 `cd x && …` 可以）。最简单，不会错。
- 人的命令可以在后台留下进程（`nohup`）；中止和超时只结束它的进程组，留下的进程算人自己的选择，停止任务时一起结束（本机按会话，虚拟机按 uid）。
- **环境**：本机用 daemon 自己的环境，去掉 `gated_env`、`env_passthrough` 的名字和任务注入的值，PATH 里**没有**任务的 bin 目录（`chat-run` 和线 5 的审批拦截都在那里，人的命令不被拦截）。虚拟机里只给 `PATH`、`LANG`、`TERM=dumb`、`HOME`。
- **推送**：在自己的个人电脑上，人的命令用的是这台电脑上他自己的 git / gh 登录，能推。团队虚拟机里人的命令没有任何凭证，推不了；要推就在交还留言里让 AI 成员推（它用请求人的 GitHub 身份）。登录命令在虚拟机里写进临时 HOME，交还后删掉，不会留在 AI 成员的家目录。
- 自己这台电脑（实时查看的任务正跑在打开应用的这台电脑上）上额外显示工作目录的路径和「在访达中打开」（Windows「在资源管理器中打开」）。别的电脑不显示。

### 3.5 交还后怎么接着做

- **持有期间的输入**：只有本来要交给这次任务 CLI 的跟进消息受影响，@ 别的成员、话题里的接力照常（已定 4）。这些跟进照今天的路径发到 daemon，daemon 在持有期间先存在 `run.rs` 里，交还时排在交还说明之后交给 CLI。提问卡和审批卡的回答照今天的路径走，CLI 恢复后读到；daemon 自己的审批（密钥、拦截的命令）照常立刻处理。
- **交还说明**：daemon 在 `resume` 时用新文件 `apps/daemon/src/handback.rs` 拼一段英文（和其他发给 CLI 的提示一样），作为 `Control::Input`：

```
[team-chat] A person held your computer. <name> paused you for 12 min and ran 3 commands in <work dir>:
1. `npm test` → failed … (last 5 lines)
2. `git commit -am "fix env"` → ok
Work folder changes: HEAD a1b2c3 → d4e5f6; modified: .env.example
<name>'s note: …
Check these changes before you continue; do not redo what the person did.
```

  总长不超过 6000 字，超出时先截命令输出。快照在接管开始时记（HEAD 和 `git status --short`；没有 git 时记文件的修改时间）。只是暂停、没跑命令时，只交留言（和今天一样）。

  实现时更正（T4）：daemon 只知道 `run.pause.by` 的成员 id，不知道名字，所以 `<name>` 写成 `Member <id>`（CLI 用 `chat-run member list` 能对上名字）。说明只在接管（`hold: "takeover"`）后交还时写，哪怕一条命令都没跑（人可能在访达里改过文件）；普通暂停只交留言。`git status` 里接管前就有的改动，按文件的大小和修改时间再比一次，人又改过的也算进「changed」。成功的命令不附输出，失败、超时、被中止的附最后 5 行。
- **插话，不打断**：说明在 CLI 当前这一步结束、轮次切换时被读到。三种运行方式的打断接口各不同，做全了超出这一版。
- **话题里**：接管时一行「{who} 接管了 {member}」；交还时一行「{who} 交还了 {member} · {n} 分钟 · 跑了 {k} 条命令 · 改了 {files}」。由桌面端从 `run.updated` 的 `holdKind` 变化和 `lastHandback` 画出来，文字来自文案目录。

### 3.6 协议（只做新增）

**客户端 ↔ 服务端**（`docs/protocol.md`「Run fields, pause and resume」）

```ts
BotRun += {
  phase?: string        // z.string().max(32)；已知值导出为 RUN_PHASES；不认识的值客户端按 status + pausedBy 自己算
  waitingFor?: string   // z.string().max(32)；已知值 RUN_WAITING_FOR = question|stuck|report|queue|offline|booting
  holdKind?: string     // "pause" | "takeover"，只在 pausedBy 存在时有；pausedBy 含义不变，接管时也有值
  heldAt?: Millis
  takeover?: string     // 这台电脑能怎么接管："commands" | "isolated" | "none"
  lastHandback?: { by: Id, heldMs: number, commands: number, files: string[] /* <=20 */, headFrom?: string, headTo?: string }
}
RunCommandRequest = { command: string /* 1..4000 */ }
```

Rust 端（`crates/chat-protocol`）这几个字段用 `String`；fixture 里放一个未知 `phase` 的例子。照 `MonitorRun.state` 的写法（`monitor.ts:62-68`），以后加值不会让旧客户端解析失败。

| 路由 | 谁 | 结果 |
| --- | --- | --- |
| `POST /v1/runs/:id/takeover` | 3.2「谁能接管」 | 带 `holdKind: "takeover"` 的 `BotRun`；从 paused 来时升级；审计 `run.takeover` |
| `POST /v1/runs/:id/pause` | 3.2 | 不变，`holdKind: "pause"` |
| `POST /v1/runs/:id/resume` | 3.2；另外管理员可替人交还 | 不变 |
| `POST /v1/runs/:id/commands` | 只有持有人 | `{ commandId }`；不在接管、已有命令在跑、电脑不在线时 `conflict`；审计 `run.command`（命令文本先脱敏） |
| `POST /v1/runs/:id/commands/:commandId/cancel` | 只有持有人 | `{ ok: true }` |
| `GET /v1/runs/:id/events` | 不变 | `hand.command`、`hand.result` 群成员都看得到；`hand.output` 只给持有人和管理员，别人那里过滤掉 |

**服务端 ↔ daemon**（`docs/machine-protocol.md`；新特性 `takeover`，虚拟机执行器另报 `takeover-isolated`）

```ts
{ t: "run.pause", runId, by?, hold?: "takeover" }               // 新的可选字段
{ t: "run.command", runId, commandId, command /*<=4000*/, by }  // 新帧，只在线时发
{ t: "run.command.cancel", runId, commandId }                   // 新帧，只在线时发
run.event += { type: "hand.command", commandId, command, by }
           | { type: "hand.output",  commandId, text /* 每段 <= 16 KiB */ }
           | { type: "hand.result",  commandId, ok, exitCode?, ms, timedOut?, error?: "not_held"|"busy"|"unsupported" }
           | { type: "hand.summary", by, heldMs, commands, files, headFrom?, headTo? }
```

- `hand.*` 不算 AI 成员的进展：不重置卡住计时，不计入工具调用数，不进打转检测。
- 服务端只对报了 `takeover` 的 daemon 发 `hold` 和 `run.command`。
- 同时把 `machine-protocol.md` 里 `takeoverUrl` 的 tailnet 示例换成占位写法，并注明它不是接管的方式。

**vm-agent 线路**（同一个 daemon 二进制内部，没有版本问题）：`vmwire.rs` 新增 `Hold = 6 {stop: bool}`、`Exec = 7 {id, argv, cwd, env}`、`ExecCancel = 8 {id}`、`ExecOut = 106 {id}+字节`、`ExecExit = 107 {id, code, ms}`、`Restore = 9`（交还时把人建的文件改回任务 uid）。走现有那条 vsock 连接：主机到虚拟机由 `VmProcess.ctl` 发，虚拟机到主机由 `Job` 按标签分给 CLI 的 stdout 或这次命令的输出通道。背压：vm-agent 每条命令最多发 1 MiB 输出（每段 16 KiB），超出只计数、末尾 16 KiB 随 `ExecExit` 带回；CLI 停着时这条连接上没有别的流量。

### 3.7 服务端

- **desk 数据**：命名迁移 `desk-hold-1`，只加列：

```sql
ALTER TABLE runs ADD COLUMN hold_kind TEXT;              -- NULL | 'pause' | 'takeover'
ALTER TABLE runs ADD COLUMN hold_reminded INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN hand_command TEXT;           -- 正在跑的 commandId
ALTER TABLE runs ADD COLUMN handed_back_at INTEGER;      -- 3.3 的 2 分钟规则
ALTER TABLE runs ADD COLUMN last_handback TEXT;          -- hand.summary 的 JSON
```

- **link 数据**：命名迁移 `link-hold-1`：`runs` 加 `held INTEGER NOT NULL DEFAULT 0`。新 RPC `setHeld(runId, held)`：desk 在暂停 / 接管时设 1，交还 / 停止时设 0。`expireLeases()` 跳过 `held = 1` 的任务（`AND held = 0`），所以持有中的任务不会被 `runLost` 换电脑或结束；outbox 也不被清。`runLost` 再加一道：`paused_by` 不为空时只记日志，不动。
- **`toRun(frame, { noQueue: true })`**：链路断开时不进 outbox，返回 `{ ok: true, sent: false }`，命令路由据此回 `conflict`。帧类型联合加 `run.command`、`run.command.cancel`。`run.pause`、`run.resume` 照旧走 outbox，不在 hello 时另外重发（重发会让 `pause_gen` / `pause_seen` 计数错乱，`events.ts:168-180`）。
- **`takeover.ts`**：
  - `takeover()`：检查 3.2，发 `run.pause {hold: "takeover"}`，`pause_gen + 1`，写 `hold_kind`。从 paused 来时同样发一次（daemon 升级，见 3.8），计数照旧成立。
  - `resume()`：保持今天的写法，立刻清 `paused_by`、`hold_kind`，写 `handed_back_at`；先结束正在跑的人的命令；`setHeld(false)`。所以交还后 `scheduleStall` 和 `onMachines`（`stall.ts:20-37`、`116`）照常计时，没有中间状态。
  - `runCommand()`、`cancelCommand()`：只有持有人；`hand_command` 保证一次一条。
  - `releaseHolds(memberId)`：成员被移出群（`server/src/core/routes.ts:112` 的 `removeMemberFromGroup`）或线 13 的移出团队时调用，按 3.2 自动交还。
  - `holdDueAt()`：给 `stall.ts` 的 `scheduleStall()` 多一个最小值，到点发提醒（复用盯活的通知路径），写 `hold_reminded`。
- **`events.ts`**：`hand.result` 清 `hand_command`；`hand.summary` 写 `last_handback` 并广播 `run.updated`；`hand.*` 不碰 `last_event_at`。任务在 `handed_back_at` 后 2 分钟内以失败结束时，走线 3 的同会话重开一次（3.3）。
- **daemon 重启**：持有期间 daemon 崩溃或重启，CLI 没了。按线 3 对丢失任务的规则处理（同会话重开），desk 先清掉持有，交还说明里写「the computer restarted while a person held it」，话题里一行「{computer} 重启过，接管已结束」。线 8 的自更新在这台电脑有持有中的任务时推迟。
- **`core.ts` 的 `toRun()`**：用 `phaseOf()` 填 `phase`、`waitingFor`、`holdKind`、`heldAt`、`takeover`、`lastHandback`。
- **`routes.ts`**（`server/src/bots/routes.ts`）：加 3 条路由、审计，以及 `/events` 对 `hand.output` 的过滤（`routes.ts:379`）。
- **监控**：不改。`monitor/index.ts:59` 继续把持有显示成 paused。

### 3.8 daemon

- **`run.rs`**：
  - `RunCtl::Pause { by, hold }`：没暂停时照旧停住；已暂停而 `hold == takeover` 且当前是 pause，就升级成 takeover，此刻记快照，再报 `paused: true`（今天的「再报一次」路径）。
  - `RunCtl::Resume`：没在暂停时也报 `paused: false`，有留言就照常交给 CLI（今天是直接 `continue`）。在暂停时：结束人的命令、恢复 CLI、拼交还说明交给 CLI、放出持有期间存下的跟进、发 `hand.summary`。
  - 新增 `RunCtl::Command { id, command, by }`、`RunCtl::CommandCancel { id }`：只在 takeover 持有、且 `by` 等于这次 takeover 的 `by` 时接受，否则回 `hand.result { ok: false, error: "not_held" }`；已有命令在跑回 `busy`。
  - 持有期间收到的 `Control::Input` 先存进 `Paused`，交还时放出。
  - 持有期间这个任务不会被 `daemon.rs:559` 当成服务端不认识的任务取消，因为 link 一直保留它（3.7）；链路断开时 daemon 继续保留持有中的任务，不设超时。
- **执行器**（实现时更正：放在 `Executor` 上，不放 `Process`。`Process` 归 runtime 持有，`run.rs` 的监督循环只拿得到 `Executor` 和运行目录）：
  - `Executor::stop(run_dir)` 不变；虚拟机执行器改成按运行目录找到这台虚拟机的 `ctl`（弱引用登记，`VmProcess` 释放后虚拟机照常结束），发 `Hold {stop: true}`，恢复时发 `Hold {stop: false}`。整台暂停的代码删掉。
  - `fn takeover(&self) -> Option<&str>`（`"commands"` / `"isolated"`）、`fn hand(&self, &HandReq) -> io::Result<HandChild>`、`fn hand_back(&self, run_dir)`（虚拟机发 `Restore`）、`fn hand_end(&self, run_dir)`（任务结束时结束人的命令留下的进程：本机按每条命令的进程组，Windows 按每条命令的 job）。默认「不支持」。
  - `HandChild` 带输出流（stdout 和 stderr 合进一根管道）、结束结果和 `cancel`。10 分钟上限、每段 16 KiB、每条 1 MiB 上限和末尾 16 KiB 在 `apps/daemon/src/hand.rs` 里统一做。
- **`vm_agent.rs`**：`Hold` 对 CLI 的进程组 SIGSTOP / SIGCONT；第一次 `Exec` 时把工作目录改成组可读写（`chmod -R g+rwX`，目录加 setgid），家目录不放开；人的命令以任务 uid + 1 起；`Restore` 把人建的文件 chown 回任务 uid。共享目录不允许改权限时，`Exec` 回 `unsupported`，界面显示「这台电脑不支持在接管时运行命令」。
- **`daemon.rs`**：`FEATURES` 加 `takeover`（本机 macOS、Linux、Windows 和虚拟机都能跑命令）；配了 `[vm]` 时再加 `takeover-isolated`；处理 `Down::RunCommand`、`Down::RunCommandCancel`。
- `vmwire::Exec` 另带 `open: [工作目录, git 目录]`（人的命令能看见的共享），`ExecExit` 另带 `dropped`、`tail` 和 `error: "unsupported"`。

### 3.9 桌面端

- **状态词**：话题顶部任务条、成员页、桌面、实时查看标题统一读 `phase`。新增 `RunPhase` 和回退函数（服务端没有 `phase` 或值不认识时，用 `status` 和 `pausedBy` 算）。每个状态都是「词 · 已 N 分钟」，不留空白。
- **实时查看**（`views/live/mod.rs`）底部：

| phase | 按钮（第一个是主按钮） | 横幅 |
| --- | --- | --- |
| working、waiting_person、waiting_approval | 暂停、接管（我能接管时）、停止… | 状态词 |
| needs_reply | 去回复（回到话题输入框） | 「{member} 在等你回复」或「卡住了」 |
| waiting_computer | 停止… | 「在等电脑：…」 |
| paused | 继续、接管（我能接管时）、停止… | 「{who} 暂停了 {member}」，下面留言框 |
| taken_over（我） | 交还、停止… | 「你在操作 {computer} · {member} 已停下」；终端下面命令框和留言框 |
| taken_over（别人） | 管理员看到「替 {who} 交还」 | 「{who} 在操作 {computer}」 |

- 「我能接管」按 3.2 在客户端也算一遍（读 `takeover`、线 13 的角色和电脑主人），服务端仍是裁判。不能接管的人看不到这个按钮，不用灰按钮占位。
- 命令框用 gpui-component 的 `Input`，回车运行；有命令在跑时按钮变「中止」。人的命令在终端里一条一行，原地从「运行中」变成「成功」或「失败」（退出码只在开发者视图里）。
- 持有期间每秒拉一次 `GET /v1/runs/:id/events`；只有持有人在拉，时间短，不另做推送。
- 交还不弹窗：留言框在横幅里，可以不填，点「交还」立即交出。
- 话题里画 3.5 的两行。
- 屏幕共享地址：保留今天的入口，不推广。

**新增的界面文字**（写进 `copy/live.rs`；提醒写进 `copy/notify.rs`；失败提示改 `copy/overlays.rs` 的 `TAKE_OVER_REFUSED` / `HAND_BACK_REFUSED` 旁边）

| 用途 | 文字 |
| --- | --- |
| 状态词 | 在做；等 {who} 回复；卡住了，需要你看一下；等 {who} 批准；在等电脑：排队第 {n} 个；在等电脑：电脑不在线；在等电脑：正在准备；{who} 暂停了 {member}；你在操作 {computer}；{who} 在操作 {computer}；完成；没做完；已停止；· 已 {n} 分钟 |
| 按钮 | 暂停；继续；接管；交还；替 {who} 交还；运行；中止；去回复；在访达中打开；在资源管理器中打开 |
| 命令框 | 在 {folder} 里运行一条命令；运行中；成功；失败；超过 10 分钟，已中止；电脑不在线，回来后才能运行；已有一条命令在运行；这台电脑不支持在接管时运行命令 |
| 留言框 | 留句话给 {member}（可不填）；你跑过的 {n} 条命令会一起告诉 {member} |
| 不可用 | 电脑还没准备好；这台电脑的后台服务太旧，更新后才能接管；{member} 已停下，你在操作 |
| 话题里 | {who} 接管了 {member}；{who} 交还了 {member} · {n} 分钟 · 跑了 {k} 条命令 · 改了 {files}；{who} 已离开，自动交还；{computer} 重启过，接管已结束；{member} 交还后出错，已从上次的进度重新开始 |
| 提醒 | {member} 还停着，等你交还；{who} 接管 {member} 已经 {h} 小时 |

### 3.10 一个人用和团队用

- **一个人用**：他是 owner、请求人、电脑主人，所有判断直接放行。点「接管」，在命令框里跑命令，点「交还」，没有多出的步骤、概念或画面。命令在他自己的 Mac 或 Windows 上、以他自己的身份跑，能推送，能在访达里打开工作目录。
- **团队用**：请求人能暂停，团队电脑上管理员也能，个人电脑上是电脑主人能、管理员不能；个人电脑只有主人能接管；团队电脑只有请求人、只在虚拟机里能接管，命令换 uid 跑、碰不到 AI 成员家目录里的登录和别人的东西；其他人看到「{who} 在操作」和话题里的两行，看不到命令输出；管理员能替忘了交还的人交还。

## 4. 兼容和迁移

- **旧客户端**：不读 `phase`；看到 `pausedBy` 照旧显示「你在操作 / 已暂停」，能交还，没有命令框。新字段都是可选的字符串，以后加值也不会解析失败。
- **旧服务端配新客户端**：用回退函数算状态；`/takeover` 返回 404 时只显示「暂停」。
- **旧 daemon**：没有 `takeover` 特性，服务端不发 `hold`、`run.command`。暂停照旧（虚拟机仍整台暂停），界面上没有「接管」。旧 daemon 在没暂停时收到 resume 仍会忽略，不影响：服务端今天就在 resume 时立刻清 `paused_by`。
- **旧数据**：`hold_kind` 为空按 `pause` 读；link 的 `held` 默认 0。迁移只加列。
- **vm-agent**：和 daemon 同一个二进制，同时升级。

## 5. 安全

- **接管等于在那台电脑上跑命令**，读得到命令身份能读的一切。所以接管权限按「这个身份本来能读什么」来定，不靠去掉环境变量：
  - 个人电脑上，人的命令以 daemon 用户（就是电脑主人自己）跑，能读这台电脑上的一切，包括在这台电脑上跑的别人任务的凭证（同一 uid 能读 `/proc/<pid>/environ`、daemon 配置和钥匙串）。所以只有主人能接管；主人把电脑开放给别人用，别人任务的凭证本来就出现在他的电脑上，这一点由线 13 在「开放给别人」时写明。
  - 团队的 Mac / Windows：不给接管。以 daemon 用户拿到 shell 就能读别人的 GitHub 凭证、机器 token 和其他任务的目录。
  - 团队虚拟机：人的命令用另一个 uid，没有任务的环境变量；`/proc/<CLI pid>/environ` 属于任务 uid，读不到；AI 成员家目录不放开组权限，读不到 CLI 的订阅登录和以前任务留下的东西；只放开这次任务的工作目录。只有请求人能接管，他在工作目录里看到的本来就是他自己任务的东西。
- **去掉密钥只是减少误伤**（防止人的命令把 token 打进输出），不是边界。输出和审计里的命令文本都过 `redact.rs`，它只认得已知的值，这一点不当成保证。
- **服务端和 daemon 都检查持有人**：服务端按 3.2；daemon 核对 `run.command.by` 等于这次 takeover 的 `by`。
- **命令不补发**：`run.command` 不进 outbox，电脑离线时直接拒绝，不会在几个小时后突然执行。
- **人的命令不走审批拦截**：PATH 里没有任务的 bin 目录；审批拦的是 AI 成员。每条命令都进审计（`run.command`）和运行记录。
- **谁能看输出**：`hand.output` 只给持有人和管理员；别人只看到命令和成功 / 失败。
- **交还说明**交给 CLI 时标明来自人；话题消息照接 2.x 的规则当不可信数据框起来。
- 虚拟机不再整台暂停：人的命令仍只在这台虚拟机里，碰不到主机。

## 6. 拆成哪些 PR

都在第 2 波，线 3 合并之后开始（线 1、2、3 改的是同一批 desk 和 `run.rs`，usable.zh.md:300）。

| PR | 做什么 | 主要文件 | 依赖 | 怎么测 |
| --- | --- | --- | --- | --- |
| T1 协议（并进四份设计共用的协议 PR，新模块 `takeover.ts` / `takeover.rs`） | 3.6 的全部新增；换掉 tailnet 示例 | `docs/protocol.md`、`docs/machine-protocol.md`、`packages/protocol/src/{bots,machine}.ts`、`crates/chat-protocol`、`protocol/fixtures` | — | 两边的 fixture 测试，含未知 `phase` |
| T2 服务端状态和持有 | `phase.ts`、`desk-hold-1`、`/takeover`、resume 写法、link 的 `held` 和 `noQueue`、`runLost` 的持有判断、提醒、`releaseHolds`、2 分钟规则、daemon 重启的处理 | `server/src/bots/desk/{phase,takeover,core,events,stall,runs}.ts`、`server/src/bots/link.ts`、`server/src/bots/routes.ts`（只 `/takeover`）、`server/src/core/routes.ts` 一处 | T1、线 3；主人判断要线 13，之前按 3.2 的回退 | vitest：`phaseOf` 每条规则、切换表每一行和每种拒绝、A 的电脑跑 B 的任务、持有中租约过期后仍是 taken_over、`runLost` 不动持有中的任务、旧数据读取 |
| T3 服务端命令 | 命令路由、`hand.*` 处理、`/events` 过滤 `hand.output`、审计 | 新文件 `server/src/bots/desk/hand.ts`、`server/src/bots/routes.ts` | T2 | vitest：离线时 `conflict` 且 outbox 为空、一次一条、非持有人被拒、`hand.output` 对非持有人过滤 |
| T4 daemon 持有和交还 | `hold` 升级、resume 新行为、输入暂存、`handback.rs`、`hand.summary`、`takeover` 特性、本机和 Windows 的 `Process::hand` | `apps/daemon/src/{run,daemon,handback,proto}.rs`、`executor/{mod,local,local_windows}.rs` | T1（可和 T2 并行） | cargo test：从 paused 升级并记快照、没暂停时 resume 回 `paused: false`、交还说明拼法和长度、环境去掉密钥和任务 bin、10 分钟超时、停止时结束命令 |
| T5 daemon 虚拟机 | 只停 CLI 进程组、`Hold` / `Exec` / `Restore`、换 uid、`takeover-isolated` | `executor/{vmwire,firecracker}.rs`、`apps/daemon/src/vm_agent.rs` | T4 | cargo test（帧往返、输出上限）；Linux 主机上走第 7 节第 3 组 |
| T6 桌面端状态 | `RunPhase` 和回退、状态词、按钮表（不含命令框）、留言框、话题两行、提醒 | `apps/desktop/src/views/live/mod.rs`、`provider/panes.rs`、`provider/topic_v2.rs`、`copy/{live,notify,overlays}.rs` | T1（真实效果要 T2） | 文案目录测试；截图自测，浅色和深色，每种 phase；`bun run dev:desktop` 走一遍 |
| T7 桌面端命令框 | 命令框、输出、中止、打开工作目录 | `views/live/mod.rs`、`copy/live.rs`、`provider/panes.rs` | T6、T3 | 截图自测（运行中、成功、失败、超时、离线）；沙箱里走一遍 |
| T8 端到端和文档 | `tools/e2e` 用假 CLI 走接管、命令、交还、离线；PRODUCT.md 的接管说明；agent-harness 里的状态 | `tools/e2e/src`、`docs/PRODUCT.md`、`docs/design/agent-harness.zh.md` | T3、T4 | e2e 对 `wrangler dev` 跑通 |

- 并行：T1 之后 T2 和 T4 并行（服务端和 daemon，不重叠）；T6 和它们并行。T3 接 T2（都改 `bots/routes.ts`），T5 接 T4（都改执行器），T7 接 T6（都改 `live/mod.rs`）。
- 底线：T1、T2、T4、T6 是必做；T3、T5、T7 是命令框，时间不够时整体推迟（3.4）。

## 7. 真电脑上的验收步骤

每组留截图（只截应用窗口，浅色和深色）和运行记录。

1. **Mac，一个人**
   1. 交代一件要跑测试的事。点「接管」：标题「你在操作 …」，`ps` 里 CLI 是 T 状态。
   2. 命令框跑 `git status`、`npm test`，输出实时出现，标「你」，结束时显示「成功」或「失败」。
   3. 改一个文件并提交；`git push` 成功（用的是自己的登录）。点「在访达中打开」能打开工作目录。
   4. 留言「我修了 .env」，点「交还」：状态变「在做」；话题里出现「… 交还了 … · 跑了 3 条命令 · 改了 .env.example」。
   5. AI 成员接下来的回复提到你的提交，没有重做。
   6. 点「暂停」再「继续」：状态只在「已暂停」和「在做」之间变，不出现「完成」。
2. **Windows**：同第 1 组，命令用 PowerShell；`dir`、`git log` 有输出；中止 `Start-Sleep 600` 立即生效。
3. **Linux 主机上的虚拟机**（团队电脑，请求人接管）
   1. 接管后虚拟机没有整台暂停：daemon metrics 里这次任务的 `vm_paused` 是 false（firecracker API 状态 Running）。
   2. `hostname` 是虚拟机的名字；`id -u` 不是任务的 uid。
   3. `env` 里没有注入的密钥和 GitHub token；`cat /proc/*/environ` 读不到 CLI 的环境；`cat ~<任务用户>/.claude/.credentials.json`（或 Codex 的对应文件）被拒绝。命令里连 vsock CID 2 端口 1025（比如 `python3 -c 'import socket; socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM).connect((2, 1025))'`）失败（EPERM），`git credential fill` 拿不到任务的 token。
   4. 在工作目录里改一个文件；交还后 `ls -l` 显示它属于任务 uid，CLI 接着做。
4. **团队**
   1. A 的个人电脑跑 B 的任务：B 只看到「暂停」；A 能接管。
   2. 成员 C 在团队 Mac 上自己的任务：只有「暂停」，没有「接管」。
   3. 管理员看 B 在虚拟机里的接管：看到「B 在操作」、命令和输出（普通成员只看到命令和成功或失败）；2 小时后 B 收到提醒；管理员能「替 B 交还」。
   4. 把持有中的 B 移出群：任务自动交还，话题里一行「B 已离开，自动交还」。
5. **其他**
   1. 接管时合上笔记本，30 分钟后打开：仍是「你在操作」（租约没过期，任务没换电脑），命令框恢复；离线期间点「运行」被拒，回来后不会补跑；交还后任务接着做。
   2. 接管 3 小时后交还，CLI 立刻报错：话题里一行「交还后出错，已从上次的进度重新开始」，任务没有显示「没做完」。
   3. 旧 daemon 的电脑：没有「接管」，暂停照旧能用。
   4. AI 成员结尾说「需要你给我 API key」（线 3 合并后）：话题显示「等你回复」，不是「完成」；回复后开出跟进任务，旧任务不再显示「等你回复」。

## 8. 这一版不做的

- 完整的交互终端（PTY），vim 和要密码的交互程序；命令之间保留目录和 shell 状态；文件浏览器和在线编辑。
- 看真屏幕、操作图形界面、远程桌面。屏幕共享地址保持现状，不推广。
- 团队 Mac / Windows 上的接管（需要先有隔离的身份）；管理员接管别人的任务。
- 交还时打断 CLI 当前轮次（各运行方式的 interrupt）。
- 持有超时后自动交还或自动停止（除了持有人离开）。
- 手机上接管。
- 实时查看的其他问题：它是用工具事件拼的，不是屏幕（盯 6.3，审计结论「先接受，文案别叫看电脑」）。本设计解决负责人抱怨里「暂停和任务完成」那部分；「复杂任务撑不住、不会停下来」归线 3（盯活、时限、结构化汇报）。
- 监控页区分「暂停」和「接管」。

## 9. 还要负责人定的

无。
