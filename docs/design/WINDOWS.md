# Windows client rules

The Windows app is the Mac app: the same screens, the same black-and-white finish, the same
tokens and scale. [COMPLETENESS.md](COMPLETENESS.md) is the binding finish spec on both systems
and wins over this file and over [NATIVE.md](NATIVE.md) where they differ; a screen is done on
Windows when it passes the same §4 checklist there, in light and dark.

This file lists only what differs from NATIVE.md on Windows. Anything it does not name follows
NATIVE.md as written (structure, toolbar, sidebar, controls, overlays, behavior), with every
size read as device-independent pixels (1 pt = 1 px at 100 % scale). Minimum system: Windows 10
22H2.

Status: nobody has run the app on a Windows machine yet. Each section says what was looked at
(on a Mac, or in the Windows CI screenshots) and what is only compiled. Fix this file when a
real machine shows otherwise.

## 1. Title bar and window buttons

- The app draws its own title bar; Windows draws none. It is a 32 px caption strip above the
  same 64 px toolbar row as on the Mac (NATIVE.md §2), so the page title, subtitle and toolbar
  groups do not move (`ui::native::window_chrome`). The strip holds what the Mac keeps outside
  the window: the app's menus in the main window (§2), the window's title in every other
  window, and the window buttons.
- There are no traffic lights. The window buttons are Windows' own three at the trailing edge of
  the strip, in Windows' order: minimize, maximize or restore, close. Each is 46 px wide and as
  tall as the strip, flush with the top and the trailing edge, glyph 10 px in `label`; hover is
  `fill-hover`, and the close button's hover is Windows' red (`#C42B1C`) with a white glyph: the
  one place a Windows system color enters the palette. An inactive window draws the glyphs in
  `tertiary`. A window that cannot be resized (登录, 设置) dims the maximize button.
- The sidebar keeps the windowed Mac's layout (NATIVE.md §2, Sidebar): the top row with the
  filter and 新话题 at its trailing end, the search field with the whole width under it. Only
  the lights are missing from that row, so the sidebar toggle sits at its leading edge (x 16)
  instead of after them. Full screen (F11) uses the Mac's full-screen layout.
- The strip is the drag area except over controls, as a title bar is in every Windows app; the
  toolbar row under it does not move the window. Double-click maximizes;
  the system menu opens on Alt+Space and on a right-click of the strip. Hovering the maximize
  button shows Windows 11's Snap layouts (the button answers `HTMAXBUTTON`). Windows draws the
  window's shadow and, on Windows 11, its rounded corners; a maximized window has neither and no
  border.
- Full screen is F11 and hides the strip, like the Mac hides the lights.
- The 产出 window is a system window on both systems (a standard `NSWindow` on the Mac), so on
  Windows it keeps the system's title bar, colored from the theme (`sidebar` fill, `label` text,
  `separator` border; dark when the app is dark) and with the app's icon.

Looked at in the Windows CI screenshots (100 % scale).

## 2. Menus and the tray

- Windows has no menu bar at the top of the screen. The same menus (NATIVE.md §10) sit in the
  window, in the title bar after the sidebar toggle, as gpui-component's `AppMenuBar`
  (`views::nav::menus::window_menu_bar`). Alt moves focus to them. They leave out what only the
  Mac has: 服务, 隐藏, 前置全部窗口, 表情与符号.
- The App menu's items move to where Windows keeps them: 设置… and 退出 under the first menu,
  关于 under 帮助. 关于 opens the app's own 关于 window, not a system panel.
- The menu bar status item becomes an icon in the notification area (the tray), with the same
  menu plus 退出. The setting that shows it is the same switch; its label names the tray, not the
  menu bar. While the icon is shown, closing the last window keeps the app running; without it,
  closing the last window quits.

Only compiled.

## 3. Shortcuts

- Ctrl stands where ⌘ stands, Alt where ⌥ stands, and the Windows key is never used. Bindings
  are written once with GPUI's `secondary-` modifier.
- A shortcut is written as words joined by `+`, modifiers first in Windows' order:
  `Ctrl+Alt+Shift+K`. No ⌘ ⌥ ⇧ ⌃ glyphs anywhere on Windows: menus, tooltips, the 快捷键 pane and
  copy that names a key all use this form (`settings::shortcuts::shortcut_text`). Return is
  `Enter`, Delete is `Backspace`, Escape is `Esc`.
- Differences in the bindings themselves: 设置 is `Ctrl+,`; close window `Ctrl+W`; full screen
  `F11`; 显示侧栏 `Ctrl+Alt+S`. Hide, Hide Others and Minimize have no binding (Windows has its own:
  `Win+Down`).

Only compiled.

## 4. Fonts

One type scale on both systems: COMPLETENESS.md §1.4, same sizes, line heights and weights.
Only the families differ (`theme.rs`, `UI_FONT_STACK`, `MONO_FONT_STACK`, `UI_FONT_FALLBACKS`):

| Role | macOS | Windows, first installed wins |
| --- | --- | --- |
| Interface | the system font (SF Pro) | Segoe UI Variable (its Text optical size), then Segoe UI |
| Chinese | PingFang SC (system fallback) | Microsoft YaHei UI, then the system's fallback |
| Monospace | Menlo | Cascadia Mono, then Consolas |
| Emoji | Apple Color Emoji | Segoe UI Emoji (system fallback) |

- Windows 11 has Segoe UI Variable; Windows 10 has only Segoe UI. Cascadia Mono comes with
  Windows 11 and with Windows Terminal; Consolas is on every version. The app lists the installed
  families once at launch and takes the first of each stack (`theme::ui_font_family`,
  `theme::mono_font_family`); the last of each stack needs no check.
- Chinese is named, not left to the system: Windows picks a Han font by the system language, so
  a Japanese or Traditional Chinese system would draw Simplified Chinese text in the wrong
  glyphs. Every window root names Microsoft YaHei UI ahead of the system fallback
  (`gpui_component::UiFontFallbacks`).
- Weight 500 (Medium): Segoe UI Variable has it. Segoe UI (Windows 10) and Microsoft YaHei UI
  have no Medium face, so 500 draws Regular there: Chinese text and Windows 10 lose the step
  between 400 and 500, and keep the one to 600. The theme asks for 499 on Windows
  (`MEDIUM_WEIGHT`): at 500 and above DirectWrite takes the heavier face, which drew section
  headers and labels in Bold in the Windows screenshots. Use `MEDIUM_WEIGHT`, never
  `FontWeight::MEDIUM`, for the same reason.
- Tabular figures (`tnum`) work in both Segoe families.
- Pages drawn by a web view (产出, diagrams, previews) must name the same families in their CSS:
  `"Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI"` after `-apple-system`, and
  `"Cascadia Mono", Consolas` for code.

Only compiled; the stack's pick is unit-tested (`a_font_stack_picks_its_first_installed_family`).

## 5. Icons

SF Symbols may only be drawn by Apple's systems and cannot be shipped, so Windows draws every
concept of NATIVE.md §6 from one SVG set: **Lucide** (ISC; `third_party/lucide/`), plus a few
icons drawn for this app on Lucide's grid (`assets/icons/drawn/`). macOS keeps SF Symbols.

- Views do not change: they name a concept (`Sym::Home.sidebar()`), and the asset source serves
  the symbol on macOS and the SVG icon elsewhere (`ui::sf_symbols::svgset`). gpui-component's own
  glyphs (chevrons, checkbox check, dialog close, spinner) go the same way.
- **Size**: the icon is drawn as large as fits in the ink box its SF Symbol has at that size (an
  icon squarer than its symbol may cover the same area instead), and centered in the same slot, so rows, buttons and tabs keep their rhythm. The §6 sizes stay:
  sidebar 15 in 18, toolbar 16 in 20, inline 13 in 16, small 11 in 14, tabs 18 in 24, pop-up
  mark 9.
- **Weight**: one stroke for every icon of a context, as thick as the symbol's at that size and
  weight, whatever the icon's own scale: 1.07 px at 13 Regular, 1.62 px at 16 Medium (the
  toolbar), 1.0 px at 9 Semibold (the pop-up mark). Round caps and joins.
- **Color**: the same tokens (`secondary` in sidebars and rows, `label` in the toolbar, a state
  color only when the icon is the state).
- **Filled states** (needs you, done, failed, problem, the live dot, the filled star) are filled
  shapes with the mark cut out, like the symbols. Lucide has no filled icons, so these are drawn
  here from Lucide's outlines.
- Where the Mac's symbol names Apple hardware or an Apple key, Windows draws the neutral thing:
  快捷键 is a keyboard (no ⌘ on a Windows keyboard), a Mac mini is a small desktop box, 在新窗口打开
  is a plain app window.
- A new concept gets its row in NATIVE.md §6, then its line in `sym.rs` and its line in
  `svgset.rs` (the app does not compile without it), then its row in the table below. Take the
  Lucide icon closest to the symbol's shape; draw one only when Lucide has none, as a 24-unit
  SVG with stroke 2, round caps and joins, into `assets/icons/drawn/`.
- To look at these icons on a Mac: `TC_SVG_ICONS=1` on any run of the app or of
  `desktop --screenshot` draws the SVG set instead of SF Symbols. `TC_SVG_SHEET=<dir> cargo test
  -p desktop contact_sheet` writes every concept as symbol and icon side by side.
- Not done: strokes are not snapped to whole pixels. At 100 % scale a 1.07 px line can fall
  across two pixel rows and look soft; check on a 100 % display and snap in `svgset::svg` if so.

Looked at on a Mac with `TC_SVG_ICONS=1`, light and dark, next to the SF Symbol screens. Not
seen on Windows.

Drawn for this app (7): `circle-alert-fill`, `circle-check-fill`, `circle-x-fill`,
`triangle-alert-fill`, `circle-fill`, `star-fill`, `desktop-mini`.

| Concept (`Sym::`) | SF Symbol (macOS) | Windows icon | Uses |
| --- | --- | --- | --- |
| Home | `tray` | Lucide `inbox` | 1 |
| Outputs | `square.stack` | Lucide `gallery-vertical-end` | 4 |
| Members | `person.2` | Lucide `users` | 14 |
| Topic | `number` | Lucide `hash` | 21 |
| Compose | `square.and.pencil` | Lucide `square-pen` | 5 |
| Search | `magnifyingglass` | Lucide `search` | 8 |
| Filter | `line.3.horizontal.decrease` | Lucide `list-filter` | 2 |
| AskAside | `bubble.left.and.text.bubble.right` | Lucide `messages-square` | 5 |
| OpenTopic | `text.bubble` | Lucide `message-square-text` | 1 |
| PullRequest | `arrow.triangle.pull` | Lucide `git-pull-request` | 7 |
| Branch | `arrow.triangle.branch` | Lucide `git-branch` | 1 |
| Merge | `arrow.triangle.merge` | Lucide `git-merge` | 3 |
| Inspector | `sidebar.trailing` | Lucide `panel-right` | 6 |
| SidebarToggle | `sidebar.leading` | Lucide `panel-left` | 2 |
| Attach | `plus` | Lucide `plus` | 4 |
| Plus | `plus` | Lucide `plus` | 7 |
| Minus | `minus` | Lucide `minus` | 5 |
| Send | `arrow.up` | Lucide `arrow-up` | 3 |
| NeedsYou | `exclamationmark.circle.fill` | drawn `circle-alert-fill` | 8 |
| Live | `circle.fill` | drawn `circle-fill` | 8 |
| Done | `checkmark.circle.fill` | drawn `circle-check-fill` | 26 |
| Problem | `exclamationmark.triangle.fill` | drawn `triangle-alert-fill` | 2 |
| Failed | `xmark.circle.fill` | drawn `circle-x-fill` | 30 |
| Alert | `exclamationmark.circle` | Lucide `circle-alert` | 10 |
| Warning | `exclamationmark.triangle` | Lucide `triangle-alert` | 6 |
| Waiting | `clock` | Lucide `clock` | 14 |
| Todo | `circle.dashed` | Lucide `circle-dashed` | 6 |
| Circle | `circle` | Lucide `circle` | 1 |
| Stopped | `nosign` | Lucide `ban` | 2 |
| Hourglass | `hourglass` | Lucide `hourglass` | 1 |
| Computer | `display` | Lucide `monitor` | 18 |
| Laptop | `laptopcomputer` | Lucide `laptop` | 8 |
| MacMini | `macmini` | drawn `desktop-mini` | 5 |
| Server | `server.rack` | Lucide `server` | 9 |
| Cloud | `cloud` | Lucide `cloud` | 3 |
| Offline | `wifi.slash` | Lucide `wifi-off` | 10 |
| Phone | `iphone` | Lucide `smartphone` | 2 |
| General | `gearshape` | Lucide `settings` | 2 |
| Account | `person.crop.circle` | Lucide `circle-user-round` | 4 |
| Bell | `bell` | Lucide `bell` | 2 |
| Muted | `bell.slash` | Lucide `bell-off` | 0 |
| BellBadge | `bell.badge` | Lucide `bell-dot` | 1 |
| Appearance | `paintpalette` | Lucide `palette` | 1 |
| Shortcuts | `command` | Lucide `keyboard` | 2 |
| Keyboard | `keyboard` | Lucide `keyboard` | 0 |
| Privacy | `hand.raised` | Lucide `hand` | 1 |
| Lock | `lock` | Lucide `lock` | 8 |
| Shield | `checkmark.shield` | Lucide `shield-check` | 5 |
| OpenWindow | `macwindow` | Lucide `app-window` | 0 |
| More | `ellipsis` | Lucide `ellipsis` | 4 |
| Check | `checkmark` | Lucide `check` | 27 |
| Close | `xmark` | Lucide `x` | 18 |
| ChevronDown | `chevron.down` | Lucide `chevron-down` | 14 |
| ChevronUp | `chevron.up` | Lucide `chevron-up` | 6 |
| ChevronLeft | `chevron.left` | Lucide `chevron-left` | 4 |
| ChevronRight | `chevron.right` | Lucide `chevron-right` | 15 |
| PageFirst | `chevron.left.2` | Lucide `chevrons-left` | 1 |
| PageLast | `chevron.right.2` | Lucide `chevrons-right` | 1 |
| PopUp | `chevron.up.chevron.down` | Lucide `chevrons-up-down` | 0 |
| OpenExternal | `arrow.up.forward` | Lucide `arrow-up-right` | 13 |
| Document | `doc.text` | Lucide `file-text` | 37 |
| File | `doc` | Lucide `file` | 10 |
| CodeFile | `doc.plaintext` | Lucide `file-code` | 6 |
| MissingFile | `doc.questionmark` | Lucide `file-question-mark` | 2 |
| Diff | `plus.forwardslash.minus` | Lucide `diff` | 2 |
| Image | `photo` | Lucide `image` | 18 |
| Images | `photo.on.rectangle` | Lucide `images` | 1 |
| Copy | `doc.on.doc` | Lucide `copy` | 12 |
| AiMember | `sparkles` | Lucide `sparkles` | 4 |
| Person | `person` | Lucide `user` | 5 |
| Invite | `person.badge.plus` | Lucide `user-plus` | 5 |
| Quote | `text.quote` | Lucide `text-quote` | 8 |
| List | `list.bullet` | Lucide `list` | 11 |
| NumberedList | `list.number` | Lucide `list-ordered` | 1 |
| Checklist | `checklist` | Lucide `list-checks` | 6 |
| Outline | `list.bullet.indent` | Lucide `list-tree` | 1 |
| Grid | `square.grid.2x2` | Lucide `layout-grid` | 1 |
| Table | `tablecells` | Lucide `table` | 4 |
| Download | `square.and.arrow.down` | Lucide `download` | 5 |
| Upload | `square.and.arrow.up` | Lucide `share` | 0 |
| Share | `square.and.arrow.up` | Lucide `share` | 0 |
| Diagram | `point.3.connected.trianglepath.dotted` | Lucide `workflow` | 7 |
| Video | `video` | Lucide `video` | 5 |
| Audio | `waveform` | Lucide `audio-lines` | 2 |
| Play | `play` | Lucide `play` | 4 |
| Pause | `pause` | Lucide `pause` | 5 |
| Volume | `speaker.wave.2` | Lucide `volume-2` | 0 |
| PictureInPicture | `pip` | Lucide `picture-in-picture-2` | 0 |
| ZoomIn | `plus.magnifyingglass` | Lucide `zoom-in` | 0 |
| ZoomOut | `minus.magnifyingglass` | Lucide `zoom-out` | 0 |
| Pointer | `cursorarrow` | Lucide `mouse-pointer-2` | 2 |
| Watch | `eye` | Lucide `eye` | 4 |
| Hidden | `eye.slash` | Lucide `eye-off` | 0 |
| NewChat | `plus.bubble` | Lucide `message-square-plus` | 1 |
| Info | `info.circle` | Lucide `info` | 10 |
| Help | `questionmark.circle` | Lucide `circle-question-mark` | 2 |
| Return | `return` | Lucide `corner-down-left` | 2 |
| ReplyIndent | `arrow.turn.down.right` | Lucide `corner-down-right` | 0 |
| Reply | `arrowshape.turn.up.left` | Lucide `reply` | 1 |
| ArrowUp | `arrow.up` | Lucide `arrow-up` | 8 |
| ArrowDown | `arrow.down` | Lucide `arrow-down` | 6 |
| ArrowRight | `arrow.right` | Lucide `arrow-right` | 2 |
| Sort | `arrow.up.arrow.down` | Lucide `arrow-up-down` | 3 |
| Terminal | `terminal` | Lucide `square-terminal` | 12 |
| Retry | `arrow.counterclockwise` | Lucide `rotate-ccw` | 3 |
| Refresh | `arrow.clockwise` | Lucide `rotate-cw` | 9 |
| Undo | `arrow.uturn.backward` | Lucide `undo-2` | 0 |
| Link | `link` | Lucide `link` | 3 |
| Code | `chevron.left.forwardslash.chevron.right` | Lucide `code` | 3 |
| Mention | `at` | Lucide `at-sign` | 6 |
| TextFormat | `textformat` | Lucide `a-large-small` | 1 |
| Bold | `bold` | Lucide `bold` | 1 |
| Italic | `italic` | Lucide `italic` | 1 |
| Strikethrough | `strikethrough` | Lucide `strikethrough` | 1 |
| AlignLeft | `text.alignleft` | Lucide `text-align-start` | 0 |
| Options | `slider.horizontal.3` | Lucide `sliders-horizontal` | 0 |
| Edit | `pencil` | Lucide `pencil` | 0 |
| Expand | `arrow.up.left.and.arrow.down.right` | Lucide `maximize-2` | 2 |
| Collapse | `arrow.down.right.and.arrow.up.left` | Lucide `minimize-2` | 1 |
| SignOut | `rectangle.portrait.and.arrow.right` | Lucide `log-out` | 0 |
| Emoji | `face.smiling` | Lucide `face-slightly-smiling` | 4 |
| Chart | `chart.xyaxis.line` | Lucide `chart-line` | 2 |
| BarChart | `chart.bar` | Lucide `chart-column` | 7 |
| Activity | `waveform.path.ecg` | Lucide `activity` | 2 |
| Delete | `trash` | Lucide `trash` | 0 |
| Star | `star` | Lucide `star` | 1 |
| StarFill | `star.fill` | drawn `star-fill` | 1 |
| ScreenShare | `rectangle.inset.filled.and.person.filled` | Lucide `screen-share` | 0 |
| Scan | `viewfinder` | Lucide `scan` | 1 |
| Folder | `folder` | Lucide `folder` | 1 |
| Calendar | `calendar` | Lucide `calendar` | 1 |
| Target | `scope` | Lucide `crosshair` | 0 |
| Mic | `mic` | Lucide `mic` | 0 |
| Book | `book` | Lucide `book-open` | 5 |
| Square | `square` | Lucide `square` | 0 |
| Asterisk | `asterisk` | Lucide `asterisk` | 0 |
| Cpu | `cpu` | Lucide `cpu` | 1 |
| Memory | `memorychip` | Lucide `memory-stick` | 1 |
| Disk | `internaldrive` | Lucide `hard-drive` | 2 |
| Network | `network` | Lucide `globe` | 1 |
| Key | `key` | Lucide `key-round` | 4 |
| Layers | `square.stack.3d.up` | Lucide `layers` | 0 |
| Logs | `doc.text.magnifyingglass` | Lucide `file-search` | 0 |
| History | `clock.arrow.circlepath` | Lucide `rotate-ccw-clock` | 0 |
| Spinner | `progress.indicator` | Lucide `loader` | 7 |
| Replace | `arrow.left.arrow.right` | Lucide `arrow-left-right` | 0 |
| BottomPanel | `rectangle.bottomthird.inset.filled` | Lucide `panel-bottom` | 0 |

Uses counts the places in `apps/desktop/src` that name the concept; 0 means only gpui-component
or an older Lucide name reaches it.

## 6. Scrollbars

- The same scrollbar as on the Mac, with the same behavior: the app's own thin overlay bar, no
  track, no arrow buttons, never Windows' classic 17 px bar.

Only compiled.

## 7. Sheets, alerts and dialogs

- Sheets and alerts (NATIVE.md §7.1, §7.2) are drawn by the app inside their window, so they are
  the same on Windows: attached under the toolbar row, dimming only that window.
- Anything the system draws is a Windows dialog, centered on its window and modal to it, where
  the Mac shows a sheet: open and save panels, the print dialog, the share picker. They are never
  restyled.
- Button order stays the Mac's on both systems (取消 before the default button at the trailing
  edge; stacked in alerts): one layout for one product. Enter runs the default, Esc cancels.
- Copy that names the computer says 这台电脑 where the Mac says 这台 Mac; the copy catalog needs
  the Windows form of every such string.

Only compiled.

## 8. Settings window

- A separate window with the same toolbar tabs and panes (NATIVE.md §8), opened by `Ctrl+,`, the
  first menu's 设置… and the tray menu. It uses the title bar of §1 with the maximize button
  disabled; the pane name is the window title, in the row, and on the taskbar.
- The 快捷键 tab draws a keyboard (§5) and lists shortcuts in the form of §3.
- 登录时启动 writes the current user's Run key; 菜单栏里显示 controls the tray icon (§2).
- 机器 › 用这台电脑 installs a scheduled task instead of a launch agent; the pane looks the same.

Only compiled.

## 9. What the system decides

- Light or dark follows the system's app mode. The accent follows the accent color the person
  chose in Windows' settings, as on the Mac; the two blues Windows ships with count as "not
  chosen" and give graphite, so a fresh system keeps the black-and-white finish.
- Reduce Motion follows 设置 › 辅助功能 › 视觉效果 › 动画效果 (`SPI_GETCLIENTAREAANIMATION`).
- Display scale: every size is in device-independent pixels; check 100 %, 125 %, 150 % and
  200 %, and a window dragged between two displays of different scale.
