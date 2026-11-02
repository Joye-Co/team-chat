# 精密工具 Precise pro

A design direction for the desktop app (Rust, GPUI `gpui-pre 0.3.10`, `gpui-component 0.8.1`,
`gpui-kit 0.8.1`). Mockups are in this folder; PNGs are in [`png/`](png/).

## The idea

The app is a work tool used 8 hours a day, so it looks like a precision instrument, not a
messenger. Neutral graphite grays carry the structure. One cobalt accent marks what you can act
on and what asks for you. Depth comes from soft rings and shadows, not borders. Type does the
hierarchy. Every status is a small exact glyph plus a color, and only work that waits on **you**
gets a filled chip. Everything is reachable by keyboard and the interface shows the shortcut.
Nothing animates that you trigger 100 times a day.

Spirit: Linear's inset content panel and status glyphs, Raycast's instant palette, Zed's
density, Apple's type and spacing discipline.

## Principles

1. **Color means "act" or "state", never decoration.** Cobalt = interactive, selected-in-focus,
   mentions of you, primary action. Status colors appear only on status glyphs and chips. All
   other surfaces are neutral.
2. **"Waiting on you" is the loudest thing on screen.** It is the only filled chip (等你回答,
   等你审). Waiting on someone else uses the same glyph and hue, tinted, so the eye finds your
   work first.
3. **Rings and shadows, not borders.** A 1 px ring is drawn as a shadow (`0 0 0 1px`). Hard
   borders appear only on text inputs (where macOS users expect a field edge) and as hairline
   separators between columns.
4. **One level of containers.** Window base, one inset content panel, cards in the timeline.
   Never a card inside a card; fields inside cards are plain rows.
5. **Dense yet airy.** 28 px sidebar rows and 13 px UI text, but 14/22 message text and 16 px
   gaps between message groups. Density in navigation, air in reading.
6. **Keyboard first.** Shortcut hints (`Kbd`) on the search field, in menus, in the palette,
   on primary buttons. Keyboard actions never animate.
7. **Chinese first.** PingFang SC carries the text; SF Pro carries Latin and digits; digits are
   tabular wherever they count or tell time. Initials use the given name (周子航 → 子航).

---

## Color tokens

Values are exact. "ring" means a `0 0 0 1px` box shadow, not a border.

### Surfaces

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `window` | `#F3F3F4` | `#111113` | Window base; the sidebar sits directly on it |
| `sidebar` | `#F3F3F4` | `#111113` | Sidebar (opaque, no vibrancy) |
| `content` | `#FFFFFF` | `#18181B` | Inset content panel: topic list, timeline, side panel |
| `elevated` | `#FFFFFF` | `#202024` | Cards in the timeline, composer in dark mode |
| `popover` | `#FFFFFF` | `#232328` | Menus, @mention popup, palette, dialogs, tooltips |
| `sunken` | `#F6F6F7` | `#131315` | Code blocks, invite code box, palette footer |
| `scrim` | `rgba(17,17,20,0.22)` | `rgba(0,0,0,0.42)` | Behind the Cmd+K palette |
| `sheet-scrim` | `rgba(17,17,20,0.14)` | `rgba(0,0,0,0.40)` | Inside a window behind a dialog |

### Text

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `text-1` | `#18181B` | `#EDEDEF` | Primary: titles, names, message body |
| `text-2` | `#55555D` | `#A5A5AD` | Secondary: read rows, quotes, labels |
| `text-3` | `#8B8B93` | `#74747C` | Tertiary: timestamps, meta, placeholders, section headers |
| `text-4` | `#B3B3BA` | `#4F4F56` | Quaternary: disabled, grouped-message gutter time |
| `text-on-accent` | `#FFFFFF` | `#FFFFFF` | Text on accent and on filled chips |

Contrast: `text-1` on `content` is 17:1 (light) and 15:1 (dark); `text-3` is 3.4:1 and 3.8:1,
used only for meta text of 11 to 12 px that is never the only carrier of meaning.

### Lines and fills

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `separator` | `rgba(0,0,0,0.07)` | `rgba(255,255,255,0.065)` | Hairlines between columns and sections, card footers |
| `separator-strong` | `rgba(0,0,0,0.12)` | `rgba(255,255,255,0.11)` | Input edge, quote bar, step line |
| `hover` | `rgba(0,0,0,0.035)` | `rgba(255,255,255,0.04)` | Row and message hover |
| `pressed` | `rgba(0,0,0,0.07)` | `rgba(255,255,255,0.08)` | Row pressed |
| `selected` | `rgba(0,0,0,0.06)` | `rgba(255,255,255,0.07)` | Selected row, active filter, toggled icon button |
| `fill-quiet` | `rgba(0,0,0,0.05)` | `rgba(255,255,255,0.06)` | Search field, unread badge, select, tiles |

### Accent (cobalt)

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `accent` | `#3656E0` (white text 5.9:1) | `#5272FF` (white text 4.0:1, used at 13 px 500 and above) | Primary button, send, mention badge, caret, progress |
| `accent-hover` | `#2C4BD0` | `#6380FF` | Primary hover |
| `accent-pressed` | `#2541B8` | `#4562E8` | Primary pressed, primary button ring |
| `accent-text` | `#2F4FD6` | `#8EA2FF` | Accent text: mentions, highlights, new-messages label |
| `accent-tint` | `rgba(54,86,224,0.09)` | `rgba(82,114,255,0.14)` | Keyboard-selected menu row, mention pill, file tile |
| `accent-tint-strong` | `rgba(54,86,224,0.16)` | `rgba(82,114,255,0.24)` | Mention of me |
| `ring` (focus) | `rgba(54,86,224,0.42)` | `rgba(102,132,255,0.55)` | 3 px focus halo outside a 1 px `accent` edge |
| `text-selection` | `rgba(54,86,224,0.22)` | `rgba(82,114,255,0.34)` | Selected text |

### Topic status

Glyph color / chip text color. The chip background is the glyph color at 11 to 12 % over the
surface. When the viewer is in `waitingOn`, the chip is filled with the glyph color and white
text (dark mode uses the deeper fill in the last column so white text stays above 4.5:1).

| Status | Glyph | Light glyph / text | Dark glyph / text | "You" fill (dark) |
| --- | --- | --- | --- | --- |
| `open` | dashed ring | `#9A9AA2` / no chip | `#6E6E76` / no chip | |
| `in_progress` 进行中 | ring + half disc | `#C7860A` / `#9A6500` | `#E0A33A` / `#E8B45A` | |
| `waiting_answer` 等你回答, 等{name}回答 | ring + dot | `#3656E0` / `#2F4FD6` | `#6F8BFF` / `#93A7FF` | `#4766F0` |
| `waiting_review` 等你审, 等{name}审 | ring + three-quarter disc | `#7A55D1` / `#6643BD` | `#A88BF0` / `#B9A1F5` | `#7556D6` |
| `blocked` 卡住了 | filled disc, bar cut out | `#D93A3A` / `#C02F2F` | `#F06464` / `#F58080` | |
| `done` 完成 | filled disc, check cut out | `#2A9A58` / `#20804A` | `#45C07A` / `#5FCB8D` | |

The glyph family reads as one scale of "how far along": empty (dashed) → half → waiting
(dot / three quarters) → stopped (bar) → finished (check). Waiting-on-answer shares the accent
hue on purpose: it is the status most often addressed to you.

### Signals, badges, presence, bot mark

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| unread badge | bg `fill-quiet`, text `text-2` | same | Unread count (neutral, never red) |
| mention badge | bg `accent`, text `#FFFFFF` | same | `@2`, `@99+` |
| `online` | `#2FA564` | `#3FBF75` | Presence dot, 8 px, 2 px ring in the surface color |
| `offline-ring` | `#B3B3BA` | `#5A5A62` | Offline: hollow ring 1.5 px |
| `bot-mark-bg` / `fg` | `#18181B` / `#FFFFFF` | `#EDEDEF` / `#18181B` | 14 px rounded square (radius 4) with Lucide `bot` 9 px, bottom-right of the avatar, 2 px ring in the surface color |
| `danger` | `#D93A3A` | `#F06464` | Errors, failed send, sign out, destructive actions |
| `danger-tint` | `rgba(217,58,58,0.09)` | `rgba(240,100,100,0.13)` | Error field background |
| `warning` | `#C7860A` | `#E0A33A` | Invite pending, one-time notice (tint 10 %) |

### Avatars (initials)

| Hue | Light bg / text | Dark bg / text |
| --- | --- | --- |
| 1 cobalt | `#E6ECFF` / `#2F4FD6` | `#232C52` / `#9FB1FF` |
| 2 amber | `#FDEBD9` / `#A35400` | `#3B2A18` / `#F0B878` |
| 3 green | `#DFF3E7` / `#1F7A45` | `#1A3527` / `#79D6A0` |
| 4 violet | `#F2E6FB` / `#7643B8` | `#33244A` / `#C6A8F5` |
| 5 red | `#FDE4E4` / `#B42F2F` | `#3E2222` / `#F59A9A` |
| 6 teal | `#DDF1F4` / `#11727F` | `#173438` / `#7FD3DE` |
| bot | `#ECECEF` / `#3A3A41` | `#2C2C32` / `#D5D5DA` |

Hue = stable hash of the member id mod 6. Bots use the neutral pair plus the bot mark; their
avatar shape is the same circle as people (equal members).

Initials: Chinese names use the given name, the last two characters (周子航 → 子航, 陈默 →
陈默); Latin names use first + last word initial (Aleksandra Wiśniewska-Kowalczyk → AW); skip
leading emoji and symbols (🦊 狐狸助手 → 狐狸); nothing left → Lucide `user` or `bot`. Below
24 px, one grapheme only: the last character of a Chinese name (子航 → 航), the first letter of
a Latin name (AW → A).

### Code blocks

| Token | Light | Dark |
| --- | --- | --- |
| `code-bg` | `#F6F6F7` + inner ring `separator` | `#131315` + inner ring `separator` |
| `code-text` | `#2B2B31` | `#D9D9DE` |
| keyword | `#B13A6A` | `#F08BB4` |
| function | `#2F4FD6` | `#8EA2FF` |
| type | `#8A5A00` | `#E8B45A` |
| string | `#20804A` | `#6FD29A` |
| comment | `#8B8B93` | `#6E6E76` |

### Cards

Status card, 产出 card and image frames sit on `elevated` with elevation `e2`, radius 10. Card
footer actions are separated by an inset `separator` line, not a border.

---

## Typography

Families: `.SystemUIFont` (SF Pro, optical sizes chosen by Core Text) with fallback
`PingFang SC`, then `Hiragino Sans GB`. Mono: `SF Mono`, fallback `Menlo`. In GPUI set
`Theme.font_family = ".SystemUIFont"`, `TextStyle.font_fallbacks = ["PingFang SC"]`,
`Theme.mono_font_family = "SF Mono"`.

PingFang SC has Regular 400, Medium 500 and Semibold 600 only; this scale uses exactly those.
Chinese tracking is 0 everywhere. Latin tracking comes from SF's own size-specific tracking
table (Core Text applies it for the system font); display sizes add -1 to -1.5 % where the
renderer allows (see GPUI limits).

| Role | Size / line height | Weight | Use |
| --- | --- | --- | --- |
| `display` | 22 / 28 | 600 | Login title |
| `title-1` | 20 / 26 | 600 | Settings page title |
| `title-2` | 15 / 20 | 600 | Column headers: group name, topic title, dialog title |
| `body` | 14 / 22 | 400 | Message text, composer (Chinese needs ~1.57 leading) |
| `ui` | 13 / 18 | 400; 500 for names and row titles; 600 for unread rows and card titles | Sidebar rows, topic titles, menus, buttons (500) |
| `small` | 12 / 16 | 400 / 500 | Previews, labels, chips (500), roles, field labels |
| `caption` | 11 / 14 | 500 | Timestamps, counts, badges (600), kbd, section counts |
| `mono` | 12 / 19 | 400 | Code blocks, inline code (12 / 18) |
| `mono-display` | 26 / 32 | 600, letter-spacing 0.06 em | One-time invite code |

Tabular numbers (`tnum` via `FontFeatures`): every timestamp, unread and mention badge,
member and topic count, version number, progress (41 / 58), durations and file sizes.

Line length: message paragraphs cap at 600 px (about 42 Chinese characters at 14 px).

## Spacing and layout

Scale (px): `2, 4, 6, 8, 10, 12, 16, 20, 24, 32`. Most UI uses 4, 8, 12, 16.

| Metric | Value |
| --- | --- |
| Main window default / minimum | 1440 × 900 / 1040 × 640 |
| Title area | 52 px, traffic lights at (18, 18), transparent title bar |
| Sidebar width | 232 px (resizable 200 to 300), padding 52 / 10 / 10 |
| Inset content panel | margin 8 px top, right, bottom; radius 12; `e1` |
| Topic list width | 304 px (resizable 260 to 400) |
| Side panel width | 288 px, toggled with ⌘I; auto-hidden below 1280 px window width |
| Column header height | 52 px, title-2, actions as 28 px icon buttons |
| Sidebar row | 28 px (待办 rows 30 px), radius 6, padding 0 6 0 8, icon 16, gap 8 |
| Sidebar section header | 26 px, `small` 500 `text-3`, 10 px above |
| Topic row | 2 lines, padding 9 10 10, gap 5, radius 8, 1 px gap between rows; about 62 px |
| Filter bar | 24 px segments, radius 6, 2 px gaps |
| Message | avatar 32, gap 12, padding 6 20; name row 20 px; body 14 / 22 |
| Grouping | same sender within 5 minutes: no avatar or name, 1 px top padding, time shown in the 32 px gutter on hover |
| Group gap | 12 px between groups (6 + 6 padding); day divider 14 px above |
| Bot acknowledgement | one line, 20 px avatar, indented to the text column, `ui` in `text-2` |
| Composer | margin 0 16 16, radius 10, field padding 11 14 4, toolbar 28 px icon buttons |
| Cards | padding 12 14, max width 480 (产出 card 420) |
| Popover row | 32 px, padding 0 8, radius 6, container padding 4 |
| Palette | 640 px wide, 132 px from the window top, query row 52 px, result rows 40 px |
| Settings window | 1120 × 780 default, nav 208 px, table rows 52 px |
| Dialog | 432 px wide, padding 20, radius 14 |

## Radii

| Token | Value | Use |
| --- | --- | --- |
| `r-xs` | 4 | Kbd, inline code, mention pill, bot mark |
| `r-sm` | 6 | Rows, chips, small buttons, select, search field |
| `r-md` | 8 | Topic rows, code blocks, images, palette rows |
| `r-lg` | 10 | Cards, composer, popovers |
| `r-xl` | 14 | Palette, dialogs |
| panel | 12 | Inset content panel |
| window | system (macOS 26) | Window corners |

Chips are 6 px rounded rectangles, never pills: they read as precise labels, not tags.

## Elevation

| Level | Light | Dark | Used for |
| --- | --- | --- | --- |
| `e0` | none | none | Sidebar, rows, timeline |
| `e1` | ring `rgba(0,0,0,.055)`, `0 1 2 rgba(0,0,0,.04)`, `0 4 12 -6 rgba(0,0,0,.05)` | ring `rgba(255,255,255,.055)`, `0 1 2 rgba(0,0,0,.5)`, `0 6 16 -6 rgba(0,0,0,.5)` | Inset content panel |
| `e2` | ring `rgba(0,0,0,.06)`, `0 1 2 rgba(0,0,0,.05)`, `0 2 6 -2 rgba(0,0,0,.05)` | ring `rgba(255,255,255,.06)`, inset top `rgba(255,255,255,.035)`, `0 1 3 rgba(0,0,0,.45)` | Cards, composer, secondary buttons |
| `e3` | ring `rgba(0,0,0,.075)`, `0 2 6 rgba(0,0,0,.06)`, `0 12 28 -6 rgba(0,0,0,.16)` | ring `rgba(255,255,255,.08)`, inset top `rgba(255,255,255,.05)`, `0 14 32 -6 rgba(0,0,0,.6)` | Menus, @mention popup, hover toolbar, tooltips |
| `e4` | ring `rgba(0,0,0,.08)`, `0 8 18 -8 rgba(0,0,0,.14)`, `0 28 64 -12 rgba(0,0,0,.28)` | ring `rgba(255,255,255,.09)`, inset top `rgba(255,255,255,.06)`, `0 30 70 -10 rgba(0,0,0,.75)` | Palette, dialogs |

In dark mode shadows are nearly invisible, so the ring is white-alpha and a 1 px inner top
highlight gives the edge. Surfaces get lighter as they rise (`content` 18 → `elevated` 20 →
`popover` 23), the way macOS dark mode does it.

**Borders are used only** on text inputs (`separator-strong`, inset), the column hairlines
(1 px `separator` drawn as a shadow on the column) and the quote bar. Everything else is a
ring, a fill or space.

## Materials

No vibrancy. The sidebar is an opaque `#F3F3F4` / `#111113`. Reasons: a work tool on screen all
day must read the same over any wallpaper; text on a blurred desktop drops contrast; GPUI can
blur only the whole window (`WindowBackgroundAppearance::Blurred`), not one region, so the
content panel would have to repaint an opaque layer over the blur, and the app would pay for a
blurred window it mostly covers. The inset panel gives the same "sidebar is chrome, panel is
content" layering that vibrancy gives in Apple's apps.

## Iconography

Lucide 1.43 via `gpui_kit::assets` / `IconName`, drawn with `Icon` (monochrome, `text_color`).
Sizes: 16 px in rows, headers and toolbars (stroke 1.75 at 24-unit artboard ≈ 1.17 px); 14 px
inside buttons, chips' neighbors and meta; 18 px in file tiles and the palette query; 12 px
in selects. One stroke weight per size; never mix filled and outline icons in one row.

Icons used: `search`, `hash`, `at-sign`, `plus`, `square-pen` (new topic), `bell-off` (muted
群), `settings`, `panel-right` (details), `ellipsis`, `x`, `paperclip`, `quote`, `arrow-up`
(send), `copy`, `file-text`, `file`, `image`, `check`, `bot` (bot mark only), `circle-user`,
`users`, `cpu` (机器), `bell`, `chart-pie` (成本), `activity` (诊断), `log-out`, `user-plus`,
`mail`, `triangle-alert`, `circle-alert`, `circle-check`, `globe`, `key-round`, `ban`,
`chevron-down`. Only the ones outside gpui-kit's default 101 need an app `AssetSource`.

Status glyphs are **not** Lucide: six custom 14 px SVGs (`st-open` … `st-done`) shipped as app
assets. Because GPUI renders an SVG as a one-color mask, the check and bar are cut out of the
disc with `fill-rule="evenodd"` paths, so the glyph works on any background and in filled chips
(where it turns white).

---

## Motion

Personality: crisp and fast; nothing bounces except what you throw. Durations map onto
gpui-component's `MotionTokens`, which this direction sets to:

| Token | Value |
| --- | --- |
| `duration_fast` | 120 ms |
| `duration_normal` | 180 ms |
| `duration_slow` | 240 ms |
| `easing_enter` | `cubic-bezier(0.23, 1, 0.32, 1)` (strong ease-out) |
| `easing_exit` | `cubic-bezier(0.23, 1, 0.32, 1)` at 0.7 × duration (exit is faster than enter; never ease-in) |
| `easing_move` | `cubic-bezier(0.77, 0, 0.175, 1)` (strong ease-in-out) |
| `spring_control` | critically damped, response 0.18 s |
| `spring_move` | damping 1.0, response 0.3 s; 0.8 only after a flick |

| Transition | Motion |
| --- | --- |
| Popover, menu, @mention popup open (mouse) | opacity 0 → 1 and scale 0.97 → 1 from the trigger / caret, 150 ms enter curve; close 100 ms |
| @mention popup while typing, filtering | none: rows update in place |
| Tooltip | 400 ms delay, then 120 ms fade; next tooltip within 1 s: no delay, no fade |
| Dialog (invite code) | scrim fade 180 ms; dialog opacity + scale 0.97 → 1, centered, 200 ms enter curve; close 140 ms |
| Side panel toggle (mouse) | width does not animate; panel content fades 120 ms; from ⌘I: instant |
| Status chip change | color cross-fade 180 ms `ease`; glyph swaps with the same fade |
| New message arrives at the bottom | opacity 0 → 1 and translate y 4 → 0 px, 180 ms; only when the list follows the tail |
| Bot streaming text | appended text appears without animation; caret is a static 2 px bar while tokens arrive and blinks (530 ms on / off) only after 1 s idle |
| Typing dots | opacity steps, 1.2 s cycle, stops when hidden or with Reduce Motion |
| Button press | background to pressed color, 0 ms on press; release 120 ms. No scale on 28 px controls (macOS does not scale buttons); primary 32 px buttons may use scale 0.98 |
| Jump-to-latest button | appears with opacity + y 6 → 0, 180 ms |
| Unread badge count change | none (tabular digits avoid jitter) |
| Toast / notification | in from the top-right, y −8 → 0 + opacity, 240 ms; out along the same path, 160 ms |

**Never animates:** the Cmd+K palette open and close (Raycast rule, used hundreds of times a
day), moving selection with arrow keys in any list, switching 群 or topics, sidebar and list
hover, the composer focus ring, sending a message (the message appears in place, pending state
in `text-3`), theme change (system-driven, instant), scrolling to a search hit.

Reduce Motion: drop all translate and scale; keep opacity fades at the same durations; typing
dots become static; caret stops blinking.

## States

| State | Treatment |
| --- | --- |
| Hover (row) | `hover` fill, radius 6 / 8; no motion |
| Hover (message) | `hover` fill across the row; floating `e3` toolbar (quote, edit, more) top-right |
| Pressed | `pressed` fill; primary button `accent-pressed` |
| Focus (keyboard) | 1 px `accent` edge + 3 px `ring` halo; lists show the keyboard row with `accent-tint` instead of `selected` |
| Selected | `selected` fill, `text-1`, icon to `text-1`; unread weight stays |
| Disabled | `fill-quiet` background, `text-3` label, no shadow; icons `text-4` |
| Loading (older messages) | 3 `Skeleton` message rows (avatar + 2 lines) at the top; never a centered spinner |
| Loading (app start) | sidebar and topic list skeleton rows from the cache; skeleton never longer than 300 ms before the cache paints |
| Streaming | `正在输入…` in `accent-text` with a 6 px dot + 3 px tint halo, caret bar at the end |
| Pending send | text in `text-3`, no spinner for the first 1 s; then a 12 px `Spinner` after the time |
| Failed send | `circle-alert` + 发送失败 in `danger` and a small 重试 button below the text |
| Deleted | `ban` icon + 此消息已删除 in `text-3` |
| Empty (group has no topics) | `Empty` with `square-pen` media, TOPICS_EMPTY text, composer stays usable |
| Empty (待办) | single line TODO_EMPTY in `text-3` inside the section; the section never disappears |
| Error (login) | field ring `danger`, `danger-tint` fill, message under the field in `danger`, focus stays in the field |
| Offline | 28 px bar at the top of the content panel, `fill-quiet`, `wifi-off` icon + "已离线，正在重连" in `text-2`, composer still accepts text (queued in `text-3`); presence dots of others turn to offline rings after 30 s |
| Reconnected | the offline bar turns to `online` tint with "已重新连接" for 2 s, then collapses |

---

## Mapping to gpui-component

Theme slots are `gpui_component::ThemeColor` fields (set for both modes through `ThemeConfig`
JSON or `Theme::global_mut`). App-only tokens live in `apps/desktop/src/theme.rs` `Colors`.

| Token | Theme slot(s) |
| --- | --- |
| `content` | `background`, `list`, `table`, `tab_bar` |
| `sidebar`, `window` | `sidebar`, `title_bar` |
| `popover` | `popover`; `popover_foreground` = `text-1` |
| `text-1` / `text-3` | `foreground` / `muted_foreground`; `sidebar_foreground` = `text-2` |
| `separator` | `border`, `sidebar_border`, `title_bar_border`, `table_row_border` |
| `separator-strong` | `input` |
| `hover` / `selected` | `list_hover`, `table_hover`, `secondary_hover` / `list_active`, `table_active`, `sidebar_accent`, `tab_active` |
| `fill-quiet` | `muted`, `secondary`, `skeleton`, `switch` (off) |
| `accent` set | `primary`, `primary_hover`, `primary_active`, `button_primary*`, `link`, `caret`, `progress_bar`, `slider_bar`, `switch` on via `primary` |
| `accent-tint` | `accent` (gpui-component's "accent" is the hover/keyboard tint), `accent_foreground` = `text-1` |
| `ring` | `ring` |
| `text-selection` | `selection` |
| `list_active_border` | transparent (no left bar on selected rows) |
| status colors | `yellow`/`yellow_light` (进行中), `blue` (等回答), `magenta` (等审, set to the violet), `red`/`red_light`, `green`/`green_light`; the app's `status_chip_colors` keeps the exact chip values |
| `danger`, `warning`, `online` | `danger*`, `warning*`, `success` |
| `scrim` | `overlay` |
| radii | `Theme.radius = 6`, `Theme.radius_lg = 10` |
| shadows | `Theme.shadow = true`; component shadows keep gpui-component's `ShadowTokens`; the app's `e1`..`e4` are `Vec<BoxShadow>` constants in `theme.rs` applied with `.shadow(...)` |
| fonts | `font_family = ".SystemUIFont"`, `font_size = 13`, `mono_font_family = "SF Mono"`, `mono_font_size = 12` |
| motion | `Theme.motion` (`MotionTokens`) with the values in Motion |

| UI element | gpui-component / GPUI | Notes |
| --- | --- | --- |
| Window, traffic lights, root | `gpui_kit::open_window`, `TitlebarOptions { appears_transparent: true, traffic_light_position: (18, 18) }` | Root mounts dialogs, popovers and notifications |
| Sidebar | `sidebar::Sidebar`, `SidebarGroup` (待办, 群), `SidebarMenu` / `SidebarMenuItem` with `.icon`, `.active`, `.suffix(badge)`, `.context_menu` | `SidebarHeader` holds the search field, `SidebarFooter` my profile |
| Search field + ⌘K hint | `input::Input` with `.prefix(Icon search)` and `.suffix(Kbd)`; clicking opens the palette | |
| Unread / mention badge | `badge::Badge::new().count(n).max(99)`; mention variant `.color(accent)` with an `@` prefix | Badge's own position is a dot on a child; sidebar uses it inline via `suffix` |
| DM avatar + presence | `avatar::Avatar::new().src(..)` or `.name(..)`, presence as `Badge::dot()` | see Avatar gap below |
| Bot mark | a 14 px `div` with `Icon(IconName::Bot)` layered on `Avatar` | no dedicated slot |
| Topic list | `list::List` with a `ListDelegate`, `ListItem` rows; filter bar is `tab::TabBar::segmented()` with `Tab`s | virtualized |
| Status chip | `tag::Tag::custom(bg, fg, transparent)` with `.rounded(6)` and a custom status `Icon` prefix | |
| New topic, panel toggle, more | `button::Button::new().ghost().icon(..)` with `.tooltip` + `Kbd` | |
| Timeline | `message_scroller::MessageScroller` (tail-follow, prepend, jump button) | jump button via `.with_jump_button_renderer` |
| Message groups | `message::MessageGroup` with `bubble::Bubble` `BubbleVariant::Ghost` (no bubble surface) | this direction uses no chat bubbles |
| Markdown body, quotes, code | `text::TextView` (Markdown) with the highlight theme from the code tokens | inline mentions as custom inline nodes |
| Status card fields | `description_list::DescriptionList` inside a styled `div` card | buttons are `Button` (default) and `Button::primary()` |
| 产出 card / side panel items | `attachment::Attachment` (`media`, `content`, `actions`, `status`, `progress`) | upload progress and failed retry come free |
| Image | GPUI `img()` with `ObjectFit::Cover`, radius 8, ring | click opens the 产出 window |
| Streaming label | `shimmer::ShimmerText` (once per 2 s) or plain text + dot | |
| Typing line | plain text + three dots | |
| New-messages divider, day divider | `separator::Separator::horizontal().label(..)` | color via `.color` |
| Composer | `input::Input` multi-line (`textarea`) in a styled container, IME native | |
| @mention popup | `input::popovers::completion_menu` anchored at the caret (`caret_anchored`) with `ListItem` rows + `Kbd` footer | |
| Hover toolbar | `button::ButtonGroup` of ghost icon buttons in an `e3` `div` | |
| Side panel | `resizable` panel; `description_list` for 状态/负责人; `progress::Progress` for 41 / 58 | |
| Work process steps | `stepper` (vertical) or `List`; glyphs are the status icons | |
| Cmd+K palette | `command::Command` with `CommandGroup`s, `.searchable(true)`, `.footer(..)`, opened in a `dialog::Dialog` without entrance animation | `dialog/entrance.rs` must be bypassed |
| Settings window | `setting::Settings` / `SettingPage` / `SettingGroup`; members table `table::Table`; level `select::Select`; developer view `switch::Switch` | |
| Invite code dialog | `dialog::Dialog` with `DialogHeader/Title/Description/Footer`; code shown as mono text; notice `alert::Alert` warning | the code is shown once and never logged |
| Login | `input::Input` (server), `input::OtpInput::new(..).groups(3)` (code), `Button::primary().loading(..)` | |
| Toasts | `notification::Notification` | |
| Empty states | `empty::Empty` with `EmptyMedia`, `EmptyTitle`, `EmptyDescription` | |
| Loading | `skeleton::Skeleton`, `spinner::Spinner` | |
| Tooltips | `tooltip::Tooltip::new(..).key_binding(Kbd)` | |

### What GPUI or gpui-component cannot render (and the plan)

| Gap | Plan |
| --- | --- |
| No per-element backdrop blur | No vibrancy (see Materials). Popovers are opaque `popover`. |
| No letter-spacing in `TextStyle` | Rely on SF's built-in optical tracking; the invite code uses SF Mono at 26 px, which is already open; display text has no negative tracking. |
| No `-webkit-line-clamp` for multi-line truncation with ellipsis on Chinese | Quotes clamp to 2 lines with `line_clamp(2)` where available; else truncate to one line. |
| No middle truncation (keep file extension) | Truncate 产出 names in the app: `head…tail.ext` computed with the text system's measured width. |
| Avatar initials: `extract_text_initials` takes the first char of each space-separated word and the only the first char of a one-word Chinese name (周子航 → 周) and colors by its own `IdentityColor` | Compute initials in the app (given name, grapheme-aware) and pass them space-separated (`"子 航"`) so Avatar renders "子航"; or a thin `MemberAvatar` wrapper that uses `Avatar` for images and the palette above for initials. Upstream fix preferred. |
| Gradient text, `color-mix` | Not used. Chip tints are precomputed `Hsla` with alpha. |
| SVG with two colors | Status glyphs use cut-out paths (one color). |
| Mask fade at the timeline edges | GPUI has `linear_gradient` backgrounds: draw a 16 px gradient from `content` to transparent over the list edge instead of a mask. |
| Dialog entrance motion always on | Palette uses a plain overlay element, not `Dialog`, so it opens instantly. |

---

## Worst-case rules (break-ui)

Checked in [worst-case.html](worst-case.html) / [png/worst-case.png](png/worst-case.png).

| Field | Rule |
| --- | --- |
| 群 name, topic title, member name in rows | one line, end ellipsis, full value in the tooltip and the header of its own view |
| Topic title in the topic header | one line, ellipsis; the status chip never shrinks |
| Chip with a name (等{name}回答) | max width 168 px, the name truncates inside the chip |
| Message text and unbroken strings (hashes, URLs) | wrap anywhere; never overflow the 600 px measure |
| Name row in a message | name keeps up to 60 % of the row, role truncates first, time never truncates |
| Counts | badges cap at `99+` (catalog `UNREAD_MORE`); filter counts and 产出 count cap at `999+`; member counts use a thousands separator (1,284) |
| Old times | today: `HH:mm`; yesterday: 昨天; this week: 周一; this year: `10月28日`; older: `2025年12月31日` |
| One-character names and titles | allowed; avatars still 1 grapheme; rows keep full height |
| RTL names (Arabic) | `dir=auto` per label; the row layout stays LTR |
| Emoji in names | shown in labels; skipped for initials |
| Emoji-only message | 30 / 38 when the message is 1 to 3 emoji only |
| File names | middle truncation keeping the extension |
| Filters that do not fit | the filter bar clips with a 28 px fade; no wrapping |
| Missing optional fields | 在等 shows 没有人; missing role omits the role, no dash |

Breaks found and fixed while checking: filter labels wrapped per character (now `nowrap` +
fade); emoji-only messages rendered at body size (selector fixed); two-letter Latin initials at
18 to 20 px were unreadable (one grapheme below 24 px); the worst-case toggle covered content
(moved).

## Review against the vendored skills

emil-design-eng and apple-design findings on the first renders, and what changed:

| Before | After | Why |
| --- | --- | --- |
| Bot avatars were a bot icon plus a bot mark | Initials on the neutral bot pair plus the mark | Redundant signal; equal members share one avatar form |
| Search field hint `⌘K` stretched to half the field | Kbd sized to content | Generic flex rule leaked into the Kbd |
| Timeline cut hard at the header and composer | 20 px / 16 px edge fade | Scroll edge effect instead of a hard cut (apple-design §12) |
| @mention popup at the composer start covered the bot's streaming reply | Anchored at the caret, near the right | Origin-aware popover; keeps the newest content readable |
| Status chips as pills, all tinted | 6 px chips; only "waiting on you" filled | Precision; the loudest element is the one that needs you |
| Disabled login button = primary at 45 % opacity | `fill-quiet` + `text-3` | A washed-out accent reads as "loading", not "not ready" |
| Dark "等你审" chip: white on `#A88BF0` (2.8:1) | Fill `#7556D6` (5.2:1) | Contrast of filled chips in dark mode |
| Settings dialog hid the whole members table | Taller window; rows visible below the dialog | The tab must stay recognisable behind the sheet |
| App icon: circle and square side by side | Diagonal overlap | Side by side read as a toggle switch at 16 to 32 px |
| Palette and keyboard actions considered with entrance motion | No motion | review-animations: 100+ times a day → never animate |

## App icon

[icon.html](icon.html), [png/icon.png](png/icon.png) (sheet with 128, 64, 32, 16 on light and
dark), [png/icon-1024.png](png/icon-1024.png) (1024 px with alpha). macOS 26 grid: 824 px body
at 100 px inset, continuous-corner radius 186, graphite body (`#34363D` → `#16171B`) with a
10 % top sheen and a 14 % white edge; a white circle (a person) and a cobalt rounded square (a
bot) of the same visual weight overlap diagonally, the overlap in light cobalt: people and bots
equal, the conversation where they meet. No glyphs or letters, so it holds at 16 px.

## Files

| File | What |
| --- | --- |
| `src/shared.css` | Every token above as CSS variables, light and dark, and the component styles |
| `src/partials/main.html` | The main window content (shared by main and switcher) |
| `src/partials/glyphs.html` | The six status glyphs |
| `src/partials/appicon.html` | The app icon |
| `src/*.html` | Page sources |
| `build.ts` | Inlines CSS, partials and the Lucide symbols each page uses from `gpui-kit-assets-0.8.1` (`bun build.ts`) |
| `render.sh` | Builds and renders every page to `png/` with headless Chrome at 2× |
| `main-light.html`, `main-dark.html` | Main window, scrolled to the bottom, @mention popup open |
| `main-light-top.html`, `main-dark-top.html` | Same topic scrolled up: @mention of a bot, bot acknowledgement, quote, image, new-messages divider, status card |
| `switcher.html` | Cmd+K over the main window |
| `settings-members.html` | Settings, 成员 tab, one-time invite code dialog |
| `login.html` | Sign-in with the invite code |
| `worst-case.html` | Main window with break-ui data |
| `icon.html` | App icon sheet |

Product name in the login mockup comes from `product.json` (`PRODUCT_NAME`); the code never
types it. The invite code and server address in the mockups are made up.
