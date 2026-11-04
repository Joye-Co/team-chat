# Native client rules (binding)

TeamChat is a Mac app. It must look and behave like a well-made macOS 26 (Tahoe) app, not like a
web page in a window. This file overrides `docs/DESIGN.md` §1–§8 and §12–§13 where they differ.
DESIGN.md still owns Card v2 semantics (§7.10, §7.13–§7.17), member status words (§9), content
(§10) and accessibility (§11).

Joye's words (2026-11-04): the whole product still uses web logic, not client logic. Settings has
ugly input boxes for 名字 and 岗位, ugly 退出 buttons, and icons that do not match. "按照客户端的逻辑去做".
It is not a color problem: keep the monochrome palette and the four state colors.

References (look at them before you change a screen; never capture other apps' windows):
Apple Mail, Messages, Notes and Mail Settings on macOS 26; the Apple HIG pages Settings, Toolbars,
Sidebars, Buttons, Text fields, Toggles, Pop-up buttons, Sheets, Menus; Codex and Claude desktop
for the AI-specific parts (composer pickers, inspector). Images:
`docs/design/native-refs.md` lists them with URLs.

## 1. The ten rules

1. **The window title lives in the toolbar.** No heading, breadcrumb or page title inside the
   content. A page shows a title and a subtitle at the leading edge of the toolbar row (Mail:
   "All Inboxes / Primary · 36 messages"). No "a / b" breadcrumbs.
2. **SF Symbols only.** Every icon is an SF Symbol drawn by the system (`ui::sf_symbols`), one
   symbol per concept (§6). No Lucide. One weight per context. No filled-tile icons next to line
   icons in one list.
3. **Native controls at native sizes.** Push buttons, pop-up buttons, switches, checkboxes,
   segmented controls, search fields and text fields follow §5. No 36–40 pt web inputs, no black
   rectangles, no outlined red buttons.
4. **No pills.** State is an SF Symbol plus text in the state color, or a dot. Never a colored
   capsule with a label ("需要你审 PR #61", "未合并 · 等你审", 岗位 tags, "#61 +214 −63" chips).
   The only capsules are Tahoe controls (buttons, search field, toolbar groups) and the sidebar
   unread count.
5. **Lists, not cards and dashboards.** Collections are rows (Mail, Contacts, Finder). Numbers are
   label/value rows, not stat tiles. A card is only a Card v2 message from a member.
6. **Forms are grouped forms** (System Settings style): label on the left, value or control on
   the right, rows separated by hairlines inside one rounded group. A value that people can edit
   is plain text until it has focus (§5.6), or it opens a sheet with an `编辑…` button.
7. **Settings is a separate window** (⌘, and App menu › 设置…) with toolbar tabs (§8). Changes
   apply at once; there is no Save. The window title is the pane name.
8. **Sheets, popovers, menus and alerts are the only overlays.** A sheet comes from the top of
   its window and dims only that window. Destructive actions confirm in an alert. No centered
   web modals, no toasts (§7).
9. **Mac behavior:** the arrow cursor on every control (a pointing hand only on URLs); no hover
   backgrounds on rows; a context menu on every object; arrow keys in lists; Return and Escape
   in sheets; an inactive window turns selection grey.
10. **Density is Mac density:** 13 pt interface text, 32 pt sidebar rows, 24 pt controls, 11 pt
    secondary text, message text 14 pt. Spacing on a 4 pt grid.

## 2. Window anatomy (main window)

```
┌──────────────┬───────────────────────────────────────────────┬─────────────┐
│ ● ● ●   [✎]  │ Title                         (◎ ▤ ⇡)  (▥)    │  Inspector  │
│ ⌕ 搜索        │ subtitle                                       │  (toggle)   │
│ 首页  产出  成员 │                                               │             │
│ 开发 ▾        │   transcript                                   │             │
│  · topic      │                                               │             │
│ 私聊 ▾        │   (+) [ 回复，或 @ 成员交代…            ] (↑)   │             │
└──────────────┴───────────────────────────────────────────────┴─────────────┘
 floating sidebar   content (toolbar row is transparent)          inspector
```

| Part | Rule |
| --- | --- |
| Window | Min 960 × 620, default 1280 × 820. Content background `content` (§4). |
| Sidebar | A floating panel inset 8 pt from the window's top, leading and bottom edges, width 248 (drag 200–320), corner radius 18 (concentric with the 26 pt window corner), fill `sidebar`, 0.5 pt edge `glassEdge`, shadow `float` (§4). The traffic lights sit inside it (system position: red light centre about 25, 26 pt from the window corner). The content background runs under the 8 pt gap. |
| Toolbar row | 52 pt, transparent over the content. Leading: title (15 pt semibold) over subtitle (11 pt, `secondary`), starting 20 pt after the sidebar. Trailing: toolbar groups (§5.1), 36 pt tall with 8 pt above and below, 12 pt from the inspector or window edge. At most three groups. |
| Scroll edge | Content that scrolls under the toolbar fades out over 24 pt (scroll edge effect); no hairline under the toolbar. |
| Inspector | Width 280, full height edge to edge (Tahoe inspectors do not float), fill `sidebar`, a 1 px `separator` on its leading edge. Toggle with the toolbar item `sidebar.trailing` and ⌥⌘I (View menu). |
| Split dividers | 0.5 pt `separator`, 8 pt drag area. |

## 3. Typography

System font (SF Pro; PingFang SC for Chinese). macOS text styles, with CJK line heights:

| Style | Size / line | Weight | Use |
| --- | --- | --- | --- |
| Large title | 26 / 32 | Bold | Only the member and 产出 detail name |
| Title 2 | 17 / 22 | Semibold | Sheet titles, alert titles |
| Title 3 | 15 / 20 | Semibold | Toolbar title, settings account name |
| Headline | 13 / 18 | Semibold | Sender names, row titles that need weight, section headers in content |
| Body | 13 / 18 | Regular | All interface text: rows, labels, buttons, menus, fields |
| Message | 14 / 21 | Regular | Message and card body text only |
| Callout | 12 / 16 | Regular | Card facts, captions under media |
| Subheadline | 11 / 14 | Regular | Subtitles, times, secondary lines, footers under groups |
| Section | 11 / 14 | Semibold | Sidebar and inspector section headers, `secondary` color |
| Footnote | 10 / 13 | Regular | Only the composer hint and dense table metadata |

Numbers use the system font with tabular figures (no monospace font for UI numbers; mono only in
code). Never bolder than Semibold except Large title.

## 4. Color

Keep the monochrome palette and the four state colors (amber = needs you, blue = live work,
red = problem, green = done). Use the macOS semantic values:

| Token | Light | Dark | macOS name |
| --- | --- | --- | --- |
| `content` | #FFFFFF | #1E1E1E | textBackgroundColor |
| `window` | #FFFFFF | #1E1E1E | settings window background |
| `sheet` | #FFFFFF | #2C2C2E | sheets, alerts, popovers, menus (menus at 92 %) |
| `sidebar` | #F7F7F8 | #2A2A2C | sidebar and inspector glass (Tahoe Finder measures 250 gray in light) |
| `group` | #000000 at 4 % | #FFFFFF at 6 % | grouped form section fill: quiet gray, never a white card |
| `label` | #000000 at 85 % | #FFFFFF at 85 % | labelColor |
| `secondary` | #000000 at 50 % | #FFFFFF at 55 % | secondaryLabelColor |
| `tertiary` | #000000 at 26 % | #FFFFFF at 25 % | tertiaryLabelColor |
| `quaternary` | #000000 at 10 % | #FFFFFF at 10 % | quaternaryLabelColor |
| `separator` | #000000 at 10 % | #FFFFFF at 10 % | separatorColor |
| `fill` | #000000 at 5 % | #FFFFFF at 8 % | control fill (push button, segmented track) |
| `fillHover` | #000000 at 8 % | #FFFFFF at 12 % | borderless and toolbar button hover |
| `selection` | #000000 at 6 % | #FFFFFF at 10 % | sidebar selection (Tahoe: gray fill, accent symbol and text) and unfocused list selection |
| `accent` | #1D1D1F | #F5F5F7 | app accent (ink): default button, focused selection, switch on, checkbox on |
| `onAccent` | #FFFFFF | #1D1D1F | text and symbols on `accent` |
| `focus` | accent at 50 % | accent at 50 % | 3 pt focus ring outside the control |
| `glassEdge` | #FFFFFF at 70 % | #FFFFFF at 12 % | 0.5 pt inner edge of floating glass |
| `amber` | #FF9500 | #FF9F0A | systemOrange: needs you |
| `blue` | #007AFF | #0A84FF | systemBlue: live work |
| `red` | #FF3B30 | #FF453A | systemRed: problem |
| `green` | #34C759 | #30D158 | systemGreen: done |

Shadows: `float` = 0 8 24 rgba(0,0,0,.12) plus 0 0 0 0.5 rgba(0,0,0,.06) in light;
0 8 24 rgba(0,0,0,.45) in dark. Only floating things cast a shadow (sidebar panel, toolbar
groups, menus, popovers, sheets). Rows, groups and cards have no shadow.

Inactive window: `accent` selection becomes `selection`; state colors stay; toolbar symbols use
`secondary`.

## 5. Controls

All controls come from gpui-component, styled through `ui::native`. Heights are the visible
shape. The text inside every control is Body 13 pt unless noted.

### 5.1 Toolbar items
- Icon-only items: 36 × 36 circle, symbol 16 pt Regular, color `label`.
- Related items share one capsule group: 36 pt tall, items 36 wide, no dividers, a toggled
  item shows a 28 pt gray pill inset 4 pt (`selection`). Groups are 8 pt apart.
- The one primary action of a window (if any) is prominent: `accent` fill, `onAccent` symbol,
  at the trailing edge.
- Capsule fill: light #FFFFFF at 80 % + `glassEdge` + `float` shadow; dark #FFFFFF at 8 % +
  `glassEdge`. Hover: `fillHover` on the item. Pressed: `quaternary`.
- A toggled item (inspector shown) shows its symbol in `accent` on `selection`.
- Every toolbar item is also a menu bar command.

### 5.2 Push buttons
- Regular: 24 pt tall, radius 6 (Tahoe: mini to regular are rounded rectangles), 12 pt side
  padding, min width 64. Small: 20 pt tall, radius 5, 10 pt padding, Callout 12 pt.
  Large: 28 pt tall capsule, 16 pt padding: only the bottom buttons of sheets and alerts and the
  login window.
- Standard: fill `fill`, text `label`. Hover: none. Pressed: `fillHover`.
- Default (one per view at most, Return activates it): fill `accent`, text `onAccent`.
- Destructive: a standard button whose title says what it destroys; the alert that follows
  makes the red choice. Never red outlines.
- A title that opens more UI ends with `…` (编辑…, 退出登录…, 邀请成员…).
- Square buttons for list editing: a 22 × 22 `+` / `−` pair under a list, joined, fill `fill`.

### 5.3 Pop-up buttons and pull-downs
- Same shape as a push button. Title plus a `chevron.up.chevron.down` 9 pt at the trailing edge
  (pop-up) or `chevron.down` (pull-down). Opens a native-style menu (§7.3) aligned so the chosen
  item sits over the button.
- Use a pop-up for any choice of 3+ options. Do not draw web dropdowns ("我参与的 ⌄" text links).

### 5.4 Switches, checkboxes, radio buttons
- Switch in a grouped form row: small, 32 × 18 (knob 16, pill). Regular 40 × 24 only for a
  section's master switch. On: `accent` track; off: `quaternary` track; knob white with shadow.
- Checkbox 16 × 16, radius 4; on: `accent` fill with an `onAccent` `checkmark` 11 pt.
- Radio 16 × 16 circle; on: `accent` with an `onAccent` 6 pt dot.

### 5.5 Segmented control
- 24 pt tall capsule track `fill`; selected segment a raised capsule (`group` fill, shadow
  0 1 2 rgba(0,0,0,.12)); labels 13 pt. In the toolbar it uses the toolbar group height (32).
- Use for 2–5 views of the same content (列表 / 网格; 全部 / 在做事 / 等人). Never underline tabs.

### 5.6 Text entry
- **Inline value field** (grouped form rows): no border, no fill, right-aligned, `secondary`
  color at rest; on focus it shows `label` text, a caret and the focus ring around the text
  only. Commits on Return or focus loss; Escape reverts. This is how 名字 and 岗位 look.
- **Standalone field** (sheets, search in a sheet): 24 pt tall, radius 6, fill `content`,
  1 px `separator` border, 7 pt side padding, focus ring 3 pt `focus`; login uses the 28 pt
  large size. Width fits the expected text (name 220, code 140), never full-window width.
- **Search field**: 30 pt capsule in the sidebar (24 pt elsewhere), fill `fill`,
  `magnifyingglass` 13 pt `secondary`, placeholder `搜索`. Shortcut hints live in the menu bar,
  not inside the field.
- Placeholders are hints, never the only label.

### 5.7 Progress
- Spinner (`ProgressView` style) for unknown waits; no skeleton shimmer; no "加载中…" text next to
  a spinner. A determinate bar is 4 pt tall, radius 2, track `quaternary`, fill `label`, or the
  state color when it shows a state.

## 6. Icons (SF Symbols)

- Drawn by `ui::sf_symbols` (system raster, never bundled). Weight Regular next to 13 pt text,
  Medium in the toolbar. Sizes: sidebar 15 pt in an 18 pt slot; toolbar 15 pt; inline with 13 pt
  text 13 pt; inline with 11 pt text 11 pt; settings toolbar tabs 18 pt.
- Color: `secondary` in sidebars and rows; `label` in the toolbar; state color only when the symbol
  *is* the state.
- One symbol per concept. Use this table; add a row here before you use a new concept.

| Concept | Symbol |
| --- | --- |
| 首页 | `tray` |
| 产出 | `square.stack` |
| 成员 | `person.2` |
| 群 (section) | none (section header text) |
| 话题 | `number` only where a topic needs an icon without a state |
| New topic / compose | `square.and.pencil` |
| Search | `magnifyingglass` |
| Filter | `line.3.horizontal.decrease` |
| 单独问 | `bubble.left.and.text.bubble.right` |
| PR | `arrow.triangle.pull` |
| Inspector toggle | `sidebar.trailing` |
| Sidebar toggle | `sidebar.leading` |
| Attach / more input | `plus` |
| Send | `arrow.up` on an `accent` circle |
| Needs you | `exclamationmark.circle.fill` in `amber` |
| Live work | `circle.fill` 6 pt dot in `blue` (animated ring only in 实时查看) |
| Done | `checkmark.circle.fill` in `green` (rows: `checkmark` in `secondary`) |
| Problem | `exclamationmark.triangle.fill` in `red` |
| Waiting on someone | `clock` in `secondary` |
| Computer (team machine) | `desktopcomputer`; laptop `laptopcomputer`; Mac mini `macmini` |
| Cloud runner | `cloud` |
| Settings tabs | 通用 `gearshape`, 账号 `person.crop.circle`, 通知 `bell`, 成员 `person.2`, 机器 `macmini`, 外观 `paintpalette`, 快捷键 `command` |
| Lock / needs approval | `lock` |
| Open in new window | `macwindow` |
| More | `ellipsis` (in a circle only as a toolbar item) |

## 7. Overlays

### 7.1 Sheets
- Attached to their window: slide down from under the toolbar row (0.25 s ease-out), centered
  horizontally, width 460 (forms) or 560 (lists), radius 22, fill `sheet`, shadow `float`; the
  window behind dims with black at 20 % (light) or 40 % (dark). Only one sheet at a time.
- Layout: title (Title 2) top-leading at 24 pt insets, optional one-line explanation (Body,
  `secondary`), the form, then buttons at the bottom trailing edge: `取消` then the default
  button, 12 pt apart, 20 pt from the edges. Escape = 取消, Return = default.
- Used for: 新话题, 新群, 改名, 添加成员, 编辑资料, 交代给成员, 按群设置通知.

### 7.2 Alerts
- For confirmations and errors that block: app icon 48 pt, title (Title 2 at 13 pt bold in the
  Tahoe alert layout), message, optional checkbox (e.g. 同时删除这台 Mac 上的记录), buttons stacked
  full width in the Tahoe style: default on top, `取消` below; the destructive choice has red text.
- Used for: 退出登录…, 离开群…, 停止成员的工作…, 删除…

### 7.3 Menus and popovers
- Menus: radius 12, fill `sheet` at 92 % with `glassEdge` and `float`, 5 pt inner padding,
  rows 24 pt, Body 13 pt, a leading SF Symbol 13 pt for all items of a group or for none,
  shortcut column in `secondary`, 1 px separators inset to the text column. Hover: a rounded row
  inset 5 pt, radius 6, `accent` fill, `onAccent` text.
- Context menus on every row, message, card, member, 产出 and file. Show no keyboard shortcuts
  in context menus. Every context-menu item also exists in the main UI or the menu bar.
- Popovers: radius 12, `sheet` fill, `float` shadow, an arrow pointing at the control; close on
  outside click. One at a time.

### 7.4 No toasts
- Inline status in the place it changed (a row shows 已发送 / 没有发出 · 重试), a sidebar count, or a
  system notification when the app is in the background. Errors that block use an alert.

## 8. Settings window

Separate window, ⌘, and App menu › 设置…; minimize and zoom dimmed; reopens the last pane.

- Toolbar tabs (Mail Settings style): symbol 18 pt over a 11 pt label, 56 pt tall; the selected
  tab has `selection` fill (radius 8) and `accent` symbol and label. Window title = pane name.
- Width 640; height follows the pane (animated 0.2 s), max 720 with scrolling inside.
- Panes use grouped forms on `window`: groups `group` fill (quiet gray, no shadow), radius 10,
  20 pt side margins, rows min 36 pt with 12 pt side insets, 1 px `separator` between rows inset
  12 pt, 20 pt between groups, group header 13 pt semibold `label` 6 pt above, footer 11 pt
  `secondary` 6 pt below.

Panes:

| Pane | Content |
| --- | --- |
| 通用 | 启动时 (pop-up: 打开首页 / 打开上次的话题), 登录时启动 (switch), 菜单栏里显示 (switch), 声音 (switch). |
| 账号 | Header row: avatar 64, name (Title 3), 岗位 · team (`secondary`), trailing `编辑…` → sheet (avatar, name and 岗位 as standalone fields, jojo character choice for AI members only). Group 登录的设备: rows with `laptopcomputer` / `desktopcomputer` / `iphone`, name, `这台 Mac` or last seen; selection plus `−` square button below the group signs a device out (alert). Bottom-leading `退出登录…` push button → alert with checkbox `同时删除这台 Mac 上的聊天记录和下载的产出`. |
| 通知 | Group 提醒我 (mini switches): 需要我拍板的事, 我交代的事做完了, 成员卡住或出错, 有人 @我, 每日摘要 (with time pop-up). Group 勿扰: switch + 从/到 time pop-ups. `按群设置…` → sheet with a table (群, pop-up 全部 / 只有 @我 / 不提醒). |
| 成员 | Team admin (owners only): 邀请成员…, 添加 AI 成员…, rows of members with 级别 pop-ups; group 默认规则 for AI members (who approves 合并到 main, 发布到生产, 单次花费超过 ¥…). |
| 机器 | Where AI members work, on demand. Group 团队的电脑 (rows: symbol, name, 在线 / 空闲 dot, who uses it, version); group 云端 (cloud agents and our containers; shows 还没有 when none). `+`/`−` square buttons under each group. Never say that members run only on local computers. |
| 外观 | 外观 three image tiles 浅色 / 深色 / 自动 (System Settings style), 消息文字大小 slider (A … A). |
| 快捷键 | Read-only table: action, shortcut. |

## 9. Screens

### 9.1 Sidebar
- Top: search field (§5.6) under the traffic lights row; `square.and.pencil` toolbar-style
  button at the trailing edge of the traffic lights row.
- Destinations without a header: 首页, 产出, 成员 (32 pt rows as in Tahoe Finder, symbol 15 pt
  `secondary`).
- One section per 群: header (Section style) with a disclosure chevron that appears on hover at
  the trailing edge; topic rows 32 pt: 18 pt leading slot (state symbol, member avatar 16 pt
  when a member works on it, or nothing), title 13 pt (unread: Semibold), trailing 11 pt
  `secondary` count or time. Needs you: `amber` symbol. Unread count: gray capsule 11 pt.
- 私聊 section: avatar 16 pt + name.
- Selection (Tahoe): a rounded rect inset 10 pt from the panel sides, radius 7, `selection`
  fill; the selected row's symbol and text use `accent`. No hover fill. No account row, avatar
  strip or settings button at the bottom (HIG: nothing critical at the bottom of a sidebar).
- The "我参与的" filter moves to the View menu and the `line.3.horizontal.decrease` item.

### 9.2 Topic (content)
- Toolbar: title = topic name; subtitle = 群 · status words (§9 DESIGN.md) in `secondary`; a
  needs-you status shows `exclamationmark.circle.fill` `amber` before the subtitle text.
  Trailing group (单独问, 产出, PR) and the inspector toggle.
- Transcript: full content width with 24 pt side insets; message text max 680 pt wide.
  Sender block: avatar 28, name Headline, then 岗位 and time as one `secondary` Subheadline run
  ("前端实习 · 09:12"), no tag pill. Consecutive messages from one sender within 5 minutes share
  the block. Day separator: centered `secondary` Subheadline text, no rules.
- Card v2 in the transcript: radius 12, fill `fill`, no border, no shadow, 14 pt insets. Facts
  are symbol + text runs separated by 12 pt, not chips. Actions are push buttons (§5.2), the
  default action uses the default style.
- Decision bar above the composer: a row on `fill` radius 12 with the state symbol, title
  (Headline) and one-line context, trailing push buttons (`看改动`, `要求修改…`, default `批准合并`).
- Composer (Messages + Codex): `plus` circle button 28 pt outside the field (menu: 文件…, 图片…,
  截图); field radius 18, min 36 pt, grows to 8 lines, fill `content` with 0.5 pt `separator`
  (light) or `fill` (dark); placeholder `回复，或 @ 成员交代…`; send = 24 pt `accent` circle
  with `arrow.up`, shown only when there is text. When the message mentions an AI member, a
  pop-up appears under the field: where the member works (a team computer, or the cloud when
  it is available).

### 9.3 Inspector (topic)
- Sections in this order: 负责, 计划, 改动, 产出, 用到的决定, 在哪干活, 用时和成本.
- Header: Section style with a disclosure chevron; content rows Body 13 pt; label `secondary`
  left, value `label` right, or one value per row.
- Actions are small push buttons or a trailing `arrow.up.forward` borderless button, never
  "展开 ⌄" / "看改动" text links.
- 在哪干活 shows the computer or the cloud runner: symbol, name, state dot, `secondary` detail.

### 9.4 首页
- A list like Mail's message list, grouped by section headers: 需要你拍板, 正在做, 做完了.
- Rows 56 pt: avatar 32, title Headline, one `secondary` line, trailing time; needs-you rows show
  trailing small push buttons (`看看`, default `批准`). Selection opens the topic.
- Empty: centered `secondary` Body text `没有要你处理的事`. No illustrations.

### 9.5 成员
- Two panes: list (avatar 32, name Headline, 岗位 · state Subheadline; 52 pt rows; segmented
  control 全部 / 在做事 / 等人 in the toolbar) and a detail like a Contacts card: avatar 64,
  name Large title, 岗位 and who they report to `secondary`, a row of push buttons (`交代…`
  default, `单独问`, `暂停`), then grouped sections with label/value rows: 现在在做, 这周 (完成的
  话题 6 · 平均 38 分钟; 产出 4; 成本 ¥132 / 上限 ¥300 with a 4 pt bar), 最近交付 (rows),
  在哪干活, 需要你批准的事, 能用的工具, 用到的决定. No stat tiles, no terminal preview tile.

### 9.6 产出
- Finder-like: toolbar segmented `square.grid.2x2` / `list.bullet`, a sort pop-up, search.
  Grid: 160 pt thumbnails, radius 8, name 13 pt below, `secondary` kind · time. List: columns
  名称, 类型, 话题, 时间. Space bar opens the preview.

### 9.7 Other windows
- 搜索或交代 (⌘K): a Spotlight-style floating panel, 600 wide, field 22 pt text in a 48 pt
  capsule, results 32 pt rows with symbols; Return opens, ⌘Return 交代.
- Login: centered app icon 96, Title 2 welcome line, one standalone field at a time, default
  button `继续` bottom-trailing. No marketing copy.
- 产出 window, 实时查看: same toolbar and control rules.

## 10. Behavior checklist (every screen)

- Arrow cursor everywhere except URL links (pointing hand) and text (I-beam). Remove every
  `cursor_pointer()` that is not on a URL link.
- No hover background on rows, cards or groups. Hover is allowed on toolbar items, borderless
  buttons and menu rows only.
- Right click on every object opens its context menu.
- Lists: ↑/↓ move selection, Return opens, ⌫ deletes where deleting is allowed (with an alert).
- Sheets: Escape cancels, Return runs the default button; focus starts in the first field.
- Inactive window: selection and accent fills turn `selection`.
- Labels are not selectable; message text is selectable.
- Motion: menus and popovers fade 0.12 s; sheets slide 0.25 s; no spring bounce on frequent
  interactions; honour Reduce Motion.

## 11. How to check a screen

Capture light and dark (`desktop --screenshot <dir> --theme both --scenes <yours>`), look at the
PNGs next to the reference images, and answer for every screen:
1. Is there any heading, breadcrumb or pill in the content? (must be no)
2. Is every icon an SF Symbol from §6 at the right size and weight?
3. Are all controls at §5 sizes and shapes?
4. Would this layout look at home next to Mail and Messages on macOS 26?
