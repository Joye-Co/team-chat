# Design v2.2

**v2.1 is the production revision of v2.** v2 replaced v1 ("Native calm"), which Joye found ugly
with too many basic UI and UX faults. Joye then judged the v2 mockups too small and cramped
("为什么这些输入框都那么的矮？"). v2.1 keeps every v2 decision about structure and behavior and
changes the sizes: a 96 pt composer, 36 to 40 pt fields, 32 pt buttons, 14 pt list titles, no
reader text under 12 pt, fewer colors. The structure of this file follows
[design/DESIGN-MD-GUIDE.md](design/DESIGN-MD-GUIDE.md) (tokens with roles, components with all
states, accessibility, motion, dark mode, density, checklist).

**The bar.** The app must look as good as Slack, Feishu 话题群 and GrokBot when the three are side
by side at the same scale. "Acceptable" is not the bar.

**v2.2** records what shipped after the v2.1 build and the coordinator's acceptance decisions;
the list is at the end ("Changes in v2.2").

Implementers follow this file exactly. When a value is not here, ask before you invent one.
AGENTS.md and docs/PRODUCT.md win for product rules and interface words. This file wins for how
things look and move.

| What | Where |
| --- | --- |
| Mockups (PNG at device scale 1 and `@2x`, HTML beside each) | [design/mockups-v2/](design/mockups-v2/) |
| Rebuild the mockups | `bun docs/design/mockups-v2/src/build.mjs [name ...] [--scale=1\|2]` (both scales by default) |
| Tokens in code form (the one source for the mockups) | [design/mockups-v2/src/tokens.mjs](design/mockups-v2/src/tokens.mjs) |
| Component CSS for the mockups | [kit.mjs](design/mockups-v2/src/kit.mjs) (shared), [main.mjs](design/mockups-v2/src/main.mjs) (main window), [screens.mjs](design/mockups-v2/src/screens.mjs) |
| App icon (`icon-dark.svg` on the dark login) | [design/brand/](design/brand/) |
| GPUI limits | [design/GPUI-CAPABILITIES.md](design/GPUI-CAPABILITIES.md) |
| Per-change checklist | §17 of this file (it replaces design/CHECKLIST.md where they differ) |

Units: pt. A web px at device scale 1 equals one macOS pt.

### References

| Name | What we used | Evidence (not in git) |
| --- | --- | --- |
| **Slack** | 2023 desktop layout: message rows, hover toolbar, reactions, thread pane, sidebar sections, unread by weight, the composer with a toolbar row, Block Kit task cards | `~/.cache/team-chat/design/refs/slack/` (crop-stanford-main.png, crop-thread-pane.png) |
| **Feishu** | 话题群 and 话题回复: topic list, topic pane, tabs, card spec, neutral grey ramp, 36 to 40 pt fields | `~/.cache/team-chat/design/refs/feishu/` (hc_360049067735_*.png, hc_423295624296_*.png, open_cards_*.png) |
| **GrokBot** | Bots as characters, one pending row, work cards, the ask card with lettered options, the draft card, the right pane with the bot's live state | `~/.cache/team-chat/design/refs/grokbot/` |
| **OpenBot** | Measurements only (PolyForm Noncommercial: no code, styles or assets copied): one header height, an enforced type scale, neutral selection, one status per row, collapsed tool steps, 400 ms delay before busy indicators | `~/.cache/team-chat/design/refs/openbot/` |
| **Open-source chat** | Mattermost, Zulip, Rocket.Chat Fuselage, Element Compound, Zed (measured from source; see the guide §4): 98 pt empty composer, 32 to 40 pt controls, 40 to 48 pt fields | `~/.cache/team-chat/design/refs/oss/` |

## 0. Decisions per screen

| Screen | We follow | Why | Rejected |
| --- | --- | --- | --- |
| Shell | **Slack / Feishu**: tinted sidebar, white content, one hairline between columns, one header band per pane whose bottom rules share one y at 92 pt (**OpenBot**, Slack) | Proven layout; the tint separates navigation from content without boxes | Slack's icon rail and purple chrome; Feishu's app-suite nav; GrokBot's glass sidebar |
| Sidebar | **Slack** sections (待办, 群, 私聊), a monochrome `#` glyph for each 群, unread by weight, red badges only for what is for you; **GrokBot** bot rows show the character | Scanning bold names is faster than scanning colored tiles | Colored letter tiles per 群 (v2: a rainbow of five hues looked like a template) |
| 群 → topic list | **Feishu** 话题群 model (topics, 全部 / 未读 / 等我 tabs) drawn as **Slack / Element** two-line rows | Dense rows triage faster than Feishu's large cards at 5 to 30 topics | Full topic cards; Feishu's floating `+` |
| Topic view | **Feishu** pane order (topic start, replies, composer) + **Slack** message rows | No bubbles in a team topic | GrokBot / OpenBot bubbles (1:1 bot chat only) |
| Bots and live work | **GrokBot** pending row and character; **OpenBot** elapsed time and collapsed steps; **Slack** agent states with a visible stop | "Acknowledges once, works quietly" (PRODUCT.md) | A tool log in the timeline; shimmer text |
| Cards | **Feishu** card spec + **GrokBot** work, ask and draft cards + **Slack** Block Kit plan list | Every bot card looks native | Saturated Feishu header gradients |
| Composer | **Slack / Feishu / Mattermost**: a writing area of at least two lines and a toolbar row (attach, @, emoji, format at the left; send at the right); the destination in the placeholder (**Feishu**) | Joye's requirement; topic replies are long; the toolbar teaches the actions | v2's single 40 pt row (Joye: "丑死了"); Slack's always-on format bar (it opens from `Aa`) |
| Side panel | **GrokBot** right pane: the responsible bot's live state, 产出, members; closed by default | Context one glance away, no width when empty (U07) | v1's always-open empty panel |
| Search | **Feishu** `搜索 ⌘K` field that opens one switcher; **Slack** grouped results | One search entry (U35) | A separate search dialog |
| Settings | **Slack** preferences: section list at the left, a centered content column, grouped tables | Flat, no nested pages | Solid accent selection; a raised table header |
| Login | A centered form in a full window, label above the field, invite code as boxes | Interns never need the server address (U39) | A small fixed window with the form at the top |

## 1. Principles

Ranked: when two conflict, the higher one wins.

1. **Readable first.** Body 14/22, list titles 14, nothing a reader must read under 12, controls
   32 to 40 pt. When density and readability conflict, readability wins. (Joye's feedback.)
2. **Copy what works.** Every pattern names its reference. A new pattern needs a reason that the
   references cannot cover (our 话题 status and `waitingOn` are the main one).
3. **Color means you.** Accent blue: links, mentions of others, the active field, send, primary
   buttons, the unread dot. Orange: 等你… and mentions of you, **at most once per pane**. Red:
   number badges for you and errors. Everything else is neutral, including 群 glyphs, avatars of
   people, 完成 and 等{name}… labels.
4. **Subtract.** One search entry, one header height, one chip shape, one overlay, one avatar
   rule, one date format. When two controls say the same thing, remove one. A state shows once
   per pane.
5. **Every interactive thing answers.** Hover, pressed, focus-visible, disabled and cursor on
   every row and control (§7.1). This was the most common v1 fault (U03, U04).
6. **Members are equal.** People and bots use the same rows and sizes. Bots look like their
   jojo-friends character.
7. **Quiet motion.** Short, ease-out, never on keyboard navigation (§12).

## 2. Color tokens

Values are sRGB `#RRGGBBAA` (the gpui-component theme JSON format). An alpha color is drawn over
the surface named in its role. `L` light, `D` dark. Contrast is WCAG 2.2, computed over the named
surface (`content` unless stated). Views never write a color literal; text tiers are tokens,
never opacity.

### 2.1 Surfaces

| Token | L | D | Role |
| --- | --- | --- | --- |
| `sidebar` | `#F2F3F5FF` | `#18181BFF` | Sidebar, settings section list |
| `content` | `#FFFFFFFF` | `#1F1F23FF` | Topic list, topic view, side panel, login, settings content |
| `raised` | `#F6F7F9FF` | `#28282DFF` | Cards |
| `overlay` | `#FFFFFFFF` | `#34343BFF` | Popovers, menus, @mention popup, switcher, dialogs, hover toolbar, toasts |
| `scrim` | `#0F111452` | `#0000008C` | Behind every overlay that takes focus (switcher, dialogs, sheets, alerts) |
| `code-bg` | `#F4F5F7FF` | `#26262BFF` | Inline code, code blocks, logs |

### 2.2 Text

| Token | L | D | Contrast L / D | Role |
| --- | --- | --- | --- | --- |
| `text-1` | `#1F2329FF` | `#EDEDF0FF` | 15.8 / 14.1 | Body, names, titles |
| `text-2` | `#5F6670FF` | `#A5A9B1FF` | 5.8 (5.2 on `sidebar`) / 7.0 | Previews, 岗位, secondary labels, icons at rest |
| `text-3` | `#70767FFF` | `#8A8F98FF` | 4.6 (4.1 on `sidebar`) / 5.1 | Times, counts in tabs, section labels, hints |
| `placeholder` | `#9AA0A8FF` | `#6C7079FF` | 2.6 / 3.3 | Field placeholders only (the field label or the destination says the same thing) |

### 2.3 Lines and fills

| Token | L | D | Role |
| --- | --- | --- | --- |
| `sep` | `#1F232917` | `#FFFFFF14` | 0.5 pt hairlines: column edges, header band bottom rules, table rows, date divider |
| `border` | `#1F232924` | `#FFFFFF21` | 1 pt: composer and fields at rest, cards, files, option rows, selects |
| `border-strong` | `#1F23293D` | `#FFFFFF38` | 1 pt: hover on composer and fields, secondary buttons, hovered option |
| `hover` | `#1F23290A` | `#FFFFFF0A` | Hover on content rows, messages, ghost buttons |
| `selected` | `#E3ECFEFF` | `#23324FFF` | Selected topic row; keyboard-active row in popups, menus and the switcher. A tint of `accent`, so selection never looks like hover |
| `sidebar-hover` / `sidebar-selected` | `#1F23290D` / `#DCE6FAFF` | `#FFFFFF0B` / `#22304BFF` | Sidebar and settings list rows |
| `sel-glyph` | `#175CD9FF` | `#7AA8FFFF` | The `#` glyph of the selected 群 row |
| `pressed` | `#1F23291F` | `#FFFFFF1F` | Pressed rows and ghost buttons |
| `field` | `#1F23290E` | `#FFFFFF11` | Search fields, kbd, role tag, reactions, empty send button, disabled select |
| `count-bg` / `count-text` | `#E6E8ECFF` / `#4B5058FF` | `#3A3E46FF` / `#C9CDD4FF` | Grey count in tabs and section headers |
| `count-solid` | `#697079FF` (white on it 5.0) | `#5C6370FF` (6.1) | Neutral unread badge: unread topics and DMs without a mention (§7.8) |
| `tile-ring` | `#FFFFFFFF` | `#FFFFFF1A` | 1 pt ring around a bot tile on a selected row, so the tile keeps its edge on the selection fill |
| `opt-bg` / `opt-hover` | `#F4F5F7FF` / `#ECEEF1FF` | `#2A2D33FF` / `#32353CFF` | Option rows in ask cards (no border) |
| `disabled-bg` / `disabled-text` | `#1F232914` / `#8A8F98FF` | `#FFFFFF0F` / `#8A8F98FF` | Disabled primary button: a neutral fill, never `accent` at reduced opacity. Translucent, so it reads one step above every surface (`content`, `overlay`) and never as a darker hole |
| `tile` | `#EBEDF0FF` | `#34343BFF` | Group tile in dialogs |
| `desk` | `#E9EBEFFF` | `#0E0F12FF` | Mockups only: the desk behind a framed window (settings, login, worst case) |

### 2.4 Accent, focus and mentions

| Token | L | D | Role |
| --- | --- | --- | --- |
| `accent` | `#1F6BFFFF` | `#2F6FEBFF` | Send, primary buttons, caret, my reaction ring, progress bars. White on it: 4.6 / 4.6 |
| `accent-hover` | `#165EEBFF` | `#3D7EFFFF` | Primary hover |
| `accent-text` | `#175CD9FF` (5.9) | `#7AA8FFFF` (6.9) | Links, @mentions of others, text buttons |
| `accent-soft` | `#1F6BFF1A` | `#4C8DFF29` | File tiles, my reaction |
| `focus` | `#1F6BFF8C` | `#4C8DFFA6` | 2 pt keyboard focus ring with a 2 pt gap (§11) |
| `focus-border` | `#1F6BFFFF` (4.6) | `#4C8DFFFF` (5.1) | 1 pt border of the focused composer and fields |
| `focus-soft` | `#1F6BFF29` | `#4C8DFF3D` | 3 pt outer ring of the focused composer and fields |
| `choice-bg` | `#1F6BFF0F` | `#4C8DFF1F` | The chosen option of an ask card (with a 1 pt `accent` border) |
| `mention-bg` | `#1F6BFF1A` | `#4C8DFF2E` | @mention of someone else (text `accent-text`) |
| `me-bg` / `me-text` | `#FF8A002E` / `#9A4A00FF` | `#FF9F0A38` / `#FFB547FF` | @mention of me |
| `hl-bg` | `#FF8A002E` | `#FF9F0A3D` | Search match mark in the switcher (every row type), text keeps its color |
| `unread-dot` | `#1F6BFFFF` | `#4C8DFFFF` | 6 pt dot left of an unread topic row |

The accent is our own blue; it does not follow the macOS accent color.

### 2.5 Status tones

| Token | L | D | Role |
| --- | --- | --- | --- |
| `you-bg` / `you-text` / `you-glyph` | `#FFE9D1FF` / `#9A4A00FF` (5.3) / `#E07800FF` | `#4A3418FF` / `#FFB547FF` (6.7) / `#FF9F0AFF` | 等你回答, 等你审 chip; bot pip "waits for someone". The fill is opaque so it does not mix with the `selected` fill under it |
| `you-bar` | `#F5A623FF` | `#F5A623FF` | 3 pt left bar of a card that waits for the viewer (the card border stays `border`) |
| `info-bg` / `info-text` | `#1F6BFF1A` / `#175CD9FF` | `#4C8DFF2E` / `#7AA8FFFF` | 进行中 chip (topic header only) |
| `danger` / `danger-bg` / `danger-text` | `#E5484DFF` / `#E5484D1F` / `#C2272DFF` (5.8) | `#F2555AFF` / `#F2555A2E` / `#FF7B7FFF` (6.6) | 卡住了, failed send, field errors, danger buttons |
| `success` / `success-text` | `#1E9E5AFF` / `#137042FF` | `#3CCB7FFF` / `#5BD893FF` | Done steps in plans, 已处理 in alert cards |
| `badge` | `#D93A40FF` | `#D93A40FF` | Red badges: @ mentions only. White 12/600 on it: 4.5 |
| `online` | `#1FB25AFF` | `#34C76AFF` | Presence dot |

### 2.6 People without a photo, and bot tiles

One neutral pair for every person: `avatar-bg` `#E6EAF2FF` / `#364052FF`, `avatar-fg`
`#3E4C63FF` / `#D3DCEAFF` (7.4 / 8.2). A photo replaces it when the person has one.

Bot tiles: six mid tints, one per bot by a stable hash of the character id (FNV-1a 32-bit,
offset basis `2166136261 ^ 91`, mod 6; this seed gives Pocket, Forge, Watch, Scout, Ledger and
Taro six different hues): `bot-1` to `bot-6` light `#DCE6FB` `#DDF1E6` `#FBE9D3` `#EFE3FA`
`#DDEFF4` `#F7DDE4`, dark `#6B7DA6` `#62907B` `#A08562` `#8472A8` `#6293A1` `#A0717F`. The dark
tints are mid tones, lighter than every dark surface, because the characters are drawn in navy
`#2D3272`: on a darker tile (the round-3 proposal `#2B3A57`) the art disappears. v2's eight hashed palettes are removed: with the bot
characters and status colors they made more than ten hues on one sidebar. The initial and the
name tell people apart.

### 2.7 Elevation colors

See §5. Syntax colors stay the Xcode defaults through `HighlightTheme`.

### 2.8 gpui-component theme mapping

One JSON theme set (`apps/desktop/assets/theme.json`, `light` and `dark`). App-owned roles (tones,
mentions, presence, `count-bg`, `count-text`, `sel-glyph`, `opt-*`, `disabled-*`, `bot-1` to `bot-6`, `tile`, `tile-ring`, `focus-*`, `choice-bg`, `you-bar`, `count-solid`, `hl-bg`, avatar colors,
shadows) live in `theme::Colors`.

| Slot(s) | L | D |
| --- | --- | --- |
| `background` / `foreground` | `#FFFFFFFF` / `#1F2329FF` | `#1F1F23FF` / `#EDEDF0FF` |
| `muted` / `muted_foreground` | `#1F23290E` / `#5F6670FF` | `#FFFFFF11` / `#A5A9B1FF` |
| `border` / `input` | `#1F232924` / `#1F232924` | `#FFFFFF21` / `#FFFFFF21` |
| `ring` | `#1F6BFF8C` | `#4C8DFFA6` |
| `selection` / `caret` | `#1F6BFF33` / `#1F6BFFFF` | `#4C8DFF5C` / `#2F6FEBFF` |
| `overlay` | `#0F111452` | `#0000008C` |
| `accent` / `accent_foreground` (highlighted row) | `#1F232914` / `#1F2329FF` | `#FFFFFF16` / `#EDEDF0FF` |
| `primary` / `_hover` / `_active` / `_foreground` | `#1F6BFFFF` / `#165EEBFF` / `#1252CCFF` / `#FFFFFFFF` | `#2F6FEBFF` / `#3D7EFFFF` / `#2A63D4FF` / `#FFFFFFFF` |
| `secondary` / `_hover` / `_active` / `_foreground` | `#FFFFFFFF` / `#F6F7F9FF` / `#ECEDF0FF` / `#1F2329FF` | `#FFFFFF14` / `#FFFFFF1F` / `#FFFFFF29` / `#EDEDF0FF` |
| `button*` / `button_primary*` | as `secondary*` / as `primary*` | same |
| `danger` / `_hover` / `_active` / `_foreground` | `#E5484DFF` / `#D93F44FF` / `#C2272DFF` / `#FFFFFFFF` | `#F2555AFF` / `#F66A6EFF` / `#D9474CFF` / `#FFFFFFFF` |
| `success`, `warning`, `info` | `#1E9E5AFF`, `#E07800FF`, `#1F6BFFFF` | `#3CCB7FFF`, `#FF9F0AFF`, `#2F6FEBFF` |
| `list` / `list_hover` / `list_active` / `list_active_border` | `#FFFFFFFF` / `#1F23290A` / `#1F232914` / `#00000000` | `#1F1F23FF` / `#FFFFFF0A` / `#FFFFFF16` / `#00000000` |
| `table` / `table_hover` / `table_active` / `table_row_border` / `table_head` / `table_head_foreground` | `#FFFFFFFF` / `#1F23290A` / `#1F232914` / `#1F232917` / `#FFFFFFFF` / `#70767FFF` | `#1F1F23FF` / `#FFFFFF0A` / `#FFFFFF16` / `#FFFFFF14` / `#1F1F23FF` / `#8A8F98FF` |
| `popover` / `popover_foreground` | `#FFFFFFFF` / `#1F2329FF` | `#34343BFF` / `#EDEDF0FF` |
| `sidebar` / `_foreground` / `_border` / `_accent` / `_accent_foreground` | `#F2F3F5FF` / `#5F6670FF` / `#1F232917` / `#1F232917` / `#1F2329FF` | `#18181BFF` / `#A5A9B1FF` / `#FFFFFF14` / `#FFFFFF17` / `#EDEDF0FF` |
| `tab` / `tab_active` / `tab_active_foreground` / `tab_foreground` / `tab_bar` | `#00000000` / `#00000000` / `#1F2329FF` / `#5F6670FF` / `#00000000` | same with dark text tokens |
| `switch` / `switch_thumb` | `#1F23293D` (off), `accent` on / `#FFFFFFFF` | `#FFFFFF38` / `#FFFFFFFF` |
| `slider_bar`, `progress_bar` | `#1F6BFFFF` | `#2F6FEBFF` |
| `skeleton` | `#1F23290E` | `#FFFFFF11` |
| `scrollbar` / `_thumb` / `_thumb_hover` | `#00000000` / `#1F232940` / `#1F232966` | `#00000000` / `#FFFFFF40` / `#FFFFFF66` |
| `link` / `_hover` / `_active` | `#175CD9FF` / `#1250BFFF` / `#0E449FFF` | `#7AA8FFFF` / `#94BAFFFF` / `#AECBFFFF` |
| `blue` / `blue_light` | `#1F6BFFFF` / `#1F6BFF1A` | `#2F6FEBFF` / `#4C8DFF2E` |
| `yellow` / `yellow_light` (the `you` tone) | `#9A4A00FF` / `#FFE9D1FF` | `#FFB547FF` / `#4A3418FF` |
| `red` / `red_light` | `#C2272DFF` / `#E5484D1F` | `#FF7B7FFF` / `#F2555A2E` |
| `green` / `green_light` | `#137042FF` / `#1E9E5A1F` | `#5BD893FF` / `#3CCB7F29` |
| `magenta`, `cyan` (+ `_light`) | not used; `blue` values | same |
| `window_border`, `drag_border`, `drop_target` | `#1F232924`, `#1F6BFFFF`, `#1F6BFF1A` | `#FFFFFF21`, `#2F6FEBFF`, `#4C8DFF2E` |

Theme fields: `font.family` `.SystemUIFont`, `font.size` 14, `mono_font.family` `Menlo`,
`mono_font.size` 13, `radius` 8, `radius.lg` 12, `shadow` true, `focus_ring` true,
`scrollbar_mode` `Scrolling`. `MotionTokens`: §12.

## 3. Typography

System font (`.SystemUIFont`, SF Pro); Core Text falls back to **PingFang SC** for Han and to
**Apple Color Emoji** for emoji. Mono: Menlo (code only). Weights 400, 500, 600 only. Tabular
numerals on every number. Glyphs render without font-smoothing dilation (no faux-bold stroke
widening on light or dark text), so text has the same stroke weight as the mockups (Chrome
renders them without it). A vendored gpui patch that does this is listed in docs/ARCHITECTURE.md.

**Closed scale.** Ten styles. Each style is size, line height and weight together. Nothing else
is allowed (U27). There is no 11 pt text.

| Token | Size / line | Weights | Where |
| --- | --- | --- | --- |
| `display` | 22 / 30 | 600 | Topic start title (2 lines max), settings page title, login title, empty-state title |
| `heading` | 18 / 26 | 600 | The topic view title: the one primary title of the window |
| `title` | 16 / 24 | 600 | Topic list header (群 name), side panel header, dialog title |
| `card-title` | 15 / 22 | 600 | Card titles (ask card question, `发布预览`): one step over the body, so a card does not read as one more paragraph (Feishu) |
| `body` | 14 / 22 | 400; 600 lead-ins and sender names | Message text, composer text, card text, option rows, names in messages |
| `ui` | 14 / 20 | 400; 500 list titles; 600 unread and selected | Sidebar rows, topic row titles, tabs, buttons, menus, fields, popup rows, settings rows |
| `small` | 13 / 20 | 400; 500 field labels | Topic row previews, live line, steps row, plan steps, card subtitles in panels, inline buttons, role text in popups, footnotes, switcher snippets |
| `meta` | 12 / 16 | 400; 500 chips, badges' neighbors, section labels; 600 badges; 500 sidebar section headers | Times, chips, badges, kbd, role tag, 待办 context, file metadata, typing line, field help and errors |
| `code` | 13 / 20 | 400 Menlo | Inline code (12.5 is not allowed), code blocks, logs |
| `code-entry` | 20 / 28 | 500 system UI, tabular figures, letter spacing 0 | Invite code boxes only (SF Pro, not a terminal font: Apple 2FA) |

Rules:
- List titles are at least `ui` 14. Meta is at least 12. A reader never meets text under 12.
- A name in a message has the body size (14) at 600, not a caption size (Slack).
- Steps: display 22 > heading 18 (topic) > title 16 (list, panel) > card-title 15 > body 14. Only the topic title is 18, so the window has one primary title.
- Line height 22 for 14 pt body is Feishu's and Slack's; PingFang's ascent fits in 22.

## 4. Spacing, layout and sizes

### 4.1 Spacing

4 pt base. Steps: 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 44. Group related things with 4 to 8;
separate groups with 12 to 16; separate sections with 24 or more. 10 and 14 are only for row and
card insets. No other values.

### 4.2 Control sizes

| Control | Height | Text | Horizontal padding | Radius | Icon |
| --- | --- | --- | --- | --- | --- |
| Button `sm` (inline in a message or card row: 重试) | 28 | `small` 13/500 | 10 | 6 | 14 |
| Button `md` (default: card actions, dialogs) | 32 | `ui` 14/500 | 14 | 8 | 16 |
| Button `md` page primary (settings `添加成员 ▾`) | 36 | `ui` 14/500 | 14 | 8 | 16 |
| Button `lg` (login, empty-state primary, full-width form submit) | 40 | 15/20 500 (the one size outside the scale, buttons only) | 18 | 10 | 16 |
| Icon button | 32 × 32 | — | — | 8 | 18 |
| Icon button `sm` (section `+`, inside rows) | 28 × 28 | — | — | 6 | 16 |
| Search and filter field | 36 | `ui` 14 | 12 left, 8 right | 8 | 16 |
| Form field (login, settings, dialogs) | 40 | `ui` 14 | 12 | 10 | 16 |
| Select in a table row | 36 × 120 | `ui` 14 | 12 left, 8 right | 8 | chevrons 14 |
| Invite code box | 42 × 48 | `code-entry` | — | 10 | — |
| Composer | 96 minimum | `body` | 14 | 12 | 18 |

Hit area is at least 28 × 28 for anything clickable; 32 where space allows.

### 4.3 Icons and avatars

- Icons (Lucide, the gpui-kit set plus `type`): 18 in headers, toolbars and the composer; 16 in
  buttons, fields, rows and menus; 14 in chips and labels; 12 in the role tag and section
  chevrons. Stroke 1.75.
- Avatars: 36 in messages and topic rows; 32 in the sidebar footer; 24 in sidebar rows, popups,
  the switcher, the side panel and the live line; 36 in settings rows and the side-panel progress.

### 4.4 Main window

| Metric | Value |
| --- | --- |
| Window | Min 1100 × 680, default 1440 × 900, transparent title bar, traffic lights at (20, 20) |
| Header band | **92 pt** in the topic list (52 title row + 40 tabs), the topic view (52 title row + 40 meta row) and the side panel (52 title row + 40 context row). The three 0.5 pt `sep` bottom rules sit on one y (91.5–92), and the second rows share one text center at y 72. Every title row centers at y 26 with the traffic lights. The sidebar has no rule: its own fill separates it |
| Columns | Sidebar 248 (200–320, resizable, ⌘⇧S hides it) · topic list 320 (280–400) · topic view flexible, min 480 · side panel 300 (260–380), closed by default. Each column edge is a 0.5 pt `sep` line |
| Narrow window | Shrink in this order: side panel closes, topic list to 280, sidebar to 200, then the sidebar hides. The topic view never clips (U44) |
| Content box | Timeline and composer share one box that fills the topic view with equal 24 pt gutters on both sides; the composer's right edge sits 24 pt from the pane edge. Text, cards and files share one measure: 620 (cards and files `min(620, box)`), so the stacked blocks have one right edge (Slack). Hover fills and the date divider span the box (U45). The scroll area fades the top 32 pt on an eased curve (`content` at 1, 0.9, 0.6, 0.25, 0 every 8 pt) so a row scrolled under the header never shows as a hard sliver |
| Settings window | 960 × 720 default; section list 200; content column max 704 including 32 pt side padding (640 of content), centered in the content area |
| Login window | 960 × 640 default; one centered 390 pt column |

### 4.5 Test sizes

Every screen is checked at 1100 × 680, 1440 × 900 and 1920 × 1080, in both themes, with
`--worst-case` data.

## 5. Elevation

| Level | Shadow L | Shadow D | Used by |
| --- | --- | --- | --- |
| 0 | none | none | Everything in the window, including cards (they use a 1 pt `border`) |
| 1 float | `0 0 0 0.5px #1F23291F, 0 2px 8px #0F111414` | `0 0 0 0.5px #FFFFFF1F, 0 2px 8px #00000059` | Hover toolbar, jump pill, selected segment |
| 2 overlay | `0 0 0 0.5px #1F232924, 0 8px 28px -6px #0F111438, 0 2px 6px #0F11140F` | `0 0 0 1px #FFFFFF29, 0 16px 40px -8px #000000B3` (a visible 1 pt edge: a black shadow does not read on a dark ground, Slack) | Popovers, menus, mention popup, switcher, dialogs, toasts |

Shadows only on floating layers. Never a shadow on a card, a row or a header.

## 6. Shape

- **Radii**: 4 inline marks in text (mention, inline code); 6 chips, labels, kbd, role tag,
  `sm` buttons, popup rows, hover-toolbar buttons; 8 buttons, 32 to 36 pt fields, selects, rows,
  option rows, file tiles; 10 40 pt fields, `lg` buttons, invite boxes; 12 cards, the composer,
  popovers, dialogs, the switcher, settings tables; full for badges, reactions and
  avatars. Concentric rule: outer = inner + padding.
- **Edges**: 1 pt `border` on cards, files, fields, the composer, selects and option rows; 0.5 pt
  `sep` for hairlines between rows and columns. No 0.5 pt borders on controls.

## 7. Components

Each component lists anatomy, sizes, states and do and don't. A state that is not listed does not
exist for that component. All states use the tokens of §2; one row helper and one control helper
in `ui/parts.rs` apply them, and a view never builds a clickable element without them.

### 7.1 Shared states

| State | Rows (sidebar, topic, menu, popup, settings) | Buttons and icon buttons | Fields and composer |
| --- | --- | --- | --- |
| Rest | No fill | Primary `accent`; secondary `content` + 1 pt `border-strong`; ghost no fill, `text-2` | 1 pt `border` (search: `field` fill, no border) |
| Hover | `hover` (`sidebar-hover` in the sidebar), at once | Primary `accent-hover`; others `hover` fill, icon `text-1` | 1 pt `border-strong` |
| Pressed | `pressed` | Primary `primary_active`; others `pressed` | — |
| Focus-visible (keyboard) | 2 pt `focus` ring inset 1 pt | 2 pt `focus` ring outside, 2 pt gap | 1 pt `focus-border` + 3 pt `focus-soft` outer ring (also on click focus) |
| Selected | `selected` / `sidebar-selected` (accent tints) + title 600 `text-1`; a 群 glyph in `sel-glyph` | Toggles show state by icon, not fill (U16) | — |
| Disabled | Content at 40 % | Primary: `disabled-bg` fill, `disabled-text` label, no opacity; others: label `text-3`; no pointer, no hover | `field` fill, text `text-3`, no border |
| Loading | Skeleton rows in `field` after 400 ms | Label stays, a 14 pt spinner replaces the icon (or sits before the label) after 400 ms, min 500 ms shown; input blocked | Spinner 16 at the trailing edge |
| Error | — | — | 1 pt `danger` border + 3 pt `danger-bg` ring; message 12/16 `danger-text` 6 pt under the field |
| Read only | — | — | No border, `field` fill, text `text-1`, I-beam for selection |
| Cursor | Pointer on every clickable row, button, chip, link, tab, avatar | same | I-beam |

### 7.2 Composer

The most-used control. Joye's hard requirement: a real writing area like Slack's and Feishu's.

**Anatomy** (top to bottom): optional blocking slab · optional format bar · text area · toolbar
row (`+` attach, `@`, emoji, `Aa` format at the left; send at the right).

| Part | Value |
| --- | --- |
| Box | In the content box, 16 pt from the window bottom; radius 12; `content` fill; 1 pt `border` |
| Text area | Padding 12 top, 14 left and right; `body` 14/22; min height 44 (two lines); grows with the text to 40 % of the topic view height, then scrolls inside; the box grows upward |
| Toolbar row | 40 pt (32 pt buttons + 8 pt bottom padding), padding 0 8; icon buttons 32 × 32, radius 8, 18 pt glyphs in `text-2`, gap 2: `plus` (添加附件: file, image, 产出 from disk, and the `同时发到群` option), `at-sign` (提及), `face-slightly-smiling` (表情), `type` (格式: toggles the format bar) |
| Send | 32 × 32, radius 8, `arrow-up` 18. Empty: `field` fill, `placeholder` arrow, disabled. With text: `accent` fill, white arrow |
| Empty height | 12 + 44 + 40 = **96 pt** |
| Format bar | Off by default. When on: a 36 pt row at the top of the box (bold, italic, strike, link, numbered list, bulleted list, quote, code), 32 pt icon buttons, a 0.5 pt `sep` under it |
| Placeholder | Names the destination (Feishu): `回复「夜间告警降噪：合并重复的 5xx」`; in a 群 with no topic open `在「qqbot 研发」发起新话题，第一行是标题`. One line, `…` |
| Typing line | People only (`李雪 正在输入…`): 12/20 `text-3`, 14 pt left inset so it aligns with the field text, 4 pt above the box. It takes no space when nobody types: the box keeps a fixed 8 pt above it (U32). Bots that work show the live row in the timeline instead (§9.2) |

States:

| State | Treatment |
| --- | --- |
| Rest | 1 pt `border` |
| Hover | 1 pt `border-strong` |
| Focus | 1 pt `focus-border` + 3 pt `focus-soft` outer ring. Visible at a glance in both themes |
| Has text | Send turns `accent` |
| Sending | The text clears at once; the message shows its sending state (§7.5) |
| Disabled (no permission) | `field` fill, no border, placeholder `你没有在这个群发言的权限`, toolbar hidden |
| Blocked (offline, quota) | A slab attached to the top of the box (radius 12 at the top, `raised`, 1 pt `border`, 12/16 text and one `sm` action), the box stays usable for drafts; send disabled |
| Error (send failed) | Shown on the message, not in the composer |

Keyboard: ↵ sends, ⇧↵ new line, ⌘B / ⌘I / ⌘⇧X format, ↑ in an empty composer edits my last
message, Esc closes the popup or blurs. Hints are in the send tooltip and the `+` menu footer only
(U09).

@mention popup: hangs from the token being typed: its left edge 12 pt left of the `@`, its bottom
edge 8 pt above the composer's top border; 320 pt wide, radius 10, `overlay`, level-2 shadow,
padding 4, max height 240 then scrolls. Section label 28 pt (12/16 500 `text-3`). Rows 40 pt, radius 6, padding 0 10, gap 10: 24 avatar, name `ui` 14, 岗位 13
`text-3`; active row `selected` + a trailing `corner-down-left` 14. ↑↓ moves, ↵ or Tab picks, Esc
closes. It filters as you type, so it is usually short; it must not cover the ask card the user is
answering (scroll the timeline first).

Do: keep the composer at least 96 pt in every topic, DM and new-topic state. Don't: put actions in
the text row, show permanent key hints, or tint the box when it is not focused.

### 7.3 Buttons and icon buttons

Anatomy: optional leading icon (16, gap 6) · label · optional trailing chevron (16) for a menu.
Sizes: §4.2. Variants: **primary** (one per view or card, the main action), **secondary**
(alternatives), **ghost** (low-emphasis actions, toolbars), **danger** (destructive, in its own
row or dialog). Labels are verbs from PRODUCT.md (`发布`, `修改`, `放弃`, `重试`, `加入`). States:
§7.1.

Do: 32 pt by default; one primary. Don't: use a 24 pt button anywhere; use a filled grey button
as "disabled" (it reads as another field); put two primaries side by side.

### 7.4 Fields, search and selects

| Part | Search and filter | Form field | Select in a table row |
| --- | --- | --- | --- |
| Height | 36 | 40 | 36 |
| Fill and edge | `field`, no border (sidebar); `content` + 1 pt `border` on a page that also has selects or form fields, so one page uses one field style | `content`, 1 pt `border` | `content`, 1 pt `border` |
| Radius | 8 | 10 | 8 |
| Text | `ui` 14, placeholder `placeholder` | `ui` 14 | `ui` 14 |
| Leading | `search` 16 `text-3`, gap 8 | — | — |
| Trailing | kbd hint (sidebar `⌘K`), or a clear `x` 16 when it has text | — | `chevrons-up-down` 14 `text-3` |
| Label | None (the placeholder names it) | Above the field: `small` 13/20 500 `text-2`, 8 pt gap | Column header |
| Help | — | Under the field: 12/16 `text-3`, 6 pt gap | — |

Widths: sidebar search fills the sidebar (12 pt margins); settings search 320; form fields fill
their column. States: §7.1 (error and read-only apply to form fields). A select for yourself (you
cannot change your own role) is plain text 14/20 `text-2` at the select's 12 pt text inset, with
no fill, border or chevrons, and a tooltip `你不能改自己的权限` (Slack admin).

Invite code (login): 8 Crockford base32 characters, shown and typed as `XXXX-XXXX` (case is
ignored, `I`/`L` read as 1 and `O` as 0, dashes and spaces are ignored); single use, and the
server limits redeem attempts per IP. 8 boxes grouped 4-4 (equal groups; a short last group reads as a missing
box), each 42 × 48, radius 10, 1 pt `border`, `code-entry`; 6 pt gaps, and between the groups a
6 × 2 `border-strong` dash (radius 1) with a 6 pt gap on each side: 8 × 42 + 6 × 6 + 18 = 390,
so the row ends exactly on the `加入` button's edges. The focused box uses the field
focus state. Paste fills all boxes; Backspace moves back.

### 7.5 Message row

Anatomy: gutter (36 avatar or, for a continuation, the hover time) · meta line (name, bot role
tag, time) · optional steps row · body · attachments (cards, files, images) · reactions.

| Part | Value |
| --- | --- |
| Row | Padding 6 / 8 inside a −8 margin, so hover fills reach 8 pt past the text; radius 8. Author groups 12 pt apart; a continuation sits 2 pt below the line above |
| Gutter | 36 pt, 12 pt gap to the text |
| Meta line | 22 pt: name `body` 14/22 600 (max 160, `…`), bot role tag (§7.9), time 12 `text-3` (never truncated) |
| Body | `body` 14/22, measure 620, paragraph gap 8, list item gap 4; URLs break anywhere |
| Grouping | Same author within 5 minutes with nothing between: no avatar or meta line; the time shows in the gutter on hover 12 `text-3` (Slack, U13) |
| Date divider | Text only: 12/16 500 `text-3` centered between two 0.5 pt `sep` lines that span the content box; 16 pt above, 4 below. `今天`, `昨天`, `11月2日 周日`. Sticky while its day scrolls; click jumps to a date |
| New-messages divider | A `danger` hairline with `新消息` 12/500 `danger-text` at the right |
| Reactions | 6 pt under the body, gap 6: 26 pt pills, radius 13, padding 0 8 0 6, gap 4, `field` (hover `pressed`, pressed `border`); emoji 16/16, label 12/16 400 `text-2`: names for up to 2 (`👍 Joye、Tata`), else the count only (`✅ 14`, names in the tooltip), so no pill is wider than about 60 pt beyond the two-name case; mine `accent-soft` + 1 pt `accent` ring + `accent-text` (hover and pressed `focus-soft`); the add pill (34 x 26, `field`, `face-slightly-smiling-plus` 16 `text-2`) shows only while the message is hovered or its picker is open, as in Slack. Reactions are stored with their message in the 群's room (the server fans out `reaction.changed`); one change per message and emoji is in flight at a time |
| Jumbo emoji | A message of one to three emoji and nothing else renders at 32/40 (Slack) |
| Edited | `（已编辑）` 12 `text-3` after the last line (U12) |
| First message | 16 pt below the header (or the topic start block) |

States: rest (no fill) · hover (`hover` fill + hover toolbar) · focused by keyboard (ring) ·
sending (body at 60 % and a 14 pt spinner after the last line after 400 ms) · failed
(`circle-alert` 14 + `没有发出` 12/16 `danger-text` after the body, then 8 pt later a text button
`重试`: 22 pt, padding 0 6, radius 6, 13/20 500 `accent-text`, no border, hover `hover`) ·
highlighted from search (2 s `me-bg` fill that fades out).

Hover toolbar (Slack): a capsule 18 pt above the row top, 8 pt from the content box's right edge;
`overlay`, level-1 shadow, radius 8, padding 2, four 32 × 32 icon buttons (radius 6, glyph 18):
add reaction (`face-slightly-smiling-plus`), reply (`reply`, quotes into the composer), copy as
quote (`text-quote`), more (`ellipsis`: edit, delete, copy link, mark unread). It appears and
hides at once.

Do: keep names and body the same size. Don't: show the 岗位 of people in the timeline (it is in
the hover card, U12); put a status dot on every bot avatar.

### 7.6 Topic row

Anatomy: unread dot · 36 avatar of the responsible member · line 1 (title, time) · line 2
(preview `作者：内容`, one trailing element).

| Part | Value |
| --- | --- |
| Row | 68 pt, inset 8 in the list, radius 8, padding 12 10 12 18, gap 12; line 1 and line 2 are 20 pt each, 4 pt apart |
| Avatar | The responsible member: person circle, or the bot tile (§7.9); no status pip in rows. On the selected row a bot tile gets a 1 pt `tile-ring` |
| Title | `ui` 14/20 500; unread and selected 600 `text-1`. A leading full-width bracket (`【`, `（`, `「`) stays in place: no hang (removed in v2.2) |
| Time | 12/16 `text-3` (§6.6 formats) |
| Preview | `small` 13/20 `text-2`, Markdown stripped; the full string is laid out and the text system ends it with `…` 8 pt before the trailing element, so every preview ends on the same right edge; `生成中…` while a bot streams |
| Trailing element | Exactly one, by priority (§9.1): compact `等你答` / `等你审` chip > `@n` red badge > `卡住了` label > other label (`等审`, `等回答`, `完成`) > neutral `count-solid` badge for unread without a mention. 进行中 shows nothing (U02) |
| Unread dot | 8 pt `unread-dot` at x 5 in the 18 pt left padding, centered on line 1, inside the row (U24) |

States: §7.1. No hairlines between rows (U24).

### 7.7 Sidebar rows

| Row | Value |
| --- | --- |
| Search | §7.4, 36 pt, margins 0 12 8 |
| Section header | 28 pt, 16 pt above (4 for the first), padding 0 8: chevron 12 `text-3` (rotates when collapsed), title 12/16 500 `text-2`, then for 待办 the count in `text-3` (`待办 3`), for 群 and 私聊 a `sm` icon button `plus` (新建群 / 发起私聊) |
| 待办 row | Two lines, padding 6 8, radius 8, gap 10: 24 pt glyph slot (`circle-question-mark`, `eye` or `at-sign` 16 in `text-3`), title `ui` 14/20 500, context 12/16 `text-2` (`qqbot 研发 · Watch 等你回答`). Max 5 rows, then `全部 n 项`. The section itself says "for you", so the glyphs stay grey |
| 群 row | 32 pt, radius 8, padding 0 8, gap 10: 24 pt slot with `hash` 16 `text-3` (`text-1` when unread, `sel-glyph` when selected), name `ui` 14. Unread: name 600 `text-1`. Mention: red badge `n`. Muted: row at 60 % + `bell-off` 14 |
| 私聊 row | Same; the slot holds a 24 avatar (person with presence, or the bot tile with its pip). Unread DM: name 600 + neutral `count-solid` badge (red stays for @ mentions) |
| Footer | 56 pt, top hairline inset 8: 32 avatar with presence, name `ui` 14/500, presence word 12 `text-2` (在线 / 离开 / 勿扰), a 32 pt settings icon button. The name area opens the account menu (U21) |
| Scroll edges | A 24 pt fade from `sidebar` at the top and bottom while the list scrolls |

### 7.8 Status chip, label, badge, neutral badge, kbd

| Component | Shape | Use |
| --- | --- | --- |
| Chip | 22 pt, radius 6, padding 0 8 0 6, gap 4, glyph 14, 12/16 500, filled, max width 184 | `you` (等你回答, 等你审; text 600) in the topic start block of a card-less topic, and `info` (进行中, topic header only) |
| Compact chip | 20 pt, radius 6, padding 0 6, 12/16 600, no glyph, max width 72 | Topic rows: `等你答`, `等你审` in `you` tone. The preview keeps the rest of the line |
| Quiet label | Label metrics, glyph in `you-glyph`, text `text-2`, no fill | 等你回答 / 等你审 in the topic header, the start block and the switcher |
| Label | Same metrics with no fill | 卡住了 (`danger-text`), 完成 (`text-3`), 等{name}回答 / 等{name}审 / 等回答 / 等审 (`text-3`) |
| Badge | 20 pt, min width 20, padding 0 6, radius 10, 12/16 600 white on `badge` | @ mentions only (`@n` in topic rows, `n` on a 群). Red appears for @ and 卡住了, nothing else. Cap `99+` |
| Neutral badge | Same shape, white 12/16 600 on `count-solid` | Unread topic rows and DMs without a mention. Cap `99+` |
| kbd | 20 pt, min width 20, padding 0 5, radius 6, `field`, 12/16 500 `text-2` | `⌘K`, option letters, switcher footer |

| Status | Glyph | Viewer in `waitingOn` / not |
| --- | --- | --- |
| `open` | none | none |
| `in_progress` | `circle-dot` | info chip in the topic header; nothing in rows |
| `waiting_answer` | `circle-question-mark` | you chip / label |
| `waiting_review` | `eye` | you chip / label |
| `blocked` | `circle-alert` | danger label |
| `done` | `circle-check` | grey label |

A name never goes inside a header label: when the topic waits on someone else the header shows a
20 pt avatar of that person, 6 pt gap, then the fixed label `等回答` / `等审` 13/20 `text-2`; the
full `等 Aleksandra Wiśniewska 回答` is the tooltip and is in the side panel. Only the title
truncates. In topic rows the name is dropped too (`等审`). Rule: one orange fill per
screen, in the topic row. The topic view uses the quiet label; while the card that waits for you
is on screen the header shows no label at all and the bot's pip is hidden, because the card's
3 pt `you-bar` already says it. Target: at most two orange marks per screen.

Counts above 99 render as `99+` everywhere: badges, neutral badges, tabs and the 待办 section.

### 7.9 Avatars and bot identity

- People: a circle (photo, or `avatar-bg` with `avatar-fg` 600 initials): one character for a
  CJK name (`王一鸣` → 王), two letters for a Latin name (`Alex Chen` → AC) at 32 pt and above
  (font 0.38 × size: 12 at 32, 14 at 36); **at 24 pt and below one character only at 12/16 600**
  (`AC` → A); a leading emoji is skipped (`🧑‍💻 Priya` → P) (U18, U37). Presence dot at the
  bottom right: 10 pt at offset −1/−1 on avatars of 32 and more; on 24 pt and below 8 pt at
  offset −3/−3, so it sits on the corner and does not cover the 12/16 initial (drawn 1 pt above
  center). 2 pt ring in the surface color (`sidebar` in the sidebar). Online `online`; offline
  shows no dot (U41).
- Bots: **a rounded-square tile** the size of the avatar, radius 0.25 × size (9 at 36, 8 at 32,
  6 at 24), filled with the bot's tint (`bot-1` to `bot-6`, §2.6). The jojo-friends character at
  its live status (rules in docs/ROSTER.md) sits inside it, cropped to the painted bounds of its
  idle pose (measured from pixels, so the hidden shadow and lamp layers do not count), fitted by
  the larger side to **62 %** of the tile (22 at 36, 15 at 24) with its center 2 % above the
  middle, so the face line lands near 46–48 % of the tile. Every side keeps at least 19 % inset:
  no art touches or crosses the tile edge, and wide (Ledger) and tall (Watch) characters have
  the same footprint. On a selected row the tile gets a 1 pt `tile-ring`. People are
  circles and bots are squares: the shape alone says which. The mockups measure the bounds with
  `src/charbox.mjs` into `char-boxes.json`; the app computes the same box once per character.
  **Up to 64 pt** the app draws the small variant: the status lamp at the top corner, the
  question bubble, the working ring and the sleep marks are removed, because at these sizes they
  read as notification badges (the 48 pt start block included). The face keeps the expression.
  The status then shows as a pip in the presence position, only for `working` (`accent`),
  `needs-input` (`you-glyph`) and `error` (`danger`). Above 64 pt the full art is drawn.
- A bot's avatar signal (art and pip) is one status across all its runs, by priority:
  **needs input > error > working > idle**. A bot that works in one topic and waits for an
  answer in another shows needs input. Needs input means: a topic it is responsible for waits on
  a person, one of its runs waits, or it posted an ask or draft card (`warning` tone with
  buttons) that nobody answered yet in a topic that is not done; this wins over a blocked topic
  elsewhere (settings then says 等人回答, not 出错了).
- Role tag after a bot's name (messages, side panel, settings): 20 pt, radius 6, `field`, padding
  0 6, `bot` glyph 12 + 岗位 12/16 500 `text-2`. Confirmed by Joye on 2026-11-03: the character is
  the mark, plus this tag. People show 岗位 as plain `small` text where a list needs it.
- Group glyph: everywhere a 群 is listed (sidebar, switcher) it is the `hash` glyph in a 24 pt
  slot, never a letter tile. The `tile` fill stays only for dialogs that need a group image.

### 7.10 Cards

All cards: in the message column, width `min(620, content box)` (one width for text, cards, files
and images, U29); `content` fill, 1 pt `border`, radius 12, padding 14 16 16. One border per card:
nothing inside a card has its own outline (Feishu). Title `card-title` 15/22 600 `text-1`, 4 pt
below it one sentence `body` 14/22 `text-2`; blocks with 4/8/12/16 spacing; text 12, 13 or 14 only.
Actions: `md` buttons, gap 8, 14 pt above, at most 3 then `ellipsis`; exactly one primary.

| Card | Content |
| --- | --- |
| Work card | Title, chip or label, one sentence, optional image (radius 8) or plan list, actions |
| Ask card (等你回答) | Question as title; promise line (`你选之前我不会改任何环境。`); options as 40 pt rows (radius 8, `opt-bg` fill, no border, gap 6, padding 0 12 0 8) with a 20 × 20 key letter A, B, C (12 `text-3`, no fill) and `body` text. Hover: `opt-hover`. Keyboard focus: §7.1 ring. Chosen: 1 pt `accent` + `choice-bg`. Keys A to C work while the card has focus. When it waits for the viewer: the neutral 1 pt `border` plus a 3 pt `you-bar` on the left edge, inset 12 pt from top and bottom (inside the radius), and no chip: amber appears once per card. After the answer: the card shrinks to `✓ 你选了：现在在 staging 打开` and `waitingOn` clears |
| Draft card (before anything irreversible) | Title `发布预览`; the real object as key-value rows with no outer border (rows padding 10 0, `ui` 14/20, key column 48 `text-3`, 0.5 `sep` under each row), body preview `body` clamped to 3 lines; actions primary `发布`, secondary `修改`, ghost `放弃`. The `you-bar` while it waits for the viewer |
| Alert card (integrations) | Title + label, fields (label 12 `text-3`, value 14 `text-1`), actions. Resolved: `已处理` success label |

### 7.11 File row

12 pt padding (8 at the right), radius 12, 1 pt `border`, `content`: 40 pt tile (radius 8,
`accent-soft`, type glyph 20), name `ui` 14/500 (middle ellipsis keeps the last 12 characters
and the extension, U26), meta 12/16 `text-2` (`网页产出 · v2 · 18 KB`), trailing 32 pt icon
buttons (open, show in Finder, download; shown on hover, always on while downloading).
Downloading: `正在下载 64%` in the meta and an inset progress track (left 12, right 12, bottom 6, height 3, radius 1.5, `field` track, `accent` fill), so the bar never crosses the row's rounded corners or border; cancel `x`.
Images: max 360 × 320, radius 8, 1 pt `border`; while loading a `field` skeleton of the final
size with `file-image` and the name (U30). 产出 rows in the side panel use the same parts at 48
pt with a 32 tile.

### 7.12 Overlays

One overlay component (U05, U36 to U38): `scrim`, `overlay` surface, radius 12, level-2 shadow.
Command-style panels sit 96 pt from the window top; dialogs and alerts are centered.

- **Switcher (⌘K)**: 640 wide. Query row 56: `search` 20, 18/24 query text, `esc` kbd. Groups
  (话题, 群, 私聊, 成员, 消息): label 32 pt (12/16 500 `text-3`). Rows 44 (60 for message hits),
  radius 8, padding 0 12, gap 12: a 24 pt slot with the sidebar's glyph (`hash` 18 `text-3` for a 群, the lead avatar for a topic or a message hit), title `ui` 14/500, ` · 群名` 13
  `text-3`, snippet 13/20 `text-2`. Every match in every row type (topic titles, 群 names, message snippets) gets one mark: `hl-bg`, radius 4, padding 0 2, 20 pt tall, text color unchanged. Trailing: the same compact `等你答` / `等你审` chip as the topic list (one name and one style per state everywhere). Active row
  `selected` + `corner-down-left`. Footer 40: 12/16 `text-3` with 20 pt kbd. With no query:
  最近 (5 topics), then 群 and 私聊 (U34). Height follows the content up to 70 % of the window,
  whole rows only. ⌘F opens it scoped to the 群 with a removable `在 qqbot 研发` token; while
  scoped, the footer offers `⌘F 在全部群里搜索`, and ⌘F removes the scope. The list fades its
  bottom 24 pt from `overlay` while more rows follow.
- **Dialog**: width 440 (forms) or 560; padding 20; title `title` 16/24 with 16 below; body
  `body`; form fields 40 pt with labels above; footer right-aligned `md` buttons (secondary then
  primary), 20 pt above. Primary is disabled until the form is valid. No close `x` when a cancel
  button exists. Checkbox lists: 16 pt checkbox, 40 pt rows.
- **Alert**: centered, 320 wide, title `title`, buttons `md`.
- **Menu / popover**: padding 4, rows 32 (radius 6, `ui` 14, icon 16), separators 0.5 `sep` with
  4 pt margins.

## 8. Screens

### 8.1 Main window

Sidebar (§7.7) · topic list · topic view · side panel. Screens: `main-*`, `short-topic-*`,
`worst-case-*`, `switcher-*`.

**Topic list header**: 52 pt, padding 0 10 0 16: 群 name `title` 16/24 (truncates), then a 32 pt
icon button `square-pen` 18 with the tooltip `新话题 ⌘N` (icon only, so the 群 name keeps the
width; Slack's compose button). The member count lives in the topic view header (U22).

**Tabs**: 40 pt band, gap 24, 16 pt left: 全部 · 未读 n · 等我 n, `ui` 14/20 `text-2`, count 13
`text-3` (hidden at 0, `99+` cap); active `text-1` 600 with a 2 pt `text-1` underline on the
bottom hairline.

**Topic view header**: 92 pt, padding 0 12 0 24, a 0.5 pt `sep` bottom rule level with the tab
rule. Row 1 (52): title `heading` 18/26 (takes all free width, truncates), then trailing (gap 4)
32 pt icon buttons `bell` (follow), `panel-right` (state shown by the icon, U16), `ellipsis`.
Row 2 (40), `small` 13/20 `text-2`, gap 8, items split by a `text-3` `·`: breadcrumb `hash` 14
`text-3` + 群 name (truncates first); the status label (quiet `等你…`, or a 20 pt avatar + `等回答`;
hidden while the ask card is on screen, §7.8); `users` 14 + `n 位成员` (opens members in the side
panel). The title shows in every state, including while the start block is on screen (Slack's
channel header and intro both name the channel).

**Topic start block**: the first thing in every topic (U01): the responsible member's avatar at
48 pt (bot tile radius 12, small idle art, §7.9) with 12 pt below; title `display` 22/30 (2 lines max); one
line 8 pt below: `Joye 发起 · Taro 负责` (`small` `text-2`, truncates) + ` · 今天 14:05` (never
truncated); 16 pt below it a 1 pt `border` rule, then the first message 12 pt under the rule.
The status is not repeated here: the header row 2 shows it. The timeline
always anchors to the bottom, short topics included (Slack): the start block sits right above the
first message and the last message sits right above the composer, so a short topic leaves no gap
between the conversation and the composer; free space stays above the start block, where the
avatar gives it an anchor.

**Jump pill**: only when the first unread message is above the viewport (U14): `n 条新消息 ↑`,
28 pt, `overlay`, level-1 shadow, centered 12 pt under the header.

**Side panel**: closed by default. Opens on `panel-right` or the member count, and by itself once
per topic when a bot starts working in it or the first 产出 arrives (the user's close is
remembered per topic). Never shown with no topic open (U07).

| Part | Value |
| --- | --- |
| Header | 92 pt with a 0.5 pt `sep` bottom rule level with the other two: row 1 (52) `话题详情` `title` 16/24 + close `x` 32; row 2 (40) one context line `small` 13/20 `text-2` that the timeline does not show (`Taro 负责 · 周四 20:00 截止`). Sub-pages (a 产出's versions, the process view, a member) replace the content with a `chevron-left` back button and the sub-page title |
| Sections | 16 pt side padding, 16 pt between sections (16 for the first, under the rule). Label 12/16 600 `text-3` with the count inline (`产出 · 2`), 8 pt below |
| Progress | When a bot is responsible: flat like the other sections (no card): 36 pt bot tile, name `ui` 14/600, state `small` `text-2` (`等你审发布预览`); plan list, first match wins: the bot's run plan (protocol "Run plans"), else a plan-style card list, else the run's latest steps. 28 pt rows, 16 glyph (`circle-check` `success` for done, `circle-dot` `text-1` for current, `eye` `text-1` for waiting on a member, `circle-dashed` `text-3` for later), `small` 13/20 text, current and waiting items `text-1` 500; at most 6 rows, a window from two items before the first open one. `查看过程` text button opens the process view |
| 产出 | 48 pt rows (§7.11). One quiet line when empty: `还没有产出` 12 `text-3`. The label row ends in a 28 pt `sm` ghost icon button, `plus` 16 `text-2`, tooltip `上传产出`: it picks a file from disk and adds it as a 产出 of the topic, the same action as 产出 from disk in the composer's `plus` menu (§7.2); disabled while an upload runs |
| 成员 | 40 pt rows: 24 avatar, name `ui` 14, trailing 岗位 13/20 `text-3` for everyone, with a 12 pt `bot` glyph and 4 pt gap before it for bots (no tag fill); then a 40 pt `添加成员` row in the same columns: a 24 pt slot with `user-plus` 16 `accent-text`, 10 pt gap, label `ui` 14/20 500 `accent-text`; hover `hover` fill, radius 8 |

### 8.2 Dates and times (one format everywhere, U23)

| Age | Lists, switcher, 待办 | Message meta |
| --- | --- | --- |
| Today | `14:32` | `14:32` |
| Yesterday | `昨天` | `14:32` (under the 昨天 divider) |
| Within 7 days | `周一` | `14:32` |
| This year | `10/28` | `14:32` |
| Older | `2024/12/31` | `14:32` |

Date dividers spell the day: `今天`, `昨天`, `周一`, `10月28日 周二`, `2024年12月31日`. Tooltips on
any time show `2025年11月4日 23:59`.

### 8.3 Login (`login-*`)

960 × 640 window, `content`. One column, 390 wide, centered (24 pt above the optical center):
app icon 72, drawn bare: the brand SVG cropped to its squircle (viewBox `100 100 824 824`; `icon-dark.svg` in dark mode), its own shadow filter removed and replaced by `drop-shadow(0 4 12 #0F11141F)` on the image; no tile, radius or frame around it (U39); 24 pt; `欢迎加入团队` `display`;
6 pt; `输入管理员发给你的邀请码` `body` `text-2`; 28 pt; label `邀请码` (§7.4); 8 pt; the invite
code boxes (§7.4); 24 pt; `加入` `lg` primary, full column width, disabled until complete;
for the built-in server, 12 pt then a centered text button `使用其他服务器` 13/20 500
`accent-text` (the raw host is not shown); for a custom server, 16 pt then the server line `server` 14 +
`team.joye.co` 13/20 `text-3` + ` · ` + `更改` 13/20 500 `accent-text`. Both reveal a 40 pt
server field. The first box has focus and the caret when the screen opens; `加入` is enabled
once 8 characters are in. A pasted `tc_` credential or a 12-character code of the earlier
format shows as one locked field. Errors: the code boxes take the error border, `邀请码不对，请检查后再试。`
shows under them, and the typed code stays in the boxes with the caret after its last
character and no selection color; the next character typed or pasted starts a new code from box
1, and Backspace edits it from the end. Opening the server field moves the focus into it.

### 8.4 Settings (`settings-members-*`)

960 × 720. Section list 200 pt on `sidebar`, padding 0 8: 36 pt rows (`ui` 14/20, icon 16, gap
10), selected `sidebar-selected` + 600. Content on `content`, column max 704 with 32 pt side
padding, centered: page title `display`, 36 pt from the top, the primary action at its right; 20 pt; the
search field (36 × 320, `content` + 1 pt `border` like the selects); 16 pt; the grouped table: radius 12, 1 pt `border`; header row 36 pt
**inside** the table, no fill, 12/16 500 `text-3`, a 0.5 `sep` under it (U41); rows 60 with 0.5
`sep` between; a footnote `small` 13/20 `text-3` 12 pt under the table.

- **成员**: columns member (36 avatar, name `ui` 14/500 + `你` 12 `text-3` 4 pt after it, 岗位 `small` `text-2`
  with the `bot` glyph 12 for bots), status, 96 pt (8 pt dot + `ui` `text-2`: 在线 `online`, 工作中
  `accent`, 等人回答 `you-glyph`, 离线 hollow `text-3`; a machine goes on line 2 as 12/16
  `text-3`, `Mac mini`, with the dot on line 1), 权限, 132 pt (select 36 × 120; your own row is
  plain text, §7.4), trailing 32 pt `ellipsis` or an empty slot of the same width. Names are
  trimmed (U41). One primary `添加成员 ▾` (36 pt page primary) with `邀请同事` and `添加 AI 成员`.
  Order: me, the other owners, the other people, then bot members, then deactivated members; by
  name inside each part. The 权限 words are `管理员` and `普通成员`, the words of the login and
  invite copy; earlier mockups' `所有者` and `成员` are superseded. Select text is regular (400). The
  page scrolls; its end keeps 32 pt under the footnote.
- **我的资料**: avatar with `更换头像` and its help `PNG、JPEG 或 WebP，不超过 1 MB`; name and 岗位
  form fields save on blur; sign-out at the bottom; `退出并删除本机记录` in its own danger row
  (U40).
- **通知**: per 群 one segmented control `所有消息 / 仅 @我 / 不提醒` (32 pt, selected segment a
  `content` fill with a level-1 shadow) and a 静音 switch; 群 and 私聊 in separate tables (U42).
  While a 群 is muted its control shows `不提醒`, disabled; unmuting brings back its level.
- **机器**: rows with an icon by machine type, Chinese tags, status centered (U43).
- **成本**: hidden until it exists (U43).
- **通用** (replaces the earlier 关于 page; last in the section list, `settings` gear 16, as in the mockup): the about card
  (app icon, version, update state, 检查更新, server), 诊断, and the developer-view switch.

### 8.5 Empty and error states

| Case | Treatment |
| --- | --- |
| No 群 at all | One centered welcome in the content area: character `jojo` 64, `display` `还没有群`, one `body` `text-2` sentence, `lg` buttons `新建群` primary and `发起私聊` secondary. Topic list, composer and side panel hidden (U06). The sidebar hides 待办 while it is empty and keeps the 群 and 私聊 headers with their `+` |
| 群 with no topics | Topic list: one 12 `text-3` line `还没有话题` under the tabs; topic view: the composer with the new-topic placeholder |
| No topic selected | Topic view: centered `body` `text-2` `选一个话题，或写下第一行开始新话题` |
| Empty filter | `没有未读话题` / `没有等你的话题`, 12 `text-3`, top of the list |
| Search, no result | `没有找到「词」` + `在所有群里搜索` text button |
| Load error | Inline row at the top of the timeline: `circle-alert` `danger` + `没能加载更早的消息。` + `sm` `重试` |
| Field error | §7.1 error state; submit stays disabled (U40) |

### 8.6 Banners

App-level banners (update ready, offline) sit above the sidebar footer as a 52 pt card (radius 8,
`raised`, 1 pt `border`, padding 8 8 8 12, 12 pt side margins and 8 pt under it), never in the
topic column, so header lines stay aligned (U17). Anatomy: a 16 pt glyph, gap 10, the title
(`small` 13/20 500 `text-1`) over the detail (12/16 `text-3`), each one line with `…`, then the
`sm` actions with a gap of 4. The detail is short enough for a 248 pt sidebar
(`安装盘已在访达中打开`, `8 秒后重连`, `版本 0.2.0`); a longer sentence goes to the tooltip.
Both banners use one card helper, so they share size and rhythm.

## 9. Bots and live work

### 9.1 Status in lists

One status per row, by priority (OpenBot): 等你… chip > @ badge > 卡住了 > 完成 / 等审 / 等回答 >
unread count. 进行中 is the default state of work and shows nothing in rows; the topic header
shows it.

### 9.2 Bot replies (GrokBot, Slack agent sessions)

- **Acknowledge once**: the first reply is one short line (`收到，先查告警来源。`).
- **Pending row**: while a bot works and has not posted, one 28 pt row at the bottom of the
  timeline, 12 pt below the last message: the 24 pt working character centered in the 36 pt
  gutter, then `**Scout** 正在 staging 上验收` `small` `text-2` (name `text-1` 600), three
  pulsing dots, elapsed time 12 `text-3` (`2:14`), `·`, and a `查看过程` text button (`small`
  `accent-text`). The person who asked also sees `停止`. The label is the bot's latest step, one
  phrase. Several bots: one row each, at most 3, then `另外 n 位成员在工作`.
- **Steps row**: above a reply that used tools, one collapsed 24 pt row: `chevron-right` 12 +
  `看了 6 个文件，运行 3 条命令` `small` `text-2` + `· 4 分 12 秒` `text-3`; failures add
  `1 次失败` in `danger-text`. Opening it shows the steps inline (1 pt left rule, 12/16 rows, max
  220 then scroll); the full process view is in the side panel.
- **Streaming**: the reply streams in place in one message. The topic row preview shows `生成中…`.
- **Lead with the result**: bot prompts ask for a bold lead-in (`**改好了**：…`) and two or three
  short paragraphs, with long content in a 产出. This is a prompt rule.

## 10. Content

- Interface words come only from PRODUCT.md (话题, 群, 私聊, 待办, 产出, 成员, 岗位). Bots are
  marked by the role tag, not by a word.
- Buttons are verbs. Errors say what failed and what to do (`没能加载更早的消息。` + `重试`).
- Placeholders name the destination; they are the only instruction in the composer (U08).
- Punctuation (U47): full-width punctuation in Chinese (`：` `，` `（）`) with no space after `：`.
  A Latin word next to Han gets one space (`staging 上`, `等 Aleksandra 回答`). Ellipsis is
  U+2026 `…`, drawn by one truncation helper that trims trailing punctuation and spaces before
  it (U26).
- A title that starts with a full-width bracket (`【紧急】`, `（无标题）`) draws the bracket in
  place, with no hang into the margin (the v2.1 `halt` trim is removed in v2.2). CJK punctuation
  stays off line starts and ends (the vendored gpui line breaker).

## 11. Accessibility

- Floor: WCAG 2.2 AA. Content text ≥ 4.5:1 and icons, borders of focused controls and large text
  ≥ 3:1 over their surface, in both themes (values in §2). Known exceptions: `placeholder` (2.6 /
  3.3; the label or destination repeats it) and `text-3` on `sidebar` in light (4.1; used only for
  the 待办 count and chevrons).
- Focus: every control and row is reachable by keyboard. Focus-visible ring 2 pt `focus` with a
  2 pt gap in the surface color (inset 1 pt on rows). Fields and the composer use the
  `focus-border` + `focus-soft` ring on any focus.
- Keyboard map: ⌘K switcher · ⌘F search in the 群 · ⌘N new topic · ⌘⇧S sidebar · ↑↓ in lists ·
  ↵ opens · Esc closes the top overlay · A to C answer a focused ask card · ↑ in an empty composer
  edits my last message · Tab moves through header, list, timeline, composer.
- No meaning by color alone: every status has a glyph and a word; unread has weight and a dot;
  badges have numbers.
- Hit areas ≥ 28 × 28 (32 where space allows).
- Reduce motion (§12) and Increase contrast: with Increase contrast on, `border` and `sep` use the
  `border-strong` value and `text-3` uses `text-2`.
- Text size: the app follows the system text size setting through the density tokens (§14).
- Every icon button has a tooltip and an accessible name (the tooltip text).

## 12. Motion

Curve **out** `cubic-bezier(0.23, 1, 0.32, 1)` for every enter and exit; never ease-in. Tiers:
`instant` 0 · `fast` 120 ms · `normal` 160 ms · `slow` 200 ms. Nothing over 240 ms except loops.
The more often a control is used, the less it moves.

| What | Animation | Reduce motion |
| --- | --- | --- |
| Hover fills, hover toolbar, focus rings | Instant | same |
| Press | Pressed fill at once; release 120 ms | same |
| Menus, popovers, select | 150 ms opacity + 4 pt travel from the trigger | opacity only |
| @mention popup, switcher | None: appear and filter at once | same |
| Dialogs and sheets | 200 ms opacity + scale 0.98 → 1; exit 150 ms | opacity only |
| New message from others while visible | 120 ms opacity only, no rise (it fires 100+ times a day, often while the person reads); none when the window is not focused or for a batch of more than 3 | same |
| My sent message | None | — |
| Pending row enter / exit | Enter 200 ms opacity + 6 pt rise; exit 160 ms opacity after a 400 ms hold | opacity only |
| Busy indicators (dots, spinners, skeletons) | Wait **400 ms** before showing; once shown, stay at least 500 ms | same |
| Pending dots | Three 4 pt dots, opacity 0.3 → 0.9, 1.2 s loop, 150 ms apart | static 0.9 / 0.6 / 0.3 |
| Streaming bot text | Appears as it arrives; no per-word fade | same |
| Character status change | Crossfade 160 ms | instant |
| Chip change | 160 ms crossfade | instant |
| Ask card after answer | Height snaps, content fades 160 ms; check scales 0.6 → 1 over 200 ms | no scale |
| Composer focus ring | 120 ms opacity | instant |
| Side panel | Width snaps. When it opens by itself (a bot starts work), content fades 120 ms; ⌘⌥I and the button stay instant | instant |
| New sidebar row | 200 ms opacity + 6 pt from the left, only for an occasional row (a new 群 or DM); never for a batch | opacity only |
| Never animated | Selection, keyboard navigation, switching 群 or topics, counts, tabs, theme change, resize, composer growth | |

## 13. Dark mode

- Every token in §2 has a dark value; product code never writes a light or dark literal.
- Elevation steps differ by at least 4 % lightness: `sidebar` #18181B < `content` #1F1F23 <
  `raised` #28282D < `overlay` #34343B (U46). Selection (`selected` #23324F, an accent tint) is stronger than hover
  (0.04).
- Filled primary controls use the darker dark `accent` #2F6FEB so white labels keep 4.5:1; focus
  uses the lighter `focus-border` #4C8DFF so the ring reads on dark surfaces.
- The dark login uses `icon-dark.svg`.
- Check on a real display before release (§18).

## 14. Density

One density ships: **comfortable** (the values in this file). Density is a token-level setting:
a later compact mode changes only the tokens (control heights −4, row heights −8, `body` stays
14/22, nothing under 12), never view code (Zed derives densities from one spacing table). Compact
mode is out of scope for v1.

## 15. Worst-case data (break-ui CATALOG)

Checked in `worst-case-*` at the 1100 × 680 minimum window.

| Slot | Rule |
| --- | --- |
| Names, titles, chips | One line, `…` at the end, full text in a tooltip |
| Sender name in the meta line | Max 160, `…`; the time is never truncated |
| File names | Middle ellipsis, keeps the last 12 characters and the extension |
| Topic start title | Two lines, then `…` |
| Start block meta | The people part truncates; ` · date` stays whole |
| Previews, 待办 context, typing line, placeholder | One line, `…` |
| Counts | `99+` everywhere: badges, neutral badges, tabs, the 待办 section |
| Ellipsis | One helper: trims trailing spaces, open brackets and punctuation before `…`, and drops a cut version number or a Latin word cut to under 4 characters (`Joye Huang…`, never `Joye Huang（…` or `v0.9.…`) |
| Message body | Wraps; URLs break anywhere; code scrolls horizontally |
| Missing title | `（无标题）` in `text-2` |
| Emoji-first names | The initial skips the emoji; the name keeps it |
| Long reactions | `✅ 14` (count only past two names; the names are in the tooltip) |
| 等{name}… | Name max 96 with spaces around a Latin name |

## 16. Examples and keeping in sync

| Mockup | Shows |
| --- | --- |
| `main-light`, `main-dark` | Sidebar with 待办, 群, 私聊; topic list; a long topic with people and bots, steps row, file, ask card waiting for the viewer, Scout's pending row, the focused composer with a draft and the @mention popup |
| `short-topic-*` | A short topic anchored to the bottom with the start block (48 pt owner tile, title, meta, rule), the two-row header with the `# 社媒` breadcrumb and 等你审, the hover toolbar, the draft card, the side panel with Taro's live card, 产出 and 成员 |
| `worst-case-*` | 1100 × 680 window on the desk, long names and titles, counts capped at 99+, a jumbo emoji message, emoji names, a huge file downloading, a failed send, a long typing line |
| `switcher-*` | ⌘K over the scrim with grouped results and a message hit |
| `settings-members-*` | Settings window on the desk, 成员 page with a centered column |
| `login-*` | Invite-code login on the desk with the label, the 8-box 4-4 code with the focused box, the neutral disabled `加入` and, for the built-in server, `使用其他服务器` |

Every mockup is rendered as `<name>.png` (scale 1) and `<name>@2x.png` (scale 2, the scale of the
reference captures). Compare at 2x next to the Slack and Feishu captures.

Sync rules:
- One token source: `tokens.mjs` for the mockups; `theme.json` + `theme::Colors` for the app.
  Each value change updates this file, `tokens.mjs`, the mockups and `theme.json` in one commit.
- Sizes are Rust enums or constants in `ui/` (`TextStyle`, `ControlSize`, `Radius`, `Space`), so
  an off-scale value does not compile (Zed pattern). Views never write a color, size or duration
  literal.
- A component preview screen in the app shows every component of §7 in all its states in both
  themes. Each visual pull request adds before and after captures at 2x (Zulip rule).

## 17. Checklist

Run screen by screen, in both themes, at 1100 × 680 and 1440 × 900, with `--worst-case`, next to
the reference captures at 2x. The audit item is in brackets.

Sizes and type
- [ ] Composer: empty height ≥ 96 pt, two text lines, toolbar row (attach, @, emoji, format left;
      send right), radius 12, grows to 40 % of the topic view; focus ring visible at a glance.
- [ ] Search and filter fields 36 pt, form fields 40 pt, table selects 32 pt; 14 pt text; labels
      above form fields.
- [ ] Buttons 32 (`md`), 28 only inline (`sm`), 40 on login and empty states; icon buttons 32.
- [ ] Only the eight type styles of §3; body 14/22; list titles ≥ 14; nothing a reader reads
      under 12. [U27]
- [ ] Only the spacing, radius and size steps of §4 and §6. [U27, U48]

Color
- [ ] The `you` chip appears at most once per pane; cards that wait for you use the 3 pt `you-bar`, not an amber border.
- [ ] Red only for @ badges, 卡住了 and errors; unread without a mention is a neutral `count-solid` badge;
      完成 and 等{name}… are grey labels. [U20, U25]
- [ ] No per-群 or per-person hues: `hash` glyphs in the sidebar, neutral avatars.
- [ ] Bot avatars up to 64 pt use the small art; pips only for working, needs-input and error;
      one signal per bot across its runs (needs input > error > working > idle).
- [ ] Contrast per §2 and §11 in both themes.

Structure and behavior
- [ ] Short content starts at the top with the start block; the header hides title and chip while
      the block shows; long content anchors at the bottom. [U01]
- [ ] Topic rows: 68 pt, two lines, `作者：内容`, one trailing element, no 进行中. [U02]
- [ ] Every clickable element has hover, pressed, focus-visible, disabled and a pointer (§7.1).
      [U03, U04]
- [ ] Every overlay uses the one overlay component with the scrim. [U05, U36, U37, U38]
- [ ] Never more than one empty state on a screen. [U06]
- [ ] Side panel closed by default, opens by itself on bot work or the first 产出, never empty. [U07]
- [ ] One instruction per place; placeholders name the destination; no permanent key hints;
      send is accent only with text. [U08, U09]
- [ ] Markdown fences after text on the same line parse; never an empty code block. [U10]
- [ ] Header bands 92 pt in the topic list, topic view and side panel, their bottom rules on one y; title rows 52 pt centered with the traffic lights. [U11, U17]
- [ ] Meta line = name 14/600 + bot role tag + time; `（已编辑）` after the body. [U12]
- [ ] Same-author grouping within 5 minutes; groups 12 pt apart. [U13]
- [ ] Jump pill only when the first unread is above the viewport. [U14]
- [ ] Message hover is a soft fill plus the hover toolbar, which never covers the content box
      edge. [U15]
- [ ] Toggle state shown by icon; no shadows except floating layers. [U16]
- [ ] 群 and 私聊 are separate sections; one avatar rule; one initial for CJK. [U18, U37]
- [ ] Sidebar search opens ⌘K; nothing beside it; one selection at a time. [U19]
- [ ] Footer shows name + presence and opens the account menu. [U21]
- [ ] Topic list header 52 pt (`title` 16/24) with the icon-only 新话题 button; tabs 40 pt; counts hidden at 0.
      [U22]
- [ ] One date format (§8.2). [U23]
- [ ] No hairlines between topic rows; unread dot inside the row. [U24]
- [ ] Every truncation uses `…` from one helper; file names keep the extension. [U26]
- [ ] Cards: title 14/600, body 14, one primary, `md` buttons; one width for cards, files and
      images. [U28, U29]
- [ ] File rows: skeleton while loading; actions on hover; 产出 and files share a row. [U30]
- [ ] Process view: steps with icons, output in mono on `code-bg`, opens as a side-panel
      sub-page. [U31]
- [ ] Typing line for people only, aligned to the field text; no reserved space when empty. [U32]
- [ ] Sending dims the body; failure shows `没有发出` + the text button `重试`. [U33]
- [ ] Switcher: grouped, whole rows, recent items with no query. [U34, U35]
- [ ] Login: centered column, label above the 8-box code, `使用其他服务器` on the built-in server, dark icon. [U39]
- [ ] Settings: help under its control, inline errors, danger row apart, headers inside the
      table, status dots by state, plain-text role for yourself, no 成本 page. [U40 to U43]
- [ ] Columns resize and collapse; the topic view never clips; one content box. [U44, U45]
- [ ] Dark: elevation steps distinct; selection stronger than hover. [U46]
- [ ] Copy: PRODUCT.md words, full-width punctuation, Latin spacing. [U47]
- [ ] Motion per §12 with reduce motion checked. Run review-animations, break-ui, apple-design and
      emil-design-eng (docs/design/skills/emilkowalski).

## 18. Open items

- Dark values were computed, not seen: Feishu and Slack dark modes were not captured. Check the
  dark theme on a real display before release.
- `添加 AI 成员` wording for adding a bot (PRODUCT.md has no word for it yet).
- The small bot art (§7.9) is generated in the mockups by removing SVG layers; the app needs the
  same small variant from the character generator in `apps/desktop/src/characters/`.
- The `type` (format) icon is not in the gpui-kit icon set yet; add Lucide `type`.
- The 200 ms entry animations were cut from GrokBot's 280 ms; check the feel in the app.
- GPUI: the sidebar fade, the sticky date divider, the caret-anchored popup and the composer's
  growth to 40 % need custom elements; see GPUI-CAPABILITIES.md.
- The mockups trim text with `fit()` in `kit.mjs`, which estimates widths; the app's helper uses
  measured text (§15 Ellipsis rule).
- People photos: the mockups show initials for everyone because no licensed portraits are in
  the repo. With real photos most rows show a face and the initials disc is the exception.
- A check that diffs this file's token tables against `tokens.mjs` and `theme.json` does not exist
  yet (DESIGN-MD-GUIDE §3).

## Changes in v2.2

- §7.9: the small character art (no lamp, bubble, working ring or sleep marks) is drawn up to
  **64 pt** (was 40), so the 48 pt start block uses it too.
- §7.6, §10: the full-width bracket hang is removed; titles draw a leading `【` or `（` in place.
- §3, §7.7: sidebar section headers are **500** (was 600).
- §8.4: the 权限 words are `管理员` and `普通成员`.
- §7.4: invite codes are 8 Crockford characters (`XXXX-XXXX`); the server limits redeem
  attempts per IP.
- §7.5: reactions are stored with their message in the 群's room.
- §3: text renders without font-smoothing dilation, so stroke weight matches the mockups.
- §8.4: the 关于 page is now 通用.
- §8.3: on the built-in server the login shows `使用其他服务器`, not the host.
- §7.9: a bot's avatar signal across all its runs follows needs input > error > working > idle.
- §8.1: the side panel is closed by default and opens by itself on bot work or the first 产出.
