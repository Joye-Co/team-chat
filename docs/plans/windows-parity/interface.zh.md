# Windows 对齐清单：界面

范围：`apps/desktop/src/**` 里界面这一块（`app.rs`、`main.rs`、`theme.rs`、`ui/**`、`views/**`（不含
`nav`、`settings`、`files`）、`diagrams/**`、`markdown/**`、`selftest/**`、`characters/**`、`drafts.rs`）和
`crates/artifact-view/**`。系统集成、机器、发布各有自己的清单（同一目录下）。

目标：Mac 上用户能做的每一件事，Windows 上结果相同。Windows 没有对应设施的，做一个结果相同的替代并写明原因。

状态的写法：

- **一样**：Windows 上结果相同（或只有 `docs/design/WINDOWS.md` 允许的差别：标题栏、窗口按钮、Ctrl 键位、字体、图标集）。
- **已修**：原来有差距，已经改好，括号里是 PR。
- **待修**：有差距，还没改。
- **没有对应**：Windows 没有这个系统设施，写了原因和替代做法。
- **真机**：代码和 CI 都判断不了，步骤在最后一节。

证据来源：逐个读了范围内所有按平台分支的代码（`cfg(target_os)`、`cfg(windows)`、`cfg!(…)`、直接读
`modifiers.platform` 的地方、每个 macOS 框架调用）；在这台 Mac 和 Windows CI（`windows-2025`，100% 缩放，软件渲染）
上各跑了全部截图场景（第一轮 270 个，现在 273 个），浅色深色各一遍，逐对比较；按 `docs/PRODUCT.md` 的功能逐项读到 GPUI 的 Windows
平台实现（`gpui-pre-windows 0.3.10`）。

## 1. 进程和窗口

| 项 | Mac | Windows 原来 | 状态 | 做法 |
| --- | --- | --- | --- | --- |
| 高分屏、系统画的控件的样式 | 系统按 2 倍绘制 | exe 带着 GPUI 的应用清单（PerMonitorV2、通用控件 6；`gpui-pre-platform` 打开了 GPUI 的 `windows-manifest`），所以是 DPI 感知的。CI 是 100%，截图证明不了这一点 | 一样 | 截图自测现在检查进程确实是 PerMonitorV2，不是就失败（安装包测试也跑这一步），以后清单丢了会被发现。其他缩放比例下的样子要真机看 |
| 主窗口、登录、设置、团队管理的标题栏 | 红绿灯在 64 高的工具栏行里 | 工具栏行上面多一条 32 高的标题条（主窗口放菜单，其余放窗口标题），右边是最小化、最大化、关闭 | 一样（标题栏差别） | 记录的决定：标题条保留。Mac 的菜单在屏幕顶部，Windows 的菜单要有地方放；`WINDOWS.md` §1 原来写的「一行」改成实际做法 |
| 拖动窗口、双击缩放 | 64 高的工具栏行（它就是标题栏）、侧栏顶部、团队管理页头都能拖 | 32 高的标题条能拖、双击最大化、拖到边缘贴靠；它下面的工具栏行不能拖 | 一样（标题栏差别） | 记录的决定：Windows 应用靠标题栏拖动，标题栏下面的工具栏不是拖动区。把工具栏行也标成拖动区的话，行里每个控件的点击都要经过系统的标题栏消息，没有真机验证不了，不值得冒这个险 |
| 产出窗口的标题栏 | 系统标题栏（标准 `NSWindow`），跟随应用的浅色深色 | 系统标题栏，但永远是浅色；没有窗口图标 | 已修（#162） | Mac 上这个窗口本来就是系统标题栏，所以 Windows 也用系统标题栏：跟随应用主题（DWM 深色；Windows 11 上底色、文字、边框用和应用标题条相同的 token），带应用图标。Windows 10 只有深浅两种，颜色是系统的 |
| 产出窗口的版本选择 | 标题栏右侧的 `NSPopUpButton` | 页面上方 40 高的白色条里放一个 `COMBOBOX`，深色下仍是白的 | 已修（#162） | 条的底色和标题栏相同，下拉框用系统的深色 / 浅色样式。没有对应：Windows 的系统标题栏里放不了控件，所以它在标题栏下面的条里 |
| 产出窗口里页面的浅色深色 | 跟随应用的选择 | 跟随系统，不跟随应用里选的浅色 / 深色 | 已修（#162） | 打开时和主题变化时设置页面的 `prefers-color-scheme` |
| 系统画的菜单（托盘菜单、窗口的系统菜单）、系统控件的下拉列表 | 跟随应用的选择（`NSApp.appearance`） | 跟随系统 | 已修（#162） | `uxtheme` 的 `SetPreferredAppMode`（序号 135，资源管理器、终端、记事本都在用）：应用选深色就强制深色，选浅色就强制浅色，跟随系统就跟随 |
| 打开外部链接、接管前的确认 | `NSAlert`，按钮是「打开」「取消」，回车是「打开」 | 系统消息框，按钮是系统语言的「确定 / 取消」（英文系统是 OK / Cancel） | 已修（#162） | 任务对话框（`TaskDialogIndirect`），按钮是应用自己的「打开」「取消」。和 Mac 有一处故意不同：**默认按钮是「取消」**，而且对话框出现后的 0.7 秒内「打开」按不动。这个对话框是页面离开沙箱的唯一出口，什么时候弹由页面决定，它可以趁用户正在打字时弹出来，已经按下去的 Enter、空格或点击不能变成「打开」。有测试（`the_link_dialog_cancels_by_default_and_holds_its_agree_button`）。进程没有通用控件 6 时退回消息框，默认也是取消 |
| 关闭主窗口的快捷键 | ⌘W 关主窗口，应用留在 Dock | 托盘图标显示时 Ctrl+W 关主窗口，应用留在托盘；托盘隐藏时不关（关了应用就退出了） | 一样（系统集成那边改的，`app.rs` 里按 `nav::close_command_closes` 判断） | Dock 图标在 Mac 上总在；Windows 上「关了窗还能回来」取决于托盘图标 |
| 再次启动应用 | 回到已运行的那个 | 起第二个进程 | 系统集成清单 | 那边在做单实例（`provider/instance.rs`） |
| 系统强调色 | 跟随系统设置；没选过就是石墨色 | 跟随系统里选的强调色；Windows 自带的两种蓝算「没选过」，也是石墨色（`theme.rs` 的 `read_system_accent`、`chosen_accent`、`watch_system_accent`） | 一样（系统集成那边改的） | 一台新 Windows 总有强调色（默认蓝），照搬就不是黑白规范了；所以只有用户自己选过的颜色才用 |
| 减弱动画、高对比度 | `NSWorkspace` | `SPI_GETCLIENTAREAANIMATION`、`SPI_GETHIGHCONTRAST` | 一样 | |
| 数据、缓存、下载目录 | `~/Library/…`、`~/Downloads` | `%APPDATA%`、`%LOCALAPPDATA%`、`%USERPROFILE%\Downloads` | 已修（#166） | 「下载」取系统登记的位置（`FOLDERID_Downloads`，用户可能把它移到别的盘）。存不进去时提示「没能存到「下载」」：原来两个系统都静默失败，这条提示 Mac 上也会出现（只在出错时） |

## 2. 键盘

所有应用自己的绑定都经过 `crate::secondary!`（⌘ → Ctrl），自己读修饰键的地方用 `crate::command_key`。
范围内直接读 `modifiers.platform` 的只有两处，都是对的：`views/composer.rs`（按平台分支）和
`views/refs/keyboard.rs`（两个修饰键任一按下就不处理）。字面的 `cmd-` 只在 Mac 分支里。

| 键 | Mac | Windows | 状态 | 做法 |
| --- | --- | --- | --- | --- |
| 设置、新话题、单独问、话题栏、首页 / 成员 / 产出、团队管理、话题内查找 | ⌘, ⌘N ⌘; ⌥⌘R ⇧⌘H/M/O ⌥⌘, ⌘F ⌘G ⇧⌘G | Ctrl 对应键 | 一样 | |
| 显示侧栏 | ⌃⌘S | Ctrl+Alt+S | 一样 | Windows 只有一个命令键 |
| 全局搜索 | ⇧⌘F | Ctrl+Alt+F | 一样 | Ctrl+Shift+F 是微软拼音的简繁切换 |
| 退出 | ⌘Q | Alt+F4、关闭按钮、托盘的「退出」 | 没有对应 | Windows 应用不绑 Ctrl+Q |
| 在窗口各区域之间移动焦点 | F6、⌥Tab | F6 | 已修（#156） | Alt+Tab 是系统的窗口切换，应用收不到；只在 Mac 上绑 ⌥Tab |
| 查找下一个 / 上一个 | ⌘G、⇧⌘G | Ctrl+G、Ctrl+Shift+G，另加 F3、Shift+F3 | 已修（#156） | Windows 的习惯键 |
| 提问列表里上下移动 | ↑↓、⌃P、⌃N | ↑↓ | 已修（#156） | Windows 上 Ctrl+N 是新话题，原来在这个列表里被 ⌃N 抢走 |
| 发送、换行 | ↵、⌘↵、⇧↵ | Enter、Ctrl+Enter、Shift+Enter | 一样 | |
| 加粗、斜体、删除线 | ⌘B、⌘I、⇧⌘X | Ctrl+B、Ctrl+I、Ctrl+Shift+X | 一样 | |
| 输入框里的编辑键 | ⌥←→、⌘←→、⌘↑↓、⌥⌫、⌘Z、⇧⌘Z | Ctrl+←→、Home/End、Ctrl+Home/End、Ctrl+Backspace、Ctrl+Z、Ctrl+Y | 已修（#166） | 另加 Windows 的第二套习惯键：Ctrl+Shift+Z 重做、Shift+Insert 粘贴、Ctrl+Insert 复制 |
| 右键菜单的键盘入口 | 没有（Mac 没有这个键） | 菜单键、Shift+F10 没反应 | 一样 | Mac 上右键菜单只能用鼠标开，Windows 上也是。菜单里的每个动作都有别的入口（悬停工具条、快捷键） |
| 图的查看器缩放 | ⌘= ⌘- ⌘0，⌘ 加滚轮 | Ctrl+= Ctrl+- Ctrl+0，Ctrl 加滚轮 | 一样 | 小键盘的 + - 要真机确认 |
| 引用提问 | ⌥ 悬停、⌥ 点击、⌥↵、⇧⌥↵ | Alt 对应键 | 真机 | Alt 松开会不会把焦点带到菜单栏 |
| 页面有焦点时的应用快捷键 | ⌘K、⌘N、⌘, 仍然有效（这些键先到菜单栏） | Ctrl+K、Ctrl+N、Ctrl+, 被内嵌页面吃掉 | 已修（#162） | 应用里全局绑定的键（带 Ctrl 或 Alt 的，以及功能键；从 GPUI 的键位表里读，设置里改了键也跟着变）在页面有焦点时交还给应用窗口，同时把键盘焦点还给应用，所以接下来打的字（比如切换器里的搜索词）不会进页面。复制、粘贴、撤销、全选留给页面。CI 用真实按键验证：`hostKeyGivesTheHostTheKeyboard`、`pageHearsNeitherTheHostKeyNorWhatFollows` |
| 界面上写的键 | ⌘ ⇧ ⌥ ↩ esc | `Ctrl+Shift+K`、Enter、Esc | 已修（#156） | 提问列表和切换器页脚的 ↩、esc 原来没有分系统 |

## 3. 功能

| 功能 | Mac | Windows 原来 | 状态 | 做法 |
| --- | --- | --- | --- | --- |
| 话题列表、话题页、群、成员页、时间线、卡片和卡片动作、产出列表、搜索、切换器、单独问、草稿、Markdown、代码块复制 | | 同一份代码，依赖的平台调用（打开链接、打开文件、定位、打开 / 保存对话框、文本剪贴板、拖入文件）Windows 都有实现 | 一样 | |
| 成员头像（上传的照片） | 显示 | 永远是字母：判断「是不是本机文件」用的是路径以 `/` 开头，`C:\…` 不算 | 已修（#156） | 改成绝对路径判断，有测试 |
| 角色头像（AI 成员） | 显示 | 一样；CI 截图里是字母，因为开源构建没有私有图包 | 一样 | |
| 输入框的「截图」 | 调 `screencapture -i`，截完变成附件 | 菜单项在，点了没反应 | 已修（#166） | 没有对应的命令行工具：启动系统的截图（`ms-screenclip:`，就是 Win+Shift+S 那个），它把图放进剪贴板，输入框等剪贴板变化后把图变成附件；取消截图就什么都不发生 |
| 粘贴：带图片的文字（Excel、WPS 的单元格等） | 只粘贴文字 | 贴成一张图片，文字丢掉（Windows 剪贴板同时给文字和图片） | 已修（#166） | 有文字、没有文件时不取图片，和 Mac 一样；有测试 |
| 粘贴：截图 | PNG | 剪贴板只有位图时附件是没压缩的 BMP（1080p 约 8 MB） | 已修（#166） | 粘贴时把 BMP 转成 PNG；有测试 |
| 复制图片（图片查看、图的查看器） | 任何应用都能粘贴 | 只写了「PNG」格式，只认位图的应用（旧画图、部分聊天软件）粘贴不了 | 已修（#166） | 同时写位图（`CF_DIB`，透明处垫白）；Windows CI 上对着真的剪贴板测 |
| 拖入文件 | 可以 | 可以（`CF_HDROP`） | 一样 | 从浏览器拖出的图片（虚拟文件）要真机确认 |
| 图：存到本机、用系统应用打开 | 可以 | 标题里有 `? * " < > \|` 或以点、空格结尾时写文件失败，按钮没反应 | 已修（#156、#162） | 只在 Windows 上把这些字符换掉、去掉结尾的点和空格、避开保留名；Mac 上文件名和以前完全一样（两边各有测试） |
| 视频播放器的画中画 | 可以 | 按钮没反应（用的是 WebKit 专有接口） | 已修（#156） | 没有 WebKit 接口时用标准的 `requestPictureInPicture`；两个都没有就不显示按钮。真机确认 |
| 视频：系统没有解码器（没装 HEVC 扩展时的 HEVC、MOV） | 能解的都能预览；解不了时显示「预览失败」和「这种格式没法在应用里打开」，带「用系统应用打开」 | 同一个状态和同一句话，但 Windows 上原因通常是缺扩展 | 已修（#166） | Windows 上视频解不出来时写「这台电脑没有这种视频的解码器，没法在应用里播放」，按钮不变 |
| 音频、PDF、图片、代码、表格预览 | AVFoundation、PDFKit | Media Foundation、`Windows.Data.Pdf` | 一样 | CI 上跑同一套测试 |
| 内嵌页面和预览的字体 | 系统字体 | 视频播放器、表格预览、引用提问按钮、样例页面只写了苹果字体，非中文系统上汉字字形会错 | 已修（#156） | CSS 字体栈加上 Segoe UI Variable Text、Segoe UI、Microsoft YaHei UI、Cascadia Mono、Consolas |
| 页面里的右键菜单 | WebKit 自带菜单（拷贝等） | 没有菜单，只能 Ctrl+C | 已修（#162） | 只留复制、剪切、粘贴、全选；会离开页面的项（后退、刷新、另存为、打印、共享、搜索）去掉 |
| 页面里打字算不算「用户操作过」 | 任何按键都算 | 只有点击和非文字按键算，只打字的人发出的 `emit` 可能被挡 | 已修（#162） | 页面有焦点期间读系统的原始键盘输入（只看「有键按下」，不看是哪个键；页面伪造不了），算作操作。CI 上用真实按键验证 |
| `window.print` | 没有打印面板 | 换成空函数 | 一样 | 剩下的那个口子见计划 C2 |
| 页面的下载、弹窗、权限、外部协议 | 都拒绝 | 都拒绝 | 一样 | 两个平台同一套隔离测试 |
| 内嵌页面的圆角 | 抗锯齿 | 窗口 region，整像素台阶 | 没有对应 | 子窗口没有抗锯齿的裁剪；要平滑得换成 WebView2 的合成模式，那需要 GPUI 用 DirectComposition，而它正好盖住子窗口（计划 C2）。半径 8，100% 下台阶 1 到 2 像素 |
| 页面的滚动条 | 细的浮动滚动条 | Chromium 的经典滚动条，带箭头，占 17 宽 | 已修（#162） | 浏览器进程打开 Windows 11 的浮动滚动条（`FluentOverlayScrollbar`），页面样式里再给一份细滚动条的默认值（放在层里，页面自己的规则优先） |
| 接管（`vnc://`） | 打开系统的「屏幕共享」 | Windows 没有自带的 VNC 客户端，确认后什么都不发生 | 没有对应，已做替代（#166） | 没有程序注册这个协议时不再问「打开吗」，直接说这台电脑上没有能打开屏幕共享的应用、要装一个 VNC 查看器，并把地址放进剪贴板；装了就和 Mac 一样先确认再打开 |
| 登录的设备的图标 | 按名字认 iMac、Mac mini、MacBook、iPhone | Windows 电脑一律是笔记本图标 | 已修（#156） | `DESKTOP-…` 是台式机，Android 是手机。设备名来自服务器，所以这条两个系统都生效：Mac 用户的设备列表里出现 Windows 电脑时图标也对。有测试 |
| 中文输入法（输入框） | 候选窗贴着光标；候选时 Enter 上屏不发送 | 走 GPUI 的 Windows 输入法实现（`WM_IME_*`，候选窗定位在光标处）；候选中的按键是 `VK_PROCESSKEY`，GPUI 不当成按键分发，输入框自己也检查「正在组字」再决定发不发送（`views/composer.rs`） | 真机 | 第 7 节第 1 条。代码路径齐全，但候选窗位置和 Enter 的行为只有真的输入法能验证 |
| 实时查看：终端、回放、计划、缩略图 | 右侧面板里看 AI 成员在做什么 | 同一份代码，全部是 GPUI 画的文字和图片，没有按平台分支 | 一样 | 场景 `live`、`core-live`、`live-takeover`、`panel-run`、`panel-plan`、`panel-failed` 在 Windows 上通过，逐对比较只有字体和内容区矮 32 的差别。只有「接管」里的 `vnc:` 地址不同，见下一行 |
| 团队管理「添加电脑」的说明 | 「在 Mac 或 Linux 上…」 | 同一句话 | 已修（#166） | 文案加上 Windows。对话框里的安装命令（现在只有 bash 的）在 `views/settings` 和 `provider`，归系统集成清单 |
| 抓手光标（图的查看器拖动） | 张开的手 | 箭头 | 没有对应，已做替代（#178） | Windows 没有抓手光标；GPUI 的 Windows 层只有箭头、I 形、十字、手、调整大小和禁止这几种。用系统的手形（`IDC_HAND`）表示「可以拖」；拖动本身一样 |

## 3a. 按平台分支的代码

范围内每一处 `cfg(target_os…)`、`cfg(windows)`、`cfg(unix)`、`cfg!(…)`（不含只为测试和调试的）。两个分支结果相同的写「一样」。

| 位置 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| `main.rs`：`secondary!`、`command_key`、`mod win32` | ⌘ | Ctrl | 一样 |
| `app.rs`：`cmd-q` 只在 Mac 绑；`ctrl-cmd-s` / `ctrl-alt-s`；⌥Tab 只在 Mac 绑；Ctrl+Shift+Z、Shift+Insert、Ctrl+Insert 只在 Windows 绑 | | | 一样（第 2 节） |
| `app.rs`：主窗口的 `CloseWindow`（非 Mac） | ⌘W 关主窗口 | 托盘图标显示时关，隐藏时不关 | 一样（第 1 节） |
| `paths.rs`：主目录、数据、缓存、下载 | `~/Library/…` | `%APPDATA%` 等，下载取 `FOLDERID_Downloads` | 一样（#166） |
| `theme.rs`：`set_app_appearance` | `NSApp.appearance` | `SetPreferredAppMode` | 已修（#162） |
| `theme.rs`：字体栈、`MEDIUM_WEIGHT` | 系统字体、Menlo、501 | Segoe UI Variable / Segoe UI、微软雅黑、Cascadia Mono / Consolas、499 | 一样（`WINDOWS.md` §4） |
| `theme.rs`：系统强调色（`read_system_accent`、`watch_system_accent`） | `NSColor.controlAccentColor` | `UISettings` 的强调色，自带的两种蓝当作没选 | 一样（第 1 节） |
| `ui/native/tokens.rs`：`TRAFFIC_LIGHTS_INSET`；`app.rs` 的 `sidebar_toggle_x` | 84，给红绿灯留位置；侧栏开关在红绿灯后面 | 56；没有红绿灯，侧栏开关在最左边。侧栏其余部分和 Mac 窗口状态下一样（工具在顶行右端，搜索框占整行） | 已修（#178）：原来用的是 Mac 全屏时的排法（搜索框缩成一小块挤在顶行），现在只差红绿灯 |
| `copy/mod.rs`：`per_system` | Mac 的说法（Mac、访达、菜单栏、⌘） | Windows 的说法（电脑、资源管理器、通知区域、Ctrl） | 一样（按系统的写法；目录测试检查两套都没有多余空格） |
| `main.rs`：`windows_subsystem = "windows"` | 不适用 | 发布版不带控制台窗口 | 一样 |
| `selftest/mod.rs`：激活策略、装菜单、`exit_now` | 不占 Dock、不抢焦点；菜单在系统菜单栏 | 把菜单装进窗口的标题条；其余相同 | 一样（只影响自测） |
| `selftest/capture.rs`：`keep_frames`、`undo_clamp`、`frame_size` | 撤销 AppKit 把窗口限制在屏幕内 | 空实现：Windows 不限制，CI 把屏幕设成 1920×1080 | 没有对应（第 5 节） |
| `ui/sf_symbols/asset.rs`：`screens_scale` | 按最大的屏幕缩放比栅格化 SF Symbols | 固定返回 2，但 Windows 不走这条路：图标是 SVG，由 GPUI 按窗口实际缩放比画 | 一样；100% 在 CI 截图里是清晰的，125%、150% 真机确认（第 7 节） |
| `crates/artifact-view/src/page.rs`：`rules_ready`、`NO_NETWORK_SCRIPT` | WebKit 的内容规则表编好之前不加载页面 | 没有规则表：WebView2 靠请求过滤和死代理断网，页面一创建就能加载 | 一样（两边跑同一套隔离测试） |
| `ui/native/window_chrome.rs` | 不画 | 32 高的标题条 | 一样（标题栏差别） |
| `ui/motion.rs`：减弱动画、高对比度 | `NSWorkspace` | `SystemParametersInfo` | 一样 |
| `ui/sf_symbols/**` | SF Symbols | Lucide SVG | 一样（`WINDOWS.md` §5） |
| `ui/clipboard.rs` | GPUI 的剪贴板就够 | 另写位图；截图等剪贴板 | 已修（#166） |
| `ui/web_embed.rs`：`attach_native`、`prepare_process`、`host_keys` | AppKit 子视图 | 子 `HWND`，关 DirectComposition，应用快捷键交还 | 一样（#162） |
| `views/artifacts/window.rs`：确认对话框、`chrome`、`vnc` | `NSAlert`；系统外观 | 任务对话框；标题栏颜色；没有 VNC 应用时提示 | 已修（#162、#166） |
| `views/composer/native.rs`：截图 | `screencapture -i` | `ms-screenclip:` 加剪贴板 | 已修（#166） |
| `views/composer.rs`：格式键读修饰键 | ⌘，不能同时按 ⌃ | Ctrl，不能同时按 Win | 一样 |
| `views/search/mod.rs`：全局搜索的键 | ⇧⌘F | Ctrl+Alt+F | 一样 |
| `views/timeline/find.rs`：F3 | 没有 | F3、Shift+F3 | 一样（Windows 多一组习惯键） |
| `views/ask/mod.rs`：⌃P、⌃N | 绑 | 不绑 | 一样（第 2 节） |
| `views/admin/mod.rs`：侧栏顶部给红绿灯留的一段 | 64 | 12 | 已修（#156） |
| `views/admin/usage.rs`：预算虚线 | 虚线边框 | 逐段画 | 已修（#156、#162） |
| `views/media/native.rs` 和 `native_windows.rs` | AVFoundation、PDFKit | Media Foundation、`Windows.Data.Pdf`、`MediaPlayer` | 一样（H.264、AAC、MP3、WAV、PDF；CI 同一套测试） |
| `views/media/native.rs`：HEVC 测试只在 Mac 上要求能解 | 系统自带解码器 | 没装扩展就解不了，测试允许「没有预览」 | 没有对应（解码器要用户从商店装；解不了时界面说明，#166） |
| `views/media/states.rs`：`failed_hint` | 通用的话 | 视频说缺解码器 | 已修（#166） |
| `diagrams/mod.rs`：图里的字体、渲染器 | 苹果字体，WKWebView | Windows 字体，WebView2 | 一样 |
| `diagrams/viewer.rs`：导出的文件名 | 只换 `/ : \` | 另换 Windows 不接受的字符 | 已修（#156、#162） |
| `selftest/capture.rs` | `screencapture -l`、`isKeyWindow` | `PrintWindow`、激活窗口、DPI 检查 | 一样 |
| `selftest/artifact_window.rs` | 没有（产出窗口跟随系统外观，不用单独验证） | 截真的产出窗口 | 只有 Windows 需要（#162） |
| `crates/artifact-view`：`mac.rs` / `win.rs` 两个后端 | WKWebView | WebView2 | 一样；差别见第 1、3 节 |
| `crates/artifact-view/src/resolve.rs`：链接逃逸测试 | 符号链接（`cfg(unix)`） | 原来没有对应的测试 | 已修（#162）：Windows 上用文件符号链接、目录符号链接、junction 和反斜杠路径各试一遍，都拿不到文件夹外面的文件 |

## 4. 截图对比

第一轮两边各 270 个场景、浅色深色，共 540 对，每一对都看过（上 Mac、下 Windows 拼在一起）。不算差别的：标题条和窗口
按钮、内容整体下移 32、字体和图标集本身、表情的画法、AI 成员头像（开源构建没有私有图包，CI 上是字母）、
默认服务器（开源构建没有，登录的出错场景停在「连接服务器」这一步）、表情的常用顺序（Mac 上读到了本机真实的使用记录）。

每个场景一行的明细在 [interface-scenes.zh.md](interface-scenes.zh.md)（由 `scripts/windows-scene-table.ts` 从两边的 `screenshots.json` 生成，场景数和 run 号写在它的开头）。

最后一轮是 run 43020519758（`windows_scenes=all`，产物 `windows-screenshots`，在 #176 的分支上）；下面那张「看哪张」的表里的图，在 run 43000800678 和这一轮里都有。失败的场景：

| 场景 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| `artifact_ask_pending` | 原来失败 | 原来失败 | 已修（#178）：话题样例变长之后，页面一打开就滚到了视口上方，按钮点不到（两边都一样）。场景现在先把页面滚进来再点；Mac 上通过，Windows 看新一轮 |
| `state-hover-settings-row` | 原来失败 | 原来失败 | 场景删掉了（#178）：它悬停的「按群设置」那一行在通知设置搬到群和话题页之后就不存在了，通知页现在只有开关和下拉框，没有行悬停 |
| `web-embed-scrolled` | 通过 | 原来失败：「the inline page did not load」；最后一轮通过 | 已修（#171） | 场景打开时停在长话题的末尾，页面有没有一条边露在视口里取决于内容区高度；Windows 的内容区矮 32，页面一开始完全在视口外，不算「显示」。场景现在先把页面滚进来。和应用本身无关 |
| `live-takeover`（浅色） | 那一次没等到帧（偶发） | 通过 | 一样 |

已修的行对应的截图（都在上面那个产物里，下载后看过）：

| 改了什么 | 看哪张 |
| --- | --- |
| 500 字重不再是粗体（工具栏按钮、表头、标签、头像字母） | `main-light`、`core-side-chat-light`、`group-chat-info-light` |
| 对话框、查看器、命令面板挂在工具栏行下面 | `new-topic-light`、`leave-group-light`、`diagram-viewer-light`、`switcher-light`、`group-purpose-light` |
| 团队管理：页标题是黑的、侧栏末行完整、预算虚线 | `admin-usage-light`、`admin-usage-dark` |
| 悬停落在对的行；没有标题条的窗口不偏移 | `state-hover-message-light`、`state-hover-page-row-light`、`state-card-tooltip-light`、`state-card-option-hover-light` |
| 页脚的键写成 Enter、Esc | `switcher-light`、`ask-light` |
| 页面的细滚动条、等宽字是 Consolas | `artifact-pane-v2-light`、`artifact-v2-light` |
| 页面跟随深色 | `artifact-page-back-dark` |
| 非活动窗口的标题条变淡 | `window-inactive-dark` 对比 `main-dark`（标题条那 32 行的像素不同） |
| 产出窗口：标题栏颜色、应用图标、下拉框和展开的列表、页面深色 | `artifact-window-light`、`artifact-window-dark`、`artifact-window-dark-versions`（真应用，`desktop --artifact-window`） |
| 页面有焦点时的应用快捷键、打字算操作、右键菜单、链接对话框 | 没有截图：`artifact-view-windows` 产物里的 `embed/report.json`（`hostKeyGivesTheHostTheKeyboard`、`pageHearsNeitherTheHostKeyNorWhatFollows`、`typedKeyIsPageInput`）和 `win.rs` 的测试 |
| 截图、粘贴、复制图片、下载目录、`vnc:` | 没有截图：`ui/clipboard.rs`、`views/composer/tests.rs`、`win.rs` 的测试（Windows CI 上跑） |

看到的差别（同一个原因的合成一行）：

| 哪里 | Mac | Windows | 状态 | 做法 |
| --- | --- | --- | --- | --- |
| 中等字重的文字：工具栏按钮「详情 / 产出 / 单独问 / 改动」、表头、列表行标题、标签、选中的分段 | 常规到中等 | 粗体 | 已修（#156） | 这些地方用的是 `FontWeight::MEDIUM`（500）。DirectWrite 把 500 及以上向上取到更重的字重，而 Segoe UI 和微软雅黑没有中等字重，于是画成 Bold。Windows 上把这个常量改成 499（`third_party/gpui-pre`），一处改完，gpui-component 里的也跟着好 |
| 半粗的中文：话题标题、卡片标题、对话框标题、未读的话题行 | 苹方 Semibold | 微软雅黑 Bold，比 Mac 重一档；同一行里的英文是 Segoe 的 Semibold，比中文轻 | 没有对应 | 微软雅黑只有 Light、Regular、Bold 三个字重，Windows 自带的中文字体都没有 Semibold。`WINDOWS.md` §4 已写明 |
| 中等字重的名字、数字（发言人名字、表格金额、选中的侧栏行） | Medium | Regular | 没有对应 | 同上：没有中等字重时取 Regular（499）。Windows 11 的 Segoe UI Variable 有这个字重，英文和数字在真机上是 Medium |
| 团队管理的页标题 | 黑色 | 灰色 | 已修（#156） | 自测的窗口在 Windows 上不是活动窗口，标题按非活动窗口画。标题改用和其他窗口一样的判断，自测在 Windows 上把窗口真的激活 |
| 获得焦点的输入框 | 有光标和选区 | 有焦点环，没有光标和选区 | 已修（#156） | 同上（窗口不是活动窗口）。真实使用时窗口是活动的 |
| 悬停类场景（消息行、话题行、产出行、侧栏行） | 悬停在指定的行 | 悬停在上一行，或没有悬停 | 已修（#156） | 自测的悬停坐标是按 Mac 的布局量的，Windows 上加上标题条的高度 |
| 对话框、提示框、图的查看器、媒体查看器、命令面板 | 挂在工具栏行下面 | 位置和 Mac 一样从窗口顶算，于是盖住工具栏行的下半截 | 已修（#156） | 从标题条下面算（`window_chrome::sheet_top`），高度也相应减掉 |
| 产出页面（整页） | 页面高度到窗口底 | 高出 32，底部被窗口边切掉 | 已修（#156） | 高度减去标题条 |
| 团队管理的侧栏 | 顶部 64 留给红绿灯 | 标题条下面还空着 64，最后一行「团队记忆」被页脚切掉 | 已修（#156） | Windows 上不留这一段 |
| 用量图表里的预算线 | 橙色虚线 | 没有画出来 | 已修（#156） | GPUI 的 Direct3D 渲染对没有高度的盒子不画虚线边框；Windows 上用同样的 2 实 1 空的小方块画 |
| 内嵌页面的滚动条 | 没有（浮动的细滚动条） | Chromium 的经典滚动条，带箭头，深色下是白的 | 已修（#162） | 见第 3 节 |
| 内嵌页面里的等宽字 | 等宽西文字体 | 中文系统上落到宽的中文等宽字体（页面只写了 `Menlo, monospace`） | 已修（#162） | 页面是给两个系统写的，常常只写苹果的字体名。Windows 上给每个页面注入字体别名：`-apple-system`、`PingFang SC`、`Menlo`、`SF Mono` 等指到 Segoe UI、微软雅黑、Cascadia Mono、Consolas |
| 内嵌页面的深色（`artifact-page-back` 深色） | 深色 | 白底 | 已修（#162） | 除了 profile 的设置，再直接设这个页面的 `prefers-color-scheme`（DevTools 协议） |
| 内嵌页面在截图里是空白（`web-embed` 的两个页面；第一轮还有 `compose-extend`） | 有内容 | 普通截图（`PrintWindow`）里有时是空白 | 一样（截图方法的限制）；真机确认 | 不是用户会看到的空白。同一轮里从屏幕上读的截图（`screen/web-embed-light`、`screen/web-embed-dark`、`screen/artifact-pane-v2-*`、`screen/compose-extend-*`，run 43020519758）页面全部在，浅色深色都对。`PrintWindow` 是向每个窗口要画面，另一个进程的 WebView2 子窗口不一定给；#176 让截图先等页面画完第一帧（两次 `requestAnimationFrame`），`compose-extend` 之后都有了，`web-embed` 仍然时有时无，所以内嵌页面以 `screen/` 那一遍为准。CI 没有显卡，整个进程的第一个页面最长用了 46 秒，等待放宽到 60 秒并记录用时；真机上的用时见第 7 节 |
| 页面里的音频控件（`web-embed`） | WebKit 的深色控件条 | Chromium 自己的控件 | 已修（#162）；样子没有对应 | 页面的 `color-scheme` 和 `prefers-color-scheme` 跟随主题，深色下控件是深色（`screen/web-embed-dark`）。控件本身是两个浏览器引擎各自的，长得不一样 |
| 产出缩略图（`artifacts-grid`） | 页面上一次显示时的样子，缩小放进格子 | 同样的做法；第一轮里右边有一条滚动条，起始位置比 Mac 低一段 | 滚动条已修（#176）；起始位置真机确认 | 缩略图就是页面上一次在屏幕上时的快照。滚动条：页面里的滚动条改成只在指针在页面上时出现，快照里不再有。起始位置不同是因为两边「上一次显示」时页面滚动到的位置不同，我没有查到是哪个场景滚动了它；真机上看第 7 节第 15 条 |
| 图的查看器（`diagram-viewer`）：图按 80% 显示、右边和下边超出、有横向滚动条、「[连不上时]」压在 loop 框的上边线上 | 同样 | 同样 | 一样 | 用现在的代码在这台 Mac 上重截了同一个场景，和 Windows 那张逐项对过：缩放 80%、三个参与者的位置、loop 框和「[连不上时]」的重叠、横向滚动条的长度两边一致。这是图本身的排版（Mermaid 的时序图把条件文字放在框线上，查看器窗口比图窄），不是 Windows 的字体造成的；量字和画字用的是同一个页面里的同一套字体 |
| 群资料里人的头像（`group-chat-info`） | 字母是常规字重 | 字母是粗体，整个圆看起来发黑 | 已修（#156） | 同第一行（500 字重） |
| 表情 | Apple Color Emoji | Segoe UI Emoji，同样大小的格子里图形略小一点，样子不同 | 一样 | 系统的表情字体 |
| 「群说明」等设置里的窗口高度、设置的标题出现两次、设置的分页没有文字 | | 设置窗口比内容矮 32，最后一行被切 | 系统集成清单 | 已告知（`views/settings` 归那边） |
| Tab 键先走到窗口里的菜单栏 | 菜单不在窗口里 | 焦点环落在菜单「显示」上 | 系统集成清单 | 已告知：Windows 的菜单用 Alt 进入，不该在 Tab 顺序里 |
| 文件右键菜单里的「快速查看」、手动安装更新的说明里的「访达」「应用程序」 | | Mac 的说法留在 Windows 上 | 系统集成、发布清单 | 已告知 |
| 非活动窗口的标题条（`window-inactive`） | 红绿灯变灰 | 工具栏和标题变灰了，标题条里的菜单和窗口按钮没有变 | 已修（#162） | 非活动窗口的菜单和窗口按钮变淡 |
| 卡片样例窗、`interact_*`、`artifact-quiet-page` 这些自测专用的窗口 | 有红绿灯 | 没有标题条 | 已修（#176） | 这些窗口也套上标题条（`window_chrome::Framed`），和应用里的窗口一致；看 `cards-v2-states-light`、`artifact-quiet-page-light`、`compose-extend-light` |
| 全角标点「」（）【】｜“” | 苹方的标点靠近文字 | 微软雅黑的标点占满一格，前后空隙大，个别行因此多折一行 | 没有对应 | 字体本身的差别 |
| 文字宽度：个别句子在 Windows 上多折一行（登录页副标题、更新说明、卡片提示） | | 微软雅黑比苹方宽 | 一样（没有被切的） | 布局按内容自适应，多折一行时下面的内容跟着下移。逐个看过会折行的场景（`login-email`、`whats-new`、`update-notes`、`interact_report_rev1`、`topic`）：没有文字被切或重叠。第一轮里 `whats-new` 的脚注被切是当时那一版更新说明更长；现在的内容在两边都完整（和这台 Mac 上重新截的图比过），弹窗本身有最大高度和滚动 |
| 内容区矮 32 带来的：消息列表第一行被日期标签压住、右侧面板最后一块看不全 | | 窗口同样大时内容少 32 | 一样（标题栏差别） | 和把 Mac 窗口拉矮 32 是同一个结果 |
| 文字边缘 | 灰度抗锯齿 | ClearType 的彩色边 | 一样 | Windows 自己的文字渲染 |
| 图（Mermaid）排版略大、个别标签有灰框 | | Chromium 和 WebKit 量字的差别 | 真机 | |

## 5. 只在 Mac 上跑的测试和自测

| 项 | Mac | Windows | 状态 | 做法 |
| --- | --- | --- | --- | --- |
| 截图自测 | `screencapture -l` | `PrintWindow`，全部场景 CI 上都能跑 | 一样 | CI 默认只跑 24 个主要场景（`scripts/windows-screenshots.sh` 的 `main_screens`），`windows_scenes=all` 手动跑全部 |
| 截图时输入框显示光标（`draw_as_key`） | 窗口当作 key window 画 | 空实现，输入框按非活动窗口画 | 已修（#156） | Windows 上自测把窗口真的激活（CI 上没有人在用） |
| 截图前核对窗口大小（`frame_size`、`undo_clamp`） | 撤销 AppKit 把窗口限制在屏幕内 | 空实现 | 没有对应 | AppKit 的这个限制 Windows 没有；CI 把屏幕设成 1920×1080，每张 PNG 的尺寸写在 `selftest.log` 里，这次 540 张都和 Mac 的点数一致 |
| `views/media/poster.rs` 的视频封面测试 | 断言 | 不断言 | 已修（#167） | Windows 上也断言有封面和时长 |
| `theme.rs` 的字体测试 | 断言苹果字体 | 没有对应断言 | 已修（#167） | 断言 Segoe UI、Consolas |
| SF Symbols 的几个测试 | 只能在 Mac 上跑 | 不编译 | 没有对应 | Windows 不画 SF Symbols；SVG 图标集有自己的测试 |

## 6. 2026-11-10 之后其他工作合进 `main` 的界面改动

| 改动 | Windows 上 | 状态 |
| --- | --- | --- |
| 团队管理窗口（总览、成员、用量、活动记录、订阅账号、API 密钥、机器） | 14 个 `admin-*` 场景在 Windows CI 上都通过，逐对比较见第 4 节 | 一样 |
| 预算暂停卡片、改预算 | `admin-budget-edit` 场景通过 | 一样 |
| 右侧面板可拖宽、命令框可滚动 | 同一份代码（GPUI 的拖动和滚动） | 真机 |

## 7. 要在真机上确认

CI 是 Windows Server 2025、100% 缩放、软件渲染、没有人操作。下面这些只有真的 Windows 电脑能回答，按影响从大到小：

1. **中文输入法**（微软拼音、搜狗）：在输入框里打「你好」，候选窗应该贴着光标；候选时按 Enter 是上屏，不发送；
   候选时按 Esc 只取消候选。在内嵌页面的输入框里再试一遍。
2. **内嵌页面**：打开一个带页面的话题，滚动消息列表、拖动窗口大小，页面应该跟着走，没有闪烁和拖影；打开一个
   对话框（页面这时换成快照）再关掉，不该闪一下。在页面上滚滚轮和触控板双指滑动，都应该滚动消息列表。
3. **缩放**：在 125%、150%、200% 的屏幕上看主窗口和设置，文字和 1 px 的线应该清晰；把窗口拖到另一个缩放比的
   屏幕上，内嵌页面的位置和大小应该还对。
4. **产出窗口**：标题栏的颜色和主窗口的标题条一样（Windows 11）；在设置里切换浅色 / 深色，已打开的产出窗口
   跟着变；版本下拉框展开的列表在深色下是深色的。
5. **页面有焦点时的键**：点一下内嵌页面，按 Ctrl+K 应该打开切换器，Ctrl+N 新话题，Ctrl+C 复制页面里选中的字；
   在页面的输入框里打字后点页面上的按钮，按钮的回应应该发得出去。
6. **截图**：输入框的 + › 截图，系统的截图覆盖层出现，框选后图片成为附件；按 Esc 取消后什么都不发生。
7. **粘贴和复制**：从 Excel 或 WPS 复制几个单元格粘贴进输入框，应该是文字；按 PrintScreen 再粘贴，附件应该是
   PNG；在图片查看里点复制，到「画图」里应该能粘贴。
8. **引用提问**：按住 Alt 悬停消息里的一段，出现标记；Alt+点击加入引用；松开 Alt 后焦点不该跑到菜单栏上。
9. **视频**：H.264 的 MP4 有封面、能播放；没装 HEVC 扩展时 iPhone 拍的视频显示「这台电脑没有这种视频的解码器」
   和「用系统应用打开」；画中画按钮能弹出小窗。
10. **拖入**：从资源管理器拖文件进窗口成为附件；从浏览器拖一张图片进来（这种没有真实文件，现在收不到）。
11. **字体**：Windows 11 上界面是 Segoe UI Variable，发言人名字等中等字重的英文比正文略重；Windows 10 上是
    Segoe UI，两者一样重。
12. **图**：流程图、时序图在话题里显示完整，文字不被节点边框切掉；查看器里 Ctrl 加滚轮、Ctrl+=、Ctrl+-、
    Ctrl+0 和小键盘的 + - 都能缩放。
13. **第一次打开页面**：启动应用后第一次打开带页面的话题或产出，页面应该在一两秒内出现（CI 的软件渲染
    下第一个页面最长用了 46 秒，之后的是 0 到 3 秒）。
14. **输入框的光标**：点进任何输入框都有闪烁的光标（截图里 Windows 的输入框多数没有光标，因为自测的
    窗口不一定拿得到前台）。
15. **产出缩略图**：打开一个页面产出看一眼再回到产出列表的图标视图，缩略图应该是页面从顶部开始的样子。
16. **贴靠布局**：鼠标悬停在最大化按钮上，Windows 11 弹出贴靠布局；Win+←、Win+→、Win+↑ 把窗口贴到
    半屏和最大化，最大化时四周没有多出来的边。
17. **有显卡时的渲染**：CI 是软件渲染。在有独立显卡和只有核显的电脑上各看一遍主窗口：滚动消息列表流畅，
    拖动窗口大小时内嵌页面跟得上，深色下没有闪白。
18. **图标的清晰度**：125% 和 150% 缩放下侧栏和工具栏的图标线条清晰，没有发虚。
