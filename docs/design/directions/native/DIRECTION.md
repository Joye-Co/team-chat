# 原生质感 Native calm

## The idea

The app should look like it shipped with the Mac, next to Mail, Notes and Messages. The window
uses the system's materials, type, accent color and spacing. Its own character comes from how it
shows work, not from decoration: people are circles and bots are rounded squares, so the two
kinds of member read as equals in the same list. A topic's status chip is colored only when the
topic waits on you. Everything else stays gray. The goal is to look ordinary for eight hours and
still show at a glance what needs you. The direction is calm because it uses little color, not
because it hides anything.

Mockups (1440 x 900 at 2x, built from `src/`):

| File | Shows |
| --- | --- |
| `main-light.png` / `.html` | Main window, latest messages: @mention, quote, code, bot acknowledgement, new-messages divider, status card with two actions, streaming bot reply, typing line, composer with the @mention popup open, side panel on 产出 |
| `main-dark.png` | The same topic in dark mode, scrolled to its start: topic header, @mention, bot acknowledgement, pasted image, 产出 card, "3 条新消息" jump pill, empty composer, side panel on 过程 (a bot's process view) |
| `settings-members.png` (+ `-dark`) | Settings window, 成员 pane, one-time invite code sheet |
| `login.png` (+ `-dark`) | Sign-in window with the invite code field focused |
| `switcher.png` | Cmd+K quick switcher over the main window |
| `worst-case.png` | break-ui data: long names and titles, 999+, unbroken strings, one-character names, mixed scripts, emoji, RTL, offline, a 7-line draft |
| `icon.png`, `icon-1024.png` | App icon at 1024, and at 64/32/16 on light and dark |

Rebuild: `bun docs/design/directions/native/src/build.mjs` (all) or name screens
(`... build.mjs main-light worst-case`). The build reads Lucide icons from the `gpui-kit-assets`
crate, the same SVGs the app ships, and caches them in `src/icons.json`. `src/tokens.mjs`
holds the token values. Keep it in step with this file.

## Principles

1. **System first.** Use the system font, accent color, materials and control shapes. Make a custom
   choice only when the system has nothing for our concept: the member shape, status chips, cards.
2. **Color means "you".** Accent and orange appear only where the viewer must act: unread dot,
   mention badge, 待办 count, "等你…" chips, primary buttons. 等{name} chips, counts, metadata
   and icons stay gray.
3. **Lines are hairlines, and only where needed.** Separate regions with spacing first, then with
   a 0.5 pt hairline. Use a shadow only for things that float (popovers, dialogs, the hover
   toolbar). Cards in the timeline get a hairline only. A card never sits in a card.
4. **Members are equal and different.** People are circles and bots are rounded squares with a
   small graphite mark. Both use the same size, palette, row and name style.
5. **Quiet motion.** Anything the keyboard triggers, or that a user sees tens of times a day, does
   not animate. Only surfaces that appear on their own (dialogs, toasts, new messages) move, for
   less than 220 ms.
6. **Readable all day.** Message text is 14/22. Chinese uses PingFang SC at weights 400, 500 and
   600 only. Every count and time uses tabular figures. Secondary text keeps at least 4.5:1
   contrast on its surface.
7. **Worst-case ready.** Every text slot has one rule: truncate, wrap or clamp (see States).

## Color tokens

Values are sRGB. `rgba` values are drawn on top of the surface named in the role.

### Surfaces

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `sidebar-tint` | `rgba(234,234,239,0.74)` | `rgba(36,36,40,0.76)` | Tint over the window-level blur in the sidebar (the window is `Blurred`, so the desktop shows through only here) |
| `content` | `#FFFFFF` | `#1E1E20` | Topic list, timeline, composer, login window |
| `panel` | `#F7F7F8` | `#232326` | Side panel (产出, 过程) |
| `window-bg` | `#ECECEE` | `#1E1E20` | Settings pane background behind grouped sections |
| `group` | `#FFFFFF` | `#2A2A2D` | Grouped (inset) form sections in settings |
| `elevated` | `#FFFFFF` | `#2C2C2F` | Popovers, menus, @mention popup, switcher, dialogs, hover toolbar |
| `toolbar-capsule` | `rgba(255,255,255,0.86)` | `rgba(58,58,62,0.86)` | Grouped toolbar buttons (macOS 26 capsule) |
| `scrim` | `rgba(0,0,0,0.18)` | `rgba(0,0,0,0.38)` | Behind a sheet. The switcher has no scrim |

### Text

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `text-1` | `rgba(0,0,0,0.85)` | `rgba(255,255,255,0.86)` | Body, names, titles |
| `text-2` | `rgba(0,0,0,0.55)` | `rgba(235,235,245,0.58)` | Previews, 岗位, metadata, inactive icons (4.6:1 on white, 5.2:1 on `#1E1E20`) |
| `text-3` | `rgba(0,0,0,0.32)` | `rgba(235,235,245,0.32)` | Timestamps in messages, section headers, hints. Never for content |
| `placeholder` | `rgba(0,0,0,0.30)` | `rgba(235,235,245,0.30)` | Field placeholders |

### Lines and fills

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `sep` | `rgba(0,0,0,0.09)` | `rgba(255,255,255,0.085)` | 0.5 pt hairlines: column edges, list separators (inset to the text) |
| `sep-strong` | `rgba(0,0,0,0.14)` | `rgba(255,255,255,0.13)` | Process-view connector line |
| `fill-hover` | `rgba(0,0,0,0.035)` | `rgba(255,255,255,0.045)` | Message row hover |
| `fill-selected` | `rgba(0,0,0,0.065)` | `rgba(255,255,255,0.085)` | Selected topic row when the list does not have focus |
| `sidebar-selected` | `rgba(0,0,0,0.075)` | `rgba(255,255,255,0.10)` | Selected sidebar row when the sidebar does not have focus |
| `fill-pressed` | `rgba(0,0,0,0.10)` | `rgba(255,255,255,0.13)` | Pressed rows and ghost buttons, disabled send button |
| `fill-field` | `rgba(0,0,0,0.045)` | `rgba(255,255,255,0.07)` | Search fields, segmented track, kbd, file tiles |
| `control` | `#FFFFFF` | `rgba(255,255,255,0.10)` | Secondary button and input background |
| `control-border` | `rgba(0,0,0,0.13)` | `rgba(255,255,255,0.10)` | Composer border at rest |
| `card-border` | `rgba(0,0,0,0.10)` | `rgba(255,255,255,0.10)` | Timeline cards, decision box, grouped sections |
| `image-border` | `rgba(0,0,0,0.07)` | `rgba(255,255,255,0.08)` | Inner hairline on images, so white images keep an edge |
| `quote-bar` | `rgba(0,0,0,0.16)` | `rgba(255,255,255,0.20)` | 2 pt bar of a quote |

### Accent, focus, selection

The app reads `NSColor.controlAccentColor` (and listens for its change) and derives these values
from it. The values below are for the default blue.

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `accent` | `#007AFF` | `#0A84FF` | Primary buttons, send, unread dot, active menu row, focused-list selection, 待办 icon |
| `accent-hover` | `#0A6FE6` | `#2B93FF` | Primary hover |
| `accent-pressed` | `#0062CC` | `#0070E0` | Primary pressed |
| `accent-text` | `#0066D6` | `#4DA3FF` | Links and @mentions as text (4.9:1 on white) |
| `accent-soft` | `rgba(0,122,255,0.11)` | `rgba(10,132,255,0.22)` | HTML 产出 tile |
| `focus-ring` | `rgba(0,122,255,0.42)` | `rgba(10,132,255,0.55)` | 3 pt keyboard focus ring outside a control, and the 1 pt composer border when focused |
| `selection` | `rgba(0,122,255,0.20)` | `rgba(10,132,255,0.36)` | Text selection |
| `mention-bg` | `rgba(0,122,255,0.10)` | `rgba(10,132,255,0.20)` | Background of an @mention of someone else |
| `mention-me-bg` / text | `rgba(255,149,0,0.18)` / `#A35400` | `rgba(255,159,10,0.24)` / `#FFBD5C` | An @mention of me |

### Topic status chips

The chip is 18 pt high with a 5 pt radius, 11 pt text at weight 500, and 6 pt side padding.

| Status | Light bg / text | Dark bg / text | Note |
| --- | --- | --- | --- |
| 进行中 | `rgba(0,122,255,0.10)` / `#0062CC` | `rgba(10,132,255,0.20)` / `#6CB4FF` | |
| 等你回答, 等你审 | `rgba(255,149,0,0.17)` / `#9C5100` | `rgba(255,159,10,0.22)` / `#FFB547` | Weight 600. The only chip that asks for attention |
| 等{name}回答, 等{name}审, 等回答, 等审 | `rgba(0,0,0,0.05)` / `rgba(0,0,0,0.58)` + 6 pt dot `#FF9500` | `rgba(255,255,255,0.07)` / `rgba(235,235,245,0.62)` + dot `#FF9F0A` | The name truncates (max 104 pt); 等 and 回答/审 always stay visible |
| 卡住了 | `rgba(255,59,48,0.11)` / `#C4001A` | `rgba(255,69,58,0.20)` / `#FF7A70` | |
| 完成 | `rgba(52,199,89,0.14)` / `#1C7A35` | `rgba(48,209,88,0.17)` / `#5FD37A` | |
| open | no chip | no chip | |

### Badges, presence, bot mark

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `badge-bg` / `badge-text` | `rgba(0,0,0,0.065)` / `rgba(0,0,0,0.55)` | `rgba(255,255,255,0.10)` / `rgba(235,235,245,0.62)` | Unread count capsule: 18 pt high, 11 pt / 500, tabular. Over 999 it shows `999+` |
| `badge-mention-bg` | `#007AFF` | `#0A84FF` | Mention badge `@3` (white, 600); over 99 it shows `@99+`. The 待办 count uses the same fill without the @ |
| unread dot | `accent` | `accent` | 8 pt dot in the topic row's leading gutter (as in Mail) |
| `online` | `#34C759` | `#30D158` | Presence dot, 30 % of the avatar size (min 8 pt), with a 1.5 pt ring of the surface color. Offline shows no dot |
| `bot-mark` | `#7C7C82` | `#8E8E94` | Circle at the bottom-right of a bot avatar, 42 % of the avatar size (min 11 pt), white Lucide `bot` glyph, 1.5 pt ring of the surface color |

### Avatars (monograms)

Muted tints, chosen by a stable hash of the member id. No photos are needed. A photo replaces
the tint and keeps the shape and the mark.

| Palette | Light bg / fg | Dark bg / fg |
| --- | --- | --- |
| slate | `#E3E8EF` / `#46566C` | `#3A4352` / `#C3CFDF` |
| sage | `#E0ECE2` / `#3B6646` | `#33463A` / `#B6D6BE` |
| sand | `#F0E7D8` / `#76572A` | `#4A4031` / `#E2CFAE` |
| rose | `#F3E2E4` / `#874450` | `#4D3539` / `#EBC0C7` |
| sky | `#DCE9F6` / `#2E5C87` | `#2F4257` / `#B4D2EE` |
| clay | `#EFE1D9` / `#864D36` | `#4C372E` / `#EBC4B2` |
| teal | `#DAEDED` / `#2C6868` | `#2C4646` / `#AEDADA` |
| graphite | `#E6E6E9` / `#55555C` | `#3C3C41` / `#CFCFD6` |

The monogram is chosen as follows. For a Chinese name, at 28 pt and up, a person shows the last
two characters (林晓, 思远) and a bot the first two (设计, 构建). Below 28 pt both show the first
character. A Latin name shows the initials of the first and last word (AW). A mixed or emoji name
shows the first letter only (`🦊 Fox 狐狸` gives F). A one-character name shows that character.
Group tiles in the sidebar are 20 pt rounded squares (radius 5.5) with the group's first
character at 11 pt / 600, in the same palettes.

### Content blocks

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `code-bg` | `#F5F5F7` | `#28282B` | Code block, no border |
| `code-text` | `rgba(0,0,0,0.85)` | `rgba(255,255,255,0.86)` | |
| keyword / string / type / function / comment / number | `#9B2393` / `#C41A16` / `#0B4F79` / `#326D74` / `#5D6C79` / `#1C00CF` | `#FF7AB2` / `#FF8170` / `#6BDFFF` / `#67B7A4` / `#7F8C98` / `#D9C97C` | Xcode's default palettes, so code looks as it does in Xcode |
| card | `content` + 0.5 pt `card-border`, radius 10, no shadow | same | 产出 card, status card |
| `danger` / `danger-text` | `#FF3B30` / `#D70015` | `#FF453A` / `#FF6961` | Destructive buttons, send failure |
| `new-divider` | `#FF3B30` at 70 % (line), 100 % (label) | `#FF453A` | "新消息" divider |
| `success`, `warning` | `#34C759`, `#FF9500` | `#30D158`, `#FF9F0A` | Check icons, progress |

## Typography

- **Family**: GPUI's `.SystemUIFont` (SF Pro). Core Text falls back to **PingFang SC** for Han
  characters, so one family covers both scripts. The theme's `font_family` stays
  `.SystemUIFont`. Do not name PingFang directly: Latin and digits would then be drawn in
  PingFang's Latin, which is wider and looks foreign next to native controls.
- **Mono**: Menlo at 12.5 pt for code and the invite code. SF Mono is not a public family on
  macOS 26 (the system names it `.AppleSystemUIFontMonospaced`). Try that name in GPUI and keep
  Menlo as the fallback (not verified in GPUI).
- **Weights**: 400, 500 and 600 only. PingFang SC has no 700 that matches 600 SF, and bold Han
  characters at 13 pt clog.
- **Tracking**: Core Text applies SF's size-specific tracking automatically. GPUI has no
  letter-spacing, so add none. Han characters get no tracking. Exception: the invite code is
  drawn as three separate runs with a 2 pt gap, which gives the effect of wide tracking.
- **Numbers**: `tnum` (`FontFeatures`) on every count, badge, time, size and version, so values
  do not move when they change.

| Role | Size / line height | Weight | Where |
| --- | --- | --- | --- |
| Display | 22 / 28 | 600 | Login welcome |
| Title | 15 / 20 | 600 | Toolbar titles (群 name, topic title, settings pane), sheet title, topic-start header |
| Body (message) | 14 / 22 | 400 | Message text, composer text. Leading 1.57 for Han |
| Body (UI) | 13 / 18 | 400 (500 for row titles, 600 when unread) | Sidebar rows, topic titles, menus, buttons, fields, card fields |
| Callout | 12 / 16 | 400 | Previews, 岗位, card metadata, typing line, hints |
| Caption | 11 / 14 | 500 (600 for section headers) | Chips, badges, section headers (群, 产出, 步骤), timestamps in messages, kbd |
| Code | 12.5 / 19 | 400 Menlo | Code blocks |
| Invite code | 24 / 30 | 500 Menlo, tabular | The one-time code |

Message text is at most 620 pt wide (about 44 Han characters). Long URLs and unbroken strings wrap
anywhere (`overflow-wrap: anywhere` in the mockups; GPUI wraps by character when a word does not
fit).

## Spacing and layout

Spacing scale (pt): 2, 4, 6, 8, 10, 12, 16, 20, 24, 32. Nothing else.

| Metric | Value |
| --- | --- |
| Window | Min 1100 x 680. Default 1440 x 900. Window corner radius is the system's |
| Toolbar band | 52 pt, unified with the title bar (transparent title bar). Traffic lights at x 20, centered in the band |
| Sidebar | 236 pt (range 200 to 320, resizable). Rows 28 pt, radius 7, inset 10 pt from the edges, 8 pt inner padding, 8 pt icon-to-label gap, 20 pt leading icon or avatar |
| Sidebar section header | 11 / 14, 600, `text-3`, 16 pt above, 6 pt below |
| Topic list | 300 pt (range 260 to 380). Row is two lines, about 64 pt tall: 9/10 pt vertical padding, 20 pt leading gutter for the unread dot, 4 pt between lines. Rows are inset 8 pt with radius 8. Hairline separators are inset to the text and hidden around the selected row |
| Timeline | Flexible, at least 480 pt. Messages: 20 pt leading, 24 pt trailing, 32 pt avatar, 12 pt gutter |
| Message grouping | Same author within 5 minutes and no other message between: no avatar or name, 2 pt top. New group: 12 pt top. Bot acknowledgement: one 20 pt row, 6 pt top |
| Side panel | 280 pt (range 240 to 360), hidden below a 1100 pt window width. Section headers like the sidebar's. Items 46 pt (32 pt tile) |
| Composer | 20 pt side margin, 16 pt bottom. Radius 12. Text 14/22, grows from 1 to 7 lines and then scrolls. Tool row 28 pt with 2 pt gaps. Send button 28 pt circle |
| Typing line | 22 pt, directly above the composer, reserved even when empty so the timeline does not move |
| Popover | Radius 10, 5 pt inner padding, rows 32 pt (mention) or 36 pt (menu), radius 6 |
| Switcher | 640 pt wide, 132 pt from the top of the window. Field 52 pt at 18 pt text. Rows 40 pt. Footer 36 pt |
| Settings window | 900 x 640. Pane list 220 pt with 30 pt rows. Grouped sections radius 10, rows 52 pt, separators inset to the text |
| Sheet (dialog) | 420 pt, radius 14, 20 pt padding, 52 pt from the window top |

## Radii

| Token | Value | Use |
| --- | --- | --- |
| xs | 4 | kbd, mention background |
| chip | 5 | Status chips, group tiles (5.5), settings icon tiles |
| sm | 6 | Small buttons, menu rows, segmented items |
| md (`theme.radius`) | 7 | Buttons, fields, sidebar rows |
| row | 8 | Topic rows, code blocks, images, inputs (34 pt) |
| lg (`theme.radius_lg`) | 10 | Cards, popovers, grouped sections |
| composer | 12 | Composer, side-panel decision box (8) |
| xl | 14 | Sheets, the switcher |
| full | 50 % | Avatars of people, badges, send button, toolbar capsules |
| bot | 28 % of size | Bot avatars (9 pt at 32 pt) |

In gpui-component, set `radius = 7` and `radius_lg = 10`. The other values are set where the app
draws its own elements.

## Elevation and borders

| Level | Light | Dark | Use |
| --- | --- | --- | --- |
| 0 | none | none | Columns. Separated by a 0.5 pt `sep` line on the leading edge |
| 1 | 0.5 pt `card-border`, no shadow | same | Timeline cards, grouped sections, decision box |
| control | `0 0 0 0.5 rgba(0,0,0,.12), 0 1 1.5 rgba(0,0,0,.06)` | `0 0 0 0.5 rgba(255,255,255,.08), 0 1 1.5 rgba(0,0,0,.25)` | Secondary buttons, inputs, the selected segment |
| capsule | `0 0 0 0.5 rgba(0,0,0,.08), 0 1 3 rgba(0,0,0,.06)` | `0 0 0 0.5 rgba(255,255,255,.08), 0 1 3 rgba(0,0,0,.30)` | Toolbar capsules |
| 2 | `0 0 0 0.5 rgba(0,0,0,.12), 0 4 12 rgba(0,0,0,.08), 0 14 36 rgba(0,0,0,.12)` | `0 0 0 0.5 rgba(255,255,255,.10), 0 4 12 rgba(0,0,0,.30), 0 14 36 rgba(0,0,0,.45)` | Popovers, menus, @mention popup, hover toolbar, jump pill, tooltips |
| 3 | `0 0 0 0.5 rgba(0,0,0,.14), 0 6 16 rgba(0,0,0,.10), 0 24 64 rgba(0,0,0,.20)` | `0 0 0 0.5 rgba(255,255,255,.12), 0 6 16 rgba(0,0,0,.35), 0 24 64 rgba(0,0,0,.55)` | Sheets, the switcher |

The first shadow of each level is a 0.5 pt spread ring and acts as the border. In GPUI it is a
`BoxShadow` with `spread_radius: 0.5`, so no separate border is drawn. The toolbar has no bottom
border. Instead a 14 pt fade from `content` to transparent appears under it while content scrolls
beneath (a scroll-edge effect).

## Materials

- **Sidebar vibrancy: yes.** Open the main and settings windows with
  `WindowBackgroundAppearance::Blurred`. On macOS, GPUI adds an `NSVisualEffectView`
  (`UnderWindowBackground`, behind-window blending) under the whole window. The sidebar paints
  `sidebar-tint` (74 to 76 % opaque) over it. Every other region paints an opaque color, so the
  desktop shows only through the sidebar. This is the Mail and Notes model.
- **Why the sidebar is not inset.** macOS 26 shows sidebars as floating inset panes. GPUI can
  blur only the whole window, not a layer inside it. An inset pane would need a translucent layer
  on top of another translucent layer, which apple-design forbids because the text loses
  legibility. A full-height sidebar keeps the real material and looks right on macOS 26: Notes
  and Mail windows with a narrow toolbar still look like this.
- **Popovers, menus and the switcher are opaque** (`elevated`). GPUI cannot blur behind an
  element. A flat fill with a level-2 or level-3 shadow is honest and stays legible.
- **Reduce transparency**: when `accessibilityDisplayShouldReduceTransparency` is set, open the
  window `Opaque` and paint the sidebar `#ECECEE` (light) or `#2A2A2D` (dark).
- **Inactive window**: traffic lights turn gray (system), the sidebar keeps its tint, and
  selection fills keep their gray. Accent fills in lists change to `fill-selected`, as in AppKit.

## Iconography

- Lucide through gpui-kit (`IconName`, the `gpui-kit-assets` SVGs). Add icons outside the 101
  default component icons through the app's own `AssetSource` (about 2 KB each), not with
  `AllAssets`.
- Sizes: 16 pt in toolbars, the composer and sidebar leading icons; 14 to 15 pt in fields, the
  hover toolbar and inline status; 13 pt in buttons and chips; 20 pt in 产出 tiles (40 pt tile).
- Stroke: Lucide's 2 pt on its 24 grid, which is 1.33 pt at 16 pt. This is close to the weight
  of SF Symbols "regular" next to 13 pt text. GPUI draws SVG icons as single-color masks, so an
  icon takes the color of its text role (`text-2` at rest, `accent` when it shows state such as
  the open side panel or the 待办 icon).
- Used: `search, inbox, plus, panel-left, panel-right, settings, square-pen, bell, bell-off,
  ellipsis, paperclip, image, at-sign, text-quote, copy, arrow-up, arrow-down, file-code,
  file-text, file, git-branch, circle-check, triangle-alert, loader, circle-dashed, wifi-off,
  monitor, users, user, user-plus, server, chart-column, activity, code, log-out, eye,
  chevrons-up-down, message-square-plus, bot`.
- No emoji in the interface. Emoji typed by members render as text, in the system emoji font.

## Motion

Two curves only: **out** `cubic-bezier(0.23, 1, 0.32, 1)` for things that enter or leave, and
**move** `cubic-bezier(0.77, 0, 0.175, 1)` for things that move on screen. Override
gpui-component's `MotionTokens`: `easing_exit` defaults to `(0.4, 0, 1, 1)`, an ease-in, which
review-animations rejects. Set it to the out curve. Set `duration_fast` to 120 ms,
`duration_normal` to 180 ms and `duration_slow` to 220 ms.

| What | Animation | Why |
| --- | --- | --- |
| Button, row, toolbar press | Fill changes to the pressed color at once on mouse-down. Release returns over 120 ms, ease | Native buttons darken and do not scale. GPUI has no generic transform for divs. The response is immediate, which is what apple-design §1 asks |
| Hover fills (message row, ghost buttons) | Instant on, 100 ms fade off | Seen tens of times a minute |
| Message hover toolbar | Appears instantly. No fade | Seen constantly |
| Tooltip | 500 ms delay the first time, then instant for neighbors. 120 ms opacity fade in, ease-out | emil-design-eng tooltip rule |
| Context menu, select, popover from a click | 150 ms opacity plus 4 pt travel from the trigger side, out curve. Exit 100 ms opacity only | Anchored to the trigger |
| @mention popup | **None.** Appears and filters at once | Typing triggers it |
| Cmd+K switcher | **None** to open, close or select | Keyboard, many times a day (the Raycast rule) |
| Sheet (invite code, confirmations) | `DialogEntrance::FadeSlide`: 200 ms opacity plus 8 pt down from the title bar, out curve. Scrim fades over 200 ms. Exit 150 ms, same path back | Occasional, and it comes from the title bar as a sheet does |
| Side panel toggle | 200 ms width change, move curve, from a click. **None** from the shortcut ⌥⌘0 | Spatial when clicked, instant from the keyboard |
| New message from someone else | 150 ms opacity fade in place. No slide; the timeline stays pinned to the bottom | No jump |
| My own sent message | None | My own action, many times a day |
| Streaming bot text | Text appears as it arrives, no per-token fade. The caret blinks 530 ms on and 530 ms off (system insertion-point rate). The "正在输出" label uses `ShimmerText` with a 1.6 s linear loop | Shows life without motion in the text |
| Typing dots | Opacity pulse of 1.2 s, dots offset by 150 ms | Small, low contrast |
| Status chip change | 150 ms crossfade of color, ease | State change |
| Toast (gpui `Notification`) | 220 ms in from the top right (8 pt travel plus opacity, out curve), 150 ms out the same way | Spatial consistency |
| Jump pill ("3 条新消息") | 150 ms fade plus 4 pt up | Occasional |
| Spinner (`loader`) | 0.8 s per turn, linear | A faster spinner feels faster |
| **Never animated** | List selection, keyboard navigation, topic or group switching, scroll position on open, unread counts and badges, tab and pane switches in settings, sidebar collapse from the keyboard, theme change, window resize | |

Reduce motion (`accessibilityDisplayShouldReduceMotion`, which gpui-base honors): sheets, toasts
and the side panel cross-fade in 150 ms without travel. The typing dots and the shimmer turn
static ("正在输入…", "正在输出"). The caret stays solid.

## States

| State | Treatment |
| --- | --- |
| Hover | Message rows: `fill-hover` plus the floating action toolbar (quote, @, copy, more; level 2, top right, 30 pt). Sidebar and topic rows: no hover fill, as in AppKit lists. Ghost icon buttons: `fill-hover` |
| Pressed | `fill-pressed` (rows, ghost buttons), `accent-pressed` (primary). Immediate |
| Focus (keyboard) | 3 pt `focus-ring` outside the control, radius follows the control. Lists with keyboard focus switch the selected row to a solid `accent` fill with white text (and white secondary text at 78 %) |
| Composer focus | Border changes from `control-border` to 1 pt `focus-ring`. No outer ring, because the composer has focus most of the day |
| Selected | Unfocused list: `fill-selected` / `sidebar-selected`. Focused list: `accent` fill. Menu rows, the switcher and the @mention popup always use the `accent` fill for the active row |
| Disabled | 40 % opacity on the content, no hover, no pointer change. Deactivated members: the row at 50 % |
| Loading | Topic list: three skeleton rows (gpui `Skeleton`, `fill-field`, no shimmer). Timeline: no skeleton (it opens from the cache in under 100 ms). If it takes longer than 300 ms, a centered 16 pt `Spinner`. Sending: the message shows at once at 60 % text opacity with a 12 pt `clock`; the opacity returns when the server confirms |
| Empty | Centered `Empty` component, 13/18 `text-2`, max 280 pt wide, copy from the catalog (for example `这个群还没有话题，写下第一条消息就会开始一个话题`). No illustration |
| Error | Send failure: a `circle-alert` icon in `danger` next to the message and a `重试` text button. Inline field errors go below the field in 12 pt `danger-text`, never in an alert. Load failures use a gpui `Alert` row at the top of the timeline |
| Offline | The sidebar footer replaces the profile with `wifi-off` + "离线，正在重新连接…". A neutral bar above the composer says "网络已断开。新消息会在连上后发出。" The composer stays usable and queued messages show the sending state. No red and no modal |
| Unread | Topic: accent dot, title at weight 600, gray count. 群: name at 600 and a gray count. Mention: accent `@n` badge in place of the count |
| Inactive window | See Materials |

## Truncation rules (break-ui)

| Slot | Rule |
| --- | --- |
| Sidebar name, topic title, toolbar title, file name | One line, ellipsis at the end. The full text is in a tooltip |
| 产出 file names | Middle ellipsis that keeps the extension (`Q3 Board Deck — FINAL (revised) v…legal].pdf`) |
| Topic preview, 岗位, ack line, typing line | One line, ellipsis. 岗位 truncates before the name (it shrinks twice as fast) |
| Chip name | Truncates at 104 pt; 等 and 回答/审 stay |
| Counts | `999+` for unread, `@99+` for mentions, 待办 `99+`. Thousands use separators (`1,284`) elsewhere |
| Message body | Wraps anywhere. Code blocks never wrap: they scroll horizontally, with a 40 pt fade at the right edge |
| Card titles and field values | One line, ellipsis, tooltip. A card has at most 3 actions in one row |
| Composer | 7 lines, then scrolls inside. Tools and send stay visible |
| Images | Max 360 x 320 pt, aspect kept. Taller images are cropped to the top 320 pt with a "查看原图" action |
| Dates | Today `14:32`, yesterday `昨天`, this week `周一`, this year `11月2日`, older `2025年12月31日` |
| Missing title | `（无标题）` in `text-2` |
| RTL names | Drawn with their own direction inside an LTR row. The badge stays at the trailing edge |

## Component map (gpui-component 0.8.1, gpui-kit 0.8.1, GPUI 0.3.10)

Theme slots (`ThemeColor`) to set for each mode:

| Slot | Light | Dark |
| --- | --- | --- |
| `background` | `content` | `content` |
| `foreground` | `text-1` | `text-1` |
| `muted_foreground` | `text-2` | `text-2` |
| `muted` | `fill-field` | `fill-field` |
| `border`, `input` | `sep`, `control-border` | same |
| `sidebar` | `sidebar-tint` | `sidebar-tint` |
| `sidebar_foreground` / `sidebar_accent` / `sidebar_accent_foreground` / `sidebar_border` | `text-1` / `sidebar-selected` / `text-1` / transparent | same |
| `primary`, `primary_hover`, `primary_active`, `primary_foreground` | `accent`, `accent-hover`, `accent-pressed`, white | same |
| `accent` / `accent_foreground` (gpui-component's hover fill) | `fill-hover` / `text-1` | same |
| `secondary`, `secondary_hover`, `secondary_active` | `control`, `fill-hover` over `control`, `fill-pressed` | same |
| `button_*` | as primary and secondary | same |
| `list`, `list_hover`, `list_active`, `list_active_border` | `content`, transparent, `fill-selected`, transparent | same |
| `popover`, `popover_foreground` | `elevated`, `text-1` | same |
| `ring` | `focus-ring` | same |
| `selection`, `caret` | `selection`, `accent` | same |
| `tab_bar_segmented`, `tab`, `tab_active` | `fill-field`, transparent, `control` | same |
| `danger*`, `success*`, `warning*`, `info*` | system red, green, orange, blue as above | same |
| `blue/_light`, `yellow/_light`, `red/_light`, `green/_light` | Status chip text and background | same |
| `overlay` | `scrim` | `scrim` |
| `scrollbar_thumb` | `rgba(0,0,0,0.25)`, overlay mode | `rgba(255,255,255,0.25)` |
| `skeleton` | `fill-field` | `fill-field` |
| `title_bar`, `title_bar_border` | transparent, transparent | same |
| `link` | `accent-text` | same |
| Theme fields | `font_family` `.SystemUIFont`, `font_size` 13, `mono_font_family` Menlo, `mono_font_size` 12.5, `radius` 7, `radius_lg` 10, `shadow` true, `focus_ring` true, `scrollbar_mode` Scrolling | same |

The app's own palette (status chips, avatar palettes, mention colors, bot mark, code syntax) goes
into `apps/desktop/src/theme.rs` as `Colors` fields, as AGENTS.md requires.

| UI element | Component | Notes |
| --- | --- | --- |
| Window, title bar | `gpui_kit::open_window`, `TitlebarOptions { appears_transparent: true, traffic_light_position: (20, 20) }`, `WindowBackgroundAppearance::Blurred` | No `TitleBar` component; the toolbar band is part of each column |
| Sidebar | `Sidebar` + `SidebarMenu` / `SidebarMenuItem` (`.icon`, `.suffix` for badges, `.active`, `.context_menu`) | Custom leading element for avatar or group tile |
| Search field in sidebar | `Input` with `InputGroupAddon` (search icon) plus `Kbd` | Opens the switcher |
| Section headers | `SidebarGroup` label | |
| Topic list | `List` with `ListDelegate` / `ListItem` (virtualized) | Custom row content. `list_active` gives the unfocused gray |
| Filter (全部/未读/待办) and panel tabs (产出/过程) | `TabBar` + `Tab::segmented()` | |
| Status chip | `Tag` with `TagVariant::Custom { color, foreground, border }`, `Size::XSmall` | The dot for 等{name} is a prefix element |
| Badges | `Badge::count(n).max(999)` with a custom color; mention variant builds the `@` text | Or a small `Tag` |
| Avatars | `Avatar::new().name(..)` for people; bots draw their own rounded square (`Avatar` is circular) with the mark as an absolute child | `AvatarGroup` for the toolbar stack |
| Toolbar buttons | `Button::ghost().icon(..)` in a capsule `div` (`rounded_full`, capsule shadow) | |
| Timeline | `MessageScroller` / `MessageScrollerState` (bottom anchored, virtualized) or `virtual_list` | 10,000 messages at 60 fps is a V1 requirement |
| Message group and body | `MessageGroup` / `message` with `BubbleVariant::Ghost` (no bubble) | Avatar, name and time row is app code |
| Markdown | `TextView` (markdown) with `TextViewStyle` set to the body scale | Mentions are inline styled runs |
| Code block | `TextView` code block + `SyntaxHighlighter` with a `HighlightTheme` built from the Xcode palette | Horizontal scroll |
| Image | `img` with `ObjectFit::Cover`, radius 8, plus the inner hairline as a level-1 ring | |
| 产出 card | `div` with level-1 ring + `Button::small()` | Opens the 产出 window |
| Status card | `DescriptionList` (2 columns) + `Button::primary().small()` / `Button::small()` | Actions send to the card's author |
| Quote | `div` with a 2 pt left border | |
| Streaming label | `ShimmerText` + `Spinner` (`loader`) | |
| Bot acknowledgement | `ListItem`-like row with a 20 pt avatar | |
| New-messages divider | `Separator::horizontal().label("新消息").color(new-divider)` | |
| Jump pill | `Button` with level-2 shadow, `rounded_full` | |
| Typing line | text + three dots | |
| Composer | `Input` on a multi-line `InputState` (`auto_grow(1, 7)`) inside an `InputGroup` | IME and paste are handled by the input |
| @mention popup | `Popover` anchored at the caret with a `List` | No animation |
| Hover toolbar | `div` with level-2 shadow and `Button::ghost()` icons + `Tooltip` | |
| Side panel | `ListItem` rows, `Progress` for step progress, `Skeleton` for loading | Resizable split (`resizable` panels) |
| Switcher | `Command` (`CommandState`, `CommandGroup`, `CommandItem`, `.header`, `.footer`) inside a `Dialog` with `DialogEntrance::None` and a transparent overlay | |
| Settings window | `Settings` / `SettingPage` / `SettingGroup` / `SettingItem`, or a `Sidebar` + `GroupBox` | Pane icons are 20 pt colored tiles (System Settings style) |
| Level select (权限) | `Select` / `DropdownMenu` | |
| Invite code sheet | `Dialog` (`DialogHeader`, `DialogTitle`, `DialogDescription`, `DialogFooter`), `DialogEntrance::FadeSlide` | The code uses mono runs and a `clipboard` copy button |
| Login | `Input` (server), `Input` or `OtpInput` (code, mono), `Button::primary()` full width | Inline error under the field |
| Toasts | `Notification` / `NotificationList` | |
| Tooltips | `Tooltip` | |
| Empty states | `Empty` (`EmptyTitle`, `EmptyDescription`) | No `EmptyMedia` illustration |
| Context menus | `ContextMenu` / `PopupMenu` | |
| Switches (通知, 开发者视图) | `Switch` | |
| Keyboard hints | `Kbd` | |

### What GPUI cannot render (and the fallback)

| Wanted | Limit | Fallback |
| --- | --- | --- |
| Blur behind a popover, menu, toolbar or inset sidebar pane | Only window-level blur (`NSVisualEffectView` behind the whole window) | Opaque elevated surfaces. A full-height translucent sidebar |
| Letter-spacing | `TextStyle` has none | Core Text tracking for SF. The invite code is split into runs |
| `transform: scale` on press | No transforms on divs | Pressed fill color, which is the AppKit behavior anyway |
| The accent color as a theme value | GPUI does not read `controlAccentColor` | Read it with `objc2` at startup and on `NSSystemColorsDidChangeNotification`, then rebuild the accent tokens (about 30 lines). Until then, system blue |
| 0.5 pt hairlines | Fractional widths depend on the scale factor | On 2x displays 0.5 pt is one device pixel. On 1x, round up to 1 px at the same alpha |
| SF Mono | Not a public family | Menlo (see Typography) |
| Continuous ("squircle") corners | GPUI draws circular arcs | Circular radii. The difference cannot be seen at these sizes |
| The app icon's glass layers (macOS 26 Icon Composer) | The DMG icon is a static `.icns` | Ship `icon-1024.png` as `.icns`. An Icon Composer `.icon` file can be added later for the dynamic light, dark and tinted variants |

## Checks against the vendored skills

| Skill | Check | Result |
| --- | --- | --- |
| emil-design-eng | No `ease-in`, nothing scales from 0, keyboard actions do not animate, popovers come from their trigger and sheets stay centered, durations under 300 ms, tooltips instant after the first | Met in the motion table. gpui-component's default `easing_exit` must be overridden |
| emil-design-eng | Tabular numbers, no layout jump when counts change | `tnum` everywhere |
| review-animations | Frequency table: the switcher and @mention popup are not animated, rows have no hover animation | Met |
| apple-design | Materials: no translucent layer on another; the sidebar uses the system material; scroll-edge fade instead of a toolbar border; dim only for modal sheets | Met; this is why the sidebar is not inset |
| apple-design | Feedback on mouse-down, reduced motion and reduced transparency handled | Met (see Motion and Materials) |
| apple-design | Familiarity: traffic lights top left, Mail's three columns, System Settings layout, sheets from the title bar | Met |
| break-ui | `worst-case.png`: long names, 999+, unbroken strings, 1-character names, mixed scripts, emoji, RTL, offline, a 7-line draft | No overflow. Rules are in Truncation |

## Risks

- **Vibrancy cost and look.** A `Blurred` window keeps the desktop compositor busy, and GPUI
  repaints the whole window. Measure the 60 fps scroll and the cold start with blur on. If either
  regresses, use `Opaque` and the reduce-transparency colors; the design still holds.
- **Accent color is not built in.** Without the `objc2` bridge, users with a non-blue accent get
  blue, which is "almost native" rather than native.
- **Squircle bot avatars** are drawn by the app, because `Avatar` is circular. A small custom
  element must stay in step with `Avatar` (size, image, fallback).
- **Restraint can read as plain.** Judges looking for brand color will find little. The
  character lies in the member shapes, the "you" color rule and the density, not in a palette.
- **Mixed-script weight.** PingFang and SF at 500 and 600 do not match perfectly. Titles at 600
  look slightly heavier in Han text. This was checked in the mockups; on screen it needs a check
  in the real app.
- **The side panel takes width.** At 1440 pt the timeline is 624 pt. Below 1100 pt the panel is
  hidden by default.
- **The mockups simulate the desktop** behind the sidebar with a soft gradient. The real tint
  depends on the user's wallpaper, so check it on bright and dark wallpapers.
