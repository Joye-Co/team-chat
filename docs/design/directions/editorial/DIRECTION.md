# 温暖编辑 Warm editorial

## The idea

The app reads like a well-set page, not like a dashboard. Paper-warm neutrals, ink-dark text and
one terracotta accent (陶土) carry the whole interface. Long Chinese threads get a book measure
(about 38 characters a line) and a 1.7 line height, so a person can read them for 8 hours. The
chrome steps back: no bubbles, no borders around regions, and color only where it means something
(a status, a mention, the one action that waits on you). People and bots look equal. A bot differs
only by shape (a squircle, not a circle) and a small ink mark.

Mockups: `main-light.png` (scrolled to the newest message), `main-dark.png` (the same topic,
scrolled up to its start, with the jump button), `settings-members.png`, `login.png`,
`switcher.png`, `worst-case.png`, plus `-dark` variants of each, and `icon.png` /
`icon-1024px.png`. Source: `src/tokens.css` (all tokens, once), `src/build.py` (writes the
self-contained HTML files, with Lucide icons read from the `gpui-kit-assets` crate that the app
ships), `src/render.sh` (headless Chrome, 2x).

## Principles

1. **Content leads, chrome is quiet.** Regions are separated by a change of paper tone and one
   hairline, never by a box. Messages have no bubbles. Cards are the only raised surfaces.
2. **One accent, used sparingly.** Terracotta means "this concerns you": an @mention of you, the
   mention badge, the primary action, the new-messages divider, focus and the caret. Nothing
   else is terracotta.
3. **A status chip states who waits.** When the topic waits on *you*, the chip is filled
   (等你审). When it waits on someone else, the chip is quiet: an outline and a small dot
   (等林晓回答). One look at the list tells you which topics are yours.
4. **Reading comfort before density.** Body text is 14/24, with at most 38 characters a line. UI
   text is 13/20. Numbers are tabular everywhere.
5. **People and bots are equal.** Bots use the same type, the same row and the same place in
   lists. Only the avatar shape and the mark differ. A bot's acknowledgement is one quiet line,
   because the product says that a bot acknowledges once and then works quietly.
6. **Never add a frame inside a frame.** A card never holds a card. A code block in a message sits
   on the page, not in a card.
7. **Motion only for feedback and for continuity of place,** never on keyboard-driven actions.

## Color tokens

All values are in `src/tokens.css`. "Ink" means warm near-black. Alpha colors are drawn on the
surface below them.

### Surfaces

| Role | Light | Dark | gpui-component slot |
| --- | --- | --- | --- |
| Window / timeline (paper) | `#FCFBF8` | `#211F1C` | `background` |
| Sidebar (linen) | `#F1EEE8` | `#191715` | `sidebar`, `title_bar` |
| Topic list | `#F7F5F0` | `#1D1B18` | `list` (app panel bg) |
| Side panel | `#F7F5F0` | `#1D1B18` | app token `panel` |
| Elevated: cards, composer, selected topic row (light) | `#FFFFFF` | `#292622` | `group_box`, app token `surface` |
| Popover, menu, dialog, Cmd+K | `#FFFFFF` | `#2D2A26` | `popover` |
| Active row inside a popover | `#F4F0EA` | `#36322D` | `accent` |
| Code block, invite-code field | `#F4F1EB` | `#191715` | `muted` (via TextViewStyle `code_background`) |
| Modal overlay | `rgba(38,30,20,0.22)` | `rgba(10,8,6,0.42)` | `overlay` |
| Desktop behind windows (mockup only) | `#D9D3C9` | `#121110` | — |

### Text

| Role | Light | Dark | Slot |
| --- | --- | --- | --- |
| Primary (titles, body) | `#26221D` | `#ECE6DC` | `foreground`, `popover_foreground`, `sidebar_foreground` |
| Secondary (previews, quotes, ack lines) | `#5C554C` | `#B0A79B` | `secondary_foreground` |
| Tertiary (meta, section labels, times in lists) | `#8C8478` | `#83796E` | `muted_foreground` |
| Quaternary (times in timeline, placeholders, disabled) | `#B7AFA3` | `#5C554D` | app token `text_4` |
| Link / accent text / mention text | `#A54C25` | `#E59A72` | `link`, `link_hover` = accent hover |

### Lines, states, accent

| Role | Light | Dark | Slot |
| --- | --- | --- | --- |
| Hairline separator | `rgba(72,52,28,0.09)` | `rgba(255,238,214,0.07)` | `border`, `sidebar_border`, `title_bar_border`, `table_row_border` |
| Strong line (input, composer, quote bar) | `rgba(72,52,28,0.15)` | `rgba(255,238,214,0.12)` | `input` |
| Hover fill | `rgba(72,52,28,0.045)` | `rgba(255,238,214,0.04)` | `list_hover`, `secondary_hover`, `table_hover` |
| Pressed fill | `rgba(72,52,28,0.08)` | `rgba(255,238,214,0.07)` | `secondary_active` |
| Selected row (sidebar, settings nav) | `rgba(72,52,28,0.085)` | `rgba(255,238,214,0.075)` | `sidebar_accent`, `list_active`, `table_active` |
| Accent (terracotta) | `#B4542C` | `#C9693F` | `primary`, `caret`, `progress_bar`, `switch` (on), `slider_bar` |
| Accent hover | `#A2481F` | `#D57A50` | `primary_hover` |
| Accent pressed | `#8F3E1A` | `#B85C34` | `primary_active` |
| Text on accent | `#FFFFFF` | `#FFF8F2` | `primary_foreground` |
| Accent soft (artifact tile, mention chip) | `rgba(180,84,44,0.10)` | `rgba(219,132,89,0.14)` | app token |
| Focus ring (4px outside a 1px accent edge) | `rgba(180,84,44,0.30)` | `rgba(219,132,89,0.38)` | `ring` |
| Text selection | `rgba(180,84,44,0.18)` | `rgba(219,132,89,0.26)` | `selection` |
| Danger | `#B23A35` | `#E0726B` | `danger`, `button_danger`, `red` |
| Danger soft | `#F6E3E0` | `rgba(224,114,107,0.15)` | `danger` tint, `red_light` |
| `list_active_border` | transparent | transparent | (no focus stripe on rows) |

### Topic status chips

Filled chip = the topic waits on the viewer, or a status that is not about waiting. Quiet chip =
it waits on someone else: transparent background, 1px `rgba(72,52,28,0.16)` /
`rgba(255,238,214,0.14)` outline, `text-2` label, and a 5px dot in the status color.

| Status | Chip | Light fg / bg | Dark fg / bg | Slot |
| --- | --- | --- | --- | --- |
| `open` | (none) | — | — | — |
| `in_progress` | 进行中 | `#3D6A93` / `#E8EEF4` | `#93B5D6` / `rgba(120,160,205,0.15)` | `blue` / `blue_light` |
| `waiting_answer`, `waiting_review` (you) | 等你回答, 等你审 | `#94600F` / `#F6EAD2` | `#E2B462` / `rgba(220,165,70,0.16)` | `yellow` / `yellow_light` |
| same, someone else | 等{name}回答, 等{name}审 | `#5C554C` text, `#94600F` dot | `#B0A79B` text, `#E2B462` dot | quiet `Tag::outline` |
| `blocked` | 卡住了 | `#A8343A` / `#F6E2E0` | `#E58A84` / `rgba(224,114,107,0.15)` | `red` / `red_light` |
| `done` | 完成 | `#4A7650` / `#E6EEE3` | `#98BF92` / `rgba(130,180,125,0.14)` | `green` / `green_light` |

The dark chips use the alpha fills on purpose: a solid tint would look like a sticker on warm
charcoal.

### Badges, presence, bot mark, avatars

| Role | Light | Dark |
| --- | --- | --- |
| Unread count badge | bg `rgba(72,52,28,0.09)`, fg `#5C554C` | bg `rgba(255,238,214,0.09)`, fg `#B0A79B` |
| Mention badge (`@1`, `@99+`) | bg accent `#B4542C`, fg `#FFFFFF` | bg `#C9693F`, fg `#FFF8F2` |
| @mention in text | accent text on `rgba(180,84,44,0.10)`; a mention of you on `0.16`, weight 600 | `rgba(219,132,89,0.14)` / `0.22` |
| Presence: online | `#5E9A58` | `#79B373` |
| Presence: away | `#C9923A` | `#D9A54E` |
| Presence: offline | hollow ring `#B7AFA3` | hollow ring `#5C554D` |
| Bot mark (corner tile with Lucide `bot`) | bg `#3A332B`, fg `#FBFAF7` | bg `#E4DDD2`, fg `#1F1D1A` |
| Code text | `#3A342D`; keyword `#A0472A`, function `#3D6A93`, type `#7A5A2E`, number `#4A7650`, comment `#9A9285` italic | `#DCD4C8`; `#E39A72`, `#93B5D6`, `#D5B47E`, `#98BF92`, `#7A7166` |
| Diff `+` / `−` | `#4A7650` / `#B23A35` | `#98BF92` / `#E58A84` |
| Card border / shadow | `0 0 0 1px rgba(72,52,28,0.10)`, `0 1px 2px rgba(60,40,20,0.04)` | `rgba(255,238,214,0.08)`, `0 1px 2px rgba(0,0,0,0.24)` |

Avatar tones (bg / fg). Light: clay `#EBD9CC/#8A4A2A`, sage `#DCE5D5/#46623E`, slate
`#DAE2EA/#3E5A75`, ochre `#EFE0C0/#7C5A1C`, plum `#E6D8E0/#6D4560`, sand `#E9E2D5/#6A5E4A`, ink
`#3A332B/#F3EEE6`. Dark: clay `#4A3328/#E7B79C`, sage `#34402F/#B5CCA9`, slate `#2F3B47/#ABC1D6`,
ochre `#4A3D22/#E2C78E`, plum `#43303C/#D9B6CA`, sand `#3D372E/#D3C7B2`, ink `#E4DDD2/#2A2621`. The
tone is a stable hash of the member id. Initials: the given name (the last two Chinese characters,
or two Latin initials). At 20 px or smaller, one character only.

## Typography

- **Family:** the system font (`.SystemUIFont` in GPUI). CoreText draws SF Pro for Latin and digits
  and falls back to PingFang SC for Chinese, so mixed text gets matched metrics. Do not name
  PingFang first: then Latin and digits come from PingFang, which looks worse.
- **Mono:** SF Mono when GPUI can resolve it, else Menlo (GPUI's default). Not confirmed: I did not
  test whether GPUI resolves "SF Mono" by name; I looked only in `/System/Library/Fonts`
  (`SFNSMono.ttf` is present).
- **Numbers:** `FontFeatures([("tnum", 1)])` on every count, time, badge, version and code (the
  same way `gpui-component`'s `time_field.rs` does it).
- **Tracking:** Chinese stays at 0. The design asks for −0.01em on titles of 17 px and more and
  −0.02em on the 26 px login title. **GPUI has no letter-spacing**, so the app renders tracking
  at 0. CoreText's automatic SF optical sizing (Display cuts at 20 pt and more) covers most of the
  difference. That is plausible but not confirmed in a GPUI build.

| Role | Size / line height | Weight | Color | Where |
| --- | --- | --- | --- | --- |
| Display | 26 / 34 | 600 | text-1 | login title |
| Settings page title | 20 / 28 | 600 | text-1 | 成员, 机器 … |
| Topic title (header) | 17 / 24 | 600 | text-1 | timeline header |
| Group title | 15 / 20 | 600 | text-1 | topic list header; dialog title 15/22 |
| Body (message, composer) | **14 / 24** | 400 | text-1 | timeline; max width 34em ≈ 476 px ≈ 38 Chinese characters |
| Topic row title | 13.5 / 20 | 500, unread 600 | text-1 | topic list, card titles |
| UI | 13 / 20 | 400; selected or unread 600 | text-2 → text-1 | sidebar rows, buttons (500), menus |
| Author name | 13 / 18 | 600 | text-1 | message head |
| Quote, ack line | 13 / 20 | 400 | text-2 | timeline |
| Preview, meta | 12 / 18 | 400 | text-3 | topic list line 2, field values 13/20 |
| Time, role, small meta | 11.5 / 16 | 400 | text-3 (lists), text-4 (timeline) | |
| Section label, chip, badge | 11 / 14–18 | 600 | text-3; chips use status fg | 待办, 群, 私聊, 产出 |
| Code | 12.5 / 20 mono | 400 | code fg | code blocks; inline code 12 |
| Invite code | 22 / 28 mono | 600 | text-1 | dialog |

Set `Theme::font_size = 14px` and keep the rem at 14 px, so the components' `text_sm` (13 px
after their scale) and the body (14 px) line up. **Check this with a build:** gpui-component's
default is 16 px.

## Spacing and layout

Spacing scale (px): 2, 4, 6, 8, 10, 12, 16, 20, 24, 32. Window minimum 1100 × 680.

| Metric | Value |
| --- | --- |
| Sidebar width | 224 (resizable 200–280) |
| Topic list width | 288 (resizable 260–360) |
| Side panel width | 272, closed by default below 1280 px window width |
| Timeline | flexible; side padding 32; content column max 720; text max 34em |
| Top bar height (traffic lights inside the sidebar, no separate title bar) | 52 sidebar, 56 timeline and panel, 64 topic list header |
| Sidebar row | 30 high, 8 px inner left, radius 7, 20 px group monogram / avatar, gap 8 |
| 待办 row | two lines, 13/19 + 11.5/15, padding 6 8 |
| Section label | 28 high, 8 px above |
| Topic row | ≈ 64 high: padding 9 12 10, gap 4 between lines, inset 8 from column edges, radius 9 |
| Message | 32 avatar, 12 gap; 16 above a new author; 4 above a continuation (same author within 5 minutes; time shows in the gutter on hover); 20 above a day divider |
| Ack line | 20 avatar centered in the 32 column, one line, 10 above |
| Card | max 460 wide, padding 12 16 14 (status), 10 12 (产出), 8–10 above |
| Image | max 352 × 176 inline (keep aspect ratio, cover only in the thumbnail), radius 10, 1px card border |
| Composer | radius 12, padding 11 14 4, toolbar 2 8 8, 28 send button; aligned with message text (44 px inset) |
| Popover / menu row | 36 high (mention, Cmd+K), radius 6–7, 5 px popover padding |
| Settings window | 900 × 640; nav 200; content padding 24 32; member row 52 |
| Dialog | 440 wide, padding 20 20 16, radius 14 |
| Cmd+K | 640 wide, 132 px from the top of the window, input 52, rows 36, footer 36 |

## Radii

| Token | px | Use |
| --- | --- | --- |
| `radius` (md) | 7 | buttons, inputs (8), rows, select |
| sm | 4 | chips, inline code, kbd, mention highlight |
| `radius_lg` | 10 | cards, popovers, images, code blocks (8) |
| xl | 12–14 | composer (12), dialog and Cmd+K (14), settings window (system 12) |
| full | — | person avatars, badges, presence |
| squircle | 28% of size | bot avatars (GPUI draws circular-arc corners, not a continuous curve; at these sizes this is fine) |

## Elevation and borders

- **No borders between regions.** The sidebar, list, timeline and panel meet with a tone change and
  one inset hairline (`border`). The timeline header has no line: a 70% → 0 gradient of the paper
  color fades content under it (a scroll-edge effect).
- **Cards:** 1px hairline ring and a 1–2 px soft shadow. A hairline is used here because cards sit on
  near-white paper, where a shadow alone disappears.
- **The selected topic row** in light mode is a white surface with a 0.5px ring: the list reads as a
  stack of pages. In dark mode it is a flat selected fill (shadows do not show on charcoal).
- **Popover / menu / mention / Cmd+K:** `0 0 0 0.5px rgba(50,36,18,.14), 0 12px 32px -8px
  rgba(50,36,18,.20), 0 2px 6px rgba(50,36,18,.06)`. Dark: `0.5px rgba(255,238,214,.10)` ring,
  `0 16px 40px -8px rgba(0,0,0,.55)`.
- **Second window (settings):** the system window shadow. The background window goes inactive (gray
  traffic lights) and is not dimmed. The dim in the mockup only marks the focus.
- **Inputs:** 1px strong line; when focused, a 1px accent edge and a 4px `ring` halo (macOS focus ring
  size).

## Materials

**No sidebar vibrancy.** GPUI only offers a whole-window `WindowBackgroundAppearance::Blurred`. It
would pull desktop colors into the warm linen and break the paper idea, and it costs contrast for
small gray text. The sidebar is an opaque warm surface, like Things and Bear. If vibrancy is wanted
later, use it only in the sidebar, with an 85% linen tint over `Blurred`, and test the 11 px labels.

## Iconography

Lucide via gpui-kit (`Icon::new(IconName::…)`), stroke 2 on a 24 grid. That is 1.33 px at 16 px,
which matches the 13 px text weight. Sizes: 16 in toolbars, 15 in settings nav and composer, 14 in
sidebar search and buttons, 12–13 in chips, kbd and the bot mark. Color: text-3 at rest, text-1
when on or hovered, accent only for a meaning (the jump arrow). Icons used: `search`,
`square-pen`, `bell`, `bell-off`, `panel-right`, `x`, `settings`, `paperclip`, `image`, `at-sign`,
`arrow-up`, `arrow-down`, `copy`, `check`, `external-link`, `corner-down-left`, `chevron-down`,
`ellipsis`, `git-pull-request`, `file-text`, `file-spreadsheet`, `scale`, `message-square-text`,
`hash`, `bot`, `users`, `user-round`, `user-plus`, `monitor`, `wallet`, `stethoscope`,
`code-xml`, `wifi-off`. Most of these are not in gpui-kit's default 101-icon `Assets`. Register
them through the app's own `AssetSource` (about 2 KB each, per gpui-kit-assets' README). Group
monograms (全, 开, 产) replace generic `#` glyphs in the sidebar.

## Motion

Map to `MotionTokens`: `duration_fast` 120 ms, `duration_normal` 180 ms, `duration_slow` 240 ms;
`easing_enter` = `cubic-bezier(0.23, 1, 0.32, 1)`; `easing_move` = `cubic-bezier(0.77, 0, 0.175,
1)`; `spring_control` 180 ms, damping 1.0; `spring_move` 300 ms, damping 1.0. **Change
gpui-component's default `easing_exit` (`0.4, 0, 1, 1`, an ease-in).** Exits use the enter curve
at a shorter duration (review-animations: never ease-in on UI).

| Transition | Motion |
| --- | --- |
| Cmd+K open / close | **none** (keyboard, 100+ times a day; Raycast rule) |
| @mention popup open, filter, close | **none** (it follows typing) |
| Topic / group selection, list keyboard navigation | **none** |
| Hover fills, pressed fills | instant in; 120 ms `ease` out on hover end only |
| Button press | pressed color at pointer-down, no delay. Scale 0.97 is not available on GPUI divs (see below) |
| Context menu, select dropdown, tooltip | opacity 0 → 1 + 4 px from the trigger side, 150 ms enter curve; exit opacity only, 100 ms. Tooltip delay 500 ms; once one is open, the next opens instantly |
| Dialog (invite code, delete confirm) | overlay fade 180 ms; dialog opacity + scale 0.97 → 1 from center, 200 ms enter curve; exit 140 ms |
| Notification toast | from the top-right edge, 8 px + opacity, 220 ms; exits the same way, 160 ms |
| New message at the tail (others) | opacity + 4 px rise, 160 ms; none when loading history, none when catching up more than 3 messages |
| Own sent message | appears at once in text-3 (发送中), turns text-1 when acknowledged: color 180 ms |
| Streaming bot reply | text appends with no per-token animation; the accent caret is solid (no blink) and fades out 200 ms when done |
| Typing dots | opacity steps 0.3 → 1, 1.2 s loop (the one loop; it stops at once when typing ends) |
| Jump to latest button | `MessageScroller::with_jump_button_transition(160 ms)`, opacity + 4 px |
| Status chip change | background and text color cross-fade 180 ms `ease` |
| Unread / mention counts | **none**; tabular numbers keep the width |
| Side panel open / close | width snaps (no layout animation); panel content fades in 120 ms |
| Sidebar section collapse | chevron rotates 150 ms; rows hide at once (no height animation) |
| Light / dark switch | follows the system, no app animation |
| Scrollbar | gpui-component's built-in fade |

Reduced motion (`accessibilityDisplayShouldReduceMotion`): remove every translate and scale and keep
the opacity fades at ≤ 150 ms. The typing dots become static.

## States

| State | Treatment |
| --- | --- |
| Hover | `hover` fill on rows; icon buttons get the same fill and text-1 icon; links underline |
| Pressed | `pressed` fill; primary goes to `primary_active` |
| Focus (keyboard) | 1px accent edge + 4px `ring` halo on inputs, buttons and the composer; list rows use the selected fill (no ring) |
| Selected | sidebar: selected fill + weight 600; topic list: raised white row (light) / selected fill (dark); settings nav: selected fill + text-1 icon |
| Disabled | text-4 text and icons, no fill; send button turns gray when the composer is empty |
| Loading | `Skeleton` rows in the topic list and timeline shaped like the real rows (avatar circle + two bars); 加载更早的消息 / 正在加载… as a centered text-3 line with a 14 px `Spinner` |
| Empty | one text-3 sentence centered in the column, no illustration (这个群还没有话题…, 没有等你处理的话题) |
| Error | inline under the field in danger color (login); a failed message shows `circle-alert` + 发送失败 · 重试 in danger 12 px below the text |
| Offline / reconnecting | an amber band at the top of the sidebar (`wifi-off` + 连接已断开，正在重连…); the composer stays usable and messages queue as 发送中 |
| Unread | weight 600 on the row title + count badge; mention badge replaces the count in the sidebar (both in the topic list when there is room) |
| Streaming | 正在输入… after the author's time + solid accent caret at the end of the text |

## Break-ui rules (from worst-case.png)

- Names in a status chip are cut to 6 Chinese or 12 Latin columns plus "…", and the verb stays
  (等Aleksandra W…审). The chip itself is at most 168 px.
- In a sidebar row with an @ badge, the @ badge replaces the count badge, so the name keeps room.
- Titles, names, previews and file names truncate at the end on one line. Branch names, URLs and
  hashes in card fields use middle truncation (GPUI only truncates at the end: compute this in the
  app).
- Message text breaks anywhere (`overflow-wrap: anywhere` in the mockup). In GPUI, long unbroken
  runs must wrap by character. Check this with a URL and with 90 × "A".
- Counts show `999+` and `@99+`. User text is always literal (no HTML, no Markdown in names or
  previews).
- One-character names get a one-character avatar. Emoji-led names use the first Latin letter, or a
  neutral initial (never half a surrogate pair).
- Time: 刚刚, n 分钟前, HH:mm today, 昨天, 周一…, then 10月12日, and with the year for an earlier year
  (2025年12月31日).

## Map to gpui-component 0.8.1

| UI element | Component | Theme slots / notes |
| --- | --- | --- |
| Window, traffic lights | `gpui_kit::open_window`, `TitleBar::title_bar_options()` (transparent, traffic lights at 20, 20) | `title_bar` = sidebar color, `title_bar_border` transparent |
| Sidebar | `sidebar::Sidebar` with `SidebarGroup` (待办, 群, 私聊), `SidebarMenu`/`SidebarMenuItem` rows, `SidebarFooter` | `sidebar`, `sidebar_foreground`, `sidebar_accent` (selected), `sidebar_border` |
| Search field | `input::Input` (`.small()`, prefix `search` icon, suffix `Kbd`) | `input`, `muted` fill via app token |
| Unread / mention badge | `Badge::count(n).max(999)` (mention: `.color(primary)`) | own style for the `@` prefix |
| Group monogram, avatar, presence | `avatar::Avatar` (`.name()` gives initials; bot: own rounded radius); presence dot as a child | avatar tone tokens are app tokens |
| Topic list | `list::List` with a `ListDelegate`, `ListItem` two-line rows | `list`, `list_hover`, `list_active`, `list_active_border` = transparent |
| Status chip | `Tag::custom(bg, fg, border)` filled; `.outline()` for quiet | `blue/_light`, `yellow/_light`, `red/_light`, `green/_light` |
| New topic, bell, panel toggle | `Button::new().ghost().icon(…)` with `Tooltip` | `secondary_hover` |
| Timeline | `MessageScroller` (`MessageScrollerState`, bottom-anchored, `jump_button`, `with_bottom_fade(background)`) | — |
| Message row | `message::Message` + `MessageAvatar` + `MessageHeader` + `MessageContent` with `Bubble` `BubbleVariant::Ghost` (no bubble) | timeline text style |
| Markdown, quotes, inline code, code blocks | `text::TextView::markdown` with `TextViewStyle` (paragraph gap 0.5 rem, code block radius 8, `code_background` = code bg) and the tree-sitter highlighter | `highlight_theme` from the code tokens |
| @mention in text | `TextView` inline highlight (`RangeHighlight`) | accent text + mention bg |
| Image | `gpui::img` in a rounded container; viewer on click | card border |
| Status card | own `div` (`group_box` surface) with `description_list::DescriptionList` for fields and two `Button`s (`.primary()`, default) | `group_box`, `description_list_label` |
| 产出 card | own row: icon tile + label + `Button::ghost` 打开 | — |
| New messages / day divider | `separator::Separator` with a label | accent at 35% / text-3 |
| Typing line | `Label` + three dots (own) | text-3 |
| Composer | `input::Input` multi-line (`InputState::multi_line().auto_grow(1, 10)`) inside an own frame; toolbar `Button::ghost().small()` | `input`, `ring`, `caret` |
| @mention popup | `popover::Popover` anchored at the caret (or `input` `completion_menu`) with `List` rows | `popover`, `accent` (active row) |
| Side panel | `resizable` panel + `DescriptionList`, `List` for 产出, `stepper` or own list for 工作过程 | app `panel` token |
| Cmd+K | `command::Command` in a `dialog` without animation (`.searchable(true)`, groups, footer with `Kbd`) | `popover`, `accent` |
| Settings window | second window; nav as `SidebarMenu`; content in `setting::Settings` / `SettingPage`, members in `table::Table` or a `List`; `select::Select` for 权限; `switch::Switch` for 开发者视图 | `table_*` = surface and hairlines |
| Invite code dialog | `dialog::Dialog` (header, description, footer); code as a read-only `Input` in mono 22 | `overlay` |
| Login | `form::Form` + `Input` + `Button::primary().large()` | — |
| Toasts | `notification::Notification` | `popover` |
| Loading | `skeleton::Skeleton`, `spinner::Spinner` | `skeleton` = selected fill |
| Context menu | `menu::ContextMenu` / `PopupMenu` | `popover`, `accent` |

### What GPUI cannot render as drawn

- **Letter-spacing:** no tracking support. Headings use 0, with CoreText optical sizing.
- **Per-element transforms on divs** (button press `scale(0.97)`, dialog scale-in). Use the pressed
  color, and opacity-only dialog entry. I did not confirm whether gpui-base animations can scale an
  arbitrary element; I looked in `gpui-pre-0.3.10/src/style.rs` and found no transform field.
- **Backdrop blur per element and mask images:** not available. The header fade is a gradient overlay
  (`linear_gradient` background) instead of a content mask.
- **Continuous-curve (squircle) corners:** circular arcs only.
- **Middle truncation:** compute in app code; GPUI truncates at the end.
- **0.5 px rings:** draw them as 1 px borders at reduced alpha if a hairline looks blurry on a
  non-retina display.
- **App icon:** macOS 26 layered Icon Composer files are out of scope. Ship the flat 1024 PNG and
  the simplified small art (16, 32) in an `.icns`.

## App icon

Two Chinese corner quotation marks, 「 in terracotta and 」 in ink, with two quiet text lines
between them: speech set in type, which is the product. The icon uses Apple's 824 px squircle on
a 1024 canvas, with a paper surface, a soft drop shadow, a light top rim, and slight vertical
lighting on the glyphs, in current macOS style. At 32 and 16 px the art drops the text lines and
uses thicker brackets, so it stays a clear 「」 mark.
