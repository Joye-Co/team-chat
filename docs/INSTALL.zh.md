# 在 Mac 上安装 TeamChat

English version: [INSTALL.md](INSTALL.md)。

需要一台 Apple 芯片(M1 或更新)的 Mac,系统 macOS 13 或更新。这个 App 没有经过 Apple 公证,
所以第一次打开时 macOS 会要你手动允许一次(第 3 步)。

## 1. 下载

两种方式任选一种:

- **邀请链接**:打开 Joye 发给你的链接,会下载最新的 `TeamChat-<版本号>.dmg`。
  链接附带的**邀请码**请保存好,第 4 步要用。
- **GitHub Releases**(有仓库权限的同学):打开
  <https://github.com/Joye-Co/team-chat/releases>,选最新的版本,在 Assets 里下载
  `TeamChat-<版本号>.dmg`。也可以在终端里运行:

  ```sh
  gh release download --repo Joye-Co/team-chat --pattern 'TeamChat-*.dmg*'
  ```

可选:检查文件是否完整(同一个版本里有对应的 `.sha256` 文件):

```sh
shasum -a 256 -c TeamChat-<版本号>.dmg.sha256   # 显示 "OK" 就是完整的
```

## 2. 安装

1. 双击下载好的 DMG。
2. 在弹出的窗口里,把 **TeamChat** 拖到 **Applications(应用程序)** 文件夹上。
3. 推出磁盘映像(在访达侧边栏里点它旁边的推出按钮)。

## 3. 第一次打开

从"应用程序"或启动台打开 TeamChat。macOS 会提示"无法验证"这个 App。点 **完成**
(**不要**点"移到废纸篓"),然后:

1. 打开 **系统设置 → 隐私与安全性**。
2. 往下滑,找到关于 TeamChat 的提示,点 **仍要打开**。
3. 输入 Mac 的登录密码,再点一次 **仍要打开**。

每次手动安装新的一份只需要做一次。也可以不走上面的步骤,在终端里运行下面这行,然后正常打开:

```sh
xattr -dr com.apple.quarantine /Applications/TeamChat.app
```

## 4. 登录

1. 在登录界面粘贴邀请码。
2. 填写你的名字。名字、岗位和头像以后都可以在"设置"里改。

邀请码只能用一次。App 会用它换成一个登录凭证,存在你 Mac 的"钥匙串"里,所以重启后仍然是登录状态。
**不要把邀请码发给别人,也不要发到群里。** 如果邀请码用不了,找 Joye 要一个新的。

## 5. 更新

App 会自己检查服务器上有没有新版本,有的话会提示你安装,装完自动重启。

想手动更新:按第 1 步下载新的 DMG,再把 App 拖到"应用程序"里,选 **替换**。
手动下载的新版本需要再做一次第 3 步。

## 数据存在哪里

| 内容 | 位置 |
| --- | --- |
| App 本身 | `/Applications/TeamChat.app` |
| 本地缓存(消息、打开过的文件、设置) | `~/Library/Application Support/co.joye.teamchat/` |
| 日志 | `~/Library/Logs/co.joye.teamchat/` |
| 登录凭证 | 钥匙串里名为 `co.joye.teamchat` 的项目(打开"钥匙串访问"搜索即可) |

完整的历史记录都在服务器上。删掉本地缓存后,重新登录 App 会再下载回来。

## 发送诊断信息

遇到问题时,打开 **设置 → 诊断**,发送诊断信息。里面有 App 版本、macOS 版本、连接状态和最近的日志,
其中的登录凭证和邀请码都已经去掉,**不包含你的消息内容**。发送后,在 App 里告诉 Joye
发生了什么、大概是几点。

如果 App 打不开,把日志文件夹打包,用别的方式发给 Joye:

```sh
cd ~/Library/Logs && zip -r ~/Desktop/teamchat-logs.zip co.joye.teamchat
```

## 卸载

1. 退出 TeamChat(菜单栏图标里也要退出),把 `/Applications/TeamChat.app` 移到废纸篓。
2. 可选:删除 `~/Library/Application Support/co.joye.teamchat/` 和
   `~/Library/Logs/co.joye.teamchat/`。
3. 可选:删除登录凭证:在"钥匙串访问"里删除 `co.joye.teamchat` 项目,或在终端运行
   `security delete-generic-password -s co.joye.teamchat`。

## 常见问题

- **提示"已损坏,无法打开"**:运行第 3 步里的 `xattr` 命令,再打开。
- **"隐私与安全性"里找不到"仍要打开"**:先从"应用程序"里打开一次 App,看到提示后再去设置里找;
  这个按钮只在最近一次尝试打开后的一小时内出现。
- **一直显示"连接中"**:检查网络;如果网络正常还是这样,发送诊断信息并告诉 Joye。
