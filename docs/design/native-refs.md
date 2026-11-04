# Native references

Web images of macOS 26 (Tahoe) apps that [NATIVE.md](NATIVE.md) follows. Open them in a browser;
do not save them in the repository and never capture other apps' windows on this computer.

## Apple HIG (base `https://developer.apple.com/tutorials/images/com.apple.HIG/`)

| Image | Shows |
| --- | --- |
| `toolbars-mac-window-anatomy@2x.png`, `~dark@2x.png` | Window anatomy: floating sidebar, glass toolbar groups |
| `toolbars-notes-app-expanded-icons@2x.png`, `~dark@2x.png` | Notes toolbar groups, title and subtitle, a menu with symbols |
| `sheets-macos-notes@2x.png`, `~dark@2x.png` | A sheet over a dimmed window, capsule default button |
| `search-fields-toolbar-search-mac@2x.png`, `~dark@2x.png` | Toolbar search field and suggestions |
| `window-states@2x.png` | Key, main and inactive windows |
| `macos-calendar-tab-view-segmented-control-comparison@2x.png` | Segmented control versus tab view |
| `toolbars-prominent-action-tinted@2x.png` | The prominent toolbar action |
| `attached-popover~dark@2x.png` | Popover |
| `checkbox-selected@2x.png` | Tahoe checkbox |

## Apple guides and newsroom

| Image | Shows |
| --- | --- |
| https://help.apple.com/assets/69DD569682238CF8EC0621E2/69DD569982238CF8EC0621E9/en_US/39a248d4c8a062813cf5aba9c966bbaa.png | Tahoe Finder: floating sidebar, gray selection with accent text, toolbar groups |
| https://ipcdn-web.apple.com/assets/v2/web/ca4c7412-b7a7-4282-894b-dc6a91eef72c | Mail: title and subtitle in the toolbar, list rows, selection |
| https://ipcdn-web.apple.com/assets/v2/web/86b319bd-e7ff-46ec-b223-adb464f73546 | Mail: toolbar groups and filter |
| https://ipcdn-web.apple.com/assets/v2/web/5a24a9a5-0306-4a81-b484-77263c529c2d | Mail Settings: toolbar tabs, `+`/`−` buttons, pop-up button, help button |
| https://ipcdn-web.apple.com/assets/v2/web/ac6fdcef-3869-4845-9f6c-0fd057348046 | Messages: floating sidebar, rows, composer |
| https://ipcdn-web.apple.com/assets/v2/web/f11e2b73-6e00-4651-94a2-b628ba6f7459 | Messages: the Name and Photo sheet (how a profile is edited) |
| https://www.apple.com/newsroom/images/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/article/Apple-WWDC25-macOS-Tahoe-26-Messages-250609_big.jpg.large_2x.jpg | Messages in dark mode |

## AI apps (for the composer and the inspector only)

| Image | Shows |
| --- | --- |
| https://static.simonwillison.net/static/2026/codex-app.jpg | Codex: title in the toolbar, composer with pickers and the `Local ▾` run-location picker |
| https://static.simonwillison.net/static/2026/claude-cowork.jpg | Claude: inspector sections with disclosure |
| https://static.beebom.com/wp-content/uploads/2024/06/Customize-ChatGPT-Launcher-Keyboard-Shortcut.jpg?w=1024 | ChatGPT (native): grouped settings rows with symbols |

## Metrics sources

- WWDC25 session 310 "Build an AppKit app with the new design": controls slightly taller, mini to
  regular are rounded rectangles, large and extra large are capsules, concentric corners,
  inspectors edge to edge, toolbar items share glass groups.
- Window corner 26 pt with a toolbar; sidebar inset 8 pt and radius 18–19 pt; toolbar row 52 pt
  with 36 pt groups (measured from the Finder image above at 1.2 px/pt).
- Regular controls 24 pt tall with 6 pt radii, groups 10 pt, large surfaces 16 pt (gpui-macos
  design notes, read from AppKit intrinsic sizes on macOS 26.2).
- macOS text styles: HIG Typography (Body 13 pt, Subheadline 11 pt, Title 3 15 pt, Title 2 17 pt).
