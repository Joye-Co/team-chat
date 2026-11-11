# Install Cahoots on a Mac

Chinese version for interns: [INSTALL.zh.md](INSTALL.zh.md). For a Windows computer, see
[Windows](#windows) below.

The app signs in to your team's server. If you have none yet (you use the app alone, or you set
up the team), deploy your own first: [SELF-HOSTING.md](SELF-HOSTING.md). It ends with this app
signed in with a sign-in code that you enter in step 4, after choosing 使用其他服务器… and your
server's address.

Cahoots needs a Mac with Apple silicon (M1 or later) and macOS 13 or later. The app is not
notarized by Apple, so macOS asks you to approve it once (step 3).

## 1. Download

Use one of these:

- **Invite link**: open the link your team's owner sent you. It downloads the newest
  `Cahoots-<version>.dmg`. Keep the invite code that came with it: you need it in step 4.
- **GitHub Releases** (team members with repository access): open
  <https://github.com/Joye-Co/team-chat/releases>, pick the newest release, and download
  `Cahoots-<version>.dmg` from Assets. Or in Terminal:

  ```sh
  gh release download --repo Joye-Co/team-chat --pattern 'Cahoots-*.dmg'
  ```

Optional check that the file is complete: compare the output with the `sha256:` digest shown
next to the DMG under Assets of the release:

```sh
shasum -a 256 Cahoots-<version>.dmg
```

## 2. Install

1. Double-click the DMG.
2. Drag **Cahoots** onto the **Applications** folder in the window that opens.
3. Eject the disk image (the eject button next to it in Finder).

## 3. Open the app the first time

Open Cahoots from Applications or Launchpad. macOS says it cannot verify the app. Click
**Done** (do not move it to the Bin), then:

1. Open **System Settings → Privacy & Security**.
2. Scroll down to the message about Cahoots and click **Open Anyway**.
3. Enter your Mac password, and click **Open Anyway** again.

You do this once per installed copy. Instead of the steps above, you can also run this in
Terminal and then open the app normally:

```sh
xattr -dr com.apple.quarantine /Applications/Cahoots.app
```

## 4. Sign in

1. Paste the invite code from your invite into the sign-in screen.
2. Enter your name. You can change your name, 岗位 and avatar later in Settings.

The invite code works once. The app swaps it for a sign-in token and keeps that token in your
macOS Keychain, so you stay signed in after restarts. Do not share your invite code or post it
in a chat. If it does not work, ask your team's owner for a new one.

## 5. Updates

The app checks the server for a new version and offers to install it; it restarts by itself.
To update by hand, download the new DMG (step 1) and drag the app onto Applications again,
choosing **Replace**. A copy that you download by hand needs step 3 again.

## Where your data is

| What | Where |
| --- | --- |
| The app | `/Applications/Cahoots.app` |
| Local cache (messages, files you opened, settings) | `~/Library/Application Support/co.joye.teamchat/` |
| Logs | `~/Library/Application Support/co.joye.teamchat/logs/` |
| Sign-in token | Keychain item with the name `co.joye.teamchat` (open **Keychain Access** and search for it) |

The server keeps the full history. If you delete the local cache, the app downloads it again
after you sign in.

## Send diagnostics

When something goes wrong, open **Settings → 诊断** and send the diagnostics. They contain the
text you write, the app version, the kind of system and recent logs with tokens and invite codes
removed; they do not contain your messages. Then tell your team's owner in the app what happened and at
what time.

If the app does not start, zip the log folder and send it to your team's owner another way:

```sh
cd ~/Library/Application\ Support/co.joye.teamchat && zip -r ~/Desktop/teamchat-logs.zip logs
```

## Uninstall

1. Quit Cahoots (also from the menu bar item) and move `/Applications/Cahoots.app` to the
   Bin.
2. Optional: delete `~/Library/Application Support/co.joye.teamchat/` (the logs are in it).
3. Optional: remove the sign-in token: in Keychain Access, delete the `co.joye.teamchat` item,
   or run `security delete-generic-password -s co.joye.teamchat`.

## Windows

Cahoots for Windows needs 64-bit Windows 10 (version 22H2) or Windows 11. The installer needs
no administrator rights: it installs for your user account only.

The installer is x64. A Windows 11 computer with an ARM processor installs and runs it through
the system's x64 emulation; we have not tried it on such a computer. Windows 10 on ARM has no
x64 emulation and cannot run it.

### Download and install

1. Get `Cahoots-<version>-setup.exe`: open your invite link in a browser on the Windows
   computer (it downloads the installer there, and the DMG on a Mac), or download it from
   Assets on <https://github.com/Joye-Co/team-chat/releases>, or run:

   ```powershell
   gh release download --repo Joye-Co/team-chat --pattern 'Cahoots-*-setup.exe'
   ```

   Keep the invite code for the sign-in.
2. Optional check that the file is complete, in PowerShell (compare with the `sha256:` digest
   shown next to the installer under Assets of the release):

   ```powershell
   Get-FileHash .\Cahoots-<version>-setup.exe -Algorithm SHA256
   ```

3. Double-click the installer. Until the installer is signed with a certificate that Windows
   knows, **Windows protected your PC** (SmartScreen) appears: click **More info**, then
   **Run anyway**. If the browser holds the download back first, choose **Keep**.
4. The installer puts the app in your Start menu and offers to open it. Sign in as in step 4
   above. On Windows the sign-in token is kept in the Windows Credential Manager.

If the computer has no Microsoft Edge WebView2 Runtime (Windows 11 has it; some Windows 10
computers do not), the installer downloads and installs it. That step needs a network
connection. Without the runtime the app works, but 产出 pages and diagrams do not show.

Silent install, for scripts: `Cahoots-<version>-setup.exe /S`.

### Updates

As on the Mac, the app checks the server, downloads the new installer, checks its SHA-256
and offers 重启安装. The app then quits, a small update program
(`team-chat-update.exe`) replaces the files and opens the new version. If the new version
does not start, the update program puts the old version back and opens it. To update by
hand, quit the app and run a newer installer over the installed app; until the installer is
signed, each installer you download by hand shows the SmartScreen warning again.

### Where your data is

| What | Where |
| --- | --- |
| The app | `%LOCALAPPDATA%\Programs\Cahoots\` (an app first installed as TeamChat stays in `Programs\TeamChat\` and keeps a `TeamChat.exe` next to `Cahoots.exe`) |
| Local cache (messages, files you opened, settings) and logs | `%APPDATA%\co.joye.teamchat\` (logs in `logs\`) |
| Sign-in token | Windows Credential Manager → Windows Credentials, the entries that start with `co.joye.teamchat:` (the server address follows) |
| Caches the app can make again (产出 pages, diagrams) | `%LOCALAPPDATA%\team-chat\` |
| Start menu shortcut | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Cahoots.lnk` |

Paste a path with `%...%` into the address bar of File Explorer to open it.

### Send diagnostics

As on the Mac: **Settings → 诊断**. If the app does not start, zip the log folder in PowerShell
and send it to your team's owner another way:

```powershell
Compress-Archive -Path "$env:APPDATA\co.joye.teamchat\logs" -DestinationPath "$env:USERPROFILE\Desktop\teamchat-logs.zip"
```

When an update did not install, `updates\update.log` next to that folder says what the update
program did at each step; send it too.

### Uninstall

1. Quit Cahoots (also from the icon in the notification area).
2. Open **Settings → Apps → Installed apps**, find Cahoots and choose **Uninstall**. Or run
   `"%LOCALAPPDATA%\Programs\Cahoots\uninstall.exe"` (add `/S` for no window).
3. The uninstaller removes the app, its Start menu shortcut, its registry entries (the
   uninstall entry, `teamchat://` links, the app id of its notifications, 登录时打开), its
   caches (`%LOCALAPPDATA%\team-chat\`) and the background task that 用这台电脑 registered.
   It keeps your local data and your sign-in, as a Mac does when the app goes to the Bin, so
   a reinstall needs no new sign-in.
4. Optional, to remove everything: delete `%APPDATA%\co.joye.teamchat\`; in **Credential
   Manager → Windows Credentials** delete the entries that start with `co.joye.teamchat:`, or
   run in PowerShell:

   ```powershell
   cmdkey /list | Select-String 'co\.joye\.teamchat:\S+' | ForEach-Object { cmdkey /delete:$($_.Matches[0].Value) }
   ```

   If this computer was one of the team's machines, there are also
   `%USERPROFILE%\.config\team-chat\` (the machine's identity) and
   `%LOCALAPPDATA%\team-chat-daemon\` (logs).

## For maintainers

- `scripts/build-dmg.sh` builds `dist/<name>-<version>.dmg` and its `.sha256`;
  `scripts/test-dmg.sh` checks the DMG without opening the app.
- `scripts/build-daemon.sh` builds the machine daemon into `dist/`.
- `scripts/release.sh <version> [--env staging|production] [--notes <file>] --windows-installer <setup.exe> [--dry-run]`
  sets the version, builds and checks everything, tags `v<version>` on main, creates the GitHub
  Release with the DMG and the Windows installer together, and uploads both to the server. A
  release is for both systems: without the installer it stops before it changes anything
  (`--without-windows` publishes the Mac alone on purpose; `--add-windows <setup.exe>` adds the
  installer to that release later). The admin secret comes from
  `~/.config/team-chat/<env>/admin-secret` (mode 600). Run it with `--dry-run` first. If an
  upload fails or the server has no release endpoint yet, retry later with
  `scripts/release.sh <version> --env <env> --upload-only [--platform windows]`.
- Windows: `scripts/build-windows-installer.sh` (Git Bash on Windows, NSIS) builds
  `dist/<name>-<version>-setup.exe` and its `.sha256`; `scripts/test-windows-installer.sh`
  installs it silently, checks files, registry and shortcut, starts the app and uninstalls;
  `scripts/test-windows-update.sh <a-setup.exe> <b-setup.exe>` installs A, serves B from a
  local server and lets the app update itself, then tests the rollback. CI runs all three on
  a Windows runner (job `windows-installer`) and keeps the unsigned installer as the
  artifact `windows-installer`. Code signing is optional and never in the repository: see
  `scripts/windows/sign.sh` and [RUNBOOK.md](RUNBOOK.md#release-from-github-actions).
  `scripts/release.sh <version> --upload-only --platform windows` uploads an installer in
  `dist/` to the server as the Windows file of the release.
- The **Release** workflow (Actions tab, manual) runs `scripts/release.sh` on a macOS runner:
  a dry run on any branch, the whole release or an upload retry on main. Its Windows jobs build,
  test and publish the installer next to the DMG. Its secrets and modes
  are in [RUNBOOK.md](RUNBOOK.md#release-from-github-actions).
