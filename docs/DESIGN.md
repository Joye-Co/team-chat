# Design v3

**v3 is the design of TeamChat 1.1.0.** It replaces v2.2. Joye found v2 "太朴素、太草稿了，不像
production-ready" and asked for a polished, finished feel, more motion, a rethought left side and a
right side modeled on Claude Desktop. v3 keeps the v2 rules that worked (96 pt composer, closed
type scale, hover states on everything, color means something) and changes the rest: an ink-on-paper
palette (HELMET2), a three-zone shell without the topic-list column, title bars with pane buttons,
a home desk (首页), the topic rail, 单独问, 实时查看, Card v2 and a motion pass. The second part of 1.1.0 adds Card v2.1
(composition, choice forms, status band), the interaction loop, 引用提问, diagrams and 产出 media
(§7.13–§7.17). The structure of this
file follows [design/DESIGN-MD-GUIDE.md](design/DESIGN-MD-GUIDE.md).

**The bar.** Each screen must look as finished as Claude Desktop, Linear and Slack side by side at
the same scale, in light and dark mode. "Acceptable" is not the bar.

Implementers follow this file exactly. When a value is not here, take it from the artboard of
that screen ([design/mockups-v3/](design/mockups-v3/)); when the artboard does not say either, ask
before you invent one. AGENTS.md and docs/PRODUCT.md win for product rules and interface words.
This file wins for how things look and move.

| What | Where |
| --- | --- |
| Artboards (visual truth: `.dc.html` source + light and dark PNG at scale 1) | [design/mockups-v3/](design/mockups-v3/) |
| Tokens in source form (the `<helmet>` block of every artboard) | §2 of this file; the same block heads each `.dc.html` |
| Earlier mockups (login and settings layout still apply) | [design/mockups-v2/](design/mockups-v2/) |
| App icon (`icon-dark.svg` on the dark login) | [design/brand/](design/brand/) |
| GPUI limits | [design/GPUI-CAPABILITIES.md](design/GPUI-CAPABILITIES.md) |
| Per-change checklist | §17 of this file |

Units: pt. A web px at device scale 1 equals one macOS pt.

### References

| Name | What we used |
| --- | --- |
| **Claude Desktop** | The left list of work with a live status per row; the right **session rail** of compact cards (where it runs, what changed, cost) with a show/hide toggle in the title bar; **side chat** (⌘;) that reads the main thread and never writes back to it; panes opened from title-bar icon buttons |
| **Linear** | Ink-on-paper neutrals, grey selection, quiet hairlines, mono numerals, one primary per view |
| **Slack** | Message rows, the composer with a toolbar row, Block Kit-style cards, emoji reactions |
| **Feishu** | 话题群 model: every message belongs to a topic; card fields as key-value rows |
| **GrokBot** | Members as characters; the ask card with lettered options; the draft card before anything irreversible |
| **OpenBot** | Measurements only (PolyForm Noncommercial: no code, styles or assets copied): one header height, neutral selection, one status per row, 400 ms delay before busy indicators |

## 0. Decisions per screen

| Screen | We follow | Why | Rejected |
| --- | --- | --- | --- |
| Shell | **Claude Desktop**: left rail · page · right pane. The topic-list column is removed | Topics live in the rail with their live state; the page gets the width | v2's four columns (sidebar, topic list, topic, side panel) |
| Left rail | **Claude Desktop** list of work, in three modes the user picks (A work, B attention, C members) | Joye: "直接放人，然后接话题产出，感觉怪怪的"; each mode has a real tradeoff (§8.2), so it is a setting | One fixed rail; people-first sidebar sections |
| 首页 | A desk: what needs you, what is done, what is moving | Opening the app answers "what do I do now" without a click | 待办 as a sidebar section |
| Topic page | **Slack** rows + **Claude Desktop** session rail on the right | Context (owner, plan, change, 产出, cost) beside the conversation, not in it | v2's closed-by-default side panel |
| 单独问 | **Claude Desktop** side chat (⌘;) | Ask a member about a topic without noise in the topic | A DM that loses the topic context |
| 实时查看 | **Claude Desktop** panes; the member's screen with 回放 and 接管 | Trust comes from seeing the work, and from stopping it | A tool log in the timeline |
| Cards | **Feishu** fields + **GrokBot** ask and draft + **Slack** Block Kit, as **Card v2** | One spec for 首页, 话题, 群 and notifications | Members that format their own messages |
| Card composition | Semantic containers (上下叠, 并排, 分组, 对比, 标签页, 问卷) and more choice forms, with a status band on every card | Joye: 「组合有无限的可能」, yet it must not look like AI slop: the member picks structure, TeamChat picks the look | Members that set widths, colors or styles |
| Interaction | **Claude Artifacts**: every click in a card or a 产出 page goes back to the member, which changes it in place | The member does the work, so it answers the gesture; one shared card per 话题 | A static card that only answers once |
| 引用提问 | Point at one part (cell, point, step, node) and ask | The member knows exactly what is asked and the value that was seen | Describing the part in words |
| ⌘K | **Linear** command bar: hand off work or search in one field | Handing off work is the main action; it deserves the main shortcut | A separate search dialog |
| 群, 成员, 产出 | Pages in the main window with a rail of cards | One window; no settings detour to see a member | 产出 only in a separate window; 成员 only in settings |
| Settings, login | v2.2 layout with v3 tokens; a new 外观 page | They worked; only the palette changes | — |

## 1. Principles

Ranked: when two conflict, the higher one wins.

1. **Readable first.** Body 14/22; rail and list rows 13/20; nothing a reader must read under 12.
   Text inside a real-content thumbnail (a miniature of a page, terminal or table) is decoration,
   not reading text, and may be smaller.
2. **Color means something.** Amber: the viewer is waited on. Blue: live work. Red: a problem.
   Green: a passed check or a done step, only as a glyph. Everything else is ink and grey. Primary
   buttons are ink, never blue.
3. **Finished, not drafted.** Every card has `lift`, every primary button `btn-hi`, every tile
   `tile-edge`; numbers are mono; previews show real content, never grey bars.
4. **Copy what works.** Every pattern names its reference (above).
5. **Subtract.** One status per row, one primary per card, one hairline per boundary, a state said
   once per screen. A rail card with no data is hidden, not empty.
6. **Every interactive thing answers.** Hover, pressed, focus-visible, disabled and a pointer on
   every row and control (§7.1).
7. **Members are equal.** People and members with a character share rows and sizes. Members with
   a character show it in a rounded-square tile; people are circles.
8. **Motion shows life, not decoration.** Live work breathes; first paint rises once; answers draw
   their check. Nothing moves on keyboard navigation or selection (§12).

## 2. Color tokens

Source: the HELMET2 `<helmet>` block of the artboards. `L` light, `D` dark. Values are sRGB; the
theme JSON writes them as `#RRGGBBAA`. Contrast is WCAG 2.2 over `bg` unless stated. Views never
write a color literal; text tiers are tokens, never opacity. `theme::Colors` holds every token under
the same name in snake_case (`ink_2`, `you_bg`); a different name in code must name the HELMET2
token in its doc comment.

### 2.1 Surfaces

| Token | L | D | Role |
| --- | --- | --- | --- |
| `bg` | `#FFFFFF` | `#111113` | Page, cards, composer, side chat, dialogs, secondary buttons |
| `rail` | `#F7F7F8` | `#161618` | Left rail, right rails (topic rail, 群资料, 工作环境, 来龙去脉, 正在做) |
| `sunken` | `#F3F3F4` | `#1C1C1F` | Tags (岗位), kbd, empty send button, decision-record rows, quiet notes, data wells inside cards |
| `raised` | `#FAFAFA` | `#151517` | Card footers (the action row), content wells in a card |
| `screen` / `screen-ink` / `screen-dim` | `#16161A` / `#C9C9D1` / `#7A7A84` | `#0B0B0D` / `#C9C9D1` / `#6E6E78` | The member's screen and terminal thumbnails (dark in both themes) |

### 2.2 Text

| Token | L | D | Contrast L / D (on `bg`, on `rail`) | Role |
| --- | --- | --- | --- | --- |
| `ink` | `#111113` | `#F1F1F3` | 18.9, 17.6 / 16.7, 16.0 | Titles, body, names, selected rows |
| `ink-2` | `#52525B` | `#A9A9B2` | 7.7, 7.2 / 8.1, 7.7 | Secondary text, rail rows at rest, icons at rest, tag text |
| `ink-3` | `#6E6E76` | `#8C8C95` | 5.1, 4.7 / 5.7, 5.4 | Meta, times, section labels, placeholders, breadcrumbs |

There is no separate placeholder token: placeholders use `ink-3`, so they pass 4.5:1.

### 2.3 Lines, fills and buttons

| Token | L | D | Role |
| --- | --- | --- | --- |
| `line` | `#EBEBED` | `#252529` | 1 pt card borders, hairlines between rows and columns, title-bar rule |
| `line-2` | `#DCDCE0` | `#34343A` | 1 pt borders of the composer, fields, secondary buttons, option rows |
| `hover` | `#F0F0F2` | `#1D1D20` | Hover fill of rows and ghost buttons |
| `sel` | `#E9E9EC` | `#26262A` | Selected rail row, selected pane button, selected tab or segment, active row in ⌘K and menus. Grey, not an accent tint |
| `btn` / `btn-ink` | `#111113` / `#FFFFFF` | `#F1F1F3` / `#111113` | Primary buttons (18.9:1); the viewer's own avatar |
| `btn-quiet` | `#F1F1F3` | `#222226` | The quiet card button (secondary action in a card): soft fill, no border. Hover `#E8E8EB` / `#2A2A2F`, pressed `#E1E1E5` / `#313137` |
| `person` | `#E6E6E9` | `#2C2C31` | People's avatar circle without a photo (initial in `ink-2`) |

### 2.4 State colors

Each state has a mark color (dots, rings, glyphs), an ink (text) and, for amber, a background.

| Token | L | D | Contrast of the ink L / D | Means |
| --- | --- | --- | --- | --- |
| `you` / `you-ink` / `you-bg` | `#E8590C` / `#B54708` / `#FFF4EC` | `#F27A2E` / `#F7A26A` / `#2B1C12` | 5.4 / 9.3 (5.0 / 8.1 on `you-bg`) | The viewer is waited on (需要你拍板, 等你). Only the waited-on viewer sees amber |
| `live` / `live-ink` | `#3B6EF5` / `#2453D1` | `#6B93F7` / `#93B1FA` | 6.5 / 8.9 | Live work: breathing rings, the current plan step, the computer-active dot, 实时 |
| `ok` / `ok-ink` | `#1F9D55` / `#177245` | `#3CC27A` / `#6AD69C` | 6.0 / 10.5 | Done steps, passed checks, `+` line counts |
| `bad` / `bad-ink` | `#E5484D` / `#C42B30` | `#F2555A` / `#FF8A8E` | 5.6 / 8.3 | 卡住了, failed checks, errors, `−` line counts |

Rules: a mark color (`you`, `live`) is for shapes of at least 7 pt and for rings; text uses the
`-ink` value. Amber appears at most once per card and never for someone else's wait (that is
`等{name}` in `ink-3`). No red count badges: the 首页 count is `you-ink` mono text.

### 2.5 Member tiles

Each member with a character has a named tint; the character art is drawn in `tile-ink`.

| Token | L | D |
| --- | --- | --- |
| `c-jojo` | `#F3E6BF` | `#CFC39E` |
| `c-tata` | `#DCE5F7` | `#B8C1D2` |
| `c-pocket` | `#E4E2F4` | `#C0BED0` |
| `c-forge` | `#F6E1D3` | `#D0BDB0` |
| `c-watch` | `#D8ECEF` | `#B4C6C9` |
| `c-scout` | `#DCEFE2` | `#B7C9BC` |
| `c-ledger` | `#F4DDE3` | `#CDB9BE` |
| `c-taro` | `#EFDDEB` | `#CAB9C6` |
| `tile-ink` | `#2D3272` | `#232858` |
| `ph-1` / `ph-2` / `ph-3` | `#D9D3C7` / `#C9D5D3` / `#E2D2C4` | `#4A463F` / `#3D4645` / `#4E463F` |

Dark tints stay mid tones, lighter than every dark surface, so the navy art stays visible. A member
added later without a named tint takes the tint of the roster character it uses, else `c-tata`.
`ph-*` are photo placeholders inside draft previews only.

### 2.6 Shadows

| Token | L | D | Used by |
| --- | --- | --- | --- |
| `lift` | `0 1px 2px rgba(17,17,19,.05)` | none | Every card, file row, rail card, pinned decision card, composer in a page |
| `btn-hi` | `inset 0 1px 0 rgba(255,255,255,.16)` | `inset 0 -1px 0 rgba(0,0,0,.14)` | Every primary button |
| `sec-hi` | `0 1px 1px rgba(17,17,19,.05)` | none | Secondary buttons, the composer, the rail search field |
| `tile-edge` | `inset 0 0 0 1px rgba(17,17,19,.07)` | `inset 0 0 0 1px rgba(0,0,0,.2)` | Every member tile |
| `pop` | `0 18px 48px rgba(17,17,19,.16), 0 2px 8px rgba(17,17,19,.06)` | `0 18px 48px rgba(0,0,0,.55), 0 0 0 1px rgba(255,255,255,.06)` | ⌘K, menus, popovers, dialogs, toasts |

GPUI draws outer box shadows. An inset shadow (`btn-hi`, `tile-edge`) is drawn as a 1 pt inner
border or a 1 pt top or bottom line in the same color; the result must match the artboard at 2x.

### 2.7 gpui-component theme mapping

One JSON theme set (`apps/desktop/assets/theme.json`, `light` and `dark`). App-owned roles (state
colors, member tints, shadows, `screen*`) live in `theme::Colors`.

| Slot(s) | Token |
| --- | --- |
| `background` / `foreground` | `bg` / `ink` |
| `muted` / `muted_foreground` | `sunken` / `ink-2` |
| `border` / `input` | `line` / `line-2` |
| `ring` | `live` at 55 % |
| `selection` / `caret` | `live` at 20 % (L) or 36 % (D) / `ink` |
| `overlay` (scrim) | `#11111366` L / `#00000099` D |
| `accent` / `accent_foreground` (highlighted row) | `sel` / `ink` |
| `primary` / `_hover` / `_active` / `_foreground` | `btn` / `btn` at 90 % / `btn` at 82 % / `btn-ink` |
| `secondary` / `_hover` / `_active` / `_foreground` | `bg` / `hover` / `sel` / `ink` |
| `danger` / `_foreground` | `bad` / `#FFFFFF` |
| `success`, `warning`, `info` | `ok`, `you`, `live` |
| `list` / `list_hover` / `list_active` | `bg` / `hover` / `sel` |
| `popover` / `popover_foreground` | `bg` / `ink` |
| `sidebar` / `_foreground` / `_accent` / `_accent_foreground` | `rail` / `ink-2` / `sel` / `ink` |
| `tab_active_foreground` / `tab_foreground` | `ink` / `ink-2` |
| `switch` (off) / `switch_thumb` | `line-2`, `btn` when on / `#FFFFFF` |
| `progress_bar`, `slider_bar` | `ink` (plan progress uses its own segments, §7.10) |
| `scrollbar_thumb` / `_hover` | `ink-3` at 25 % / at 40 % |
| `link` | `live-ink` |
| `blue` / `yellow` / `red` / `green` (+ `_light`) | `live-ink` / `you-ink` / `bad-ink` / `ok-ink` (`_light`: the mark at 12 %; yellow light = `you-bg`) |

Theme fields: `font.family` `.SystemUIFont`, `font.size` 14, `mono_font.family` `SF Mono` (falls
back to Menlo), `mono_font.size` 12, `radius` 8, `radius.lg` 12, `shadow` true, `focus_ring` true.

## 3. Typography

System font (SF Pro; PingFang SC for Han; Apple Color Emoji). Mono: SF Mono, Menlo fallback,
tabular figures. Weights 400, 500, 600. Glyphs render without font-smoothing dilation (the vendored
gpui patch in docs/ARCHITECTURE.md).

**Closed scale.** Each style is size, line height and weight together.

| Token | Size / line | Weights | Where |
| --- | --- | --- | --- |
| `page-title` | 17 / 24 | 600 | Title bar of 首页, 产出, 成员, 群 pages; settings page title |
| `topic-title` | 15 / 22 | 600 | Title bar of a topic page; 产出 page title |
| `card-title` | 16 / 24 | 600 | Card titles (the question or result), member name in a detail header |
| `body` | 14 / 22 | 400; 500 lead-ins | Message text, composer, card body, side chat answers |
| `ui` | 13 / 20 | 400; 500 names and selected rows; 600 section headings | Rail rows, buttons, fields, message names, menu rows, card fields, rail-card text |
| `meta` | 12 / 18 | 400; 500 rail-card labels and group headers in the rail | Times, tags, breadcrumbs, labels, hints, chips |
| `mono` | 12 / 18 | 400 | Every time, count, version, id, step counter, line count, cost, fps (`10:42`, `3/5`, `v2`, `+214 −63`, `¥6.20`) |
| `num` | 24 / 30 | 600 mono figures | Metric values in data cards and 这周 stats |
| `code` | 12 / 18 | 400 mono | Diffs, terminal lines, inline code |
| `code-entry` | 20 / 28 | 500 | Invite code boxes only (login) |

Rules:
- Nothing a reader reads under 12. Thumbnails of real content are exempt (principle 1).
- Every number is mono, including inside running Chinese text where it stands alone (`3/5`, `09:55`);
  digits inside a sentence (`148 项通过`) may stay in the body font.
- Titles get `letter-spacing −0.01em` in the artboards; GPUI has no tracking control, so the app
  draws them at 0 (accepted difference).
- A card never uses more than three sizes.

## 4. Spacing, layout and sizes

### 4.1 Spacing

4 pt base. Steps: 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 48. Group related things with 4 to 8;
separate groups with 10 to 16; separate sections with 24 or more.

### 4.2 Control sizes

| Control | Height | Text | Padding | Radius | Icon |
| --- | --- | --- | --- | --- | --- |
| Button `md` (default: card actions, pane footers) | 32 | `ui` 13/500 | 0 14 | 8 | 16 |
| Button `sm` (inline: 记一条, 重试 in a row, 选这个) | 28 | `ui` 13/500 | 0 10 | 7 | 14 |
| Card action (Card v2 footers, 首页 asks, the pinned card: primary, quiet and text alike) | 28 | `ui` 13/500 | 0 12 | 7 | 14 |
| Text action (看改动, 验收报告, 展开) | 28 to 32 | `ui` 13 `ink-2` | 0 10 | 8 | — |
| Title-bar icon button | 28 × 28 | — | — | 7 | 16 |
| Rail icon button (新话题) | 28 × 28 | — | — | 7 | 16 |
| Rail search field | 34 | `ui` 13 `ink-3` | 0 8 0 10 | 9 | 15 |
| Form field (in cards, settings, dialogs) | 36 | `ui` 13 | 0 12 | 8 | 16 |
| Option button (choice in a card) | 36 | `ui` 13 | 0 12 0 6 | 9 | key letter 20 |
| Composer | 96 minimum | `body` | 12 14 | 14 | 16 |
| Send button | 32 × 32 round | — | — | full | 16 |

Hit area is at least 28 × 28.

### 4.3 Icons and avatars

- Icons: Lucide, stroke 1.75 at 16 pt; stroke 2 to 2.5 at 12 to 13 pt. 16 in rows, buttons and the
  title bar; 15 in the rail search; 13 for check glyphs inside text; 12 for chevrons.
- Avatar sizes: 16 in rail rows and event lines; 18 to 20 in card headers and tags; 24 in rail
  cards and the team strip (22 tile + ring); 28 in timeline gutters, member list rows and the desk
  正在做 cards; 40 in a ⌘K 交代 row; 48 in a member detail header.

### 4.4 Main window

| Metric | Value |
| --- | --- |
| Window | Min 1100 × 680, default 1440 × 900, transparent title bar, traffic lights centered on the 52 pt title row |
| Zones | Left rail **260** (resizable 220–320, ⌘⇧S hides it) · page (flexible, min 560) · right pane (by kind, §4.5). Each zone edge is a 1 pt `line` |
| Title bar | **52 pt** in every zone (rail header, page title bar, pane header). All title rows center on y 26 with the traffic lights. A 1 pt `line` under the page title bar only where content scrolls under it |
| Narrow window | Shrink in this order: the right pane closes (its title-bar button stays), the rail to 220, then the rail hides. The page never clips |
| Page content | 首页: one column, max 680, centered in the space left of its 正在做 column. Topic timeline and composer: max 720, 24 pt gutters. 群: max 760. Cards inside a timeline: `min(680, box)` |

### 4.5 Right panes

| Pane | Width at 1440 | Surface | Opens from |
| --- | --- | --- | --- |
| Topic rail | 320 | `rail`, cards on `bg` | Default on topic pages when the window is ≥ 1280; the rail-toggle button (`panel-right`) |
| 单独问 | 380 | `bg` with a 1 pt `line` left edge | 单独问 button, ⌘; |
| 电脑 (实时查看) | 50 % of the window, min 560, max 760 (738 at 1440) | `bg` | 电脑 button; 看 X 的电脑 links; 实时查看 in a plan card |
| 产出 | 344 | `rail` | 产出 button; 在右侧打开 on a 产出 ref |
| 首页 正在做 | 320 | `rail` | Always on 首页 (toggle in its own title bar) |
| 群资料, 工作环境, 来龙去脉 | 300 to 320 | `rail` | Always on 群, 成员 detail, 产出 pages (toggle in the title bar) |

One right pane at a time. Opening another replaces it; its button shows `sel`. Esc or the close `x`
closes 单独问 and 电脑 and returns to the topic rail.

### 4.6 Test sizes

Every screen at 1100 × 680, 1440 × 900 and 1920 × 1080, in both themes and with Increase contrast,
with `--worst-case` data.

## 5. Elevation

| Level | Shadow | Used by |
| --- | --- | --- |
| 0 | none | Rails, title bars, rows, timeline messages |
| 1 `lift` | §2.6 | Cards, file rows, pinned decision card, the composer (`sec-hi`). Rail sections have none (§7.8) |
| 2 `pop` | §2.6 | ⌘K, menus, popovers, dialogs, notifications, toasts |

Only levels 1 and 2 have shadows. Dark mode has no `lift`: cards separate by their `line` border.

## 6. Shape

- **Radii**: 2 progress segments; 5 tags (岗位) and kbd; 6 chips, state tags, small thumbnails;
  7 `sm` buttons and card actions, rail group headers, title-bar icon buttons; 8 buttons, rail rows, fields; 9 option
  buttons, the rail search field; 10 file rows, data wells; 12 cards, the pinned
  decision card; 14 the composer; 16 overlays (⌘K, dialogs). Concentric rule: outer = inner +
  padding.
- **Edges**: 1 pt everywhere (`line` for cards and hairlines, `line-2` for controls). No 0.5 pt
  lines in v3.

## 7. Components

Each component lists anatomy, sizes, states and do and don't. All states use §2 tokens. One row
helper and one control helper in `ui/` apply them; a view never builds a clickable element without
them. Build every control from gpui-component (AGENTS.md).

### 7.1 Shared states

| State | Rows (rail, lists, menus, ⌘K) | Buttons | Fields and composer |
| --- | --- | --- | --- |
| Rest | No fill; text `ink-2` in the rail, `ink` in pages | Primary `btn` + `btn-hi`; secondary `bg` + 1 pt `line-2` + `sec-hi`; text action `ink-2` | 1 pt `line-2`, `bg` |
| Hover | `hover`, at once | Primary at 90 % opacity; secondary and text `hover` fill | Border `ink-3` |
| Pressed | `sel` | Primary at 82 %; others `sel` | — |
| Focus-visible | 2 pt ring in `live` at 55 %, inset 1 pt | 2 pt ring, 2 pt gap | 1 pt `ink-3` border + 3 pt `live` 20 % outer ring |
| Selected | `sel` fill, text `ink` 500, `aria-current` | Pane and toggle buttons: `sel` fill | — |
| Disabled | Content at 40 % | Primary: `sunken` fill, `ink-3` label; others `ink-3` label; no hover | `sunken` fill, `ink-3` text |
| Loading | Skeleton rows in `sunken` after 400 ms | Spinner 14 replaces the icon after 400 ms, min 500 ms | Spinner at the trailing edge |
| Error | — | — | 1 pt `bad` border; message `meta` `bad-ink` 6 pt under |
| Cursor | Pointer on every clickable thing | same | I-beam |

### 7.2 Title bar

52 pt, in every page and pane. **Left**: breadcrumb `群 /` (`topic-title` size, `ink-3`; it opens
the 群 page) + title (`topic-title` on topic and 产出 pages, `page-title` on 首页, 产出 list, 成员 and
群 pages) + one state element. **Right**: a group of 28 pt icon buttons, gap 2, each with a tooltip
and an accessible name; a 1 pt × 16 `line` divider before the rail toggle.

| Page | State element | Right buttons |
| --- | --- | --- |
| Topic | One of: amber tag `需要你审 PR #61` (22 pt, radius 6, `you-bg`, 6 pt `you` dot, `meta` 500 `you-ink`) when the viewer is waited on; `live` 6 pt dot + `Forge 在做 3/5 · 跑一万条消息的压测` `ui` `ink-2` while a member works; `等小王` `ink-3`; `做完了 · 09:55` `ink-3`; nothing when open | 单独问 (`message-circle`, ⌘;), 产出 (`layers`), 电脑 (`monitor`, only while a member's computer is in use for this topic, with a 6 pt `live` dot at the top right), 改动 (`file-diff`, only with a change), divider, rail toggle (`panel-right`) |
| 首页 | Date `11 月 3 日 周一` `meta` `ink-3` | — (the 正在做 column has its own toggle) |
| 群 | Purpose `ui` `ink-3`, member stack (20 pt, −4 overlap, max 5) + `9 位成员` | 单独问, 产出, `ellipsis`; 群资料 toggle in the pane header |
| 成员 | Count `13` mono `ink-3`; detail: breadcrumb `成员 /` + name | 单独问, 产出, 电脑, rail toggle |
| 产出 | Breadcrumb `群 /` + title + kind tag (`网页`) | Version picker (32 pt secondary: `v3` mono 600 + `今天 09:40` + chevron), `分享` secondary, `external-link` (open in its own window), 单独问; 来龙去脉 toggle in the pane header |

Do: show the state once (the rail row and the title bar may both show it; the timeline does not
repeat it). Don't: put a primary button in a title bar (except 交代给 X on a member detail).

### 7.3 Left rail (all modes)

Anatomy, top to bottom: header (52: traffic lights, `square-pen` 新话题 28 pt at the right) · search
field `搜索或交代` with `⌘K` kbd (34 pt, `bg`, 1 pt `line`, `sec-hi`, radius 9; opens ⌘K) · 10 pt ·
destinations (30 pt rows, radius 8, padding 0 10, gap 10, icon 16 + `ui`): 首页 (`house`, trailing
count in `you-ink` mono when something needs you), 产出 (`layers`), 成员 (`users`) · 16 pt · the
mode's list (§8.2) · the team strip · the footer.

| Part | Value |
| --- | --- |
| Section header | 28 pt, padding 0 6 0 10: label `meta` 500 `ink-3` (`话题`, `成员`); right a 22 pt ghost menu button (`我参与的 ▾`, `先看在忙的 ▾`) |
| Group header | 26 pt, radius 7, padding 0 10 0 8, gap 6: chevron 12 + 群 name `meta` 500 `ink-3`; clicking the name opens the 群 page, the chevron collapses |
| Topic row | 30 pt, radius 8, padding 0 10, gap 8: a 16 pt state slot · title `ui` (one line, `…`) · trailing mono `meta` `ink-3` (`3/5`, `09:12`) or `等小王` |
| State slot | One by priority: 7 pt `you` dot (the topic waits on the viewer) > the working member's 16 pt tile with a breathing `live` ring > `circle` 8 pt hollow `ink-3` ring (waits on someone else) > `check` 13 `ok` (做完了 today) > nothing |
| Team strip | 24 pt tiles of the active members (working first, then waiting on the viewer) with their rings, gap 4, then `+8` `meta` `ink-3`; click opens 成员 |
| Footer | 40 pt: 22 pt viewer circle (`btn` / `btn-ink` initial) + name `ui` 500 + a 28 pt `sliders-horizontal` settings button |
| Scroll edges | 24 pt fade from `rail` at top and bottom while the list scrolls |

States: §7.1. Selected row: `sel`, `ink` 500. The rail shows one selection at a time (a
destination, a 群 header or a topic).

### 7.4 Composer

| Part | Value |
| --- | --- |
| Box | Min 96 pt; radius 14; `bg`; 1 pt `line-2`; `sec-hi`. In a topic: in the content box, 16 pt from the bottom |
| Text area | Padding 12 14 0; `body` 14/22; min 2 lines; grows to 40 % of the page height, then scrolls |
| Toolbar | 40 pt, padding 0 8 8: left 28 pt icon buttons `paperclip` (附件), `at-sign` (提到); right a hint (`只想问问？` `meta` `ink-3` + kbd `⌘;` + `单独问` text action, topic pages only), then send |
| Send | 32 round. Empty: `sunken` fill, `ink-3` arrow, disabled. With text: `btn` fill, `btn-ink` arrow, `btn-hi` |
| Placeholder | Names what happens: topic `回复，或 @ 成员交代…`; 群 `在开发里开个话题，或 @ 成员交代…` + hint `Jojo 会先看一眼，再交给合适的成员`; while a member works `插一句…` + hint `Forge 会在下一步前看到`; 单独问 `单独问 Jojo…` + hint `⌘; 关闭` |

States: rest, hover (border `ink-3`), focus (§7.1), has text, disabled (no permission:
`sunken`, placeholder `你没有在这个群发言的权限`, toolbar hidden), blocked (a slab above the box,
`raised`, 1 pt `line`, `meta` text + one `sm` action). Keyboard: ↵ sends, ⇧↵ new line, ↑ in an empty
composer edits my last message, Esc blurs. @mention popup: v2.2 behavior, v3 tokens (`bg`, `pop`,
radius 12, 36 pt rows, active row `sel`).

### 7.5 Buttons

Variants: **primary** (ink, one per card or view), **secondary** (bordered), **quiet** (the
secondary of a card: `btn-quiet` fill, no border, no shadow), **text action** (no border, `ink-2`),
**danger** (only in a confirm dialog: `bad` fill, white label). Every action in a card is the
28 pt card action (§4.2): one height and one radius 7 for primary, quiet and text, so a card's
action row reads light; the primary stays the only ink fill. Bordered secondaries stay for pages,
dialogs and settings. Labels are
verbs from the copy catalog (`批准合并`, `发布到生产`, `确认发布`, `接管`, `交还`, `停止`). Destructive
or irreversible actions open a confirm step (§7.10 批准).

### 7.6 Timeline message

| Part | Value |
| --- | --- |
| Row | Gap 12 between gutter and text; author groups 16 pt apart; a continuation 4 pt below |
| Gutter | 28 pt avatar (person circle or member tile with its state ring) |
| Meta line | 20 pt: name `ui` 500 · 岗位 `meta` `ink-3` for people, the 岗位 tag for members (§7.9) · time mono `ink-3` |
| Body | `body` 14/22, 4 pt under the meta line; paragraph gap 8 |
| Event line | 20 pt, inset to the text column: 16 pt tile or glyph + `meta` `ink-3` (`Jojo 转给了 Forge`, `Forge 开始用 Mac mini`) + mono time |
| Decision record | A full-width 40 pt row on `sunken`, radius 10: `check` 13 `ok` + `Joye 选了「改成指数退避，放进同一个 PR」` `ui` + mono time + text action `看问题` |
| Date divider | Centered `11 月 3 日 周一` `meta` `ink-3` between two 1 pt `line` rules |
| File row | 300 wide, radius 10, 1 pt `line`, `lift`, padding 6 12 6 6, gap 12: a 72 × 44 real-content thumbnail (radius 6; a video shows its first frame or the `screen` miniature with a play disc), name `ui` 500, meta mono `0:18 · 4.2 MB` |
| Reactions | 24 pt pills, radius 12, `sunken`, emoji 14 + count mono; mine 1 pt `ink-3` ring |
| Hover | `hover` fill on the row + the hover toolbar (`bg`, `pop`, 28 pt buttons: reaction, quote, 单独问 about this, more) |

States: rest, hover, focus-visible, sending (body 60 %), failed (`circle-alert` 14 + `没有发出`
`bad-ink` + text action `重试`), highlighted from search (2 s `you-bg` fill that fades).

Members acknowledge with a reaction (`👀`), not a text line (§9.2).

### 7.7 Pinned decision card

Above the composer on a topic page, when the topic has an open ask for the viewer (the newest one).
Min 62 pt, radius 12, 1 pt `line`, `lift`, padding 12 16, gap 12: 7 pt `you` dot + title `ui` 600
(`审 PR #61`) over a `meta` `ink-3` line (`12 个文件 · 批准后 Forge 合并到 main，话题完成`), then at
the right the card's actions (text action, secondary, primary). After the answer it becomes the
record line (§7.10 answered) for 4 s, then leaves; the record stays in the timeline.

### 7.8 Rail card (topic rail, 群资料, 工作环境, 来龙去脉, 正在做)

A rail section, not a boxed card: no border, fill or shadow; sections sit on the pane's `rail`
surface, padding 14 4, and a 1 pt `line` (inset 4) divides each two (`ui::rail_section`,
`ui::rail_stack`). Rail padding 4 16 16 for the topic rail; side columns keep their 52 pt header.
The same holds for 正在做 (a clickable run section hovers with an 8 pt bleed, radius 8; its
terminal thumbnail keeps its own radius 8), 群资料, 来龙去脉 and 工作环境. Label row:
`meta` 500 `ink-3` + optional right text action (`看改动`, `打开`, `展开`, `记一条`). A card with no data is
not drawn.

Topic rail cards, in this order:

| Card | Content |
| --- | --- |
| 负责 | 24 tile + name `ui` 500 + 岗位 tag + right state (`✓ 做完了 · 09:55` or `在做 · 3/5`); line 2 the asker `小王 提出 · 09:12` (16 avatar, `meta`) |
| 计划 | The plan: ✓ done (`ink-3`, mono duration), ● current (`live`, breathing), ○ todo; segments bar (4 pt, radius 2, gap 4: done `ink-3`, current `live`, todo `line-2`). When done it collapses to `✓ 5 步都做完了 · 41 分钟` with `展开` |
| 改动 | Title `ui` 500; `#61 +214 −63` mono (`ok-ink` / `bad-ink`) with a 5-cell diffstat; `12 个文件`; checks `✓ 3/3 项检查通过`; status tag (`未合并`, `合并中`, `已合并`). No merge button here: the decision is the pinned card |
| 产出 | A real-content thumbnail (title + tiny table) 96 tall, radius 8, `line`; name `ui` 500 + `v2 · 09:55` mono; extra rows for recordings (`▶` tile 32 × 22 + name + `0:42`) |
| 用到的决定 | `check` + decision text `ui` + `Joye 09:31` (16 avatar, mono) |
| 电脑 | `monitor` 28 tile + `Mac mini` + `Forge 用完了 · 09:52`; right `○ 空闲` or `● 在用` (`live`) + text action `看 Forge 的电脑` |
| 用时和成本 | `用时 41 分钟 · ¥6.20` mono + right `今天 09:14–09:55` |

### 7.9 Member identity: avatars, rings and tags

- **People**: circle, `person` fill, initial `ink-2` 600 (one character for CJK, one letter at 28 pt
  and below, two letters at 40 pt and above). The viewer's own circle is `btn` with a `btn-ink`
  initial. A photo replaces the circle fill. Away people: avatar at 50 %, name `ink-3`. No presence
  dots and no typing dots.
- **Members with a character**: a rounded-square tile, radius ≈ 0.3 × size (16→5, 20→6, 24→7, 28→8,
  40→12, 48→14), filled with the member's tint (§2.5), `tile-edge`. The jojo-friends character art
  for the member's current status sits centered at **72 %** of the tile (rules and art in
  docs/ROSTER.md and `apps/desktop/assets/characters/`). The tile is `aria-hidden`; the name is
  always text beside it.
- **Status art** by context: a working ring → `working`; a waiting ring or an ask to the viewer →
  `needs-input`; a finished result in that spot (做完了 rows and cards, answered states, proof) →
  `success`; queued (排队) → `idle`; else the member's overall status. One overall status per member
  across all its runs: needs input > error > working > idle.
- **Rings** replace v2's pips: a 1.5 pt ring, 3 pt outside the tile (`inset −3`), radius tile + 3.
  `live` and **breathing** (opacity 0.8 ↔ 0.25, 2.4 s, ease-in-out) while working; `you`, static,
  while it waits on the viewer; `bad`, static, when stuck; none when idle. A ring is drawn inside
  the slot's layout box, so rows keep their height. Rings show from 16 pt up.
- **岗位 tag** after a member's name: 18 pt, radius 5, `sunken`, padding 0 6, `meta` `ink-2` (`开发`,
  `运维`). People show 岗位 as plain `meta` `ink-3`.

### 7.10 Card v2

The generative UI. Members send semantic cards; TeamChat renders them. One spec for 首页, 话题, 群,
产出 pages and notifications ([Cards artboard](design/mockups-v3/Cards.dc.html)). Members cannot
choose layout or colors, stream half-formed text into a card, post progress nobody asked for, or
skip the confirm step before an irreversible action.

**Anatomy.** Card: radius 12, 1 pt `line`, `bg`, `lift`, width `min(680, box)` (首页) or the
message column. The card is the only outlined level: nothing inside it has both an outline and a
fill (lists are divided rows, wells are soft fills, proof chips are `sunken` pills). Body padding
14 16 16:
1. Header (18 pt): 20 tile + name `ui` 500 + 岗位 tag + `群 / 话题` breadcrumb (`meta` `ink-3`, topic in
   `ink-2`, a link) + mono time at the right: when the card message was created (a new rev or
   an edit changes the card, not this time). In the card's own topic the breadcrumb is dropped and
   the intent state takes its place (`✓ 做完了`); the 28 pt gutter avatar stays beside the card.
2. Title: `card-title` 16/24, 10 pt below the header; one sentence that says what is wanted.
3. Blocks (below), 10 to 12 pt apart.
4. Footer: in the card under the blocks, no band, no rule and no fill (`ui::card_footer`): padding
   0 16 14, card actions (§4.2) primary, quiet, text, gap 8, and an optional right note `meta` `ink-3` (`发布前会再确认一次`, `🔒 发出后不能撤回` as `lock` 13 +
   text). Exactly one primary. A two-option choice needs no footer.

**Intents** set the header state and the amber rule: `ask` (needs someone; amber dot before the
section on 首页, `需要你拍板` only for the asked viewer), `done` (`✓ 做完了` `ok` glyph + `ink-3`),
`working` (`live` dot), `info` (nothing).

**Blocks.**

| Block | Look |
| --- | --- |
| Fields | Key-value grid: key 56 `ink-3`, value `ink`, `ui` 13/20, row gap 6; a check glyph in `ok` may lead a value |
| Decision (选择) | 2 short options: one row of option buttons (36, radius 9, 1 pt `line-2`, key letter tile 20 on `sunken` + label), a press answers. 3–6 options: a column, one marked `推荐` (`meta` `ink-3` tag), an optional `补充一句` field, footer `提交` disabled until chosen. A recommendation line under the options: `Forge 推荐 A：…` `meta` `ink-3`. Chosen: 1 pt `ink` border + `check` at the right |
| Approval (批准) | Fields (包含, 验收, 影响) + primary verb (`发布到生产`). Pressing opens a confirm dialog: title, one sentence, a field `输入 qqbot-prod 确认`, `确认发布` disabled until the name matches, `取消`; Esc cancels |
| Form (表单) | Max 8 fields, 36 pt, labels above (`meta` 500 `ink-2`); chips for multi-select (selected `ink` border); footer `提交` disabled with a note `还差名字、岗位、入职日期` |
| Draft (草稿) | The real object in a `raised` well, radius 10, padding 12: images (radius 6), title `ui` 600, body `ui` `ink-2`. `改一下` turns the well into fields with counters (`18/20`) in place; `改好了，确认发布` replaces the whole draft |
| Plan (计划) | The rail plan list (§7.8), with mono durations and a `看 Forge 的电脑` / `实时查看` action; a waiting step says `· 等 Joye 拍板` + `看问题` |
| Metric (数据) | Up to 4 numbers in a row of cells split by 1 pt `line`: label `meta` `ink-3`, value `num`, change `meta` mono (`+12% 比 9 月`; up or down said with a sign, never by color alone; lower-is-better metrics say so) + a tiny bar or sparkline |
| Chart | One line or bar chart, `ink` for the main series, `ink-3` / `line-2` for others, labels on the lines, axis `meta` mono; a legend only for more than 3 series |
| Table | Header `meta` `ink-3` over a 1 pt `line`; 32 pt rows (`TABLE_ROW`), 1 pt `line` between. A column whose cells are all numbers (after the first) is a number column: right-aligned at the row end, mono cells, as wide as its widest header or cell and at least `TABLE_NUM_COL` (52 pt); text columns share the rest. A percent cell has a 48 pt bar (`TABLE_PCT_BAR`) before it. A total is an ordinary last row (Card v2 has no `summaryRow`): when its first cell is `合计` it is drawn 600 with a 1 pt `line-2` rule above. A link to the full 产出 |
| Diff (对比) | A `sunken` well, radius 8, no outline: file header (`src/sync/reconnect.ts` mono + `+5 −2`) over a 1 pt `line-2`, one hunk in `code` with `ok` / `bad` 10 % line fills (code text stays `ink`), `还有 11 个文件`, `在 GitHub 打开` |
| Evidence (证据) | Up to 3 thumbnails (real screenshots, radius 6) then `+n`; a recording tile with mono duration; test result chips (`✓ 测试 148 项通过 · 0 失败`). No evidence: a neutral line `这次没有附证据` + secondary `让 Watch 补上` (never red) |
| Refs (链接) | No box of its own: rows divided by a 1 pt `line`, their hover fill reaching 8 pt past the text (`ui::divided`, `ui::inset_row`). One row style for PR, 产出, 话题, 成员: 16 glyph or tile · name `ui` 500 · one line `meta` `ink-3` · right state. Stale state says how old (`PR 状态 10 分钟前`); a deleted 产出 keeps a row `产出已删除 · 原来是「…」` |
| Proof chips | 26 pt, radius 7, `sunken` fill, no border: `#61 +214 −63`, `✓ 测试 148 项通过`, `▶ 录屏 0:42` (mono numbers) |
| v1 blocks | Text, fields, table, list, progress, code, image, divider, link render with the same tokens |

**States** (each one is drawn; the card changes in place, no new message):

| State | Treatment |
| --- | --- |
| pending | Controls live for the asked viewer |
| waiting on someone | Other viewers: controls drawn but disabled, footer note `J 等 Joye 拍板` (16 avatar) + `只有被问的人能选` |
| sent | The answer is on its way: controls disabled, spinner after 400 ms on the pressed control |
| answered | Controls collapse to one record line that rises in: `check` drawn (§12) + `你选了 B：8 个分格，逐位输入 · 已发给 Forge` + mono time + text action `撤回` (while the member has not started). Others see `Joye 选了 B` + `展开` |
| executing | An approval becomes progress in place: `Joye 批准了 10:40`, `第 2/3 步`, a step list with live counters (`已重启 9/24`), a moving `live` bar |
| done | Result with proof and the reverse action at hand (`回滚到 v2.13` secondary + note `回滚约 2 分钟，随时可以`) |
| failed | `circle-alert` `bad` + what failed + `重试` |
| expired | The ask passed its time: muted body, note `过期了` + `重新问` |
| superseded | A newer draft replaced it: a `sunken` bar `Taro 改了草稿，这一版作废` + `看新版`; controls removed |

**Per-viewer controls**: only the members in the ask's `from`/addressee list get live controls;
everyone sees the same card. **v1 fallback**: 1.0.x clients get the server-derived v1 card (title,
fallback text, fields, v1 buttons).

**Notifications** (系统通知): only 做完了 and 需要你拍板. Title `Watch：把 qqbot v2.14 发布到生产吗？`,
one line of fields, the same two actions on hover; an irreversible action opens the app's confirm
step instead of acting.

### 7.11 ⌘K 交代或搜索

Overlay 640 wide, 96 pt from the window top, radius 16, `bg`, `pop`, scrim. Query row 52: `search`
18 + query `body` 15/22 with the caret + a destination chip at the right (`发到 夹夹增长 ▾`, 28 pt,
1 pt `line-2`). Sections (label 28 pt `meta` 500 `ink-3`): **交代** first when the query reads as a
request: `交给 Ledger` (40 tile, name + 岗位 tag, line 2 `开一个话题：拉一下 10 月各渠道的新增`, right
the member's state `○ 空闲` / `● 在做 本周周报` + kbd `↵`) and `交给 Jojo 安排` (kbd `⌘↵`); then 话题, 产出,
成员 (44 pt rows, 24 avatar or a real-content thumbnail, title `ui` 500, context `meta` `ink-3`, mono
time). Matches are marked with `you-bg` text background, radius 4. Active row `sel`, radius 10.
Footer 40 on `raised`: kbd hints `↵ 交给 Ledger`, `⌘↵ 交给 Jojo`, `↑↓ 选择`, `esc 关闭`. With no
query: 最近 topics, then 成员. ↵ on a 交代 row creates a topic addressed to that member in the chosen 群.

### 7.12 Overlays, menus, dialogs

One overlay component: scrim, `bg`, radius 16 (dialogs, ⌘K) or 12 (menus, popovers), `pop`.
Menus: padding 4, 32 pt rows (radius 8, `ui`, icon 16), separators 1 pt `line`. Dialogs: width
440, padding 20, title `card-title`, footer right-aligned secondary then primary.

### 7.13 Card v2.1: composition and choice forms ([CardsCompose](design/mockups-v3/CardsCompose.dc.html))

Card v2.1 (`catalog: "tc.card/2.1"`) keeps the §7.10 anatomy and blocks and adds composition, more
choice forms and a status band. The member picks the blocks, their order and which ones sit side
by side; TeamChat owns every size, color and motion. Members cannot send style keys (`color`,
`padding`, `width` and the rest are refused by the server).

**Status band** (every card except intent `info`): 34 pt (`card_v2::STRIP`), padding 0 16, `meta` 12/18, 1 pt bottom
rule; a mark (8 pt dot or glyph) + the word `meta` 600 + one detail (deadline, `1/3`, reason), cut
with `…`. The band says the state once; the body never says it again.

| Band | Word | Fill / text | Mark |
| --- | --- | --- | --- |
| amber | `等你` | `you-bg` / `you-ink`; card border `you` 34 % over `line` | `you` dot with a 3 pt `you` 22 % halo |
| grey | `等 Joye`, `等 n 位成员` | `sunken` / `ink-2` | 9 pt ring, 1.5 pt `ink-3` |
| blue | `处理中` | `live` mix / `live-ink` | breathing `live` dot (§12) |
| green | `完成` | `ok` mix / `ok-ink` | check, drawn once |
| red | `失败` | `bad` mix / `bad-ink`; card border tinted | `circle-alert` |
| dimmed | `已作废`, `已过期` | `sunken` / `ink-3`; whole card at 62 % opacity | — |

**Containers** (depth ≤ 2, a row holds 2 or 3 blocks, 80 blocks per card):

| Container | Artboard word | Look |
| --- | --- | --- |
| `stack` | 上下叠 | The default: blocks at the card's block gap (10–12) |
| `row` | 并排 | Columns in the weight ratio (`2 : 1`), split by a 1 pt `line`. Under 560 pt of content width (首页 right column, narrow windows, notifications) it folds to a stack, left block on top; reading and Tab order never change |
| `group` | 分组 | Title row `ui` 600 + count + chevron; collapsed it shows its one-line summary `meta` `ink-3` (`1 个做完 · 1 个在做 · 1 个等你`). Collapse is local per viewer |
| `compare` | 对比 | 2–3 options side by side (4 as a 2 × 2 grid), each with a real preview, label, pros and cons rows and a `选 A` secondary; chosen: 1 pt `ink` border + `已选 A`. A `补充一句` field and `Tata 推荐 A` note |
| `tabs` | 标签页 | gpui-component segmented tabs and one pane; the tab is local per viewer |
| `questionnaire` | 问卷 | Header `问卷 · 3 题` + `必答 1/2`; numbered questions (mono number `ink-3`, kind tag `单选 · 必答`); one `一起提交` primary for the card, `先存着` secondary |

**Choice forms** (all are input blocks; the band detail names the form):

| Form | Band detail | Look |
| --- | --- | --- |
| 单选 + 其他 | `单选，可以写「其他」` | §7.10 option column; `其他` opens a field under it |
| 多选 | `多选，选 2–3 个` | Option rows with check boxes; `已选 2 个 · 至少 2 个，最多 3 个`; when full the rest go to 40 % |
| 排序 (`rank`) | `排序，拖动或用键盘` | 40 pt rows (radius 9, 1 pt `line-2`, `sec-hi`) with a mono rank, a grip, the label and a mono estimate `约 2 周`, 28 pt 上移 / 下移 buttons; a dashed `line-2` rule with `前 3 个进 4.0`; ⌥↑ / ⌥↓ move; `恢复 Tata 排的顺序` text action |
| 打分 (`rating`) | `评分` | Five 36 pt buttons (radius 8, no border) in a radio group, range `1–5 分` at the right; the chosen value says its label `清楚 · 4 分` |
| 量表 (`scale`) | `量表` | 0–10 segmented row, ends `0 · 不会` and `10 · 一定会` |
| 确认 (`confirm`) | `是或否` | Two buttons (`可以` primary, `不行` secondary) that answer at once; no submit |
| 图片选择 (`imageChoice`) | `图片选择` | 2–4 images with a letter tile (20, `sunken`) + label under each; chosen: 1 pt `ink` border; footer `用这张` primary + `看大图` |
| 矩阵 (`matrix`) | `矩阵，每行选一个` | Rows = items, columns = options (max 8 × 5), radio cells, `已选 2/4 行` |
| 时间段 (`slots`) | `选一个时间段` | Day columns of 52 pt slot cells (radius 9) with a mono time and `都能来` (`ok` check) or `缺 Alex` / `缺 2 人`; chosen: `sunken` + 1 pt `ink` border |
| 投票 (`poll`) | `已投 7/13` + `今天 18:00 截止` | 44 pt option rows (radius 9) with the result as an `ink` 10 % fill behind the label, `12 票 · 40%` mono, voter tiles when public; my vote: 1 pt `ink` border + the `你投的` tag; `还没投：…` under; a voted viewer sees the grey band |

**Decision layout** (2.1, Cards artboard 01): two short options sit side by side as one row
of option buttons of equal width, gap 8. Short means a single select without `其他`, and each
option's label plus detail at most 16 characters. Three or more options, a multi-select, `其他`
or a longer option are stacked. A decision in a column of a side-by-side `row` stays stacked
(Interact artboard 01); once the row folds to a stack, the short pair goes side by side again.

**Other 2.1 blocks**: `buttons` (1–3 secondary or text buttons inside the body, never primary),
table row select and sort, chart point select, metric press, input `slider`, `diagram` (§7.16) and
`embed` (a live 产出 page in a frame, §7.17, with the foot `页面里的操作会发给 Ledger`).

**Unknown block** (a newer catalog): a quiet `sunken` box: `info` glyph + `这部分需要新版本` +
mono `tc.card/2.2 · 地图` + the block's `fallbackText`. When it is the only input, the footer shows
`更新 TeamChat` (product name from `PRODUCT_NAME`) and submit is disabled. 1.0.x gets the
server-derived v1 card.

### 7.14 The interaction loop ([Interact](design/mockups-v3/Interact.dc.html))

Every pick, chart point, table row, input and button in a card goes back to the member that made
the card; the member changes the card in place (a new rev), posts a new message, or resolves the
event without a change. A shared card stays one object: everyone sees the newest rev.

- **Optimistic state**: the gesture shows at once (the bar highlighted and the others faded, the
  row selected, the option ticked). Local state (selection, sort, tab, collapse, drafts, the chart
  point) is kept per block id across revs.
- **Pending band** (only the person who acted sees it; it says the stage of their newest event
  once, in the band, in its own tone):

  | Time since the server took it | Band | At the block |
  | --- | --- | --- |
  | 0–400 ms | unchanged | the optimistic state only |
  | 400 ms → soft | blue, spinner: `等 Ledger 回应` | — |
  | soft → hard | blue, spinner: `Ledger 还在处理` (+ `实时查看` for runner members) | — |
  | ≥ hard | grey, still mark: `Ledger 没有回应` | `重试` secondary + `不等了` text action |

  Soft and hard depend on the member (connected SDK member 8 s / 45 s; a run 20 s / 120 s; a run
  that must start 45 s / 180 s). Offline: a red band `失败` + `Ledger 不在线，这一步没有发出` for
  4 s, then the state reverts; an answer gets the grey band `Ledger 不在线，回来后会看到`. Busy:
  the red band `失败` + `Ledger 现在接不了，稍后再试`. A block says the stage itself (spinner and
  words, under the block) only for an older event that the band no longer shows.
- **Sent and answered**: `你点了「B 站」→ Ledger`; for an answer the §7.10 record line
  `你选了 B · 已发给 Ledger` + `撤回`.
- **Update band** (Interact artboard 02 step 3): a rev (2 or later) that a person's event
  caused replaces the band of a done or `info` card with `Ledger 更新了这张卡` (`RotateCw` 16 in
  `ink-2`, the word in `ink`, fill `live` 6 % over `sunken`, `UPDATED_FILL`) and the detail
  `{time} · {why}`. The time is `刚刚` for the first minute, then the mono clock time. The why is
  `按你点的「B 站」` for the viewer whose event caused the rev (the bar's x, the option's label or
  the selected row's first cell, read from that viewer's own sent event), `按你的选择` when that
  event is no longer at hand, and `按 Joye 的选择` for everyone else. Other viewers cannot be told
  which bar was picked: the rev's `cause` names the person and the block, not the value. A card
  with an open question or work in progress keeps its own band; my waiting event's band wins over
  the update band. The band stays until a rev without a cause. Who caused the rev is said only
  here, never per block.
- **Changed and inserted blocks**: a top-level block that the rev changed or inserted spans the
  card's width (padding 12 16) and fades in over 200 ms (`CHANGE_IN`) with a `live` 14 % tint
  (`CHANGE_FILL`) that fades over 1.6 s (`CHANGE_FLASH`, §12). A changed block ends on `bg`; an
  inserted one keeps a steady `live` 7 % tint (`INSERTED_FILL`) and `Ledger 刚加的` (`meta`
  `live-ink`) at its top right, sharing the row of a `问你` / `等 Joye` mark or a group's title, until
  the next rev. Changed or inserted top-level blocks that follow each other sit with no gap
  between them and a 1 pt `line` hairline at the top of each later one; a changed block settles on
  `bg` after its flash, an inserted one keeps its tint. A nested block
  gets a small rounded tint instead (radius 8).
- **Picked bar** (Interact artboard 02): a single-series bar chart the viewer can pick from is
  drawn as rows, one per x: 30 pt rows (`BAR_ROW`, radius 7), the x in a 60 pt column
  (`BAR_NAME`), a 10 pt track on `sunken` (radius 3) and the mono value `ink-2` at the right; hover
  fills the row `hover`. Before a pick every bar is `ink-3` with `ink` names; after one, the picked
  bar is `ink` with its name 600 and the others are `line-2` with `ink-3` names. A second click on
  the picked row clears it (`chart.clear` when declared). The rows keep the chart label (`meta` 500
  `ink`) above, the pick chip `● B 站 · 9` (`meta` 500 `ink`) under them, and the chart's text
  summary (`meta` `ink-3`) last. The chip is left out when the pick is the one that caused the
  rev: the update band already names it. The viewer's own pick that caused the rev stays marked
  in the new rev until they clear it.
- **Conflict**: the card re-renders on the newest rev, keeps the person's values where the block
  and option ids still exist, and a toast says `卡片刚更新了，请再看一眼`. A draft being typed when a
  rev arrives stays, with the inline line `内容变了` + `用新的`.
- **Who can act**: the card says who may send (by default the asker and the responsible member;
  approvals only the asked person). Others see the controls read-only and the grey band; a
  click on a chart only highlights on their own screen.
- **产出 pages that talk both ways** (frame of §7.17): buttons and filters in a member-made page
  go to that member; the member pushes new data into the open page (`Ledger 推来的 · 刚刚`, no new
  version) or adds a version (title bar `已更新到 v4` for 3 s, or the chip `有 v4` when an older
  version is picked); the page can ask the member and wait for the answer in the page (the
  sample page shows its question and the waiting answer right under its tabs, in the chart's
  place, so the inline frame shows them without scrolling). The frame foot says who gets the
  page's actions and the labels of its declared events: `页面里的操作会发给 Ledger · 按渠道看 ·
  数据有问题` (a page that may not send says `页面能做的：读这份数据`); for a runner member that may
  ask it adds `回答会记进工作记录` (whole notice on hover). Line rules: the head
  `页面里的操作会发给 {name}` stays on one line; when even the head cannot fit, it wraps only before
  the name, never inside it. The labels follow in the same run of text, joined with ` · ` (one
  space each side of the dot), take only the room left and are cut with `…`; the whole line shows
  in a tooltip. While a page event waits, the foot shows the same pending words and leaves the
  labels out. A page never approves: an irreversible step becomes an approval card in the 话题.

### 7.15 引用提问 ([Refs](design/mockups-v3/Refs.dc.html))

Any part of a card, a message's Markdown, a diagram or a 产出 can go into the composer as a
reference: a table cell, row or column; a chart point, bar or series; a metric; a list item; a plan
step; a diff line or hunk; a box on an evidence screenshot; a diagram node; an element of an HTML
产出. TeamChat writes the label (`来源 · 块 · 部分 · 值`); the member never does.

| Affordance | Look and behavior |
| --- | --- |
| Hover | The smallest part under the pointer gets `hover` + a 1 pt `line-2` ring at the part's radius |
| `问这里` | After 300 ms of rest, a gpui-component ghost xsmall button with `↳` at the part's top-right (table row: right after the row's name, 「B 站 [问这里]」, without changing the row height; when the name leaves no room in the first column, at the row end, centered on the row; in a card table it floats over the row end so the number columns stay at the right edge. Chart: in the tooltip; diagram: beside the node). Not shown on answer controls |
| ⌥-click | Adds the part at once, also on controls (it does not pick the option); while ⌥ is held, parts show the outline |
| Context menu | `就这里提问`, `单独问这里`, `复制`; on the card header `就这张卡提问` |
| Keyboard | Tab into a block, arrows move a 2 pt `ink-3` part ring, ⌥↵ adds the part, ⇧⌥↵ the whole block |
| Added | The part flashes `sel` for 600 ms and keeps a quiet `已引用` mark until the message is sent or the chip removed. From 首页, 群 or a 产出, TeamChat opens the 话题 and the toast says `已放进话题的输入框` |

**Composer chips**: a row above the text input (beside the quote chip), wrapping; chip 26 pt,
1 pt `line`, `sunken`, radius 7, `meta` 12/18: the part's 13 pt glyph (table, chart, …) + label cut in the middle at 320 pt + `x`. Hover shows the full
label and the value seen. At most 5 chips (`最多引用 5 处`); the same part again only flashes its
chip. A chip whose source changed says `已更新`. Backspace at the start of an empty input removes the
last chip. The asked member is filled in as an `@` mention when the text has none.

**Sent message**: one row above the body, `meta` `ink-3`: one chip, or `引用了 3 处` that expands to
each chip. A click goes back to the part and flashes it. A changed source keeps the value seen
(`引用的是旧版` + `看现在`); a deleted one says `原消息已删除`. In 单独问 the chips go with the private
question and never reach the 话题.

### 7.16 Diagrams ([Refs](design/mockups-v3/Refs.dc.html) 消息里的图, 卡片里的图)

A `mermaid` fence in a message or a 产出 and the Card v2.1 `diagram` block are drawn by TeamChat on
this computer, with no network, in the app's look (never the library's default theme): node fill
`raised`, borders `line-2`, text `ink` in the UI font 12–13 with mono numbers, lines `ink-3`,
clusters `sunken`. Only gantt task states use blue, green and red, with their status meanings; a
card may mark node states (done, current, waiting on you) with the same tokens. Types: 流程图,
时序图, 状态图, 类图, 关系图, 甘特图, 时间线; anything else shows the source.

- **Inline**: content width, never above 1×. A block title (`时序图 · 重连的顺序`) above, the caption
  `meta` `ink-2` under. A hover toolbar of ghost xsmall buttons: `看原文` / `源码`, `复制原文`, `放大`.
  Height at most 360 pt (`MAX_INLINE_HEIGHT`, REFS §B4): a taller diagram shrinks while its scale
  stays ≥ 0.6, else it shows its top 360 pt at 1× with a 72 pt fade into `raised` and a secondary sm `展开`.
- **Viewer** (gpui-component dialog at 90 % of the window, or the 产出 pane): fit on open, zoom
  25–400 % (`放大`, `缩小`, `适合窗口`, ⌘= ⌘- ⌘0, ⌘ + wheel), drag to pan, source beside it with
  `复制原文`, `复制图片`; zoom changes the layout size, never a transform. Esc returns to the diagram.
- **States**: drawing `正在画图` in a 160 pt `sunken` box with a moving line (static with reduce
  motion). An error state is a frame (radius 12, `raised`) with a 36 pt header (icon 14, the
  diagram type word in `meta` `ink-3`, the reason 600, then what it is about in `ink-2`, a copy
  button for the source) and the source with line numbers below, collapsed to 8 lines + `展开`:

  | Reason | When | After the reason | Look |
  | --- | --- | --- | --- |
  | `图没有画出来` | the source cannot be read, the renderer failed or timed out | `第 3 行看不懂`: the line of the source as sent (the library's message stays in the log) | The only red one: `bad` 35 % border, `bad` 7 % header, `circle-x`, `bad-ink` text; no type word |
  | `这种图还画不了` | a type outside the 7 | — | `line` border, `raised` header, `ink-3` icon |
  | `图里有不支持的写法` | the source has a refused construct (`click`, `javascript:`, `<script>`, `<iframe>`, `<foreignObject>`, `%%{` init lines, front matter) | — | as above |
  | `图太大了，只显示原文` | more than 150 nodes, more than 300 edges, a side over 4096 pt, or a source over 16 KiB | `160 个节点` when the node count is what is over | Its own quiet state: `line` border, `sunken` header, `clock` icon, `ink-2` text |

  The renderer draws in a hidden window and sends each whole SVG back in one message of up to
  4 MiB, past the 16 KiB limit of a 产出 page's events, so a large but allowed diagram is never
  dropped.
- Nodes are parts (§7.15): hover ring, `问这里`, ⌥-click; the chip goes to the diagram's 话题 and
  the viewer stays open.

### 7.17 产出 media ([Media](design/mockups-v3/Media.dc.html))

Every 产出 has two forms: **inline** in the 话题 (height-limited, usable without opening anything)
and **full** in the 产出 pane or window (versions, 来龙去脉). Everything renders on this computer;
previews (posters, frames, waveforms, page thumbnails) are made by the sender's computer.

| Type | Inline | Full |
| --- | --- | --- |
| HTML 网页 | Live page in a frame, 300 pt high. The wheel scrolls the 话题, not the page. The page starts at its top and is cut at the frame's height; while more of it lies below, its bottom edge fades into the frame's surface over 48 pt (`EMBED_FADE`), so the cut never runs through a row; the fade follows the page's scroll and size and the theme. Header: version chip, `沙盒 · 不联网`, `在窗口打开`; the foot has `展开` (opens the full view) and the §7.14 line `页面里的操作会发给 Ledger · …`. At most 2 live pages; others show their snapshot | Page + 版本 list + 页面发出的事件 |
| Markdown | Native excerpt: the first 8 lines with headings and lists, then foot chips `表格 1 · 图 1 · 后续 2/4` and `展开全文 · 约 4 分钟`. A new version leaves the old excerpt and adds a line + `看改了什么` | Native reader with a TOC that follows the scroll; code blocks, tables, diagrams, checklists (a toggle writes a new version) |
| 图片 | Grid of up to 4, `+n` on the 4th; one image keeps its ratio, max 360 pt high; meta `9 张 · 1242 × 1656 · 14.2 MB` | Lightbox on `screen`: the image centered, ← →, filmstrip, `复制`, `存到本机`, `发到话题` |
| 视频 | Poster (never autoplays), play disc, mono duration chip, chapter ticks on the bottom edge, the `视频截图` strip of 6 frames (click seeks; hover `把这一帧发到话题`) | Player (play, time, scrubber with chapter marks, speed, mute, full screen), the chapter list, the frame strip, `复制这一帧` / `把这一帧发到话题` (sent: `这一帧（0:24）已发到话题 · 刚刚` + `撤回`) |
| 音频 | Waveform bars, play, position mono, `1.25×`; 要点 rows with mono times that seek; `文字稿 · 186 句` | Large waveform with speaker segments, ±15 s, a speed segmented control, the transcript with search; the current line follows playback |
| PDF | The first 3 page thumbnails + name, mono pages and size; the whole card opens the reader | System PDF view with a page sidebar, `n / N`, zoom, search; links ask before opening |
| 代码 | Only the lines the member points at, real mono line numbers, header with path and range | The whole file, highlighted range, wrap toggle, the PR link. Monochrome: keywords 600 `ink`, strings and numbers `ink-2`, comments `ink-3` |
| 表格 | Header + the first 4 rows, numbers right-aligned mono, `显示全部 124 行` | Virtualized table, sticky header, sort by header, totals for the selection |

**Upload and preview states** (never by color only): `正在上传 62%` (number + bar), `正在生成预览` (words +
moving line, static with reduce motion), `预览失败` (icon + words, `用系统应用打开`), `发不了` + the
reason in `bad-ink`. A preview arriving later never changes the row's height.

## 8. Screens

### 8.1 Shell

Left rail (§7.3) · page with its title bar (§7.2) · right pane (§4.5). Navigation:
`Place::{Desk, Topic, Group, Members(member), Artifacts, Artifact{id, version}}` and
`RightPane::{None, TopicRail, SideChat(member), Live(run), Artifact(..)}`. Saved navigation stays
additive (old saved places still restore).

### 8.2 Rail modes ([RailOptions](design/mockups-v3/RailOptions.dc.html))

The user picks one in **设置 → 外观 → 侧栏样式** and in the rail's header menu. Stored locally;
default A. All three keep the header, search, destinations, team strip and footer of §7.3.

| Mode | List | Tradeoff (the caption on the setting) |
| --- | --- | --- |
| **A 以工作为中心** (default) | `话题` header with `我参与的 ▾` filter; topics grouped by 群 (开发, 运维, 夹夹增长, 全员), then 私聊; each row with its state slot (§7.3) | 话题按群排，每行带一个状态，找某件事最快。缺点是等你的事散在各个群里，要靠琥珀色的点去扫 |
| **B 以「需要我」为中心** | A row of 群 filter chips (`全部` selected `sel`, then each 群; 28 pt, `ui`); sections `需要你 4` (amber dot in the header), `在做 3`, `等别人 1`, `做完了 · 今天 3` (collapsed by default); rows show the 群 name at the right in `meta` `ink-3` | 一打开就先看到等你拍板的事。缺点是同一个群的话题被拆进三段，群只能当筛选用 |
| **C 以成员为中心** | Destinations become 首页, 话题, 产出; `成员` header with `先看在忙的 ▾`; each member row (24 tile with ring + name + 岗位 `meta` + right `● 等你 2` / `2/4`) expands to its topics (indented 30 pt rows with their state and `等你` in `you-ink`); one collapsed row for idle members (`空闲 · Ledger、Scout、Pocket` with 16 tiles); then `同事` (people, same rows) and one row for absent people (`不在 · Alex Chen、陈晨`) | 谁在忙、谁在等你，一眼就清楚。缺点是一个话题有几位成员时会重复出现，找事情要先想是谁在做 |

Switching mode never animates and keeps the selection.

### 8.3 首页 ([Main](design/mockups-v3/Main.dc.html))

Page title bar: `首页` + date. Column max 680, starting 24 pt under the title bar:

1. **Priority line** (only when Jojo has a desk note today): 18 tile + `Jojo：` + one sentence `ui`
   (`先批 Watch 的发布。晚上 8 点是群里的高峰，越早发越稳。`). Hidden otherwise.
2. **需要你拍板 n**: header `ui` 600 with a 6 pt `you` dot and the count mono `ink-3`. Then the
   real interactive cards (Card v2 asks, v1 ask and approval cards, and topics waiting on the
   viewer as a compact card with an `打开` action), 12 pt apart, newest first.
3. **做完了 今天**: header `ui` 600 + `今天` `meta` `ink-3`; rows: 20 tile (success art) + `Ledger 做好了
   10 月数据月报` `ui` 500 + mono time, then indented proof chips or a real-content thumbnail.
4. Empty desk: one quiet line `都处理完了` `ui` `ink-3` under 需要你拍板; no illustration.

Right column **正在做 n** (320, `rail`): cards per active run, 10 pt apart: a live terminal
thumbnail (only for runs with tool events: `screen`, 120 tall, traffic dots, `● 实时` tag, mono
lines scrolling, fps) above a body: 28 tile with breathing ring + name `ui` 500 + topic + `3/5`
mono, line 2 the current step + `已 3 分钟`, then the plan segments. **排队** card only when queued
runs exist: rows with 16 tile + `Pocket 发晨报` + mono `明早 08:00`.

### 8.4 Topic page ([Topic](design/mockups-v3/Topic.dc.html))

Title bar (§7.2) · timeline (§7.6) with the date divider, messages, event lines, decision records
and Card v2 messages (the done card with proof chips and refs) · pinned decision card (§7.7) ·
composer (§7.4) · topic rail (§7.8) on the right. The timeline anchors to the bottom. The first
message of a topic is the topic start: asker avatar, title as the first line, `小王 提出 · 09:12`.

### 8.5 单独问 ([SideChat](design/mockups-v3/SideChat.dc.html))

The topic page with the 单独问 pane (380) instead of the topic rail; the title-bar 单独问 button is
`sel`. Pane: header 52 `单独问` `ui` 600 + member picker chip (28 pt, 1 pt `line-2`, 18 tile + name +
chevron; default Jojo) + close `x`. A note row on `sunken`, radius 8: `lock` 13 + `只有你能看到，不会发到
话题里` `meta` `ink-2`. The thread starts at the bottom with a divider `Jojo 读过话题里 10:20 之前的内容`.
My message: right-aligned bubble, `sunken`, radius 12, `body`. The member's answer: no bubble; 20 tile
+ name + 岗位 tag + time, then `body` text; numbered points (mono number `ink-3`) each with a ref chip
(24 pt, 1 pt `line`, glyph + mono or `ui` text: `reconnect.ts seq 去重`); a source line `eye` 13 +
`看了 PR #61 的 12 个文件和「重连测试报告」v2` `meta` `ink-3`. A suggestion button (secondary, `corner-down-right`
16 + `让 Forge 补一个多端测试`) with the note `按下才会发到话题`; pressed, it becomes `已发到话题 · 刚刚`.
Nothing else ever leaves the side chat. Composer 96 at the bottom. ⌘; opens and closes.

### 8.6 实时查看 + 接管 ([Live](design/mockups-v3/Live.dc.html))

The topic page with the 电脑 pane; the timeline shows the expanded plan card. Pane: header 52 `Forge
的电脑` `ui` 600 + `● 实时` tag (`live-ink`) + `Mac mini` `ink-3`; right `已用 19 分钟` mono, `maximize`,
close. **Screen**: radius 12, `screen` fill, 1 pt `line`; in 1.1.0 it is a terminal rebuilt from the
run's tool events (command lines in `screen-ink`, output in `screen-dim`, `完成` in `ok`, a blinking
caret), with the window frame of the app it drives when reported. **回放**: a 28 pt scrubber row:
label `回放` `meta` `ink-3`, a 1 pt `line-2` track with a 10 pt hollow knob per plan step and a 14 pt
filled `live` knob at `现在 10:21`; step names under the knobs (`✓ 读现有列表实现`). Dragging shows the
screen at that step. **验收标准** card (rail card style): rows `一万条消息滚动不卡 · 最低 ≥ 55 fps · sparkline ·
现在 57 fps ✓`; a check in progress says `检查中` in `live-ink` with a dot; header right `来自 Joye 的要求 · 2/3
达标`. **Footer** 56, 1 pt `line` top: primary `接管` (`mouse-pointer` icon), secondary `停止`, hint
`接管时 Forge 先停下；交还后，它从这一步接着做。` `meta` `ink-3`, right text action `开发者视图 ↗`.
**Taken over**: a banner replaces the hint row above the screen: `你在操作 Forge 的电脑 · Forge 已暂停`
on `you-bg` with primary `交还`; the member's ring turns static amber; `交还` restores. When the machine
reports a takeover URL, a text action `在屏幕共享里打开` appears in the banner.

### 8.7 群 ([Group](design/mockups-v3/Group.dc.html))

Title bar: `开发` + purpose + member stack. Column max 760: composer (96, placeholder per §7.4) ·
**进行中 n** + `● 2 个等你` (`you-ink`) · a two-column grid of topic cards (radius 12, `lift`, padding
14 16; header 20 tile + name + tag + mono time; title `ui` 600 15; state line (`● 需要你审 PR #61` in
`you-ink`, `● 3/5 在跑…` `live`, `○ 等小王 确认` `ink-3`); a real preview well (`raised`, radius 8:
diff lines, the choice inputs, a live terminal, a search box with highlighted matches); footer line
`小王 提出 · ✓ 测试 148 项通过 · +214 −63`) · **最近完成 这周** rows (`check` `ok` + title `ui` 500 +
one line `ink-3` + 20 tile + mono date) with `全部话题`. Right pane **群资料**: 群里的决定 n (+ `记一条`
`sm` secondary: opens an inline field row; note `成员做事前会先读这里。`; rows decision `ui` + 16 avatar +
name + mono date), 最近的产出 (thumbnail 44 × 48 + name + `v2 · 网页 · Forge` + mono time), 电脑
(`Mac mini ● 在用` + `看 Forge 的电脑 →` full-width secondary).

### 8.8 成员 ([Members](design/mockups-v3/Members.dc.html))

A list column (340, `bg`, 1 pt `line` right): title `成员 13` + `+ 添加` secondary; tabs `全部 13`,
`在做事 3`, `等人 2` (segmented, selected `sel`); sections `● 等你 2`, `在做事 3`, `空闲 3`, `同事 5`; rows
48 pt: 28 avatar with ring, name `ui` 500 + 岗位, right `本周 5 个话题` `meta` `ink-3`, line 2 state
(`等你 · 批准 qqbot v2.14 发布到生产` with `等你` in `you-ink`; `话题列表虚拟滚动 · 在跑压测 · 3/5`). Selected
row `sel`. **Detail** (max 480): header 48 tile with ring + name `card-title` + 岗位 tag + line `向 Tata 汇报
· 在开发、运维两个群`; actions `暂停` secondary + `交代给 Forge` primary. Sections: **现在** (`在做 1 · 排队 1`; a
card with title, `● 在跑…`, labeled plan segments, an 排队 row), **这周** (three stat cells: 完成的话题
`6` + `平均 38 分钟`, 产出 `4` + `3 个 PR · 1 个网页`, 成本 `¥132` + `上限 ¥300` + a bar), **最近交付** (ref rows
with state tags `未合并 · 等你审` `you-ink`, `已合并`), **最近的问题** (`circle-alert` `bad` + text + `卡住了 · 昨天
23:10` + `重试`). Right pane **工作环境**: 电脑 (live thumbnail + `Mac mini 在用` + disk bar), 需要你批准的事
(`lock` rows: 合并到 main, 发布到生产, 改生产环境配置, 单次花费超过 ¥50; note `做这些事之前，Forge 会先发卡片等你批准。`;
`修改` opens the settings form), 能用的工具 (name + scope tag `可以提 PR`, `只读`), 用到的决定. Stats come from
the client runs cache and `BotRun.usage`.

### 8.9 产出 viewer ([Artifact](design/mockups-v3/Artifact.dc.html))

Title bar per §7.2. The page sits in a frame (radius 12, 1 pt `line`, `lift`, max 800, 24 pt
margins): embedded when the child WKWebView works, else a snapshot with `打开` (native window).
Choices inside a member-made page are forwarded to that member (`artifact.event`); the page shows
the record `你选了「B 站」· 10:12 · 已发给 Ledger`. Right pane **来龙去脉**: 来自话题 (check + topic + `夹夹增长 ·
做完了 09:40` + chevron; 做的人 / 要求 rows with tiles and tags), 版本 (rows `v3 加了渠道拆分 今天 09:40`, selected
`sel`; `和 v2 对比`), 页面里的回应, 用到的决定, 被引用. Switching versions never animates; charts draw once
per version.

### 8.10 Settings → 外观 (new) and the rest

Settings keep the v2.2 layout (section list 200 on `rail`, centered column max 704, grouped tables
with the header inside) with v3 tokens. New page **外观** (`palette` 16), first under 通用:
**主题** segmented `跟随系统 / 浅色 / 深色`; **侧栏样式** three 120 × 84 previews of the rail modes (a
miniature of each rail, selected with a 1.5 pt `ink` ring) with the names and the one-sentence
tradeoff of §8.2 under each. Changes apply at once without animation.

### 8.11 Login

v2.2 §8.3 layout with v3 tokens: the `加入` button is ink (`btn`), the code boxes use `line-2` and the
v3 focus state.

### 8.12 Empty and error states

| Case | Treatment |
| --- | --- |
| No 群 | 首页 shows one line `还没有群` `card-title` + one sentence `ui` `ink-3` + `新建群` primary. Rail keeps its header and destinations |
| Desk with nothing for you | `都处理完了` `ui` `ink-3` under 需要你拍板; the 做完了 section stays |
| Rail card with no data | Not drawn |
| 单独问 before the first question | Only the note and the divider; the composer has focus |
| Live pane with no tool events yet | Screen shows `Forge 还没开始用电脑` `ui` `screen-dim` centered; 回放 hidden |
| Load error | Inline row: `circle-alert` `bad` + `没能加载更早的消息。` + `sm` `重试` |
| Search, no result | `没有找到「词」`; the 交代 rows still show |

## 9. Members at work

### 9.1 Status words (closed set, one meaning each)

| Word | Means | Color |
| --- | --- | --- |
| 需要你拍板 | The viewer must decide or review (also `需要你审 PR #61`) | `you-ink` |
| 等你 | Short form in rails and lists | `you-ink` |
| 等{name} | Waiting on someone else | `ink-3` |
| 正在做 / 在做 | Live work | `live` mark, `ink-2` text |
| 排队 | Queued | `ink-3` |
| 做完了 | A result arrived | `ok` check + `ink-3` |
| 卡住了 | Problem | `bad-ink` |
| 完成 | Topic closed | `ink-3` |

`TopicStatus` values map to these words in the copy catalog; the closed protocol enums do not change.

### 9.2 How members reply

- **Acknowledge with a reaction** (`👀`), not a text line. The reaction is removed when the result or
  an ask arrives.
- **No progress messages in the topic.** Progress lives in the plan (rail and plan card), the rail
  row's `3/5`, and 实时查看.
- **Come back with a result**: a done card (intent `done`) with proof (PR link, test counts,
  recording, 产出 refs) when the run ends; or an ask card when a decision is needed; or `卡住了`
  with the reason and `重试`.
- **Lead with the result**: `改好了：重连后不再重复` as the card title; long content goes to a 产出.

## 10. Content

- Product nouns only 成员, 群, 话题, 产出 (PRODUCT.md). UI words added in v3: 单独问, 实时查看, 接管, 交还,
  交代, 来龙去脉, 群里的决定, 验收标准, 回放, 需要你拍板, 做完了, 正在做, 排队.
  Added in 1.1.0 part 2: the choice-form names (排序, 打分 / 评分, 量表, 是或否, 图片选择, 矩阵, 时间段,
  投票), the band words (§7.13), the loop words (§7.14), the 引用提问 and diagram words (§7.15,
  §7.16) and 图 for a diagram. The full lists are in PRODUCT.md. Never `Mermaid`, `block`, `render`
  or `event` in UI text.
- Buttons are verbs. Errors say what failed and what to do.
- Full-width punctuation in Chinese, a space between Latin and Han (`staging 上`), `…` U+2026 from
  one truncation helper.
- No emoji in labels, chips or previews; emoji only as a member's reaction or in message text.

## 11. Accessibility

- WCAG 2.2 AA: every text token passes 4.5:1 on `bg` and `rail` in both themes (§2.2, §2.4). Mark
  colors (`you` 3.6 in light) are used only for shapes ≥ 7 pt with a word beside them.
- No meaning by color alone: every state has a glyph or a word (amber dot + `需要你`, ring + `在做`).
- Focus-visible on every control and row; Tab order: rail, title bar, page, pane, composer.
- Keyboard: ⌘K 交代或搜索 · ⌘↵ in ⌘K 交给 Jojo · ⌘; 单独问 · ⌘N 新话题 · ⌘⇧S rail · ↑↓ in lists · ↵ opens ·
  Esc closes the top overlay or pane · A–F answer a focused choice card.
- **Increase contrast**: `line` uses the `line-2` value, `line-2` uses `ink-3`, `ink-3` uses `ink-2`,
  `hover` uses `sel`, and `lift` becomes a 1 pt `line-2` border. Rings become 2 pt.
- Every icon button has a tooltip and an accessible name; member tiles are hidden from
  accessibility because the name is text beside them.
- Reduce motion: §12.

## 12. Motion

Adapted from the artboards (CONCEPT2 §5) to GPUI: **no transforms on boxes**. A "rise" is a 6 pt
layout offset (top margin or padding animated to 0) together with opacity; a "slide from the right"
is a 12 pt left offset with opacity; a check "draw" and chart lines animate an SVG path or a
canvas stroke (SVG transforms are allowed). Use gpui-base `transition`, `Presence` and `Stagger` and
the app's `ui::motion` helpers; read durations and curves from theme tokens.

Curve **out** `cubic-bezier(0.23, 1, 0.32, 1)` for every enter; never ease-in. UI transitions ≤ 300
ms; only live signals loop.

| What | Animation | Reduce motion |
| --- | --- | --- |
| First paint of a page (cards, rows, rail cards) | Rise once: opacity 0 → 1 + 6 pt offset, 360 ms, stagger 40–50 ms, at most 8 items; later items appear at once | Opacity 150 ms, no stagger |
| Live ring on a working member | Breathing opacity 0.8 ↔ 0.25, 2.4 s ease-in-out loop | Static at 0.8 |
| Current plan step dot | Same breathing | Static |
| Live terminal / screen | New lines revealed one by one (opacity 120 ms); thumbnails scroll content up linearly (9 s loop); caret blinks 1 s steps | Lines appear at once; no scroll; caret steady |
| fps and counters on a live screen | Change value without animation, at most 2 per second | same |
| Answering a card | Controls fade out 120 ms; the record line rises in (200 ms); the check stroke draws 300 ms (240 ms in small sizes), 80 ms after | Record appears; check drawn at once |
| Executing progress | The `live` bar fills by transition 400 ms per step; counters step | Snap |
| Panes (单独问, 电脑, 产出) | Enter: opacity + 12 pt offset from the right, 200 ms; width snaps; exit 150 ms opacity | Opacity only |
| ⌘K, dialogs | Opacity + 6 pt rise, 160 ms; exit 120 ms opacity | Opacity only |
| Menus, popovers | Opacity + 4 pt from the trigger, 150 ms | Opacity only |
| Charts | Lines draw once (stroke dash, 600–700 ms, 50 ms stagger per series); bars grow from 0 height once, 400 ms, staggered 40 ms | Drawn at once |
| Suggestion button pressed (单独问) | Label crossfades to `已发到话题 · 刚刚`, 160 ms | Instant |
| Takeover banner | Rise in 200 ms; ring changes color by crossfade 160 ms | Instant |
| New message from others | 120 ms opacity, no rise; none for a batch of more than 3 | same |
| Busy indicators | Wait 400 ms before showing; once shown, at least 500 ms | same |
| A card rev from the member (§7.14) | Changed and inserted blocks fade in (opacity, 200 ms) under a `live` 14 % tint that fades over 1.6 s, to `bg` for a changed block and to the steady `live` 7 % for an inserted one; removed top-level blocks fade out over 240 ms | No fade or flash: inserted blocks show the 7 % tint at once; the update band says the rest |
| Status band | Word and fill cross-fade 160 ms; the `完成` check draws once | Instant |
| 引用提问 part added | `sel` flash 600 ms on the part | No flash |
| Diagram drawing, preview generating | A moving line in the placeholder (loop while waiting) | Static |
| Hover | Fill changes at once | same |
| Never animated | Selection, keyboard navigation, switching pages or topics, rail mode switch, counts, tabs, theme change, resize, composer growth, version switch | |

Respect `cx.reduce_motion()` in every custom animation; gpui-base helpers honor it. Never animate
inside long lists (GPUI re-lays out the window per frame).

## 13. Dark mode

- Every token has a dark value; code never writes a light or dark literal.
- Surfaces step `bg` #111113 < `raised` #151517 < `rail` #161618 < `sunken` #1C1C1F; cards separate
  by `line` (no `lift` in dark).
- Primary buttons invert: `btn` #F1F1F3 with `btn-ink` #111113.
- Member tints use their mid-tone dark values; the art stays navy (`tile-ink` #232858).
- The `screen` stays dark in light mode (it shows a terminal).
- Check both themes on a real display before release.

## 14. Density

One density ships: comfortable (the values here). Compact is a token-level setting for later.

## 15. Worst-case data

Checked with `--worst-case` at 1100 × 680.

| Slot | Rule |
| --- | --- |
| Rail rows, card titles, breadcrumbs, names | One line, `…`, full text in a tooltip; the trailing mono meta never truncates |
| Title-bar title | Truncates first; the state tag keeps its text up to 200 pt, then `…` |
| Card titles | Two lines, then `…` |
| Card fields | Values wrap to 3 lines, then `…` + `展开` |
| Choice options | 2 short options (single select, no `其他`, label + detail ≤ 16 characters) sit side by side; anything else, and a decision in a side-by-side row column, is stacked (§7.13) |
| 6 options, 8 form fields | The maximum in a card; more belongs in a 产出 page |
| Metric values | `99,999+` cap; units stay; a missing value shows `—` + why (`埋点 11-05 才上线`) |
| Tables | Max 8 rows in a card, then `还有 n 行` + the 产出 link; wide tables scroll horizontally inside the well |
| Diff | One hunk, max 12 lines, then `还有 n 行`; long lines scroll horizontally |
| Evidence | 3 thumbnails, then `+n` |
| Counts | `99+` everywhere (首页 count, tabs, `+8` strip) |
| Team strip | Max 6 tiles, then `+n` |
| Plan | Max 8 steps shown, a window from two before the current one |
| Rail mode C | A member with more than 5 topics shows 5 + `还有 n 个话题` |
| Live terminal | Keeps the last 200 lines; long lines wrap |
| 单独问 answers | Max 6 ref chips per answer; long code in a 产出 |
| Missing title | `（无标题）` in `ink-2` |
| Emoji-first names | Initial skips the emoji |
| Composed cards | More than 8 top-level blocks fold behind `展开另外 n 块` (low emphasis first); a row folds to a stack under 560 pt |
| Choice forms | Options, items and rows past the limits (6 options, 8 rank items, 8 × 5 matrix, 12 slots) belong in a 产出 page |
| 引用提问 chips | Labels cut in the middle at 320 pt; 5 chips; a sent message with more than one shows `引用了 n 处` |
| Diagrams | Inline height at most 360 pt (§7.16); more than 4 per message stay as source; past 150 nodes, 300 edges or 16 KiB of source, `图太大了，只显示原文` |
| Media | Long file names cut in the middle; durations and sizes never truncate |

## 16. Examples and keeping in sync

The artboards in [design/mockups-v3/](design/mockups-v3/README.md) are the examples, one per screen,
each in light and dark. The app's screenshot self-test has a scene per artboard, rendered with the
same team data, so a capture can be compared side by side at 2x.

Sync rules:
- One token source per side: the HELMET2 block for the artboards; `theme.json` + `theme::Colors`
  for the app. A value change updates this file, the artboard helmet and `theme.json` in one
  commit.
- Sizes are Rust enums or constants in `ui/` (`TextStyle`, `ControlSize`, `Radius`, `Space`), so an
  off-scale value does not compile. Views never write a color, size or duration literal.
- Each visual pull request adds before and after captures at 2x in both themes.

**Where 1.1.0 differs from the artboards on purpose** (the app is right; the artboard is not
redrawn):
- Inline diagram height is at most 360 pt, shrinking while the scale stays ≥ 0.6, else the top
  360 + `展开` (REFS §B4); the Refs artboard says 480 with scrolling in the block.
- Diagrams draw 7 types (流程图, 时序图, 状态图, 类图, 关系图, 甘特图, 时间线); the Refs artboard text
  lists flowchart, sequence and state only.
- An inline HTML 产出 page fades into the frame at its cut edge (§7.17); the Media artboard shows a
  plain cut.
- A table total `合计` is an ordinary last row the member sends (Card v2 has no `summaryRow`); the
  app only draws it 600 with a `line-2` rule.
- The update band for other viewers says `按 Joye 的选择` and cannot name the bar: the rev's `cause`
  carries the person and the block, not the event value.

## 17. Checklist

Run screen by screen in both themes, at 1100 × 680 and 1440 × 900, with `--worst-case` and
Increase contrast, next to the artboard PNG.

Tokens and type
- [ ] Only HELMET2 tokens (§2); primary buttons ink with `btn-hi`; secondary `line-2` + `sec-hi`.
- [ ] Every card has `line` + `lift` (light) and radius 12; every member tile has `tile-edge`.
- [ ] Only the type styles of §3; nothing a reader reads under 12; every number mono.
- [ ] Amber only for the waited-on viewer, at most once per card; blue only for live work; red only
      for problems; no red count badges.

Shell and navigation
- [ ] Rail 260, title bars 52 everywhere, title rows centered with the traffic lights.
- [ ] Rail modes A, B and C match RailOptions; 设置 → 外观 switches them; selection survives a switch.
- [ ] One right pane at a time; its title-bar button shows `sel`; Esc closes 单独问 and 电脑.
- [ ] The topic-list column is gone; old saved navigation still restores.

Screens
- [ ] 首页: priority line only with a desk note; 需要你拍板 cards are interactive; 做完了 has proof; 正在做
      thumbnails only for runs with tool events; 排队 only with data.
- [ ] Topic: pinned decision card above a 96 pt composer; rail cards in order, hidden without data;
      no merge button in 改动.
- [ ] 单独问: privacy note, context divider, ref chips, the suggestion posts only when pressed.
- [ ] 实时查看: terminal from tool events, 回放 by step, 验收标准, 接管 / 交还 / 停止 work and show state.
- [ ] ⌘K: 交代 rows first for a request; ↵ and ⌘↵ hand off; matches marked.
- [ ] 群, 成员, 产出 pages match their artboards, with their right panes.

Cards
- [ ] Card v2 anatomy: header, 16/24 title, blocks, `raised` footer with one primary.
- [ ] Every state drawn: pending, waiting on someone, sent, answered (with 撤回), executing, done,
      failed, expired, superseded.
- [ ] Irreversible actions confirm (typed name for production); controls only for the asked viewer.
- [ ] Card v2.1: every container and choice form matches CardsCompose; the status band on every
      card except `info`; a row folds under 560 pt; the unknown-block box.
- [ ] Interaction loop: optimistic state, pending band at 400 ms / soft / hard with 重试 and 不等了,
      the update band (`按你点的「…」` / `按 {name} 的选择`), `Ledger 刚加的`, changed and inserted blocks
      without gaps (hairlines, flash on change, tint on insert), the picked bar rows, the conflict toast and read-only controls for other
      viewers (Interact).
- [ ] 引用提问: 问这里 after 300 ms, ⌥-click, context menu, keyboard; 5 chips; sent chips go back to
      the part (Refs). Diagrams in the app's colors in both themes; every error state keeps the source.
- [ ] 产出 media: inline and full forms per Media; upload and preview states with words, not color.

Members and motion
- [ ] Character art at 72 % of the tile, right status per context; rings breathe only while working.
- [ ] Members acknowledge with a reaction; no progress messages in the topic.
- [ ] Motion per §12; reduce motion checked; nothing animates on selection or navigation.

States and quality
- [ ] Every clickable element has hover, pressed, focus-visible, disabled and a pointer (§7.1).
- [ ] Worst cases of §15 hold at 1100 × 680.
- [ ] Copy from the catalog only; new words only from §10; no developer words.
- [ ] Run emil-design-eng, review-animations, break-ui and apple-design
      (docs/design/skills/emilkowalski) before merging.

## 18. Open items

- Real screen frames in 实时查看 are deferred to 1.2; 1.1.0 rebuilds a terminal from tool events.
- The embedded 产出 page depends on the WKWebView child-view spike; the fallback is a snapshot with
  `打开`.
- GPUI has no inset shadows, tracking or box transforms: `btn-hi` and `tile-edge` are drawn as 1 pt
  lines, titles have no negative tracking, rises use layout offsets. Compare at 2x and accept only
  these differences.
- Dark values come from the artboards; check them on a real display before release.
- Scheduled runs (排队 from schedules), typed asks from CLIs and secret inputs in forms are 1.2.
- Video time, audio segment and PDF page references, and image bytes for runner members, are 1.2.
- The settings window and login were not redrawn as v3 artboards; they follow v2.2 layout with v3
  tokens until they are.

## Changes in v3 (from v2.2)

- §2: the blue-accent palette is replaced by HELMET2 ink-on-paper tokens; primary buttons are ink;
  selection is grey `sel`; state colors are amber (you), blue (live), red (problem); new `lift`,
  `btn-hi`, `sec-hi`, `tile-edge`, `pop` shadows; named member tints replace the hashed `bot-1..6`.
- §3: rail and list rows are 13/20 (were 14/20); card titles 16/24; numbers are mono (SF Mono).
- §4, §8.1: the shell is rail 260 · page · right pane; the topic-list column and the 92 pt
  two-row header are removed; every title bar is 52 pt.
- §7.3, §8.2: the left rail has three modes (A work, B attention, C members), chosen in 设置 → 外观.
- §7.7, §7.8: the pinned decision card and the topic rail replace the closed-by-default side panel.
- §7.9: rings (breathing blue, static amber, static red) replace pips; tiles radius 0.3 × size, art
  at 72 %; no presence or typing dots.
- §7.10: Card v2 with 12 block kinds, 4 intents and 9 states.
- §7.11: ⌘K is 交代或搜索.
- §8.3–§8.10: new screens 首页, 单独问, 实时查看 + 接管, 群, 成员 and 产出 pages, 设置 → 外观.
- §9.2: members acknowledge with a reaction and post no progress messages.
- §12: first-paint stagger, breathing live signals, check draw, pane entrance and chart draw.
- §7.13–§7.17 (1.1.0 part 2): Card v2.1 composition, choice forms and the status band; the
  interaction loop and 产出 pages that talk both ways; 引用提问; diagrams; 产出 media. Artboards
  CardsCompose, Interact, Refs and Media.
