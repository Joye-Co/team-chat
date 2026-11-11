# Windows 对齐清单：安装、更新、卸载、发版

范围：一个人从下载到卸载会碰到的每一步，和运营方发一个版本要做的每一步。左边是 Mac 现在的做法，右边是
Windows 现在的做法，都是从代码和脚本里读出来的（括号里是出处）。客户端的系统集成（通知、托盘、链接跳转、
单实例）、界面、daemon 和「用这台电脑」各有自己的清单；这里只在它们和安装包、更新有关的地方提到。

状态的写法：

- **相同**：两边结果一样（实现可以不同）。
- **已补**：原来有差距，已经改掉；写明改在哪个 PR、怎么验证的。
- **待补**：有差距，还没改完；写明谁来改。
- **无对应**：Windows 没有这个东西，写明原因和替代做法。

验证方式：没人有 Windows 电脑。「CI 实测」指 `windows-installer` 任务在 `windows-2025` runner 上真的装、
跑、卸了一遍；「只编译」指只过了 `cargo check --target x86_64-pc-windows-gnu` 和 Windows CI 的编译。

## 一、用的人这一侧

### 1. 下载

| 项 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| 邀请链接 `<服务器>/download#code=<邀请码>` | 页面脚本把邀请码用 POST 发回去，拿到最新的 DMG（`server/src/files/ops.ts`） | 同一个页面；浏览器的 User-Agent 里有 Windows 时多发 `platform=windows`，拿到最新的 `-setup.exe` | 相同。服务器测试覆盖（`server/test/files.test.ts`「keeps the Windows installer apart from the DMG」） |
| 旧式链接 `/download?code=<邀请码>`（邀请码在地址里，老版本客户端生成的） | 给 DMG | 原来也给 DMG，Windows 上没法用 | **已补**（PR 1）：请求没写 `platform` 时，服务器看 `User-Agent`，是 Windows 就给安装包；写了 `platform` 的以写的为准。新增测试：Edge 的 UA 拿到安装包，Safari 的 UA 拿到 DMG，`platform=macos` 能让 Windows 浏览器拿 DMG。`docs/protocol.md` 已更新 |
| 这个平台还没有版本时 | 404「No release yet.」 | 404「No release for this system yet.」，不会给 DMG | 相同 |
| 客户端里的下载接口 `GET /v1/updates/latest`、`/v1/updates/:version/download` | 不带参数 | 带 `?platform=windows`；回答的 `platform` 不是 `windows` 就当没有（`crates/chat-core/src/updates.rs`） | 相同 |
| GitHub Releases | `<名字>-<版本>.dmg`；校验和是 GitHub 在 Assets 里显示的 `sha256:` | `<名字>-<版本>-setup.exe`，同样看 `sha256:`（`release.yml` 的 `windows-publish`，`check-release-digest.sh`） | 相同。1.2.7 起每个版本的 Assets 里都有安装包（`TeamChat-1.2.7-setup.exe` 是第一个） |
| 官网（`apps/website`） | 没有下载按钮，只有文字 | 没有下载按钮；文字写着 Windows「正在开发」 | 相同（都没有按钮）。1.2.7 已经有 Windows 版，这句文字该改了：官网首页正被另外两个 PR 重写，留给运营方，见第三节 |
| 不装客户端、只装 daemon | `<slug>-daemon-<版本>-aarch64-apple-darwin`（`build-daemon.sh`） | `<slug>-daemon-<版本>-x86_64-pc-windows-msvc.exe`（`build-windows-installer.sh`），配 `scripts/install-daemon.ps1` | 相同 |

### 2. 校验

| 项 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| 人手动校验 | `shasum -a 256 <dmg>`，和 Assets 里的 `sha256:` 对比 | `Get-FileHash`，和 Assets 里的 `sha256:` 对比（INSTALL 文档） | 相同 |
| 系统对来源的检查 | 没有公证：第一次打开要去「隐私与安全性」点「仍要打开」，或 `xattr -dr com.apple.quarantine` | 没有证书时 SmartScreen：「更多信息 → 仍要运行」；有证书后按信誉逐步消失 | 相同（都要手动放行一次；证书见第三节） |
| 签名 | 自签证书，designated requirement 固定到证书（`build-dmg.sh`、`test-dmg.sh`） | Authenticode，可选；`scripts/windows/sign.sh` 给客户端、更新程序、daemon、卸载程序、安装包签名 | 相同（签名路径 CI 用临时证书实测） |

### 3. 安装和第一次打开

| 项 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| 安装 | 打开 DMG，拖到「应用程序」 | 双击安装包；只装给当前用户，不要管理员；`/S` 静默 | 相同。CI 实测 |
| 系统要求 | Apple 芯片，macOS 13+（`Info.plist` 的最低版本） | 64 位 Windows 10 22H2+；安装包在更低的系统上直接退出并说明 | 相同 |
| 程序的架构 | 只有 arm64；`test-dmg.sh` 检查 | 只有 x64；原来测试不检查 | **已补**（PR 1）：`test-windows-installer.sh` 读每个 exe 的 PE 头，必须是 x64 |
| 另一种芯片 | Intel Mac 装不了 | ARM 的 Windows，见下面「Windows on ARM」 | 相同（都只出一种架构） |
| 带的东西 | 客户端、daemon（在 `.app` 里）、图标、URL scheme（`Info.plist`） | 客户端、更新程序、daemon、卸载程序、图标和版本资源、URL scheme（注册表）、开始菜单快捷方式（带 AppUserModelID）、WebView2 bootstrapper | 相同。CI 实测 |
| 内嵌网页的运行时 | 系统自带 WKWebView | Windows 11 自带 WebView2；没有时安装包联网装 Evergreen runtime，装不上也不拦安装 | 无对应的部分已有替代。CI 只走过「已经有」这条路，「没有」要真机（第三节） |
| 服务器地址 | 发版的构建从 `deploy/servers.json`（或 `TC_SERVERS_JSON`）嵌进去，登录页默认连正式服务器（`release.sh` 的 `release_ops`、`apps/desktop/build.rs`） | 发版 workflow 的 Windows 任务原来自己拼了一个 ops 目录；CI 的安装包没有地址，登录时要手填 | **已补**（PR 1）：`build-windows-installer.sh` 自己做和 `release_ops` 一样的事（只嵌 `servers.json`，不嵌 ops 目录里别的东西）；发版时 `REQUIRE_SERVERS=1`，没有地址就停；CI 从仓库变量 `SERVERS_JSON` 拿，仓库里不放地址。`--selftest-installed` 报告嵌进去的地址的 SHA-256（日志里不出现地址），`test-windows-installer.sh` 核对它和给构建的一致；没给地址时核对确实没有 |
| 登录 | 邀请码或邮箱验证码；凭证进钥匙串（service = bundle id，account = 服务器地址） | 同一个登录页；凭证进凭据管理器（`<bundle id>:<服务器地址>`） | 相同（只编译和单元测试；真机见第三节） |
| 登录窗口里换服务器 | 系统菜单栏里有「正式服务器 / 测试服务器」 | 菜单只挂在主窗口标题栏里，登录窗口没有；可以点「使用其他服务器…」手填 | 待补，属界面会话（已写进交接笔记） |
| `<scheme>://` 链接打开客户端 | `Info.plist` 登记了 scheme，系统会启动或激活客户端；客户端没有注册 `on_open_urls`，不跳到链接指的地方 | 安装包登记 `HKCU\Software\Classes\<scheme>`，链接作为参数启动客户端；参数被忽略，不跳转 | 相同（两边都只打开客户端）。跳转属系统集成会话 |
| 客户端已在运行时再点链接或再启动一次 | 系统只激活已有的那一个 | 会启动第二个完整的进程（没有单实例） | 待补，属系统集成会话（已写进交接笔记） |

### 4. 更新

检查、提示、下载、说明页这几步是同一份代码（`apps/desktop/src/updater/mod.rs`、`banner.rs`、`notes.rs`），没有按
系统分支；下表只列出处和各自的验证。

| 项 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| 什么时候检查 | 登录后立刻一次；成功后每 6 小时，失败后 30 分钟；调度器每 20 秒看一次到没到点 | 同一份代码 | 相同 |
| 问哪台服务器 | 当前账号所在的服务器；切换账号后问新的 | 同上 | 相同 |
| 客户端开着好几天 | 每 6 小时再查；有新版本自动下载（设置里可关）；已经下好一个版本后不再查，直到重启 | 同上 | 相同 |
| 「稍后」 | 只记在内存里，24 小时不再提示这个版本；更新的版本照样提示 | 同上 | 相同 |
| 提示条、侧栏卡片、说明页、更新记录、「已更新到」 | `banner.rs`、`notes.rs`；说明来自编进客户端的 `docs/releases/*.md` 和服务器给的 `notes` | 同一份代码、同一批说明 | 相同（截图自测在 Windows CI 上跑） |
| 说明的内容 | `docs/releases/*.md` 是写给所有人的 | 以前的说明里有只和 Mac 有关的句子 | 相同（文件是共用的）。以后的说明怎么写见第三节 |
| 下载和进度 | 流式写到 `<数据目录>/updates/<名字>-<版本>.dmg.part`，带进度和取消；不支持断点续传 | 同一份代码，文件是 `-setup.exe` | 相同。CI 实测（`--selftest-update`） |
| 下载后的校验 | 大小和 SHA-256，对了才改名；安装前再算一次 | 同上；更新程序开始前再算一次；装的版本有签名时还要求同一个发布者的有效签名 | 相同（Windows 多一道签名） |
| 安装 | 客户端自己做：挂载 DMG，检查 bundle id、版本、designated requirement，`ditto` 到旁边，改名换掉旧的 | 运行中的 exe 不能覆盖，所以交给更新程序（`apps/update-helper`）：等客户端退出，把旧文件挪到备份目录，静默跑安装包 | 无对应的部分已有替代。CI 实测 |
| 重新打开 | 一个 shell 等旧进程退出后 `open` 新的 | 更新程序启动新版本 | 相同。CI 实测 |
| 新版本起不来 | 换文件这一步失败会放回旧的；新版本启动后崩溃没有回滚 | 新版本 90 秒内没有确认，更新程序把旧版本放回去并重新打开 | 相同（Windows 多一层）。CI 实测 |
| 更新被打断（断电、注销） | 旧 bundle 留在旁边，下次启动清理 | 日志（journal）加 `RunOnce`：下次登录、下次启动、下次更新前恢复 | 相同。CI 实测（真的杀掉更新程序再恢复） |
| 不能自动装的时候（装在不可写的位置、开发构建） | 在访达里打开 DMG，文字教人拖到「应用程序」 | 在资源管理器里选中安装包；但文字还是访达和「应用程序」那一套 | **已补**（PR 1）：这几句文案按系统分开（`copy/update.rs` 的 `per_system`），Windows 上说「安装包已在文件资源管理器中选中，退出后双击安装」 |
| daemon 跟着客户端换 | daemon 在 `.app` 里，文件被换掉后空闲时自己退出，launchd 拉起新的 | 安装包按「改名替换」协议换文件，不停进程；daemon 空闲时退出，计划任务一分钟内拉起新的 | 待补（PR 2）：文件被换掉 CI 已实测；「正在跑的 daemon 换到新版本」整条链路在 PR 2 里放到 runner 上跑 |
| 几个账号 | 更新只看当前账号的服务器；装的是同一个客户端 | 同上 | 相同 |
| 还有消息没发出去时点了安装 | 先等发件箱清空（每 2 秒看一次，`SEND_POLL`），界面写「还有消息正在发出，发完后自动更新」；点「稍后」就不等了 | 同一份代码，等完之后才启动更新程序、退出客户端 | 相同 |
| 更新程序里的测试开关 | 没有更新程序 | `UPDATE_HELPER_TEST_PAUSE_AT` 原来在发出去的程序里 | **已补**（PR 1）：只有带 `test-pause` feature 编译的更新程序才认这个变量；CI 单独编一个给「打断」测试用，并检查装好的更新程序里没有这个字符串 |

### 5. 诊断和日志

| 项 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| 日志在哪 | `~/Library/Application Support/<bundle id>/logs/app.<日期>.log`，按天一个，留 14 个（`chat-core/src/logging.rs`） | `%APPDATA%\<bundle id>\logs\`，同一份代码 | 相同。文档原来写的是 `~/Library/Logs/…`，和代码不符，**已改**（PR 1） |
| 更新的日志 | 写在应用日志里 | 更新程序另写 `%APPDATA%\<bundle id>\updates\update.log` 和 `update-result.json`；安装包写 `<安装目录>\install.log` | 相同（Windows 多两个文件，INSTALL 文档已写） |
| daemon 的日志 | `~/Library/Logs/<slug>-daemon/` | `%LOCALAPPDATA%\<slug>-daemon\daemon.log` | 相同 |
| 「发送诊断信息」 | 设置 › 诊断：说明文字、应用版本、系统类型和架构、最近的日志（去掉凭证，gzip），`POST /v1/diagnostics` | 同一份代码；系统类型是 `windows x86_64` | 相同 |
| 「发送诊断信息」实际带上的日志 | 上传时读的是「账号目录」下的 `logs`，日志写在「数据根目录」下的 `logs`，所以**两边现在都带不上日志** | 同左 | 相同（同一个缺陷）。不在本清单的范围里改（会改 Mac 的行为），已报给总控 |
| 打不开时手动取日志 | INSTALL 文档给了 `zip` 命令 | 原来没写 | **已补**（PR 1）：INSTALL 文档加了 `Compress-Archive` 命令和 `update.log` 的位置 |
| 崩溃报告 | 没有自己的崩溃上报；系统的崩溃报告在「控制台」里 | 没有自己的崩溃上报；系统的在「事件查看器 › Windows 日志 › 应用程序」 | 相同 |

### 6. 卸载

Mac 没有卸载程序：把 `.app` 拖进废纸篓，别的都留下。Windows 的卸载程序以这个为下限：是人的东西留下，
是程序造出来、能再造出来的都删掉。

| 东西 | Mac（拖进废纸篓后） | Windows（卸载后） | 状态 |
| --- | --- | --- | --- |
| 程序本身 | 删 | 删（客户端、更新程序、daemon、卸载程序、`install.log`、更新留下的备份目录） | 相同。CI 实测 |
| 开始菜单快捷方式、卸载项、URL scheme、AppUserModelID | 无对应（Launch Services 自己忘掉） | 删 | 无对应；已删。CI 实测 |
| 「登录时打开」 | 系统管的登录项，App 没了就失效 | 删 `Run` 下的值；原来留下任务管理器记的那条 `StartupApproved\Run` | **已补**（PR 1）：两处都删，CI 实测 |
| 更新的恢复登记（`RunOnce`） | 无对应 | 删 | 无对应；已删 |
| daemon 的开机自启 | **留下** `~/Library/LaunchAgents/<bundle id>.daemon.plist`，指向一个已经不在的程序 | 先跑 `<daemon> uninstall-service` 删掉计划任务，再结束进程 | 相同（Windows 更干净）。PR 2 在 CI 上实测 |
| 缓存（能再生成的） | 留下 `~/Library/Caches/team-chat/diagrams` | 原来留下 `%LOCALAPPDATA%\team-chat\webview2` 和 `diagrams` | **已补**（PR 1）：卸载时删，CI 实测。理由：Windows 用户认为卸载会清掉缓存，WebView2 的用户数据目录还可能有几十 MB |
| 消息缓存、设置、日志 | 留下 `~/Library/Application Support/<bundle id>` | 留下 `%APPDATA%\<bundle id>` | 相同。CI 实测「卸载后数据还在」 |
| 登录凭证 | 留在钥匙串里 | 留在凭据管理器里 | 相同。重装后不用再登录 |
| 这台电脑当机器的身份 | 留下 `~/.config/<slug>`（配置和机器 token）和 daemon 日志 | 留下 `%USERPROFILE%\.config\<slug>` 和 `%LOCALAPPDATA%\<slug>-daemon\daemon.log` | 相同。重装后再点「用这台电脑」接着用原来那台机器 |
| 想删干净 | INSTALL 文档给了路径和 `security delete-generic-password` | INSTALL 文档原来只说去凭据管理器里找 | **已补**（PR 1）：给了每个路径和删凭据的 PowerShell 命令；凭据的名字写对了（`<bundle id>:<服务器地址>`） |

### 7. 开发和自测用的开关

| 开关 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| `--selftest-installed --report` | 能用 | 能用；现在还报告嵌进去的服务器地址的 SHA-256 | 相同 |
| `--selftest-update <url> --token-file … --install-dir …` | 对着本地服务器真的更新一次（`updater/e2e-install-dir.sh`） | 同名开关，走 Windows 的更新程序（`scripts/test-windows-update.sh`） | 相同。Windows 的每个 PR 都在 CI 上跑；Mac 的两个脚本只能手动跑 |
| `--selftest-updated <marker>` | 新版本第一次启动时做的事 | 同一份代码 | 相同 |
| `--screenshot`、`--selftest-smoke` 等界面自测 | 能用 | 能用（`desktop-windows` 任务） | 相同 |
| 开发构建不自己更新 | 不在 `.app` 里运行时走「手动安装」 | 旁边没有更新程序和卸载程序时走「手动安装」 | 相同 |

### Windows on ARM

Mac 只出 Apple 芯片的版本；Windows 只出 x64 的版本。ARM 的 Windows 电脑：

- **Windows 11 on ARM**：系统能模拟运行 x64 程序。安装包本身是 32 位的 NSIS 程序，也在模拟下运行。按文档
  推断能装、能用：WebView2 的 Evergreen runtime 有 ARM64 版，x64 的宿主进程可以用它；daemon 用到的 Job
  Object、ConPTY、命名管道、计划任务都不分架构，x64 的 daemon 可以启动 ARM64 原生的 CLI。GPUI 用的
  Direct3D 11 在模拟下可用，性能会差一些。**没有验证过**（没有这样的电脑，CI 也没有 ARM 的 Windows runner
  跑这个任务）。
- **Windows 10 on ARM**：只能模拟 32 位 x86，x64 程序跑不了。原来安装包会装上去、然后客户端打不开；
  **已补**（PR 1）：安装包在 ARM 的 Windows 10 上直接退出并说明要 Windows 11。这个判断只能编译验证。
- 不出 ARM64 原生版的理由：和 Mac 不出 Intel 版一样，一个平台一种架构；要出的话是另一个安装包、另一条
  CI，和 `docs/plans/windows.zh.md` 里「范围以外」的决定一致。

## 二、运营方这一侧

| 项 | Mac | Windows | 状态 |
| --- | --- | --- | --- |
| 预发布版本（`1.3.0-rc.1`） | 出 DMG，只放 GitHub，不上传服务器（客户端只更新到正式版本） | 原来什么都不出：workflow 跳过 Windows 任务，构建脚本也拒绝这种版本号 | **已补**（PR 1）：Windows 任务照样构建、测试安装包，`windows-publish` 只把它加进 GitHub Release、不上传服务器；安装包的四段数字版本取 `-` 前面的部分。本机用 `1.3.0-rc.1` 编过 NSIS 脚本；整条 workflow 要等下一次预发布才真的跑到 |
| 服务器地址不进日志 | 原来 `release.yml` 把仓库变量放进 `env` 和 `GITHUB_ENV`，每一步的日志开头都会把地址印出来（仓库是公开的，日志也是） | 我在 PR 1 第一版里给 CI 的安装包任务也这么传了，同样印了出来 | **已补**（PR 1）：用到地址的任务都挪进被调用的 workflow（`release-jobs.yml`、`windows-installer.yml`），地址作为它的 secret 传进去，日志里是 `***`；每个任务第一步再把每个地址和主机名登记成要遮掉的字符串；不再写进 `GITHUB_ENV`；最后一个任务 `No server address in the logs` 读本次运行每个已完成任务的日志，出现主机名就失败（`scripts/check-logs-for-servers.sh`，有测试）。**以前的运行日志里还有这些地址**，只有运营方能删，见第三节 |
| 发版入口 | `scripts/release.sh <版本>`（本机或 `release.yml` 的 macOS 任务）：检查、改版本号、构建、测试、提交、打 tag、建 GitHub Release、上传服务器 | `release.yml` 的 `windows` 任务构建、签名、测试；`windows-publish` 把安装包加进同一个 GitHub Release 并上传服务器 | 相同。Windows 的构建只能在 Windows 上跑，所以没有「本机发版」；`release.sh` 对 `--platform windows` 只做上传 |
| `release.sh` 的模式 | `--dry-run [--build]`、正式、`--upload-only`、`--same-as` | workflow 的 `dry-run`（构建、签名、测试，不发）、`release`、`upload-only`（从 GitHub Release 取回安装包再上传） | 相同 |
| 版本号 | `release.sh` 改 `Cargo.toml`、`Cargo.lock`、CHANGELOG 日期并提交 | Windows 任务在自己的工作区里设同一个版本号，不提交 | 相同 |
| 说明 | `docs/releases/<版本>.md` 上传给服务器，也编进客户端 | 同一个文件 | 相同 |
| 嵌进去的服务器地址 | `release_ops`：只嵌 `servers.json` | 见上「服务器地址」 | 已补（PR 1） |
| 签名 | `SIGNING_KEY_PEM`、`SIGNING_CERT_PEM`；正式发版没有就拒绝 | `WINDOWS_SIGN_PFX_BASE64`、`WINDOWS_SIGN_PASSWORD`，或 `WINDOWS_SIGN_CMD`；没有就不签，照样发 | 相同（Windows 允许不签：没有证书时也要能发，这是本次的前提） |
| 和上一个版本是同一个发布者 | `test-dmg.sh --same-as <上一版 DMG>`：designated requirement 不变，钥匙串才继续信任 | 原来没有：换了证书或忘了签，已安装的客户端会拒绝更新，发出去才知道 | **已补**（PR 1）：`scripts/previous-windows-installer.sh` 取「不是本次版本、且带安装包的最新一个 Release」的安装包（Mac 任务先建好本次的 Release 也不会拿错；列表、读取、下载任何一步失败都让任务失败，只有「以前没有任何安装包」才跳过），`test-windows-installer.sh --same-as` 再让**装好的更新程序自己**回答「上一版的客户端收不收这个安装包」（`<slug>-update.exe --accepts`，就是更新时用的 `signer_allows`，所以发版检查和真实更新不会判得不一样）。测试：`test-release-scripts.sh` 用假的 `gh` 测选哪个版本和各种失败；CI 用本次运行的四个安装包、一个别家签名的程序、以及把测试证书从受信任的根里拿掉之后的情况测这条规则 |
| 构建产物的检查 | `test-dmg.sh`：校验和、镜像完整、bundle id、名字、版本、最低系统、URL scheme、图标、带了 daemon、arm64、签名 | `test-windows-installer.sh`：校验和、真的安装、文件和版本资源、注册表、快捷方式和 AppUserModelID、x64、图标、带了 daemon、签名、启动并画一屏、覆盖安装、卸载后没有残留、数据还在 | 相同（Windows 的更重：真的装） |
| 上传 | `PUT /v1/admin/releases/<版本>` | `PUT /v1/admin/releases/<版本>/assets/windows` | 相同 |
| 撤回 | `withdraw-release`：这个版本不再被提供、不能下载 | 同一个命令，两个平台的文件一起撤回 | 相同。不能只撤一个平台：RUNBOOK 已写明做法（撤回后两边一起发更高的版本） |
| 退回上一个版本 | 让人下载上一版 DMG 拖进去 | 原来没写 | **已补**（PR 1）：RUNBOOK 写了「退出客户端，运行上一版的 `-setup.exe` 覆盖安装」 |
| `release-check.sh` | 检查版本号、`Cargo.lock`、CHANGELOG、说明、tag、服务器健康 | 同一个脚本，和平台无关 | 相同 |
| `test-release-scripts.sh` | 测 `deploy-server.sh` 和 `release-check.sh` 的防呆；不测 `release.sh` | 同左；`release.sh --upload-only --platform windows` 没有测试 | 待补（PR 2）：给 `release.sh` 的上传加一个对着本地假服务器的测试，两个平台的路由都测 |
| staging 冒烟（`staging-smoke.ts`） | 不查更新和下载接口 | 同左 | 相同 |
| CI | DMG 的构建和测试只在手动触发时跑 | 安装包的构建、安装、更新、回滚、恢复、卸载在每个碰到相关文件的 PR 上跑 | 相同（Windows 的更多） |

## 三、要运营方或真机做的

这些事代码和 CI 做不了。步骤写全，照着做就行。

### 1. 代码签名证书（不做也能发版，只是有 SmartScreen 提示）

1. 买一张 Windows 代码签名证书（OV 或 EV，签发给运营方的公司或个人名）。2023 年 6 月以后签发的证书私钥
   必须在硬件或云端密钥库里，通常拿不到 PFX 文件。
2. 能拿到 PFX 的：在仓库的 Settings › Secrets and variables › Actions 里加两个 secret：
   `WINDOWS_SIGN_PFX_BASE64`（`base64 -i cert.pfx | tr -d '\n'` 的输出）和 `WINDOWS_SIGN_PASSWORD`。
3. 拿不到 PFX 的（云端签名，如 Azure Trusted Signing、DigiCert KeyLocker）：在 `release.yml` 的 Windows 任务
   里设 `WINDOWS_SIGN_CMD`，值是一条「给最后一个参数指的文件签名」的命令（`scripts/windows/sign.sh` 开头有
   说明），凭据放 secret。
4. 跑一次 `mode=dry-run` 的 Release workflow：「Test the installer」里每个程序的签名都应该是 `Valid`。
5. 从签名的第一个版本起，以后每个版本都要用同一个主体（Subject）的证书签；`--same-as` 那一步会在发版前
   拦住不一致的情况。

### 2. 发版（1.2.7 已经带着 Windows 安装包发过一次）

1. 服务器要先部署到有 `release_assets` 表和 Windows 上传接口的版本（1.2.7 时已经是）。
2. 仓库变量 `SERVERS_JSON` 是 `{"staging": "<地址>", "production": "<地址>"}`，不用动。
3. Actions › Release：`version` 填新版本号，先 `mode=dry-run`，看 `Windows installer` 任务全绿；想先在
   Windows 电脑上装一次，从产物 `windows-installer-<版本>` 里拿安装包。
4. 再跑 `mode=release`。完成后检查：GitHub Release 的 Assets 里有 `-setup.exe` 和 Windows 的 daemon；
   `curl -H "Authorization: Bearer <token>" "<服务器>/v1/updates/latest?platform=windows"` 返回这个版本；
   最后一个任务 `No server address in the logs` 是绿的。
5. Windows 任务失败而 Mac 已经发出去：在 `main` 上修好，在那次运行上点 **Re-run failed jobs**。
6. 官网（`apps/website/src/index.html`）还写着「Windows 版正在开发」，改成已支持并加上下载说明。
7. 写 `docs/releases/<版本>.md` 时，只和一个系统有关的条目标明「Mac：」或「Windows：」，因为两边的
   「更新记录」显示的是同一份说明。

### 2b. 删掉旧日志里的服务器地址

这个仓库是公开的，Actions 的日志任何人都能看。PR #149 合并之前的运行（`Release` workflow 的每一次运行，
以及 PR #149 自己最早的几次 CI 运行）日志里有 staging 和 production 的地址。新的运行不会再有。旧的只有
仓库管理员能删：

```bash
# 列出 Release workflow 的所有运行，逐个删掉日志（运行记录本身还在）
gh run list --workflow release.yml --limit 200 --json databaseId -q '.[].databaseId' |
  xargs -I{} gh api -X DELETE repos/Joye-Co/team-chat/actions/runs/{}/logs
# PR #149 分支在修好之前的 CI 运行
gh run list --branch claude/win-parity-release-76fdb9 --limit 50 --json databaseId -q '.[].databaseId' |
  xargs -I{} gh api -X DELETE repos/Joye-Co/team-chat/actions/runs/{}/logs
```

地址本身不是密钥（发出去的客户端里就有），但仓库的规则是部署信息不出现在仓库里；如果介意已经被人看到，
换地址是另一回事，这里不替运营方决定。

### 3. 真机上看一遍（一台 Windows 11 x64；有条件再加一台 Windows 10 22H2 和一台 ARM 的 Windows 11）

1. 用 Edge 打开邀请链接：下载下来的是 `-setup.exe`。
2. 双击安装：SmartScreen 的样子和 INSTALL 文档写的一致；安装界面是中文；装完开始菜单里有图标。
3. 打开，登录页默认连的是正式服务器，用邀请码登录；关掉再开不用再登录。
4. 设置 › 关于 › 检查更新。发下一个版本后：提示条出现 → 下载有进度 → 「重启安装」→ 一分钟内新版本打开，
   「已更新到」的说明出现。
5. 设置里点过「用这台电脑」的：更新后几分钟内，设置 › 机器里这台电脑的 daemon 版本变成新的，期间在跑的
   bot 没有被打断。
6. 设置 › 诊断 › 发送诊断信息，管理员那边能收到。
7. Windows 10 上没有 WebView2 的电脑：安装时联网装上了 runtime，产出页面能显示。
8. 卸载：设置 › 应用里卸载后，开始菜单、`teamchat://` 链接、计划任务都没了；重装后不用再登录。
9. ARM 的 Windows 11：能装、能打开、产出页面能显示、「用这台电脑」能跑一个 bot。

### 4. SmartScreen 信誉

新证书（OV）签的安装包一开始仍然会被 SmartScreen 拦，下载和安装的人多了以后提示才消失；EV 证书和
Azure Trusted Signing 一开始就有信誉。这段时间 INSTALL 文档里「更多信息 → 仍要运行」的说明保持不变；
也可以把安装包提交到 <https://www.microsoft.com/wdsi/filesubmission> 请微软复核，加快这个过程。

## 四、PR

- PR 1：本清单；服务器地址、邀请链接、测试开关、卸载、ARM、同一发布者检查、文案、文档。
- PR 2：整条链路在 runner 上跑（安装 → 连本地服务器 → daemon 的计划任务 → 更新 → daemon 换到新版本 →
  卸载）；通知的 COM 激活器登记；`release.sh` 上传的测试。
