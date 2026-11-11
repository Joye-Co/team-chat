# Windows 对齐清单：系统集成

范围：`apps/desktop/src/provider/**`（不含 `this_mac.rs`）、`views/nav/**`、`views/settings/**`、
`views/files/**`、`win32.rs`、`main.rs` 里启动和按系统分支的部分、`crates/chat-core/**`。目标是 Mac 版在这些地方给用户的每一个功能和行为，
Windows 版都有同样的结果。

这份清单是对着代码做的：上面这些文件里每一处 `cfg(target_os = …)`、`cfg!(…)`，每一个在 macOS 以外
什么都不做或返回固定值的函数，Mac 版的每一个菜单项、设置行、命令，以及每一处 macOS 框架调用。GPUI 的
Windows 实现（`gpui-pre-windows 0.3.10`）也逐项查过：哪些调用是真的做了事，哪些是空的。

状态的意思：

- **一样**：Windows 上已经是同样的结果。
- **没有对应**：Windows 没有这个系统设施，写了原因和替代做法。

「验证」一列：`CI` 是 Windows CI 上有测试或截图（括号里是测试或场景的名字），`真机` 是列在文末要在真机上确认；
只写「真机」的行是 CI 的机器做不了的（要人点、要第二台设备、要睡眠或换网络）。

## 1. 通知

| 功能 | Mac 上 | Windows 上现在 | 状态 | 怎么补 / 验证 |
| --- | --- | --- | --- | --- |
| 应用在后台时弹通知 | `UNUserNotificationCenter`，应用不在前台才弹 | WinRT Toast，前台窗口不是本应用的可见窗口才弹 | 一样 | CI（单元测试），真机 |
| 通知按群归组 | `threadIdentifier` = 群 id，通知中心里同一个群叠在一起 | 之前不分组。现在应用自己建 Toast：同一个群的通知在一个标题（群名）下面，`Group` 是群 id 的短哈希 | 一样（PR 3） | CI（XML、真的发一条再从通知中心读回），样子要真机 |
| 通知图标 | 应用图标 | 之前没有开始菜单快捷方式时是系统的默认图标。现在应用在 `AppUserModelId\<id>` 下登记 `IconUri`（应用图标） | 一样（PR 3） | CI（注册表），真机 |
| 群头像 | 没有（通知里只有标题和正文） | 没有 | 一样 | Mac 的代码没有给通知加头像，两边都只显示应用图标 |
| 点通知打开对应话题 | 带群和话题 id，点击后打开话题并把应用带到前台 | 应用在运行时同样；窗口已关（在托盘里）时先打开主窗口 | 一样 | CI（单元测试），真机 |
| 应用退出后点通知中心里的旧通知 | 系统启动应用，再打开那个话题 | 之前什么都不发生。现在 Toast 是协议激活：点击打开 `<scheme>://group/<g>/topic/<t>`，系统启动应用，应用按链接打开话题；应用在运行时由新进程转交给它。安装包已经登记了 URL scheme，不需要 COM 激活器；没经过安装包运行的版本在没人占用这个 scheme 时自己登记 | 一样（PR 3） | CI（XML、scheme 登记、两个进程之间转交），点击要真机 |
| 声音 | 设置 › 通用 › 声音 打开时用系统默认通知音；专注模式下不响 | 之前自己调 `PlaySoundW`，专注助手打开时也响。现在声音写在 Toast 里（`<audio>`），由系统播放，专注助手打开时不响；开关关掉时 Toast 静音 | 一样（PR 3） | CI（XML），真机 |
| 勿扰 / 专注模式 | 系统扣住横幅，通知进通知中心 | 系统扣住 Toast，通知进通知中心；不响（声音在 Toast 里），任务栏按钮不闪，角标照常更新 | 一样 | 真机（专注助手要人打开） |
| 读完后收回通知 | 群读完（本机或别的设备）后把这次运行发的通知从通知中心拿掉 | 同样：这次运行发过通知的群读完后，按群把它的通知拿掉 | 一样 | CI（发一条、读回、拿掉） |
| 第一次使用时的权限 | 系统弹一次授权 | Windows 不需要授权，用户可以在系统设置里关 | 没有对应 | Windows 的通知默认允许，没有授权这一步 |
| 提醒用户注意 | 正式版不跳 Dock：靠横幅、声音和 Dock 角标（没有应用包的 `cargo run` 才跳一下） | 之前每条通知都闪任务栏按钮。现在和 Mac 一样不闪：靠 Toast、声音和任务栏角标 | 一样（PR 3） | 有了任务栏角标后闪烁是多出来的行为，去掉了 |

## 2. 未读数：Dock、菜单栏、任务栏、托盘

| 功能 | Mac 上 | Windows 上现在 | 状态 | 怎么补 / 验证 |
| --- | --- | --- | --- | --- |
| 应用图标上的未读数 | Dock 角标，未读数，0 时没有 | 任务栏按钮上的角标（`ITaskbarList3::SetOverlayIcon`）：红底白字的未读数，超过 99 写 `99+`（角标只有 16 px），0 时没有。应用的每个有任务栏按钮的窗口都有，包括产出窗口（它是普通的 Win32 窗口，GPUI 不知道它）：应用用 `SetWinEventHook` 看本进程每一个显示出来的顶层窗口，监听系统发给它的 `TaskbarButtonCreated`，按钮建好后设角标；资源管理器重启后任务栏对象重新取，新按钮建好时再设一次 | 一样 | CI（`the_taskbar_takes_the_unread_badge`：runner 上真的收到 `TaskbarButtonCreated`、设置和清除；`a_window_gpui_does_not_know_gets_the_badge_too`：普通 Win32 窗口被看到）。`crates/artifact-view` 没有改。重启资源管理器没有在 CI 上做（会打断同一个任务里后面的截图步骤），真机 |
| 菜单栏图标 | `NSStatusItem`，有未读时带点，悬停提示未读数，点击打开主窗口 | 通知区域图标，同样 | 一样 | CI（`the_template_images_become_icons`、`the_icon_is_added_to_the_notification_area_and_removed`：图标真的加进通知区域再拿掉），样子要真机 |
| 菜单栏图标的开关 | 设置 › 通用 › 菜单栏里显示 | 同一个开关，文案是「通知区域里显示」；关掉后图标拿掉，再同步也不会加回来 | 一样 | CI（同上一行的测试） |
| Dock 图标的右键菜单 | 新话题、新建群…、发起私聊…、显示主窗口 | 任务栏按钮的跳转列表有这四项（GPUI 用 `set_dock_menu` 建的）。应用先设 AppUserModelID、让进程加入多线程套间，再建列表。之前两样都缺：GPUI 在后台线程上提交列表，那个线程没有 COM，提交直接失败（CI 的探针证实了：不加入套间时系统里没有这份列表），所以任务栏按钮上其实没有这四项；点一项会启动新进程，新进程把命令转给正在运行的应用。托盘图标右键也有这四项加设置…和退出 | 一样 | CI（`win_window_probe`：真的 GPUI 应用按应用的顺序建列表，系统的跳转列表文件里有任务的参数，按这个 id 删除后文件消失，说明落在这个 id 下；`a_second_launch_hands_its_command_to_the_first`：转交），真机 |
| 点 Dock 图标重新打开主窗口 | `on_reopen` | GPUI 在 Windows 上不会调 `on_reopen`。现在从开始菜单或任务栏再启动，正在运行的应用到前台并打开主窗口（见第 4 节）；点托盘图标也是 | 一样（PR 3） | CI（转交），真机 |
| 关掉最后一个窗口后应用继续运行 | 是 | 托盘图标在时是；图标被隐藏时关掉最后一个窗口就退出（否则没有办法再找到它） | 一样 | CI（`the_app_outlives_its_windows_only_with_the_icon`），真机 |

## 3. 菜单

Windows 没有屏幕顶部的菜单栏，同一套菜单画在主窗口标题栏里。逐项对照 `views/nav/menus.rs`：

| 菜单项 | Mac 上 | Windows 上现在 | 状态 | 说明 |
| --- | --- | --- | --- | --- |
| 关于 | 应用菜单第一项，系统的关于面板（图标、名称、版本） | 帮助 › 关于，应用自己的关于窗口，同样三样内容 | 一样（PR 1） | Windows 没有系统关于面板；位置按 WINDOWS.md §2。CI 截图 `about` |
| 检查更新… / 下载进度 / 重启安装 | 应用菜单 | 第一个菜单 | 一样 | CI（`the_window_menus_drop_only_what_macos_alone_has`：Windows 的菜单逐项对照 Mac 的；截图 `main` 里能看到菜单条） |
| 设置… | 应用菜单，⌘, | 第一个菜单，Ctrl+, | 一样 | CI（同上；`a_recorded_key_becomes_the_shortcut` 里 Ctrl+, 是设置的键） |
| 服务器 子菜单 | 应用菜单 | 第一个菜单 | 一样 | CI（同上；`the_window_menu_commands_have_their_windows_keys`：两个服务器命令有处理函数） |
| 服务 | 系统的服务子菜单 | 没有 | 没有对应 | Windows 没有「服务」这个系统设施 |
| 隐藏、隐藏其他、全部显示 | ⌘H、⌥⌘H | 没有 | 没有对应 | Windows 不能隐藏整个应用；对应的系统操作是最小化（Win+↓、Win+Home、Win+D），关掉窗口后应用留在托盘里 |
| 退出 | 应用菜单 › 退出，⌘Q | 第一个菜单 › 退出，托盘菜单 › 退出，两处都是同一个退出命令。没有快捷键：Windows 上 Alt+F4 和关闭按钮是关窗口，托盘图标在时应用留在托盘里（图标被隐藏时关掉最后一个窗口就是退出） | 没有对应（快捷键） | Windows 应用没有「退出整个应用」的通用快捷键，Ctrl+Q 不是 Windows 的惯例，所以不绑；退出命令本身两处都有。CI（菜单项和托盘菜单的单元测试），真机 |
| 文件：新话题、新建群…、发起私聊…、关闭窗口 | ⌘W 关当前窗口，主窗口也关，应用留在 Dock | Ctrl+W 同样：托盘图标在时主窗口也关，应用留在托盘里。图标被隐藏时关掉最后一个窗口应用就退出了，所以那时 Ctrl+W 不关主窗口、不关最后一个窗口（关闭按钮和 Alt+F4 照常） | 一样 | CI（`the_close_command_never_ends_the_app`、`the_window_menu_commands_have_their_windows_keys`），真机 |
| 编辑：撤销、重做、剪切、复制、粘贴、全选、查找三项 | | 同样 | 一样 | CI（菜单逐项对照） |
| 编辑 › 表情与符号 | 系统的字符检视器，⌃⌘空格 | 系统的表情面板（Win+.）：应用替用户按下这个键，浏览器的做法 | 一样（PR 1） | 之前 Windows 上没有这一项。真机 |
| 前往：快速切换…、首页、产出、成员、下一个未读话题、上一个未读话题（六项） | | 同样 | 一样 | CI（菜单逐项对照） |
| 显示：全部项、勾选状态、切换全屏幕 | ⌃⌘F | F11 | 一样 | CI（`the_view_menu_runs_the_toolbar_and_filter_commands`；`the_window_menu_commands_have_their_windows_keys`：F11；`win_window_probe`：真窗口进出全屏） |
| 窗口 › 最小化 | ⌘M | 有，没有快捷键（系统的是 Win+↓） | 一样 | CI（`win_window_probe`：真窗口 `IsIconic`） |
| 窗口 › 缩放 | 放大或还原 | 最大化或还原（GPUI 的缩放只会最大化，还原是应用补的），文案是「最大化或还原」 | 一样 | CI（`win_window_probe`：真窗口 `IsZoomed`；`a_maximized_window_is_restored`） |
| 窗口 › 显示主窗口 | | 同样 | 一样 | CI（菜单逐项对照），真机 |
| 窗口 › 前置全部窗口 | 应用的所有窗口到前面 | 逐个把窗口提到前面，当前窗口在最上 | 一样 | CI（`win_window_probe` 逐个激活两个真窗口并报告谁在最前；CI 会话可能不给前台，所以只报告不判定），真机 |
| 窗口菜单里的窗口列表 | 系统加的 | 没有 | 没有对应 | Windows 上每个窗口在任务栏和 Alt+Tab 里 |
| 帮助 › 搜索 | 系统的菜单搜索 | 没有 | 没有对应 | Windows 没有菜单搜索；菜单用 Alt 加方向键走 |

## 4. 启动、链接、单实例

| 功能 | Mac 上 | Windows 上现在 | 状态 | 怎么补 / 验证 |
| --- | --- | --- | --- | --- |
| 只运行一个应用 | 系统保证：再次打开只是把它带到前台 | 之前再启动一次就多一个进程、多一个托盘图标。现在是单实例（命名互斥量加隐藏窗口的 `WM_COPYDATA`）：后启动的进程把要做的事交给第一个、让它到前台，然后退出。第一个进程不接受的内容（太长的链接、不认识的消息）只是被丢掉，后启动的进程照样退出，不会因此开出第二个应用；只有第一个进程一直没有窗口（正在退出或卡死）时才自己启动。带链接启动时命令行上别的参数一律不读（链接的内容来自网页）；带别的参数启动的（自测、`--fake`）不参与 | 一样（PR 3） | CI（真的抢两次、经窗口转交、乱码和空指针不崩、链接后面的参数被忽略），真机 |
| 点 `<scheme>://` 链接 | 系统把应用带到前台（应用不处理链接的内容） | 之前打开第二个应用。现在应用到前台，链接指向自己账号里的群或话题时打开它（通知点击走的就是这条路，所以 Windows 上比 Mac 多做了这一步；Mac 上接 `on_open_urls` 用同一个 `instance::run` 就能补上，不在这次范围里） | 一样（PR 3） | CI（解析、转交），真机 |
| 登录时启动 | `SMAppService`，要批准时打开系统设置 | `HKCU\…\Run` 加 `StartupApproved`，任务管理器里关掉后开关显示关 | 一样 | CI（对真的注册表），真机 |
| 睡眠唤醒后立即重连 | `NSWorkspaceDidWakeNotification` 加时钟跳变兜底 | `WM_POWERBROADCAST`（GPUI）加时钟跳变兜底 | 一样 | 真机 |
| 网络变化后立即重连 | 每 3 秒比较一次网卡地址 | 同样的做法，`GetAdaptersAddresses` | 一样 | CI（读网卡），真机 |
| 设备名 | `gethostname` 去掉 `.local` | `COMPUTERNAME` | 一样 | CI（`the_device_name_is_the_computer_name`） |

## 5. 文件

| 功能 | Mac 上 | Windows 上现在 | 状态 | 怎么补 / 验证 |
| --- | --- | --- | --- | --- |
| 空格预览（快速查看） | Quick Look 面板：系统认识的类型都能预览，不离开应用 | 应用里的预览面板（当前窗口上的一张 sheet），再按空格或 Esc 关掉。图片、视频、音频、PDF、文本和代码、Markdown、表格用应用自己的查看器；网页附件在隔离的产出视图里显示（不能联网，没有任何桥接能力）；其余类型（Word、Excel、PowerPoint、RTF、邮件、字体，以及装了的程序登记过的类型）由系统的预览处理程序画在面板里（`IPreviewHandler`，和资源管理器预览窗格用的是同一套，跑在系统的 `prevhost.exe` 里）；这台电脑上没有处理程序的类型，面板写明「这台电脑没有能预览这种文件的程序」并给「用系统应用打开」。Windows 会运行的文件（程序、脚本宿主的文件、快捷方式）永远不预览，只在资源管理器里选中；面板上的「用系统应用打开」也过同一道检查 | 一样 | 预览处理程序就是 Windows 上 Quick Look 插件的对应物。网页选了「隔离视图里渲染」而不是「显示源码」：Quick Look 和应用里的「打开」都是渲染出来给人看，源码对多数人没用，隔离视图已经保证它碰不到网络和应用。CI（`the_system_handler_draws_a_file_the_app_has_no_viewer_for`：runner 上系统的 RTF 处理程序真的在应用的子窗口里画出来；`a_risky_file_is_never_given_to_a_handler`；截图 `files-quick-look`、`files-quick-look-handler`、`files-quick-look-page`），真机（装了 Office 的电脑上看 Word 文件） |
| 共享… | 系统的共享面板 | 之前在资源管理器里选中文件。现在是 Windows 的共享界面（`DataTransferManager`，Win32 窗口用 `IDataTransferManagerInterop`），文件和网页文件夹都作为存储项交过去 | 一样（PR 2） | CI（存储项和窗口的共享管理器），界面本身要真机 |
| 存储为… | 系统的存储面板，默认在下载文件夹 | 系统的保存对话框（GPUI 的 `IFileSaveDialog`） | 一样 | 真机 |
| 把文件拖出窗口 | 拖到访达或别的应用 | 之前拖出窗口什么都不发生（GPUI 的 Windows 端没有实现向外拖）。现在指针离开窗口时应用自己发起系统拖放（`DoDragDrop`，用系统给这个文件的数据对象，带 `CF_HDROP`），只允许复制 | 一样（PR 2） | CI（数据对象里读回路径、放下和取消的判断），手感要真机 |
| 把文件拖进输入框 | 支持 | 支持（GPUI 的 `IDropTarget`，资源管理器里的文件） | 一样 | 输入框归界面那条线；从浏览器拖图片（虚拟文件）GPUI 不接受，已告知 |
| 打开、在访达中显示 | `open_with_system`、`reveal_path` | 同样的调用，GPUI 用 `ShellExecuteW` 和资源管理器；文案是「在文件资源管理器中显示」 | 一样 | 真机 |
| 有风险的文件不直接打开 | 会运行代码的类型只在访达里显示 | 同一份名单加 Windows 会运行、安装、挂载的类型，文件名按 Windows 的读法判断 | 一样 | CI（单元测试） |
| 下载的文件带「来自互联网」的标记 | `com.apple.quarantine`，打开时 Gatekeeper 检查 | 之前没有；现在写 `Zone.Identifier`（Mark of the Web）：程序过 SmartScreen，Office 文档进受保护的视图 | 一样（PR 1） | CI（写完读回，改名和复制后还在） |
| 打开和选择文件的面板 | `NSOpenPanel` | `IFileOpenDialog`（GPUI） | 一样 | 真机 |

## 6. 凭证

| 功能 | Mac 上 | Windows 上现在 | 状态 | 说明 |
| --- | --- | --- | --- | --- |
| 登录 token | 钥匙串，service 是 app id，account 是服务器地址 | 凭据管理器，目标名 `<app id>:<服务器地址>` | 一样 | CI（对真的凭据管理器存、取、删） |
| 其他放在钥匙串里的东西 | 没有：客户端只有这一样进钥匙串（查过 `security_framework` 和 `TokenStore` 的全部用处） | — | 一样 | 机器 token 是 daemon 的文件，归机器那条线 |

## 7. 设置，逐行

五个分页（通用、账号、通知、外观、快捷键）和团队管理里由这部分代码画的页面（成员、AI 服务、机器）。没有列出的行两边走同一段代码，没有按系统分支。

| 行 | Mac 上 | Windows 上现在 | 状态 | 怎么补 / 验证 |
| --- | --- | --- | --- | --- |
| 通用 › 登录时启动 | 见第 4 节 | | 一样 | |
| 通用 › 菜单栏里显示 | 见第 2 节 | | 一样 | |
| 通用 › 声音 | 见第 1 节 | | 一样（PR 3） | |
| 通用 › 版本和更新的各个状态 | DMG | 安装包加更新程序 | 一样 | 归发布那条线 |
| 通用 › 手动安装的步骤文字 | 「安装盘已在访达中打开…」 | Windows 的说法（「安装包已在文件资源管理器中选中…」） | 一样（发布线 #149） | 文案在 `copy/update.rs`，归发布那条线；这一行随 #149 合并成立 |
| 账号 › 更换头像 | 打开面板 | 打开对话框（GPUI 的 `IFileOpenDialog`） | 一样 | 真机（系统对话框要人选文件） |
| 账号 › 登录的设备 | 「这台 Mac」，按设备名选图标 | 「这台电脑」；`DESKTOP-…` 这类 Windows 默认电脑名画成台式机，其余是笔记本 | 一样 | `DeviceKind::from_name`（`data.rs`，界面那条线已改） |
| 通知 › 每日摘要、勿扰的时间 | 带上这台电脑的 IANA 时区 | 之前读不到时区（只会读 `/etc/localtime`），时间可能按错的时区算；现在从系统读（`Windows.Globalization.Calendar`） | 一样（PR 1） | CI（读到的是 IANA 名） |
| 外观 › 浅色 / 深色 / 自动 | 自动时跟随系统 | 同样（GPUI 读 `UISettings`，系统切换时收到通知） | 一样 | 真机 |
| 快捷键 › 列表和录制 | ⌘⌥⇧⌃ 符号 | `Ctrl+Shift+K` 的写法 | 一样 | CI（`shortcuts_read_as_windows_menus_write_them`、`a_recorded_key_becomes_the_shortcut`；截图 `settings-shortcuts`） |
| 快捷键 › 录制时接受的修饰键 | ⌘、⌃、⌥ 或 F 键 | Ctrl、Alt 或 F 键；带 Win 键的组合不接受（之前会被存下来，而 Win 组合键大多被系统占用），和提示「要带上 Ctrl 或 Alt」一致 | 一样（PR 4） | CI（单元测试） |
| 设置窗口高度跟着分页变 | `NSWindow setFrame`，带动画 | `SetWindowPos`，不超出工作区 | 一样 | CI（截图：`settings-profile` 是 640×633，`settings-about` 是 640×760，高度各随分页），顺滑程度要真机 |
| 机器 › 用这台电脑 | launchd | 计划任务 | 一样 | 归机器那条线 |
| 机器 › 添加电脑的安装命令 | 只有给 Mac / Linux 的 bash 命令 | 两个系统上都一样：添加成功后先选「那台电脑的系统」（Mac 或 Linux / Windows，默认是自己这台的系统），下面给对应的命令。Windows 的是 PowerShell 命令，做的事和 bash 那条相同：建只有当前用户、SYSTEM、Administrators 能打开的配置文件夹，写 token 和配置，再运行装好的应用自带的 daemon 的 `doctor` 和 `install-service`（计划任务）。和 bash 那条一样要求那台电脑先装应用，所以不下载东西，也就不需要 `-Sha256` | 一样 | 这一行两个系统都改了（Mac 上也多了这个选择）。没装应用的 Windows 电脑仍然用 `scripts/install-daemon.ps1`（见 `docs/bots.md`）：服务器没有分发这个脚本和 daemon 的接口，应用里给不出一条能直接下载的命令，已告知。CI（`the_windows_install_command_runs_in_powershell`：命令在 runner 的 Windows PowerShell 5.1 里真的跑，检查写出的配置、token 和文件夹的访问列表；截图 `settings-machine-created`），真机 |

## 8. 跟随系统

| 功能 | Mac 上 | Windows 上现在 | 状态 | 怎么补 / 验证 |
| --- | --- | --- | --- | --- |
| 浅色 / 深色 | `window.appearance()` | 同样 | 一样 | 真机 |
| 强调色 | 用户在系统设置里选了强调色就跟随，没选是石墨色 | 之前永远是石墨色。现在读 `UISettings` 的强调色，系统变化时更新。Windows 总有一个强调色，所以出厂的两种蓝（Windows 11 的 `#0078D4`、Windows 10 的 `#0078D7`）当作「没选」，仍是石墨色：和 Mac 上没选时一样，默认仍是黑白规范 | 一样（PR 4） | 按 WINDOWS.md §9；计划文档里「不接」的旧决定作废。CI（读到系统的颜色），真机 |
| 减弱动画 | `accessibilityDisplayShouldReduceMotion` | `SPI_GETCLIENTAREAANIMATION`，变化时更新 | 一样 | CI（读设置），真机 |
| 增强对比度 | `accessibilityDisplayShouldIncreaseContrast` | `SPI_GETHIGHCONTRAST` | 一样 | 真机 |

## 8.1 上面没有单独成行的按系统分支

对着 `grep -rnE 'cfg!?\(|cfg_attr\('`（筛 `target_os`、`windows`、`unix`）在范围内文件里的每一处核过；菜单、通知、托盘、
文件、凭证、登录时启动这些已经在上面各节里，下面是剩下的。

| 位置 | Mac 上 | Windows 上 | 状态 | 说明 |
| --- | --- | --- | --- | --- |
| `provider/session.rs` `device_name` 的兜底名 | 读不到主机名时叫「Mac」 | 叫「PC」 | 一样 | 各自系统的叫法；正常情况下用的是电脑名 |
| `provider/session.rs` `host_name` | `gethostname` | `COMPUTERNAME` | 一样 | 见第 4 节。CI |
| `provider/mod.rs`、`app_glue.rs` 选 `macos` / `windows` 模块 | | | 一样 | 两个模块对外是同一组函数 |
| `provider/system_events.rs`、`views/nav/tray_icon.rs` 的 `allow(dead_code)` | 不用这些纯函数 | 用 | 一样 | 只是为了让它们在每个系统上都跑测试 |
| `views/nav/menus.rs` `register_system_menus` | 告诉 AppKit 哪个是窗口菜单、帮助菜单 | 空 | 没有对应 | 就是第 3 节的窗口列表和菜单搜索 |
| `views/nav/menus.rs` 快捷键绑定 | ⌘H、⌥⌘H、⌘M、⌘W、⌃⌘F、⌃⌘空格 | Ctrl+W、F11 | 一样 / 没有对应 | 隐藏类没有对应；最小化和表情面板用系统自己的键（Win+↓、Win+.），菜单项都在。CI（`the_window_menu_commands_have_their_windows_keys`） |
| `views/nav/menus.rs`、`app.rs` 关闭窗口 | 任何窗口都关，应用留在 Dock | 托盘图标在时同样；图标被隐藏时不关主窗口和最后一个窗口 | 一样 | 见第 3 节「文件」一行 |
| `views/settings/shortcuts.rs` 快捷键的写法 | ⌃⌥⇧⌘ 符号 | `Ctrl+Alt+Shift+K` | 一样 | WINDOWS.md §3。CI（`shortcuts_read_as_windows_menus_write_them`） |
| `views/settings/window_fit.rs` 改窗口高度 | `setFrame`，系统做动画 | `SetWindowPos`，应用自己一步步改高度 | 一样 | CI（设置各分页的截图高度不同） |
| `views/settings/window_fit.rs` 首次定高前隐藏 | 透明度 0 再恢复 | DWM cloak 再取消 | 一样 | CI（设置的截图能截到，说明 cloak 取消了），打开时闪不闪要真机 |
| `views/settings/window_fit.rs` `pin_content_to_top` | 系统做高度动画时把上一帧钉在顶边，不被拉伸 | 空 | 没有对应 | 这是修 macOS 图层把旧帧拉伸的问题；Windows 上没有系统动画，每一步高度都是应用自己画的一帧，不存在被拉伸的旧帧。切换分页是否顺滑要真机 |
| `views/settings/window_fit.rs` `dim_window_buttons` | 最小化和缩放按钮变灰 | 去掉窗口的最小化和最大化能力（双击标题栏、Snap 最大化也跟着没有），标题条上的最大化按钮画成灰的 | 一样 | 结果相同：这个窗口只有一个尺寸。CI（设置的截图里最大化按钮是灰的） |
| `views/settings/login_item.rs` | `SMAppService` | 注册表 | 一样 | 见第 4 节。CI（对真的注册表读写） |
| `views/files/mod.rs` `opens_unsafely` / `open_kind` | 通用名单 | 通用名单加 Windows 名单 | 一样 | 见第 5 节。CI |
| `crates/chat-core/src/token.rs` `FileTokenStore` 的文件权限 | 0600 | 不设（Windows 没有权限位） | 一样 | 应用不用它存 token（Mac 用钥匙串，Windows 用凭据管理器）；它只给测试、Linux 和 `--sandbox` 开发模式用，文件在用户自己的配置目录里，由目录的访问列表保护 |
| `crates/chat-core/src/updates.rs` `PLATFORM` | `macos` | `windows` | 一样 | 归发布那条线 |
| `crates/chat-core/src/lib.rs` 导出哪个 token 存储 | 钥匙串 | 凭据管理器 | 一样 | 见第 6 节。CI |
| `main.rs` `windows_subsystem` | — | 正式版是窗口程序，后面不带控制台窗口；调试版保留控制台给日志和自测 | 没有对应 | Mac 的应用包本来就没有控制台。CI（`windows-installer` 任务启动装好的正式版） |
| `main.rs` `secondary!`、`command_key` | ⌘ | Ctrl | 一样 | 应用自己的快捷键和读修饰键的地方都经过这两个，Windows 键从不当命令键。CI（各快捷键测试在 Windows 上用 Ctrl 跑） |
| `main.rs` 启动时认领单实例、读链接和 `--dock-action` | 系统做 | 应用做 | 一样 | 见第 4 节 |

## 9. 发现的、归别的会话的事

写在这里是为了不丢，已经放进共享笔记（`win-parity-system.md`）：

- 输入框 › 截图：当时直接调 `/usr/sbin/screencapture`；界面那条线已改成 Windows 上用 `ms-screenclip:`。
- 粘贴图片：剪贴板里 `CF_DIB` 排在 PNG 前面时，贴进来的是没压缩的 BMP；复制图片出去时没有写 `CF_DIB`（GPUI 的剪贴板，界面）。
- 手动安装更新的文案是 Mac 的（`copy/update.rs`，发布；#149 已改）。
- 登录的设备里 Windows 电脑的图标（`data.rs`，界面；已改）。
- 添加电脑的 Windows 安装命令：已做（见第 7 节）。服务器没有分发 `install-daemon.ps1` 和 daemon 的接口，所以没装应用的电脑仍然要按 `docs/bots.md` 手动取脚本。
- 团队管理 › 机器的说明写「在 Mac 或 Linux 上装一个小程序」（`copy/admin.rs`，界面）：现在也能装 Windows 机器，这句话要跟着改。

## 10. PR

| PR | 内容 | 状态 |
| --- | --- | --- |
| #152 | 这份清单；任务栏未读角标；关于窗口；菜单补齐（关于、表情与符号、前置全部窗口、缩放能还原）；时区；下载文件的互联网标记 | 已合并 |
| #160 | 空格预览；共享；拖出文件 | 已合并 |
| #163 | 单实例、链接、跳转列表；通知（分组、图标、声音、退出后点击） | 已合并 |
| #165 | 强调色；快捷键录制；真机清单 | 已合并 |
| PR 5 | 复核后的修正：Ctrl+W 关主窗口；产出窗口也有角标；先设 AppUserModelID 再建跳转列表；给没验证过的行补 CI 测试；改正这份清单 | 已提 |
| PR 6 | 空格预览接系统的预览处理程序、网页附件在隔离视图里显示；添加电脑的 Windows 安装命令 | 已提 |

## 要在真机上确认

CI 的 Windows 机器有桌面会话，上面标了 CI 的行都对着真的系统跑过。下面是 CI 做不了的：要人看、要人点、要第二台
设备、要睡眠或换网络。

**要准备的**：一台 Windows 10 22H2 或 Windows 11 电脑；安装包（或 CI 的 `windows-installer` 产物），装好并登录；另一台
设备上的第二个账号，和你在同一个群里，能发消息；一个带 `.docx`、`.pdf`、图片和 `.bat` 附件的话题；能断开再连上的
网络（Wi-Fi 开关或网线）。

**要多久**：全部 28 步大约 45 分钟（第 27 步要第二台 Windows 电脑，没有就跳过），其中第 24 步要注销再登录一次，第 25 步要让电脑睡眠一次。只想先看最要紧的，
做第 1 到 9 步，约 12 分钟。每一步是一个动作和应该看到的结果；结果不对就是问题。

未读和通知

1. 把应用切到后台，让第二个账号发 3 条消息。→ 任务栏按钮上出现红底白字的 `3`，托盘图标带点；回到应用读完后两个都消失。
2. 打开设置窗口（Ctrl+,），再让第二个账号发 1 条。→ 主窗口和设置窗口的任务栏按钮上都有角标。
3. 打开一个产出的独立窗口，再让第二个账号发 1 条。→ 产出窗口的任务栏按钮上也有角标。
4. 有未读时，在任务管理器里重新启动「Windows 资源管理器」。→ 任务栏回来后几秒内，角标和托盘图标都回来。
5. 应用在后台时收一条消息。→ 弹出通知，标题上方有群名，图标是应用图标，有系统通知音；任务栏按钮不闪。
6. 设置 › 通用里关掉声音，再收一条。→ 有通知，不响。打开专注助手（勿扰）再收一条。→ 不弹、不响，通知中心里有，角标加一。
7. 点一条通知。→ 应用到前台并打开那个话题。
8. 再收一条，不点；从托盘菜单选退出；打开通知中心点那条旧通知。→ 应用启动并打开那个话题。

启动和窗口

9. 应用开着时从开始菜单再启动一次。→ 只是已有窗口到前台，托盘里仍然只有一个图标。
10. 在主窗口按 Ctrl+W。→ 主窗口关掉，托盘图标还在；点托盘图标，主窗口回来。
11. 设置 › 通用里关掉「通知区域里显示」，在主窗口按 Ctrl+W。→ 什么都不发生。点关闭按钮。→ 应用退出（任务管理器里没有它了）。重新启动，把开关打开。
12. 右键任务栏按钮。→ 跳转列表里有新话题、新建群…、发起私聊…、显示主窗口；各点一次，应用里出现对应的界面。
13. 在话题里对一条消息选「复制链接」，粘贴到 Win+R 里运行。→ 应用到前台并打开那个话题。
14. 帮助 › 关于。→ 出现小的关于窗口（图标、名称、版本），Esc 关掉。
15. 在输入框里点编辑 › 表情与符号。→ 出现系统的表情面板，选一个表情能输入进去。
16. 窗口 › 最大化或还原点两次。→ 窗口最大化，再回到原来的大小。窗口 › 最小化。→ 窗口收到任务栏。
17. 开着设置窗口时，切到别的应用再切回主窗口，点窗口 › 前置全部窗口。→ 两个窗口都在别的应用前面。
18. 设置里依次点五个分页。→ 窗口高度跟着变，顶边不动，变化顺滑，打开时不闪一下再跳到位。

文件

19. 把指针放在图片、PDF、视频、`.md`、`.csv`、`.txt` 附件上各按一次空格。→ 应用里出现预览；再按空格关掉。
20. 对 `.docx`（装了 Office 的电脑）按空格。→ 面板里是 Word 文件的内容。对一个没有程序能预览的类型（比如 `.zip`）按空格。→ 面板写「这台电脑没有能预览这种文件的程序」，有「用系统应用打开」。对 `.html` 附件按空格。→ 面板里是渲染出来的网页。对 `.bat` 按空格。→ 不预览，只在资源管理器里选中。
21. 右键一个文件选「共享…」。→ 出现 Windows 的共享界面，里面是这个文件。
22. 右键一个文件选「存储为…」，存到桌面。→ 出现系统的保存对话框，默认在「下载」；桌面上有这个文件。对存下来的 `.docx` 右键 › 属性。→ 底部有「此文件来自其他计算机」。
23. 把一个文件从话题里拖到桌面。→ 桌面上多出这个文件。再拖一次，途中按 Esc。→ 什么都不发生，之后应用里点击仍然正常。

系统

24. 设置 › 通用里打开「登录时启动」，注销再登录。→ 应用自己启动。在任务管理器 › 启动应用里把它禁用，回到设置。→ 开关显示为关。
25. 让电脑睡眠一分钟再唤醒。→ 几秒内连接恢复，第二个账号这时发的消息马上出现。关掉 Wi-Fi 十秒再打开。→ 同样几秒内恢复。
26. 系统设置 › 个性化 › 颜色里把主题色改成红色。→ 应用里的强调色几秒内变红；改回默认蓝，应用回到石墨色。系统设置 › 辅助功能 › 视觉效果里关掉动画效果。→ 应用里的装饰动画停掉。
27. 团队管理 › 机器 › 添加电脑，填名字后添加，系统选 Windows，把命令复制到另一台装了应用的 Windows 电脑的 PowerShell 里运行。→ 几秒后那台电脑在机器列表里显示在线。
28. 设置 › 账号里点更换头像，选一张图片。→ 出现系统的打开对话框，选完后头像换掉。设置 › 快捷键里录制时按 Win+K。→ 被拒绝并提示要带上 Ctrl 或 Alt；按 Ctrl+Alt+K 能存下来。设置 › 通知里打开勿扰并设一个包含当前时间的时段，第二个账号发消息。→ 这台电脑不提醒。
