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
代码只在那里运行）。下文「CI 跑过」都指后者。

## 0. 缺口和它们的结局

清单第一版列出 20 个缺口，真 CLI 的测试在 Windows 上又抓出一个（G32）。现在全部补完；剩下的只有 CI
做不到的事，在第 8 节。

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
| `CliInfo.gates` | Claude Code：`true` | Claude Code 2.1.295 及以上：`true`。更旧的、或者读不出版本的：不上报，服务器把规则留在提示词里（和对 Codex 一样） | 一致（G1） | 版本下限的原因见第 6 节 |
| `health.diskFreeBytes` / `diskTotalBytes` | `statvfs` | `GetDiskFreeSpaceExW` | 一致 | — |
| `health.load1/5/15` | `getloadavg` | 以前是 0。现在 daemon 自己算：每 5 秒取一次「忙的处理器个数」（忙的比例 × 处理器数），按 1、5、15 分钟做和 Unix 内核一样的指数平均 | 一致（本 PR，G4） | 满载以下和 Unix 的数同义（2.0 = 两个处理器在忙）；满载以上停在处理器数，Unix 会继续数排队的。服务器只用它在两台机器里挑闲的那台。启动后头 5 秒是 0 |
| `health.cpuCount` / `activeRuns` | 同一段代码 | 同 | 一致 | — |
| `metrics.cpuPct` | Linux `/proc/stat`，macOS `host_statistics` | 以前没有。现在 `GetSystemTimes` | 一致（本 PR，G3） | — |
| `metrics.memTotalBytes` / `memAvailableBytes` | `/proc/meminfo`，macOS `sysctl` + `host_statistics64` | 以前没有。现在 `GlobalMemoryStatusEx` | 一致（本 PR，G3） | — |
| `metrics.uptimeSecs` | `/proc/uptime`，macOS `kern.boottime` | 以前没有。现在 `GetTickCount64` | 一致（本 PR，G3） | — |
| `metrics.netRxBps` / `netTxBps` | 只有 Linux 有（`/proc/net/dev`）；macOS 不报 | 不报 | 一致（和 Mac 一样不报） | — |
| `metrics.vmSlots` / `boot` / 每个 VM 的 CPU、内存、盘 | 只有配了 `[vm]` 的 Linux 主机 | 没有 | 无对应 | Firecracker 只在 Linux 上有，Mac 也没有；本机 run 两边都只报 run id 和开始时间 |
| `metrics.logins`（Claude Code） | Linux：`.credentials.json` 里有令牌；macOS：`.claude.json` 里有账号（凭据在钥匙串）；否则看 `ANTHROPIC_API_KEY` | 和 Linux 同一条规则（凭据文件）；另外 CLI 自己说已登录时（`claude auth status`），不论系统都算已登录（`daemon.rs` 里补的那一步） | 一致（G5：CLI 的 Windows 版把凭据写在 `<配置目录>\.credentials.json`，只有打开 `CLAUDE_CODE_FORCE_WINDOWS_CREDMAN` 才用凭据管理器；那种情况由 `auth status` 那一步兜住） | — |
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
| `install-service` | launchd（macOS）、systemd 用户服务（Linux） | 当前用户的计划任务；注册前先做启动时的检查、检查程序文件别人能不能换 | 一致（做法见 `windows.zh.md` M3） |
| `uninstall-service` | 卸载并停止 | 结束任务、让 daemon 收尾退出、删除任务 | 一致 |
| `restart-service` | 没有：`launchctl kickstart -k`、`systemctl --user restart` 就是它 | 有：结束计划任务只结束 conhost，所以要 daemon 自己的命令 | 无对应（反过来的）：同一个结果 |
| `repair-program <路径>` | 没有：Unix 上替换程序文件是一次原子的 `rename` | 有：计划任务的第一个动作，把更新中途断电留下的 `.old-*` 改名回去 | 无对应（反过来的） |
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
| `runtime/pty.rs`、`winpty.rs` | `pty` 运行时 | `openpty`、`setsid`；取消是 SIGTERM 进程组 | ConPTY，同样先挂起、进 job；取消是往控制台打 Ctrl+C，3 秒后结束 job | 一致（CI 跑过：输出、输入、退出码、暂停、取消）。输出是「画屏幕」，见 `windows.zh.md` M2 | — |
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
| `gates.rs`（`enforced_by`、`reported_for`） | 谁的 run 会拦命令 | Claude Code | Claude Code 2.1.295 及以上 | 一致（G1） | 见第 6 节 |
| `gates.rs`（`fails_closed`、`hook_failure`） | 钩子问不到 daemon | 放行（一直如此） | 拒绝 | 一致（Windows 更严，有意的） | — |
| `gates.rs`（`claude_settings`） | 钩子怎么写 | 一行 shell：`'<路径>' gate` | 不经过 shell：`command` 是程序，`args` 是 `["gate"]`，另加 `onFailure: "block"` | 一致（G1） | 见第 6 节 |
| `gates.rs`（`matcher`、`SHELL_TOOLS`） | 钩子匹配哪些工具 | `^(Bash\|PowerShell\|Monitor\|mcp__.*)$`（原来只有 `Bash` 和 MCP；这是 Mac 和 Linux 上唯一变了的地方） | 同 | 一致（G2） | — |

### 服务

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- | --- |
| `service.rs`（`install`、`install_systemd`、`install_task` 及其辅助）、`wintrust.rs` | 开机 / 登录自启、退出后重启 | launchd `KeepAlive`、systemd `Restart=always` | 计划任务：登录时启动，每分钟触发器把退出的 daemon 一分钟内拉起，单实例，无时限 | 一致（CI 对着真的任务计划程序跑过；真的注销登录、重启见「要在真机上确认」） |
| `service.rs`（`uid`、`launchctl`） | launchd 的用户域 | `gui/<uid>` | 不用（任务属于当前用户的 SID） | 无对应：launchd 的概念 |
| `service.rs`（PATH） | 服务的 `PATH` | 安装时把当时的 `PATH` 写进 plist / unit | 不写：计划任务每次启动拿用户当前的环境变量 | 一致（Windows 的更新得更勤；常见安装目录由 daemon 自己补，G24） |
| `service.rs`（`log_to`、`wait_stop`、`restart_task`、`task_*`） | 日志、停止、重启 | launchd / systemd 提供 | daemon 自己做 | 一致 |

## 4. 运行时

| 运行时 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- |
| Claude Code（stream-json、插话、续接、effort、旁聊的只读模式、会话文件上传） | — | 同一段代码；假 CLI 的 `runtimes.rs` 和整个 daemon 的 `integration.rs` CI 跑过；真 CLI 见第 6 节 | 一致（假 CLI 和真 CLI 加假模型都跑过） |
| Codex（app-server、审批、插话、续接、effort、旁聊的 MCP 关闭） | — | 同上 | 一致（假 CLI；真 CLI 的不需要模型的部分跑过） |
| ACP | — | 同一段代码，`runtimes.rs` CI 跑过 | 一致 |
| `pty` | `openpty` | ConPTY；批处理文件不能当程序（明确报错） | 一致 |

## 5. 测试

| 测试 | Mac / Linux CI | Windows CI | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| 单元测试（`src/**`） | 跑 | 跑（另有 Windows 专有的：访问列表、命名管道、任务 XML、命令行引号） | 一致 | — |
| `tests/normalize.rs`、`redact.rs`、`chat_run.rs` | 跑 | 跑 | 一致 | — |
| `tests/process_tree.rs` | 跑 | 跑（另有 `.cmd`、job、ConPTY 的用例） | 一致 | — |
| `tests/runtimes.rs` | 跑 | 跑，除了两个 `pty` 用例（Windows 的在 `process_tree.rs` 里）和 Unix 沙箱那一个（它测的是 seatbelt / landlock 挡住 socket 和网络；Windows 上对应的是 `codex_on_windows_reaches_the_daemon_through_the_request_folder`） | 一致（G8） | — |
| `tests/real_cli.rs`（真的 Claude Code，假模型） | Linux 跑（装了 CLI）；Mac 本地有 CLI 就跑 | 跑，CLI 必须在 | 一致 | — |
| `tests/integration.rs`（13 个用例：整个 daemon 对着进程内的假服务器和假 CLI） | 跑 | 以前不跑。现在跑（本 PR，G6）：假 CLI 经 `fake-cli.exe` 启动；「是否停住」在 Windows 上用线程挂起计数判断；崩溃后留下 CLI 的那一段是 Unix 专有的（见第 3 节 `kill_leftover`） | 一致 | — |
| `tests/setup.rs`（协议 fixture、配置、doctor、worktree、镜像、清理、只读目录） | 跑 | 以前不跑。现在跑（本 PR，G7），除了 launchd 和 systemd 两个用例（Windows 的是 `service_task.rs`） | 一致 | — |
| `tests/service_task.rs` | 不适用 | 跑（真的任务计划程序） | 无对应（反过来的）：Mac 的 launchd 只测 plist 文本 | — |
| `scripts/test-install-daemon.ps1` | 不适用 | 跑 | 同上 | — |
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
| 版本下限 | `args` 是 2.1.139 加的，`onFailure` 是 2.1.295 加的（CLI 的更新日志）。更旧的 CLI 遇到启动不了的钩子会放行，所以 Windows 只对 2.1.295 及以上上报 `gates` | 更新日志；`gates.rs` 单元测试 |
| 同一套写法在 Mac 和 Linux 上 | 也成立（CLI 跑钩子的是同一段代码）。Mac 和 Linux 的钩子没有改，仍是一行 shell、失败放行；但 Windows 的写法在这两个系统的真 CLI 上也验证了 | `real_cli.rs::the_windows_form_of_the_hook_blocks_when_it_cannot_start`（三个系统都跑） |
| 能执行命令的工具 | `Bash`（Windows 上没有 Git Bash 时没有这个工具）、`PowerShell`（Windows 上没有 Git Bash 时启用，或用 `CLAUDE_CODE_USE_POWERSHELL_TOOL=1`）、`Monitor`。三个的输入都有 `command`。钩子都匹配 | 读代码；CI 上打开 PowerShell 工具跑了一遍 |
| 默认的审批规则认不认 PowerShell 的写法 | `gh pr merge`、`git push origin main`、`wrangler deploy` 这类在哪个 shell 里都一样。`rm -rf /` 这类删除规则是按 POSIX 写的，`Remove-Item -Recurse` 不在默认规则里 | 这是服务器的默认规则（`packages/protocol`），不归 daemon；已告知负责服务器的人。装了 Git for Windows 的机器走 Bash，不受影响 |
| npm 的 `claude.cmd`、`codex.cmd` | `claude.cmd` 指向包里的 `bin\claude.exe`（安装时换成真的程序），daemon 解析后直接启动 exe。`codex.cmd` 是 `node bin\codex.js`，daemon 解析成 `node.exe` 加脚本，脚本再启动包里的 `codex.exe`。两边都不经过 `cmd.exe` | 读代码；CI 断言解析结果是 `.exe`、`--version` 有输出、`doctor` 报出来 |
| 登录放在哪 | Claude Code：`<CLAUDE_CONFIG_DIR 或 %USERPROFILE%\.claude>\.credentials.json`，明文文件，和 Linux 一样。Codex：`<CODEX_HOME 或 %USERPROFILE%\.codex>\auth.json`。daemon 现有的判断就是这两个文件 | 读代码 |
| 会话文件放在哪 | `<配置目录>\projects\<工作目录，非字母数字换成 ->\<会话 id>.jsonl`。工作目录按 CLI 自己看到的算：链接解析掉，短文件名（`RUNNER~1`）原样留着 | CI：第一次跑时文件不在 daemon 算的目录里（daemon 把短文件名展开了），改完后就在 |
| Codex 的 Windows 沙箱 | 默认没有（要在 Codex 配置里写 `[windows] sandbox` 才有）。daemon 给 run 的是 `danger-full-access`，这时不加任何限制，`chat-run` 的请求目录照常用 | 读代码；CI：真 Codex 的 `command/exec` 启动真 `chat-run`，请求到达 daemon，回答送回 |
| Codex 的旁聊（`read-only`） | 命令写不了文件，工作目录里外都一样 | CI：`runtimes.rs::codex_read_only_sandbox_writes_nothing`（三个系统都跑） |
| 登录时的浏览器 | 见第 3 节 `login_env` 一行 | 读代码；CI |
| Git for Windows | 不需要。没有它时 Claude Code 用 PowerShell 当命令工具（2.1 起） | 更新日志；读代码 |

## 7. 「用这台电脑」和安装命令

| 事 | Mac | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| daemon 在哪 | 应用包里 `Contents/MacOS/<slug>-daemon` | 安装目录里客户端旁边的 `<slug>-daemon.exe`；debug 构建和 `target` 目录下一律当作没有 | 一致 | — |
| 写配置和 token | 目录 700，文件 600，先写临时文件再改名 | 目录先设成只有当前用户、SYSTEM、Administrators（`icacls`），再写文件 | 一致 | — |
| 注册服务 | `<daemon> install-service`，把登录 shell 的 `PATH` 加常见安装目录传给它 | `<daemon> install-service`，不传 `PATH`。daemon 每次启动自己把存在的常见目录补到 `PATH` 后面：`%USERPROFILE%\.local\bin`（Claude Code 的安装程序把 `claude.exe` 放在这里，不一定加进 `PATH`）、`.claude\local`、`%APPDATA%\npm`、`.bun\bin`、`.cargo\bin`、`nodejs`、Git | 一致（G24） | — |
| 状态 | 读 launchd 的 plist | 读 `task.xml`（UTF-16） | 一致 | — |
| 重启 | `launchctl kickstart -k` | `<daemon> restart-service` | 一致 | — |
| 机器名、标签 | `scutil --get ComputerName`、`mac` | `COMPUTERNAME`、`windows` | 一致 | — |
| 实际运行过 | 是 | 是：CI 的 `Desktop build and tests (Windows)` 里，客户端的代码写配置和 token（目录只有当前用户、SYSTEM、Administrators），刚编译的真 daemon 注册任务，状态读成「正常」，daemon 在跑、写了日志，重启换了一个进程，卸载后什么都不剩 | 一致（G10） | — |
| 安装命令 | `curl … \| bash`（`docs/bots.md` 第 6 节） | `scripts/install-daemon.ps1`：https、必填 SHA-256、token 不上命令行、私有目录、`doctor`、`install-service`、`-Update` | 一致（CI 跑过，Windows PowerShell 5.1） | 设置页「添加机器」里给出 Windows 命令归客户端那条线 |

## 8. 要在真机上确认

CI 做不到的只有这些。每条两三分钟。

1. **一次真的、已登录的 Claude Code run，带一条审批。**
   1. 在 Windows 电脑上装客户端，登录，设置 › 机器 › 「用这台电脑」。
   2. 在这台电脑上打开终端运行 `claude`，按提示登录一次（或者在客户端的机器页里点登录）。
   3. 团队设置里勾上「合并到 main」这一类审批。
   4. 在群里 @ 一个跑在这台机器上的 bot：「在一个临时仓库里运行 `git push origin main`」。
   5. 应该看到：话题里出现审批卡片，命令停着；点拒绝，bot 说明没有执行；再来一次点同意，命令执行。
   6. 这一条同时确认了：计划任务里的 daemon 启动的 CLI 读得到用户的登录；真模型下工具调用和 CI 上假模型
      下一样。
2. **一次真的重启。** 重启电脑，登录，不打开客户端。一分钟内在另一台设备的机器页里看到这台机器在线。
   （CI 上验证的是任务被结束后再启动，和每分钟触发器把它拉起；没有真的重启。）
3. **取消一个真的 run。** bot 干活时点「停止」。应该看到：run 结束，任务管理器里没有留下 `claude.exe`、
   `node.exe`、`bash.exe`。（CI 上假 CLI 的整棵树都验证了；真 CLI 在无窗口控制台里收到 Ctrl+Break 后的
   收尾没见过，收不了尾也会在几秒后被整棵结束。）
4. **没装 Git for Windows 的电脑。** 同第 1 条。区别是 Claude Code 用 PowerShell 工具。（CI 的 runner 装了
   Git，PowerShell 工具是用开关打开来测的。）
5. **Windows 10 22H2。** 装一遍，跑第 1 条。（CI 是 Windows Server 2025；`conhost.exe --headless` 和 ConPTY
   在 Windows 10 上的表现没见过。）
6. **公司管理的电脑。** 「用这台电脑」能成功（组策略允许普通用户注册计划任务），杀毒软件不拦没签名的
   daemon。
7. **对着已部署的环境跑一遍端到端**（可选，需要这台电脑上有 bun、python 和仓库）：
   `bun apps/daemon/e2e/local-e2e.ts --base-url <e2e 环境的地址> --owner-token-file <owner token 文件>`。
   它自己建群、成员、bot 和机器，跑完删掉。
