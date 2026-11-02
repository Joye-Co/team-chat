# Design

The visual design of the desktop app. Implementers follow this file exactly. When a value is
not here, ask before you invent one. AGENTS.md and docs/PRODUCT.md win over this file for
product rules and interface words; this file wins for how things look and move.

- Mockups: [design/mockups/](design/mockups/) (PNG at device scale 1, HTML source beside each
  PNG, built by `bun docs/design/mockups/src/build.mjs`; add `--scale=2` for a retina check).
- Tokens in code form: [design/mockups/src/tokens.mjs](design/mockups/src/tokens.mjs). Keep it
  in step with this file.
- App icon: [design/brand/](design/brand/) (`icon.svg` is the 1024 master; PNG renders beside it;
  rebuild with `bun docs/design/mockups/src/brand.mjs`).
- What GPUI can and cannot do: [design/GPUI-CAPABILITIES.md](design/GPUI-CAPABILITIES.md).
  The per-change checklist: [design/CHECKLIST.md](design/CHECKLIST.md).

## 0. How this design was chosen

Three directions were made and judged by three judges (craft and taste 45 %, daily use 35 %,
buildability 20 %):

| Direction | Craft | Daily use | Build | Weighted |
| --- | --- | --- | --- | --- |
| 原生质感 Native calm | 7.3 | 8.3 | 8.3 | **7.85** |
| 温暖编辑 Warm editorial | 7.6 | 7.7 | 7.8 | 7.68 |
| 精密工具 Precise pro | 7.9 | 7.6 | 7.2 | 7.66 |

No judge disqualified a direction. **Native calm is the base.** These ideas from the judges were
folded in, because they make it better without breaking its rules:

| Graft | From | Why |
| --- | --- | --- |
| Opaque sidebar by default; window vibrancy is an option that ships only after it is measured | precise, editorial, build judge | The 60 fps / 10,000-message and under-1 s cold start budgets come first |
| Three chip weights only: filled orange for 等你…, red tint for 卡住了, a quiet outlined chip with a colored 11 pt glyph for everything else | native rule, precise glyphs, editorial quiet chip | "Color means you" held strictly; status is told by shape as well as hue |
| No solid accent selection anywhere: list selection is gray, the active row in popups, menus and the switcher is a light accent tint | craft judge | The saturated blue fills were louder than anything that asked for the user |
| Settings panes use monochrome icons, no colored tiles | craft judge | The tiles broke "color means you" |
| Message text 14/24 with a 560 pt measure (about 40 Han characters); topic title 17/22 | editorial | Reading comfort for long Chinese threads |
| Sidebar 待办 lists the waiting topics with a context line, not only a count | precise, editorial | You see what waits on you without a click |
| Filter tabs show counts (全部 14 · 未读 3 · 待办 2) | precise | Triage at a glance |
| Kbd footer in the @mention popup; shortcuts in tooltips and menus | precise | Keyboard-first, but not on every daily surface |
| Offline band at the top of the sidebar as well as the composer bar | editorial | The state is visible on every screen |
| Failed send, deleted message, pending send, reconnected states | precise | They were not specified before |
| Invite code as `OtpInput` in 3 groups; a disabled primary button is a quiet gray fill | precise | Clearer than a faded accent |
| Side panel width snaps; only its content fades (120 ms) | editorial, build judge | No layout animation |
| Streaming caret is solid while text arrives and blinks only after 1 s of idle | precise | Motion only when it carries meaning |
| Contrast ratio next to each text token; text-3 raised to at least 3.4:1 | precise, craft judge | Native's text-3 was 2.4:1 |
| A 「 」 app icon instead of a generic two-bubble glyph | editorial idea, native shapes | The brand had no point of view |
| `MemberAvatar` wraps gpui-component `Avatar` with a radius override for bots | build judge | `Avatar` implements `Styled`; no fully custom element |

Rejected: precise's five status hues and Kbd hints on every surface (too busy for 8 hours);
editorial's terracotta accent (too close to the danger red), separate 私聊 section (PRODUCT.md
says one list of 群) and ink bot tiles (they outweigh people).

## 1. Principles

1. **System first.** System font, system blue, hairlines, native title bar and traffic lights,
   Mail's columns, System Settings' layout. Make a custom choice only for our own concepts:
   member shapes, status chips, cards.
2. **Color means you.** Accent blue and orange appear only where the viewer must act: the unread
   dot, the @n and 待办 badges, the 等你回答 / 等你审 chips, @mentions of you, primary buttons and
   the caret. Everything else is gray. 卡住了 is the one other colored fill, because it is an
   alarm for everyone.
3. **Lines are hairlines, and only where needed.** Separate with spacing first, then a 0.5 pt
   hairline. Shadows only on things that float. A card never sits in a card.
4. **Members are equal and different.** People are circles. Bots are rounded squares with a
   small graphite mark. Same sizes, palettes, rows and name style.
5. **Quiet motion.** What the keyboard triggers, or what a user sees tens of times a day, does not
   animate. Surfaces that appear on their own move for at most 220 ms.
6. **Readable all day.** Message text 14/24 at most 560 pt wide. Weights 400, 500, 600 only.
   Tabular figures on every number. Text that carries content is at least 4.5:1.
7. **Worst-case ready.** Every text slot has exactly one rule: truncate, wrap or clamp (§12).

## 2. Color tokens

All values are sRGB as `#RRGGBBAA` (the gpui-component theme JSON format). An alpha color is
drawn over the surface named in its role. `L` = light, `D` = dark. Contrast is measured over the
named surface.

### 2.1 Surfaces

| Token | L | D | Role |
| --- | --- | --- | --- |
| `sidebar` | `#F0F0F2FF` | `#262628FF` | Sidebar and settings pane list (opaque, the default) |
| `sidebar-tint` | `#EAEAEFBD` | `#242428C2` | Only with the vibrancy option: painted over the `Blurred` window instead of `sidebar` |
| `content` | `#FFFFFFFF` | `#1E1E20FF` | Topic list, timeline, composer, login, 产出 window chrome |
| `panel` | `#F7F7F8FF` | `#232326FF` | Side panel, 产出 window version list |
| `window-bg` | `#ECECEEFF` | `#1E1E20FF` | Settings pane background behind grouped sections |
| `group` | `#FFFFFFFF` | `#2A2A2DFF` | Grouped (inset) sections in settings |
| `elevated` | `#FFFFFFFF` | `#2C2C2FFF` | Popovers, menus, @mention popup, switcher, dialogs, toasts, hover toolbar, jump pill |
| `toolbar-capsule` | `#FFFFFFDB` | `#3A3A3EDB` | Grouped toolbar buttons |
| `scrim` | `#0000002E` | `#00000061` | Behind a sheet or alert. The switcher has none |

### 2.2 Text

| Token | L | D | Contrast (L / D) | Role |
| --- | --- | --- | --- | --- |
| `text-1` | `#000000D9` | `#FFFFFFDB` | 15.1 / 12.5 | Body, names, titles |
| `text-2` | `#0000008C` | `#EBEBF594` | 4.7 / 5.6 (4.6 / 5.3 on sidebar) | Previews, 岗位, metadata, icons at rest |
| `text-3` | `#00000075` | `#EBEBF56B` | 3.5 / 3.6 | Timestamps, section headers, kbd hints. Never content |
| `placeholder` | `#0000005C` | `#EBEBF55C` | 2.5 / 3.0 | Field placeholders only |

### 2.3 Lines and fills

| Token | L | D | Role |
| --- | --- | --- | --- |
| `sep` | `#00000017` | `#FFFFFF16` | 0.5 pt hairlines: column edges, list separators (inset to the text) |
| `sep-strong` | `#00000024` | `#FFFFFF21` | Process-step connector |
| `fill-hover` | `#00000009` | `#FFFFFF0B` | Message row hover, ghost button hover |
| `fill-selected` | `#00000011` | `#FFFFFF16` | Selected topic row, toggled toolbar button, current version |
| `sidebar-selected` | `#00000013` | `#FFFFFF1A` | Selected sidebar and settings-pane row |
| `fill-pressed` | `#0000001A` | `#FFFFFF21` | Pressed rows and ghost buttons, disabled primary and send, progress track |
| `fill-field` | `#0000000B` | `#FFFFFF12` | Search fields, segmented track, kbd, file tiles, bands, alerts |
| `fill-active` | `#007AFF1F` | `#0A84FF42` | Active row in popups, menus, the switcher and the @mention popup |
| `control` | `#FFFFFFFF` | `#FFFFFF1A` | Secondary button, input and OTP box background |
| `control-border` | `#00000021` | `#FFFFFF1A` | Composer border at rest |
| `control-border-focus` | `#00000042` | `#FFFFFF3D` | Composer border while it has focus (no blue) |
| `chip-ring` | `#00000024` | `#FFFFFF29` | Ring of the quiet chip |
| `card-border` | `#0000001A` | `#FFFFFF1A` | Cards, decision box, grouped sections, sheet panels |
| `image-border` | `#00000012` | `#FFFFFF14` | Inner ring on images |
| `quote-bar` | `#00000029` | `#FFFFFF33` | 2 pt quote bar |

### 2.4 Accent, focus, selection

The accent follows `NSColor.controlAccentColor` (§16); values below are for the default blue.

| Token | L | D | Role |
| --- | --- | --- | --- |
| `accent` | `#007AFFFF` | `#0A84FFFF` | Primary buttons, send, unread dot, badges, caret, progress fill |
| `accent-hover` | `#0A6FE6FF` | `#2B93FFFF` | Primary hover |
| `accent-pressed` | `#0062CCFF` | `#0070E0FF` | Primary pressed |
| `accent-text` | `#0066D6FF` | `#4DA3FFFF` | Links, @mentions of others, text buttons (5.4 / 6.3) |
| `accent-soft` | `#007AFF1C` | `#0A84FF38` | HTML 产出 tile |
| `on-accent` | `#FFFFFFFF` | `#FFFFFFFF` | Text on accent (4.0 / 3.6: labels only at 13 pt weight 500 or larger) |
| `focus-ring` | `#007AFF6B` | `#0A84FF8C` | 3 pt keyboard focus ring outside controls |
| `selection` | `#007AFF33` | `#0A84FF5C` | Text selection |
| `mention-bg` | `#007AFF1A` | `#0A84FF33` | Background of an @mention of someone else |
| `mention-me-bg` / `mention-me-text` | `#FF95002E` / `#A35400FF` | `#FF9F0A3D` / `#FFBD5CFF` | An @mention of me (4.8 / 6.2) |

### 2.5 Status chips

Chip: 18 pt high, radius 5, padding 4 pt left and 6 pt right, gap 3 pt, an 11 pt Lucide glyph,
11 pt text at weight 500. Maximum width 168 pt.

| Status | Kind | Glyph | L bg / text / glyph | D bg / text / glyph |
| --- | --- | --- | --- | --- |
| `open` | none | | | |
| 进行中 | quiet | `circle-dot` | transparent + `chip-ring` / `#0000009E` / `#007AFFFF` | transparent + `chip-ring` / `#EBEBF5A3` / `#0A84FFFF` |
| 等{name}回答, 等回答 | quiet | `circle-question-mark` | same / same / `#E68600FF` | same / same / `#FF9F0AFF` |
| 等{name}审, 等审 | quiet | `eye` | same / same / `#E68600FF` | same / same / `#FF9F0AFF` |
| 完成 | quiet | `circle-check` | same / same / `#1F9D45FF` | same / same / `#30D158FF` |
| 等你回答, 等你审 | you | `circle-question-mark`, `eye` | `#FF95002B` / `#9C5100FF` (5.1:1), weight **600**, glyph in text color | `#FF9F0A38` / `#FFB547FF` (6.1:1) |
| 卡住了 | blocked | `circle-alert` | `#FF3B301C` / `#C4001AFF` (5.4:1) | `#FF453A33` / `#FF7A70FF` (5.2:1) |

Quiet chip text is 6.2 / 6.5:1. In 等{name}…, the name truncates at 104 pt; 等 and 回答/审 always
stay visible (`等Aleksandra Wiśni…审`).

### 2.6 Badges, presence, bot mark

| Token | L | D | Role |
| --- | --- | --- | --- |
| `badge-bg` / `badge-text` | `#00000011` / `#00000094` | `#FFFFFF1A` / `#EBEBF59E` | Unread count: 18 pt capsule, 11 pt / 500, tabular, padding 6 pt, min width 18 pt |
| `badge-mention-bg` | `#007AFFFF` | `#0A84FFFF` | `@3` (white, 600). The 待办 count uses the same fill without `@` |
| `online` | `#34C759FF` | `#30D158FF` | Presence dot: 30 % of avatar (min 8 pt), 1.5 pt ring of the surface |
| `bot-mark` | `#7C7C82FF` | `#8E8E94FF` | Circle at the bottom-right of a bot avatar: 42 % of avatar (min 11 pt), white Lucide `bot` at 28 % (min 8 pt), 1.5 pt ring of the surface, offset −22 % of its size |

### 2.7 Avatar palettes

Chosen by a stable hash (FNV-1a of the member id, modulo 8). A photo replaces the tint and keeps
the shape, mark and presence dot.

| Palette | L bg / fg | D bg / fg |
| --- | --- | --- |
| slate | `#E3E8EF` / `#46566C` | `#3A4352` / `#C3CFDF` |
| sage | `#E0ECE2` / `#3B6646` | `#33463A` / `#B6D6BE` |
| sand | `#F0E7D8` / `#76572A` | `#4A4031` / `#E2CFAE` |
| rose | `#F3E2E4` / `#874450` | `#4D3539` / `#EBC0C7` |
| sky | `#DCE9F6` / `#2E5C87` | `#2F4257` / `#B4D2EE` |
| clay | `#EFE1D9` / `#864D36` | `#4C372E` / `#EBC4B2` |
| teal | `#DAEDED` / `#2C6868` | `#2C4646` / `#AEDADA` |
| graphite | `#E6E6E9` / `#55555C` | `#3C3C41` / `#CFCFD6` |

### 2.8 Content blocks and system colors

| Token | L | D | Role |
| --- | --- | --- | --- |
| `code-bg` / `code-text` | `#F5F5F7FF` / `#000000D9` | `#28282BFF` / `#FFFFFFDB` | Code blocks, no border |
| syntax keyword / string / type / function / comment / number | `#9B2393` / `#C41A16` / `#0B4F79` / `#326D74` / `#5D6C79` / `#1C00CF` | `#FF7AB2` / `#FF8170` / `#6BDFFF` / `#67B7A4` / `#7F8C98` / `#D9C97C` | Xcode default palettes, as `HighlightTheme` |
| `danger` / `danger-text` | `#FF3B30FF` / `#D70015FF` | `#FF453AFF` / `#FF6961FF` | Destructive fills / text (5.4 / 5.9) |
| `success` | `#34C759FF` | `#30D158FF` | Check icons |
| `warning` | `#FF9500FF` | `#FF9F0AFF` | Warning icons |
| `new-divider` | `#FF3B30B3` line, label in `danger-text` | `#FF453AB3` line, label `danger-text` | 新消息 divider |
| `scrollbar-thumb` | `#00000040` | `#FFFFFF40` | Overlay scrollbar |

## 3. gpui-component theme mapping

Load one JSON theme set with a `light` and a `dark` theme (GPUI-CAPABILITIES §2, option 1) from
`apps/desktop/assets/theme.json`. Every `ThemeColor` slot the app can show is set; values are the
tokens of §2.

| Slot(s) | L | D |
| --- | --- | --- |
| `background` | `#FFFFFFFF` | `#1E1E20FF` |
| `foreground` | `#000000D9` | `#FFFFFFDB` |
| `muted` | `#0000000B` | `#FFFFFF12` |
| `muted_foreground` | `#0000008C` | `#EBEBF594` |
| `border` | `#00000017` | `#FFFFFF16` |
| `input` | `#00000021` | `#FFFFFF1A` |
| `ring` | `#007AFF6B` | `#0A84FF8C` |
| `selection` | `#007AFF33` | `#0A84FF5C` |
| `caret` | `#007AFFFF` | `#0A84FFFF` |
| `overlay` | `#0000002E` | `#00000061` |
| `accent` / `accent_foreground` (component hover and highlighted row) | `#007AFF1F` / `#000000D9` | `#0A84FF42` / `#FFFFFFDB` |
| `primary` / `_hover` / `_active` / `_foreground` | `#007AFFFF` / `#0A6FE6FF` / `#0062CCFF` / `#FFFFFFFF` | `#0A84FFFF` / `#2B93FFFF` / `#0070E0FF` / `#FFFFFFFF` |
| `secondary` / `_hover` / `_active` / `_foreground` | `#FFFFFFFF` / `#F7F7F7FF` / `#E6E6E6FF` / `#000000D9` | `#FFFFFF1A` / `#FFFFFF24` / `#FFFFFF2E` / `#FFFFFFDB` |
| `button` / `button_hover` / `button_active` / `button_foreground` | as `secondary` | as `secondary` |
| `button_primary*` | as `primary*` | as `primary*` |
| `danger` / `_hover` / `_active` / `_foreground` | `#FF3B30FF` / `#F0322AFF` / `#D92B22FF` / `#FFFFFFFF` | `#FF453AFF` / `#FF5A50FF` / `#E03A30FF` / `#FFFFFFFF` |
| `success`, `warning`, `info` (+ `_foreground` white) | `#34C759FF`, `#FF9500FF`, `#007AFFFF` | `#30D158FF`, `#FF9F0AFF`, `#0A84FFFF` |
| `list` / `list_hover` / `list_active` / `list_active_border` / `list_even` / `list_head` | `#FFFFFFFF` / `#00000000` / `#00000011` / `#00000000` / `#FFFFFFFF` / `#FFFFFFFF` | `#1E1E20FF` / `#00000000` / `#FFFFFF16` / `#00000000` / `#1E1E20FF` / `#1E1E20FF` |
| `table` / `table_hover` / `table_active` / `table_row_border` / `table_head` / `table_head_foreground` | `#FFFFFFFF` / `#00000009` / `#00000011` / `#00000017` / `#FFFFFFFF` / `#0000008C` | `#2A2A2DFF` / `#FFFFFF0B` / `#FFFFFF16` / `#FFFFFF16` / `#2A2A2DFF` / `#EBEBF594` |
| `popover` / `popover_foreground` | `#FFFFFFFF` / `#000000D9` | `#2C2C2FFF` / `#FFFFFFDB` |
| `group_box` / `group_box_foreground` | `#FFFFFFFF` / `#000000D9` | `#2A2A2DFF` / `#FFFFFFDB` |
| `description_list_label` / `_foreground` | `#00000000` / `#0000008C` | `#00000000` / `#EBEBF594` |
| `sidebar` / `sidebar_foreground` | `#F0F0F2FF` / `#000000D9` | `#262628FF` / `#FFFFFFDB` |
| `sidebar_accent` / `sidebar_accent_foreground` | `#00000013` / `#000000D9` | `#FFFFFF1A` / `#FFFFFFDB` |
| `sidebar_primary` / `sidebar_primary_foreground` | `#007AFFFF` / `#FFFFFFFF` | `#0A84FFFF` / `#FFFFFFFF` |
| `sidebar_border` | `#00000017` | `#FFFFFF16` |
| `title_bar` / `title_bar_border` | `#00000000` / `#00000000` | same |
| `tab_bar` / `tab_bar_segmented` / `tab` / `tab_active` / `tab_foreground` / `tab_active_foreground` | `#00000000` / `#0000000B` / `#00000000` / `#FFFFFFFF` / `#0000008C` / `#000000D9` | `#00000000` / `#FFFFFF12` / `#00000000` / `#FFFFFF29` / `#EBEBF594` / `#FFFFFFDB` |
| `switch` / `switch_thumb` | `#00000024` / `#FFFFFFFF` (on = `primary`) | `#FFFFFF29` / `#FFFFFFFF` |
| `progress_bar` / `skeleton` | `#007AFFFF` / `#0000000B` | `#0A84FFFF` / `#FFFFFF12` |
| `scrollbar` / `scrollbar_thumb` / `scrollbar_thumb_hover` | `#00000000` / `#00000040` / `#00000066` | `#00000000` / `#FFFFFF40` / `#FFFFFF66` |
| `link` / `link_hover` / `link_active` | `#0066D6FF` / `#0055B3FF` / `#004799FF` | `#4DA3FFFF` / `#6BB3FFFF` / `#8CC4FFFF` |
| `blue` / `blue_light` | `#007AFFFF` / `#007AFF1A` | `#0A84FFFF` / `#0A84FF33` |
| `yellow` / `yellow_light` (used for 等你… chips) | `#9C5100FF` / `#FF95002B` | `#FFB547FF` / `#FF9F0A38` |
| `red` / `red_light` | `#C4001AFF` / `#FF3B301C` | `#FF7A70FF` / `#FF453A33` |
| `green` / `green_light` | `#1F9D45FF` / `#34C75924` | `#30D158FF` / `#30D1582B` |
| `magenta`, `cyan` (+ `_light`) | not used; set to `blue` values | same |
| `chart_1` … `chart_5` | `#007AFF`, `#34C759`, `#FF9500`, `#AF52DE`, `#8E8E93` | `#0A84FF`, `#30D158`, `#FF9F0A`, `#BF5AF2`, `#98989D` |
| `window_border`, `drag_border`, `drop_target` | `#00000024`, `#007AFFFF`, `#007AFF1A` | `#FFFFFF21`, `#0A84FFFF`, `#0A84FF33` |

Theme fields (both modes): `font.family` `.SystemUIFont`, `font.size` 13, `mono_font.family`
`Menlo`, `mono_font.size` 12.5, `radius` 7, `radius.lg` 10, `shadow` true, `focus_ring` true,
`scrollbar_mode` `Scrolling`, `highlight_theme` built from §2.8.

`MotionTokens` (set in code after every `Theme::change`, or in the JSON when the schema allows):
`duration_fast` 120 ms, `duration_normal` 180 ms, `duration_slow` 220 ms, `easing_enter`
`cubic_bezier(0.23, 1, 0.32, 1)`, `easing_exit` `cubic_bezier(0.23, 1, 0.32, 1)` (replaces the
default ease-in), `easing_move` `cubic_bezier(0.77, 0, 0.175, 1)`; springs and distances keep
their defaults.

App-owned roles (chip kinds and glyph colors, avatar palettes, mention colors, bot mark,
presence, `fill-active`, `control-border-focus`, `chip-ring`, code syntax, elevation shadows) are
fields of `theme::Colors` in `apps/desktop/src/theme.rs`, one value per mode. Views never write a
color literal.

## 4. Typography

- **Family**: `.SystemUIFont` (SF Pro). Core Text falls back to **PingFang SC** for Han. Do not
  name PingFang as the primary family (Latin would then be PingFang's Latin). If a screenshot ever
  shows a non-PingFang Han face, pin the fallback with `FontFallbacks::from_fonts(["PingFang SC"])`.
- **Mono**: Menlo (try `.AppleSystemUIFontMonospaced` first; not verified in GPUI).
- **Weights**: 400, 500, 600 only. Never 700.
- **Tracking**: none (GPUI has no letter-spacing). The invite code in the sheet is drawn as three
  runs with a 2 pt gap.
- **Numbers**: `FontFeatures` `tnum` on every count, badge, time, date, size, version and progress.

| Role | Size / line | Weight | Where |
| --- | --- | --- | --- |
| Display | 22 / 28 | 600 | Login welcome |
| Topic title | 17 / 22 | 600 | Topic view toolbar |
| Title | 15 / 20 | 600 | Topic-list toolbar (群 name), settings pane title, sheet title, topic-start header, panel sheet titles |
| Message | 14 / 24 | 400 | Message body and composer text. Measure: max 560 pt |
| UI | 13 / 18 | 400; 500 for row titles; 600 for unread | Sidebar, topic rows, menus, buttons, fields, card fields |
| Callout | 12 / 16 | 400 | Previews, 岗位, card metadata, typing line, hints, quote text is 13 / 19 |
| Caption | 11 / 14 | 500; 600 for section headers | Chips, badges, section headers, message timestamps, kbd, sidebar context lines |
| Code | 12.5 / 19 | 400 Menlo | Code blocks, log blocks |
| Invite code | 24 / 30 | 500 Menlo | Sheet; the login OTP boxes are 16 pt / 500 Menlo |
| Emoji-only message (1 to 3 emoji) | 32 / 40 | 400 | Timeline |

## 5. Spacing and layout

Spacing scale (pt): 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32. Nothing else.

### 5.1 Main window

| Metric | Value |
| --- | --- |
| Window | Minimum 1100 × 680, default 1440 × 900. `TitlebarOptions { appears_transparent: true, traffic_light_position: (20, 18) }`. Opaque background (§8) |
| Toolbar band | 52 pt, part of each column (no `TitleBar` component). Traffic lights centered in the band |
| Columns | Sidebar 236 (200–320, resizable) · topic list 300 (260–380) · topic view flexible, min 480 · side panel 280 (240–360). Below a 1100 pt window the side panel is hidden. Each column after the first has a 0.5 pt `sep` line on its leading edge |
| Scroll edge | No toolbar border. A 14 pt gradient from `content` to transparent under the toolbar while content scrolls beneath |

### 5.2 Sidebar

| Part | Value |
| --- | --- |
| Insets | Rows inset 10 pt from both edges; row inner padding 8 pt |
| Offline band | Only when offline: 28 pt, radius 7, `fill-field`, `wifi-off` 13 pt + "离线，正在重新连接…" 12 pt `text-2`, 10 pt side margin, 8 pt below |
| Search | 28 pt field, `fill-field`, radius 7, `search` 14 pt, `⌘K` kbd right aligned in `text-3`, 10 pt below |
| Section header | 11 / 14, 600, `text-3`; 14 pt above (6 pt for the first), 6 pt below; trailing control 20 pt |
| 待办 header | "待办" + the 待办 count badge (accent, no `@`) at the trailing edge |
| 待办 rows | 40 pt, two lines: title 13 / 500, context 11 / 14 `text-2` (`开发 · 等你审`, `运维 · 值守助手 @你`). Leading 20 pt column with a 15 pt glyph: `eye` (审) or `circle-question-mark` (回答) in `glyph-wait`, `at-sign` in `accent`. At most 5 rows, newest first; then a 22 pt "全部 n 项 ›" row in 12 pt `text-2` |
| 群 rows | 28 pt, radius 7, 8 pt gap; 20 pt leading: group tile (rounded square radius 5.5, first character 11 / 600, palette by hash) or, for a DM, the other member's 20 pt avatar with presence. Name 13 pt (600 when unread, `text-2` when muted). Trailing: `@n` mention badge, else unread badge, else `bell-off` 13 pt `text-3` when muted |
| Footer | 52 pt: 26 pt avatar with presence, name 13 / 500 and 岗位 11 pt `text-2` (both truncate), settings icon button 28 pt |

### 5.3 Topic list

| Part | Value |
| --- | --- |
| Toolbar | 群 name 15 / 600 and a sub line `8 位成员 · 14 个话题` 11 pt `text-2`; trailing `square-pen` icon button (新话题, tooltip `新话题 ⌘N`) |
| Filter | `TabBar` segmented, full width minus 12 pt margins: `全部 14` `未读 3` `待办 2`; the count in `text-3` weight 400, tabular, capped at `999+` |
| Rows | Inset 8 pt, radius 8. Padding 9 top, 10 bottom, 20 left (unread-dot gutter), 10 right; 4 pt between lines. Line 1: title 13 / 500 (600 unread) + time 12 pt `text-2`. Line 2: chip + preview 12 pt `text-2` (`作者：内容`, one line) + unread badge or `@n`. Unread dot 8 pt `accent` at x 6, y 14. Hairline separators inset to the text, hidden next to the selected row. Selected: `fill-selected`. No hover fill |

### 5.4 Topic view

| Part | Value |
| --- | --- |
| Toolbar | Title 17 / 600 + its chip; sub line `林晓 发起 · 设计助手 负责 · 今天 10:12` 11 pt `text-2`. Trailing: `AvatarGroup` of up to 3 participants (22 pt, −6 pt overlap, 1.5 pt `content` ring), then a capsule (32 pt, radius 16, `toolbar-capsule`, capsule shadow) with `bell`, `panel-right`, `ellipsis` ghost buttons 32 × 28. A toggled button (open panel) has `fill-selected`, not accent |
| Timeline | 20 pt leading, 24 pt trailing, 32 pt avatar, 12 pt gutter. Body max 560 pt |
| Grouping | Same author within 5 minutes and nothing between: no avatar or name, 2 pt top. A new group: 12 pt top |
| Typing line | 22 pt, directly above the composer, always reserved |
| Composer | 20 pt side margin, 16 pt bottom, radius 12, padding 10 / 12 / 8. Grows from 1 to 7 lines (24 pt each), then scrolls inside. Tool row 28 pt: `paperclip`, `image`, `at-sign`, `text-quote`; hint `↵ 发送　⇧↵ 换行` 11 pt `text-3`; send 28 pt circle |

### 5.5 Other windows

| Window | Value |
| --- | --- |
| Settings | 900 × 640. Pane list 220 pt on `sidebar` with 30 pt rows; content on `window-bg` with 20 pt padding; grouped sections radius 10, rows 52 pt, separators inset to the text (50 pt) |
| Login | 440 × 580, `content`. 88 pt icon 28 pt below the title bar; form 344 pt wide |
| 产出 window | Default 1140 × 760, min 720 × 480. 52 pt toolbar; version list 260 pt on `panel` (toggle) |
| Sheet (Dialog) | 420 pt wide (alert 360), radius 14, padding 20 / 20 / 16, 52 pt from the window top |
| Switcher | 640 pt wide, 132 pt from the window top. Field 52 pt with 18 pt text. Rows 40 pt. Footer 36 pt |
| Popover | Radius 10, 5 pt inner padding; rows 32 pt (@mention), 28 pt (context menu), 36 pt (menus with two lines) |

## 6. Radii

| Token | Value | Use |
| --- | --- | --- |
| xs | 4 | kbd, mention background, inline code |
| chip | 5 | Status chips; group tiles 5.5 |
| sm | 6 | Small buttons, menu rows, segmented items, tooltips |
| md (`theme.radius`) | 7 | Buttons, fields, sidebar rows, OTP boxes, file tiles |
| row | 8 | Topic rows, code blocks, images, bands, alerts |
| lg (`theme.radius_lg`) | 10 | Cards, popovers, grouped sections |
| composer | 12 | Composer, toasts |
| xl | 14 | Sheets, alerts, the switcher |
| full | 50 % | People avatars, badges, send button, capsules, jump pill |
| bot | 28 % of size | Bot avatars (9 pt at 32 pt, 6 pt at 20 pt) |

Nested radii: inner = outer − padding.

## 7. Elevation

The first shadow of each level is a 0.5 pt spread ring and is the border; no separate border is
drawn. In GPUI: `BoxShadow { spread_radius: px(0.5), blur_radius: px(0.), .. }` first in the list.

| Level | L | D | Use |
| --- | --- | --- | --- |
| 0 | none | none | Columns |
| 1 | ring `card-border`, no shadow | same | Cards, grouped sections, decision box, images (ring `image-border`) |
| control | `0 0 0 .5 #0000001F, 0 1 1.5 #0000000F` | `0 0 0 .5 #FFFFFF14, 0 1 1.5 #00000040` | Secondary buttons, inputs, OTP boxes, the selected segment |
| capsule | `0 0 0 .5 #00000014, 0 1 3 #0000000F` | `0 0 0 .5 #FFFFFF14, 0 1 3 #0000004D` | Toolbar capsules |
| 2 | `0 0 0 .5 #0000001F, 0 4 12 #00000014, 0 14 36 #0000001F` | `0 0 0 .5 #FFFFFF1A, 0 4 12 #0000004D, 0 14 36 #00000073` | Popovers, menus, @mention popup, hover toolbar, jump pill, tooltips, toasts |
| 3 | `0 0 0 .5 #00000024, 0 6 16 #0000001A, 0 24 64 #00000033` | `0 0 0 .5 #FFFFFF1F, 0 6 16 #00000059, 0 24 64 #0000008C` | Sheets, alerts, the switcher |

## 8. Materials

- **Opaque by default.** Windows open with `WindowBackgroundAppearance::Opaque`; the sidebar
  paints `sidebar`. This is what ships first.
- **Vibrancy option.** A switch "侧边栏透明" in the developer view (catalog `copy::dev`) turns on
  `Blurred` for the main and settings windows and paints `sidebar-tint`.
  It becomes the default only after the 10,000-message scroll holds 60 fps and cold start stays
  under 1 s with it on (measure with `--selftest-smoke`), and after a look on a bright and a
  dark wallpaper. Never on when "Reduce transparency" is set.
- **No translucent layer on another.** Popovers, menus, the switcher and dialogs are opaque
  (`elevated`) with level 2 or 3 shadows. The sidebar is full height, not an inset pane.
- **Inactive window**: traffic lights gray (system), selection fills stay gray, the caret hides.

## 9. Iconography

- Lucide through gpui-kit (`IconName`). Icons outside the 101 defaults go through the app's own
  `AssetSource` that falls back to `gpui_kit::assets::Assets` (not `AllAssets`).
- Sizes: 16 pt toolbar, composer and sidebar; 15 pt 待办 glyphs, hover toolbar and menus (14);
  13 to 14 pt in buttons and fields; 11 pt in chips; 20 pt in 产出 tiles and empty states.
- Stroke: Lucide 2 on 24 (1.33 pt at 16). Color = the text role around it (`text-2` at rest);
  state colors only per §2.
- Set used: `search inbox plus panel-left panel-right settings square-pen bell bell-off ellipsis
  paperclip image at-sign text-quote copy arrow-up arrow-down file-code file-text file
  git-branch circle-check circle-alert circle-dot circle-question-mark eye triangle-alert loader
  circle-dashed circle wifi-off monitor users users-round user user-plus server chart-column
  activity code log-out chevrons-up-down chevron-right message-square-plus bot clock x lock
  download layers list list-checks table terminal rotate-ccw pencil trash search-x`.
- No emoji in the interface. Member-typed emoji render as text in the system emoji font.

## 10. Motion

Two curves: **out** `cubic-bezier(0.23, 1, 0.32, 1)` for enter and exit, **move**
`cubic-bezier(0.77, 0, 0.175, 1)` for on-screen movement. Durations ≤ 220 ms except where a
component hard-codes more (marked).

| What | Animation |
| --- | --- |
| Press (buttons, rows, toolbar) | Pressed fill at once on mouse-down; back over 120 ms, out. No scale (GPUI cannot; AppKit does not) |
| Hover fills | On at once, off over 100 ms |
| Message hover toolbar | Appears at once, no fade |
| Tooltip | 500 ms delay the first time, then instant for neighbors within 1 s; 120 ms opacity in |
| Context menu, select, popover from a click | 150 ms opacity + 4 pt travel from the trigger side, out. Exit 100 ms opacity |
| @mention popup | **None**. Appears and filters at once |
| Cmd+K switcher | **None** to open, close or select (`DialogEntrance::None`) |
| Sheets and alerts | `DialogEntrance::FadeSlide`: `duration_normal` 180 ms, `distance_short` travel, out. Scrim fades with it. Exit the same path back, 150 ms |
| Side panel | Width **snaps**; the panel content fades in over 120 ms, out. Same from mouse and ⌥⌘0 |
| New message from others | 150 ms opacity in place; the timeline stays pinned to the bottom |
| My sent message | None |
| Streaming bot text | Text appears as it arrives (no per-token fade, `stream_fade` off). The caret is solid while tokens arrive and starts blinking (530 / 530 ms) after 1 s without a token. "正在输出" is `ShimmerText`, 1.6 s linear loop |
| Typing dots | 1.2 s opacity pulse, dots 150 ms apart |
| Status chip change | 150 ms crossfade of fill and color |
| Jump pill (`n 条新消息`) | 150 ms fade + 4 pt up (`MessageScroller` jump-button transition 150 ms) |
| Toast (`Notification`) | gpui-component's built-in: 400 ms in, 200 ms out, ease. **Known deviation** (the constant is not themable in 0.8.1); in-app toasts are rare because notifications go to macOS when the app is inactive |
| Spinner | 0.8 s per turn, linear |
| Upload progress | Bar width follows the value with a 180 ms move transition |
| **Never animated** | List selection, keyboard navigation, topic or 群 switching, scroll position on open, counts and badges, tab and pane switches, sidebar collapse, theme change, window resize, the composer growing |

**Reduce motion** (`cx.reduce_motion()`): sheets and the side-panel content cross-fade in 150 ms
without travel; typing dots and shimmer are static (`正在输入…`, `正在输出`); the caret stays solid;
the jump pill has no travel. Custom `with_animation` code checks `cx.reduce_motion()` itself.

## 11. States

| State | Treatment |
| --- | --- |
| Hover | Message rows: `fill-hover` + the floating toolbar (quote, @, copy, more; level 2, 30 pt, top right at −14 pt). Sidebar and topic rows: no hover fill. Ghost buttons: `fill-hover` |
| Pressed | `fill-pressed`; primary `accent-pressed`. Immediate |
| Keyboard focus | 3 pt `focus-ring` outside the control. A focused list keeps its gray selection; the row under keyboard focus gets the ring inset 1 pt |
| Composer focus | Border `control-border` → `control-border-focus`; no ring and no blue (the caret shows focus) |
| Selected | Sidebar `sidebar-selected`; topic list `fill-selected`; popups, menus, switcher active row `fill-active` with `text-1` (never white on blue) |
| Disabled | 40 % opacity on the content; a disabled primary button is a `fill-pressed` fill with `text-3` label; no pointer change. Deactivated members: row at 50 % |
| Loading | Topic list: 5 `Skeleton` rows (`fill-field`, no shimmer) only after 300 ms. Timeline: none (cache); after 300 ms a centered 16 pt `Spinner` |
| Sending | The message shows at once, body at 60 % opacity, then `clock` 11 pt + "发送中" in `text-3` after the text; normal when the server confirms |
| Send failed | Under the body: `circle-alert` 13 pt + "发送失败" in `danger-text`, then text buttons "重试" (`accent-text`) and "删除" (`text-2`) |
| Edited | "（已编辑）" 11 pt `text-3` after the body |
| Deleted | One line "此消息已删除" 13 pt `text-3`, no actions |
| Offline | Sidebar band (§5.2) and a neutral bar above the composer: `wifi-off` + "网络已断开。新消息会在连上后发出。" The composer stays usable; queued messages show "发送中". No red, no modal |
| Reconnecting / reconnected | The bar shows `loader` + "正在重新连接…", then `circle-check` (success) + "已重新连接，补齐了 n 条消息" for 3 s, then hides |
| Unread | Topic: accent dot, title 600, gray count. 群: name 600 and gray count. Mention: accent `@n` instead of the count |
| Empty | §13.15 |
| Error loading | `Alert` row at the top of the timeline on `fill-field`: `circle-alert` (danger) + "没能加载更早的消息。" + `重试` small button |
| Field error | Under the field, 12 pt `danger-text`. Never an alert |

## 12. Worst-case data rules

| Slot | Rule |
| --- | --- |
| Sidebar name, topic title, toolbar title, group name, card title, file name in rows | One line, ellipsis at the end, full text in a tooltip |
| 产出 file names | Middle ellipsis that keeps the extension (`.text_ellipsis_middle()`) |
| Topic preview, 岗位, ack line, typing line, 待办 context | One line, ellipsis. 岗位 shrinks before the name (flex-shrink 2) |
| Chip | Name truncates at 104 pt; the verb stays |
| Counts | Unread `999+`, mention `@99+`, 待办 `99+`, filter tabs `999+`. Elsewhere thousands separators (`1,284`); never `1000000` |
| Message body | Wraps anywhere (character wrap when a word does not fit). Code never wraps: horizontal scroll with a 40 pt fade at the right edge |
| Topic title in the list | One line (titles with newlines show the first line; `\n` becomes a space) |
| Missing title | `（无标题）` in `text-2` |
| Composer | 7 lines, then scrolls inside; tools and send stay visible |
| Images | Max 360 × 320 pt, aspect kept; taller ones crop to the top with "查看原图" |
| Card | Titles and values one line + tooltip; at most 3 actions in a row (more go into `ellipsis`) |
| Names | First grapheme for monograms; emoji-first and mixed names show the first letter; RTL names render in their own direction inside an LTR row, badges stay trailing |
| Dates | Today `14:32`, yesterday `昨天`, this week `周一`, this year `11月2日`, older `2025年12月31日` |
| Literal text | `<script>` and Markdown-looking names render as text |
| Long typing line | `A、B 和另外 1,282 位正在输入…` |

## 13. Screens

Mockups for each are named in brackets (light and dark exist for each).

### 13.1 Main window [main, main-top, worst-case]

Sidebar | topic list | topic view | side panel, as §5. The selected topic sits in the selected 群.
DMs are 群 of two in the same list (no separate 私聊 section).

### 13.2 Sidebar

As §5.2. The 待办 rows open the topic. The `+` in the 群 header opens "新建群". Search opens
the switcher. The footer opens settings (the gear) or the member card (the avatar).

### 13.3 Topic list

As §5.3. The filter is per 群 and remembered. A context menu on a row: 标为已读, 关注 / 取消关注,
复制链接. Chips per §2.5. `open` topics have no chip.

### 13.4 Topic view header

As §5.4. The topic start (scrolled to the top) shows a 32 pt `message-square-plus` tile on
`fill-field`, the title 15 / 600 and `林晓 在「开发」发起了这个话题 · 今天 10:12` 12 pt `text-2`.

### 13.5 Timeline rows and message kinds

| Kind | Spec |
| --- | --- |
| Message | 32 pt avatar; meta row: name 13 / 600, 岗位 12 pt `text-2`, time 11 pt `text-3`; body 14 / 24 Markdown (`TextView`) |
| Continuation | Same author within 5 minutes: no avatar or meta, 2 pt top |
| Bot acknowledgement | One 20 pt row, 6 pt top: 20 pt bot avatar in the 32 pt gutter, `**设计助手** 已接手，开始做方案，大约 20 分钟。` 12.5 pt `text-2`, time 11 pt `text-3`. One line, ellipsis |
| Quote | 2 pt `quote-bar` on the left, 10 pt indent, author 12 / 600, text 13 / 19 `text-2`, max 2 lines |
| @mention | `mention-bg` / `accent-text`, radius 4, weight 500; of me: `mention-me-bg` / `mention-me-text` |
| Inline code | Menlo 12.5, `fill-field`, radius 4, 1 / 4 pt padding |
| Link | `accent-text`, no underline; underline on hover |
| Code block | `code-bg`, radius 8, max 560; header 28 pt with the language 11 pt `text-3` and a copy button; code 12.5 / 19, 12 pt sides, 10 pt bottom |
| Image | Radius 8, `image-border` ring, max 360 × 320 |
| 产出 card | Level-1 card, 10 / 12 padding: 40 pt tile (HTML: `accent-soft` + `file-code` in accent; others `fill-field` + `text-2`), name 13 / 600, meta 12 pt `text-2` (`HTML 页面 · 第 2 版 · 13:50`), `打开` small button (files: `在访达中显示`) |
| Card | §13.9 |
| Streaming reply | Body as it arrives + a 2 × 16 pt accent caret; under it `loader` 13 pt + `正在输出` shimmer, 12 pt `text-3`. Bots say 正在输出, people 正在输入 |
| Day separator | Centered 11 / 500 `text-3` (`昨天`, `11月2日`) |
| 新消息 divider | `Separator` with label "新消息" on the right, line `new-divider`, label `danger-text` 11 / 600; 14 pt above |
| Jump pill | Centered, 30 pt, radius 15, `elevated` + level 2, unread dot + `n 条新消息` 12 / 500 + `arrow-down`; 16 pt above the typing line |
| Typing | Three 4 pt dots + `陈思远 正在输入…` 12 pt `text-2` |
| States | Sending, failed, edited, deleted, emoji-only: §11 [states] |

### 13.6 Composer and its popups

As §5.4. Placeholder `在「登录页改版」里回复` (or `写下第一条消息，开始一个新话题` in a 群 with no
topic selected). Send is `accent` when there is text, else `fill-pressed` with `text-3`.

**@mention popup** (`Popover` + `List`, no animation): 300 pt wide, anchored 8 pt above the caret
line, left edge at the `@`. Header "成员" (caption). Up to 5 rows of 32 pt: 24 pt avatar, name
13 / 500 with the typed match underlined in 600, 岗位 12 pt `text-2`. Active row `fill-active`.
Footer 28 pt with a hairline: `↑ ↓ 选择`, `↵ 提及`, `esc 关闭` (kbd + 11 pt `text-2`). Bots and
people are mixed, sorted by recent activity in this 群.

**Quote**: a quoted message shows above the text inside the composer as a quote block with an
`x` to remove.

**Attachments**: chips above the text (`Attachment` small) with upload progress.

### 13.7 Side panel [side-panel]

`TabBar` segmented: `产出 n` and `过程`.

- **产出 tab**: sections "结论" (decision box: level 1, radius 8, 13 / 19 text, meta 11 pt
  `text-2` `林晓 定下 · 今天 14:10`), "产出" (rows 46 pt: 32 pt tile, name 13 / 500, meta 11 pt;
  uploading rows show `正在上传 · 38.2 / 61.5 MB` and a 3 pt progress bar and an `x`; failed rows
  show `上传失败 · 超过 100 MB` in `danger-text` and `重试`; use `Attachment` for these states),
  "谁在做什么" (`DescriptionList`: 负责 / 等待 / 关注 with 20 pt avatars).
- **过程 tab**: the working member (28 pt bot avatar, name 13 / 600, `正在工作 · 已用 18 分钟` or
  `卡住了 · 14:52 起没有进展` with 卡住了 in `st-blocked`), "步骤" list (16 pt glyph: `circle-check`
  success, `loader` accent, `circle-dashed` `text-3`, `circle-alert` danger; title 13 / 500; meta
  11 pt; a 1 pt `sep-strong` connector; a running step may show a 4 pt `Progress` 180 pt wide),
  then the machine box (`fill-field`, radius 8: `monitor` + `运行在 Mac mini（工作室）` + elapsed).
  When stuck: `我来处理` (primary small) and `让它重试`, then `查看完整记录` (text button).
- **Empty**: §13.15.

### 13.8 产出 window [artifact-window]

A separate native window per HTML 产出 (WKWebView, one origin, strict CSP, no network). Toolbar
52 pt: traffic lights, title 15 / 600 + sub line `开发 · 登录页改版 · HTML 页面`, then
`lock` + `离线页面` (11 pt `text-2`, tooltip `这个页面不能访问网络`), a version `Select`
(`第 3 版`), and a capsule with copy link, download, and the version-list toggle. The version list
(260 pt, `panel`) shows each version: 24 pt author avatar, `第 3 版` 13 / 500 + `当前` 11 pt
`text-2`, `设计助手 · 今天 14:30`, and the change note 12 pt `text-2`; the current version has
`fill-selected`. The page itself keeps its own colors in both modes.

### 13.9 Cards and blocks [cards]

A card is a level-1 surface on `content`, radius 10, no shadow, max 460 pt wide, 14 pt side
padding. Blocks stack with 10 pt between them.

| Block | Spec |
| --- | --- |
| status (header) | 16 pt glyph (`text-2`; `danger` for failures, `success` for done) + title 13 / 600 (one line) + optional chip at the trailing edge; optional summary 12 / 16 `text-2` |
| fields | `DescriptionList`, 2 pairs per row: label 56 pt `text-2`, value 13 pt (mono 12 for ids and branches), one line + tooltip |
| table | `Table` without stripes: header 11 / 600 `text-2`; rows 12.5 pt with 6 pt padding and a hairline above; numbers right aligned, tabular; max 8 rows, then `查看全部 n 行` |
| progress | Label 13 pt + `41 / 58` 12 pt `text-2` tabular on one line; 6 pt `Progress` track `fill-pressed`, fill `accent`, radius 3 |
| list | Rows with a 15 pt `circle-check` (success, text `text-2` when done) or `circle` (`text-3`) and 13 pt text, 6 pt apart |
| code | The code block of §13.5 inside the card; logs use the same style with the label `日志` |
| image | The image rules of §13.5, max card width; optional caption 12 pt `text-2` |
| actions | `Button::small()` row, 8 pt gap, at most 3 (first may be primary), more in `ellipsis`. After a choice, the row is replaced by `circle-check` + `Joye 选了「现在发布」 · 14:40` 12 pt `text-2` and the chip changes to 完成 |

Only a card that waits on the viewer carries a 等你… chip and a primary button.

### 13.10 Settings [settings-members]

`Settings` / `SettingPage` / `SettingGroup`. Pane list on `sidebar`: search field, then 我的资料,
成员, 机器, 通知, 成本 (`即将推出` 11 pt `text-3`), 诊断, a hairline, 开发者视图, and 退出登录 at
the bottom. Icons 16 pt `text-2`, no colored tiles; selected row `sidebar-selected`.

成员 pane: title 15 / 600 + `8 位` 12 pt `text-2`; buttons `添加自动成员` (secondary, `bot`) and
`邀请成员` (primary, `user-plus`). A filter field 240 pt and a `权限` column label. One grouped
section: rows 52 pt with a 28 pt avatar (bots rounded square + mark), name 13 / 500 (+ `（你）`),
岗位 12 pt `text-2` (bots add `· 运行在 Mac mini（工作室）`), status column 92 pt (`●在线`, `2 小时前`,
`已停用`, or a quiet `等待加入` chip), a 92 pt level `Select` (管理员 / 普通成员), and an `ellipsis`
menu (改岗位, 停用…, 移出…). Deactivated rows at 50 %.

### 13.11 Login [login]

Centered 440 × 580 window on `content`: 88 pt icon, `欢迎使用 {PRODUCT_NAME}` 22 / 600, lead
`工作在话题里推进，成员一起完成。` 13 pt `text-2`. Form 344 pt: label `服务器` (12 / 500 `text-2`)
+ 34 pt `Input` with a trailing `circle-check` in `success` once the server answers; label `邀请码`
+ `OtpInput::groups(3)` of 4 boxes (22 × 36, radius 7, Menlo 16 / 500, 4 pt gap, a `-` between
groups); hint `向管理员要一个邀请码。粘贴带横线或空格的格式也可以。` 12 pt `text-2`; a full-width
large primary `登录 ↵` that is disabled (quiet gray) until 12 characters are in. Errors go under
the code in `danger-text` (`这个邀请码无效或已用过。`). Footer `版本 0.4.2 · 登录后凭证保存在钥匙串里`
11 pt `text-3`.

### 13.12 Quick switcher [switcher]

`Command` inside a `Dialog` with `DialogEntrance::None` and no overlay. 640 pt, level 3, radius
14. Field 52 pt (`search` 18 pt, 18 pt text, `esc` kbd). Groups `话题`, `群和成员`, `消息` (caption
headers); rows 40 pt: a 24 pt tile (`message-square-plus` on `fill-field`, or the avatar), title
with the match in 600, chip, `开发 · 14:32` 12 pt `text-2`; active row `fill-active` + `↵` kbd.
Footer 36 pt: `↑ ↓ 选择`, `↵ 打开`, `⌘ ↵ 在新窗口打开`. With no query it lists recent topics.

### 13.13 Dialogs [states]

- **Sheet** (`Dialog`, FadeSlide, `margin_top(52)`): title 15 / 600, description 13 / 19 `text-2`,
  content, footer buttons right aligned (secondary then primary).
- **Invite code sheet**: code box 64 pt `fill-field` radius 10 with the code 24 / 500 Menlo in
  three runs (dashes `text-3`) and `复制 ⌘C`; `DescriptionList` (权限, 有效期, 次数); a note on
  `fill-field` with `eye` in `glyph-wait`: `邀请码只显示这一次，关闭后无法再查看。`; footer
  `撤销邀请` / `完成`.
- **Alert** (`AlertDialog`, 360 pt): destructive confirms use a `danger` filled button on the
  right (`停用`), `取消` on the left. The text says what stays and what is lost.

### 13.14 Banners and notifications [states]

- Bands (offline, reconnecting, reconnected) per §11: 30 pt, radius 8, `fill-field`, 12 pt
  `text-2`; never red.
- In-app toast (`Notification`, top right, 340 pt, radius 12, level 2): 32 pt avatar, title
  `林晓 @了你` 13 / 600, context `开发 · 登录页改版` 12 pt `text-2`, one line of the message, `x`.
  Only for the three quiet-notification cases of PRODUCT.md; when the app is inactive use the
  macOS notification instead (`delivery`).
- Menus (`ContextMenu`): 220 pt, 28 pt rows, 14 pt icons, shortcuts right in `text-3`, a hairline
  before destructive items, destructive text `danger-text`, active row `fill-active`.
- Tooltips: 26 pt, radius 6, `elevated` level 2, 12 pt text + `Kbd` for shortcuts
  (`侧边面板 ⌥⌘0`, `新话题 ⌘N`).

### 13.15 Empty and error states [empty-states]

`Empty` centered, max 280 pt: one 20 pt Lucide icon in `text-3`, title 13 / 600, description
12 / 17 `text-2`, optional small button. No illustrations.

| Where | Icon | Title | Description | Action |
| --- | --- | --- | --- | --- |
| No 群 | `users-round` | 还没有群 | 管理员把你加进群后，群会出现在侧边栏里。 | 新建群 (owners only) |
| 群 without topics | `message-square-plus` | 这个群还没有话题 | 写下第一条消息就会开始一个话题，标题默认取第一行。 | |
| 待办 empty | `circle-check` | 没有等你的事 | 有人 @你 或话题等你回答、等你审时会出现在这里。 | |
| Search | `search-x` | 没有找到「…」 | 只搜了「开发」。 | 在所有群里搜索 ⌘↵ |
| 产出 empty | `layers` | 还没有产出 | 成员上传的文件、页面和定下的结论会出现在这里。 | |
| 过程 empty | `activity` | 还没有成员在这个话题里工作 | @ 一位自动工作的成员，它的步骤会显示在这里。 | |
| Notifications | `bell` | 都看过了 | 只在有人 @你、话题等你、或你关注的话题有结果时通知你。 | |
| Load failure | | | `Alert` row (§11) | 重试 |

All strings come from the copy catalog; the catalog test checks the four nouns.

### 13.16 Menu bar item

A template (monochrome) 16 pt version of the icon's brackets and circle; a dot appears at the top
right when something waits on you. Its menu lists 待办 items and `打开 {PRODUCT_NAME}`.

## 14. Members: `MemberAvatar`

One app component, used everywhere a member appears. It wraps gpui-component `Avatar`:

- Photo: `Avatar::new().src(url)`; bots add `.rounded(px(size * 0.28))` (Avatar is `Styled`
  and passes corner radii to its image and fallback).
- No photo: `Avatar`'s initials and `IdentityColor` cannot follow our rules (周子航 gives 周, and
  its palette is its own), so the wrapper draws the monogram box itself with §2.7 palettes, at
  the same size and radius, inside the same `flex_none` box.
- Monogram: Chinese name at 28 pt and up: a person shows the last two characters (林晓, 思远),
  a bot the first two (设计, 构建); below 28 pt both show the first character. Latin: initials of
  the first and last word (AW). Mixed or emoji-first: the first letter (`🦊 Fox 狐狸` → F). One
  character: itself. Font: 34 % of size for two Han characters, 44 % for one, 38 % for Latin;
  weight 500.
- Overlays (absolute children): the bot mark (§2.6) and the presence dot. The ring color is the
  surface the avatar sits on (pass it in).
- Sizes: 32 (timeline), 28 (settings, panel header, member cards), 26 (sidebar footer), 24
  (popups, switcher, version list), 22 (toolbar stack), 20 (sidebar, ack line, panel rows).
- Character avatars (decided by Joye after these mockups were made): a member with a
  `character` (a jojo-friends id; see docs/PRODUCT.md and docs/ROSTER.md) shows that character's
  art instead of a monogram. The character's silhouette is the bot shape (no radius override,
  no extra bot mark), and its status signal replaces the presence dot: idle, working,
  needs-input, success, error or offline, from the mapping in `apps/desktop/src/characters/`.
  Below 32 pt use the small-size renders. People keep photos or monograms with the presence dot;
  a bot without a character keeps the monogram and the bot mark. Wherever a mockup shows
  monogram bots (设计助手, 构建助手, 周报助手), the app shows the roster bots (Jojo, Tata, Forge
  and the others) with their characters.

## 15. App icon [icon, design/brand/]

「 ● ■ 」: Chinese corner quotation marks around a circle (a person, in the accent: "color means
you") and a rounded square (a bot), set like quoted speech. People and bots, equal, in one
exchange. The circle sits a little higher than the square, following the brackets' diagonal.

- Grid: 1024 canvas, 824 body at (100, 100), radius 186; body gradient `#FFFFFF` → `#E9E9EE`
  (dark: `#3A3A3E` → `#1F1F22`), 1.5 px inner hairline, a soft drop shadow (y 10, blur 12, 20 %).
- Brackets: round caps and joins, stroke 60, ink `#2C2C30` (dark `#F2F2F5`). Circle: r 92,
  `#3B9BFF` → `#0A74F0`. Square: 180, radius 50, `#56565C` (dark `#9A9AA1`).
- Optical variant below 64 px (`icon-small.svg`): stroke 84, circle r 106, square 204.
- Files: `design/brand/icon.svg` (master), `icon-dark.svg`, `icon-small.svg`,
  `icon-small-dark.svg`, PNG renders `icon-{1024,512,256,128,64,32,16}.png` and
  `icon-dark-{1024,128,64,32,16}.png`. The DMG ships a static `.icns` built from these.

## 16. GPUI limits and the fallback for each

| Wanted | Limit | Fallback |
| --- | --- | --- |
| Blur behind popovers or an inset sidebar | Only window-level `Blurred` | Opaque `elevated` surfaces; full-height sidebar; vibrancy is an option (§8) |
| Letter-spacing | None | Core Text's own tracking; the invite code is three runs |
| `scale` on press | No div transforms | Pressed fill color (AppKit behavior) |
| System accent color | Not read by GPUI | Read `NSColor.controlAccentColor` with `objc2` at start and on `NSSystemColorsDidChangeNotification`; rebuild the accent tokens. Until then system blue |
| 0.5 pt hairlines | Exact only at 2× | On 1× displays draw 1 px at the same alpha |
| SF Mono | Not public | Menlo |
| Continuous corners | Circular arcs only | Circular radii |
| Toast timing | `Notification` hard-codes 400 / 200 ms | Accepted (§10); ask upstream for themable timing |
| Middle truncation | Available | `.text_ellipsis_middle()` on file names |
| Edge fades | No masks | `linear_gradient` overlays; `MessageScroller::with_bottom_fade` |
| Layered macOS 26 icons | `.icns` only | Static icon; an Icon Composer file can come later |

## 17. Design check procedure

Before merging any UI change:

1. Build with your own target dir and run the screenshot self-test:
   `desktop --screenshot <dir> --theme both` and again with `--worst-case`; also
   `--size 1100x680` once. Read every PNG of the scenes you touched (scene list:
   `desktop --list-scenes`; add a `Scene` in `apps/desktop/src/selftest/scenes.rs` when your
   screen has none, for example `switcher`, `side-panel`, `cards`, `states`).
2. Compare with the mockup of the same name in `docs/design/mockups/` and with the baseline
   from the previous run. Differences must be deliberate.
3. Check with the vendored skills in `docs/design/skills/emilkowalski/`:

| Skill | Checks here |
| --- | --- |
| `emil-design-eng` | Tokens only, no literals; tabular numbers; radii nest; no ease-in; nothing appears from scale 0; keyboard actions do not animate; tooltips instant after the first; durations ≤ 220 ms |
| `apple-design` | Familiar macOS structure (traffic lights, columns, sheets from the title bar); feedback on mouse-down; no translucent layer on another; reduce motion and reduce transparency handled; wayfinding (selected 群 and 话题 visible) |
| `review-animations` + `STANDARDS.md` | Every animation is in the §10 table with its duration and curve; frequency rule (switcher, @ popup, selection: none); no layout animation (side panel snaps) |
| `break-ui` + `CATALOG.md` | `--worst-case` screenshots against §12: no overflow, no overlap, counts capped, chips keep their verb, RTL and emoji names, long URLs wrap |
| `ui-cheatsheet.md` | Measure ≤ 560 pt, old dates absolute, avatars never squish |

4. Light and dark, worst case, and the narrow window all pass, and `docs/design/CHECKLIST.md`
   items for the touched screens are ticked in the PR description.

## 18. What implementers must watch

- **Theme font size 13 vs the 16 default.** Check that `Button`, `Input`, `TabBar` and `List`
  internal sizes still match §5 in screenshots; set sizes with `.small()` / `.with_size()` where
  they do not.
- **Re-apply `MotionTokens` and in-code overrides after every `Theme::change`** (a mode switch
  reloads the config).
- **`MemberAvatar`** must be the only avatar path, or bots will appear as circles.
- **Chips** use `Tag::custom(bg, fg, border)` with the glyph as a child; the quiet chip uses a
  transparent `bg` and `chip-ring` as `border`. No `TagVariant::Warning` and friends.
- **No accent selection.** gpui-component's default `list_active` and menu highlight must be
  overridden as in §3, or blue rows come back.
- **Side panel**: no width animation; fade only the content.
- **Caret**: solid while streaming; blink only after 1 s idle.
- **Vibrancy stays off** until measured (§8).
- **Copy**: all strings from the catalog; `{PRODUCT_NAME}` from `product.json`; the four nouns
  only. The mockups' Chinese is the intended copy.
- **Things the mockups cannot prove**: PingFang/SF weight matching at 500/600, hairline rendering
  at 1×, shadow softness and Menlo for codes are judged in GPUI screenshots, not in Chrome.
