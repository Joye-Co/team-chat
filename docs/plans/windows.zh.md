# Windows 客户端计划

状态：W0 已完成（代码能为 Windows 编译并链接出 exe，还没在 Windows 上运行过）。待定的决定见文末。

## 目标

在 Windows 10/11（x64，之后加 ARM64）上发一个和 Mac 版功能一致、视觉达到同一验收标准的桌面客户端。
服务器、协议和 `chat-core` 不变；工作全部在 `apps/desktop`、`crates/artifact-view`、`crates/chat-core`
的平台层、发布脚本和 CI 里。

## 现状

可以直接复用的部分：

- 服务器（Cloudflare Workers）、`docs/protocol.md`、`crates/chat-protocol`：与平台无关。
- `crates/chat-core`：只有 Token 存储（`token.rs`，`security-framework`）和少量文件路径代码按 macOS 分了支。
- 界面：GPUI 0.3（`gpui-pre`）和 gpui-component 0.8 都支持 Windows（DirectX 渲染）；列表、消息、
  Markdown、输入框、弹层是纯 GPUI，预期能直接编译。
- 原生代码已经用 `cfg(target_os = "macos")` 隔开，很多模块在非 macOS 上本来就是空实现。

只在 macOS 上能跑、需要 Windows 实现的部分（按依赖 macOS 的程度排序）：

| 模块 | 现在用的 macOS 接口 | Windows 方案 |
| --- | --- | --- |
| `crates/artifact-view`（产出窗口、内嵌页面） | WKWebView、`WKURLSchemeHandler`、`WKContentRuleList`、子视图嵌入 | WebView2：自定义 scheme（`AddWebResourceRequestedFilter`）、请求拦截做网络封锁、子 HWND 嵌入 |
| `src/diagrams`（Mermaid 图） | `artifact_view::diagram::DiagramRenderer`，隐藏的 WKWebView | 同一个 WebView2 后端的隐藏实例 |
| `src/views/media/native.rs`（音视频、PDF 预览） | AVFoundation、AVFAudio、PDFKit | Media Foundation 加 `Windows.Data.Pdf`（WinRT）；或者两端共用的 ffmpeg 加 pdfium，见决定 D2 |
| `src/provider/macos.rs`（通知、睡眠唤醒、网络变化） | `UNUserNotificationCenter`、`NSWorkspace` | WinRT Toast（需要 AUMID）、`WM_POWERBROADCAST`、`NotifyIpInterfaceChange` |
| `crates/chat-core/src/token.rs` | Keychain | Windows Credential Manager（`keyring` crate 或直接用 `windows` crate） |
| `src/views/settings/login_item.rs`（开机启动） | `SMAppService` | `HKCU\...\Run`，MSIX 用 `StartupTask` |
| `src/views/nav/menus.rs`、`status_item.rs` | 菜单栏、状态栏图标、关于面板 | 窗口内菜单加系统托盘图标 |
| `src/views/files/native.rs` | Quick Look、分享面板 | 用默认程序打开、资源管理器里定位、`DataTransferManager` 分享 |
| `src/ui/sf_symbols` | SF Symbols 栅格化 | 自带一套 SVG 图标，见决定 D3 |
| `src/theme.rs`、`src/ui/motion.rs` | 系统外观、强调色、减弱动画 | 注册表里的 `AppsUseLightTheme`、`UISettings`、`SPI_GETCLIENTAREAANIMATION` |
| `src/views/settings/window_fit.rs` | NSWindow 尺寸和位置 | `GetMonitorInfo` 和工作区 |
| `src/updater`（DMG 下载、`hdiutil`、替换 `.app`） | DMG、`hdiutil` | 安装包加单独的更新程序，见决定 D1 |
| `src/selftest/capture.rs`（截图自测） | NSWindow 编号、`screencapture` | `PrintWindow` 或 `Windows.Graphics.Capture` |

## 阶段

每个阶段单独提 PR，按 AGENTS.md 的规则：每个 worktree 用自己的 `CARGO_TARGET_DIR`，测试跑过才算完成。

### W0 能编译（已完成）

实际情况比预计简单：GPUI、gpui-component 和 `chat-core` 不用改就能为 Windows 编译，原生模块在非 macOS
上本来就有空实现，所以没有做 `platform` 模块的大重构（等 W1 写第一个 Windows 实现时再按需要整理）。

- 桌面端只有 11 处编译错误，都是 Unix 专用的文件权限、`libc` 调用和进程组，已经按平台分开。
- 新增 `apps/desktop/src/paths.rs`：数据目录在 Windows 上是 `%APPDATA%\<app id>`，缓存在
  `%LOCALAPPDATA%`，下载目录在 `%USERPROFILE%\Downloads`。macOS 的路径不变。
- Windows 上暂时没有的功能：通知、产出窗口、Mermaid 图、媒体预览、自动更新、托盘、开机启动、
  「用这台电脑」。Token 暂时存在数据目录下的文件里（W1 换成 Credential Manager）。
- CI 增加 `desktop-windows` 任务（`windows-2025`）：编译桌面端，跑 `chat-protocol` 和 `chat-core` 的测试；
  桌面端自己的测试先只报告不拦截，W1 修到全过后去掉 `continue-on-error`。
- DMG 安装器的测试（要用 `hdiutil`）只在 macOS 上编译。

在 Mac 上检查 Windows 编译：`brew install mingw-w64`，`rustup target add x86_64-pc-windows-gnu`，然后
`cargo check --target x86_64-pc-windows-gnu -p desktop --all-targets`。正式的目标是
`x86_64-pc-windows-msvc`，以 Windows CI 为准。

不在这份计划里：`apps/daemon`（在成员的电脑上跑 bot，用了 pty、Unix socket 和进程组）和 `tools/e2e`
都不能为 Windows 编译。Windows 客户端不需要它们；「用这台电脑」要等 daemon 另外移植。

### W1 能用（约 2 周）

- Token 存到 Credential Manager；配置和缓存放到 `%APPDATA%` / `%LOCALAPPDATA%` 下的产品目录。
- 窗口：自绘标题栏、最小化/最大化/关闭按钮、拖动区、Snap 布局、高 DPI 缩放和不同缩放比的显示器。
- 输入：中文输入法（微软拼音、搜狗），包括候选窗的位置和组合中的文字；快捷键从 Cmd 映射到 Ctrl。
- 通知（Toast、点击打开话题），睡眠唤醒和网络变化后立即重连。
- 托盘图标、开机启动。
- `bun run dev:desktop` 支持 Windows，或者写一份 Windows 版的启动说明。

完成标准：在 Windows 上登录 sandbox，收发消息、看话题、收到通知、重连都正常；中文输入法没有问题。

### W2 产出和图（约 3 周，风险最大）

- `artifact-view` 增加 WebView2 后端，接口和现在的 WKWebView 后端一样：
  - 每个版本一个 origin：`artifact://` 换成 WebView2 认的 `https://<id>-v<n>.artifact.invalid` 虚拟主机，
    用 `SetVirtualHostNameToFolderMapping` 或 `WebResourceRequested` 提供内容。
  - 同样严格的 CSP；拦截所有 http/https/ws/file 请求和跨 origin 跳转。
  - `window.artifact.getData()` / `emitEvent` 改走 `chrome.webview.postMessage`，按 `Capabilities` 检查。
  - 嵌入宿主窗口：子 HWND 每帧按 GPUI 布局定位，处理焦点和 Tab 键切换。
- 没装 WebView2 Runtime 的机器（老的 Windows 10）：安装包带 Evergreen bootstrapper。
- Mermaid 渲染器用同一个后端。
- 把现有的隔离测试移植过来（不能联网、不能跳转、没有授权的调用被拒绝）。

完成标准：产出窗口和内嵌页面在两个平台上通过同一套隔离测试，Mermaid 图能显示。

### W3 媒体和文件（约 2 周）

- 视频首帧和时长、音频波形和播放、PDF 页面预览。
- 文件操作：用默认程序打开、在资源管理器里显示、分享。

完成标准：sandbox 里的样例产出在 Windows 上都能预览。

### W4 打包、签名、更新（约 2 周）

- 安装包（按 D1 选 MSIX 或 NSIS），带 WebView2 bootstrapper。
- 代码签名（证书由运营方提供，放在 CI secret 里，不进仓库）。
- 更新器：下载、校验 SHA-256（沿用现有逻辑），由一个小的更新程序在主程序退出后替换文件再重启。
- `release.yml` 增加 Windows 任务，发版产物和 Mac 的 DMG 放在一起。

完成标准：从上一个版本自动更新到新版本，SmartScreen 不拦截（或者有说明怎么放行）。

### W5 设计验收（约 2 周，可以和 W2–W4 同时做）

- 写 `docs/design/WINDOWS.md`：标题栏、窗口按钮、菜单、托盘、字体（微软雅黑 / Segoe UI Variable）、
  圆角、滚动条、对话框，作用相当于 NATIVE.md 的 Windows 版。COMPLETENESS.md 的黑白 token 和尺寸规范照用。
- 准备图标：一套 SVG 图标，覆盖现在用到的 SF Symbols（约 40 个文件）。
- 截图自测移植到 Windows，浅色和深色都截。
- 每个页面过一遍 COMPLETENESS.md §4 清单，再用 `docs/design/skills/emilkowalski/` 里的四个技能检查。

完成标准：每个页面在 Windows 上通过 §4 清单。

总计约 10–12 周（一个人全职），W5 和 W2–W4 同时做能缩短到 8–9 周左右。

## 风险

- **GPUI 是预发布版本**（`gpui-pre =0.3.10`）：Windows 上的输入法、DPI、透明效果问题可能需要给上游打补丁，
  或者先在本仓库 vendor。W0 时先在真机上跑 gpui-component 的示例确认。
- **WebView2 嵌入**：子 HWND 和 GPUI 的 DirectX 交换链叠在一起，可能出现闪烁、Z 顺序或焦点问题。
  W2 第一周先做一个只嵌入一个页面的原型，确认可行再往下做。
- **代码签名**：没有 EV 证书时，SmartScreen 需要积累一段时间的信誉才放行，影响首批用户。
- **没有 Windows 开发机**：CI 能编译和跑测试，但界面要真机看。需要一台 Windows 机器或者虚拟机（Mac 上 ARM 版 Windows 可以先用）。

## 待定的决定

- **D1 安装包格式**：MSIX（系统管更新和卸载，开机启动用 `StartupTask`，但沙箱限制多）还是
  NSIS/WiX（灵活，自己写更新程序）。倾向 NSIS 加自己写的更新程序，和 Mac 的 DMG 流程一样。
- **D2 媒体预览**：用 Windows 原生接口（安装包小，两套代码），还是 ffmpeg 加 pdfium 两个平台共用
  （一套代码，安装包大约多 30–60 MB，Mac 端也要换）。倾向原生，保持 Mac 端不动。
- **D3 图标**：Windows 上用 Fluent UI System Icons（MIT 协议，和 Windows 风格一致），还是两个平台
  统一用一套（比如 Lucide），连 Mac 端也换掉。
- **D4 设计方向**：Windows 版看起来像 Windows 应用（Fluent 风格的标题栏和控件），还是和 Mac 版一模一样。
  影响 W5 的工作量和 COMPLETENESS.md 要不要分平台写。
- **D5 最低系统版本**：只支持 Windows 11，还是也支持 Windows 10 22H2（需要处理 WebView2 Runtime 的安装和
  没有 Mica 效果的情况）。
