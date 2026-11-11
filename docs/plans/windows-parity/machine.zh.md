# Windows 机器对齐清单（daemon、端到端、「用这台电脑」、安装命令）

目标：一次 bot 的 run、机器的主人、服务器，从 Windows 机器得到的结果和从 Mac 机器得到的一样。
这份清单从代码里逐处查出来，每一行是一处：Mac / Linux 上怎样，Windows 上现在怎样，状态，怎么补。
背景、已做的决定见 [`../windows.zh.md`](../windows.zh.md)，线上约定见
[`../../machine-protocol.md`](../../machine-protocol.md)。

状态只有三种：

- **一致**：结果相同（做法可以不同，写明做法）。
- **缺口**：Windows 上少了东西、或者还没证明。每个缺口有编号（G1…），补完改成「一致」。
- **无对应**：Windows 没有这件 Unix 的东西，写明原因，以及给出同样保证的替代做法（没有替代的，
  说明为什么不需要）。

范围：`apps/daemon/**`、`tools/e2e/**`、`apps/desktop/src/provider/this_mac.rs`、
`scripts/install-daemon.ps1` 和它的测试。客户端其余部分、安装包和更新程序各有自己的清单。

没有人有 Windows 电脑。验证只有两条路：在 Mac 上
`cargo check --locked --target x86_64-pc-windows-gnu -p daemon -p e2e --all-targets --features test-fakes`
（只管编译），和 Windows CI 的 `Daemon tests (Windows)` 任务（`windows-2025`，真的 Windows，Windows 的
代码只在那里运行）。下文「CI 跑过」都指后者。daemon 的测试在三个系统的 CI 上都跑：`Rust`（Ubuntu）、
`Daemon tests (Windows)`、`Daemon tests (macOS)`（macOS 15，真的 Claude Code 和 Codex 对着假模型）。

## 0. 缺口和它们的结局

清单第一版列出 20 个缺口，真 CLI 的测试在 Windows 上又抓出一个（G32），三轮评审又指出 19 个（G33 到 G51），修评审意见时自己又查出一个（G52：有的是
清单第一版把没验证过的行写成了「一致」，有的是两个系统上都有的洞）。除了下面声明的两处差别，现在全部补完；剩下的是 CI 做不到的事，在第 8 节。

声明的差别（不是「一致」，清单里每一处都按这个写）：

1. **Codex 旁聊的命令在 Windows 上没有被断网**（G52 的后半）。Mac 和 Linux 上 Codex 的 `read-only` 沙箱同时挡住写文件和联网；
   Windows 上 daemon 给旁聊打开的是 Codex 沙箱里不需要管理员的 `unelevated` 档，它挡住写文件，联网只是把命令环境里的
   代理设置指向一个不通的端口：守规矩读代理设置的程序（curl、git、npm、pip）连不出去，不理会代理设置的程序照样连得上。
   为什么补不上，见第 6 节「Codex 的旁聊」那一行。机器主人看到的：Windows 上 Codex 的旁聊改不了任何文件，但它跑的命令
   能连网络，也就是能把它读到的工作目录内容发出去；`daemon doctor` 在配了 Codex 的 Windows 机器上有一行
   `codex side chat: read-only (no writes); its commands can still reach the network on Windows`。Claude Code 的旁聊不受影响
   （它在三个系统上都只有读文件的工具，不跑命令）。
2. **`CliInfo.gates` 在 Windows 上有版本前提**（G1、G40）：低于 2.1.322 或读不出版本的 Claude Code 不拦命令，见第 1 节那一行。

| 编号 | 缺口 | 结局 | 证据（都在 CI 上） |
| --- | --- | --- | --- |
| G1 | Windows 的 Claude Code 不上报 `CliInfo.gates`，带审批的 run 不拦命令 | 已打开。钩子在 Windows 上改成不经过 shell 的写法，加「钩子失败就拦」 | `integration.rs::the_real_cli_waits_for_a_person_at_a_gated_command`、`real_cli.rs`（真的 CLI，假模型） |
| G2 | 钩子只匹配 `Bash` 和 MCP 工具 | 匹配改成 `Bash`、`PowerShell`、`Monitor` 和 MCP（两边都改） | `real_cli.rs::the_powershell_tool_goes_through_the_hook_too` |
| G3 | 不报 CPU、内存、开机时间 | 已报 | `metrics.rs` 单元测试 |
| G4 | 负载恒为 0 | daemon 自己算 | `sys.rs` 单元测试 |
| G5 | 登录状态判断里只认 macOS 的那一支 | 核对过：Windows 的 Claude Code 把凭据放在 `.credentials.json`（和 Linux 一样），现有规则就是对的 | 读 CLI 随包代码（见第 6 节） |
| G6 | `tests/integration.rs` 只在 Unix 上跑 | 在 Windows 上跑 | `Daemon tests (Windows)` |
| G7 | `tests/setup.rs` 只在 Unix 上跑 | 在 Windows 上跑（launchd、systemd 两个用例除外） | 同上 |
| G8 | 真 Codex 的用例只在 Unix 上跑 | 在 Windows 上跑，并加了「旁聊写不了东西」 | `runtimes.rs` |
| G9 | `local-e2e.ts` 只在 Linux 上跑 | 在 Windows 上跑 | `Daemon chain (Windows…)` |
| G10 | 「用这台电脑」只编译过 | 在 runner 上从头到尾跑一遍 | `this_mac.rs::this_computer_becomes_a_machine_and_stops_being_one` |
| G11 | 真的 `claude`、`codex` 没见过 | 按名字找到、`.cmd` 解析到 `.exe`、有版本、`doctor` 报出来 | `real_cli.rs::the_real_clis_are_found_by_name_and_say_their_version` |
| G12 | 会话目录名规则是推断的 | 推断错了一处，已改：CLI 在 Windows 上不把短文件名（`RUNNER~1`）展开，daemon 原来会展开。对着真 CLI 核对：文件就在 daemon 算出的目录里 | `real_cli.rs::the_real_cli_runs_a_turn_and_keeps_its_session_where_the_daemon_looks` |
| G13 | Codex 沙箱下的 `chat-run` | run 实际拿到的沙箱下（`danger-full-access`）真 Codex 启动真 `chat-run`，经请求目录往返 | `runtimes.rs::codex_on_windows_reaches_the_daemon_through_the_request_folder` |
| G14 | 登录时会不会开本机浏览器 | 不会：CLI 在 Windows 上只会运行 `BROWSER` 指的程序，没有别的路 | 读 CLI 随包代码；`real_cli.rs::a_sign_in_with_the_real_cli_reports_the_link_and_can_be_cancelled` |
| G18 | 只读目录在普通用户身份下没测全 | 以没有管理员权限的用户跑了一遍，八种改动全被拒绝 | `Daemon tests (Windows)` 里的一步 |
| G19 | 负载下的暂停 | 每 20 毫秒启动一个新进程的树，暂停后没有任何进展 | `process_tree.rs::pause_holds_a_tree_that_keeps_starting_processes` |
| G20 | `git` 超时后留下子进程 | `git` 进 job | 代码 |
| G24 | 不在 `PATH` 上的常见安装目录 | daemon 每次启动把存在的常见目录补到 `PATH` 后面 | `sys.rs` 单元测试 |
| G27 | `doctor` 不查 Git for Windows | 不需要查：Claude Code 2.1 起没有 Git Bash 就用 PowerShell。`doctor` 现在写明它用哪个 | 读 CLI 更新日志和随包代码 |
| G32 | 会话 id 是从 `/dev/urandom` 读的，Windows 没有这个文件：每个 Claude Code 会话的 id 都是同一个全零的值，续接可能接到别的话题的会话 | 改用系统的随机数源（三个系统同一段代码） | `runtimes.rs::session_ids_are_random_on_every_system`；是 `real_cli.rs` 在 Windows 上第一次跑时暴露的 |
| G31 | 整条链没连起来跑过 | 安装命令、计划任务、本地服务器、run、取消、空闲时更新、run 进行中更新、注销和登录、卸载，一条龙 | `Daemon chain (Windows…)` |
| G33 | 守住「bot 读写不出 run 目录」的三个单元测试只在 Unix 上跑；Windows 上那两段检查从没对着 `..\`、`C:foo`、别的盘、`\\server\share`、`\\?\`、junction、短文件名、结尾的点和空格、`file::$DATA`、大小写跑过 | 三个测试在 Windows 上跑，加了这些写法的用例。查出并补了一类真的问题：网络路径（`\\server\share\…`）会让 daemon 以用户的身份去登录 CLI 指定的服务器，设备和 `\\?\` 写法能绕过规则。现在这些写法在碰文件系统之前就按文本拒绝（`sys::windows_path_problem`） | `chat_run_files.rs`、`chat_run_context.rs`、`sys.rs` 的单元测试 |
| G34 | `apps/daemon/e2e/real-cli-pause.ts` 是「真 CLI 的暂停」唯一的证明，它读 Unix 的进程状态 `T`，要登录，清单里没列 | 加了不用登录的对应测试：真的 Claude Code 对着假模型，在它的 Bash 工具里跑一个每秒写一行的循环；暂停后 run 下面每个进程都停住（Unix 是状态 `T`，Windows 是进程的每个线程都被挂起）、不再写行，继续后接着写，取消后全部结束 | `tests/real_cli_pause.rs`（三个系统）。原脚本仍是 Mac / Linux 上带登录、真模型的手动检查 |
| G35 | Windows 的负载：`doctor` 是一次性进程，永远打印 0；每个 daemon 启动后从 0 起步，约三分钟才收敛，所以每次重启或更新之后，忙的 Windows 机器会报「很闲」并在派活时被优先选中 | 第一次读取时先实测半秒，三个平均值都从实测值起步 | `sys.rs::the_first_windows_load_is_measured_not_zero` |
| G36 | 登录状态那一行写过头了，还说「本 PR 已补」，其实没改登录的代码 | 改正了那一行（第 1 节）：没有改代码，Windows 和 Linux 用同一条规则 | — |
| G37 | 把测试搬到 Windows 时放松了两处：管道「run 结束后没了」没断言；「权限 600 / 700」换成了只看管道名 | 补上：run 结束后连不上管道（不存在）；从一个连接上读出管道的所有者和访问列表，只有本用户 | `integration.rs`、`chat_run.rs` 的单元测试 |
| G38 | 默认的审批规则认不出 Windows 上的写法：`gh.exe pr merge`、带路径带引号的程序、`Remove-Item -Recurse -Force C:\`、`rd /s /q`、`del`、`erase`。而机器一旦上报 `gates`，服务器就告诉 bot「机器会问，你别自己问」 | daemon 匹配时除了原样，还按「去掉目录、引号、`.exe`/`.cmd`/`.bat`/`.com`/`.ps1` 的程序名加去掉引号的参数」再匹配一次（默认规则和团队自己写的规则都受益，Unix 上带引号的写法也补上了）；默认的删除规则加了 PowerShell、cmd、Git Bash 删盘根和用户目录的写法。原有的 Unix 命中和不命中一个没变 | `gates.rs` 单元测试（每一类、两种系统的写法）；`integration.rs` 里真 CLI 经 PowerShell 工具逐条被拦 |
| G39 | 用户或项目的 Claude Code 设置里 `disableAllHooks: true`，或者受管设置只允许受管钩子，审批钩子就被去掉，而机器仍在说自己会拦。Mac 和 Linux 上一样 | 对着真 CLI 证实了这个洞。run 的设置里写明 `disableAllHooks: false`（它的优先级高于用户和项目的设置，真 CLI 上验证）；受管设置优先级更高、改不了，所以再装一个启动钩子：CLI 一启动就跑它，daemon 等它跑过才把提示词交给 CLI。钩子被关掉的 CLI 永远拿不到带审批的 run 的提示词，run 以明确的原因失败 | `integration.rs`：真 CLI 加用户的 `disableAllHooks`；CI 上写入受管设置后真 CLI 的 run 不启动（Linux 和 Windows）；假 CLI 的同一条规则 |
| G40 | 运行时用的判断（`enforced_by`）和上报用的（`reported_for`）不是同一个：审批规则如果到了一个低于版本下限的 Windows CLI，钩子失败会放行。下限 2.1.295 也没测过 | 没被上报为「会拦」的 CLI 收到带审批的 run，run 直接失败；下限改成 CI 实际测的 2.1.322 | `gates.rs` 单元测试 |
| G41 | 重启和每分钟触发器抢跑：结束任务只结束 conhost，触发器这时可能再起一个 daemon，它覆盖 pid 文件；前一个退出时 pid 文件被无条件删掉，之后卸载找不到 daemon，第二个继续接活而界面显示「未设置」。没有单实例保证 | 每个用户、每份配置一把全机的锁（命名互斥量，daemon 活着就拿着）：第二个 daemon 立刻退出。pid 文件只在它还写着被停掉的那个进程时才删。安装失败不留东西 | `service_task.rs`：同配置的第二个 daemon 不运行；停得慢的 daemon 加不断触发的启动之下重启、卸载，最后恰好一个（或零个）daemon，pid 文件指着它 |
| G42 | `task.xml` 在注册任务之前就写下了，注册失败它还在，「用这台电脑」读成「正常」 | `task.xml` 只在注册成功之后出现；客户端的状态先问任务计划程序任务在不在（`schtasks /Query` 的退出码），再读 `task.xml` 看它跑的是哪个 daemon | `service_task.rs::a_failed_install_leaves_nothing`；`this_mac.rs` 的测试里有「有文件、没任务」读成未设置 |
| G43 | `local-e2e.ts --service` 的自我保护太窄，清理按进程名杀：在开发者的电脑上可能覆盖他的任务、杀掉他的 daemon | 任务已存在、状态目录已存在、有同名进程在跑、`HOME` 下有配置、设了 `CHAT_DAEMON_CONFIG`，任何一条都拒绝运行；清理只杀脚本自己看见启动的进程 | 代码 |
| G44 | `pty` 两行写着「一致」但不一致：Windows 拒绝批处理文件；输出是「画屏幕」，行会连在一起；Windows 的测试暂停后没有断言、也没在运行中途发输入 | 批处理文件能当程序；伪控制台的输出还原成行；Unix 的两个测试搬到 Windows，断言相同，另加批处理和暂停 | `runtimes.rs`：`pty_prompt_input_and_exit_code`、`pty_runs_a_batch_file`、`pty_pause_holds_and_cancel_ends_the_tree`；`pty.rs` 单元测试 |
| G45 | 非管理员用户没被证明：CI 上那一步是「只报告」，其余的服务测试都以提升权限的管理员跑 | 那一步改成必须通过：建一个标准用户，让它经远程桌面登录到 runner 自己（一个真的登录会话），以它的身份跑 daemon 的测试程序和安装命令；计划任务是任务计划程序在这个会话里启动的（见第 5 节） | `scripts/test-daemon-standard-user.ps1`，`Daemon tests (Windows)` 里的一步 |
| G46 | `codex_sandbox_lets_chat_run_answer_without_network` 只在 Unix 上有 | Windows 上没有对应的证明，只有查清的事实：run 用不到 Codex 的 Windows 沙箱（用户自己的配置开了也一样），这一条有测试；手动打开沙箱时，两档都没能启动 `chat-run`（第 6 节），所以「沙箱里的 `chat-run` 能用」在 Windows 上没被证明，第 8 节第 2 条留了真机步骤 | `runtimes.rs::codex_windows_sandbox_modes_start_nothing_or_name_the_sandbox`（只证明：不卡住；每次调用要么没启动命令，要么 `chat-run` 回答了，要么报错里点了沙箱的名）、`codex_on_windows_reaches_the_daemon_through_the_request_folder` |
| G47 | 单元测试 `a_folder_the_sandbox_will_not_write_names_the_sandbox` 只在 Unix 上跑，清单里没列 | 改成两边同一个测试 | `chat_run.rs` |
| G48 | 电脑睡眠：Windows 的单调时钟把睡着的时间也算进去，醒来后睡了五分钟以上的 run 会被报成「卡住」，有时长上限的 run 会被当成超时停掉；连接要等三个心跳没回音才重连 | 三个系统同一段代码：计时的地方先问「上一次到这一次是不是睡过去了」，睡着的时间不算沉默、不算进时长；心跳发现睡过就立刻重连 | `integration.rs::a_machine_that_slept_connects_again_at_once_and_its_run_is_not_stalled`（把一个真的 daemon 进程冻住 66 秒，三个系统） |
| G49 | 暂停着的 run 遇上睡眠，Windows 上时长上限被推后两次：暂停时钟本来就停着，睡着的时间又加了一遍 | 睡着的时间只算一次：run 暂停着时不再另加（`run.rs` 的 `after_sleep`） | `run.rs` 单元测试 `a_sleep_is_counted_once_also_for_a_paused_run`（三个系统） |
| G50 | 「是不是睡过去了」用墙上时钟判断：改系统时间、对时会被当成睡眠，连接被白白断开重连 | 心跳改读「开机以来」的时钟，它把睡着的时间算进去、不受改时间影响（Linux `CLOCK_BOOTTIME`，macOS `CLOCK_MONOTONIC`，Windows `GetTickCount64`） | `sys.rs::the_time_since_boot_moves_forward`、`integration.rs` 的冻住 66 秒那一条（三个系统）。「改时间不被当成睡眠」没有测试（CI 上不改系统时间），靠的是这三个时钟的定义 |
| G51 | ConPTY 屏幕转成行时，「光标右移 N 格」没有上限：一个程序输出很多 `ESC[999C` 就能让一行无限变长 | 右移不超过屏幕宽度（200 列），一行的总长也封顶 | `pty.rs` 单元测试（转换的代码不分系统，三个系统的 CI 都跑） |
| G52 | Codex 的旁聊在 Windows 上并不只读：Codex 的 `read-only` 只靠它自己的 Windows 沙箱执行，而那个沙箱默认不开，命令照样能写文件（工作目录里有 daemon 的访问列表挡着，目录外没有），也能联网。原来的测试在 Windows 上通过，是因为测试里那条写文件的命令引号写错、命令本身失败了 | **补了一半**。写文件：补上了，旁聊的 run 在 Windows 上打开 Codex 沙箱里不需要管理员的那一档（`windows.sandbox="unelevated"`），写被拒绝，读照常。联网：**没有补上，是声明的差别**（本节开头第 1 条，原因在第 6 节） | 写文件：`runtimes.rs::codex_read_only_sandbox_writes_nothing`（改对了命令，目录外的目标不放在临时目录下，加了「读得到文件」；三个系统）。联网：`runtimes.rs::codex_side_chat_network_on_windows` 用 daemon 实际发的参数断言「读代理设置的程序连不到本机地址上的监听端口」，并把「不理会代理设置的程序连上了」记在 CI 日志里 |

## 1. 上报给服务器的能力

`hello`、心跳里 daemon 自己报的每一项（`apps/daemon/src/daemon.rs`、`health.rs`、`metrics.rs`）。

| 项 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| `machine.features`：`pause` `answer` `aside` `acceptance` `interact` `output` `resume` `context` `effort` `gates` | 全部上报 | 全部上报（同一个常量 `FEATURES`） | 一致 | — |
| `machine.features`：`accounts` | 有 `machine.key` 时上报（token 旁边，600） | 同样；`machine.key` 创建时就带访问列表，别人能读就不启用 | 一致 | — |
| `machine.os` / `arch` | `macos` / `linux` | `windows` | 一致 | — |
| `machine.publicKey` | X25519 公钥 | 同 | 一致 | — |
| `machine.takeoverUrl` | 配置里的 `takeover_url`（如 `vnc://…`） | 同一个校验，不限协议，`rdp://主机` 可以填 | 一致 | — |
| `CliInfo.available` / `command` | 按 `PATH` 找 | 按 `PATH` 加 `PATHEXT` 找，只认 `.com/.exe/.bat/.cmd` | 一致（真 CLI 见 G11） | — |
| `CliInfo.version` | `<cli> --version` 第一行，10 秒 | 同，子进程在 job 里 | 一致（真 CLI 见 G11） | — |
| `CliInfo.models` / `efforts` | `claude --help`、`codex app-server` 的 `model/list` | 同（`runtimes.rs` 在 CI 上对着假 CLI 跑过） | 一致 | — |
| `CliInfo.gates` | 每一个 Claude Code 都上报 `true`，不看版本 | 有条件：`claude --version` 在 10 秒内读得出来，并且版本不低于 2.1.322（CI 实际测的版本）才上报。读不出版本或更旧：不上报；这样的 CLI 如果仍收到带审批的 run，run 失败 | **有条件的差别**，不是完全一致（G1、G40）。原因：Windows 上钩子靠 2.1.295 才有的 `onFailure`，更旧的 CLI 钩子启动失败会放行；Unix 的钩子一直是失败放行，没有这个前提 | 机器主人在旧 CLI 的 Windows 机器上看到的：`daemon doctor` 多一行「gated actions: off」并写明要升级到哪个版本；团队那边，服务器不再对这台机器的 run 说「机器会拦」，而是把规则作为文字交给 bot（和 Codex 一样，bot 自己在话题里问）。升级 CLI、daemon 下次连接后恢复 |
| `health.diskFreeBytes` / `diskTotalBytes` | `statvfs` | `GetDiskFreeSpaceExW` | 一致 | — |
| `health.load1/5/15` | `getloadavg` | daemon 自己算：进程第一次读取时实测半秒「忙的处理器个数」（忙的比例 × 处理器数），三个平均值都从这个值起步；之后每 5 秒取一次，按 1、5、15 分钟做和 Unix 内核一样的指数平均 | 一致（G4、G35） | 满载以下和 Unix 的数同义（2.0 = 两个处理器在忙）；满载以上停在处理器数，Unix 会继续数排队的。服务器只用它在两台机器里挑闲的那台。刚启动的 daemon 和 `doctor` 报的是「现在」，不是过去 1、5、15 分钟：Windows 不记过去 |
| `health.cpuCount` / `activeRuns` | 同一段代码 | 同 | 一致 | — |
| `metrics.cpuPct` | Linux `/proc/stat`，macOS `host_statistics` | 以前没有。现在 `GetSystemTimes` | 一致（本 PR，G3） | — |
| `metrics.memTotalBytes` / `memAvailableBytes` | `/proc/meminfo`，macOS `sysctl` + `host_statistics64` | 以前没有。现在 `GlobalMemoryStatusEx` | 一致（本 PR，G3） | — |
| `metrics.uptimeSecs` | `/proc/uptime`，macOS `kern.boottime` | 以前没有。现在 `GetTickCount64` | 一致（本 PR，G3） | — |
| `metrics.netRxBps` / `netTxBps` | 只有 Linux 有（`/proc/net/dev`）；macOS 不报 | 不报 | 一致（和 Mac 一样不报） | — |
| `metrics.vmSlots` / `boot` / 每个 VM 的 CPU、内存、盘 | 只有配了 `[vm]` 的 Linux 主机 | 没有 | 无对应 | Firecracker 只在 Linux 上有，Mac 也没有；本机 run 两边都只报 run id 和开始时间 |
| `metrics.logins`（Claude Code） | Linux：`.credentials.json` 里有令牌；macOS：`.claude.json` 里有账号（凭据在钥匙串）；否则看 `ANTHROPIC_API_KEY` | 和 Linux 同一条规则（凭据文件）。这里没有改代码：读了 CLI 的 Windows 包，凭据就写在 `<配置目录>\.credentials.json`。只有用户打开 `CLAUDE_CODE_FORCE_WINDOWS_CREDMAN`（凭据进凭据管理器）时这条规则会判成未登录；那时，如果机器有 `machine.key`（功能 `accounts`），daemon 会用 `claude auth status` 的结果改判成已登录，没有 `machine.key` 就仍是未登录 | 一致（G5、G36：和 Linux 相同，包括 Linux 上没有 `machine.key` 时的同一个局限） | — |
| `metrics.logins`（Codex） | `$CODEX_HOME/auth.json` | 同 | 一致 | — |
| `metrics.secrets`、`metrics.usage` | 钥匙库、`claude -p /usage`、ChatGPT 用量接口 | 同一段代码；子进程在 job 里 | 一致（真 CLI 见 G11） | — |

## 2. `daemon.toml` 的每个键、每个子命令

配置键（`apps/daemon/src/config.rs`）。没有哪个键只在一个系统上被接受。

| 键 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- |
| `server_url`、`machine_id`、`name`、`labels`、`max_parallel_runs`、`keep_runs_hours`、`min_free_gb`、`stall_minutes`、`heartbeat_secs`、`env_passthrough`、`gated_env`、`takeover_url` | — | 同一段代码 | 一致 |
| `name` 的默认值 | `gethostname` | `COMPUTERNAME` | 一致 |
| `token_file` | 必须是 600（组和其他人不可读） | 所有者必须是当前用户（或 SYSTEM、Administrators），访问列表里每条允许项只给这三者；不满足 daemon 不启动 | 一致（见 3「机器 token」） |
| `work_root` | 默认 `~/<slug>-runs` | 默认 `%USERPROFILE%\<slug>-runs`；`~/` 和 `~\` 都展开 | 一致 |
| `repo_roots` | 路径前缀检查 | 同（`setup.rs` 现在在 CI 上跑） | 一致 |
| `[runtimes.*]`：`kind`、`command`、`args`、`env`、`model`、`models`、`efforts` | — | 同；`command` 可以是名字（按 `PATHEXT` 找）或路径 | 一致 |
| `[runtimes.*]`：`approval_policy`、`sandbox`、`permission_mode`、`allowed_tools` | 1.2.1 起忽略 | 同 | 一致 |
| `runtimes` 为空时自动加 `codex`、`claude` | 在 `PATH` 上找到就加 | 同（找 `codex.cmd` / `claude.cmd` / `.exe`） | 一致 |
| `[vm]` | 只有 Linux 接受；macOS 上是配置错误 | 配置错误（同一句话） | 无对应：Firecracker 是 Linux 的东西，Mac 一样没有 |
| 配置文件本身 | 不检查权限 | 必须只有当前用户、SYSTEM、Administrators 能打开（里面可以有 `env` 里的 key） | 一致（Windows 更严） |
| 默认位置 | `~/.config/<slug>/daemon.toml`，`CHAT_DAEMON_CONFIG` 可改 | `%USERPROFILE%\.config\<slug>\daemon.toml`，同一个变量 | 一致 |

子命令（`apps/daemon/src/main.rs`）：

| 子命令 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- |
| `run` | 收到 SIGTERM / Ctrl+C 收尾 | 收到 Ctrl+C、Ctrl+Break、控制台关闭、或停止事件（`Local\<slug>-daemon-stop-<pid>`）收尾 | 一致 |
| `run --log-file <文件>` | 没有：launchd / systemd 把输出接到日志文件 | 有：计划任务的无窗口控制台没人看得到，daemon 自己写 | 无对应（反过来的）：同一个结果，日志在文件里。Windows 启动时超过 10 MB 轮转一份；launchd 的日志从不轮转 |
| `doctor` | CLI、版本、模型、磁盘、负载、配置问题 | 同一段代码（CI 上对着真的 CLI 跑过）。多一行 `claude shell:`，写明 Claude Code 的命令工具和钩子用 Git Bash 还是 PowerShell | 一致（G27） |
| `install-service` | launchd（macOS）、systemd 用户服务（Linux） | 当前用户的计划任务；注册前先做启动时的检查、检查程序文件别人能不能换 | 一致（做法见 `windows.zh.md` M3）。不需要管理员权限：CI 上以标准用户、在它自己的登录会话里注册、由任务计划程序启动、卸载过（G45） |
| `uninstall-service` | 卸载并停止 | 结束任务、让 daemon 收尾退出、删除任务 | 一致 |
| `restart-service` | 没有：`launchctl kickstart -k`、`systemctl --user restart` 就是它 | 有：结束计划任务只结束 conhost，所以要 daemon 自己的命令 | 无对应（反过来的）：同一个结果 |
| `repair-program <路径>` | 没有：Unix 上替换程序文件是一次原子的 `rename` | 有：计划任务的第一个动作，把更新中途断电留下的 `.old-*` 改名回去 | 无对应（反过来的） |
| `private-dir <目录>` | 有，建一个 700 的目录。Mac 的客户端不用它（自己建目录、设 700） | 有：建出来的那一下就只有当前用户、SYSTEM、Administrators 的目录；已有的目录只有本来就私有、或还是空的才接受。Windows 的客户端写 token 之前用它建配置目录 | 一致（同一个子命令；用它的只有 Windows 的客户端，因为 Windows 上「先建再收紧」有空档，Unix 的 `mkdir` 加 `chmod` 对一个空目录没有） |
| `example-config` | 打印示例 | 同 | 一致 |
| 以 `chat-run` 的名字启动 | `bin/chat-run` 是符号链接 | `bin\chat-run.exe` 是硬链接，跨盘时复制；按文件名（不分大小写、去掉 `.exe`）识别 | 一致 |
| 以 `vm-agent` 的名字启动 | 只在 Linux 的 microVM 里 | 报错退出，和 macOS 一样 | 无对应 |

## 3. 代码里按平台分开的每一处

`apps/daemon/src` 和 `tools/e2e/src` 里所有 `cfg(unix)`、`cfg(windows)`、`cfg(target_os = …)`、`cfg!(…)`
（共 180 处，测试里的另列在第 5 节）。同一件事的几处并成一行。没有写 `cfg` 却只在 Unix 上成立的代码
（写死的 `/dev/…`、`/bin/…`、`ps`、`sh`）也搜了一遍：只有一处，G32。

### 进程

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- | --- |
| `executor/mod.rs`、`executor/local.rs` / `local_windows.rs`、`lib.rs`（`proctree` / `proctree_windows`）、`winjob.rs` | 启动 CLI，管它的整棵进程树 | 进程组；`ps` 找逃出进程组的子孙（按环境变量 `CHAT_RUN_FOLDER`） | Job Object：先挂起启动、进 job 再运行，job 不许脱离、句柄关闭即全杀 | 一致（CI 跑过，含要求脱离的进程、没有控制台的进程、自成进程组的进程） | — |
| 同上 | 取消 | 向进程组发 SIGTERM，宽限后 SIGKILL | `CTRL_BREAK_EVENT`，宽限后 `TerminateJobObject`；事件发不出去就直接结束 job | 一致 | — |
| 同上 | 暂停 / 继续 | SIGSTOP / SIGCONT 整棵树 | 挂起 / 恢复 job 里每个进程的每个线程，重复扫到没有新线程 | 一致（G19：负载下也测了） | — |
| `executor/mod.rs`（`kill_leftover`）、`workspace.rs`（`RunMeta.pgid`） | daemon 崩溃后留下的 CLI | 重启时按 `run.json` 里的进程组杀掉 | 不会留下：job 的唯一句柄在 daemon 手里，daemon 没了系统就结束整棵树 | 无对应：不需要，保证相同（没有 CLI 活过 daemon）。CI 上有用例 | — |
| `health.rs`（`spawn`、`output`）、`models.rs`（`codex_models`）、`accounts.rs` | 探测和登录用的子进程 | 子进程本身；Codex 的 app-server 用进程组杀 | 每个在自己的 job 里，放弃时连它启动的一起结束 | 一致 | — |
| `workspace.rs`（`git`） | 跑 `git`，600 秒超时 | 超时杀 `git` | `git` 在 job 里，超时连它启动的 `git-remote-https` 一起结束 | 一致（G20） | — |
| `runtime/pty.rs`（`ConsoleText`、`batch_line`）、`winpty.rs` | `pty` 运行时 | `openpty`、`setsid`；程序写什么，daemon 收到什么；取消是 SIGTERM 进程组 | ConPTY，同样先挂起、进 job。伪控制台发来的是「屏幕的变化」（换行可能是「光标移到第 7 行」，一串空格可能是「光标右移 12 格」），daemon 跟着光标把它还原成程序写的行。批处理文件（`.cmd`、`.bat`）可以当程序：npm 的包装照旧解析成它启动的程序，其余的经 `cmd.exe` 启动，参数按 `cmd.exe` 的规则转义（和其他运行时用的 Rust 标准库同一套规则），带换行的参数报错。取消是往控制台打 Ctrl+C，3 秒后结束 job | 一致（G44）。按行输出的程序两边得到相同的文字。剩下的一处不同做不到一样：在屏幕上「画」的程序（光标上移、覆盖已写的字），Windows 给的是它每一步画完的行，Unix 给的是同样的步骤但不带换行，两边都不是可读的文字 | CI 跑过：提示词、运行中途的输入、退出码、各行原样、批处理文件加带 `&`、`%`、引号的参数、暂停后每个线程被挂起且不再写、继续、取消后整棵树结束 |
| `executor/firecracker.rs`、`vm_agent.rs`、`executor/vmwire.rs`、`metrics.rs`（`allocated_bytes`、`process`） | 每个 run 一个 microVM | 只有 Linux | 没有 | 无对应：Mac 也没有 | — |
| `health.rs`（`which_in`、`windows_names`、`launch`、`shim_target`） | 找 CLI、启动 CLI | `PATH`、可执行位 | `PATH` + `PATHEXT`；npm 的 `.cmd` 解析出它真正启动的程序直接启动，参数不经过 `cmd.exe`；其余 `.cmd`/`.bat` 由标准库转义，带换行的参数启动前报错 | 一致（假的 npm `.cmd` CI 跑过；真的见 G11） | — |
| `sys.rs`（`OS_ENV`）、`run.rs`、`accounts.rs`（`login_env`） | 给 CLI 的环境变量 | `PATH`、`HOME` 等固定一组 | 另加 `SystemRoot`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、`TEMP`、`PATHEXT` 等 24 个系统变量 | 一致 | — |
| `accounts.rs`（`login_env`） | 登录链接不在机器上开浏览器 | `BROWSER=true`（`true` 是个什么都不做的程序） | 同一个值。CLI 的 Windows 版：设了 `BROWSER` 就只运行它指的程序，不再退回 `rundll32`。`true` 不存在就什么都没启动；装了 Git 时 `true.exe` 存在，也什么都不做。链接照样打印出来 | 一致（G14；真 CLI 在 CI 上走到「给出链接」再取消） | Codex 的 `login --device-auth` 本来就不开浏览器 |
| `main.rs`（`run`） | 停止信号 | SIGTERM、Ctrl+C | Ctrl+C、Ctrl+Break、控制台关闭、停止事件 | 一致 | — |

### 文件和权限

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- | --- |
| `sys.rs`（`mode`、`set_mode`、`protect`、`create_private_dir`、`create_private_file`、`acl_problem`、`open_to_others`、`owner`、`euid`）、`winacl.rs` | 私有的文件和目录 | 600 / 700 权限位 | 所有者 + 访问列表（当前用户、SYSTEM、Administrators），在创建的那一下就带上 | 一致 | — |
| `config.rs`（`problems`）、`accounts.rs`（`private_enough`、`write_private`） | **机器 token**、`machine.key`、钥匙库 | 明文文件，600；别人可读 daemon 不启动。不用钥匙串 | 明文文件，访问列表只给当前用户、SYSTEM、Administrators；所有者不对或多一条允许项 daemon 不启动 | 一致 | 决定：不放进 Credential Manager，也不用 DPAPI 再包一层。理由：Mac 和 Linux 的保证是「只有本用户和 root 能读」，Windows 的访问列表给的是同一个保证（本用户、SYSTEM、管理员），还多查了所有者。Credential Manager 里的凭据同一用户的任何进程都能读，不比文件多挡住谁，而从 SSH / WinRM 会话里装机器时它不可用。DPAPI 在管理员重置用户密码后会让 token 读不出来，机器就无声地掉线 |
| `workspace.rs`（`private_run_dir`） | run 目录 | `create_dir_all`，里面每个文件各自的权限位 | `<work_root>\runs` 和 run 目录创建时就私有；已有的同名目录别人进得去就拒绝 | 一致（Windows 更严） | — |
| 同上 | `work_root` 在 FAT、exFAT、网络共享上 | 不检查。在 macOS 上用 exFAT 映像试过：权限位改不了（一律 700），Unix socket 建不出来（`Operation not supported`），所以 run 没有 `chat-run`，带审批的 run 失败 | run 被拒绝，并说明要把 `work_root` 放到本机 NTFS 上 | 一致（都用不了；Windows 说得明白，Mac 是半残地跑）。不放宽 Windows：放宽等于让别的用户读到 run 里的密钥 | — |
| `workspace.rs`（`set_read_only`、`set_mode_bits`）、`winacl.rs`（`set_read_only`） | 旁聊的只读目录 | 递归去掉写权限位；用户自己能改回去，root 不受约束 | 目录访问列表上一条对当前用户的「拒绝写」，已有的和以后建的都继承；用户自己能改回去，提升权限的进程建目录、删文件时不受约束 | 一致（root ≙ 提升权限的管理员；daemon 不要提升权限运行。G18：CI 上以普通用户身份验证了改、建、删、改名全被拒绝） | — |
| `workspace.rs`（`remove_dir`） | 删除 run 目录 | 删一次 | 文件被占用时重试 3 秒，还删不掉留给下次清理 | 一致 | — |
| `workspace.rs`（`git`） | 长路径 | — | `git -c core.longpaths=true` | 一致 | — |
| `chat_run.rs`（`install_link`、`link_name`、`started_as`） | `bin/chat-run` | 指向 daemon 的符号链接 | 硬链接，跨盘复制 | 一致 | — |
| `selfupdate.rs`（`Stamp`、`clean_stale`、`repair`） | 认出程序文件被换了 | 设备号、inode、大小、修改时间、可执行位 | 卷序列号、文件索引、大小、写入时间；启动时删 `.old-*` | 一致（CI 跑过整个替换协议） | — |
| `runtime/claude.rs`（`project_dir_name_of`） | Claude Code 的会话目录名 | 路径里非字母数字的都换成 `-` | 先去掉 `\\?\` 前缀再换（`C:\Users\me\work` → `C--Users-me-work`） | 一致（G12） | Windows 上有两个候选名：按 daemon 交给 CLI 的路径算（CLI 解析链接但保留短文件名，通常就是这个），和按规范路径算；找会话和续接两个都照顾到。CLI 的规则是 `s.replace(/[^a-zA-Z0-9]/g, "-")`，按 UTF-16 单元算（daemon 原来按字符算，基本平面以外的字符差一个 `-`，两边一起改了）。超过 200 个字符时 CLI 截断并加哈希后缀，daemon 没有照做：两个系统上一样，这时退回「在所有项目目录里按会话 id 找」，续接会开新会话。run 目录的路径远短于 200 |
| `runtime/mod.rs`（`uuid_v4`） | 会话 id | 读 `/dev/urandom` | 以前读不到，id 全零。现在两边都用系统的随机数源 | 一致（G32） | — |
| `runtime/mod.rs`（`RunSpec::home`）、`config.rs`（`home`、`expand`） | 用户目录 | `HOME` | `HOME`，没有就 `USERPROFILE` | 一致 | — |
| `sys.rs`（`PATH_SEP`、`is_executable`、`host_name`、`disk`、`load_avg`、`cpu_times`） | 系统的小差别 | — | 见第 1 节 | 一致 | — |
| `tools/e2e/src/harness.rs`（`write_secret`）、`main.rs`（`home`） | 端到端工具存 owner token | 600，在 `~` 下 | 不设权限，靠用户目录本身私有（`%USERPROFILE%` 下，默认只有本用户、SYSTEM、管理员能读） | 一致（开发工具，不在机器上运行；放到别处由运行的人负责） | — |

### `chat-run`

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- | --- |
| `chat_run.rs`（`socket_path`、`Server`、`call`） | CLI 到 daemon 的通道 | Unix socket，600，另查对端 uid；路径太长时放临时目录 | 命名管道 `\\.\pipe\chat-run-<哈希>-<pid>`，访问列表只给当前用户，拒绝远程，必须是这个名字的第一个实例；客户端只许对方识别身份 | 一致（CI 跑过） | — |
| `chat_run.rs`（文件传输）、`chat_run_files.rs` | Codex 用的请求目录 | 目录 700，请求文件按属主过滤 | 目录继承 run 目录的访问列表，请求文件按所有者和访问列表过滤；读结果时遇到「正在被换上去」重试 | 一致（假 CLI CI 跑过；真 Codex 的沙箱见 G13） | — |
| `chat_run.rs` 的每个 op（`card.post`、`card.patch`、`interaction.resolve`、`artifact.*`、`desk.note`、`member.*`、`secret`、`gate.check`、文件上传、产出） | — | 同一段代码 | 同一段代码；`integration.rs` 现在在 CI 上跑（card patch、artifact create、resolve、超时两种说法） | 一致 | — |
| `gates.rs`（`enforced_by`、`reported_for`）、`run.rs` | 谁的 run 会拦命令 | Claude Code | Claude Code 2.1.322 及以上；上报和运行用同一个判断，没被上报的 CLI 收到带审批的 run 就失败 | **有条件的差别**，和第 1 节 `CliInfo.gates` 那一行是同一件事（G1、G40）：Windows 上低于 2.1.322 或读不出版本的 Claude Code 不拦命令，Unix 上每个版本都拦。2.1.322 及以上两边一致 | 见第 1 节那一行和第 6 节 |
| `gates.rs`（`plain_command`、`matches`） | 命令怎么和规则对 | 每条简单命令原样匹配 | 两边都改成：原样匹配，再按「程序名去掉目录、引号和扩展名，参数去掉引号」匹配一次 | 一致（G38；Unix 上只多拦了带引号的写法） | — |
| `gates.rs`（`claude_settings` 的 `disableAllHooks`、`SessionStart`、`alive_main`）、`runtime/claude.rs` | 钩子被设置关掉时 | 以前：照常跑，命令不被拦 | 两边都改成：run 的设置把钩子打开；启动钩子没跑过，CLI 拿不到提示词，run 失败 | 一致（G39；Unix 的变化只是堵上这个洞） | — |
| `chat_run_files.rs`（`inside`）、`chat_run_context.rs`（`resolve_out`）、`sys.rs`（`windows_path_problem`） | bot 给的路径不出 run 目录 | 解析（含符号链接）后必须在工作目录里 | 同，另外先按文本拒绝 Windows 才有的写法：网络路径、设备和 `\\?\` 路径、带冒号的（`C:foo`、`file:stream`）、以点或空格结尾的名字、设备名（`NUL`、`CON`、`COM1`…） | 一致（G33） | junction、短文件名、大小写、别的盘由「解析后比较」挡住，CI 上逐条测了 |
| `gates.rs`（`fails_closed`、`hook_failure`） | 钩子问不到 daemon | 放行（一直如此） | 拒绝 | 一致（Windows 更严，有意的） | — |
| `gates.rs`（`claude_settings`） | 钩子怎么写 | 一行 shell：`'<路径>' gate` | 不经过 shell：`command` 是程序，`args` 是 `["gate"]`，另加 `onFailure: "block"` | 一致（G1） | 见第 6 节 |
| `gates.rs`（`matcher`、`SHELL_TOOLS`） | 钩子匹配哪些工具 | `^(Bash\|PowerShell\|Monitor\|mcp__.*)$`（原来只有 `Bash` 和 MCP；这是 Mac 和 Linux 上唯一变了的地方） | 同 | 一致（G2） | — |

### 服务

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- | --- |
| `service.rs`（`install`、`install_systemd`、`install_task` 及其辅助）、`wintrust.rs` | 开机 / 登录自启、退出后重启 | launchd `KeepAlive`、systemd `Restart=always` | 计划任务：登录时启动，每分钟触发器把退出的 daemon 一分钟内拉起，单实例，无时限。`task.xml` 只在注册成功后出现，失败不留东西（G42） | 一致（CI 对着真的任务计划程序跑过；真的注销登录、重启见「要在真机上确认」） |
| `service.rs`（`single_instance`、`stoppable`、`stop_daemon`）、`main.rs` | 同一份配置只有一个 daemon | launchd、systemd 不会把一个服务起两份 | 任务计划程序会（结束任务只结束 conhost，触发器可能在旧 daemon 收尾时再起一个）。所以 daemon 自己拿一把锁：全机可见的命名互斥量，按用户和配置文件命名，进程活着就拿着；第二个 daemon 立刻退出。pid 文件只属于拿到锁的那个 | 无对应，替代做法给出同样的保证（G41；CI 上在不断触发启动的情况下重启、卸载） |
| `service.rs`（`uid`、`launchctl`） | launchd 的用户域 | `gui/<uid>` | 不用（任务属于当前用户的 SID） | 无对应：launchd 的概念 |
| `service.rs`（PATH） | 服务的 `PATH` | 安装时把当时的 `PATH` 写进 plist / unit | 不写：计划任务每次启动拿用户当前的环境变量 | 一致（Windows 的更新得更勤；常见安装目录由 daemon 自己补，G24） |
| `service.rs`（`loginctl enable-linger`） | 没人登录时 daemon 在不在 | **Linux**：`install-service` 打开 linger，开机后不用登录 daemon 就在。**Mac**：launchd 的用户代理，要登录才启动 | 要登录才启动（任务只在用户已登录的会话里跑，这样 CLI 才读得到用户的登录状态） | 和 Mac 一致；和 Linux 不同。无对应：Windows 上要「不登录就运行」得存用户密码或改用服务，服务读不到用户的 CLI 登录（`windows.zh.md` M3 的决定）。要常年在线的 Windows 机器：设成自动登录，或保持登录后锁屏 |
| `service.rs`（`log_to`、`wait_stop`、`restart_task`、`task_*`） | 日志、停止、重启 | launchd / systemd 提供 | daemon 自己做 | 一致 |

## 4. 运行时

| 运行时 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- |
| Claude Code（stream-json、插话、续接、effort、旁聊的只读模式、会话文件上传） | — | 同一段代码；假 CLI 的 `runtimes.rs` 和整个 daemon 的 `integration.rs` CI 跑过；真 CLI 见第 6 节 | 一致（假 CLI 和真 CLI 加假模型都跑过） |
| Codex（app-server、审批、插话、续接、effort、旁聊的 MCP 关闭、旁聊的只读） | — | 同一段代码；旁聊多一个参数 `windows.sandbox="unelevated"` | 一致（假 CLI；真 CLI 的不需要模型的部分跑过），**除了旁聊的联网：声明的差别**（第 0 节开头第 1 条）。旁聊写不了文件这一点一致 |
| ACP | — | 同一段代码，`runtimes.rs` CI 跑过 | 一致 |
| `pty` | `openpty` | ConPTY；输出还原成行；批处理文件可以当程序 | 一致（G44；差别只在「画屏幕」的程序，见第 3 节） |

## 5. 测试

| 测试 | Mac / Linux CI | Windows CI | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| 单元测试（`src/**`） | 跑 | 跑。不在 Windows 上跑的单元测试一共七个，分三组。（一）`chat_run.rs` 两个，各有 Windows 的对应：`a_blocked_connect_names_the_sandbox`（socket 连不上）对 `a_blocked_pipe_names_the_sandbox`（管道打不开）；`a_long_run_folder_gets_a_short_socket_path` 对 `a_long_run_folder_gets_a_short_pipe_name`。（二）`proctree.rs` 两个（整个文件是 Unix 的实现，Windows 的实现是 `proctree_windows.rs`，它没有单元测试，由 `tests/process_tree.rs` 测）：`orphans_are_found_by_the_run_marker`（脱离父进程的工具靠环境里的 run 标记找到）在 Windows 上没有同名的事，Job Object 里的进程脱离不了，对应的是 `tests/process_tree.rs::cancel_leaves_no_process_of_the_tree` 和 `a_helper_process_ends_with_everything_it_started`；`signal_skips_a_pid_that_is_another_process_now`（pid 被别的进程重用时不发信号）在 Windows 上不会发生，daemon 拿着的是 Job 和进程的句柄，不按 pid 找进程，没有对应的测试。（三）`executor/firecracker.rs` 的三个（这个文件只在 Unix 上编译，功能只有 Linux 有，Windows 无对应）：`a_bot_gets_its_own_kept_home`、`slots_get_their_own_slash_30`、`guest_env_replaces_path_and_relays_chat_run`。同属 Firecracker 的 `executor/vmwire.rs` 不分系统，它的三个测试（`frames_round_trip_and_end_cleanly`、`unknown_tags_and_huge_frames_are_refused`、`async_and_blocking_sides_agree`）在 Windows 上也跑，只是 Windows 上没有用到这段代码的功能。路径规则的三个测试原来只在 Unix 上跑，现在两边都跑（G33）。`a_folder_the_sandbox_will_not_write_names_the_sandbox`（请求目录写不进去时报成沙箱）原来只在 Unix 上跑，现在两边同一个测试：Unix 去掉目录的写权限位，Windows 加一条拒绝写的访问列表项（G47）。Windows 另有专有的：访问列表、命名管道（往返、访问列表、结束后消失）、任务 XML、命令行引号、Windows 路径写法，以及另外六个只在 Windows 上跑的：`chat_run.rs::a_request_others_could_write_is_not_taken`、`chat_run_files.rs::windows_spellings_do_not_leave_the_working_folder`、`health.rs::launch_resolves_an_npm_shim`、`sys.rs` 的 `windows_cpu_times_move_forward` 和 `the_first_windows_load_is_measured_not_zero`、`workspace.rs::a_run_folder_is_private_under_an_open_work_root` | 一致（G33、G37）；`firecracker.rs` 的三个无对应 | — |
| `tests/normalize.rs`、`redact.rs`、`chat_run.rs` | 跑 | 跑 | 一致 | — |
| `tests/process_tree.rs` | 跑 | 跑（另有 `.cmd`、job、ConPTY 的用例） | 一致 | — |
| `tests/runtimes.rs` | 跑 | 跑。`pty_prompt_input_and_exit_code` 两边是同一个测试（各用自己的 shell）。`pty_cancel_kills_group` 是 Unix 的，Windows 的对应是 `pty_pause_holds_and_cancel_ends_the_tree`（还多测了暂停）和 `pty_runs_a_batch_file`。`codex_sandbox_lets_chat_run_answer_without_network` 是 Unix 的（seatbelt / landlock 挡住 socket 和网络）；Windows 的对应是两个：`codex_on_windows_reaches_the_daemon_through_the_request_folder`（run 实际的情形，含用户自己的 Codex 配置打开了 Codex 的 Windows 沙箱）和 `codex_windows_sandbox_modes_start_nothing_or_name_the_sandbox`（手动打开 Codex 的 Windows 沙箱会怎样，见第 6 节；它不证明沙箱里的 `chat-run` 能用） | run 实际的情形一致（G8、G44）；沙箱里的 `chat-run` 在 Windows 上没有证明（G46），第 8 节第 2 条 | — |
| `tests/real_cli.rs`（真的 Claude Code，假模型） | Linux 跑（装了 CLI）；Mac 本地有 CLI 就跑 | 跑，CLI 必须在 | 一致 | — |
| `tests/real_cli_pause.rs`（真的 Claude Code 和它的工具进程，暂停、继续、取消） | Linux 跑；Mac 本地有 CLI 就跑 | 跑 | 一致（G34） | — |
| `apps/daemon/e2e/real-cli-pause.ts`（登录过的真 CLI、真模型，暂停十分钟） | 手动，Mac / Linux | 不能跑（读 Unix 的进程状态，要登录） | 无对应：它要一台登录过的机器。不用登录的那部分是上一行；带登录的在第 8 节 | — |
| `tests/integration.rs`（17 个用例：整个 daemon 对着进程内的假服务器和假 CLI） | 跑 | 以前不跑。现在跑（G6）：假 CLI 经 `fake-cli.exe` 启动；「是否停住」在 Windows 上用线程挂起计数判断；崩溃后留下 CLI 的那一段是 Unix 专有的（见第 3 节 `kill_leftover`） | 一致 | — |
| `tests/setup.rs`（协议 fixture、配置、doctor、worktree、镜像、清理、只读目录） | 跑 | 以前不跑。现在跑（本 PR，G7），除了 launchd 和 systemd 两个用例（Windows 的是 `service_task.rs`） | 一致 | — |
| `tests/service_task.rs` | 不适用 | 跑（真的任务计划程序） | 无对应（反过来的）：Mac 的 launchd 只测 plist 文本 | — |
| `scripts/test-install-daemon.ps1` | 不适用 | 跑 | 同上 | — |
| `scripts/test-daemon-standard-user.ps1`（以没有管理员权限的用户跑 daemon 的测试） | 不适用：Mac / Linux 的 CI 用户本来就不是 root | 跑，必须通过。建一个标准用户（中等完整性级别，不在 Administrators 里），以它的身份、用它自己的用户目录跑三个测试程序：`process_tree`（run、取消、暂停、ConPTY）、`integration`（整个 daemon：run 目录、命名管道、请求目录、只读目录、审批、真 CLI）、`service_task`（计划任务） | 一致（G45）。脚本先让这个用户经远程桌面登录到 runner 自己（`mstsc` 连 `127.0.0.2`，最多试三次），得到一个真的登录会话：任务带「登录时」触发器注册，由任务计划程序在这个会话里启动，其余断言照常（单实例、被杀后再起、替换协议更新、重启、更新中途断电后修复、卸载后不再起）；随后以这个用户跑安装命令的测试（`scripts/test-install-daemon.ps1`）。登录不成时脚本仍然跑完（自己按任务定义启动动作、跳过安装命令），然后**让这一步失败**并写明「没有登录会话，任务计划程序启动任务没有被证明」：这一步是绿的，就一定是真会话。没看到的只有：用户在登录界面登录那一下触发任务 | 第 8 节第 8 条 |
| `apps/daemon/e2e/local-e2e.ts`（`wrangler dev`、真 daemon、假 CLI） | Linux 跑 | 跑，而且 daemon 是装出来的计划任务（`--service`） | 一致（G9） | — |
| `tools/e2e`（客户端和服务器的场景；`runs_v11` 自己扮演一台机器，不用 daemon） | 对着已部署的环境手动跑，CI 只编译 | 同。默认的密钥目录在 `~` 下，Windows 上原来找不到用户目录（`HOME` 没设），现在用 `USERPROFILE` | 一致 | 这个工具不经过 daemon，和系统无关。经过 daemon 的端到端是上一行的 `local-e2e.ts`，它也能对着已部署的环境跑（`--base-url`） |
| 整条链（安装、任务、run、取消、两种更新、注销登录、卸载） | 没有对应的自动测试（Mac 的 launchd 只测 plist 文本） | 跑（`Daemon chain (Windows…)`） | 一致（G31；Windows 测得更多） | — |

## 6. 依赖真 CLI 的事：怎么在没有登录的 runner 上看到的

runner 能装真的 Claude Code 和 Codex（npm），不能登录。办法是把模型换成假的，CLI 用真的：

- `apps/daemon/tests/fake/model.rs` 是一个假的 Anthropic Messages 接口（`POST /v1/messages`，流式）。
  真的 `claude` 用 `ANTHROPIC_BASE_URL` 指过去、`ANTHROPIC_API_KEY` 随便填，就能不登录跑完整的一轮，
  包括调工具。脚本写在提示词里：有一行 `TOOL <工具名> <命令>`，假模型就让 CLI 调那个工具，再把工具结果
  原样说回来。daemon 和 CLI 之间的一切（启动、参数、钩子、会话文件）都是真的。
- Codex 不需要模型的部分用它自己的 `command/exec` 和 `mcp list`。
- CI 把 CLI 版本钉死（`@anthropic-ai/claude-code@2.1.322`、`@openai/codex@0.167.1`），新版本不会让不相干
  的 PR 变红；要升级时改 `ci.yml` 里的版本号，测试会说出新版本哪里变了。
- 另外读了这两个版本 Windows 包里的代码（Claude Code 的 JS 嵌在 exe 里，Codex 是开源的），下面每条
  「读代码」指这个。

| 事 | 结论 | 怎么知道的 |
| --- | --- | --- |
| 钩子用哪个 shell | 钩子写成一行命令时：找得到 Git Bash 就用 `bash -c`，否则用 PowerShell；从不用 `cmd.exe`。带引号的路径加参数（`"C:/…/chat-run.exe" gate`）在 bash 里是命令，在 PowerShell 里是语法错误，没有一种写法两边都行 | 读代码 |
| 所以 Windows 上钩子怎么写 | 不经过 shell 的写法：`command` 是程序路径，`args: ["gate"]`（CLI 直接启动它），再加 `onFailure: "block"`（钩子启动不了、超时、以意外的退出码结束，都拦下命令）。加上钩子自己「问不到 daemon 就拒绝」，没有哪种失败会放行 | 读代码；CI 上三个用例：钩子问不到 daemon → 命令被拒绝；钩子程序不存在 → 命令被拦；一切正常 → 问到 daemon、等人答 |
| 版本下限 | `args` 是 2.1.139 加的，`onFailure` 是 2.1.295 加的（CLI 的更新日志）。更旧的 CLI 遇到启动不了的钩子会放行。Windows 只对 CI 实际测的版本（2.1.322）及以上上报 `gates`；2.1.295 到 2.1.321 理论上也行，没测过就不算 | 更新日志；`gates.rs` 单元测试 |
| 钩子被设置关掉 | 用户或项目设置里的 `disableAllHooks: true` 会去掉 run 的钩子；run 的设置里写 `disableAllHooks: false` 能压过它（`--settings` 的优先级更高）。受管设置（`allowManagedHooksOnly`，或那里的 `disableAllHooks`）压不过。`SessionStart` 钩子在 CLI 启动时、读入任何输入之前就跑，钩子被关时不跑：daemon 用它判断这个 CLI 的钩子是不是开着 | 在 Mac 上对着真 CLI 试了三种组合；CI 上 Linux 和 Windows 各写入一份受管设置跑一遍 |
| 钩子的路径里有空格、中文、`&()^%!` | 照常启动：Windows 上钩子不经过 shell，路径是一个参数 | `integration.rs` 的真 CLI 用例，整个 run 目录、配置目录都放在这样命名的目录下（三个系统） |
| 真 CLI 的测试会不会悄悄跳过 | 不会：CI 装了 CLI 的任务（Linux、Windows）都设了 `TC_REQUIRE_REAL_CLI`，找不到 CLI 就失败；Windows 另设 `TC_REQUIRE_REAL_CODEX` | `ci.yml` |
| 同一套写法在 Mac 和 Linux 上 | 也成立（CLI 跑钩子的是同一段代码）：Windows 的写法在这两个系统的真 CLI 上也验证了。Mac 和 Linux 上 `PreToolUse` 钩子本身的写法没变（一行 shell、失败放行），但它们的设置变了三处，见第 3 节：匹配的工具多了 `PowerShell` 和 `Monitor`，加了 `disableAllHooks: false`，加了 `SessionStart` 启动钩子 | `real_cli.rs::the_windows_form_of_the_hook_blocks_when_it_cannot_start`（三个系统都跑） |
| 能执行命令的工具 | `Bash`（Windows 上没有 Git Bash 时没有这个工具）、`PowerShell`（Windows 上没有 Git Bash 时启用，或用 `CLAUDE_CODE_USE_POWERSHELL_TOOL=1`）、`Monitor`。三个的输入都有 `command`。钩子都匹配 | 读代码；CI 上打开 PowerShell 工具跑了一遍 |
| 默认的审批规则认不认 Windows 的写法 | 认。`gh pr merge`、`git push origin main`、`wrangler deploy` 这类在哪个 shell 里都一样，带 `.exe`、路径、引号的写法由 daemon 归一后匹配。删除一类的默认规则里有 `Remove-Item` / `ri` / `rm` / `rmdir` / `rd` / `del` / `erase` 删盘根（`C:\`、`C:\*`）和用户目录本身（`~`、`$HOME`、`$env:USERPROFILE`、`%USERPROFILE%`），以及 Git Bash 的 `rm -rf /c/`（`packages/protocol/src/settings.ts`）。和 Unix 的规则一样保守：只认删根和删家，`Remove-Item -Recurse .\build` 不拦 | `gates.rs` 单元测试；`integration.rs` 里真 CLI 逐条被拦 |
| npm 的 `claude.cmd`、`codex.cmd` | `claude.cmd` 指向包里的 `bin\claude.exe`（安装时换成真的程序），daemon 解析后直接启动 exe。`codex.cmd` 是 `node bin\codex.js`，daemon 解析成 `node.exe` 加脚本，脚本再启动包里的 `codex.exe`。两边都不经过 `cmd.exe` | 读代码；CI 断言解析结果是 `.exe`、`--version` 有输出、`doctor` 报出来 |
| 登录放在哪 | Claude Code：`<CLAUDE_CONFIG_DIR 或 %USERPROFILE%\.claude>\.credentials.json`，明文文件，和 Linux 一样。Codex：`<CODEX_HOME 或 %USERPROFILE%\.codex>\auth.json`。daemon 现有的判断就是这两个文件 | 读代码 |
| 会话文件放在哪 | `<配置目录>\projects\<工作目录，非字母数字换成 ->\<会话 id>.jsonl`。工作目录按 CLI 自己看到的算：链接解析掉，短文件名（`RUNNER~1`）原样留着 | CI：第一次跑时文件不在 daemon 算的目录里（daemon 把短文件名展开了），改完后就在 |
| Codex 的 Windows 沙箱 | Codex 自己的 Windows 沙箱默认不开（要在 Codex 配置里写 `[windows] sandbox`）。daemon 给 run 的是 `danger-full-access`，不论用户的 Codex 配置里开没开，这时都不加任何限制，`chat-run` 的请求目录照常用。手动把它打开会怎样：`unelevated` 那一档在 `workspace-write` 下拒绝启动任何命令（「cannot enforce split writable root sets directly; refusing to run unsandboxed」），一条命令也跑不起来；`elevated` 那一档在 runner 上（管理员身份，Codex 自己建了沙箱账户）时好时坏：有时能启动 `cmd.exe`，有时连它也启动不了（`CreateProcessWithLogonW failed: 1326`、`267`）；启动 run 目录里的 `chat-run.exe` 时被拒绝（`CreateProcessAsUserW failed: 5`；最可能是沙箱账户读不了只对用户自己开放的目录，没有再查）。两档都没能让 `chat-run` 在沙箱里跑起来，所以这里没有「沙箱里的 `chat-run` 能用」的证明；daemon 的 run 不用这个沙箱，不受影响。顺带看到：`elevated` 那一档会把它的沙箱账户的写权限加到用户的临时目录上，daemon 的程序和 run 目录不在那里 | 读代码；CI：真 Codex 的 `command/exec` 启动真 `chat-run`，请求到达 daemon，回答送回（用户配置开和不开沙箱各一遍）；手动开沙箱的两档，每次调用的回答原文在 CI 的日志里（`Daemon tests (Windows)` 的最后一步；`elevated` 单独放在最后跑，因为装它会改这台电脑） |
| Codex 的旁聊（`read-only`） | **写文件：一致。联网：声明的差别。** Mac / Linux：命令写不了文件（工作目录里外都一样），Codex 的沙箱同时挡住联网。Windows：同样写不了文件、读得到文件，靠的是 daemon 给旁聊打开的 Codex 沙箱 `unelevated` 档（G52）。联网在这一档只是环境变量：`HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`GIT_HTTP(S)_PROXY` 被指到 `127.0.0.1:9`，读这些设置的程序连不出去，`curl --noproxy *` 这样不理会它们的程序连得上本机地址，也连得上外网。为什么补不上，三条路都试过或查过：（一）Codex 的 `elevated` 档用防火墙规则挡住它自己的沙箱账户，要管理员安装一次，标准用户用不了；装好之后它的沙箱账户进不了 run 目录（run 目录只对 daemon 的用户开放，每条命令都以 `CreateProcessWithLogonW failed: 267` 失败），要用就得把旁聊的工作目录和它指向的仓库都开放给这两个账户，等于在放 token 和代码的私有目录上开口子；即使开了，它也只挡外网，本机地址照样连得上。（二）AppContainer：不带网络能力的 AppContainer 不需要管理员，但命令是 Codex 自己启动的，daemon 只能把整个 CLI 放进去，而 CLI 要连模型，断了网就不能用。（三）按进程的防火墙规则或 Windows Filtering Platform 过滤器都要提升权限。所以没有管理员权限就挡不住别的程序启动的进程联网。旁聊本来就读得到工作目录里的全部内容，多出来的风险是命令能把读到的东西发出去 | CI：`runtimes.rs::codex_read_only_sandbox_writes_nothing`（三个系统都跑）；`codex_side_chat_network_on_windows`（Windows，断言见 G52；`elevated` 档的三种情形在 `Daemon tests (Windows)` 最后一步的日志里）；`real_cli.rs` 断言 `doctor` 有那一行 |
| 登录时的浏览器 | 见第 3 节 `login_env` 一行 | 读代码；CI |
| Git for Windows | 不需要。没有它时 Claude Code 用 PowerShell 当命令工具（2.1 起） | 更新日志；读代码 |

## 7. 「用这台电脑」和安装命令

| 事 | Mac | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| daemon 在哪 | 应用包里 `Contents/MacOS/<slug>-daemon` | 安装目录里客户端旁边的 `<slug>-daemon.exe`；debug 构建和 `target` 目录下一律当作没有 | 一致 | — |
| 写配置和 token | 目录 700，文件 600，先写临时文件再改名 | 目录由 daemon 建（`<daemon> private-dir`），建出来的那一下就只有当前用户、SYSTEM、Administrators，不是先建再收紧；已有的目录只有本来就私有、或者还是空的才用。再写文件 | 一致 | — |
| 注册服务 | `<daemon> install-service`，把登录 shell 的 `PATH` 加常见安装目录传给它 | `<daemon> install-service`，不传 `PATH`。daemon 每次启动自己把存在的常见目录补到 `PATH` 后面：`%USERPROFILE%\.local\bin`（Claude Code 的安装程序把 `claude.exe` 放在这里，不一定加进 `PATH`）、`.claude\local`、`%APPDATA%\npm`、`.bun\bin`、`.cargo\bin`、`nodejs`、Git | 一致（G24） | — |
| 状态 | 读 launchd 的 plist（plist 在就是已注册） | 先问任务计划程序任务在不在（`schtasks /Query` 的退出码，不看它的文字），再读 `task.xml` 看任务跑的是哪个 daemon；有文件没任务是「未设置」，有任务没文件是「别处」 | 一致（G42） | — |
| 重启 | `launchctl kickstart -k` | `<daemon> restart-service`：让 daemon 收尾退出，再启动任务。期间触发器起的第二个 daemon 拿不到锁，立刻退出（G41） | 一致 | — |
| 机器名、标签 | `scutil --get ComputerName`、`mac` | `COMPUTERNAME`、`windows` | 一致 | — |
| 实际运行过 | 是 | 是：CI 的 `Desktop build and tests (Windows)` 里，设置页调用的那段代码（`this_mac::set_up`）对着本地服务器（`wrangler dev`）跑：按安装目录找到 daemon，以 owner 身份向服务器注册这台电脑，写配置和 token，真 daemon 注册任务并在服务器上显示在线；再点一次不会重复注册；重启后换了一个进程、重新在线；日志里没有 token；卸载后任务、进程都没了，服务器上离线 | 一致（G10） | 没覆盖的只有 `current_exe()` 这一步（测试进程不是安装好的客户端）和设置页的按钮本身 |
| 安装命令 | 没有一条命令。`docs/bots.md` 第 6 节是手动步骤：编译或下载 daemon、用 `admin.ts machine` 登记机器、把程序和 token 拷过去、手写 `daemon.toml`、跑 `doctor` 和 `install-service`。（另一条路是装客户端点「用这台电脑」，两边都有） | 有一条：`scripts/install-daemon.ps1`（先登记机器）：https 下载、必填 SHA-256、token 不上命令行、私有目录、写配置、`doctor`、`install-service`，`-Update` 按替换协议换程序 | 不一致，Windows 多：Unix 没有一行安装。结果相同（机器装好、服务注册），Unix 那边步骤多。给 Unix 也写一条安装脚本不在这次范围里 | CI 跑过（Windows PowerShell 5.1）：以 runner 的管理员跑一遍，以标准用户在它自己的登录会话里再跑一遍。设置页「添加机器」里给出 Windows 命令归客户端那条线 |

## 8. 要在真机上确认

CI 做不到的只有这些。每条都是可以照着做的步骤和应该看到的结果。准备：一台 Windows 11 电脑（第 9 条另要
Windows 10），装了客户端并用 owner 登录，装了 Claude Code 和 Codex。

1. **「用这台电脑」，然后一次已登录的 Claude Code run，带一条审批。**
   1. 终端里运行 `claude`，按提示登录一次，退出。
   2. 客户端：设置 › 机器 › 「用这台电脑」。应该看到：几秒后这台电脑出现在机器列表里，状态在线。
   3. 团队设置里勾上「合并到 main」这一类审批；把一个 bot 的运行位置设成这台机器（运行时 Claude Code）。
   4. 在群里 @ 这个 bot：「在一个临时目录里 `git init`，然后运行 `git push origin main`」。
   5. 应该看到：话题里出现审批卡片，写着那条命令，run 停着等。点「拒绝」：bot 回复说没有执行。
   6. 再发一次同样的话，点「同意」。应该看到：命令执行（会因为没有远端而报错，这是对的），bot 回复结果。
   7. 这一条同时确认：计划任务里的 daemon 启动的 CLI 读得到用户的登录；真模型下的工具调用和 CI 上假模型
      下一样。
2. **一次已登录的 Codex run。**
   1. 终端里运行 `codex login`，登录。
   2. 把一个 bot 的运行时设成 Codex，位置是这台机器。在群里 @ 它：「在当前目录建一个文件 hello.txt，内容是
      hi，然后用一张卡片告诉我做完了」。
   3. 应该看到：bot 发出一张卡片（这一步走的是 `chat-run` 的请求目录），run 结束，状态是完成。
   4. 资源管理器打开 `%USERPROFILE%\<slug>-runs\runs`，最新的目录里 `work\hello.txt` 存在，
      `work` 下没有留下 `.chat-run-` 开头的目录。
   5. 打开了 Codex 自己的 Windows 沙箱的电脑（CI 上没能证明的一种）：在 `%USERPROFILE%\.codex\config.toml` 里加两行
      `[windows]` 和 `sandbox = "elevated"`，在终端里运行一次 `codex`，按它的提示装好沙箱（要管理员确认），
      然后把第 2 步再做一遍。
   6. 应该看到：和第 3、4 步一样，卡片发出、run 完成（daemon 给 run 的设置盖过这一项，run 不在沙箱里）。
3. **暂停和继续一个已登录的 run。**
   1. @ 第 1 条的 bot：「运行这条命令并等它结束：`for i in $(seq 1 120); do date +%s >> ticks.txt; sleep 1; done`」
      （没装 Git for Windows 的电脑换成 `1..120 | % { Get-Date -UFormat %s >> ticks.txt; Start-Sleep 1 }`）。
   2. run 开始十秒后，在话题里点「接管」（暂停）。
   3. 打开 run 目录下的 `work\ticks.txt`，等十秒再打开一次。应该看到：行数没有变。任务管理器 › 详细信息里
      `claude.exe` 的状态是「已挂起」。话题不显示「卡住」。
   4. 等满六分钟（超过 `stall_minutes`），应该仍然不显示「卡住」。点「交还」。应该看到：`ticks.txt` 接着涨，
      run 正常结束。
4. **取消一个真的 run。**
   1. 再发一次第 3 条的命令，run 开始十秒后点「停止」。
   2. 应该看到：run 几秒内结束，状态是已停止；任务管理器里没有留下这次 run 的 `claude.exe`、`node.exe`、
      `bash.exe`、`sleep.exe`；`ticks.txt` 不再涨。
5. **睡眠和唤醒，机器空闲时。**
   1. 确认机器在线，没有 run。合上盖子（或开始菜单 › 睡眠），等两分钟，唤醒并登录。
   2. 应该看到：另一台设备上的机器页里，这台机器在唤醒后十秒内回到在线。
   3. `%LOCALAPPDATA%\<slug>-daemon\daemon.log` 末尾有一行 `the machine was asleep for … s: connecting again`。
6. **睡眠和唤醒，run 进行中。**
   1. 发第 3 条的命令，run 开始十秒后让电脑睡眠，等六分钟（超过 `stall_minutes`），唤醒并登录。
   2. 应该看到：机器十秒内回到在线；这个 run 还在进行，没有被标成「卡住」，也没有失败；`ticks.txt` 接着涨
      （中间缺了睡着的那几分钟），最后 run 正常结束。
   3. 时长上限（每个 run 的上限是服务器定的两小时，没有设置项；这一步要两个多小时，可以放着过夜）：把第 3 条
      命令里的 `120` 换成 `600` 再发一次，run 开始十秒后让电脑睡眠，两小时十分钟后唤醒并登录。
   4. 应该看到：run 没有因为超时被停，状态仍是进行中；`ticks.txt` 接着涨到 600 行，run 正常结束。
      （睡着的时间不算进上限。CI 上这一条是单元测试和「冻住 daemon 进程」的测试，没有真的睡眠。）
7. **一次真的重启。**
   1. 重启电脑，登录，不打开客户端。
   2. 应该看到：一分钟内，另一台设备的机器页里这台机器在线。
   3. 再 @ 一次 bot（随便一句话），应该正常回复。（CI 上验证的是任务被结束后再启动，和每分钟触发器把它
      拉起；没有真的重启。）
8. **标准（非管理员）账户。**
   1. 设置 › 账户 › 其他用户，新建一个本地账户，类型是「标准用户」。用它登录 Windows。
   2. 装客户端（装到这个用户自己的目录，不会要管理员密码），登录团队，点「用这台电脑」。
   3. 应该看到：不弹 UAC，机器出现并在线。重复第 1 条的第 3 到 6 步，结果相同。
   4. 注销再登录这个账户，不打开客户端。应该看到：一分钟内机器在团队里重新在线（任务在「登录时」被触发）。
   5. （CI 上标准用户经远程桌面登录后跑过 daemon 的测试、安装命令，任务是任务计划程序启动的；没有跑过的是
      在登录界面登录触发任务，和客户端的「用这台电脑」按钮在标准账户下。）
9. **Windows 10 22H2。** 在一台 Windows 10 电脑上做第 1、3、4 条，结果相同。（CI 是 Windows Server 2025；
   `conhost.exe --headless` 和 ConPTY 在 Windows 10 上的表现没见过。）
10. **没装 Git for Windows 的电脑。** 做第 1、3、4 条（第 3 条用 PowerShell 的那条命令），结果相同。区别是
    Claude Code 用 PowerShell 工具。（CI 的 runner 装了 Git，PowerShell 工具是用开关打开来测的。）
11. **公司管理的电脑。** 做第 1 条的第 2 步。应该看到：「用这台电脑」成功（组策略允许普通用户注册计划任务），
    杀毒软件不拦没签名的 daemon。不成功时把 `%LOCALAPPDATA%\<slug>-daemon\daemon.log` 和客户端的日志发回来。
12. **对着已部署的环境跑一遍端到端**（可选，需要这台电脑上有 bun、python 和仓库）：
    1. `bun apps/daemon/e2e/local-e2e.ts --base-url <e2e 环境的地址> --owner-token-file <owner token 文件>`。
    2. 应该看到：每一步前面一个 `-`，最后一行是 `report:`，退出码 0。它自己建群、成员、bot 和机器，跑完删掉。
