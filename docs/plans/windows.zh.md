# Windows 端计划

状态：W0 已完成（客户端能为 Windows 编译并链接出 exe，还没在 Windows 上运行过）。其余未开始。

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
| `src/views/media/native.rs`（音视频、PDF 预览） | AVFoundation、AVFAudio、PDFKit | 已完成（C3）：Media Foundation、`Windows.Data.Pdf`、`Windows.Media.Playback` |
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
| `src/selftest/capture.rs`（截图自测） | NSWindow 编号、`screencapture` | 已完成：`PrintWindow` 加 `PW_RENDERFULLCONTENT`，见「怎么看 Windows 界面」 |

### 机器（`apps/daemon`，约 1.15 万行）

现在为 Windows 编译有 57 处错误，集中在 6 个地方。协议、运行时对各 CLI 的解析、脱敏、模型列表、
配置这些大头和平台无关。

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

## 怎么看 Windows 界面

没人有 Windows 电脑，界面靠 CI 截图看。`desktop-windows` 任务（`windows-2025`）编译完客户端后跑截图自测
（`desktop.exe --screenshot`，数据来自进程内的 fake，不连服务器），浅色深色各截一遍，把 PNG、
`screenshots.json` 和 `selftest.log` 传成名为 `windows-screenshots` 的产物，保留 14 天。

1. 推分支并开 PR（改到 `apps/desktop/**` 就会跑），或者手动跑并指定场景：

   ```bash
   gh workflow run ci.yml --ref <分支> -f desktop=false -f windows_scenes=main,settings-about
   ```

   `windows_scenes` 留空是默认的主要页面（列表在 `scripts/windows-screenshots.sh`），`all` 是全部场景；
   场景名用 `desktop --list-scenes` 看，新场景加在 `apps/desktop/src/selftest/scenes.rs`。
2. 等 `Upload the screenshots` 这一步跑完（不用等整个任务），下载：

   ```bash
   gh run download <run id> -n windows-screenshots -D .tmp/win-shots
   ```

3. 读 PNG（`<场景>-<light|dark>.png`）。一张都没有时看 `selftest.log`：应用没启动起来或某个场景失败，
   原因在里面。

要知道的几点：

- 截图用 `PrintWindow` 加 `PW_RENDERFULLCONTENT`（`apps/desktop/src/selftest/capture.rs`），只截应用自己
  窗口的客户区，被别的窗口挡住或者比屏幕大也能截。
- runner 没有显卡，GPUI 走 WARP（软件 Direct3D），缩放是 100%，所以 1440×900 的窗口截出来就是
  1440×900 像素（Mac 上是 2 倍）。别的缩放比例 CI 上测不了。
- runner 的屏幕默认 1024×768，任务先改成 1920×1080，否则窗口会被系统限制在屏幕以内。
- 截图步骤失败会让任务变红，产物照样上传。
- 颜色没有经过显示器色彩配置转换，PNG 里的像素值就是主题 token 的值。

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

媒体预览已完成，代码在 `apps/desktop/src/views/media/native_windows.rs`，对外还是 `native.rs` 那几个函数，
macOS 的代码没动：

- 视频：Media Foundation 的 `IMFSourceReader` 解出 RGB32 的帧，按显示区域裁剪、按旋转信息转正、缩到调用方
  要的大小；时长取自 `MF_PD_DURATION`。
- 音频波形：同一个 source reader 解成 PCM（8/16/24/32 位整数或浮点），算出峰值；时长按实际解出的帧数算。
- 音频播放：`Windows.Media.Playback.MediaPlayer`（播放、暂停、跳转、位置、倍速），不进系统的媒体控制条。
- PDF：`Windows.Data.Pdf` 把每页渲染成 PNG 流再解成位图；页数和页面大小也从它来。
- 线程：进程用 `CoIncrementMTAUsage` 加入多线程套间一次；每次解码在自己的线程里跑，调用方带超时等结果，
  解码器卡住也只占一个线程；播放器在后台线程里打开，界面线程只发短命令。
- Media Foundation 按名字动态加载（`mfplat.dll`、`mfreadwrite.dll`），不静态链接：Windows N 版和 Windows
  Server 默认没有它，应用照样能启动，媒体预览退回文件行。
- 系统没有解码器的格式（比如没装 HEVC 扩展时的 HEVC）和坏文件一样返回「没有预览」，界面显示原来的文件行。

测试在 `native.rs` 末尾，macOS 和 Windows CI 跑同一套：MP4 的时长、尺寸和各帧的朝向与颜色，带旋转信息的
MP4，HEVC，WAV、MP3、M4A 的峰值，PDF 的页数和渲染结果，每种各有坏文件。样本在
`apps/desktop/src/views/media/fixtures/`（ffmpeg 用纯色和正弦波生成，每个几 KB）。CI 的 `desktop-windows`
任务先装 `Server-Media-Foundation`（Windows Server 才需要），再跑 `cargo test -p desktop views::media::native`
（失败会拦截），截图自测默认多跑 `media-video`、`media-audio`、`media-pdf` 三个场景。

还要在真的 Windows 电脑上确认的：声音确实能放出来（CI 的机器没有声卡）、拖动进度和倍速的手感、
Windows 10 22H2 上的表现、MOV 和 HEVC 等格式在装了和没装扩展时的覆盖范围、大文件的耗时。

打开、定位、分享文件（`src/views/files/native.rs`）在系统集成那个 PR 里做。

**C4 设计验收（约 2 周，可以和 C2、C3 同时做）**

- 写 `docs/design/WINDOWS.md`（NATIVE.md 的 Windows 版）；COMPLETENESS.md 的黑白 token 和尺寸规范照用。
- 一套 SVG 图标替换 SF Symbols。
- 截图自测移植到 Windows，浅色深色都截；每个页面过 COMPLETENESS.md §4 清单。

### 机器

**M1 能编译、能跑一个 run（约 2 周）**

- `executor/local.rs` 和 `proctree.rs` 抽出平台层；Windows 实现用 Job Object 启动和结束进程树。
- 取消：`CTRL_BREAK_EVENT`，超时后结束 job。
- 找 CLI（`PATHEXT`、`.cmd`）、主机名、文件权限、工作区路径。
- CI 的 Windows 任务加上 `cargo test -p daemon`。

完成标准：Windows 上 `daemon run` 连上服务器，一个 Claude Code run 从开始跑到结束，取消能杀干净整棵进程树。

**M2 功能齐全（约 2 周）**

- `chat-run` 的命名管道传输和 `chat-run.exe`；受控密钥（`chat-run secret`）。
- Codex 和 ACP 运行时在 Windows 上验证；ConPTY 版的 `pty` 运行时。
- 暂停和继续；原始会话文件的路径；`daemon doctor`。

完成标准：`tools/e2e` 的机器用例对着一台 Windows 机器全过（`tools/e2e` 本身也要能在 Windows 上编译）。

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
- **进程逃出 job**：极少数程序会主动脱离 job。Job 设成不允许脱离，再加一个测试覆盖。
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
