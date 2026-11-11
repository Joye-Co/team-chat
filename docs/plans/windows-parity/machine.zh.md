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

## 0. 还没补完的缺口

| 编号 | 缺口 | 在哪一节 |
| --- | --- | --- |
| G1 | Windows 的 Claude Code 不上报 `CliInfo.gates`，带审批的 run 在 Windows 上不拦命令 | 1、6 |
| G2 | 审批钩子的匹配只认 `Bash` 和 MCP 工具；Windows 版 Claude Code 如果有别的能执行命令的工具（PowerShell），会漏 | 6 |
| G8 | `tests/runtimes.rs` 里两个真 Codex 的用例（MCP 覆盖、沙箱里的 `chat-run`）只在 Unix 上跑 | 5、6 |
| G9 | `apps/daemon/e2e/local-e2e.ts`（`wrangler dev`、真 daemon、假 CLI）只在 Linux CI 上跑；`tools/e2e` 的机器用例没对着 Windows 机器跑过 | 5 |
| G10 | 「用这台电脑」在 Windows 上只编译过，没运行过 | 7 |
| G11 | 真的 `claude`、`codex`（npm 装的）在 Windows 上：能不能按名字找到、`.cmd` 解析到哪、`--version`、`doctor` 输出，没见过 | 6 |
| G12 | Claude Code 在 Windows 上的会话目录名规则，只按推断写了，没对着真的 CLI 核对 | 6 |
| G13 | Codex 的 Windows 沙箱下，`chat-run` 能不能写请求目录，没见过 | 6 |
| G14 | 登录时 `BROWSER=true`：Windows 上没有 `true` 这个程序，CLI 会不会改开本机浏览器，没见过 | 6 |
| G18 | 旁聊的只读目录：普通用户身份下「删除」「建目录」被拒绝没有测试（CI 的用户是提升权限的管理员） | 3、5 |
| G19 | 暂停一棵正在不断启动新进程的树，没有测试 | 5 |
| G20 | `git` 超时后只结束 `git.exe`，它启动的子进程（`git-remote-https`）留着 | 3 |
| G24 | 没在 `PATH` 上的常见安装目录（`%USERPROFILE%\.local\bin` 等）：Mac 的「用这台电脑」会把这类目录加进服务的 `PATH`，Windows 没有 | 7 |
| G27 | `doctor` 不检查 Claude Code 在 Windows 上需要的 Git for Windows（bash） | 2、6 |
| G31 | 整条链没在 runner 上连起来跑过：PowerShell 安装 → 连本地服务器 → 经服务器发起 run → 取消 → 空闲时和 run 进行中各更新一次 → 注销再登录 | 5 |

本清单所在的 PR 已补的：G3、G4（主机指标和负载）、G5（登录状态判断，见 1）、G6、G7（`integration.rs`、
`setup.rs` 在 Windows 上跑）。

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
| `CliInfo.gates` | Claude Code：`true` | **不上报**（`gates::enforced_by` 是 `cfg!(unix)`） | **缺口 G1** | 在 runner 上用真的 Claude Code 证明钩子会被启动、问不到 daemon 时拒绝，然后打开 |
| `health.diskFreeBytes` / `diskTotalBytes` | `statvfs` | `GetDiskFreeSpaceExW` | 一致 | — |
| `health.load1/5/15` | `getloadavg` | 以前是 0。现在 daemon 自己算：每 5 秒取一次「忙的处理器个数」（忙的比例 × 处理器数），按 1、5、15 分钟做和 Unix 内核一样的指数平均 | 一致（本 PR，G4） | 满载以下和 Unix 的数同义（2.0 = 两个处理器在忙）；满载以上停在处理器数，Unix 会继续数排队的。服务器只用它在两台机器里挑闲的那台。启动后头 5 秒是 0 |
| `health.cpuCount` / `activeRuns` | 同一段代码 | 同 | 一致 | — |
| `metrics.cpuPct` | Linux `/proc/stat`，macOS `host_statistics` | 以前没有。现在 `GetSystemTimes` | 一致（本 PR，G3） | — |
| `metrics.memTotalBytes` / `memAvailableBytes` | `/proc/meminfo`，macOS `sysctl` + `host_statistics64` | 以前没有。现在 `GlobalMemoryStatusEx` | 一致（本 PR，G3） | — |
| `metrics.uptimeSecs` | `/proc/uptime`，macOS `kern.boottime` | 以前没有。现在 `GetTickCount64` | 一致（本 PR，G3） | — |
| `metrics.netRxBps` / `netTxBps` | 只有 Linux 有（`/proc/net/dev`）；macOS 不报 | 不报 | 一致（和 Mac 一样不报） | — |
| `metrics.vmSlots` / `boot` / 每个 VM 的 CPU、内存、盘 | 只有配了 `[vm]` 的 Linux 主机 | 没有 | 无对应 | Firecracker 只在 Linux 上有，Mac 也没有；本机 run 两边都只报 run id 和开始时间 |
| `metrics.logins`（Claude Code） | Linux：`.credentials.json` 里有令牌；macOS：`.claude.json` 里有账号（凭据在钥匙串）；否则看 `ANTHROPIC_API_KEY` | 和 Linux 同一条规则（凭据文件）；另外 CLI 自己说已登录时（`claude auth status`），不论系统都算已登录（`daemon.rs` 里补的那一步） | 一致（G5：凭据放在哪待 G11 在真 CLI 的代码里确认；不论放在哪，`auth status` 那一步都能得到正确结果） | — |
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
| `doctor` | CLI、版本、模型、磁盘、负载、配置问题 | 同一段代码（CI 跑过）。不检查 Git for Windows | **缺口 G27** |
| `install-service` | launchd（macOS）、systemd 用户服务（Linux） | 当前用户的计划任务；注册前先做启动时的检查、检查程序文件别人能不能换 | 一致（做法见 `windows.zh.md` M3） |
| `uninstall-service` | 卸载并停止 | 结束任务、让 daemon 收尾退出、删除任务 | 一致 |
| `restart-service` | 没有：`launchctl kickstart -k`、`systemctl --user restart` 就是它 | 有：结束计划任务只结束 conhost，所以要 daemon 自己的命令 | 无对应（反过来的）：同一个结果 |
| `repair-program <路径>` | 没有：Unix 上替换程序文件是一次原子的 `rename` | 有：计划任务的第一个动作，把更新中途断电留下的 `.old-*` 改名回去 | 无对应（反过来的） |
| `example-config` | 打印示例 | 同 | 一致 |
| 以 `chat-run` 的名字启动 | `bin/chat-run` 是符号链接 | `bin\chat-run.exe` 是硬链接，跨盘时复制；按文件名（不分大小写、去掉 `.exe`）识别 | 一致 |
| 以 `vm-agent` 的名字启动 | 只在 Linux 的 microVM 里 | 报错退出，和 macOS 一样 | 无对应 |

## 3. 代码里按平台分开的每一处

`apps/daemon/src` 和 `tools/e2e/src` 里所有 `cfg(unix)`、`cfg(windows)`、`cfg(target_os = …)`、`cfg!(…)`
（共 180 处，测试里的另列在第 5 节）。同一件事的几处并成一行。

### 进程

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- | --- |
| `executor/mod.rs`、`executor/local.rs` / `local_windows.rs`、`lib.rs`（`proctree` / `proctree_windows`）、`winjob.rs` | 启动 CLI，管它的整棵进程树 | 进程组；`ps` 找逃出进程组的子孙（按环境变量 `CHAT_RUN_FOLDER`） | Job Object：先挂起启动、进 job 再运行，job 不许脱离、句柄关闭即全杀 | 一致（CI 跑过，含要求脱离的进程、没有控制台的进程、自成进程组的进程） | — |
| 同上 | 取消 | 向进程组发 SIGTERM，宽限后 SIGKILL | `CTRL_BREAK_EVENT`，宽限后 `TerminateJobObject`；事件发不出去就直接结束 job | 一致 | — |
| 同上 | 暂停 / 继续 | SIGSTOP / SIGCONT 整棵树 | 挂起 / 恢复 job 里每个进程的每个线程，重复扫到没有新线程 | 一致（负载下见 G19） | **缺口 G19**：加一个「树在不断启动新进程时暂停」的用例 |
| `executor/mod.rs`（`kill_leftover`）、`workspace.rs`（`RunMeta.pgid`） | daemon 崩溃后留下的 CLI | 重启时按 `run.json` 里的进程组杀掉 | 不会留下：job 的唯一句柄在 daemon 手里，daemon 没了系统就结束整棵树 | 无对应：不需要，保证相同（没有 CLI 活过 daemon）。CI 上有用例 | — |
| `health.rs`（`spawn`、`output`）、`models.rs`（`codex_models`）、`accounts.rs` | 探测和登录用的子进程 | 子进程本身；Codex 的 app-server 用进程组杀 | 每个在自己的 job 里，放弃时连它启动的一起结束 | 一致 | — |
| `workspace.rs`（`git`） | 跑 `git`，600 秒超时 | 超时杀 `git` | 超时只杀 `git.exe` | **缺口 G20** | 改用 `health::output`（进 job） |
| `runtime/pty.rs`、`winpty.rs` | `pty` 运行时 | `openpty`、`setsid`；取消是 SIGTERM 进程组 | ConPTY，同样先挂起、进 job；取消是往控制台打 Ctrl+C，3 秒后结束 job | 一致（CI 跑过：输出、输入、退出码、暂停、取消）。输出是「画屏幕」，见 `windows.zh.md` M2 | — |
| `executor/firecracker.rs`、`vm_agent.rs`、`executor/vmwire.rs`、`metrics.rs`（`allocated_bytes`、`process`） | 每个 run 一个 microVM | 只有 Linux | 没有 | 无对应：Mac 也没有 | — |
| `health.rs`（`which_in`、`windows_names`、`launch`、`shim_target`） | 找 CLI、启动 CLI | `PATH`、可执行位 | `PATH` + `PATHEXT`；npm 的 `.cmd` 解析出它真正启动的程序直接启动，参数不经过 `cmd.exe`；其余 `.cmd`/`.bat` 由标准库转义，带换行的参数启动前报错 | 一致（假的 npm `.cmd` CI 跑过；真的见 G11） | — |
| `sys.rs`（`OS_ENV`）、`run.rs`、`accounts.rs`（`login_env`） | 给 CLI 的环境变量 | `PATH`、`HOME` 等固定一组 | 另加 `SystemRoot`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、`TEMP`、`PATHEXT` 等 24 个系统变量 | 一致 | — |
| `accounts.rs`（`login_env`） | 登录链接不在机器上开浏览器 | `BROWSER=true`（`true` 是个什么都不做的程序） | 同一个值；Windows 没有 `true` | **缺口 G14** | 查真 CLI 的代码它在 Windows 上怎么开浏览器，给一个保证不开的值 |
| `main.rs`（`run`） | 停止信号 | SIGTERM、Ctrl+C | Ctrl+C、Ctrl+Break、控制台关闭、停止事件 | 一致 | — |

### 文件和权限

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- | --- |
| `sys.rs`（`mode`、`set_mode`、`protect`、`create_private_dir`、`create_private_file`、`acl_problem`、`open_to_others`、`owner`、`euid`）、`winacl.rs` | 私有的文件和目录 | 600 / 700 权限位 | 所有者 + 访问列表（当前用户、SYSTEM、Administrators），在创建的那一下就带上 | 一致 | — |
| `config.rs`（`problems`）、`accounts.rs`（`private_enough`、`write_private`） | **机器 token**、`machine.key`、钥匙库 | 明文文件，600；别人可读 daemon 不启动。不用钥匙串 | 明文文件，访问列表只给当前用户、SYSTEM、Administrators；所有者不对或多一条允许项 daemon 不启动 | 一致 | 决定：不放进 Credential Manager，也不用 DPAPI 再包一层。理由：Mac 和 Linux 的保证是「只有本用户和 root 能读」，Windows 的访问列表给的是同一个保证（本用户、SYSTEM、管理员），还多查了所有者。Credential Manager 里的凭据同一用户的任何进程都能读，不比文件多挡住谁，而从 SSH / WinRM 会话里装机器时它不可用。DPAPI 在管理员重置用户密码后会让 token 读不出来，机器就无声地掉线 |
| `workspace.rs`（`private_run_dir`） | run 目录 | `create_dir_all`，里面每个文件各自的权限位 | `<work_root>\runs` 和 run 目录创建时就私有；已有的同名目录别人进得去就拒绝 | 一致（Windows 更严） | — |
| 同上 | `work_root` 在 FAT、exFAT、网络共享上 | 不检查。在 macOS 上用 exFAT 映像试过：权限位改不了（一律 700），Unix socket 建不出来（`Operation not supported`），所以 run 没有 `chat-run`，带审批的 run 失败 | run 被拒绝，并说明要把 `work_root` 放到本机 NTFS 上 | 一致（都用不了；Windows 说得明白，Mac 是半残地跑）。不放宽 Windows：放宽等于让别的用户读到 run 里的密钥 | — |
| `workspace.rs`（`set_read_only`、`set_mode_bits`）、`winacl.rs`（`set_read_only`） | 旁聊的只读目录 | 递归去掉写权限位；用户自己能改回去，root 不受约束 | 目录访问列表上一条对当前用户的「拒绝写」，已有的和以后建的都继承；用户自己能改回去，提升权限的进程建目录、删文件时不受约束 | 一致（root ≙ 提升权限的管理员；daemon 不要提升权限运行） | **缺口 G18**：普通用户身份下删除和建目录被拒绝，补一个以普通用户身份跑的检查 |
| `workspace.rs`（`remove_dir`） | 删除 run 目录 | 删一次 | 文件被占用时重试 3 秒，还删不掉留给下次清理 | 一致 | — |
| `workspace.rs`（`git`） | 长路径 | — | `git -c core.longpaths=true` | 一致 | — |
| `chat_run.rs`（`install_link`、`link_name`、`started_as`） | `bin/chat-run` | 指向 daemon 的符号链接 | 硬链接，跨盘复制 | 一致 | — |
| `selfupdate.rs`（`Stamp`、`clean_stale`、`repair`） | 认出程序文件被换了 | 设备号、inode、大小、修改时间、可执行位 | 卷序列号、文件索引、大小、写入时间；启动时删 `.old-*` | 一致（CI 跑过整个替换协议） | — |
| `runtime/claude.rs`（`project_dir_name_of`） | Claude Code 的会话目录名 | 路径里非字母数字的都换成 `-` | 先去掉 `\\?\` 前缀再换（`C:\Users\me\work` → `C--Users-me-work`） | **缺口 G12** | 对着真 CLI 的代码核对规则（含长路径的哈希后缀、非 ASCII 字符） |
| `runtime/mod.rs`（`RunSpec::home`）、`config.rs`（`home`、`expand`） | 用户目录 | `HOME` | `HOME`，没有就 `USERPROFILE` | 一致 | — |
| `sys.rs`（`PATH_SEP`、`is_executable`、`host_name`、`disk`、`load_avg`、`cpu_times`） | 系统的小差别 | — | 见第 1 节 | 一致 | — |
| `tools/e2e/src/harness.rs`（`write_secret`） | 端到端工具存 owner token | 600 | 不设权限，靠用户目录本身私有 | 一致（开发工具；文件在用户目录下，默认只有本用户、SYSTEM、管理员能读。放到别处由运行的人负责，和 daemon 无关） | — |

### `chat-run`

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- | --- |
| `chat_run.rs`（`socket_path`、`Server`、`call`） | CLI 到 daemon 的通道 | Unix socket，600，另查对端 uid；路径太长时放临时目录 | 命名管道 `\\.\pipe\chat-run-<哈希>-<pid>`，访问列表只给当前用户，拒绝远程，必须是这个名字的第一个实例；客户端只许对方识别身份 | 一致（CI 跑过） | — |
| `chat_run.rs`（文件传输）、`chat_run_files.rs` | Codex 用的请求目录 | 目录 700，请求文件按属主过滤 | 目录继承 run 目录的访问列表，请求文件按所有者和访问列表过滤；读结果时遇到「正在被换上去」重试 | 一致（假 CLI CI 跑过；真 Codex 的沙箱见 G13） | — |
| `chat_run.rs` 的每个 op（`card.post`、`card.patch`、`interaction.resolve`、`artifact.*`、`desk.note`、`member.*`、`secret`、`gate.check`、文件上传、产出） | — | 同一段代码 | 同一段代码；`integration.rs` 现在在 CI 上跑（card patch、artifact create、resolve、超时两种说法） | 一致 | — |
| `gates.rs`（`enforced_by`） | 谁的 run 会拦命令 | Claude Code | 没有 | **缺口 G1** | 见第 6 节 |
| `gates.rs`（`fails_closed`、`hook_failure`） | 钩子问不到 daemon | 放行（一直如此） | 拒绝 | 一致（Windows 更严，有意的） | — |
| `gates.rs`（`hook_command_line`） | 钩子命令行 | `'<路径>' gate` | `"<正斜杠路径>" gate`（Git Bash 和 `cmd.exe` 里都是一个词） | 待证（G1） | — |
| `gates.rs`（`claude_settings`） | 钩子匹配哪些工具 | `^(Bash|mcp__.*)$` | 同 | **缺口 G2** | 查真 CLI 的 Windows 版有哪些能执行命令的工具，都要匹配，并让钩子读得懂它们的输入 |

### 服务

| 文件 | 事 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- | --- |
| `service.rs`（`install`、`install_systemd`、`install_task` 及其辅助）、`wintrust.rs` | 开机 / 登录自启、退出后重启 | launchd `KeepAlive`、systemd `Restart=always` | 计划任务：登录时启动，每分钟触发器把退出的 daemon 一分钟内拉起，单实例，无时限 | 一致（CI 对着真的任务计划程序跑过；真的注销登录、重启见「要在真机上确认」） |
| `service.rs`（`uid`、`launchctl`） | launchd 的用户域 | `gui/<uid>` | 不用（任务属于当前用户的 SID） | 无对应：launchd 的概念 |
| `service.rs`（PATH） | 服务的 `PATH` | 安装时把当时的 `PATH` 写进 plist / unit | 不写：计划任务每次启动拿用户当前的环境变量 | 一致（Windows 的更新得更勤；常见安装目录见 G24） |
| `service.rs`（`log_to`、`wait_stop`、`restart_task`、`task_*`） | 日志、停止、重启 | launchd / systemd 提供 | daemon 自己做 | 一致 |

## 4. 运行时

| 运行时 | Mac / Linux | Windows 现在 | 状态 |
| --- | --- | --- | --- |
| Claude Code（stream-json、插话、续接、effort、旁聊的只读模式、会话文件上传） | — | 同一段代码；假 CLI 的 `runtimes.rs` 和整个 daemon 的 `integration.rs` CI 跑过；真 CLI 见第 6 节 | 一致（假 CLI）；G11、G12 |
| Codex（app-server、审批、插话、续接、effort、旁聊的 MCP 关闭） | — | 同上 | 一致（假 CLI）；G8、G13 |
| ACP | — | 同一段代码，`runtimes.rs` CI 跑过 | 一致 |
| `pty` | `openpty` | ConPTY；批处理文件不能当程序（明确报错） | 一致 |

## 5. 测试

| 测试 | Mac / Linux CI | Windows CI | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| 单元测试（`src/**`） | 跑 | 跑（另有 Windows 专有的：访问列表、命名管道、任务 XML、命令行引号） | 一致 | — |
| `tests/normalize.rs`、`redact.rs`、`chat_run.rs` | 跑 | 跑 | 一致 | — |
| `tests/process_tree.rs` | 跑 | 跑（另有 `.cmd`、job、ConPTY 的用例） | 一致 | G19 加负载下暂停 |
| `tests/runtimes.rs` | 跑 | 跑，除了：两个 `pty` 用例（Windows 的在 `process_tree.rs` 里）、两个真 Codex 的用例 | **缺口 G8** | 在 runner 上装真的 Codex，让那两个用例在 Windows 上跑 |
| `tests/integration.rs`（13 个用例：整个 daemon 对着进程内的假服务器和假 CLI） | 跑 | 以前不跑。现在跑（本 PR，G6）：假 CLI 经 `fake-cli.exe` 启动；「是否停住」在 Windows 上用线程挂起计数判断；崩溃后留下 CLI 的那一段是 Unix 专有的（见第 3 节 `kill_leftover`） | 一致，除了 `gated_commands_wait_for_a_person`：Windows 上现在只断言「不上报 `gates`」，G1 打开后自动跑完整用例 | — |
| `tests/setup.rs`（协议 fixture、配置、doctor、worktree、镜像、清理、只读目录） | 跑 | 以前不跑。现在跑（本 PR，G7），除了 launchd 和 systemd 两个用例（Windows 的是 `service_task.rs`） | 一致 | — |
| `tests/service_task.rs` | 不适用 | 跑（真的任务计划程序） | 无对应（反过来的）：Mac 的 launchd 只测 plist 文本 | — |
| `scripts/test-install-daemon.ps1` | 不适用 | 跑 | 同上 | — |
| `apps/daemon/e2e/local-e2e.ts`（`wrangler dev`、真 daemon、假 CLI） | Linux 跑 | 不跑 | **缺口 G9** | 脚本里四处 Unix 写法改成两边都能跑，加进 `daemon-windows` |
| `tools/e2e` 的机器用例 | 对着已部署的环境手动跑 | 只编译 | **缺口 G9** | 同上（本地服务器在 runner 上） |
| 整条链 | — | 没有 | **缺口 G31** | 见第 0 节 |

## 6. 依赖真 CLI 的事（没有登录，能在 runner 上证明多少证明多少）

runner 能装真的 Claude Code 和 Codex（npm），不能登录。不需要登录的都要在 runner 上变成「见过」；
需要模型调用的，用本机的假模型接口（CLI 是真的，模型是假的）让真的 CLI 走到调工具那一步。

| 事 | 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- |
| Claude Code 在 Windows 上启动钩子命令（用哪个 shell、怎么处理引号和路径） | 没见过 | **缺口 G1** | 读它的文档和随包代码；runner 上真的 CLI 对着假模型接口发起一次 Bash 工具调用，看钩子是否被启动、拒绝是否生效、daemon 问不到时是否拒绝 |
| 钩子匹配的工具 | 只有 `Bash` 和 MCP | **缺口 G2** | 同上 |
| `claude.cmd`、`codex.cmd` 的解析，`--version`，`doctor` | 只对着假的 `.cmd` 测过 | **缺口 G11** | runner 上 `npm i -g` 两个真包，跑 `daemon doctor` 并断言找到、有版本 |
| 登录和会话文件在 Windows 上放哪 | 按推断 | **缺口 G12** | 读真 CLI 的代码；runner 上真的 CLI 跑一轮（假模型），看会话文件落在 daemon 算出的目录里 |
| Codex 的 Windows 沙箱和 `chat-run` | 没见过 | **缺口 G13** | runner 上跑 `runtimes.rs` 里现成的真 Codex 用例（`command/exec`，不需要模型） |
| 登录时不开本机浏览器 | `BROWSER=true` | **缺口 G14** | 读真 CLI 的代码 |
| Git for Windows | 不检查 | **缺口 G27** | 查真 CLI 是否必须有 bash；必须的话 `doctor` 报出来 |

## 7. 「用这台电脑」和安装命令

| 事 | Mac | Windows 现在 | 状态 | 怎么补 |
| --- | --- | --- | --- | --- |
| daemon 在哪 | 应用包里 `Contents/MacOS/<slug>-daemon` | 安装目录里客户端旁边的 `<slug>-daemon.exe`；debug 构建和 `target` 目录下一律当作没有 | 一致 | — |
| 写配置和 token | 目录 700，文件 600，先写临时文件再改名 | 目录先设成只有当前用户、SYSTEM、Administrators（`icacls`），再写文件 | 一致 | — |
| 注册服务 | `<daemon> install-service`，把登录 shell 的 `PATH` 加常见安装目录传给它 | `<daemon> install-service`，不传 `PATH` | **缺口 G24** | 查 Windows 上 CLI 的常见安装目录是否都在用户 `PATH` 上；不在的由 daemon 在找 CLI 和给 run 的 `PATH` 里补上 |
| 状态 | 读 launchd 的 plist | 读 `task.xml`（UTF-16） | 一致 | — |
| 重启 | `launchctl kickstart -k` | `<daemon> restart-service` | 一致 | — |
| 机器名、标签 | `scutil --get ComputerName`、`mac` | `COMPUTERNAME`、`windows` | 一致 | — |
| 实际运行过 | 是 | **只编译过**（单元测试只覆盖纯函数） | **缺口 G10** | runner 上一个测试：写配置、注册任务、读状态、重启、卸载，用刚编译的真 daemon |
| 安装命令 | `curl … \| bash`（`docs/bots.md` 第 6 节） | `scripts/install-daemon.ps1`：https、必填 SHA-256、token 不上命令行、私有目录、`doctor`、`install-service`、`-Update` | 一致（CI 跑过，Windows PowerShell 5.1） | 设置页「添加机器」里给出 Windows 命令归客户端那条线 |

## 8. 要在真机上确认

补完缺口之后，只剩下 CI 做不到的。每条写明步骤，放在最后一个 PR 里补全。现在已知的两条：

1. 一次真的、已登录的 Claude Code run（CI 不能登录）。
2. 一次真的重启电脑（CI 只能模拟任务的结束和再启动）。
