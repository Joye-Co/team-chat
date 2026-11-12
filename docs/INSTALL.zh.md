# 在 Mac 上安装 Cahoots

English version: [INSTALL.md](INSTALL.md)。Windows 电脑看下面的[在 Windows 上安装](#在-windows-上安装)。

应用登录的是团队的服务器。还没有服务器的话（一个人用，或者你是搭团队的人），先自己部署一个：
[SELF-HOSTING.zh.md](SELF-HOSTING.zh.md)。它最后会给你一个登录码：第 4 步登录时先选「使用其他服务器…」填你的服务器地址，
再填这个登录码。

需要一台 Apple 芯片(M1 或更新)的 Mac,系统 macOS 13 或更新。这个 App 没有经过 Apple 公证,
所以第一次打开时 macOS 会要你手动允许一次(第 3 步)。

## 1. 下载

两种方式任选一种:

- **邀请链接**:打开管理员发给你的链接,会下载最新的 `Cahoots-<版本号>.dmg`。
  链接附带的**邀请码**请保存好,第 4 步要用。
- **GitHub Releases**(有仓库权限的同学):打开
  <https://github.com/Joye-Co/team-chat/releases>,选最新的版本,在 Assets 里下载
  `Cahoots-<版本号>.dmg`。也可以在终端里运行:

  ```sh
  gh release download --repo Joye-Co/team-chat --pattern 'Cahoots-*.dmg'
  ```

可选:检查文件是否完整(输出要和该版本 Assets 里 DMG 旁边显示的 `sha256:` 一致):

```sh
shasum -a 256 Cahoots-<版本号>.dmg
```

## 2. 安装

1. 双击下载好的 DMG。
2. 在弹出的窗口里,把 **Cahoots** 拖到 **Applications(应用程序)** 文件夹上。
3. 推出磁盘映像(在访达侧边栏里点它旁边的推出按钮)。

## 3. 第一次打开

从"应用程序"或启动台打开 Cahoots。macOS 会提示"无法验证"这个 App。点 **完成**
(**不要**点"移到废纸篓"),然后:

1. 打开 **系统设置 → 隐私与安全性**。
2. 往下滑,找到关于 Cahoots 的提示,点 **仍要打开**。
3. 输入 Mac 的登录密码,再点一次 **仍要打开**。

每次手动安装新的一份只需要做一次。也可以不走上面的步骤,在终端里运行下面这行,然后正常打开:

```sh
xattr -dr com.apple.quarantine /Applications/Cahoots.app
```

## 4. 登录

1. 在登录界面粘贴邀请码。
2. 填写你的名字。名字、岗位和头像以后都可以在"设置"里改。

邀请码只能用一次。App 会用它换成一个登录凭证,存在你 Mac 的"钥匙串"里,所以重启后仍然是登录状态。
**不要把邀请码发给别人,也不要发到群里。** 如果邀请码用不了,找管理员要一个新的。

## 5. 更新

App 会自己检查服务器上有没有新版本,有的话会提示你安装,装完自动重启。

想手动更新:按第 1 步下载新的 DMG,再把 App 拖到"应用程序"里,选 **替换**。
手动下载的新版本需要再做一次第 3 步。

## 数据存在哪里

| 内容 | 位置 |
| --- | --- |
| App 本身 | `/Applications/Cahoots.app` |
| 本地缓存(消息、打开过的文件、设置) | `~/Library/Application Support/co.joye.teamchat/` |
| 日志 | `~/Library/Application Support/co.joye.teamchat/logs/` |
| 登录凭证 | 钥匙串里名为 `co.joye.teamchat` 的项目(打开"钥匙串访问"搜索即可) |

完整的历史记录都在服务器上。删掉本地缓存后,重新登录 App 会再下载回来。

## 发送诊断信息

遇到问题时,打开 **设置 → 诊断**,发送诊断信息。里面有你写的说明、App 版本、系统类型和最近的日志,
其中的登录凭证和邀请码都已经去掉,**不包含你的消息内容**。发送后,在 App 里告诉管理员
发生了什么、大概是几点。

如果 App 打不开,把日志文件夹打包,用别的方式发给管理员:

```sh
cd ~/Library/Application\ Support/co.joye.teamchat && zip -r ~/Desktop/teamchat-logs.zip logs
```

## 卸载

0. 这台 Mac 用过「用这台电脑」（AI 成员在这里干活）的话，先在 **设置 › 电脑** 里这台电脑的 ⋯ 菜单选
   **不再用这台电脑…**：它从团队里移除，后台服务（launchd）停掉并删掉，
   `~/.config/team-chat/` 里登记这台电脑的文件（`daemon.toml`、`machine-token`、`machine.key`、
   `secrets.json`）一起删掉。不做这一步就把 App 拖进废纸篓，后台服务还会指向已经不在的程序。
1. 退出 Cahoots(菜单栏图标里也要退出),把 `/Applications/Cahoots.app` 移到废纸篓。
2. 可选:删除 `~/Library/Application Support/co.joye.teamchat/`(日志也在里面)。
3. 可选:删除登录凭证:在"钥匙串访问"里删除 `co.joye.teamchat` 项目,或在终端运行
   `security delete-generic-password -s co.joye.teamchat`。

## 在 Windows 上安装

需要 64 位的 Windows 10(22H2)或 Windows 11。安装不需要管理员权限,只装给当前登录的用户。

安装包是 x64 的。ARM 芯片的 Windows 11 电脑(比如骁龙本)靠系统自带的 x64 模拟也能装、能用,但我们没有在
这样的电脑上试过;ARM 的 Windows 10 不支持 x64 模拟,装不了。

### 下载和安装

1. 拿到 `Cahoots-<版本号>-setup.exe`:在这台 Windows 电脑的浏览器里打开邀请链接(在 Windows 上
   下载的是安装包,在 Mac 上是 DMG),或者到 <https://github.com/Joye-Co/team-chat/releases>
   的 Assets 里下载,或在终端里运行:

   ```powershell
   gh release download --repo Joye-Co/team-chat --pattern 'Cahoots-*-setup.exe'
   ```

   邀请码留着,登录时要用。
2. 可选:在 PowerShell 里检查文件是否完整(和该版本 Assets 里安装包旁边显示的 `sha256:` 对比):

   ```powershell
   Get-FileHash .\Cahoots-<版本号>-setup.exe -Algorithm SHA256
   ```

3. 双击安装包。在安装包用 Windows 认可的证书签名之前,会出现
   **Windows 已保护你的电脑**(SmartScreen):点 **更多信息**,再点 **仍要运行**。
   如果浏览器先拦住了下载,选 **保留**。
4. 装好后开始菜单里会有 Cahoots,安装程序也会问你要不要立刻打开。登录方法和上面第 4 步一样。
   在 Windows 上,登录凭证存在"Windows 凭据管理器"里。

如果电脑上没有 Microsoft Edge WebView2 Runtime(Windows 11 自带,部分 Windows 10 没有),安装程序会
联网下载并安装它。没有它 App 也能用,但产出页面和图表显示不出来。

脚本里静默安装:`Cahoots-<版本号>-setup.exe /S`。

### 更新

和 Mac 一样:App 检查服务器,下载新的安装包,校验 SHA-256,然后提示"重启安装"。点了以后 App 退出,
一个很小的更新程序(`team-chat-update.exe`)替换文件并打开新版本。如果新版本打不开,更新程序会把
旧版本放回去并重新打开。想手动更新:先退出 App,再运行新的安装包,覆盖安装;手动下载的安装包在签名之前
每次都会再出现 SmartScreen 提示。

### 数据存在哪里

| 内容 | 位置 |
| --- | --- |
| App 本身 | `%LOCALAPPDATA%\Programs\Cahoots\`(最早以 TeamChat 这个名字安装的,仍然在 `Programs\TeamChat\`,里面除了 `Cahoots.exe` 还会留一个 `TeamChat.exe`) |
| 本地缓存(消息、打开过的文件、设置)和日志 | `%APPDATA%\co.joye.teamchat\`(日志在 `logs\` 里) |
| 登录凭证 | "Windows 凭据管理器 → Windows 凭据"里以 `co.joye.teamchat:` 开头的项目(后面是服务器地址) |
| 可以重新生成的缓存(产出页面、图表) | `%LOCALAPPDATA%\team-chat\` |
| 开始菜单快捷方式 | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Cahoots.lnk` |

把带 `%...%` 的路径粘到资源管理器的地址栏里就能打开。

### 发送诊断信息

和 Mac 一样:打开 **设置 → 诊断** 发送。如果 App 打不开,在 PowerShell 里把日志文件夹打包,用别的方式
发给管理员:

```powershell
Compress-Archive -Path "$env:APPDATA\co.joye.teamchat\logs" -DestinationPath "$env:USERPROFILE\Desktop\teamchat-logs.zip"
```

更新没装上时,同一个文件夹旁边的 `updates\update.log` 写着更新程序每一步做了什么,一起发过来。

### 卸载

0. 这台电脑用过「用这台电脑」的话，先在 **设置 › 电脑** 里这台电脑的 ⋯ 菜单选 **不再用这台电脑…**：
   它从团队里移除，后台任务删掉，`%USERPROFILE%\.config\team-chat\` 里登记这台电脑的文件一起删掉。
1. 退出 Cahoots(通知区域的图标里也要退出)。
2. 打开 **设置 → 应用 → 安装的应用**,找到 Cahoots,选 **卸载**。或者运行
   `"%LOCALAPPDATA%\Programs\Cahoots\uninstall.exe"`(加 `/S` 不显示窗口)。
3. 卸载会删掉 App、开始菜单快捷方式、它的注册表项(卸载项、`teamchat://` 链接、通知用的应用 ID、
   "登录时打开")、缓存(`%LOCALAPPDATA%\team-chat\`),以及"用这台电脑"注册的后台任务。
   **保留你的本地数据和登录凭证**,和 Mac 上把 App 拖进废纸篓一样,重新安装后不用再登录。
4. 可选,想删干净:删除 `%APPDATA%\co.joye.teamchat\`;在 **凭据管理器 → Windows 凭据** 里删除以
   `co.joye.teamchat:` 开头的项目,或在 PowerShell 里运行:

   ```powershell
   cmdkey /list | Select-String 'co\.joye\.teamchat:\S+' | ForEach-Object { cmdkey /delete:$($_.Matches[0].Value) }
   ```

   这台电脑当过团队的机器的话,还有 `%USERPROFILE%\.config\team-chat\`(机器的身份)和
   `%LOCALAPPDATA%\team-chat-daemon\`(日志)。

## 常见问题

- **提示"已损坏,无法打开"**:运行第 3 步里的 `xattr` 命令,再打开。
- **"隐私与安全性"里找不到"仍要打开"**:先从"应用程序"里打开一次 App,看到提示后再去设置里找;
  这个按钮只在最近一次尝试打开后的一小时内出现。
- **一直显示"连接中"**:检查网络;如果网络正常还是这样,发送诊断信息并告诉管理员。
- **Windows 上提示"Windows 已保护你的电脑"**:安装包还没有签名时会这样。点 **更多信息**,再点
  **仍要运行**。
