# Windows 端计划

状态：W0 已完成（客户端能为 Windows 编译并链接出 exe，还没在 Windows 上运行过）。M1 已完成（daemon 能为
Windows 编译；Windows CI 上用假 CLI 跑通了一个 run、取消和暂停；还没在真的 Windows 电脑上跑过真的 CLI）。
其余未开始。

## 目标

Windows 电脑能做 Mac 电脑现在能做的每一件事。不是只做一个聊天客户端，是三样东西一起：

1. **客户端**：和 Mac 版功能一致，视觉达到同一验收标准（聊天、话题、产出窗口、图、媒体预览、通知、
   托盘、设置、更新）。
2. **机器**：Windows 电脑能当团队的机器跑 bot。`apps/daemon` 在 Windows 上原生运行，能启动
   Claude Code、Codex 等 CLI，支持 `chat-run`、取消、审批、受控的密钥、工作区、开机自启和自己更新。
3. **安装和发布**：一个安装包同时装好客户端和 daemon；设置里的「用这台电脑」在 Windows 上一键可用；
   没装客户端的机器有一条 PowerShell 安装命令；发版流程同时出 Mac 和 Windows 的产物。

范围以外（要做再另说）：在 Windows 上开发本项目（`bun run dev:desktop`、发版脚本仍然在 Mac 或 Linux
上跑，Windows 的编译和测试靠 CI）；ARM64 的 Windows；Firecracker runner（那是 Linux 主机上的另一种
执行方式，和 Windows 无关）。

不变的部分：服务器、`docs/protocol.md`、`docs/machine-protocol.md`、`crates/chat-protocol`、
`packages/bot-sdk`（跑在 Bun 和 Node 上，Windows 本来就能用）。

## 现状

### 客户端（`apps/desktop`、`crates/artifact-view`、`crates/chat-core`）

GPUI、gpui-component 和 `chat-core` 不用改就能为 Windows 编译；W0 修掉了 11 处 Unix 专用代码。
下面这些在 Windows 上现在是空实现，要写 Windows 版：

| 模块 | 现在用的 macOS 接口 | Windows 方案 |
| --- | --- | --- |
| `crates/artifact-view`（产出窗口、内嵌页面） | WKWebView、`WKURLSchemeHandler`、`WKContentRuleList`、子视图嵌入 | WebView2：虚拟主机或 `WebResourceRequested` 提供内容，请求拦截做网络封锁，子 HWND 嵌入 |
| `src/diagrams`（Mermaid 图） | 隐藏的 WKWebView | 同一个 WebView2 后端的隐藏实例 |
| `src/views/media/native.rs`（音视频、PDF 预览） | AVFoundation、AVFAudio、PDFKit | Media Foundation 加 `Windows.Data.Pdf` |
| `src/provider/macos.rs`（通知、睡眠唤醒、网络变化） | `UNUserNotificationCenter`、`NSWorkspace` | WinRT Toast（需要 AUMID）、`WM_POWERBROADCAST`、`NotifyIpInterfaceChange` |
| `crates/chat-core/src/token.rs` | Keychain | Windows Credential Manager |
| `src/views/settings/login_item.rs`（开机启动） | `SMAppService` | `HKCU\...\Run` |
| `src/views/nav/menus.rs`、`status_item.rs` | 菜单栏、状态栏图标、关于面板 | 窗口内菜单加系统托盘图标 |
| `src/views/files/native.rs` | Quick Look、分享面板 | 用默认程序打开、资源管理器里定位、`DataTransferManager` 分享 |
| `src/ui/sf_symbols` | SF Symbols 栅格化 | 自带一套 SVG 图标 |
| `src/theme.rs`、`src/ui/motion.rs` | 系统外观、减弱动画 | `UISettings`、`SPI_GETCLIENTAREAANIMATION` |
| `src/views/settings/window_fit.rs` | NSWindow 尺寸和位置 | `GetMonitorInfo` 和工作区 |
| `src/provider/this_mac.rs`（用这台电脑） | 应用包里的 daemon 加 launchd | 安装目录里的 daemon 加计划任务，见 M3 |
| `src/updater` | DMG、`hdiutil`、替换 `.app` | 安装包加单独的更新程序 |
| `src/selftest/capture.rs`（截图自测） | NSWindow 编号、`screencapture` | `PrintWindow` 或 `Windows.Graphics.Capture` |

### 机器（`apps/daemon`，约 1.15 万行）

M1 之前为 Windows 编译有 68 处错误（加上测试 115 处），M1 之后是 0。协议、运行时对各 CLI 的解析、
脱敏、模型列表、配置这些大头和平台无关。下表是每一块的 Windows 方案；M1 做了哪些见「阶段」里的 M1。

| 模块 | 现在的 Unix 做法 | Windows 方案 |
| --- | --- | --- |
| `executor/local.rs`、`proctree.rs`（启动 CLI、杀掉整棵进程树） | 进程组、`kill(-pgid)`、用 `ps` 找逃出进程组的子孙 | Job Object：CLI 和它启动的所有进程都在一个 job 里，`TerminateJobObject` 一次杀干净，比现在的 `ps` 查找更可靠 |
| 取消、暂停、继续 | `SIGTERM` 后 `SIGKILL`；`SIGSTOP`/`SIGCONT` | 先发 `CTRL_BREAK_EVENT`（CLI 用 `CREATE_NEW_PROCESS_GROUP` 启动），超时后结束 job；暂停用挂起 job 里各进程的线程 |
| `chat_run.rs`（CLI 向群里说话） | Unix socket（0600，只许本用户连）；`bin/chat-run` 是指向 daemon 的符号链接 | 命名管道，ACL 只给当前用户；`chat-run.exe` 用硬链接（不行就复制）。Codex 用的文件传输方式不用改 |
| `runtime/pty.rs`（没有结构化协议的 CLI） | `openpty`、`setsid` | ConPTY |
| `service.rs`、`selfupdate.rs` | launchd、systemd；程序文件被替换后退出，由服务管理器重启 | 当前用户的计划任务（登录时启动，失败后重启）。不能用 Windows 服务：服务跑在别的会话里，读不到用户的 CLI 登录状态。更新时先把运行中的 exe 改名再放新文件 |
| `health.rs`、`doctor.rs`（找 CLI） | 查 `PATH`、可执行位 | 按 `PATHEXT` 找，npm 装的 CLI 是 `.cmd` 文件，启动时要正确转义参数 |
| 文件权限（配置、token、运行目录） | 0600 / 0700 | 放在用户目录下靠默认 ACL；机器 token 存 Credential Manager |
| `workspace.rs`（git worktree、运行目录） | — | 打开长路径支持并用短的工作目录；删除运行目录时处理文件被占用 |
| `runtime/claude.rs`（找原始会话文件） | 按 Unix 路径算目录名 | 按 Windows 路径（盘符、反斜杠）算 |

要在 Windows 上核实的外部条件：Claude Code 和 Codex 的 Windows 原生版本各自的要求（Claude Code 需要
Git for Windows），订阅登录在 Windows 上放在哪，Codex 在 Windows 上的沙箱方式是否影响 `chat-run`。

不用 WSL 的理由：在 WSL 里跑现有的 Linux daemon 不用改代码，但 bot 就只能动 Linux 文件系统，碰不到
这台 Windows 电脑上的项目和工具，而且要求每个成员先装 WSL。所以做原生的。

## 阶段

三条线。客户端（C）和机器（M）互相不依赖，可以两个人或两个会话同时做；发布（R）在两条线都能用之后。
每个阶段单独提 PR，测试跑过才算完成。

### W0 客户端能编译（已完成）

- 客户端的 11 处 Unix 专用代码按平台分开；新增 `apps/desktop/src/paths.rs`（Windows 上数据在
  `%APPDATA%\<app id>`，缓存在 `%LOCALAPPDATA%`，下载在 `%USERPROFILE%\Downloads`）。
- CI 增加 `desktop-windows` 任务（`windows-2025`）：编译客户端，跑 `chat-protocol` 和 `chat-core` 的测试；
  客户端自己的测试先只报告不拦截。
- 在 Mac 上检查 Windows 编译：`brew install mingw-w64`，`rustup target add x86_64-pc-windows-gnu`，
  `cargo check --target x86_64-pc-windows-gnu -p desktop --all-targets`。正式目标是
  `x86_64-pc-windows-msvc`，以 Windows CI 为准。

### 客户端

**C1 能用（约 2 周）**

- Token 存到 Credential Manager。
- 窗口：自绘标题栏和窗口按钮、拖动区、Snap 布局、高 DPI 和不同缩放比的显示器。
- 输入：中文输入法（候选窗位置、组合中的文字）；快捷键从 Cmd 映射到 Ctrl。
- 通知（点击打开话题）、睡眠唤醒和网络变化后立即重连、托盘图标、开机启动。
- 客户端测试在 Windows 上全过，去掉 CI 里的 `continue-on-error`。

完成标准：在 Windows 上登录，收发消息、看话题、收通知、断线重连都正常。

**C2 产出和图（约 3 周，客户端里风险最大）**

- `artifact-view` 增加 WebView2 后端，对外接口不变：每个版本一个 origin、同样严格的 CSP、拦截所有
  外部请求和跨 origin 跳转、`window.artifact` 桥按 `Capabilities` 检查、嵌入宿主窗口。
- 第一周先做只嵌一个页面的原型，确认子 HWND 和 GPUI 的渲染叠在一起没有闪烁和焦点问题。
- Mermaid 渲染器用同一个后端。安装包带 WebView2 的 Evergreen bootstrapper。

完成标准：两个平台通过同一套隔离测试，Mermaid 图能显示。

**C3 媒体和文件（约 2 周）**：视频首帧和时长、音频波形和播放、PDF 预览；打开、定位、分享文件。

**C4 设计验收（约 2 周，可以和 C2、C3 同时做）**

- 写 `docs/design/WINDOWS.md`（NATIVE.md 的 Windows 版）；COMPLETENESS.md 的黑白 token 和尺寸规范照用。
- 一套 SVG 图标替换 SF Symbols。
- 截图自测移植到 Windows，浅色深色都截；每个页面过 COMPLETENESS.md §4 清单。

### 机器

**M1 能编译、能跑一个 run（已完成）**

做了什么：

- **进程树**（`executor/local_windows.rs`、`winjob.rs`）：CLI 以挂起状态启动（`CREATE_SUSPENDED`，同时
  `CREATE_NEW_PROCESS_GROUP`），放进一个新的 Job Object 之后才恢复运行，所以它启动的每个进程从第一条指令起
  就在 job 里。job 设了「句柄关闭就全杀」，不允许脱离，句柄只有 daemon 自己有。Unix 的代码
  （`executor/local.rs`、`proctree.rs`）一行没动，两边在 `executor/mod.rs` 和 `lib.rs` 按平台选文件。
- **取消**：先向 CLI 的进程组发 `CTRL_BREAK_EVENT`，等和 Unix 一样的宽限时间，再 `TerminateJobObject`。
  事件发不出去（daemon 没有控制台）就直接结束 job。CLI 自己退出后也会结束 job，清掉它留下的工具进程。
- **暂停和继续**（原计划放 M2，M1 一起做了）：挂起 job 里所有进程的所有线程，继续时恢复；重复扫描直到
  没有新线程，暂停期间新启动的进程也会停住。
- **daemon 崩溃后的残留**：不需要处理。daemon 一没，job 的句柄被系统关掉，整棵树跟着结束，所以 Windows 上
  `kill_leftover` 是空的。
- **找 CLI**（`health.rs`）：按 `PATHEXT` 找，只认 `.com/.exe/.bat/.cmd`，不认没有扩展名的同名文件（npm
  会在 `claude.cmd` 旁边放一个 shell 脚本 `claude`）。在 run 自己的 `PATH` 里找。
- **`.cmd`**：npm、pnpm、yarn 生成的 `.cmd` 只是转一道手。daemon 读出它真正启动的东西（包里的 `.exe`，或
  `node` 加脚本）直接启动，提示词和参数不经过 `cmd.exe`。认不出来的 `.cmd`/`.bat` 照原样启动，由 Rust
  标准库按 `cmd.exe` 的规则转义，转义不了的参数会报错而不是硬传。
- **`chat-run`**：文件传输（`CHAT_RUN_DIR`）在 Windows 上可用，M1 里 Windows 上所有运行时都走它（Unix 上
  仍然只有 Codex 走文件）。`bin\chat-run.exe` 是指向 daemon 的硬链接，跨盘时复制。socket 传输在 Windows 上
  返回明确的「还不支持」。
- **环境变量**：Windows 上给 CLI 多带一组系统变量（`SystemRoot`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、
  `TEMP`、`PATHEXT` 等）。少了 `SystemRoot`，Node 的网络和加密都起不来。
- **其他**：主机名取 `COMPUTERNAME`；用户目录取 `USERPROFILE`；文件权限靠用户目录的 ACL（`sys.rs` 里的
  mode 函数在 Windows 上不做事）；磁盘空间用 `GetDiskFreeSpaceExW`，负载报 0；git 加
  `core.longpaths=true`；删除运行目录遇到文件被占用会重试几秒，还删不掉就留给下一次清理；Claude Code
  会话目录名按去掉 `\\?\` 前缀的路径算（`C:\Users\me\work` 是 `C--Users-me-work`）；自己更新的文件指纹
  用大小、写入时间和创建时间。
- **先不支持、会明确报错的**：`pty` 运行时、`install-service` / `uninstall-service`、`[vm]`。
- `tools/e2e` 能为 Windows 编译。

测试（`apps/daemon/tests/process_tree.rs`，Unix 和 Windows 跑同一份）：测试程序自己当假 CLI，说 Claude Code
的 stream-json，不需要真的 CLI 和登录。四个用例：一个 run 启动、分段输出、结束；取消后整棵树没有活着的
进程（CLI、它的子进程、它的工具、工具启动的自成进程组的孙进程、没有控制台的后台进程、一个要求脱离 job 的
进程）；暂停后整棵树不再有输出，继续后恢复，暂停中取消也能杀干净；通过 npm 式的 `.cmd` 按名字找到并启动
CLI，带引号、`&`、`%`、换行的参数原样送到。CI 新增 `daemon-windows` 任务（`windows-2025`）：
`cargo test -p daemon`（单元测试、`chat-run` 客户端、上面这份）和 `cargo check -p e2e`。

在 Mac 上检查：`cargo check --target x86_64-pc-windows-gnu -p daemon -p e2e --all-targets`。

M1 里发现、留给后面的：

- **Windows 上还没跑的测试**：`tests/integration.rs`、`tests/runtimes.rs`、`tests/setup.rs` 用 Python 写的
  假 CLI（靠 `#!` 启动）、`/bin/sh`、信号和文件权限，现在只在 Unix 上跑。M2 把假 CLI 改成两个平台都能
  启动的形式再打开。
- **没有控制台时的取消**：`CTRL_BREAK_EVENT` 只能发给和 daemon 共用一个控制台的进程。以后 daemon 由计划
  任务在后台启动时可能没有控制台，那时取消会跳过宽限直接结束 job，CLI 来不及收尾。M3 定启动方式时
  一起决定（给 daemon 配一个隐藏的控制台，或者接受直接结束）。
- **审批（gates）**：Claude Code 的 PreToolUse 钩子通过 `chat-run` 的 socket 问 daemon，Windows 上还没有
  这条通道，所以 Windows 机器暂时不上报 `gates` 能力，服务器不会把要审批的规则发给它。M2 有了命名管道
  （或让钩子走文件传输）后打开；钩子命令的引号写法也要按 Windows 上 Claude Code 实际用的 shell 核对。
- **旁聊（aside）的只读目录**：Unix 上去掉写权限；Windows 上只给已有文件加了只读属性，拦不住在目录里新建
  文件，要用 ACL 才行。
- **自己更新**：发现程序文件被替换后 daemon 会退出等服务管理器重启，Windows 上还没有服务（M3），在那之前
  替换 exe 之后要手动重启。
- **机器 token**：仍然是用户目录下的文件，没有放进 Credential Manager。
- **要在真机上核实的**：真的 Claude Code 和 Codex 各跑一个 run；它们的 Windows 版把登录放在哪
  （`%USERPROFILE%\.claude`、`%USERPROFILE%\.codex`，还是 Credential Manager），daemon 清空环境变量后它们
  还能不能读到；Claude Code 在 Windows 上的会话目录名规则是否和上面一致；Claude Code 需要的 Git for
  Windows；杀毒软件对没签名的 daemon 和 Job Object 的反应。

**M2 功能齐全（约 2 周）**

- `chat-run` 的命名管道传输；受控密钥（`chat-run secret`）；审批钩子。
- Codex 和 ACP 运行时在 Windows 上验证；ConPTY 版的 `pty` 运行时。
- Python 假 CLI 的三套测试在 Windows 上打开；旁聊只读目录的 ACL；`daemon doctor`。

完成标准：`tools/e2e` 的机器用例对着一台 Windows 机器全过。

**M3 服务和「用这台电脑」（约 1.5 周）**

- `daemon install-service` / `uninstall-service` 的 Windows 版（计划任务）；自己更新（改名再替换）。
- 客户端的「用这台电脑」：写配置和机器 token，注册并启动计划任务，显示状态，重启。
- PowerShell 安装命令，给不装客户端的机器用。

完成标准：在设置里点一下，这台 Windows 电脑出现在团队的机器列表里，重启电脑后自己上线。

### 发布

**R1 安装包、签名、更新（约 2 周）**

- 安装包（NSIS）：客户端、daemon、WebView2 bootstrapper。
- 代码签名（证书由运营方提供，放在 CI secret 里，不进仓库）。
- 客户端更新器：下载、校验 SHA-256，由单独的更新程序在主程序退出后替换文件再重启；daemon 跟着更新。
- `release.yml` 增加 Windows 任务；服务器的版本接口按平台返回产物（只加字段，不改已有字段的含义）；
  `docs/INSTALL.md` 加 Windows 一节。

完成标准：从上一个版本自动更新到新版本，客户端和 daemon 都换成新的。

### 工期

一个人顺序做约 16–17 周（客户端 9，机器 5.5，发布 2）。客户端和机器两条线同时做约 10–11 周。

建议的顺序：C1 和 M1 先做（最早知道 Windows 上哪里有坑），然后 C2 和 M2，再 C3/C4 和 M3，最后 R1。

## 风险

- **GPUI 是预发布版本**（`gpui-pre =0.3.10`）：Windows 上的输入法、DPI 问题可能要给上游打补丁或先 vendor。
- **WebView2 嵌入**：子 HWND 和 GPUI 的渲染叠在一起可能闪烁或抢焦点，C2 先做原型。
- **CLI 在 Windows 上的行为**：Claude Code 和 Codex 的 Windows 版比 Mac 版新，工具调用、沙箱、
  会话文件位置可能不一样；M1 第一周先手动在 Windows 上各跑一遍确认。
- **进程逃出 job**：极少数程序会主动脱离 job。Job 设成不允许脱离，`tests/process_tree.rs` 里有一个要求
  脱离的进程覆盖这一点（M1 已做）。
- **代码签名**：新证书在 SmartScreen 里要积累信誉；没签名的 daemon 还可能被杀毒软件拦。
- **需要真机**：界面、输入法、计划任务、CLI 登录都要在真的 Windows 上看，CI 只能管编译和单元测试。

## 默认的决定

没有反对就按这些做：

- 安装包用 NSIS 加自己写的更新程序，和 Mac 的 DMG 流程对应。
- 媒体预览用 Windows 原生接口，Mac 端不动。
- 最低支持 Windows 10 22H2（WebView2 和 ConPTY 都有）。
- daemon 原生移植，不走 WSL；用计划任务，不用 Windows 服务。

已定：

- **外观**：Windows 版和 Mac 版一样，同一套黑白规范（COMPLETENESS.md），只换标题栏和窗口按钮；图标用
  一套两端通用的 SVG。
