# Install TeamChat on a Mac

Chinese version for interns: [INSTALL.zh.md](INSTALL.zh.md).

TeamChat needs a Mac with Apple silicon (M1 or later) and macOS 13 or later. The app is not
notarized by Apple, so macOS asks you to approve it once (step 3).

## 1. Download

Use one of these:

- **Invite link**: open the link your team's owner sent you. It downloads the newest
  `TeamChat-<version>.dmg`. Keep the invite code that came with it: you need it in step 4.
- **GitHub Releases** (team members with repository access): open
  <https://github.com/Joye-Co/team-chat/releases>, pick the newest release, and download
  `TeamChat-<version>.dmg` from Assets. Or in Terminal:

  ```sh
  gh release download --repo Joye-Co/team-chat --pattern 'TeamChat-*.dmg*'
  ```

Optional check that the file is complete (the `.sha256` file is next to the DMG in the
release):

```sh
shasum -a 256 -c TeamChat-<version>.dmg.sha256   # prints "OK"
```

## 2. Install

1. Double-click the DMG.
2. Drag **TeamChat** onto the **Applications** folder in the window that opens.
3. Eject the disk image (the eject button next to it in Finder).

## 3. Open the app the first time

Open TeamChat from Applications or Launchpad. macOS says it cannot verify the app. Click
**Done** (do not move it to the Bin), then:

1. Open **System Settings → Privacy & Security**.
2. Scroll down to the message about TeamChat and click **Open Anyway**.
3. Enter your Mac password, and click **Open Anyway** again.

You do this once per installed copy. Instead of the steps above, you can also run this in
Terminal and then open the app normally:

```sh
xattr -dr com.apple.quarantine /Applications/TeamChat.app
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
| The app | `/Applications/TeamChat.app` |
| Local cache (messages, files you opened, settings) | `~/Library/Application Support/co.joye.teamchat/` |
| Logs | `~/Library/Logs/co.joye.teamchat/` |
| Sign-in token | Keychain item with the name `co.joye.teamchat` (open **Keychain Access** and search for it) |

The server keeps the full history. If you delete the local cache, the app downloads it again
after you sign in.

## Send diagnostics

When something goes wrong, open **Settings → 诊断** and send the diagnostics. They contain the
app version, macOS version, connection state and recent logs with tokens and invite codes
removed; they do not contain your messages. Then tell your team's owner in the app what happened and at
what time.

If the app does not start, zip the log folder and send it to your team's owner another way:

```sh
cd ~/Library/Logs && zip -r ~/Desktop/teamchat-logs.zip co.joye.teamchat
```

## Uninstall

1. Quit TeamChat (also from the menu bar item) and move `/Applications/TeamChat.app` to the
   Bin.
2. Optional: delete `~/Library/Application Support/co.joye.teamchat/` and
   `~/Library/Logs/co.joye.teamchat/`.
3. Optional: remove the sign-in token: in Keychain Access, delete the `co.joye.teamchat` item,
   or run `security delete-generic-password -s co.joye.teamchat`.

## For maintainers

- `scripts/build-dmg.sh` builds `dist/<name>-<version>.dmg` and its `.sha256`;
  `scripts/test-dmg.sh` checks the DMG without opening the app.
- `scripts/build-daemon.sh` builds the machine daemon into `dist/`.
- `scripts/release.sh <version> [--env staging|production] [--notes <file>] [--dry-run]`
  sets the version, builds and checks everything, tags `v<version>` on main, creates the GitHub
  Release, and uploads the DMG to the server. The admin secret comes from
  `~/.config/team-chat/<env>/admin-secret` (mode 600). Run it with `--dry-run` first. If the
  upload fails or the server has no release endpoint yet, retry later with
  `scripts/release.sh <version> --env <env> --upload-only`.
- The **Release DMG** workflow (Actions tab, manual) rebuilds the DMG from a tag on a macOS
  runner and attaches it to that tag's release.
