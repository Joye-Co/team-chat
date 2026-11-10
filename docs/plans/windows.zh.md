# Windows 端计划

状态：W0 已完成（客户端能为 Windows 编译并链接出 exe）。C1 的系统集成部分已写完（见 C1 一节）。M1 已完成
（daemon 能为 Windows 编译；Windows CI 上用假 CLI 跑通了一个 run、取消和暂停；还没在真的 Windows 电脑上跑过
真的 CLI）。C2（产出和图的 WebView2 后端）代码已完成并在 Windows CI 上跑通隔离测试，见下。其余进行中或未开始。

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
| `crates/artifact-view`（产出窗口、内嵌页面） | WKWebView、`WKURLSchemeHandler`、`WKContentRuleList`、子视图嵌入 | WebView2（已做，见 C2）：注册自定义 scheme，`WebResourceRequested` 提供内容并拦截其余请求，子 HWND 嵌入 |
| `src/diagrams`（Mermaid 图） | 隐藏的 WKWebView | 同一个 WebView2 后端的隐藏实例（已做） |
| `src/views/media/native.rs`（音视频、PDF 预览） | AVFoundation、AVFAudio、PDFKit | 已完成（C3）：Media Foundation、`Windows.Data.Pdf`、`Windows.Media.Playback` |
| `src/provider/macos.rs`（通知、睡眠唤醒、网络变化） | `UNUserNotificationCenter`、`NSWorkspace` | WinRT Toast（需要 AUMID）、`WM_POWERBROADCAST`、`NotifyIpInterfaceChange` |
| `crates/chat-core/src/token.rs` | Keychain | Windows Credential Manager |
| `src/views/settings/login_item.rs`（开机启动） | `SMAppService` | `HKCU\...\Run` |
| `src/views/nav/menus.rs`、`status_item.rs` | 菜单栏、状态栏图标、关于面板 | 窗口内菜单加系统托盘图标 |
| `src/views/files/native.rs` | Quick Look、分享面板 | 用默认程序打开、资源管理器里定位、`DataTransferManager` 分享 |
| `src/ui/sf_symbols` | SF Symbols 栅格化 | Lucide 加几个自绘 SVG（已完成，见 C4） |
| `src/theme.rs`、`src/ui/motion.rs` | 系统外观、减弱动画 | `UISettings`、`SPI_GETCLIENTAREAANIMATION` |
| `src/views/settings/window_fit.rs` | NSWindow 尺寸和位置 | `GetMonitorInfo` 和工作区 |
| `src/provider/this_mac.rs`（用这台电脑） | 应用包里的 daemon 加 launchd | 安装目录里的 daemon 加计划任务，见 M3 |
| `src/updater` | DMG、`hdiutil`、替换 `.app` | 安装包加单独的更新程序 |
| `src/selftest/capture.rs`（截图自测） | NSWindow 编号、`screencapture` | 已完成：`PrintWindow` 加 `PW_RENDERFULLCONTENT`，见「怎么看 Windows 界面」 |

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
  已做（`apps/desktop/src/ui/native/window_chrome.rs`）：主窗口、登录、设置、团队管理窗口顶部有一条 32 高的
  标题条（gpui-component 的 `TitleBar`），左边是窗口标题，右边是最小化、最大化、关闭。标题条和三个按钮都是
  GPUI 的 `WindowControlArea`，拖动、双击最大化、拖到屏幕边缘贴靠、Windows 11 悬停最大化按钮出 Snap 布局
  都由系统处理；最大化时的边距由 GPUI 自己加。没有红绿灯，侧栏顶行用 Mac 全屏时的排法。Mac 上不画这条。
  不可调整大小的窗口（登录、设置）的最大化按钮画成灰的，点了没反应。
  还没做：产出窗口和文件预览窗口的标题条；应用菜单挂到标题条里。
  只在 CI 截图（100% 缩放）里看过，下面这些要真机确认：拖动和双击、Snap 布局、最大化时的边距、150% 和 200%
  缩放、多显示器之间拖动。
- 输入：中文输入法（候选窗位置、组合中的文字）；快捷键从 Cmd 映射到 Ctrl。
  快捷键已做：应用自己的绑定用 `crate::secondary!("k")`（Mac 是 `cmd-k`，Windows 是 `ctrl-k`；GPUI 的
  `secondary-` 前缀在 `KeyBinding::new` 里不能用，会 panic）。Windows 上不绑隐藏、最小化、表情与符号，也不绑 Ctrl+Q
  （退出用 Alt+F4 和关闭按钮）；Ctrl+W 关设置、团队管理这类窗口，不关主窗口和最后一个窗口（应用会跟着退出，
  等托盘做好后再改成收进托盘）；全屏是 F11，显示侧栏是 Ctrl+Alt+S，全局搜索是 Ctrl+Alt+F（Ctrl+Shift+F 是
  微软拼音的简繁切换）。自己读修饰键的地方用 `crate::command_key(&modifiers)`，不要直接读 `modifiers.platform`。设置 › 快捷键和界面文案里的快捷键写成 `Ctrl+Shift+K` 的样子。
  输入法还没看，要真机。
- 通知（点击打开话题）、睡眠唤醒和网络变化后立即重连、托盘图标、开机启动。
- 客户端测试在 Windows 上全过，去掉 CI 里的 `continue-on-error`。

完成标准：在 Windows 上登录，收发消息、看话题、收通知、断线重连都正常。

**C1 系统集成部分（已写完，Windows CI 跑过单元测试，没有在真机上看过）**

做了什么：

| 功能 | Windows 上的做法 | 代码 |
| --- | --- | --- |
| Token | Credential Manager 的普通凭据，目标名是 `<app id>:<服务器地址>`（和 Keychain 的 service、account 同一对），只存本机 | `crates/chat-core/src/token.rs` 的 `CredentialTokenStore` |
| 通知 | GPUI 自带的 WinRT Toast（`App::show_system_notification`）。没有包身份，GPUI 给进程设 AppUserModelID 并在 `HKCU\Software\Classes\AppUserModelId\<id>` 登记显示名，不需要开始菜单快捷方式，开发构建也走这条路。Toast 的 tag 带群和话题，点击后打开对应话题。同时闪任务栏按钮；声音由客户端自己播系统通知音（GPUI 的 Toast 是静音的） | `apps/desktop/src/provider/windows.rs` |
| 睡眠唤醒 | GPUI 的 `on_system_wake`（`WM_POWERBROADCAST`），再加时钟跳变兜底 | 同上 |
| 网络变化 | 每 3 秒用 `GetAdaptersAddresses` 比较一次网卡地址，和 Mac 的做法一样 | 同上 |
| 开机启动 | `HKCU\...\Run` 下以 app id 命名的值；同时读写 `Explorer\StartupApproved\Run`，所以在任务管理器里关掉后开关会显示关 | `views/settings/login_item.rs` |
| 托盘图标 | `Shell_NotifyIconW`，挂在一个隐藏的顶层窗口上。图标用菜单栏的模板图按任务栏深浅色染成白或黑，有未读时带点；左键打开主窗口，右键菜单是 Mac 上 Dock 菜单的几项加「设置…」和「退出」 | `views/nav/status_item_win.rs`、`tray_icon.rs`、`src/win32.rs` |
| 应用菜单 | GPUI 在 Windows 上的 `set_menus` 只把菜单存起来，不画。菜单列表去掉了只有 macOS 才有的项（服务、隐藏、前置全部窗口、表情与符号、空的帮助）；`menus::window_menu_bar` 返回 gpui-component 的 `AppMenuBar`，由窗口标题栏放进去 | `views/nav/menus.rs` |
| 减弱动画、高对比度 | `SPI_GETCLIENTAREAANIMATION`、`SPI_GETHIGHCONTRAST`，设置变化靠隐藏窗口收到的 `WM_SETTINGCHANGE` | `ui/motion.rs` |
| 文件操作 | 打开和定位本来就走 GPUI（Windows 上可用）。Quick Look 改为用默认程序打开，和「打开」走同一道检查：Windows 会运行、安装、挂载的类型（`exe`、`bat`、`scr`、`msi`、`hta`、`js`、带宏的 Office 文件等）和网页只在资源管理器里选中，文件名按 Windows 的读法判断（忽略末尾的点和空格）；分享改为在资源管理器里选中文件 | `views/files/native.rs` |
| 设置窗口高度 | `SetWindowPos`，顶边不动、不超出显示器工作区；首次定高前用 DWM cloak 隐藏；去掉最小化和最大化按钮 | `views/settings/window_fit.rs` |

过程中发现的：

- GPUI 的 Windows 平台已经有 Toast、唤醒通知、打开和定位文件、系统深浅色，这些不用自己写 Win32。
- GPUI 在 macOS 以外默认关掉最后一个窗口就退出（`QuitMode::Default`）。托盘图标在通知区域里时改成
  `QuitMode::Explicit`，关窗后程序留在托盘（和 Mac 一样），用托盘菜单的「退出」退出；图标被隐藏或没加上时
  保持默认，关掉最后一个窗口就退出，不会留下一个找不到的进程。
- GPUI 的 `hide_other_apps` 在 Windows 上是 `unimplemented!()`，调到就崩；对应的动作现在只在 macOS 注册。
- 系统强调色没有接：Windows 总有一个强调色（默认蓝），接了就不再是黑白规范，所以保持石墨色。
- 外观跟随系统不用改：`window.appearance()` 和它的变化通知在 Windows 上由 GPUI 提供。

这部分还没做的：

- `window_menu_bar` 还没有放进主窗口标题栏（标题栏在窗口那部分工作里），放进去之前菜单在窗口里没有入口。
- 快捷键没有动，`menus.rs` 里仍是 `cmd-…`，随整套键位一起映射。
- 任务栏按钮上的未读角标（`ITaskbarList3::SetOverlayIcon`，对应 Dock 角标）。
- 「关于」在 Windows 上打开设置窗口，没有单独的关于面板。
- 系统分享面板（`DataTransferManager`）留到 C3。
- 程序退出后，留在通知中心里的 Toast 点了没有反应（没有注册 COM 激活器）；安装包阶段（R1）可以一起做。
- Toast 没有群头像和按群分组。

要在真机上看的（CI 没法验证）：

1. Toast 能弹出，标题是产品名；点击后窗口到前台并打开对应话题；读完后通知中心里那条消失。
2. 专注助手打开时不弹 Toast，任务栏按钮仍然闪；设置里关掉声音后不响。
3. 睡眠再唤醒、切换 Wi-Fi 或拔网线后几秒内重连（日志里有 `system change`）。
4. 托盘图标在深色和浅色任务栏上都看得清，100% 和 150% 缩放下不糊；有未读时带点，悬停提示是未读数；
   右键菜单每一项都能用，「退出」后图标立刻消失；重启资源管理器后图标回来；设置里关掉后图标消失。
   关掉主窗口后程序还在托盘里，点图标能再打开；设置里隐藏图标后，关掉最后一个窗口程序退出。
5. 开机启动：打开开关后重新登录，程序自己启动；在任务管理器里禁用后，开关显示为关。
6. 登录后重启程序不用再登录（Token 在「凭据管理器 › Windows 凭据」里能看到一条，以 app id 开头）；退出登录后那条消失。
7. 设置窗口：切换分页时高度变化顺滑、顶边不动，打开时不闪一下再跳到位，没有最小化和最大化按钮；
   如果设置窗口打不开或看不见，先查 DWM cloak 那段。
8. 关闭系统的动画效果后，客户端的装饰动画停掉；打开对比度主题后颜色切到高对比。
9. 文件卡片上的预览用默认程序打开，分享会打开资源管理器并选中文件（路径里有空格和逗号也行）。

**C2 产出和图（代码已完成，Windows CI 上跑通；嵌入的手感还要真机看）**

做法：

- `crates/artifact-view` 增加 WebView2 后端（`src/win.rs`、`src/win_embed.rs`），对外接口和 macOS 一样
  （`ArtifactWindow`、`EmbeddedArtifact`、`DiagramRenderer`，主线程凭证统一叫 `MainThread`）。两个后端共用的
  判断（版本切换、桥、导航放行规则）抽到 `src/page.rs`。
- 绑定直接用 `webview2-com`（和 GPUI 同一个 `windows` 0.62），不用 `wry`：隔离要用到自定义 scheme 注册、
  对所有 URL 的请求过滤、各项设置开关、权限 / 下载 / 新窗口事件，`wry` 没有暴露这些。
- origin：把 `artifact` 注册成带 authority 的自定义 scheme（`CoreWebView2CustomSchemeRegistration`），页面地址
  和 macOS 完全一样：`artifact://<id>-v<version>/`，每个版本一个 origin；内容由 `WebResourceRequested` 用同一个
  `resolve::serve_range` 提供，CSP 和其他响应头逐字相同（Windows 另加一条 `Permissions-Policy`）。
- 断网三层：CSP；`WebResourceRequested` 过滤 `*`，当前版本 origin 之外的请求一律回 403；浏览器进程带
  `--host-resolver-rules="MAP * ~NOTFOUND"` 启动，任何主机名和 IP 都解析不了（WebSocket、WebRTC 不经过请求
  过滤，靠这一层和 CSP），并在每个 frame 里删掉 WebRTC 构造函数。
- 跳转：`NavigationStarting` / `FrameNavigationStarting` 取消 origin 之外的一切；`NewWindowRequested` 一律不开窗；
  外部协议（`LaunchingExternalUriScheme`）取消。
- 桥：`chrome.webview.postMessage`，只收主 frame、且来源是当前版本 origin 的消息，再交给同一个 `Bridge` 按
  `Capabilities` 检查。Host objects 关闭。
- 关掉的浏览器功能：下载、权限弹窗、脚本对话框、右键菜单、开发者工具（调试构建除外）、浏览器快捷键、
  自动填充和保存密码、缩放、滑动导航、状态栏、内置错误页、SmartScreen。
- 存储：每个页面都是 InPrivate；用户数据目录单独放在 `%LOCALAPPDATA%\team-chat\webview2`
  （`ui::web_embed::prepare_process` 设置）。
- 嵌入：GPUI 窗口的子 `HWND`（裁剪容器）里放 WebView2，每帧由宿主按布局摆放（物理像素 = 点 × 缩放）、裁剪、
  圆角（窗口 region）、隐藏。**GPUI 默认用置顶的 DirectComposition 树绘制，会盖住所有子窗口**，所以应用在
  Windows 上启动时设置 `GPUI_DISABLE_DIRECT_COMPOSITION=1`，让 GPUI 直接呈现到 `HWND`。
- 「用户点过」的信号：页面里按下鼠标时父窗口收到 `WM_PARENTNOTIFY`；非文字按键走 `AcceleratorKeyPressed`。
- 截图（图的 PNG、被遮住时的快照）走 DevTools 协议 `Page.captureScreenshot`，隐藏窗口固定 1 像素 = 1 CSS px。
- 桌面端接线：`ui/web_embed.rs`、`views/artifacts/window.rs`、`diagrams/mod.rs` 在 Windows 上走同一套代码；
  打开外部链接前的确认暂时用系统消息框。

在 Windows CI（`windows-2025`，任务 `artifact-view-windows` 和 `desktop-windows`）上实际跑过的：

- `tests/isolation.rs`：和 macOS 同一份页面、同一组断言（本 origin 文件能加载；http / https / ws / WebRTC /
  路径穿越 / 别的版本都拿不到；CSP 生效；跳转被拒；弹窗被拒；每个版本独立 origin；桥应答、拒绝未知方法、
  没有能力时拒绝 `emit` 和 `ask`）。
- `examples/win_embed_proto.rs`：普通窗口里嵌一个页面，对屏幕截图取像素验证位置、裁剪、圆角、隐藏和再显示、
  关闭；`SendInput` 真实点击到达页面并算作用户输入；独立的产出窗口和版本下拉框。
- `apps/desktop/examples/win_embed_gpui.rs`：真的 GPUI 窗口里嵌一个页面。关掉 DirectComposition 时页面显示在
  GPUI 内容之上，点击、裁剪、隐藏都对；不关时页面被 GPUI 盖住（截图里看不到，但仍能收到点击），这是必须关掉
  它的证据。
- `examples/diagram_selftest.rs`：8 个 Mermaid 样例在隐藏的 WebView2 里渲染并导出透明底的 2x PNG。

只能在真机上确认的（CI 是 100% 缩放、软件渲染、没有人操作）：

- 嵌入页面跟随滚动和窗口缩放时有没有闪烁、拖影；和 GPUI 弹层切换（隐藏换快照）时有没有闪一下。
- 焦点和 Tab 顺序：页面和 GPUI 输入框之间来回切换、Esc 和 Tab 交还焦点、中文输入法候选窗位置。
- 页面有焦点时应用级快捷键（Ctrl+W 等）现在会被页面吃掉，要决定哪些转给应用。
- 高 DPI 和多显示器不同缩放比：位置、清晰度、图的 PNG 尺寸。
- 话题里内嵌页面的滚轮转交（页面脚本把滚轮转成宿主的 `WM_MOUSEWHEEL`）手感是否自然。
- 只用键盘操作页面（不点击）时，文字按键不算「用户点过」，`emit` 可能被闸门挡住。
- 关掉 DirectComposition 对整个应用的影响（缩放窗口时的流畅度；不再支持窗口透明，应用没用到）。
- 版本下拉框是原生 COMBOBOX，外观要在 C4 里按设计规范重做；外部链接确认框同理。
- `<video>` 的分段加载（206）和自动播放。

安装包带 WebView2 Evergreen bootstrapper 属于 R1。

完成标准：两个平台通过同一套隔离测试（已满足，CI 上跑），Mermaid 图能显示（CI 上导出了 PNG；应用内显示待真机）。

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

- 已完成：`docs/design/WINDOWS.md`，只写和 NATIVE.md 不同的地方（标题栏和窗口按钮、窗口内菜单和托盘、
  Ctrl 快捷键写法、字体、图标、滚动条、对话框、设置窗口）；COMPLETENESS.md 两端通用。
- 已完成：图标。macOS 仍画 SF Symbols；其他系统用 Lucide（ISC，`third_party/lucide/`）加 7 个自己画的
  SVG（`assets/icons/drawn/`），调用处不用改。每个图标按对应 SF Symbol 的墨迹尺寸、线宽和位置来画
  （`ui/sf_symbols/svgset.rs`），新增概念没配 SVG 就编译不过，另有测试。在 Mac 上用 `TC_SVG_ICONS=1`
  看过主窗口、首页、设置的浅色和深色截图；没在 Windows 上看过。
- 已完成：字体。界面 Segoe UI Variable → Segoe UI，中文 Microsoft YaHei UI，等宽 Cascadia Mono → Consolas，
  字号不变（`theme.rs`）。只编译过。
- 待做：线条没有对齐到整像素，100 % 缩放下 1 px 的线可能发虚，要在真机或 Windows CI 截图上确认。
- 待做：`views/settings` 里两处直接用 `theme::MONO_FONT_FAMILY`（Windows 上是 Consolas），改成
  `theme::mono_font_family()`；内嵌网页的 CSS 字体栈（`ui/web_embed.rs`、`diagrams`、`views/artifacts`、
  `views/media`）加上 Windows 字体。
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

Windows CI 上实际看到的：四个用例都过；要求脱离 job 的进程被系统拒绝启动（Access is denied）；CI 里
daemon 有控制台，`CTRL_BREAK_EVENT` 送到了，假 CLI 以 `0xC000013A` 退出。

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
- **WebView2 嵌入**：原型已在 CI 上证实能显示，但前提是关掉 GPUI 的 DirectComposition（否则子窗口被盖住）；
  闪烁和焦点还要真机看，清单在 C2。
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
