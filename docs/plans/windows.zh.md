# Windows 端计划

状态：W0 已完成（客户端能为 Windows 编译并链接出 exe）。C1 的系统集成部分已写完（见 C1 一节）。M1 已完成
（daemon 能为 Windows 编译；Windows CI 上用假 CLI 跑通了一个 run、取消和暂停；还没在真的 Windows 电脑上跑过
真的 CLI）。M2 大部分已完成（访问列表、命名管道、审批、ConPTY、三个运行时的假 CLI 测试，都在 Windows CI 上
跑过；端到端用例还没进 Windows CI，原因见 M2 一节）。C2（产出和图的 WebView2 后端）代码已完成并在 Windows
CI 上跑通隔离测试，见下。其余进行中或未开始。

M3 已完成（daemon 作为当前用户的计划任务、改名替换的自己更新、「用这台电脑」、PowerShell 安装命令；计划任务和
安装命令在 Windows CI 上对着真的任务计划程序跑过，「用这台电脑」只编译过，都还没在真的 Windows 电脑上试过）。

R1（安装包、更新程序、发版流程）已完成：Windows CI 上真的装、更新、回滚、卸载了一遍；还缺证书、第一次真实
发版和真机确认，见 R1 一节。

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
| `src/views/media/native.rs`（音视频、PDF 预览） | AVFoundation、AVFAudio、PDFKit | Media Foundation 加 `Windows.Data.Pdf` |
| `src/provider/macos.rs`（通知、睡眠唤醒、网络变化） | `UNUserNotificationCenter`、`NSWorkspace` | WinRT Toast（需要 AUMID）、`WM_POWERBROADCAST`、`NotifyIpInterfaceChange` |
| `crates/chat-core/src/token.rs` | Keychain | Windows Credential Manager |
| `src/views/settings/login_item.rs`（开机启动） | `SMAppService` | `HKCU\...\Run` |
| `src/views/nav/menus.rs`、`status_item.rs` | 菜单栏、状态栏图标、关于面板 | 窗口内菜单加系统托盘图标 |
| `src/views/files/native.rs` | Quick Look、分享面板 | 用默认程序打开、资源管理器里定位、`DataTransferManager` 分享 |
| `src/ui/sf_symbols` | SF Symbols 栅格化 | Lucide 加几个自绘 SVG（已完成，见 C4） |
| `src/theme.rs`、`src/ui/motion.rs` | 系统外观、减弱动画 | `UISettings`、`SPI_GETCLIENTAREAANIMATION` |
| `src/views/settings/window_fit.rs` | NSWindow 尺寸和位置 | `GetMonitorInfo` 和工作区 |
| `src/provider/this_mac.rs`（用这台电脑） | 应用包里的 daemon 加 launchd | 安装目录里的 daemon 加计划任务，见 M3 |
| `src/updater` | DMG、`hdiutil`、替换 `.app` | 已完成：安装包加单独的更新程序（`apps/update-helper`），见 R1 |
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
| `service.rs`、`selfupdate.rs` | launchd、systemd；程序文件被替换后退出，由服务管理器重启 | 当前用户的计划任务（登录时启动，结束后一分钟内再启动）。不能用 Windows 服务：服务跑在别的会话里，读不到用户的 CLI 登录状态。更新时先把运行中的 exe 改名再放新文件。已完成，见 M3 |
| `health.rs`、`doctor.rs`（找 CLI） | 查 `PATH`、可执行位 | 按 `PATHEXT` 找，npm 装的 CLI 是 `.cmd` 文件，启动时要正确转义参数 |
| 文件权限（配置、token、运行目录） | 0600 / 0700 | 检查属主和访问列表（只许当前用户、SYSTEM、Administrators），daemon 自己建的目录和文件写明确的访问列表；机器 token 存 Credential Manager |
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
- 断网四层：CSP；`WebResourceRequested` 过滤 `*`，当前版本 origin 之外的请求一律回 403（页面已关或请求读
  不出来时也回 403，没有走默认路径的情况）；浏览器进程带 `--host-resolver-rules="MAP * ~NOTFOUND"` 启动，
  任何主机名都解析不了；所有连接（包括直连 IP 和本机）必须走一个连不上的代理（`--proxy-server` 指向
  `0.0.0.0:1`，去掉回环豁免），Chromium 对这样给的代理不会退回直连。
- WebRTC：macOS 在引擎里关掉；Windows 没有这个开关，靠上面的死代理加 `disable_non_proxied_udp`（UDP 不许
  绕过代理，TCP 也走代理，于是没有任何传输通道），再在每个 frame 里删掉构造函数；页面通过
  `contentWindow` / `contentDocument` 拿到的新 frame 窗口也当场清理。
- 跳转：`NavigationStarting` 取消 origin 之外的一切，`FrameNavigationStarting` 取消所有 frame 跳转（CSP 本来
  就不允许 frame，这样 frame 里只可能是 `about:blank` 或 `srcdoc`）；`NewWindowRequested` 一律不开窗；
  外部协议（`LaunchingExternalUriScheme`）取消。
- 桥：`chrome.webview.postMessage`，只收主 frame、且来源是当前版本 origin 的消息，再交给同一个 `Bridge` 按
  `Capabilities` 检查。Host objects 关闭。
- 关掉的浏览器功能：下载、权限弹窗、脚本对话框、右键菜单、开发者工具（调试构建除外）、浏览器快捷键、
  自动填充和保存密码、缩放、滑动导航、状态栏、内置错误页、SmartScreen。
- 存储：每个页面都是 InPrivate，并且有自己的 profile（关闭页面时删除），和 macOS 每个视图一个全新存储
  一致：同一 origin 的 localStorage 不会留到视图关闭之后，深浅色也按页面设置；用户数据目录单独放在 `%LOCALAPPDATA%\team-chat\webview2`
  （`ui::web_embed::prepare_process` 设置）。
- 嵌入：GPUI 窗口的子 `HWND`（裁剪容器）里放 WebView2，每帧由宿主按布局摆放（物理像素 = 点 × 缩放）、裁剪、
  圆角（窗口 region）、隐藏。**GPUI 默认用置顶的 DirectComposition 树绘制，会盖住所有子窗口**，所以应用在
  Windows 上启动时设置 `GPUI_DISABLE_DIRECT_COMPOSITION=1`，让 GPUI 直接呈现到 `HWND`。
- 「用户点过」的信号：页面里按下鼠标时父窗口收到 `WM_PARENTNOTIFY`，只有落在网页区域内的才算（点版本
  下拉框不算）；非文字按键走 `AcceleratorKeyPressed`。
- 滚轮转交（话题里的内嵌页面）：宿主读系统的原始鼠标输入（Raw Input），指针在这种页面上时把滚轮消息发给
  GPUI 窗口；页面发来的任何消息都不会让宿主滚动。
- `window.print()`：WebView2 没有关打印的开关，在每个 frame 里把 `print` 换成空函数。还剩一种情况没有堵死：
  页面如果能在脚本注入之前拿到一个新 frame 的窗口，可以调出打印预览；那只是一个需要用户自己确认的对话框，
  不泄露数据，所以没有再加更重的手段。
- 截图（图的 PNG、被遮住时的快照）走 DevTools 协议 `Page.captureScreenshot`，隐藏窗口固定 1 像素 = 1 CSS px。
- 图渲染器起不来时（比如电脑上没有 WebView2 运行时）：等着的图立刻显示「没有画出来」，隔 2 秒、4 秒……最多
  5 分钟再试一次，不会反复开关隐藏窗口；这些失败不写进磁盘缓存。
- 桌面端接线：`ui/web_embed.rs`、`views/artifacts/window.rs`、`diagrams/mod.rs` 在 Windows 上走同一套代码；
  打开外部链接前的确认暂时用系统消息框。

在 Windows CI（`windows-2025`，任务 `artifact-view-windows` 和 `desktop-windows`）上实际跑过的：

- `tests/isolation.rs`：和 macOS 同一份页面、同一组断言（本 origin 文件能加载；http / https / ws / WebRTC /
  路径穿越 / 别的版本都拿不到；CSP 生效；跳转被拒；弹窗被拒；每个版本独立 origin；桥应答、拒绝未知方法、
  没有能力时拒绝 `emit` 和 `ask`）。
- `tests/isolation.rs` 的「逃逸」页（两个平台同一份）：新建的 `about:blank` frame 里没有 WebRTC；对本机一个
  TCP 监听端口用 TURN-over-TCP、fetch、图片、WebSocket、beacon、弹窗、跳转各试一遍，监听端口一个连接都没
  收到；frame 自己发的桥消息被忽略；frame 加载本 origin 的页面被拒；地理位置、麦克风、通知、剪贴板都没有
  被授权；blob 下载没有落盘；别的应用的 URL scheme 被拒；关掉窗口再开同 origin 的新窗口，localStorage 是空的。
- `examples/win_embed_proto.rs`：普通窗口里嵌一个页面，对屏幕截图取像素验证位置、裁剪、圆角、隐藏和再显示、
  关闭；`SendInput` 真实点击到达页面并算作用户输入，点版本下拉框不算；真实滚轮在开启转交时滚动宿主，页面
  伪造的滚轮消息不起作用；独立的产出窗口和版本下拉框。
- `apps/desktop/examples/win_embed_gpui.rs`：真的 GPUI 窗口里嵌一个页面。关掉 DirectComposition 时页面显示在
  GPUI 内容之上，点击、裁剪、隐藏都对；不关时页面被 GPUI 盖住（截图里看不到，但仍能收到点击），这是必须关掉
  它的证据。
- `examples/diagram_selftest.rs`：8 个 Mermaid 样例在隐藏的 WebView2 里渲染并导出透明底的 2x PNG。

测试顺带发现的 macOS 问题（不在这次范围内，没有改）：在 macOS 15 的 CI 上，页面点了一个 blob 下载链接之后
脚本就不再运行（定时器不触发），应用本身不受影响，文件也没有保存；在 macOS 26 上页面照常运行。隔离测试
两个平台都检查「文件没落盘、没有连接出去」，「页面还活着」只在 Windows 上要求，macOS 上只记录。

只能在真机上确认的（CI 是 100% 缩放、软件渲染、没有人操作）：

- 嵌入页面跟随滚动和窗口缩放时有没有闪烁、拖影；和 GPUI 弹层切换（隐藏换快照）时有没有闪一下。
- 焦点和 Tab 顺序：页面和 GPUI 输入框之间来回切换、Esc 和 Tab 交还焦点、中文输入法候选窗位置。
- 页面有焦点时应用级快捷键（Ctrl+W 等）现在会被页面吃掉，要决定哪些转给应用。
- 高 DPI 和多显示器不同缩放比：位置、清晰度、图的 PNG 尺寸。
- 话题里内嵌页面的滚轮转交（Raw Input）手感是否自然，触控板的精确滚动是否也走这条路。
- 每个页面一个 profile 对打开速度和内存的影响；异常退出后留在用户数据目录里的 profile 文件夹要不要清理。
- 只用键盘操作页面（不点击）时，文字按键不算「用户点过」，`emit` 可能被闸门挡住。
- 关掉 DirectComposition 对整个应用的影响（缩放窗口时的流畅度；不再支持窗口透明，应用没用到）。
- 版本下拉框是原生 COMBOBOX，外观要在 C4 里按设计规范重做；外部链接确认框同理。
- `<video>` 的分段加载（206）和自动播放。

安装包带 WebView2 Evergreen bootstrapper 属于 R1。

完成标准：两个平台通过同一套隔离测试（已满足，CI 上跑），Mermaid 图能显示（CI 上导出了 PNG；应用内显示待真机）。

**C3 媒体和文件（约 2 周）**：视频首帧和时长、音频波形和播放、PDF 预览；打开、定位、分享文件。

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
  用卷序列号、文件索引、大小和写入时间（M3 改的：只看时间会被 NTFS 的文件名隧道骗过）。
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
  假 CLI（靠 `#!` 启动）、`/bin/sh`、信号和文件权限，M1 时只在 Unix 上跑。（M2 打开了 `runtimes.rs`。）
- **没有控制台时的取消**：`CTRL_BREAK_EVENT` 只能发给和 daemon 共用一个控制台的进程。M3 已定：计划任务用
  `conhost.exe --headless` 启动 daemon，daemon 和它启动的 CLI 共用一个没有窗口的控制台（CI 上确认了
  daemon 确实有控制台）。真的 CLI 在这个控制台里收到 `CTRL_BREAK_EVENT` 后能不能收尾，要在真机上看。
- **审批（gates）**：Claude Code 的 PreToolUse 钩子通过 `chat-run` 的 socket 问 daemon，M1 时 Windows 上
  没有这条通道，Windows 机器不上报 `gates` 能力。（M2 已解决。）
- **旁聊（aside）的只读目录**：M1 时只给已有文件加了只读属性，拦不住新建文件。（M2 已改成访问列表。）
- **自己更新**：M3 已做，见下面的「替换协议」。
- **机器 token**：仍然是用户目录下的文件，没有放进 Credential Manager（计划任务里的 daemon 也要读它，
  文件更简单）；M3 把它所在的文件夹改成只有当前用户、SYSTEM 和 Administrators 能打开。
- **要在真机上核实的**：真的 Claude Code 和 Codex 各跑一个 run；它们的 Windows 版把登录放在哪
  （`%USERPROFILE%\.claude`、`%USERPROFILE%\.codex`，还是 Credential Manager），daemon 清空环境变量后它们
  还能不能读到；Claude Code 在 Windows 上的会话目录名规则是否和上面一致；Claude Code 需要的 Git for
  Windows；杀毒软件对没签名的 daemon 和 Job Object 的反应。

**M2 功能齐全（大部分已完成，剩下的列在最后）**

先做的是 M1 评审提出的四个安全问题，它们不解决就不能把 Windows 机器真正用起来：

- **私有文件靠访问列表，不再靠「放在用户目录里」**（`winacl.rs`、`sys.rs`）。一个文件算私有，要求属主是
  当前用户（或 SYSTEM、Administrators），并且访问列表里每一条允许项都只给这三者。检查的地方：机器 token
  文件、配置文件、密钥库、`chat-run` 的请求目录和里面每一个请求文件。daemon 自己建的东西在**创建的那一下**
  就带上访问列表（`CreateDirectoryW` / `CreateFileW` 带安全描述符，文件用 `CREATE_NEW`），不是先建再收紧，
  所以不存在别人能钻进去、先放东西或先拿到句柄的空档：`<work_root>\runs`、每个 run 的目录（工作目录、
  请求目录、设置文件都在它下面，自动继承）、密钥库文件、旁聊的设置文件。同名的目录或文件如果已经在那里
  并且别人进得去，就拒绝使用，不接管。`work_root` 放在 `D:\runs` 这种默认谁都能写的地方也一样安全。放在
  做不到这一点的卷上（FAT、exFAT、多数网络共享），run 会被拒绝并说明原因；token 或配置别人能读，daemon
  不启动。还剩一个没堵的口：`work_root` 本身如果别人能写，别人可以把整个 `runs` 目录改名换掉，下一个 run
  会发现并拒绝，但正在跑的 run 按路径打开文件时有被换的可能。所以 `work_root` 仍然应该放在自己的目录里。
- **探测和登录用的子进程进 job**（`health::spawn`、`health::output`）：查版本、列模型、`claude --help`、
  用量、登录和登录状态这些子进程，各自在一个 Job Object 里，放弃时（超时、取消）连它启动的东西一起结束。
- **带换行的参数过不了非 npm 的 `.cmd`**：`cmd.exe` 的命令行到第一个换行为止，后面的会被当成别的命令。
  这种情况现在在启动前就失败，并说明要把运行时的 `command` 指到 `.cmd` 实际启动的程序。npm 的 `.cmd`
  不受影响（直接启动它指向的程序）。不带换行的参数由 Rust 标准库按 `cmd.exe` 规则转义，测试确认原样送到。
- **旁聊的只读目录**：在工作目录的访问列表里加一条对当前用户的「拒绝写」（写入、追加、建文件、建目录、
  改属性、删除），目录里已有的和以后建的都继承。和 Unix 的权限位一样，用户自己能手动改回去，只会写文件
  的 CLI 改不了。例外见下面「发现的事」。

然后是 M2 本来的内容：

- **`chat-run` 的命名管道**（`chat_run.rs`）：`\\.\pipe\chat-run-<run 目录的哈希>-<daemon 进程号>`，访问列表
  只给当前用户，拒绝远程客户端，要求自己是这个名字的第一个实例（别的程序抢先占名会让 run 的 `chat-run`
  起不来，而不是被冒充）。请求和回答的格式跟 Unix socket 完全一样，变量还是 `CHAT_RUN_SOCKET`。客户端
  以「只许识别身份」的方式连接，daemon 不能冒用它的身份。run 结束管道就没了。Windows 上现在和 Unix 一致：
  Codex 走文件，其他运行时走管道。
- **审批（gates）**：Windows 机器**仍然不上报** `gates` 能力。通道已经有了（管道），钩子在 Windows 上
  也改成了问不到 daemon 就拒绝（Unix 保持原样：问不到就放行）；带审批的 run 如果传输起不来，run 直接失败
  （两个平台一直如此）。但还有一种情况代码里堵不住：Claude Code 在 Windows 上如果根本没把钩子命令
  `"C:/.../chat-run.exe" gate` 启动起来（用哪个 shell、怎么处理引号，没在真机上见过），所有命令都会放行，
  而机器还在说自己会拦。所以在真机上确认钩子能被启动之前不打开，打开只需要改 `gates::enforced_by` 一行。
- **受控密钥**：`chat-run secret` 在管道和文件两种传输上都测过（要审批，不会直接给值）。
- **`pty` 运行时的 ConPTY 版**（`winpty.rs`、`runtime/pty.rs`）：程序在伪控制台里启动，同样先挂起、进 job
  再运行。标准库不支持给子进程挂伪控制台，所以这里自己调 `CreateProcessW`（命令行引号和环境块自己拼）。
  取消是往控制台里打一个 Ctrl+C，3 秒后结束 job。批处理文件不能当 `pty` 的程序（提示词会被 `cmd.exe`
  当命令读），会明确报错。
- **暂停和继续**：M1 已做；`pty` 的 run 现在也能被暂停。
- **`tests/runtimes.rs` 在 Windows 上打开**：Codex、ACP、Claude 三个运行时对着 Python 假 CLI 跑。Windows
  不认 `#!`，所以加了一个只在测试里编译的启动器 `fake-cli.exe`（`test-fakes` feature），它执行
  `python %FAKE_CLI_SCRIPT%`。
- **`daemon doctor`**：加了一个两个平台都跑的测试（按名字经 `PATHEXT` 找到程序、读到磁盘、私有的 token
  文件通过检查、最后是 OK）。

Windows CI（`daemon-windows`，`cargo test -p daemon --features test-fakes`）上实际跑过并通过的：85 个单元
测试（访问列表的检查、创建时即私有的目录和文件、请求文件过滤、命名管道往返、只读目录、审批钩子问不到时的两种行为、命令行引号和环境块）；`chat-run.exe`
经管道和请求目录要受控密钥；`daemon doctor`；`tests/process_tree.rs` 的 7 个用例（run、取消、暂停、npm 的
`.cmd`、非 npm 的 `.cmd`、子进程 job、ConPTY 的输出、输入、退出码、暂停和取消）；`tests/runtimes.rs` 的 12 个
用例（Codex 的审批、插话、续接、effort，ACP 的权限和追问，模型和 effort 的发现）；`normalize`、`redact`。

M2 里发现的事：

- **提升权限的进程不完全受只读目录约束**：Windows 建目录和删文件时带「备份意图」，持有恢复特权的进程
  （提升权限的管理员，比如 CI 的 runner）会跳过访问列表。写文件、建文件仍然被拒绝。普通用户身份的 CLI
  全部被拒绝。这和 Unix 上 root 不受权限位约束是一回事；结论是 daemon 不要以提升权限运行。
- **Codex 的 Windows 沙箱和私有目录可能冲突**：Codex 在 Windows 上如果用另一个受限身份跑工具，并且往工作
  目录的访问列表里加它自己的条目，请求目录就不再「只有当前用户」，`chat-run` 的请求会被丢掉。没有真机，
  没法确认；真机上第一件事就是看这个。
- **ConPTY 的输出是「画屏幕」**：程序用移动光标代替换行时，去掉转义码之后两行的字会连在一起。够用来看
  结果和判断退出码，不适合逐行解析。
- **登录时 CLI 打开的浏览器会跟着登录一起结束**：登录子进程在 job 里，它在这台机器上启动的浏览器也在 job
  里。这是有意的：登录链接是通过应用发给人的（`BROWSER` 设成不开本机浏览器），放弃的登录不该在机器上留下
  任何进程。
- **只读目录的测试在 Windows 上没有断言「删除」和「建目录」被拒绝**：原因就是上面第一条，CI 的 runner 是
  提升权限的。测试只证明了写文件和建文件被拒绝；普通用户身份下删除和建目录是否被拒绝，没有测试覆盖。

还没做的：

- **端到端用例没有接进 Windows CI**。没有在 Windows runner 上试过，所以不知道 runner 能不能跑
  `wrangler dev`；卡住的是脚本本身，不是 runner：
  - `tools/e2e` 的客户端是对着已部署的环境（staging）跑的，在 Linux CI 上也不跑。CI 里的端到端是
    `apps/daemon/e2e/local-e2e.ts`（`wrangler dev`、真 daemon、Python 假 CLI），只有 Linux 任务。
  - 这个脚本在 Windows 上会坏在四处：用 `process.kill(-pid, "SIGTERM")` 按进程组结束 wrangler 和 daemon
    （Windows 没有负的 pid）；把 `fake_claude.py`、`fake_codex.py` 直接写成运行时的 `command`（Windows 不认
    `#!`，要换成 `fake-cli.exe` 加 `FAKE_CLI_SCRIPT`）；程序路径没有 `.exe`；假 CLI 里有几种模式用
    `/bin/sh`、`setsid` 和文件权限位。
  - 要做的事：把这四处改成两个平台都能跑，再给 `daemon-windows` 加一步。`tools/e2e` 在 Windows 上能编译
    （CI 有 `cargo check -p e2e`），对着一台真的 Windows 机器跑它是 M2 的完成标准，要等真机。
- **`tests/integration.rs`、`tests/setup.rs` 仍然只在 Unix 上跑**（launchd/systemd、文件权限位、
  `/bin/sh`、信号）。其中和平台无关的用例（协议 fixture、配置解析）值得拆出来在 Windows 上跑。
- **机器 token 仍然是文件**（现在检查它的访问列表），没有放进 Credential Manager。
- **真机**：M1 列的那些都还在，加上上面 Codex 沙箱那一条，以及 Claude Code 在 Windows 上到底用哪个 shell
  执行钩子命令。

完成标准：`tools/e2e` 的机器用例对着一台 Windows 机器全过。

**M3 服务和「用这台电脑」（已完成）**

做了什么：

- **计划任务**（`apps/daemon/src/service.rs`）：`daemon install-service` 生成一份任务计划程序的 XML，用
  `schtasks /Create /XML` 注册成当前用户的任务（名字是 `<bundleId>.daemon`），马上启动；
  `uninstall-service` 结束并删除它。任务的内容：
  - 这个用户登录时启动，只在这个用户已登录的会话里跑（不存密码，用用户自己的令牌，不提权），所以 CLI 能
    读到用户的登录状态；
  - 只允许一个实例；另有一个每分钟触发一次的触发器：daemon 在跑时什么都不发生，daemon 不管因为什么结束
    （崩溃、被杀、发现新版本后自己退出），一分钟内被重新启动。这相当于 launchd 的 `KeepAlive`。只靠
    「失败后重启」不够：它不管退出码是 0 的情况，而且 daemon 是经过 conhost 启动的；
  - 没有运行时间上限，用电池、离开空闲状态都不停；
  - 启动的程序是 `conhost.exe --headless <daemon> run --config … --log-file …`：daemon 是控制台程序，
    直接启动会一直挂着一个黑窗口。无窗口的 conhost 给它一个真的控制台但不显示，run 启动的 CLI 继承这个
    控制台（它们也不弹窗），礼貌取消要用的 `CTRL_BREAK_EVENT` 也有了着落；
  - 没人看得到无窗口控制台的输出，所以 daemon 自己把日志写到 `%LOCALAPPDATA%\<slug>-daemon\daemon.log`
    （新增 `run --log-file`，只在 Windows 上有；超过 10 MB 启动时挪成 `daemon.log.1`）；
  - `PATH` 不像 macOS 那样在安装时存进去：计划任务每次启动拿到的是用户当前的环境变量。
- **停止和重启**：结束计划任务只会结束 conhost，后面的 daemon 还活着（CI 上第二轮看到的）。所以任务里的
  daemon 把自己的 pid 和进程创建时间写到 `%LOCALAPPDATA%\<slug>-daemon\daemon.pid`，并等一个命名事件
  `Local\<slug>-daemon-stop-<pid>`。`uninstall-service`、重新 `install-service` 和新增的
  `restart-service`（只在 Windows 上有）按 pid 加创建时间找到进程（只有 pid 可能已经是别的程序），设这个
  事件，daemon 像收到 Ctrl+C 一样收尾（取消并上报手上的 run）。事件只在 daemon 自己的登录会话里看得到：
  从同一用户的另一个会话（SSH、WinRM）来停，或者 20 秒还没退出，就直接结束进程。停不掉时命令报错，不会
  装作成功。
- **谁能换掉程序文件**（`apps/daemon/src/wintrust.rs`）：计划任务会一次次以这个用户的身份启动 daemon 的
  exe，所以这个文件、它的文件夹和往上每一层文件夹，只要有当前用户和管理员以外的人能改（比如装在
  `D:\Apps\X`，默认所有登录用户可改），`install-service` 就拒绝并说明原因。检查的是所有者和访问列表；
  「在盘符根目录下能新建文件夹」不算。它和 M2 的 `winacl::check` 问的不是一个问题：那边问「谁能打开」（私有
  文件，别人连读都不行），这边问「谁能改」（Program Files 里的程序别人可以读），所以是单独一个模块。
- **装之前先查**：Windows 上 `install-service` 先做 daemon 启动时会做的检查（token 在不在、配置和 token 是否
  只有当前用户、SYSTEM、Administrators 能打开），不过就不注册。否则任务每分钟启动一次、每次都马上退出，
  原因只写在日志里。
- **任务属于谁**：XML 里用当前用户的 SID，不用用户名（`whoami` 按控制台代码页输出，中文用户名会被读错）。
- **为什么用 XML 加 `schtasks`，不用任务计划程序的 COM 接口**：XML 是纯文本，在每个系统上都能单元测试；
  `schtasks` 每台 Windows 都有；daemon 不用为此写 COM 代码。不解析 `schtasks` 的输出（它跟着系统语言和
  代码页变，中文系统上是 GBK），只看退出码；任务定义另存一份在 `%LOCALAPPDATA%\<slug>-daemon\task.xml`，
  客户端读它来判断状态，和 macOS 上读 plist 一样。
- **没有管理员权限的用户**：受管的电脑上普通用户可能不许注册「登录时」触发器。注册失败时自动去掉这个
  触发器再注册一次，靠每分钟的触发器在登录后一分钟内启动。
- **替换协议**（`apps/daemon/src/selfupdate.rs`，`docs/machine-protocol.md` 里也写了）。Windows 上运行中的
  exe 不能覆盖、不能删，但能改名。所有更新方（客户端的更新程序、PowerShell 安装命令、手动）都这样做：
  1. 把新程序完整写到同一个文件夹的 `<名字>.exe.new`，校验 SHA-256；
  2. 把运行中的 `<名字>.exe` 改名成同一个文件夹里的 `<名字>.exe.old-<任意不重复的后缀>`；
  3. 把 `<名字>.exe.new` 改名成 `<名字>.exe`；这一步失败就把旧文件改回去。

  两次改名之间断电会留下「没有 `<名字>.exe`、只有 `.old-*`」的状态。这时没有 daemon 可以自救，所以由计划
  任务兜底：`install-service` 在日志旁边留一份 daemon 的副本（`repair.exe`，更新永远不碰它），任务的第一个
  动作运行 `repair.exe repair-program <daemon 路径>`：程序文件不在而 `.old-*` 在，就把最新的那个改名回去，
  然后第二个动作才启动 daemon。（先试过用一行 `cmd` 做，在 CI 上不生效，换成了自己的代码，各系统都有单元测试。）更新方不用为此做任何事；留下的 `.exe.new` 下次更新时覆盖。

  更新方不要停 daemon，也不要结束计划任务（那会取消正在跑的 run）。剩下的 daemon 自己做：每分钟看一次
  自己路径上的文件，指纹（卷序列号、文件索引、大小、写入时间）变了、连续两次一样、手上没有 run 时退出，计划任务
  一分钟内启动新文件；daemon 每次启动时删掉旁边的 `<名字>.exe.old-*`，删不掉的（旧进程还没退出）留到
  下一次。`bin\chat-run.exe` 是指向旧文件的硬链接，改名不影响它，daemon 启动时会重新建。
- **「用这台电脑」**（`apps/desktop/src/provider/this_mac.rs`，对外的函数没变，设置页不用改）：
  - daemon 是安装目录里和客户端 exe 放在一起的 `<slug>-daemon.exe`（**R1 的安装包要按这个名字放**）。
    debug 构建、或者 exe 在 cargo 的 `target` 文件夹里时一律当作没有 daemon，不会接管已安装的那一个；
  - 配置和机器 token 写在 `%USERPROFILE%\.config\<slug>\`（和 daemon 默认找的位置一致）。写 token 之前先用
    `icacls` 把这个文件夹的继承关掉，只留三条完全控制：当前用户、SYSTEM、Administrators（后两个本来就能读
    这台电脑上的任何文件），文件夹里之后建的文件都继承这三条。安装脚本写的、daemon 认可的是同一组，和
    M2 在 `sys.rs` 里的规则一致。
    token 只写进文件，不上命令行、不进日志；
  - 然后运行 `<slug>-daemon.exe install-service`（不弹窗口）。状态读 `task.xml`：没有就是没设置，里面的
    程序是自己旁边的 daemon 就是正常，是别处的就是「别处」。重启是运行 `<slug>-daemon.exe restart-service`；
  - 新登记的机器标签是 `windows`（Mac 上是 `mac`），名字取 `COMPUTERNAME`。
  - M2 的 ACL 规则已经在 main 上（`sys.rs` / `winacl.rs`）：daemon 启动时检查配置文件和 token 的所有者和
    访问列表，只认当前用户、SYSTEM、Administrators，客户端和安装脚本写的正是这一组。客户端不链接 daemon
    的代码，所以它这边仍然用 `icacls` 写；以后可以改成让 daemon 在 `install-service` 里调 `sys::protect`。
- **PowerShell 安装命令**（`scripts/install-daemon.ps1`，用法在 `docs/bots.md` 第 6 节）：给不装客户端的
  机器。下载 daemon（只接受 https）、用命令行给的 `-Sha256` 校验（下载时必填；哈希必须来自下载地址以外的
  可信来源，脚本不会去下载地址旁边取；不对就什么都不装）、写配置和 token（文件夹只有当前用户、SYSTEM 和 Administrators 能打开，token
  不回显地输入或从文件读，不上命令行）、跑 `doctor`、`install-service`。`-Update` 只按替换协议换程序。
  Windows PowerShell 5.1 能跑，不需要管理员权限。**R1 要出的发布产物**：
  `<slug>-daemon-<版本>-x86_64-pc-windows-msvc.exe` 和同名的 `.sha256`。

测试：

- 每个系统：任务 XML 的单元测试（用 XML 解析器逐项检查触发器、用户、单实例、无时限、电池、conhost 命令
  行、带空格和 `&` 的路径的转义，UTF-16 带 BOM）；`.old-*` 清理；客户端这边的状态判断、SID 解析、
  「只有安装的构建才有 daemon」。
- Windows CI（`daemon-windows` 任务，对着 runner 自己用户的真任务计划程序）：
  - `apps/daemon/tests/service_task.rs`：测试程序自己当替身 daemon。注册并启动任务 → 替身在跑且有控制台
    → 再触发一次不会有第二个实例 → `taskkill /F` 杀掉后被重新启动 → 按替换协议换程序文件（运行中的 exe
    改名成功），替身用 daemon 的同一段代码发现新文件后退出，任务启动新文件，新进程删掉旧文件 →
    `restart-service` 的那段代码让它停下再起一个新的 → 模拟更新中途断电（程序文件被改名走、进程被杀），
    任务的第一个动作把文件改回来再启动 → 卸载后
    任务没了、进程没了、过 75 秒也没有再被启动。
  - 同一个测试程序还检查：放在 `C:\Users\Public` 下的程序被 `install-service` 拒绝；注册下来的任务带着
    「登录时」触发器（用 SID 写的用户被任务计划程序接受了）。
  - `scripts/test-install-daemon.ps1`（用 Windows PowerShell 5.1 跑）：用刚编译的真 daemon 走一遍安装命令。
    SHA-256 不对、下载地址不是 https、下载时没给 `-Sha256`，都什么都没装；配置文件夹和 token 的访问列表里只有当前用户、SYSTEM 和 Administrators，且不继承上层；任务注册了，真 daemon 在跑、
    没有窗口、日志写出来了；`-Update` 之后真 daemon 自己发现新版本、退出、被任务重新启动、旧文件被删；
    日志里没有 token；卸载后任务和进程都没了。
  - 另有一步只报告不拦截：新建一个没有管理员权限的本地用户，用它运行 `install-service` 和
    `uninstall-service`。结果：都成功（退出码 0），带「登录时」触发器的任务普通用户可以给自己注册，没有走
    去掉触发器的退路。（受组策略管的公司电脑仍要在真机上看。）

还要在真机上看的：

- 真的注销再登录、重启电脑后 daemon 自己上线（CI 上任务是手动触发和每分钟触发器启动的，没有真的登录）。
- 睡眠、唤醒、合盖、切换电池之后任务还在，daemon 重新连上。
- 计划任务里的 daemon 启动的 Claude Code、Codex 能读到用户的登录（这是用计划任务而不用服务的原因）。
- `conhost.exe --headless` 在 Windows 10 22H2 上的表现（CI 是 Windows Server 2025）；真的 CLI 在这个
  控制台里的取消。
- 「用这台电脑」整条路：在设置里点一下，机器出现在列表里；用户名是中文时的路径；没有管理员权限的用户；
  公司电脑的组策略是否禁止普通用户建计划任务。
- 快速用户切换、同一台电脑两个用户各装一个；远程桌面会话。
- 杀毒软件和 SmartScreen 对没签名的 daemon、对「每分钟触发的计划任务」的反应。

留给后面的：

- 设置页「添加机器」对话框给出的安装命令还是 macOS / Linux 的 bash 版，Windows 版要在界面上加一个选择
  （界面归客户端那条线）。
- 日志只保留一份旧的（`daemon.log.1`），daemon 长时间不重启时不会轮转。

### 发布

**R1 安装包、签名、更新（已完成，等第一次真实发版和真机确认）**

做了什么：

- **安装包**：`scripts/build-windows-installer.sh`（Windows runner 上的 Git Bash）出
  `dist/<名字>-<版本>-setup.exe` 和 `.sha256`，NSIS 脚本在 `scripts/windows/installer.nsi`。只装给当前
  用户，不要管理员权限：文件在 `%LOCALAPPDATA%\Programs\<名字>`，注册表都在 `HKCU`（卸载项、URL scheme、
  AppUserModelID）。开始菜单快捷方式上带 AppUserModelID（等于 `product.json` 的 bundle id，和客户端
  进程设的一致，通知要靠它）。带 WebView2 Evergreen bootstrapper，只在系统里没有 runtime 时才运行。
  里面有客户端、更新程序和 daemon（`DAEMON=0` 可以不带）。客户端的 release 版是窗口程序（没有控制台），
  exe 里有图标（`assets/brand/AppIcon.ico`）和版本信息。
- **更新**：检查、下载、SHA-256 校验和界面不变；安装这一步在 `apps/desktop/src/updater/install/windows.rs`。
  运行中的 exe 不能被覆盖，所以由单独的更新程序 `apps/update-helper`（装在客户端旁边，
  `<slug>-update.exe`）来做：客户端把它复制到安装目录外面，启动它，自己退出；它等客户端进程结束，再校验
  一次安装包，写下日志（journal），把安装目录里的文件改名挪到旁边的备份目录，静默运行安装包，写标记文件
  并启动新版本。新版本启动后删掉标记和备份（和 Mac 同一段代码）。安装包失败、新版本提前退出或 90 秒内
  没删标记，更新程序就把备份放回去、恢复系统里显示的版本号、重新打开旧版本。
- **更新中途被打断**（更新程序被杀、用户注销、断电）：动任何文件之前，更新程序先把要做的事写进
  `%APPDATA%\<id>\updates\update-journal.json`，并在 `HKCU\...\RunOnce` 里登记
  `<更新程序> --recover <journal>`。恢复在三个时候跑：下次登录（RunOnce）、客户端下次启动、下一次更新
  开始前。恢复按 journal 里记的阶段做：旧文件还在挪的时候被打断，就把已经挪走的挪回来；安装包跑到一半，
  就清掉装了一半的文件再把备份放回去；新版本已经启动并删了标记，说明更新其实成功了，只清理 journal。
  更新程序还活着的时候恢复什么都不碰。恢复可以重复跑。
- **daemon 跟着换，但不停它**：按 M3 的「替换协议」。更新程序不挪 `<slug>-daemon.exe`（`--keep`），只往
  备份里复制一份；安装包把新 daemon 完整写成 `.exe.new`，把运行中的改名成 `.exe.old-<后缀>`，再把 `.new`
  改名到位（失败就改回去并中止）。回滚和恢复时，更新程序用同一个协议把备份里的旧 daemon 换回去，所以
  「程序文件被改名走、新文件还没到位」时被打断也能恢复。正在跑 bot 的 daemon 始终不会被杀。
- **为什么是单独的程序，不是让安装包自己做**：安装包不知道新版本有没有真的启动起来，回滚只能写在
  NSIS 脚本里，没法测。更新程序是普通 Rust 代码，整个流程的测试在 Mac、Linux、Windows 上都跑。
- **签名**：可选。`scripts/windows/sign.sh` 在设置了 `WINDOWS_SIGN_THUMBPRINT`（证书在用户证书库里）或
  `WINDOWS_SIGN_CMD` 时给客户端、更新程序、daemon、卸载程序和安装包签名；没设置就不签，安装时会有
  SmartScreen 提示。更新时的规则：已安装的版本有有效签名，只接受同一个发布者的有效签名；已安装的版本
  签了名但这台电脑不信任那张证书，只接受同一张证书签的；没签名的版本接受没签名或有效签名的安装包，
  签名对不上内容的一律拒绝。读不出签名（查询失败）算错误，不当成「没签名」。更新程序调用系统工具
  （PowerShell、reg）一律用 System32 里的绝对路径。
- **服务器**：版本信息按平台给。`UpdateInfo` 多了可选字段 `platform`，`GET /v1/updates/latest` 和下载
  接口多了可选参数 `platform`，上传 Windows 安装包用 `PUT /v1/admin/releases/:version/assets/windows`
  （新表 `release_assets`，DMG 还在原来的 `releases` 表）。不带参数就是 Mac，旧的 Mac 客户端和发版脚本
  完全不受影响；Windows 客户端只接受 `platform` 是 `windows` 的回答，所以永远不会拿到 DMG。邀请链接
  的下载页在 Windows 浏览器里给安装包。细节在 `docs/protocol.md`「Updates and downloads」。
- **发版**：`release.yml` 多了两个任务：Windows runner 上构建、签名、测试安装包；发版成功后把安装包加进
  GitHub Release 并上传到服务器（`scripts/release.sh <版本> --upload-only --platform windows`）。
- **CI**：`windows-installer` 任务（`windows-2025`）每次都真的跑一遍：release 构建，静默安装，检查文件、
  注册表、快捷方式和 AppUserModelID，启动装好的客户端（自报版本，再画一屏登录页），覆盖安装，静默卸载，
  检查没有残留；再构建下一个版本，让装好的客户端对着本地服务器（`wrangler dev`）自己更新过去，检查新
  版本在跑；再测一次回滚；再在安装目录被搬空的那一刻真的杀掉更新程序，检查 RunOnce 里登记了恢复，
  运行恢复后安装目录逐字节回到原样；最后用本次运行临时生成的证书把上面的安装和更新再跑一遍带签名的。没签名的
  安装包会作为产物 `windows-installer` 保留 7 天，有 Windows 电脑的人可以直接下载来试。
- `docs/INSTALL.md`、`docs/INSTALL.zh.md` 加了 Windows 一节，`docs/RUNBOOK.md` 加了发 Windows 版的说明。

还没做、要人来做的：

- **证书**：运营方买代码签名证书，按 `docs/RUNBOOK.md` 加两个 secret。在那之前发出去的是没签名的
  安装包。新证书在 SmartScreen 里也要积累一段时间信誉。
- **第一次真实发版**：先部署服务器（新表和新接口），再跑 Release workflow。
- **真机上看**：安装界面（中文）、SmartScreen 提示的实际样子、开始菜单和任务栏图标、点通知能不能回到
  客户端、点 `<scheme>://` 链接（现在只是打开客户端，还不会跳到链接指向的地方）、Windows 10 上没有
  WebView2 时 bootstrapper 的安装（CI 的机器自带 runtime，只走了「已经有」这条路）。
- **daemon**：CI 上验证的是 daemon 的文件按协议被换掉、回滚和恢复后回到原样；「正在跑的 daemon 发现新
  文件、空闲时退出、计划任务拉起新的」这一段是 M3 的代码，要和 M3（PR #134）合在一起后在真机上看一次。
  卸载时会先调 `<slug>-daemon.exe uninstall-service`。
- **读签名靠 PowerShell**：公司电脑如果禁了 PowerShell，读不出签名，更新会被拒绝（装的版本不变）。
  M3 的 `apps/daemon/src/wintrust.rs` 是直接调系统接口读签名的，合并后可以换成它。

完成标准：从上一个版本自动更新到新版本，客户端和 daemon 都换成新的。CI 上已经做到（daemon 换的是文件）。

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
