# Design v2

**v2 replaces v1 ("Native calm").** v1 was invented without looking at real work chats, and Joye
found the result ugly, with too many basic UI and UX faults. v2 copies proven patterns from four
reference products and gives each section the reference it follows. v1 stays in git history and
in [design/mockups/](design/mockups/) only as a record. Do not build from it.

Implementers follow this file exactly. When a value is not here, ask before you invent one.
AGENTS.md and docs/PRODUCT.md win for product rules and interface words. This file wins for how
things look and move.

- Mockups: [design/mockups-v2/](design/mockups-v2/) (PNG at device scale 1, HTML beside each PNG).
  Rebuild: `bun docs/design/mockups-v2/src/build.mjs [name] [--scale=2]`.
- Tokens in code form: [design/mockups-v2/src/tokens.mjs](design/mockups-v2/src/tokens.mjs). Keep it
  in step with §2.
- App icon: [design/brand/](design/brand/) (unchanged; the dark variant `icon-dark.svg` is used
  on the dark login, §11.6).
- GPUI limits: [design/GPUI-CAPABILITIES.md](design/GPUI-CAPABILITIES.md). Per-change checklist:
  §15 of this file (it replaces design/CHECKLIST.md where they differ).

References, in this file by short name:

| Name | What we used | Where the evidence is (not in git) |
| --- | --- | --- |
| **Slack** | 2023 desktop layout: message rows, date pill, hover toolbar, reactions, thread summary and thread pane, sidebar sections, unread by weight, Block Kit task cards, agent sessions | `~/.cache/team-chat/design/refs/slack/` (crop-stanford-main.png, crop-thread-pane.png) |
| **Feishu** | 话题群 and 话题回复: topic list, topic pane with the root repeated, 同时发送到群, 全部 / 我订阅的 tabs, card spec (width caps, spacing presets, text scale), neutral color ramp, read and reaction behavior | `~/.cache/team-chat/design/refs/feishu/` (hc_360049067735_*.png, hc_423295624296_*.png, open_cards_*.png) |
| **GrokBot** | Bots as characters, one pending row instead of a stream, work cards with a status badge, the ask card with lettered options, draft-before-send card, the right pane with the bot's live state, quiet composer | `~/.cache/team-chat/design/refs/grokbot/` (guide-images/, native/, demo-*.png, baby-grok-bot-rules.css) |
| **OpenBot** | Measurements only (PolyForm Noncommercial: no code, styles or assets copied): one header height for every pane, a small enforced type and spacing scale, neutral selection, one status spot per row with a priority, collapsed tool steps, live line with elapsed time, 400 ms delay before busy indicators | `/Users/joye/Documents/code/openbot` (read only), `~/.cache/team-chat/design/refs/openbot/` |

## 0. Decisions per screen

| Screen | We follow | Why | Rejected |
| --- | --- | --- | --- |
| Shell | **Slack / Feishu**: tinted sidebar, white content, one hairline between columns, one 52 pt header row for every pane (**OpenBot**: all header lines meet) | Proven desktop layout; the tint separates navigation from content without boxes | Slack's icon rail and purple chrome; Feishu's app-suite nav; GrokBot's glass sidebar (60 fps budget) |
| Sidebar | **Slack** sections (待办, 群, 私聊) with collapse, unread told by weight, number badges only for what is for you; **GrokBot** bot rows show the character's live status | Scanning bold names is faster than scanning dots; one place answers "who waits for me" | Slack's 6-item filter menu; GrokBot's single flat list (it mixes groups and DMs, U18) |
| 群 → topic list | **Feishu** 话题群 (topics are the unit, 全部 / 等我 tabs) drawn as **Slack/OpenBot** two-line list rows with a preview | Feishu's model is ours; dense rows triage faster than Feishu's large cards at 5 to 30 topics | Feishu's floating "+" button (we use a labeled 新话题 button); full topic cards |
| Topic view | **Feishu** topic pane order (topic start, then replies, then composer) + **Slack** message rows (avatar, name and time on one line, grouping, date pill, hover toolbar) | No bubbles in a team topic: Feishu drops them in topics, Slack never had them | GrokBot / OpenBot bubbles (they suit 1:1 bot chat, not 5 to 10 people) |
| Bots and live work | **GrokBot** pending row and character avatar; **OpenBot** elapsed time and collapsed steps; **Slack** agent session states and a visible stop | "Acknowledges once, works quietly" (PRODUCT.md) needs one living line, not a token stream | A tool log in the timeline; shimmer text (GPUI cannot clip gradients to text cheaply) |
| Cards | **Feishu** card spec (width cap, fixed spacing and text sizes) + **GrokBot** work card, ask card and draft card + **Slack** Block Kit task/plan list | Every bot card looks native whoever wrote the JSON | Saturated Feishu header gradients |
| Composer | **GrokBot / Feishu**: one box, destination in the placeholder, `+` menu at the left, `@` and emoji at the right, send is the only colored control | Fewer controls; the destination is named so nobody posts into the wrong place | Slack's always-on format toolbar; v1's permanent key hints (U09) |
| Side panel | **GrokBot** right pane: the responsible bot's live state, then 产出, then members; closed by default | Context is one glance away but takes no width when there is nothing to show (U07) | v1's always-open empty panel |
| Search and switcher | **Feishu** `搜索 ⌘K` field that opens one switcher; **Slack** grouped results and ⌘F inside the current 群 | One search entry, shortcut taught in the field | A separate fixed-height search dialog (U35) |
| Settings | **Slack** preferences: left section list, content right, grouped table | Flat, no nested pages | Solid accent selection |
| Empty and error | **Slack** inline next action, **Feishu** one quiet line; errors say what failed and what to do | Quiet, never three empty states at once (U06) | Big illustrations |
| Login | macOS native form; server address behind a disclosure | Interns never need the server address (U39) | |

## 1. Principles

1. **Copy what works.** Every pattern below names its reference. A new pattern needs a reason
   that the references cannot cover (our 话题 status and `waitingOn` are the main one).
2. **Subtract.** One search entry, one header height, one chip shape, one modal component, one
   avatar rule, one date format. When two controls say the same thing, remove one.
3. **Color means you.** Accent blue: links, mentions of others, the active tab underline, send,
   primary buttons, the unread dot, focus. Orange: 等你… chips and mentions of you. Red: number
   badges (things for you) and errors. Everything else is neutral.
4. **No void, no crowding.** Short content starts at the top; long content anchors at the
   bottom. Nothing is shown empty "in case".
5. **Every interactive thing answers.** Hover, pressed, focus and cursor on every row and button
   (§7). This is the most common fault of v1 (U03, U04).
6. **Members are equal.** People and bots use the same rows and sizes. Bots look like their
   jojo-friends character, which also shows what they are doing.
7. **Quiet motion.** Short, ease-out, never on keyboard navigation (§8).

## 2. Color tokens

Values are sRGB `#RRGGBBAA` (the gpui-component theme JSON format). An alpha color is drawn over
the surface named in its role. `L` light, `D` dark. Contrast ratios were computed (WCAG) over the
named surface. The neutral ramp is calibrated on Feishu's documented grey ramp (text #1F2329,
secondary #646A73) and Slack's light mode (near-black text, very light hairlines); the dark ramp
uses OpenBot's lesson of at least 4 % lightness between elevation steps (U46).

### 2.1 Surfaces

| Token | L | D | Role |
| --- | --- | --- | --- |
| `sidebar` | `#F2F3F5FF` | `#18181BFF` | Sidebar, settings section list |
| `content` | `#FFFFFFFF` | `#1F1F23FF` | Topic list, topic view, side panel, login, settings content |
| `raised` | `#F6F7F9FF` | `#28282DFF` | Cards, live card, table header, code blocks in dark (`code-bg` in light is `#F4F5F7FF`, dark `#26262BFF`) |
| `overlay` | `#FFFFFFFF` | `#2F2F35FF` | Popovers, menus, @mention popup, switcher, dialogs, hover toolbar, toasts |
| `scrim` | `#0F111433` | `#00000073` | Behind every overlay that takes focus (switcher, dialogs, sheets, alerts) |

### 2.2 Text

| Token | L | D | Contrast L / D | Role |
| --- | --- | --- | --- | --- |
| `text-1` | `#1F2329FF` | `#EDEDF0FF` | 15.8 / 14.1 | Body, names, titles |
| `text-2` | `#5F6670FF` | `#A5A9B1FF` | 5.8 (5.2 on sidebar) / 7.0 | Previews, 岗位, secondary labels, icons at rest |
| `text-3` | `#80868FFF` | `#7E838CFF` | 3.7 / 4.3 | Times, counts in tabs, hints. Never content |
| `placeholder` | `#9AA0A8FF` | `#6C7079FF` | 2.6 / 3.3 | Field placeholders only |

### 2.3 Lines and fills

| Token | L | D | Role |
| --- | --- | --- | --- |
| `sep` | `#1F232917` | `#FFFFFF14` | 0.5 pt hairlines: column edges, header edge on scroll, table rows |
| `border` | `#1F232924` | `#FFFFFF21` | Cards, files, date pill, popover edge, option rows |
| `border-strong` | `#1F23293D` | `#FFFFFF38` | Composer, inputs, secondary buttons, selects |
| `hover` | `#1F23290B` | `#FFFFFF0A` | Hover on content rows, messages, ghost buttons |
| `selected` | `#1F232914` | `#FFFFFF16` | Selected topic row |
| `sidebar-hover` / `sidebar-selected` | `#1F23290D` / `#1F232917` | `#FFFFFF0B` / `#FFFFFF17` | Sidebar and settings list rows |
| `pressed` | `#1F23291F` | `#FFFFFF1F` | Pressed rows and ghost buttons |
| `field` | `#1F23290E` | `#FFFFFF11` | Search fields, kbd, neutral chips, reaction pills, empty send button |
| `active-row` | `#1F6BFF1A` | `#4C8DFF33` | Keyboard-active row in popups, menus, the switcher |

### 2.4 Accent and mentions

| Token | L | D | Role |
| --- | --- | --- | --- |
| `accent` | `#1F6BFFFF` | `#3D7EFFFF` | Send, primary buttons, caret, focus ring base, selected option border |
| `accent-hover` | `#165EEBFF` | `#5590FFFF` | Primary hover |
| `accent-text` | `#175CD9FF` (5.9) | `#7AA8FFFF` (6.9) | Links, @mentions of others, text buttons, topic unread count |
| `accent-soft` | `#1F6BFF1A` | `#4C8DFF29` | File tiles, my reaction, hovered option |
| `on-accent` | `#FFFFFFFF` | `#FFFFFFFF` | Text on accent (4.6 / 3.7: labels at 13 pt weight 500 or larger only) |
| `focus` | `#1F6BFF8C` | `#4C8DFFA6` | 2 pt keyboard focus ring with a 2 pt gap in the surface color |
| `mention-bg` | `#1F6BFF1A` | `#4C8DFF2E` | @mention of someone else (text `accent-text`) |
| `me-bg` / `me-text` | `#FF8A002E` / `#9A4A00FF` | `#FF9F0A38` / `#FFB547FF` | @mention of me |
| `unread-dot` | `#1F6BFFFF` | `#4C8DFFFF` | 6 pt dot left of an unread topic row |

The accent is our own blue, calibrated between Slack's link blue and Feishu's blue-500; it does
not follow the macOS accent color (one fewer variable to test).

### 2.5 Status tones (chips and badges)

One chip shape (Feishu tag, GrokBot badge): 20 pt high, radius 6, padding 6 left / 7 right,
gap 4, a 12 pt Lucide glyph, 12/16 text weight 500, no border, max width 168 pt.

| Tone | L fill / text | D fill / text | Used for |
| --- | --- | --- | --- |
| `info` | `#1F6BFF1A` / `#175CD9FF` | `#4C8DFF2E` / `#7AA8FFFF` | 进行中 (header only, §9.1) |
| `you` | `#FF8A0029` / `#9A4A00FF` (5.4), weight 600, glyph `#E07800FF` | `#FF9F0A2E` / `#FFB547FF` (7.2), glyph `#FF9F0AFF` | 等你回答, 等你审 |
| `danger` | `#E5484D1F` / `#C2272DFF` (4.8) | `#F2555A2E` / `#FF7B7FFF` (5.6) | 卡住了, failed run |
| `success` | `#1E9E5A1F` / `#137042FF` | `#3CCB7F29` / `#5BD893FF` (6.8) | 完成, 已处理 (never red, U25) |
| `neutral` | `field` / `text-2` | `field` / `text-2` | 等{name}回答, 等{name}审, 等回答, 等审 |

| Status | Glyph | Tone (viewer in `waitingOn` / not) |
| --- | --- | --- |
| `open` | none | none |
| `in_progress` | `circle-dot` | info (hidden in rows, §9.1) |
| `waiting_answer` | `circle-question-mark` | you / neutral |
| `waiting_review` | `eye` | you / neutral |
| `blocked` | `circle-alert` | danger |
| `done` | `circle-check` | success |

In 等{name}…, the name truncates at 96 pt and 等 and the verb stay. In topic rows the name is
dropped (`等审`), because the row is narrow and the header shows it.

### 2.6 Badges, presence

| Token | L | D | Role |
| --- | --- | --- | --- |
| `badge` | `#E5484DFF` | `#F2555AFF` | Number badge: 18 pt capsule, min width 18, padding 5, 11/14 weight 600 white, tabular. Only for things for you: mentions (`@n` in topic rows, `n` on a 群), DMs, 待办. Cap `99+` |
| `online` | `#1FB25AFF` | `#34C76AFF` | Presence dot, 28 % of the avatar (min 7 pt), 2 pt ring in the surface color |
| offline dot | ring of `text-3` 1.5 pt, surface fill | same | Offline is a hollow dot, never nothing (U41) |

Unread without a mention is told by weight, not by a number (Slack): 群 name 600 + `text-1`;
topic title 600 + the unread dot + a 12 pt `accent-text` count.

### 2.7 Avatar palettes (people)

A stable hash of the member id picks one of 8 palettes (unchanged from v1 plus `violet`, which
replaces `graphite`). Values in [tokens.mjs](design/mockups-v2/src/tokens.mjs) `palettes`.

### 2.8 Content and syntax

`code-bg` §2.1; syntax colors as in v1 (Xcode defaults) through `HighlightTheme`. Danger fills
`#E5484DFF` / `#F2555AFF`; success icons `#1E9E5AFF` / `#3CCB7FFF`; warning icons `#E07800FF` /
`#FF9F0AFF`.

## 3. gpui-component theme mapping

One JSON theme set (`apps/desktop/assets/theme.json`, `light` and `dark`). App-owned roles (tones,
palettes, mentions, presence, `active-row`, shadows) live in `theme::Colors`. Views never write a
color literal.

| Slot(s) | L | D |
| --- | --- | --- |
| `background` | `#FFFFFFFF` | `#1F1F23FF` |
| `foreground` | `#1F2329FF` | `#EDEDF0FF` |
| `muted` / `muted_foreground` | `#1F23290E` / `#5F6670FF` | `#FFFFFF11` / `#A5A9B1FF` |
| `border` | `#1F232924` | `#FFFFFF21` |
| `input` | `#1F23293D` | `#FFFFFF38` |
| `ring` | `#1F6BFF8C` | `#4C8DFFA6` |
| `selection` | `#1F6BFF33` | `#4C8DFF5C` |
| `caret` | `#1F6BFFFF` | `#3D7EFFFF` |
| `overlay` | `#0F111433` | `#00000073` |
| `accent` / `accent_foreground` (component hover, highlighted row) | `#1F6BFF1A` / `#1F2329FF` | `#4C8DFF33` / `#EDEDF0FF` |
| `primary` / `_hover` / `_active` / `_foreground` | `#1F6BFFFF` / `#165EEBFF` / `#1252CCFF` / `#FFFFFFFF` | `#3D7EFFFF` / `#5590FFFF` / `#2F6BE0FF` / `#FFFFFFFF` |
| `secondary` / `_hover` / `_active` / `_foreground` | `#FFFFFFFF` / `#F6F7F9FF` / `#ECEDF0FF` / `#1F2329FF` | `#FFFFFF14` / `#FFFFFF1F` / `#FFFFFF29` / `#EDEDF0FF` |
| `button*` | as `secondary*` | as `secondary*` |
| `button_primary*` | as `primary*` | as `primary*` |
| `danger` / `_hover` / `_active` / `_foreground` | `#E5484DFF` / `#D93F44FF` / `#C2272DFF` / `#FFFFFFFF` | `#F2555AFF` / `#F66A6EFF` / `#D9474CFF` / `#FFFFFFFF` |
| `success`, `warning`, `info` | `#1E9E5AFF`, `#E07800FF`, `#1F6BFFFF` | `#3CCB7FFF`, `#FF9F0AFF`, `#3D7EFFFF` |
| `list` / `list_hover` / `list_active` / `list_active_border` / `list_even` / `list_head` | `#FFFFFFFF` / `#1F23290B` / `#1F232914` / `#00000000` / `#FFFFFFFF` / `#FFFFFFFF` | `#1F1F23FF` / `#FFFFFF0A` / `#FFFFFF16` / `#00000000` / `#1F1F23FF` / `#1F1F23FF` |
| `table` / `table_hover` / `table_active` / `table_row_border` / `table_head` / `table_head_foreground` | `#FFFFFFFF` / `#1F23290B` / `#1F232914` / `#1F232917` / `#F6F7F9FF` / `#5F6670FF` | `#1F1F23FF` / `#FFFFFF0A` / `#FFFFFF16` / `#FFFFFF14` / `#28282DFF` / `#A5A9B1FF` |
| `popover` / `popover_foreground` | `#FFFFFFFF` / `#1F2329FF` | `#2F2F35FF` / `#EDEDF0FF` |
| `sidebar` / `sidebar_foreground` / `sidebar_border` / `sidebar_accent` / `sidebar_accent_foreground` | `#F2F3F5FF` / `#5F6670FF` / `#1F232917` / `#1F232917` / `#1F2329FF` | `#18181BFF` / `#A5A9B1FF` / `#FFFFFF14` / `#FFFFFF17` / `#EDEDF0FF` |
| `tab` / `tab_active` / `tab_active_foreground` / `tab_foreground` / `tab_bar` | `#00000000` / `#00000000` / `#1F2329FF` / `#5F6670FF` / `#00000000` | same with dark text tokens |
| `switch` / `switch_thumb` | `#1F23293D` (off), `accent` on / `#FFFFFFFF` | `#FFFFFF38` / `#FFFFFFFF` |
| `slider_bar`, `progress_bar` | `#1F6BFFFF` | `#3D7EFFFF` |
| `skeleton` | `#1F23290E` | `#FFFFFF11` |
| `scrollbar` / `scrollbar_thumb` / `scrollbar_thumb_hover` | `#00000000` / `#1F232940` / `#1F232966` | `#00000000` / `#FFFFFF40` / `#FFFFFF66` |
| `link` / `link_hover` / `link_active` | `#175CD9FF` / `#1250BFFF` / `#0E449FFF` | `#7AA8FFFF` / `#94BAFFFF` / `#AECBFFFF` |
| `blue` / `blue_light` | `#1F6BFFFF` / `#1F6BFF1A` | `#3D7EFFFF` / `#4C8DFF2E` |
| `yellow` / `yellow_light` (the `you` tone) | `#9A4A00FF` / `#FF8A0029` | `#FFB547FF` / `#FF9F0A2E` |
| `red` / `red_light` | `#C2272DFF` / `#E5484D1F` | `#FF7B7FFF` / `#F2555A2E` |
| `green` / `green_light` | `#137042FF` / `#1E9E5A1F` | `#5BD893FF` / `#3CCB7F29` |
| `magenta`, `cyan` (+ `_light`) | not used; `blue` values | same |
| `window_border`, `drag_border`, `drop_target` | `#1F232924`, `#1F6BFFFF`, `#1F6BFF1A` | `#FFFFFF21`, `#3D7EFFFF`, `#4C8DFF2E` |

Theme fields: `font.family` `.SystemUIFont`, `font.size` 13, `mono_font.family` `Menlo`,
`mono_font.size` 12, `radius` 8, `radius.lg` 12, `shadow` true, `focus_ring` true,
`scrollbar_mode` `Scrolling`. `MotionTokens`: `duration_fast` 120 ms, `duration_normal` 160 ms,
`duration_slow` 200 ms, `easing_enter` and `easing_exit` `cubic_bezier(0.23, 1, 0.32, 1)`.

## 4. Typography

System font (`.SystemUIFont`, SF Pro); Core Text falls back to **PingFang SC** for Han. Mono:
Menlo. Weights 400, 500, 600 only. `tnum` on every number.

The scale has **six sizes** (OpenBot's enforced scale, Feishu's card text sizes). Nothing else is
allowed (U27). 11 pt exists only for number badges and kbd.

| Token | Size / line | Weight | Where |
| --- | --- | --- | --- |
| `display` | 20 / 28 | 600 | Topic start title (2 lines max), settings page title, login title, empty-state title |
| `title` | 16 / 22 | 600 | Topic view header title, topic list header (群 name), dialog title |
| `body` | 14 / 22 | 400 (600 for bold lead-ins) | Message text, composer text, card body. Measure 620 pt (about 44 Han characters). Paragraph gap 6; list item gap 4 (U13) |
| `ui` | 13 / 18 | 400; 500 row titles; 600 names in messages and unread rows | Sidebar, topic rows, buttons, menus, fields, card titles in lists, previews |
| `meta` | 12 / 16 | 400; 500 chips and section headers in panels; 600 sidebar section headers | Times, 待办 context, chips, tabs counts, file metadata, typing line, field help and errors |
| `badge` | 11 / 14 | 500 (kbd), 600 (badges) | Number badges, kbd, role tag |
| `code` | 12 / 18 | 400 Menlo | Code blocks, logs |

Line height 22 for 14 pt body is Feishu's and Slack's; it is enough for Han because PingFang's
ascent fits in 22.

Punctuation (U47): full-width punctuation in Chinese text (`：` `，` `（）`) with **no space after**
`：`. A Latin word next to Han gets one space (`staging 上`). Ellipsis is always U+2026 `…`, drawn
by one truncation helper (U26).

## 5. Spacing, radii, sizes

- **Spacing** (pt): 2, 4, 6, 8, 10, 12, 16, 20, 24, 32. (10 stays only for row inner padding
  and avatar gutters; remove 14 and 28, U27.)
- **Radii**: 4 (inline code, kbd, mention), 6 (chips, icon buttons in toolbars, role tag), 8
  (rows, buttons, fields, option rows, file tiles), 12 (cards, composer, popovers, dialogs),
  full (badges, pills, reactions, avatars). Concentric rule (OpenBot): outer = inner + padding.
  Remove 5, 5.5, 7, 10, 14.
- **Controls**: 28 pt standard (buttons, icon buttons, selects), 24 pt compact (inside rows and
  cards), 36 pt large (login, primary empty-state action), 30 pt fields in the sidebar and
  settings, 32 pt rows in menus.
- **Icons**: 16 pt in toolbars and the composer; 14 pt in buttons, fields and rows; 12 pt in
  chips and steps. Hit area at least 28 × 28 for every icon button (U48).
- **Avatars**: 32 messages and topic rows; 28 sidebar footer; 24 side panel, switcher, live row;
  20 sidebar rows and popups; 40 live card. People: circle with **one** initial for a CJK name
  (the surname, `王一鸣` → 王) and two letters for a Latin name (`Alex Chen` → AC); a leading
  emoji is ignored (`🧑‍💻 Priya` → P) (U18, U37). Bots: their character at its status
  (§9.3); no circle clip, no ring.

## 6. Layout

### 6.1 Main window

| Metric | Value |
| --- | --- |
| Window | Min 1100 × 680, default 1440 × 900, transparent title bar, traffic lights at (20, 20) |
| Header row | **52 pt in every pane** (sidebar top, topic list, topic view, side panel). No border; a 0.5 pt `sep` line appears under a header only while content scrolls under it (U11) |
| Columns | Sidebar 248 (200–320, drag to resize, ⌘⇧S hides it) · topic list 320 (280–400, resizable) · topic view flexible, min 480 · side panel 300 (260–380), closed by default. Each column edge is one 0.5 pt `sep` line |
| Narrow window | When the window is too narrow for all columns, shrink in this order: side panel closes, topic list to 280, sidebar to 200, then the sidebar hides. The topic view never clips (U44) |
| Reading measure | Messages, cards and the composer share one left edge and one max width: text 620 pt, composer and topic column 760 pt from the left padding (U45) |

### 6.2 Sidebar (Slack sections, GrokBot bot rows)

| Part | Value |
| --- | --- |
| Top | 52 pt band for the traffic lights only |
| Search | 30 pt field, 12 pt side margin, radius 8, `field`, `search` 14 + "搜索" placeholder + `⌘K` kbd. Clicking opens the switcher (§11.4). No other control beside it (U19) |
| Section header | 28 pt, 12 pt top gap (4 for the first), padding 8: chevron 12 `text-3` (rotates when collapsed), title 12/16 600 `text-2`, trailing: the 待办 badge, or a 22 pt `+` icon button (新建群 / 发起私聊) |
| Sections | **待办**, **群**, **私聊** (PRODUCT.md "待办, 群, 私聊"; a DM is a 群 of two but listed apart, U18). Each section collapses |
| 待办 row | Two lines, padding 6/8, radius 8: 20 pt glyph column (`circle-question-mark` or `eye` in the `you` glyph color, `at-sign` in accent), title 13/18 500, context 12/16 `text-2` (`qqbot 研发 · Watch 等你回答`). Max 5 rows, then "全部 n 项" |
| 群 row | 30 pt, radius 8, gap 8, padding 0/8: 20 pt group tile (radius 6, first character, Latin uppercased, palette by hash), name 13. Unread: name 600 `text-1`. Mention: red badge `n`. Muted: row at 60 % and a `bell-off` 13 |
| 私聊 row | Same; leading 20 pt avatar (person with presence, or the bot's character at its status). Unread DM: name 600 + red count badge |
| Selection | `sidebar-selected`, name 600. Only one row is ever selected (U19) |
| Footer | 52 pt, top hairline inset 8: 28 pt avatar with presence, name 13/500 and presence word 12 `text-2` (在线 / 离开 / 勿扰), a 28 pt settings icon button. The whole name area opens the account menu (我的资料, 设置, 退出登录) (U21) |
| Scroll edges | When the list scrolls, a 24 pt fade from `sidebar` at the top and bottom (a gradient overlay div; GPUI has no mask) |

### 6.3 Topic list (Feishu topics, Slack/OpenBot rows)

| Part | Value |
| --- | --- |
| Header | 52 pt, padding 0/12/0/16: 群 name 16/22 600 (truncates), then a 28 pt secondary button `square-pen` 14 + "新话题" (tooltip `新话题 ⌘N`). The member count lives in the topic view header, not here (U22) |
| Tabs | 34 pt text tabs, gap 20, 16 pt left: 全部 · 未读 n · 等我 n. Count in `text-3`, hidden when 0. Active tab: `text-1` 600 with a 2 pt `text-1` underline on the bottom hairline (Feishu) |
| Rows | Inset 8, radius 8, padding 10, 62 pt tall. Leading 32 pt avatar of the **responsible member** (a bot shows its live character, so the list shows who is working). Line 1: title 13/18 500 (600 when unread) + time 12 `text-3`. Line 2: preview 13/18 `text-2` as `作者：内容` (one line, Markdown stripped) + **one** trailing element by priority: `you` chip > `@n` red badge > other chip (卡住了, 完成, 等审, 等回答) > unread count 12/600 `accent-text`. 进行中 shows nothing (it is the default, U02) |
| Unread dot | 6 pt `unread-dot` at x 2, centered on line 1, inside the row padding (U24) |
| Separation | No hairlines between rows; spacing and hover/selection fills only (U24) |
| States | Hover `hover`; selected `selected`; pressed `pressed`; focus ring §7 |
| Times | §6.6 |

### 6.4 Topic view (Feishu topic pane order, Slack rows)

| Part | Value |
| --- | --- |
| Header | 52 pt, padding 0/12/0/24: title 16/22 600 (truncates) + its chip (name truncates at 96), then trailing: ghost button `users` 15 + member count (opens members in the side panel), then 28 pt icon buttons `bell` (follow), `panel-right` (side panel; its state is the icon, no fill, U16), `ellipsis`. No capsule, no shadow, no facepile |
| Topic start | When the content is shorter than the viewport, the list starts at the top with a start block (U01): 28 pt top padding, title 20/28 600 (clamp 2 lines), then one line: chip + `Joye 发起 · Taro 负责 · 今天 14:05` 12 `text-2`, 12 pt below. Long topics anchor to the bottom and show the start block when scrolled to the top |
| Rows | Padding 6/8 in a −8 margin so hover fills reach 8 pt past the text, radius 8. Gutter: 32 pt avatar, 10 gap. Meta line 20 pt: name 13/600, then for bots a role tag (§9.3), then time 12 `text-3`. Body 14/22. No 岗位 for people in the timeline (it is in the hover card, U12) |
| Grouping | Same author within 5 minutes with nothing between: no avatar or meta line, 1 pt top; the time shows in the gutter on hover (12 pt `text-3`) (Slack, U13) |
| Date divider | Centered pill on a full-width hairline: 22 pt, radius 11, 0.5 pt `border`, `content` fill, 12/500 `text-2`: 今天, 昨天, 11月2日 周日. 16 pt above, 8 below. Sticky at the top while its day scrolls. Click: jump to date (Slack) |
| New-messages divider | Red hairline with "新消息" 12/500 `danger-text` on the right |
| Jump pill | Only when the first unread message is above the viewport (U14): `n 条新消息 ↑` 28 pt pill, `overlay`, level-1 shadow, centered 12 pt under the header |
| Edited | `（已编辑）` 12 `text-3` after the last line of the body (U12) |
| Reactions | Under the body, gap 6: 24 pt pills, radius 12, `field`, emoji + names for up to 2 (`👍 Joye、Tata`), else emoji + count; mine: `accent-soft` + 1 pt `accent` ring + `accent-text` (Slack color, Feishu names) |
| Typing line | A fixed 20 pt slot above the composer: 12 `text-3` `Forge 正在输入…`, `A、B 和另外 3 人正在输入…` (U32) |
| Composer | §10 |

### 6.5 Side panel (GrokBot right pane)

Closed by default. Opens when the user clicks `panel-right` or the member count, and opens by
itself once per topic when a bot starts working in it or the first 产出 arrives (the user's
close is remembered per topic). Never shown with no topic open (U07).

| Part | Value |
| --- | --- |
| Header | 52 pt: `话题详情` 13/600, close `x` 28. Sub-pages (a 产出's versions, the process view, a member) replace the content with a `chevron-left` back button and a centered 13/600 title |
| Sections | 16 pt side padding, 20 pt between sections. Section header 24 pt: 12/600 `text-2` + count `text-3` |
| Live state | When a bot is responsible: the **live card** (`raised`, radius 12, 0.5 `border`, padding 12): 40 pt character at its status, name 13/600, state 12 `text-2` (`正在 staging 上验收`, `等你审发布预览`); then the plan list (Slack plan block): 24 pt rows, 14 pt glyph (`circle-check` success, current step glyph in accent, `circle-dashed` `text-3` for later), 12/16 text, current step `text-1` 500. A "查看过程" text button opens the process view |
| 产出 | 40 pt rows: 28 pt tile (radius 8, `accent-soft`, file-type glyph 15), name 13/500, `网页 · v3 · 今天 14:31` 12 `text-2`. Files and 产出 use the same row (U30). One quiet line when empty: `还没有产出` 12 `text-3` (one line for both, U07) |
| 成员 | 32 pt rows: 24 avatar, name 13, role tag at the trailing edge; then `user-plus` + `添加成员` text button |

### 6.6 Dates and times (one format everywhere, U23)

| Age | Lists, switcher, 待办 | Message meta |
| --- | --- | --- |
| Today | `14:32` | `14:32` |
| Yesterday | `昨天` | `14:32` (under the 昨天 divider) |
| Within 7 days | `周一` | `14:32` |
| This year | `10/28` | `14:32` |
| Older | `24/12/31` | `14:32` |

Date dividers spell the day: `今天`, `昨天`, `周一`, `10月28日 周二`, `2024年12月31日`.
Tooltips on any time show the full `2025年11月4日 23:59`.

## 7. States (every interactive element, U03 and U04)

One row helper and one icon-button helper in `ui/parts.rs` apply all of these. A view never
builds a clickable element without them.

| State | Rows (sidebar, topic, menu, popup, settings) | Buttons and icon buttons | Messages |
| --- | --- | --- | --- |
| Rest | No fill | Ghost: no fill, `text-2` icon; secondary: `content` + `border-strong` ring | No fill |
| Hover | `hover` (`sidebar-hover` in the sidebar), at once | `hover` fill, icon `text-1`; primary `accent-hover` | `hover` fill + hover toolbar |
| Pressed | `pressed` | `pressed`; primary `primary_active` | — |
| Selected | `selected` / `sidebar-selected`; popups and switcher `active-row` | Toggle buttons show state by icon, not fill | — |
| Focus (keyboard) | 2 pt `focus` ring inset 1 pt | 2 pt `focus` ring outside with a 2 pt gap | Ring on the focused row |
| Disabled | 40 % content | Primary disabled: `field` fill + `text-3` label; no pointer | — |
| Cursor | `cursor_pointer` on every clickable row, button, chip, link, tab, avatar | same | Text: I-beam |

Hover toolbar (Slack): a capsule at the row's top right, 14 pt above the row top, `overlay`,
level-1 shadow, radius 8, padding 2, four 28 × 26 icon buttons (radius 6): add reaction
(`face-slightly-smiling-plus`), reply (quote into the composer, `reply`), `text-quote` (copy as
quote), `ellipsis` (edit, delete, copy link, mark unread). It appears and hides at once.

Send states (U33): 发送中 = body at 60 % opacity and a 12 pt spinner after the last line, no
extra line. Failed = `circle-alert` 13 + `没有发出` 12 `danger-text` after the body, then a 24 pt
secondary button `重试`.

## 8. Motion

Curve **out** `cubic-bezier(0.23, 1, 0.32, 1)` for all enter and exit. Nothing over 240 ms
except loops (checked against review-animations STANDARDS.md: UI under 300 ms, ease-out, no
ease-in). Every row has a reduced-motion rule.

| What | Animation | Reduce motion |
| --- | --- | --- |
| Hover fills, hover toolbar | Instant (OpenBot `--duration-hover 0`, Slack) | same |
| Press | Pressed fill at once; release 120 ms | same |
| Menus, popovers, select | 150 ms opacity + 4 pt travel from the trigger | opacity only |
| @mention popup, switcher | None: appear and filter at once | same |
| Dialogs and sheets (with scrim) | 200 ms opacity + scale 0.98 → 1; exit 150 ms | opacity only |
| New message from others while visible | 200 ms opacity + 6 pt rise (GrokBot entry, shortened from 280 ms to fit our cap) | opacity only |
| My sent message | None | — |
| Bot pending row enter / exit | Enter 200 ms opacity + 6 pt rise; exit 160 ms opacity after a 400 ms hold | opacity only |
| Busy indicators (pending row dots, spinners, skeletons) | Wait **400 ms** before showing (OpenBot), so fast answers never flash | same |
| Pending dots | Three 4 pt dots, opacity 0.3 → 0.9, 1.2 s loop, 150 ms apart | static stepped opacities 0.9 / 0.6 / 0.3 |
| Streaming bot text | Text appears as it arrives; no per-word fade (paint cost at 10,000 messages); caret solid while text arrives | same |
| Character status change | Crossfade 160 ms between the two status SVGs | instant |
| Chip change | 160 ms crossfade | instant |
| Ask card after answer | Options collapse to the chosen line: height snaps, content fades 160 ms; the check scales 0.6 → 1 over 200 ms | no scale |
| Side panel | Width snaps; content fades 120 ms | instant |
| New sidebar row | 200 ms opacity + 6 pt from the left | opacity only |
| Never animated | Selection, keyboard navigation, switching 群 or topics, counts, tab switches, theme change, resize, composer growth | |

## 9. Members, bots and live work

### 9.1 Status in lists

One status per row, by priority (OpenBot): 等你… chip > @ badge > 卡住了 > 完成 / 等审 / 等回答 >
unread count. 进行中 is the default state of work and shows no chip in rows; the topic header
still shows it.

### 9.2 Bot replies (GrokBot, Slack agent sessions)

- **Acknowledge once**: the bot's first reply is one short line (`收到，先查告警来源。`).
- **Pending row**: while a bot works and has not posted, one row at the bottom of the timeline,
  aligned with message text: 24 pt character in its `working` status in the 32 pt gutter, then
  `**Scout** 正在 staging 上验收` 13 `text-2` (name `text-1` 600), three pulsing dots, the elapsed
  time 12 `text-3` tabular (`2:14`), `·`, and a 12 pt `查看过程` text button. The person who
  asked also sees a `停止` text button. The label comes from the bot's latest step, one phrase,
  never a log line. Several bots: one row each, at most 3, then `另外 n 位成员在工作`.
- **Steps row**: above a reply that used tools, one collapsed 24 pt row (OpenBot): `chevron-right`
  12 + `看了 6 个文件，运行 3 条命令` 12 `text-2` + `· 4 分 12 秒` `text-3`; failures add
  `1 次失败` in `danger-text`. Opening it shows the step list inline (1 pt left rule, 12/16 rows,
  max 220 pt then scroll); the full process view is in the side panel.
- **Streaming**: the reply streams in place in one message (Feishu). The topic row preview shows
  `生成中…` while it streams.
- **Lead with the result**: bot prompts ask for a bold lead-in (`**改好了**：…`, `**3 个例外**：…`)
  and two or three short paragraphs, with long content in a 产出. This is a prompt rule, not UI.

### 9.3 Bot identity

- Avatar: the jojo-friends character (`apps/desktop/src/characters/`) at its live status
  (offline, working, needs-input, error, success, idle; rules in docs/ROSTER.md). The character
  shape is the bot mark; there is no extra graphite badge (v1) and no presence dot.
- Role tag after a bot's name in messages, the side panel and settings: 18 pt, radius 4,
  `field`, `bot` glyph 11 + 岗位 11/14 500 `text-2`. People show 岗位 only in hover cards,
  settings and the side panel (U12, U47).
- Characters appear only as avatars, in the pending row, in the live card and in empty states,
  never as decoration (GrokBot avoids it too).

## 10. Composer (GrokBot / Feishu single box)

| Part | Value |
| --- | --- |
| Box | 24 pt side margin (aligned with message text), 16 pt bottom, radius 12, 0.5 pt `border-strong`, `content` fill, padding 9/8/8/12. Focus: 1 pt `border-strong` + a 3 pt `field` soft ring; no blue |
| Row | `+` icon button (attach file, image, 产出 from disk; quote is in the message toolbar) · the field (14/22, 1 to 8 lines, then scrolls inside; the box grows upward) · `at-sign` and `face-slightly-smiling` icon buttons · send |
| Placeholder | Names the destination (Feishu): `回复「夜间告警降噪：合并重复的 5xx」`; in a 群 with no topic open `在「qqbot 研发」发起新话题，第一行是标题`. One line, ellipsis. It is the only instruction; no extra label or empty-state sentence (U08) |
| Send | 28 pt circle: empty = `field` + `text-3` arrow (disabled); text = `accent` + white arrow (U09) |
| Hints | No permanent key hints. `↵ 发送 · ⇧↵ 换行` appears only in the `+` menu footer and in the tooltip of send |
| @mention popup | Above the caret, 260 pt wide (190–280), radius 12, `overlay`, overlay shadow, padding 4. Section label 12/500 `text-3` (成员), rows 32 pt radius 6: 20 pt avatar, name 13, 岗位 13 `text-3`; active row `active-row` with a trailing `corner-down-left` 13. ↑↓ moves, ↵ or Tab picks, Esc closes |
| Mentions in text | Others: `mention-bg` + `accent-text` 500, radius 4, padding 0/3. Me: `me-bg` + `me-text` |
| Blocking states | Offline, read-only, quota: a tinted slab attached to the top of the box (radius 12, tucked 12 pt under it), 12/16 text and one action. No toast |
| Topic reply option | In a reply whose result matters to everyone: a `同时发到群` checkbox in the `+` menu (Feishu) |

## 11. Other screens

### 11.1 Cards (Feishu card spec + GrokBot + Block Kit)

All cards: in the message text column, width `min(520, column)` (one width for cards, files and
images, U29), `raised` fill, 0.5 pt `border`, radius 12, padding 12/14. Title 14/20 600 + an
optional trailing status chip; one sentence 13/18 `text-2`; then blocks with 4/8/12/16 spacing
only; text 12, 13 or 14 only. Fields: label 12 `text-3`, value 13 `text-1` (U28).

| Card | Content |
| --- | --- |
| Work card | Title, chip (进行中 info / 完成 success / 卡住了 danger / 等你… you), one sentence, optional image (radius 8) or plan list, actions |
| Ask card (等你回答) | Question as title + `等你回答` chip; promise line (`你选之前我不会改任何环境。`); options as 36 pt rows (radius 8, `content`, 0.5 `border`, gap 6) with a kbd letter A, B, C; hover: 1 pt `accent` ring + `accent-soft`; keys A–C work while the card has focus; optional free-text row `其他…`. After the answer: the card shrinks to `✓ 你选了：现在在 staging 打开` and the topic's `waitingOn` clears |
| Draft card (before anything irreversible) | Title `发布预览` + `等你审`; the real object as a table (`content`, radius 8, rows 7/10 padding with 0.5 `sep` between, key 40 pt `text-3`), body preview clamped to 3 lines; actions primary `发布`, secondary `修改`, ghost `放弃`. Taro's posts and Watch's production steps use it |
| Alert card (integrations) | Title + tone chip, fields, actions. A resolved alert uses the success chip `已处理` |
| Actions | 28 pt buttons, radius 8, gap 8, at most 3 then `ellipsis`; exactly one primary; same style in both themes (U28) |

### 11.2 Files and images

File row: 36 pt tile (radius 8, `accent-soft`, glyph by type 18), name 13/500 (middle ellipsis
keeps the extension), `网页产出 · v2 · 18 KB` 12 `text-2`, trailing 28 pt icon buttons shown on
hover (open, show in Finder, download). Downloading: `正在下载 64%` in the meta line and a 2 pt
progress bar on the bottom edge; cancel `x`. Images: max 360 × 320 pt, radius 8, inner 0.5
`border`; while loading a `field` skeleton of the final size with `file-image` and the name (U30).

### 11.3 Empty and error states

| Case | Treatment |
| --- | --- |
| No 群 at all | One centered welcome in the content area (character `jojo` 64, `display` title `还没有群`, 13 `text-2` sentence, buttons `新建群` primary and `发起私聊` secondary). Topic list, composer and side panel hidden (U06) |
| 群 with no topics | Topic list: one 12 `text-3` line `还没有话题` under the tabs; topic view: the composer with the new-topic placeholder |
| No topic selected | Topic view: centered 13 `text-2` `选一个话题，或写下第一行开始新话题` |
| Empty filter | `没有未读话题` / `没有等你的话题`, 12 `text-3`, top of the list |
| Search, no result | `没有找到「词」` + `在所有群里搜索` text button |
| Load error | Inline row at the top of the timeline: `circle-alert` danger + `没能加载更早的消息。` + 24 pt `重试` |
| Field error | Red 1 pt border on the field + 12 `danger-text` directly under it; submit stays disabled (U40) |

### 11.4 Quick switcher and search (Feishu ⌘K, Slack grouped results)

Overlay with `scrim`, panel 640 pt wide, 96 pt from the window top, radius 12, `overlay`,
overlay shadow. Query row 52 pt: `search` 18, 16 pt text, `esc` kbd. Results grouped (话题, 群,
私聊, 成员, 消息), group label 12/500 `text-3`, rows 40 pt (52 for message hits with a snippet
line), radius 8: 24 pt group tile or avatar, title 13/500 + ` · 群名` `text-3`, trailing chip.
Snippets are plain text with Markdown stripped and the hit in `you-bg` (U35). With no query:
最近 (5 topics), then 群 and 私聊 (U34). The panel height follows the content up to 70 % of the
window, whole rows only. Footer 36 pt: `↑↓ 选择`, `↵ 打开`, `⌘F 在当前群里搜索`. ⌘F opens the
same panel scoped to the 群 with a removable `在 qqbot 研发` token in the query row.

### 11.5 Overlays: one component (U05, U36 to U38)

Switcher, dialogs, sheets, alerts and pickers (new 群, start DM) share one overlay: `scrim`,
`overlay` surface (one step lighter than `content` in dark), radius 12, overlay shadow, 96 pt from
the top for command-style panels and centered for dialogs and alerts. Dialog: title `title`
16/600 with 16 pt below; body 13; footer right-aligned 28 pt buttons (secondary then primary);
no close `x` when a cancel button exists. Primary is disabled until the form is valid. Checkbox
lists: 16 pt checkbox at the leading edge, 40 pt rows. Alerts: centered, 320 pt wide, title
`title` 16/600, buttons 28 pt.

### 11.6 Login

440 × 520, `content`. App icon 80 pt (`icon-dark.svg` in dark mode, U39), 72 pt from the top;
`欢迎加入团队` `display`; `输入管理员发给你的邀请码` 13 `text-2`; the invite code as 8 boxes in
groups of 3-3-2 (32 × 36, radius 8, `border-strong`, 16/500 Menlo; focused box: 1 pt accent +
3 pt `accent-soft` ring); `加入` 36 pt primary, 296 wide, disabled until complete. The server
address is behind a `更改服务器` disclosure at the bottom (12 `text-2`), developers only.

### 11.7 Settings (Slack preferences)

900 × 640. Left list 200 pt on `sidebar`: 32 pt rows, 16 pt monochrome icons, selected
`sidebar-selected` + 600. Content on `content`, 28 pt side padding: page title `display` at 16 pt
from the top, primary action at its right. Grouped tables: radius 12, 0.5 `border`; header row
34 pt `raised` 12/500 `text-2` **inside** the card (U41); rows 56 pt with 0.5 `sep`.

- **成员**: columns member (32 avatar, name 13/500 + `（你）` `text-3`, 岗位 12 `text-2` with the
  `bot` glyph for bots), status (7 pt dot: `online` or hollow grey, + word: 在线, 离线, 工作中,
  等人回答), 权限 (`所有者` / `成员` select, 26 pt; plain text for yourself), trailing 28 pt
  `ellipsis` or an empty slot of the same width. Names are trimmed and inner whitespace collapsed
  (U41). One primary button `添加成员 ▾` with two menu items: `邀请同事` and `添加 AI 成员`
  (wording to confirm with Joye).
- **我的资料**: avatar with `更换头像` and its help text `PNG、JPEG 或 WebP，不超过 1 MB` under the
  button; name and 岗位 fields save on blur; sign-out at the bottom; `退出并删除本机记录` in its
  own danger row (U40).
- **通知**: per 群 one segmented control `所有消息 / 仅 @我 / 不提醒` whose selected segment is a
  raised `content` fill with a level-1 shadow (macOS); no separate 静音 toggle; 群 and 私聊 in
  separate groups; same avatars as the sidebar (U42).
- **机器**: rows with an icon by machine type, Chinese tags, status centered vertically (U43).
- **成本**: hidden until it exists (U43). **关于**: no repeated labels.

### 11.8 Banners

App-level banners (update ready, offline) sit in the sidebar footer area as a 44 pt card above
the user row, never inside the topic column, so header lines stay aligned (U17). Buttons 24 pt.

## 12. Elevation

| Level | Shadow L | Shadow D | Used by |
| --- | --- | --- | --- |
| 0 | none | none | Everything in the window |
| 1 float | `0 0 0 0.5px #1F23291F, 0 2px 8px #0F111414` | `0 0 0 0.5px #FFFFFF1F, 0 2px 8px #00000059` | Hover toolbar, jump pill, selected segment |
| 2 overlay | `0 0 0 0.5px #1F232924, 0 8px 28px -6px #0F111438, 0 2px 6px #0F11140F` | `0 0 0 0.5px #FFFFFF24, 0 12px 36px -6px #00000099, 0 2px 6px #0000004D` | Popovers, menus, mention popup, switcher, dialogs, toasts |

## 13. Worst-case data rules (break-ui CATALOG)

Every text slot has one rule. Checked in [worst-case-light.png](design/mockups-v2/worst-case-light.png)
at the 1100 × 680 minimum window.

| Slot | Rule |
| --- | --- |
| Names, titles, file names in rows, chips | One line, `…` at the end, full text in a tooltip; file names middle-ellipsis keeping the extension |
| Topic start title | Two lines, then `…` |
| Previews, 待办 context, typing line, placeholder | One line, `…` |
| Counts | Badges `99+`; topic unread `999+`; tabs `999+` |
| Message body | Wraps anywhere a word does not fit (URLs break); code scrolls horizontally |
| Missing title | `（无标题）` in `text-2` (full-width parentheses) |
| Emoji-first names | Monogram skips the emoji; the name keeps it |
| Long reactions | `✅ Joye、李雪 和另外 12 人` |
| Times | §6.6 |

## 14. Mockups

| File | Shows |
| --- | --- |
| `main-light.png`, `main-dark.png` | Sidebar with 待办, 群, 私聊; topic list; an open long topic with people and bots, hover toolbar, steps row, file, ask card, Scout's pending row, composer with the @mention popup |
| `short-topic-light.png`, `-dark` | A short topic that starts at the top (no void), the draft card, the side panel with Taro's live card, 产出 and 成员 |
| `worst-case-light.png`, `-dark` | 1100 × 680, long names and titles, 99+ and 999 counts, emoji names, a huge file downloading, a failed send, a long typing line |
| `switcher-light.png`, `-dark` | ⌘K over a scrim with grouped results and a message hit |
| `settings-members-light.png`, `-dark` | Settings window, 成员 page |
| `login-light.png`, `-dark` | Invite-code login, dark icon variant |

## 15. UI basics checklist

Before merging UI work, check each line in both themes, at 1100 × 680 and 1920 × 1080, and with
`--worst-case`. The audit item it covers is in brackets.

- [ ] Short content starts at the top with the topic start block; long content anchors at the
      bottom. No empty area above content. [U01]
- [ ] Topic rows: two lines, preview `作者：内容`, one trailing element, no 进行中 chip. [U02]
- [ ] Every clickable element has hover, pressed, focus ring and a pointer cursor (§7). [U03, U04]
- [ ] Every overlay uses the one overlay component with a scrim and `overlay` surface. [U05, U36, U37, U38]
- [ ] Never more than one empty state on a screen; no 群 → one welcome. [U06]
- [ ] Side panel closed by default, opens on bot work or 产出, never empty. [U07]
- [ ] One instruction per place; placeholders name the destination; no permanent key hints;
      send is accent only with text. [U08, U09]
- [ ] Markdown fences after text on the same line parse; never an empty code block. [U10]
- [ ] Header hairline appears only while content scrolls under it. [U11]
- [ ] Meta line = name 600 + (bot role tag) + time `text-3`; `（已编辑）` after the body. [U12]
- [ ] Same-author grouping within 5 minutes; list items 4 pt apart. [U13]
- [ ] Jump pill only when the first unread is above the viewport. [U14]
- [ ] Message hover is a soft fill in both themes plus the hover toolbar. [U15]
- [ ] Header icon buttons are plain 28 pt; toggle state shown by icon; no shadows; `：` with no
      space. [U16]
- [ ] App banners never push the topic header; all header lines meet at 52 pt. [U17]
- [ ] 群 and 私聊 are separate sections; one avatar rule; one initial for CJK. [U18, U37]
- [ ] Sidebar search opens ⌘K; nothing beside it; one selection at a time. [U19]
- [ ] Number badges only for mentions, DMs and 待办, red, `99+`; unread is weight. [U20]
- [ ] Footer shows name + presence and opens the account menu. [U21]
- [ ] Topic list header is 52 pt with text tabs below; counts `text-3`, hidden at 0. [U22]
- [ ] One date format (§6.6) in every list, header and switcher. [U23]
- [ ] No hairlines between topic rows; unread dot inside the row. [U24]
- [ ] One chip component, five tones, 已处理 and 完成 are success. [U25]
- [ ] Every truncation uses `…` (U+2026) from one helper. [U26]
- [ ] Only the six type sizes, the spacing and the radius scales of §4 and §5. [U27, U48]
- [ ] Cards: title 14/600, body 13–14, labels 12; 28 pt buttons, one primary, same in both
      themes; one width for cards, files and images. [U28, U29]
- [ ] File rows: skeleton with name while loading; actions on hover; 产出 and files share a
      row style. [U30]
- [ ] Process view: steps with icons, output in mono 12 on `code-bg`, one state word shared
      with the header, no repeated summary; opens as a side-panel sub-page. [U31]
- [ ] Typing line in its own 20 pt slot, `…`. [U32]
- [ ] Sending dims the body; failure shows inline `没有发出` + 24 pt `重试`. [U33]
- [ ] Switcher: group tile per row, 16 pt query, grouped sections, whole rows, recent items
      with no query, chips show only the state word. [U34]
- [ ] Search panel height follows content; snippets without Markdown; no `#` prefix. [U35]
- [ ] Login: server field behind a disclosure; equal 36 pt heights; dark icon. [U39]
- [ ] Settings: help text under its control, inline field errors, danger action in its own
      row, column headers inside the card, offline dot, trimmed names, segmented controls with a
      raised selected segment, no 成本 page. [U40, U41, U42, U43]
- [ ] Columns resize and collapse; the topic view never clips; composer and messages share one
      edge and max width. [U44, U45]
- [ ] Dark: sidebar, content, raised and overlay are distinct steps; selection stronger than
      hover. [U46]
- [ ] Copy: full-width CJK punctuation, PRODUCT.md words only, bots marked by the role tag. [U47]
- [ ] Motion per §8, with reduce motion checked. Run review-animations, break-ui, apple-design
      and emil-design-eng (docs/design/skills/emilkowalski).

## 16. Open items

- Bot identity, confirmed by Joye on 2026-11-03: the character is the mark, plus the role tag
  with a `bot` glyph. No other bot mark. PRODUCT.md says the same.
- Wording `添加 AI 成员` for adding a bot (PRODUCT.md has no word for it yet).
- The 280 ms GrokBot entry and 450 ms sidebar-row animations were cut to 200 ms to keep the
  240 ms cap; check the feel in the app.
- Feishu and Slack dark modes were not seen; the dark ramp is ours and needs a check in the app
  on a real display.
- GPUI: the sidebar fade masks, the sticky date divider and container-width rules (hide the time
  when a row is narrow) need custom elements; see GPUI-CAPABILITIES.md before building.
