# TeamChat completeness spec

Status: binding. Engineers implement this spec. Reviewers accept a screen only when it passes the checklist in section 4.
It wins over `docs/design/NATIVE.md` and `docs/DESIGN.md` where they differ. The tokens of section 1 live in
`apps/desktop/src/ui/native/tokens.rs`.

Note: the audit folders named below (`completeness/shadcn-bar/`, `component-system/`, `main-window/`, `audit-pages/`,
`settings/`, `overlays/`, `content/`) hold the measurements and scripts behind this spec. They are not in this repository.

Scope: the macOS client (Rust GPUI + gpui-component 0.8.1), branch `origin/v12/native`. The style stays pure black and white, like shadcn/ui new-york-v4 neutral. This spec does not ask for a new style. It asks for **finish**: every region designed, every state drawn, every value on one scale, nothing that reads as a draft.

Keep the Mac client structure: the separate Settings window (640 wide, toolbar tab strip), sheets that drop from the toolbar edge, the toolbar title, the inset sidebar with traffic lights. Section 2.1 gives the coordinates.

All values are 1x px. "Muted" means the muted-foreground token. "Border" means the one border token.

Sources: the shadcn registry numbers and pixel checks in `completeness/shadcn-bar/` (best reference: `png/light/block_dashboard-01_full.png`, then `v3_ex_mail.png`, `ex_tasks.png`, `v3_ex_forms.png`, `v3_ex_cards.png`); the kit audit in `component-system/`; the screen audits in `main-window/`, `audit-pages/`, `settings/`, `overlays/` and `content/`. Each fix in section 3 names the audit measurement that proves it.

---

## 0. Decisions where the inputs disagree

The audits used two scales: the shadcn web scale (14 body, 36 controls) and a Mac scale (13 body, 24 controls). This spec uses one. Each row below is final.

| # | Topic | Decision | Reason |
|---|---|---|---|
| D1 | gpui-component rem | Set `theme.font_size = 16`, so rem = 16. Set root text to 14 explicitly. | At rem 13 every gpui-component part renders at 0.8125x (menu text 11.375, rows 26). At rem 16 gpui-component renders its own shadcn values. One choice removes the rem/px mix. |
| D2 | Body text | 14. Settings, sheets, menus, lists, messages: all 14. | Measured 11 px text is the largest single "unfinished" signal (header subtitle, date, hints, button labels). |
| D3 | Control heights | 3 steps: sm 28, default 32, lg 36. No 18, 20, 22, 24, 25, 26, 30, 31 or 47. | 32 = gpui-component md at rem 16 and the shadcn toolbar height. 28 fits inside a 44 form row. |
| D4 | Toolbar items | 32 square ghost icon buttons, radius 8. The 36 glass circles with drop shadow go. | One toolbar baseline (bar: toolbar controls all h32). |
| D5 | Radius | control 8, menu item and small box 6, checkbox 4, container 10, card and panel 14, full for pills. Window chrome keeps its own: inset sidebar 18, Mac sheet window 22. | shadcn new-york-v4 with --radius 10. |
| D6 | Spacing | 2, 4, 6, 8, 12, 16, 24, 32, 48. No 20. | Bar scale. The 20 gutter and 20-22 group gaps in Settings become 24. |
| D7 | Message avatar | 32. Text column = gutter + 32 + 12. | Avatar sizes 16/24/32/40/64 only. |
| D8 | Form anatomy | One grouped FormRow in Settings **and** in every sheet (label left, control right). A textarea is a full-width row in the same group. | Settings and overlay audits both accept it; it keeps the Mac look. |
| D9 | Page title | The Mac toolbar title (16 medium + 12 muted subtitle) is the page title of every main-window page. No second 24 px title in the content. 24 px titles appear only on full-window screens (login) and the member profile hero. | Keeps the Mac toolbar title; removes duplicate titles. |
| D10 | Selected row | Muted fill (#f5f5f5 light / #262626 dark) + font-medium, radius 6 or 8. Never an inverted black row. | Today the palette, start-dm and mention popup use black rows and the sidebar uses gray. |
| D11 | Cancel button | Outline variant (border + background). The secondary gray fill is not used for Cancel. | A disabled primary at 50% opacity must not look like Cancel. |
| D12 | Progress | One Progress: h4, radius full, track = primary at 20%, value = primary. | Fixes the dark-mode inversion and the 3 progress styles. |
| D13 | Right pane | 320 wide for the inspector, 改动, Home rail and side chat. 400 wide for the live pane (实时查看) and the artifact pane, which show a screen or a page. (Owner decision, v14.) | Today 280 / 320 / 380; a terminal or a web page at 320 is unreadable. |
| D14 | Error text (owner decision v16) | Every error says its words in the foreground color with a red icon, the failed message ("没有发出") too. Red text is never the only carrier. | One error recipe (2.15). |
| D15 | Group and desk list rows (owner decision v16) | The leading media of a 话题 row is the member's avatar (the kind icon when no member works on it); the state shows only in its StatusBadge. The group page splits its open 话题 into 等你 (asks to me) and 在做 (runs); a waiting row never sits under 在做. | One state carrier per row (2.12). |
| D16 | Queued 话题 in the sidebar (owner decision v17, O-1) | A 排队 话题 has no sidebar badge: the sidebar stays quiet for the resting state. The title bar, inspector and pages still say 排队. | The sidebar right column carries what needs attention or is at work. |
| D17 | 完成 and 交付 (owner decision v17, O-2) | 完成 means a done 话题, on every page (群 最近完成, 首页 rail, 成员 完成的话题, the sidebar section 完成 · 今天). A run result is 交付: its badge word is 已交付, and it counts as 交付 (首页 KPI 今天交付, 成员 最近交付 and its stats). | One word, one meaning: a page never counts runs as 完成. |
| D18 | Pinned banner place (owner decision v17, O-3) | The pinned banner docks above the composer, x 288-1096, as NATIVE.md §9.2 says. | The decision bar sits next to the reply it asks for. |

---

## 1. The system

### 1.1 Tokens: one module

- One token module: `ui/native/tokens.rs`. `theme.rs` `size::`, `radius::` and `type_scale::` become aliases, then are deleted.
- One color struct. `theme::apply` writes gpui-component `ThemeColor` in **one** pass (today 3 passes: base, v12/kit, v12/shell).
- Check: `rg 'pub const' apps/desktop/src` finds heights, radii and text sizes only in `tokens.rs`.
- Check: a test counts `px(` literals in `views/`. Today 1408. The count can only go down. Target for `.rounded(px(`, `.text_size(px(`, `.gap(px(` and `.h(px(` in views: 0.

### 1.2 Color tokens (neutral, black and white)

| Token | Light | Dark | Use |
|---|---|---|---|
| background | #ffffff | #0a0a0a | Window content, message column |
| foreground | #0a0a0a | #fafafa | All primary text and icons |
| card | #ffffff | #171717 | Card, panel, sheet, popover surface |
| sidebar | #fafafa | #171717 | Sidebar fill, group box fill in Settings |
| muted (fill) | #f5f5f5 | #262626 | Hover, selected row, skeleton, header fill of a table, icon box |
| accent | #f5f5f5 | #262626 | Same value as muted. Menu and palette highlight |
| muted-foreground | #737373 | #a1a1a1 | **All** secondary text, placeholders, meta, timestamps |
| border | #e5e5e5 | rgb(255 255 255 / 16%) (owner decision v15: raised until ≥ 1.5:1) | **All** 1 px lines: toolbar border, pane borders, separators, frames |
| input | #e5e5e5 | rgb(255 255 255 / 20%) | Input and outline button border, switch OFF track |
| ring | #a1a1a1 | #737373 | Focus ring, 3 px at 50% |
| primary | #171717 | #e5e5e5 | Primary button, switch ON track, Progress value, selected chart bar |
| primary-foreground | #fafafa | #171717 | Text on primary |

Rules:
- Exactly one muted-foreground and one border value. Today light muted text uses 6 grays (L 114-139, contrast 3.18-4.48) plus a bluish gray (111,111,119). They all become #737373 (4.7:1 on white, 4.5:1 on #fafafa).
- Placeholder = muted-foreground. Today placeholder contrast is 2.8 in both modes.
- No lighter muted variant on filled surfaces (today captions at 3.7:1 on #f2f2f2 cards).
- Dark border (owner decision v15): white at the lowest alpha that measures at least 1.5:1 on the background (#0a0a0a) **and** on the card fill (#171717): 16 %. The input border is 20 %, one step above it. 10 % measured 1.3:1 and is retired; the values live in `tokens.rs`.
- Dark values in this table come from the shadcn v4 neutral theme. I did not find the theme block in `shadcn-bar/reg/`; confirm `accent` dark (#262626) against `ui.shadcn.com/r/themes` before merge.

### 1.3 State colors (only for state)

| State | Light | Dark | Allowed on |
|---|---|---|---|
| destructive / failed | #dc2626 | #f87171 | Badge icon, destructive button fill, error icon (field and sheet error text stays foreground, v15), invalid field border |
| warning / needs you | #d97706 | #fbbf24 | Badge icon, Alert icon |
| success / done / online | #16a34a | #4ade80 | Badge icon, presence dot |
| info / in progress / unread / link | #2563eb | #60a5fa | Badge icon, unread dot (8 px), link text |

- A state color is never the only carrier of a state. It always sits next to text (badge, alert, labeled dot).
- Destructive button fill (decided in v14): #dc2626 in both modes, white label 4.8:1. The dark token #f87171 is for icons and text only (white on #f87171 is 2.8:1). Token: `tokens::DESTRUCTIVE_FILL`.
- Text selection is neutral: foreground at 15 %, never a state color.
- Diff lines (owner decision v15) may keep a 10 % green / red line tint (code convention). No other surface carries a state tint.
- No state color for provenance or decoration: the blue fill (226,240,255) for "Ledger 刚加的" blocks, the orange accent and orange progress bar in web-embed, and blue progress bars all go.

### 1.4 Type scale

Font: SF Pro Text with PingFang SC for CJK. Numbers in columns, KPIs and times use tabular figures (`tnum`).

| Role | Size / line | Weight | Color |
|---|---|---|---|
| KPI number | 30 / 36 (24 / 32 in a block narrower than 250) | 600 | foreground |
| Hero title (login, member profile) | 24 / 32 | 600 (700 allowed) | foreground |
| Dialog / sheet / alert title, empty title | 18 / 28 | 600 (empty title 500) | foreground |
| Toolbar title, card title, pane title, section title | 16 / 24 | toolbar and pane 500; card and section 600 | foreground |
| Body, message, list title, table cell, menu item, input, button | 14 / 20 (message prose 14 / 22) | 400; button, list title, field label 500 | foreground |
| Description | 14 / 20 | 400 | muted-foreground |
| Meta, timestamp, toolbar subtitle, group label, badge, column header in compact tables, caption, kbd | 12 / 16 | 400; group label and badge 500 | muted-foreground (badge: foreground) |
| Code | 13 / 20 mono | 400 | foreground |

- 6 sizes only: 12, 14, 16, 18, 24, 30 (+13 mono). No 10, 11, 11.375, 13 UI, 15, 17, 20, 22 or 26.
- Weights 400, 500, 600. 700 only for the 24 hero title.
- Check: CJK ink height at 1x is about 0.85x the size. 12 px text measures 10-11 px ink; 14 measures 12; 16 measures 13-14. Any CJK ink at 9 px or less is a failure.

### 1.5 Spacing

Scale: 2, 4, 6, 8, 12, 16, 24, 32, 48.

| Use | Value |
|---|---|
| Icon to text inside a control | 8 (6 in a badge: 4) |
| Between controls in one row | 8 |
| Control padding x | default 16 (12 with an icon), sm 12, lg 24 |
| List row padding | x 12, y 12 (row with avatar); sidebar item p 8 |
| Form row padding | x 16, y 12 |
| Card padding, section gap, page gutter, sheet padding | 24 |
| Card grid gutter, KPI gap | 16 |
| Section header to its group or card | 12 |
| Title to description | 4 (dialog header 8) |
| Between message groups / within a group | 16 / 4 |

No 3, 5, 7, 9, 10, 14, 18 or 20 for layout gaps (today `gap 3` x9, `5` x3, `10` x7, `1.5` x2, `2.5` x1).

### 1.6 Radii

| Radius | Elements |
|---|---|
| 4 | checkbox, kbd |
| 6 | menu item, palette row, 32 media box, small option box |
| 8 | button, input, select, segmented trigger, sidebar item, skeleton line, popover / menu surface, tooltip |
| 10 | group box, table frame, tabs list, alert, empty state, toast, 40 icon box, composer |
| 14 | card, inspector section card, media viewer |
| full | badge, count pill, avatar (human), switch, presence dot, search field |
| 18 / 22 | window chrome only: inset sidebar 18, Mac sheet window 22 |

Agent avatars use a squircle at radius = size x 0.25 (8 at 32). Human avatars are circles.

### 1.7 Borders and shadows

- Every bordered surface has a 1 px border token line. A shadow never replaces the border. A fill step alone is not an edge (today cards are #f2f2f2 on white at 1.12:1 with no border).
- Light and dark have the same set of lines (today light sheets have no border; dark sheets have #3f3f40).

| Shadow | Value | On |
|---|---|---|
| xs | 0 1px 2px rgb(0 0 0 / .05) | input, outline button, KPI card |
| sm | 0 1px 3px rgb(0 0 0 / .1), 0 1px 2px -1px rgb(0 0 0 / .1) | card, active segmented trigger |
| md | 0 4px 6px -1px rgb(0 0 0 / .1), 0 2px 4px -2px rgb(0 0 0 / .1) | menu, popover, tooltip, toast, palette |
| lg | 0 10px 15px -3px rgb(0 0 0 / .1), 0 4px 6px -4px rgb(0 0 0 / .1) | sheet, alert |

Elevation has 3 levels: content (none, xs, sm), float (md), modal (lg).

### 1.8 Controls

| Control | Size | Detail |
|---|---|---|
| Button default | h32, px16 (px12 with icon), radius 8, 14 medium, icon 16, gap 8 | Variants: default (primary), outline, secondary (muted fill), ghost, destructive (red fill, white text), link. Outline hover (v16, K-04): `control_hover` = accent in light; in dark the input token at 50 % over the card fill, so the step over the dark control fill is visible |
| Button sm | h28, px12 | In form rows, table cells, card footers in dense cards |
| Button lg | h36, px24 | Login, empty-state CTA in a full page, composer Send |
| Icon button | 32 (sm 28), radius 8, icon 16 | Ghost in toolbars and rows |
| Loading | Spinner 16 replaces the icon or the label; width stays the same; label "正在…" optional | |
| Disabled | 50% opacity of the same variant | Never the secondary fill |
| Input / select | h32, px12, radius 8, 1 px input border, shadow-xs, 14 text; sm h28 | Focus: border ring + 3 px ring at 50%. Error: destructive border + 3 px destructive ring at 20% |
| Search field | h32, radius full, 16 search icon at x+12 | Sidebar and filters |
| Switch | 32x18 track, 14 knob | OFF: track input, knob background (dark: knob foreground). ON: track primary, knob primary-foreground. Knob-to-track contrast >= 3:1 |
| Checkbox | 16, radius 4 | Checked: primary fill, check primary-foreground |
| Segmented / tabs | list h32, p3, radius 10, bg muted; trigger h26, px8, 14 medium, inactive 60% foreground, active background + shadow-sm | Optional count pill inside the trigger |
| Badge | h22, px8 (px6 in tables), radius full, 12 medium, 1 px border, icon 12, gap 4 | default, secondary, outline, destructive |
| Count pill | 20 circle (min-w 20), 12 medium tabular, bg muted | |
| Kbd | h20, min-w 20, px4, radius 4, 12 medium, border | One cap per key |
| Progress | h4, radius full | Track primary 20%; value primary. Steps variant: segments with 2 px gaps |
| Avatar | 16, 24, 32, 40, 64 only | Fallback: muted fill + 1 px border + initial in foreground 500 (12 at 16-24, 14 at 32-40, 24 at 64). Same style for every person, the current user included. Presence dot 8 with a 2 px ring of the surface, green = online only |

### 1.9 Component set

Use gpui-component as it is, at rem 16. Wrap each kind once in `ui/native`. Views build only from these wrappers.

| Kind | Implementation | Today | Delete |
|---|---|---|---|
| Button | `ui/native` `push_button` on gpui `Button` at sizes 28/32/36, radius 8 | 5 rendered shapes; 3 code paths; 23 raw `Button::new` in 15 view files | `ui/controls.rs` `button`, `icon_button`, `icon_button_sm`, `text_button`; capsule `PushSize::Large`. Check `rg 'Button::new\(' views` = 0 |
| Input, SearchField, Textarea | `NativeTextField` / `SearchField` on gpui `Input` | 4 shapes (30 capsule, 24 capsule, 28 boxed in a 36 row, 36 capsule); 3 paths | `controls::TextField` |
| Select / pop-up | `popup_button` on gpui `Select` look (h32/h28, chevron 16, truncates at end) | hand-rolled | |
| Menu | gpui `PopupMenu` / `DropdownMenu` / `context_menu` at rem 16 (rows 32) styled per 2.11 | 3 renderers: PopupMenu 26 rows at 11.375, MenuRow 24 rows, `overlay::menu_row`/`popup_row` (40) | `overlay::menu_row`, `overlay::popup_row`, `MenuRow` once mention popup and pop-up list use PopupMenu |
| Tabs | native `Segmented` (one height 32) | Segmented, `rows::tab()` (11 sites), Settings toolbar tabs | `rows::tab()`; Settings toolbar tab strip stays (Mac chrome) |
| Switch, Checkbox, Radio | `native/toggles.rs` on gpui_base (keep, fix dark ON) | consistent | |
| Card | new `Card` (2.4) | `parts::card`, `rail_card`, `inset_row`, `native::card_frame`, `card_well`, `card_v2` | all six |
| GroupBox / GroupedForm | gpui `GroupBox` (fill variant) with FormRow (2.8) | GroupedForm, InspectorSection, new-group list box | duplicates |
| DescriptionList | gpui `DescriptionList` (label column 112, 14 muted; value 14) | hand-rolled `label_value` | `label_value` |
| List row | `ListRow` (2.6) | 8 row kinds | `rows::list_row`, `inset_row`, `spotlight_row`, `sheet_list_row` (map to ListRow / FormRow / MenuRow) |
| Table | gpui `Table` / `DataTable` (2.7) | 0 uses; card tables hand-rolled (TABLE_ROW 32, TABLE_NUM_COL 52) | hand-rolled tables |
| Badge / StatusBadge | gpui `Badge`/`Tag` wrapped once as `Badge` + `StatusBadge` (2.12) | StatusChip, CountBadge, RoleTag, JobTag, chip Kbd + 3 gpui | the 5 own kinds |
| Progress | gpui `Progress` (D12) | 0 uses; `determinate_bar`, plan bars (5 px blue, 3 px segmented) | `determinate_bar` |
| Skeleton | gpui `Skeleton` (2.14) | 0 uses | |
| EmptyState | `parts::EmptyState` (2.13) | used once; gpui `Empty` once | ad-hoc centered lines |
| Alert / Banner | gpui `Alert` wrapped as `Alert` (2.15) | overlay Banner, `connection_banner.rs`, `banners.rs` | duplicates |
| Toast | gpui `Notification` through one `Toast` wrapper (2.16) | overlay Toast (44), Notification (6 sites, text 11.4) | `overlay::Toast` |
| Sheet / Alert modal | `native_sheet` (460 / 560) and `native_alert` (260) | 4 paths (native_sheet 8, open_dialog 12, open_sheet 9, native_alert 1), 6 widths | direct `open_dialog` / `open_sheet` in views |
| Surface | `float_surface` (menu, popover, toast, tooltip, palette) | 6 surfaces | `overlay::menu_surface`, `native::menu_surface`, `overlay_surface`, `mention_surface`, `popover_surface` |
| Separator | `hairline` (1 px border token; horizontal and vertical) | 5 kinds | `overlay::menu_separator`, `native::menu_separator`, `vhairline`, `divided` (keep one fn with an axis) |
| Section header / group label | `SectionHeader` (2.3) and `GroupLabel` (12 medium muted h32) | 6 kinds | `panel_label`, `SidebarSection` (to GroupLabel), `spotlight_section`, `parts::rail_section` |
| Kbd, Tooltip, Spinner, Popover | gpui as is | used | |

Keep as they are (already consistent): sidebar row pitch 32 and right column at x 238; settings and inspector row pitches; toggles from one source; the 2 sheet widths; the scrim (light 20%, dark #121212); sheet top at y 52; palette width 600 and 32 row pitch; red error state of the name field; ellipsis truncation in device, machine and member rows; the Mac toolbar tab strip.

---

## 2. Anatomy patterns

### 2.1 Window regions (main window, 1440 x 900)

```
x: 0 ───── 264 ─────────────────────────── 1120 ──────── 1440
   sidebar │ message column (856)           │ right pane (320)
y 0-52     traffic lights │ toolbar: title + actions (one band, border-b at y 52 across 264-1440)
```

- **Sidebar** (x 0-264): inset panel x 8-256, y 8-892, radius 18, fill sidebar, 1 px border. From top to bottom:
  1. y 8-52: traffic lights band (Mac). Nothing else.
  2. Search field h32, mx 8.
  3. Groups. Each group: GroupLabel (h32, px8, 12 medium muted), then items h32 (p8, radius 8, gap 8, icon or avatar 16 at one x, 14 text that truncates with an ellipsis before the right column, right column right-aligned at x 238 for count pills, times or one badge). Hover (owner decision v15): muted at 50 % on rows that are not selected, drawn at once (no transition); every sidebar and list row uses it, through the one focusable row (`ui/native/focus.rs` `FocusRow`: tab stop, ring distinct from the selection, Enter / Space, ↑ / ↓). One right-column kind per group: a member group shows a StatusBadge on every busy member ("等你 2", else "在做 3"); an open member keeps its badge. Every group has a label, the agent group included. Inside one group either every item has a 16 media or none has. A collapsed tail ("空闲 · 3", "不在 · 2") is one "More" item at 70% opacity with a chevron.
  4. `mt-auto` secondary items (Settings, Help) h32.
  5. Footer, pinned: user row h48, p8: avatar 32 (radius 8 for workspace logo, circle for the person), name 14 medium over 12 muted second line (workspace or role), chevrons-up-down 16 at the right. It opens account and Settings. The offline state replaces the second line and adds an Alert row above the footer (2.15). Groups scroll above the footer.
- **Message column** (x 264-1120, or 264-1440 when the right pane is closed): gutter 24, so content x 288-1096. Two left edges only: **gutter** x 288 (toolbar title, avatars, date-divider rule start, banners, composer frame, page rows) and **text column** x 332 (= 288 + 32 + 12: names, message bodies, in-thread cards, attachment items).
- **Right pane** (x 1120-1440, 320 wide; x 1040-1440, 400 wide for the live and artifact panes, D13): border-l, fill background. One anatomy for inspector, Home rail and side chat (2.5).
- Every pane runs to the bottom of the window. No blank band taller than 24% of the pane height (216 at 900) unless it holds a designed Empty state. On pages (首页, 群, 成员, 产出), space below the last frame is allowed: frames hug their rows and footer row and never stretch to the window bottom (A1).

### 2.2 Toolbar and pane headers

- **Owner decision v18 (wins over the 52 / border lines in this file):** every header (toolbar and right panes) is one line: the title, then its subtitle or state badge after it on the same line, centered in an h64 band (y 32, a little under the traffic lights; read every "y 52" here as y 64); never a subtitle under the title. Borders only where they separate something: list frames, list rows, list footers, StatusBadges (muted fill), icon boxes and Card v2 choice rows (muted fill; only the picked row draws a foreground border) have no line; a card is the card fill, shadow sm and a 6 % foreground ring, no 1 px border. The group page's 新话题 is a Secondary button with a plus, not the black primary.
- One header band h52 across the message column and the right pane. One 1 px border-b at y 52 from x 264 to 1440. Content scrolls under it; no fade.
- Message column header: title 16 medium, subtitle 12 muted under it (or a breadcrumb in 14: ancestors muted, current foreground, chevron-right 16 muted separators). Status in the subtitle row is a StatusBadge, not colored text; on a 话题 the badge stays under the title, at the start of the subtitle row (owner decision v17). Actions at the right: 32 ghost icon buttons, gap 8; a vertical separator h16, mx 8, before the right-pane toggle.
- Right pane header: same h52 band. Pane name 16 medium ("话题信息", "群信息", "今天", "单独问 Ledger", "产出"), optional 12 muted description, actions as 32 ghost icon buttons (close at the far right). A pane that shows one item names the pane, never the item: the 产出 pane reads "产出" with "<title> · <kind> · v<n>" as its description. The first content row starts at y 52 + 16 = 68.
- Page toolbars (Home, Artifacts, Members, Group): all controls h32 on one center line (y 26), gap 8: segmented h32, select h32, search field h32 w 200-250, icon buttons 32, primary sm action at the far right.
- A filter row under the page header: h32, 16 below the header border, 16 above the list or table.

### 2.3 Section header

- Title 16 semibold, description 14 muted, gap 4. Optional action (button sm h28 or ghost icon button 28) right-aligned, aligned to the top of the title. Optional count after the title as a count pill.
- 12 from the header to its group, card or list. 24 between sections.
- The sidebar, menus, palette and inspector use GroupLabel instead (12 medium muted, h32).

### 2.4 Card

- Root: flex-col, gap 24, py 24, radius 14, 1 px border, card fill, shadow-sm.
- Header px 24, grid gap 8 (title row then description row): title 16 semibold; description 14 muted; optional action in column 2 spanning both rows, right-aligned at the top (badge, segmented, close or refresh, "⋯").
- Content px 24.
- Footer px 24, flex, items-center, gap 8. With border-t it gets pt 24. The description always sits above the actions.
- A card ends with its footer or with 24 under the content. Never with a blank band.
- **Compact card** (in a thread, 640 max, dense data): gap 16, p 16, header title 14 semibold (every compact card, Card v1 and v2). Same slots. A draft well previews 6 lines, then cuts on a whole line.
- **Diagram viewer**: a 560 sheet (2.9) under the toolbar (y 52); text never below 12 (it scrolls instead).
- **In-thread card**: width = min(640, text column width), x = text column (332). Every card, attachment item, reference block and embed in one thread has this one width. Galleries use the same width or exact halves (2 columns, gap 16).
- **Card header slots** (every card kind: Card v2, in-thread, interact, web artifact): StatusBadge + title; meta 12 muted line; time 12 muted, right-aligned in one column; action top right. The author row stays outside the card in a thread and inside it on Home and in notifications, in the same slot.
- **Working card** (one component, `views/timeline/plan.rs` `WorkingCard`, used by the thread and Card v2): header = avatar 24 + "<member> 正在做：<current step>" 14 medium (no plan: "<member> 在做") + StatusBadge (spinner, "在做 3/5") at the right; content = Progress h4 + "3/5" 12 muted tabular (only with a plan), then the steps; footer = elapsed and start time 12 muted + ghost sm 查看 and **outline** sm 停止… (owner decision v15). In a thread the card sits on the text column (x 332) and its header avatar names the member; the gutter stays empty. While 实时查看 shows the run, the thread keeps only the card's header (v16): the same avatar, title and StatusBadge (已暂停 while someone holds its computer) and "在右侧打开" 12 muted.
- **KPI card**: description = label 14 muted; title = number 30 semibold tabular (24 when narrower than 250); action = trend Badge (outline, 12 icon); footer 2 lines (14 medium + icon 16, then 14 muted), gap 6. KPI cards sit in an equal grid, gap 16, shadow-xs.

### 2.5 Right pane (inspector, rail, side chat)

- Header per 2.2 (the title names the pane: 实时查看, with the computer, machine and time as its description and the run's StatusBadge beside the title, v16). Body scrolls by itself, px 16 (the live pane too: one inset for header, panels and footer), sections gap 24. Footer pinned, border-t, p 16, full-width buttons stacked (primary first, gap 8), only when the pane has an action. Exception (owner decision v15): the live pane (实时查看) footer puts 停止… and 接管 side by side as equal outline buttons.
- Panels inside a pane (live acceptance, plan, held banner, compact empties) are bordered containers (radius 10, 1 px border, px 16), never a fill step. A pane section with no data shows its compact empty (2.13) in its place.
- Section: GroupLabel (12 medium muted, h32) with an optional 28 ghost action at the right; then a DescriptionList: label 14 muted in a 112 column at x 1136, value 14 foreground from x 1256, left-aligned, one line, truncate with tooltip; a person value = avatar 16 + name (the name keeps priority; the role truncates first).
- **Topic inspector**: the same fixed sections in every topic, in this order (template: refs-card-step and topic-v2): 状态 (StatusBadge, Progress), 负责 / 提出 (people), 计划 (steps), 改动, 产出 (artifact rows), 决定, 在哪干活 (machine), 时间和花费. A section with no data shows one 16 icon + 14 muted line ("还没有计划"), and never disappears; its action ("指派" for an empty 负责) is the section header's ghost action. 计划 shows one block per member at work, each led by the member. Footer from the state: "标记完成" (primary) only while nothing waits on a review, an answer or a merge; "交给…" (outline) always, full width.
- **Home rail**: only data the main pane does not show (排队 schedule, this week KPIs as DescriptionList, machines). Never the same items as the main list.
- **Side chat**: header title "单独问 <name>", description "只有你能看到" 12 muted (once the member has read the 话题: "只有你能看到 · 读过 10:20 之前的话题", never a line in the body), member select h28 and close 32 in the header. Body: the same message rows as the main thread (no bubbles). Empty: an Empty state (2.13) centered in the body with 2-3 suggested questions as outline sm buttons. Composer docked at the bottom.

### 2.6 List row

- **ListRow** (row with media and subtitle): h56 (py 12, px 12), gap 12: media 32 (avatar, or a 32 box: radius 6, border, muted fill, 16 icon); title 14 medium (one line, truncate) over description 14 muted or meta 12 muted; then a right cluster in fixed order and fixed widths: StatusBadge column, time 12 muted tabular right-aligned, actions column (sm h28 buttons, primary at the right edge, secondary to its left, gap 8; or a 28 ghost "⋯"). All right-cluster items center on the row center.
- In one list every row has the same columns at the same x. A row with no action keeps the action column empty; a section where no row has actions drops the column, so the time stays at one x.
- **Compact row** (single line, sidebar-like): h32 or h36, media 16.
- Rows in one list are separated by 1 px border, inset to the title x, none after the last row. Hover bg muted at 50%. Selected bg muted + 500.
- A list sits in one frame (radius 10, border) or in a Card. Never loose rows on the background.
- A list longer than its region ends with a footer row h40: "看全部 23 个话题 →" 14 muted, or the count "共 12 个" left and pagination right.
- The topic list of a group uses the ListRow above (owner decision v15; the earlier mail-style item is retired).
- Hover is muted at 50 %, drawn at once (owner decision v15).
- Leading media (owner decision v16): a 话题 row on 群 and 首页 leads with the avatar of the member who does it (the run's member, else the owner, else the asker), or the kind icon box when there is no member. The status shows only in the StatusBadge column, never a second time in the media. A row's title and meta cut with "…"; the whole text shows in a tooltip only when a line is cut (§2.21), never on a row that fits.

### 2.7 Table

- Frame radius 10, 1 px border, overflow hidden. Header row h40, 14 medium foreground (12 medium muted in compact tables inside cards), bg muted when sticky. Rows h48 (h40 compact; h52 when a cell holds an h28 control), border-b between rows, none after the last. Cell padding x 12 (first and last cell x 16), align-middle, nowrap, truncate.
- Columns fill the frame: the text column is flexible, number columns have a min width so values never truncate, the last column ends at the frame's right inset. Numbers right-aligned with right-aligned headers, tabular, thousands separators.
- Column 1: checkbox 16 when rows are selectable. Type: outline badge px6. Status: StatusBadge. Last column: 28 ghost "⋯" opening a DropdownMenu (min-w 128).
- Footer under the table, px 16, h48: left "已选 0 / 68 行" 14 muted (or the count); right, gap 32: "每页" 14 medium + select h28 w80 (10, 20, 50, 100; `page::table_footer`); "第 1 / 7 页" 14 medium; 4 outline icon buttons 28 (first, previous, next, last), gap 8. An unavailable direction is disabled at 50%, not hidden.
- Empty: one cell spans all columns, h96, centered "没有结果" 14 muted. Header and footer stay.
- Deleted row or message: tombstone: 16 icon (trash) muted + 14 muted text "此消息已删除", no author emphasis.

### 2.8 Form row and fields

- **GroupBox** (Settings and sheets): radius 10, 1 px border, fill sidebar (#fafafa / #171717), no shadow. Rows inside separated by 1 px border, inset to the label x (16, or 16 + media + 12).
- **FormRow** single-line h44 (px 16, py 6 around an h32 control, or py 8 around an h28 sm control); two-line h64 (label 14 over description 12 muted, gap 4, py 12). Label 14 regular foreground at x + 16.
- Label column: 112 fixed in every sheet (fields start at the same x in new-topic, rename-group, rename-topic, new-group). In Settings the label takes the free width and the control sits in the value column.
- **Value column**: all trailing controls and trailing text right-align to group right - 16. Selects in one column share one width (default 160). A read-only value in a column of selects is a disabled select of the same width, or an outline badge right-aligned to the same edge.
- One control per row. A dependent value (每日摘要 time, 勿扰 从/到) is its own row under the parent row.
- Inline field in a row: the standard Input (h32 or sm h28), border, shadow-xs. No extra fill box or 2 px ring box around it.
- Number field: width 112, right-aligned tabular, prefix ("¥") inside the field at x + 12, thousands separators.
- Textarea: a full-width row in the same group, label in the same column x, min 3 lines, focus ring inside the group bounds.
- Error: field border destructive + 3 px ring destructive 20%; the message (owner decision v15: foreground text with a 16 destructive icon, never red text) appears in the sheet footer's left slot (2.9) or as a dependent row inside the group. The sheet height does not change. Login uses the same recipe.
- Navigation row: label + value 14 muted + chevron-right 16 muted ("按群设置 · 8 个群 ›").

### 2.9 Sheet (Mac sheet, dialog anatomy)

- Width 460 (form) or 560 (picker list). Padding 24 on all 4 sides. Surface card, 1 px border, shadow lg, chrome radius 22. Scrim unchanged.
- Header gap 8: title 18 semibold, description 14 muted (one sentence). Every sheet has both, update-download included.
- Body: 16 below the header. One content column: every block (group box, list box, search, textarea including its focus ring) shares the same left x and right x (0 px deviation).
- Footer: 24 below the body. Left slot: count ("已选 2 位") or the error (2.8: foreground text, red icon). Right: outline Cancel then primary, h32, gap 8; the right edge of the primary equals the content right edge (today 4-5 px overhang).
- Primary is disabled (primary at 50%) until the required fields are valid.
- Long content scrolls inside the body; header and footer stay.
- **Member picker** (new-group, add-members, start-dm: one component): search field h32 full width; 12 below it a list box (radius 10, border) showing 7.5 rows so the cut row signals scroll, plus a scrollbar; rows h40: avatar 24 (presence dot green only), name 14 medium, role 14 muted, truncate. Multi-select: checkbox 16 at the left. Single-select: check icon 16 at the right on a muted row. Footer left "已选 N 位". Empty: "没有匹配的成员" Empty (compact, 2.13). Keys (owner decision v15): ↓ from the search field moves into the list, ↑↓ move the highlight, Space toggles the highlighted member (multi-select), ↩ / ⌘↩ submit.
- Avatar grid (add-bot): a column count that divides the set (for example 26 avatars + 1 random cell = 3 x 9), cells 40, gap 8.

### 2.10 Alert (Mac confirm)

- Width 260, padding 16, chrome radius 22, shadow lg. App icon 48 centered, 12 below: title 14 semibold centered, 4 below: body 12 muted centered, 16 below: buttons stacked full width h32, gap 8.
- Destructive confirm (leave group, stop agent, remove member, sign out, remove device): the destructive variant (red fill, white text) at the top, outline Cancel below. Non-destructive: default primary at the top, outline Cancel below.
- A checkbox label fits one line ("同时删除本机数据").
- Information with actions (update available, manual install, downloading) uses this alert, not a sheet.

### 2.11 Menu, popover and command palette

- **Menu**: float surface, radius 8, p 4, border, shadow md, opaque, min-w 128. Item h32, px 8, radius 6, 14 text, icon 16, gap 8, shortcut 12 muted at the right. GroupLabel 12 medium muted h28 px 8. Separator 1 px, mx -4, my 4. If any item in a menu is checkable, every item reserves the 16 + 8 check column; otherwise none does. Highlight = accent fill.
- **Mention popup**: a menu anchored to the caret (left edge = caret x - 8, above the composer), w 320, max 8 rows, GroupLabel per group, selected = accent fill.
- **Command palette** (search, switcher, ask): w 600, radius 10, float surface, shadow md. Input row h48, search icon 16, 16 text, border-b. List p 4: GroupLabel 12 medium muted h32; rows h32, radius 6: leading 16 icon or 16 avatar for the kind (topic, group, DM), title 14, meta 14 muted, StatusBadge at the right if any, time 12 muted tabular right-aligned at one x in every row, then a reserved 20 column for the ↩ hint (shown only on the selected row). Selected row = accent fill. Footer h40, border-t, 12 muted Kbd hints: "↑↓ 选择 · ↩ 打开 · esc 关闭"; when no row can be chosen (no match, failed, loading) only "esc 关闭". Max 8.5 visible rows, so a half row signals scroll; the panel height is header + rows + footer, never a row cut by the panel edge.
- Palette states: empty query = recent list; no match = compact Empty ("没有找到“X”", one action); loading = 3 skeleton rows h32; error = Alert row with Retry.
- Scope and target chips: max 200, truncate at the end with "…", full value in a tooltip.

### 2.12 Badge and status

- StatusBadge = outline Badge (h22, 12 medium foreground text) with a colored 12 icon. One mapping everywhere:

| State | Text (zh) | Icon | Color |
|---|---|---|---|
| needs you | 等你 / 等你审 / 等你回答 | alert-circle | warning |
| working | 在做 3/5 | loader (spinning) | info |
| stopping | 停止中 | loader | muted |
| merging | 正在合并 | loader (spinning) | info |
| paused | 已暂停 | pause | muted |
| waiting on someone else | 等李雪回答 / 等李雪审 / 等审 | clock | muted |
| sending (my answer or message in flight) | 发送中 | loader (spinning, `BadgeState::Stopping`, v17) | muted |
| queued | 排队 (no sidebar badge, D16) | clock | muted |
| done / merged / answered | 完成 (a done 话题, D17) / 已合并 / 已回答 | check-circle (filled) | success |
| delivered (a run result, D17) | 已交付 | check-circle (filled) | success |
| failed / stuck | 失败 / 卡住了 | x-circle | destructive |
| abnormal | 异常 | alert-triangle (+ tooltip with the cause) | destructive |
| online | 在线 | dot 8 | success |
| offline / idle / away | 离线 / 空闲 / 不在 | dot 8 outline | muted |
| deactivated (also an expired or superseded Card v2, v17) | 已停用 | ban | muted |

- One mapping (owner decision v15, `topic_page::topic_state`): **在做** only while a member's run works (or waits on other members), with "n/m" when it has a plan, where n/m is done/total (steps done over all steps) in every badge, meta line and Progress bar (one count, `topic_page::plan_progress`); two or more members at work read "2 个成员在做", in the sidebar too. A 话题 marked in progress with no run is **排队** (clock, muted). Merging is **正在合并**; an answer or message in flight is **发送中** (v16); 进行中, 合并中, 排队中, 未开始 and 正在发送 are not used. The sidebar right column, title bar, inspector 状态, run log and Card v2 use these words. Card v2 (owner decision v17): an expired or superseded card reads 已停用; a PR card that waits on my review reads 等你审.
- Order (v16, T-03): a merge in progress first; then what waits on me: every open ask for me in the 话题 (`asks_for_me` less the asks my answer already reached, `topic_page::ask_answered`) reads 等你审 (a review, "等你审 PR #61" with its PR) or 等你回答 (any other ask), before any run; an ask I answered reads 已回答 until the server follows. Then stopping, at work or paused, a failed run, then the 话题's own status (等李雪回答 / 等审, clock, muted, also in the sidebar). Every page badge of a 话题 (群, 首页, 成员) shows these same words; a page never shortens 等李雪回答 to 等人 (v16). The inspector's 标记完成 follows this state.
- One clock per run (v16, P-01): every elapsed time of a run (首页, 群, 成员, the live pane, the working card, the inspector 用时) comes from `topic_page::run_elapsed` (run start to its end, or to the provider's `clock_now`). A run still at work has a start, not a range ("今天 07:23 起").
- No free colored dot, colored "!" circle or colored text as the only status carrier. The only free dot allowed: unread dot 8 (info) next to a title, and the presence dot on an avatar.
- In the sidebar the state goes to the right column as a StatusBadge, except 排队: a queued 话题 row has an empty right column (owner decision v17, D16). A 私聊 row has the member-row right column: the state of its 话题 that waits on me, else of its newest 话题 with a state (v17). An unread 话题 or 私聊 shows the unread dot by its title, never a count pill beside the badges (one right-column kind per group, v16). The icon column keeps one kind per group. A busy member row says 等你 n, else 在做 (with its plan count) only while its run works, else the state of its newest 话题.

### 2.13 Empty state

- Full region (page, pane body, list region): flex-1, centered, gap 24, p 24 (p 48 when the region is wider than 640). Optional dashed border (radius 10) when it sits inside a page with other sections.
- Media: 40 icon box (radius 10, muted fill, 24 icon). Title 18 medium. Description 14 muted, max-w 384, balanced lines. Actions gap 8: primary sm plus outline sm; optional muted link "了解更多 ↗" 14.
- Compact (a section in a pane or a card): 16 icon + 14 muted line + optional ghost sm action, left-aligned in the section.
- Never a lone line of text in the middle or at the top of a pane. The brand mascot (57) is replaced by the 40 icon box.

### 2.14 Loading

- Skeleton (muted fill, radius 8, pulse) in the region that will hold the data, with the final shapes: message row = circle 32 + lines h14 at 40% and 70% width; sidebar item = 16 square + h14 bar at 50-90% width, pitch 32; list row = 32 box + lines h14 / h12; table = rows at the real row height; card = 640 x 120 radius 14; inspector section = GroupLabel bar + 3 DescriptionList rows. 3-6 skeleton rows.
- No centered spinner and no "正在加载…" text in an empty region. Older messages loading at the top of a thread: 2 message skeleton rows above the first message, in the list (they scroll with it and never clip it).
- First sync (no 群 yet while it connects): the sidebar skeleton, the 首页 page skeleton (KPI cards and list rows), no create buttons and no 首页 count; the sidebar footer's second line reads "正在连接…".

### 2.15 Error, offline, failed

- Alert in place of the failed content: radius 10, border, px 16, py 12, icon 16, gap-x 12, title 14 medium, description 14 muted, action at the right (outline sm "重试"). Destructive variant: icon and border tint destructive; the title stays foreground (owner decision v14: state color only on icons and badges).
- Page failed state (owner decision v15): the Alert (icon, neutral title, cause, 重试) takes the place of the content, never drawn over pulsing skeletons and never next to them; the page toolbar keeps its place and layout (it never moves or jumps).
- Run log failure: the same Alert in the pane, in place of the end step: the cause once (the time is the 用时 fact). Its actions are the pane footer (2.5, v16): 重试 (primary) over 查看日志 (outline), full width, so the pane ends with them and never in a blank band. The log is top-anchored like a page: a short log may leave whitespace between its last step and the footer (owner decision v17, process-failed).
- Run failure in a thread: an Alert in place of the run card: title states the cause ("打包失败：找不到证书"), description, actions "重试" (primary sm) and "查看日志" (outline sm). The inspector 状态 row shows StatusBadge "失败".
- Failed message: below the body, x = text column: destructive icon 16 + "没有发出" 12 in the foreground color (owner decision v16, D14: the state color on the icon only, like every other error) + ghost sm "重试" (rotate-ccw icon) + ghost sm "⋯" (删除, 复制). Retry is the same button in every place (members detail, files, messages).
- Offline: a warning Alert docked above the composer (title "连接已断开", description "8 秒后重连", outline sm "立即重连"); a page with no composer shows the same Alert as the sidebar row above the footer (2.1, v17); the composer shows "消息会在连上后发出" 12 muted; the sidebar footer second line shows "离线" with a muted dot badge. One word for this state everywhere: page subtitles say "离线" too, never "未连接".
- Dates: never epoch zero. A missing date shows "从未上线" / "未知". A same-day range: "11-01 10:21–14:21".

### 2.16 Toast and banner

- Toast: float surface, radius 10, border, shadow md, p 16, w 360: icon 16, title 14 medium, description 14 muted, an action (outline sm) and a 16 close. Bottom-right, 16 from the window edges; while a composer is on screen (with or without the focus, v16), 16 above its top (`composer::floor_painted`, through `ui::overlay::set_toast_floor`), so a toast never covers Send.
- Pinned banner (the ask or review that waits on me, owner decision v17, D18): Alert anatomy (2.15) docked above the composer, x 288-1096. It gives way while the ask's card is on screen. A 话题-level ask (no card of its own) takes the 话题's newest card that waits on me (its question, asker and time), and never pins while an ask card of that 话题 is on screen (v17, T-10).
- Other in-page banners (evidence notice): Alert anatomy (2.15) at the top of the message column content, x 288-1096. Button labels never truncate.

### 2.17 Thread (message column)

- **Thread-start block** at the top of every thread: 40 icon box, title 18 medium (the topic title), 14 muted line "<creator> 在 <group> 发起 · <date>", then one row of up to 3 outline sm suggested actions for this thread (for example 添加成员, 交代给成员, 附件). 24 below the header. Short threads stay anchored under it at the top, not at the bottom (owner decision, v14): the actions in the start block, not a filler, give the short thread its content. Check: first ink is at most 24 + 16 below y 52 in every topic scene. A thread with no messages shows the start block and a compact empty line ("还没有消息，说点什么开始吧"; 3 message skeleton rows while the first page loads), never a blank column.
- **Sticky date**: once the thread scrolls or opens cut at the top, the day of the first row on screen shows as a pill (h24, float surface, radius full, label 12 medium muted, centered on the column, no full-width fill) 2 under the header line, so no date label is left cut under it. It shows only once that row's own date label has gone under the pill's middle (or the row has none: its day's label is above it), so the same date never shows twice, and a search hit keeps its author row whole.
- **Message row**: avatar 32 at x 288; name 14 medium, role 12 muted, time 12 muted on one line at x 332; body 14/22 at x 332, max width 640. Group gap 16, consecutive messages 4. One message style in every thread (no gray bubbles in side chat).
- **Date divider**: 1 px border rule across the text column with a centered label 12 medium muted on the background (px 8). 24 above, 16 below.
- **Composer**: docked at the bottom, x 288-1096, 16 above the window bottom. Frame radius 10, border, shadow-xs, p 8: textarea 14/20 (1-8 lines), then a row: ghost 28 icon buttons (attach, mention) left, Send button sm (primary, icon arrow-up) right. Reference chips max 2 rows, then "+N". Suggested actions = outline sm buttons in one row, 8 above the composer.
- **Search hit**: the hit message gets a muted band (radius 8) that fades after 2 s; matched words in 500 + underline; a search bar h40 under the header (border-b): query chip, "2 / 7" 12 muted tabular, previous / next icon buttons 28, close 28.
- **Choice options**: a RadioGroup of full-width rows h40, border, radius 8, letter in a 20 box, selected = foreground border + muted fill.
- **Code block**: horizontal scroll inside the card, never a hard clip.
- **File item** (every attachment): a bordered container (radius 10, 1 px border) h60, px 12, gap 12: 32 media box (radius 6, border, muted fill, 16 icon; play for video and audio), name 14 medium cut in the middle so the extension stays (tooltip), meta 12 muted, Progress h4 inside while it transfers. Images: one image at 640, more in 2 columns of 312, gap 16.
- **Pinned banner**: it gives way while the ask's card (the one that holds its question or 批准合并) is on screen; its title is the card's question; an ask with no card offers 回答.
- **Embeds**: when the artifact pane is open, the in-thread embed collapses to one reference row (icon 16, title, "v3" badge, "在右侧打开" 12 muted).

### 2.18 Page recipes (main window)

- **Home (desk)**: toolbar title. Content: KPI row 4-up (等你 4, 在做 3, 今天交付 2, 排队 2; 交付 counts run results, D17), cards w (808 - 48) / 4 = 190, number 24. Then one section per queue (SectionHeader with count pill) holding one framed list (2.6) with fixed columns and a footer row. The "Jojo" summary goes in an Alert (info) above the KPI row or in the first card. Rail per 2.5 without duplicate items.
- **Artifacts**: table view by default (名称 = icon 16 + title 14 medium truncate; 类型 outline badge; 话题; 做的人 avatar 16 + name; 版本; 更新 right-aligned tabular; "⋯"), table footer "3 个产出". Icon view: equal cards in a grid gap 16, a group header per day (今天, 昨天), title + "v3 · 话题" under each tile, tooltip with the full title.
- **Members**: the member list column is not framed (rows on the column surface, selection = muted fill); the detail sits beside it.
- **Member detail**: full pane width, px 24. Header block: avatar 64, name 24 semibold, 14 muted description, actions right-aligned (primary sm "交代", outline sm "单独问", outline sm "暂停"; no trailing "…" in labels). Separator 24. Then cards in a 2-column grid at widths >= 720 (现在在做 | 这周 KPIs; 最近交付 as a table; 问题 as Alert with Retry). Every section in every state (compact empty when no data). 24 bottom padding.
- **Group**: toolbar title + primary sm "新话题" at the right (no top composer capsule). Topic list = ListRows (2.6, owner decision v15), paginated, footer "看全部 23 个话题"; open 话题 in sections, in this order: 等你 (asks to me), 在做 (runs), 卡住了 (failed or stuck), 等别人 (waits on another member's answer or review), 排队 (in progress with no run); never a waiting row under 在做 (owner decision v16). 在做 always shows (compact empty when no run); the other sections show only with rows. Each row's media is the member's avatar (§2.6, D15), its badge the `topic_state` words (§2.12). Inspector per 2.5 with sections 群里的决定, 成员, 机器, each with a compact empty.

### 2.19 Settings window (640 wide)

- Height fits the tab, at most 760 (v17); a taller tab scrolls under the toolbar.

- Toolbar tab strip unchanged; tab icons share an 18 x 18 ink box (the 机器 icon is 22 x 10 today).
- Content x 24-616. First element 24 below the toolbar border.
- A tab is a stack of sections: SectionHeader (2.3) then a GroupBox (2.8). Gap between sections 24. Every group has a header, except when it is the only group on the tab, and except the 账号 profile card (owner decision v16, C1-11), which opens the tab with no section header, like the macOS Apple Account header. Section actions ("添加电脑…", "邀请成员…") sit in the section header, not under the group.
- Footnote text under a group: 12 muted, 8 below the group. Prefer the section description.
- Rows: single-line h44, two-line h64. Separators in every list. Row actions are visible in the row (ghost 28 "⋯" with a DropdownMenu), never select-then-"−".
- Machine and device rows: 28 icon box (radius 6, muted, 16 icon: laptop for a Mac laptop, the outline display for a desktop Mac (v17), server for Linux, cloud for 云端), title 14 medium, description 12 muted truncating, StatusBadge in the value column, "⋯".
- Developer view: DescriptionList with localized labels (连接, 服务器, 成员 ID), monospace 13 values, copy icon button 28.
- Every list has a loading (2 skeleton rows at the row height), empty (compact or full Empty with the add action) and failed (Alert + Retry) state.

### 2.20 Login (full window)

- No card (owner review v18): the window (480 x 480) is the surface. One centered column as wide as the code slots (376): app icon 56; 16; title 18 semibold (the scale has no 20); 4; one 14 muted line; 24; the invite code as a one-time-code input (`ui::native::CodeInput`, shadcn InputOTP): 8 bordered slots 40 x 44, radius 8, gap 6, in two groups of 4 with a dash between (group gap 20); characters upper case, centered, mono with tabular figures; the slot at the caret has a foreground border and a 3 px foreground ring at 15 %; a paste fills every slot, Backspace moves back, the arrow keys and a click move the caret, a character typed over a filled slot replaces it. 8; the error slot (16, kept): a destructive icon and neutral 12 text, centered (2.8), and every slot gets the destructive border; a refused code stays in the slots, selected, so the next character starts a new code. 16; primary lg h36 full width: primary at 30 % until all 8 slots are filled (`waiting_push`), never a mid gray; Enter submits; loading: spinner + "正在验证…". 12; a small ghost link "使用其他服务器…" centered, or a custom server's host (cut in the middle) with "更改". A pasted login credential or a legacy 12-character code shows as one locked line across the slots. No control at the window corners.
- Server step: the same column, title "连接服务器", one-sentence description, an h36 field across the column, "取消" in the link row.

### 2.21 Density and fill rules

- No region taller than 24% of its pane is blank unless it is a designed Empty state, or it is the space under a top-anchored short thread whose start block shows its suggested actions (2.17, A1). On pages, space below the last frame does not count (A1); a frame never stretches to fill the pane.
- Scroll content ends with 24 bottom padding above any pinned footer; nothing is cut by the window edge or a footer.
- No duplicate data on one screen (desk 正在做 in main and rail; artifact embed and artifact pane).
- Truncation: every single-line label truncates at the end with "…" and shows the full value in a tooltip; pop-up buttons keep the chevron; lower-priority text truncates first (role, then meta, then name). Never clip at the start or mid-glyph; never wrap a value mid-phrase in a DescriptionList.
- Numbers: tabular, thousands separators (1,000,000), right-aligned in columns.

### 2.22 Dark parity

- Every line, track, separator and frame that exists in light exists in dark at >= 1.5:1 against its surface (the dark border token is raised to meet this on the background and on the card fill, 1.2) (check: the transition count on one scan line is equal in light and dark).
- A value fill is higher-contrast than its track in both themes (Progress and chart bars use primary on primary 20%).
- Switch ON in dark: track #e5e5e5, knob #171717 (today #ffffff on #f7f7fa, 1.03:1).
- Avatar fallback in dark: muted fill #262626 + border white 10% (today #2c2c30 on #2b2b2b, 1.0:1).
- Artifact HTML: map its border token to the border token in dark (today 0 of 11 lines survive).

---

## 3. Fix list (ranked)

Rank order: foundation first (each screen fix depends on it), then what makes a screen look unfinished: (1) panes with no purpose, (2) no shared header and pane anatomy, (3) undesigned states, (4) inconsistent elements, (5) alignment and polish. "Rule" points to section 1 or 2.

### 3.1 Foundation (land first)

**K01. rem scale.** All gpui-component parts. Problem: `theme.font_size = 13` sets rem 13 (`root.rs:714`), so menu text is 11.375 (ink 10 vs 11 in the sidebar), PopupMenu rows 26, Notification text 11.4. Fix: rem 16 and root text 14 (D1); re-check every gpui part at its shadcn size. Rule: D1, 1.4.

**K02. One token module, one color struct.** Problem: two token sets disagree (DIALOG_WIDTH 440 vs SHEET_WIDTH 460, ROW_HEIGHT 32 vs LIST_ROW 28, TAG 18 vs BADGE 20 vs CHIP 30, radius MD 8 vs LIST_ROW_RADIUS 6 vs SIDEBAR_ROW_RADIUS 7); 3 color structs; ThemeColor written in 3 passes; views hold 1408 `px(` literals (16 radii, 10 text sizes, 14 gaps). Fix: 1.1-1.7; a ratchet test on literal counts. Rule: 1.1.

**K03. One Button.** Scenes: settings-general, settings-machines, members-v2, new-group, rename-topic, empty, desk, files. Problem: 5 shapes (24 r~5, 22 square, 20, 25, 28 capsule r~14, 30), 3 code paths, 23 raw `Button::new`. Fix: `push_button` at 28/32/36, radius 8, variants per 1.8; delete the controls.rs family. Rule: 1.8, 1.9.

**K04. One field family.** Scenes: main, new-group, rename-topic, composer. Problem: 4 shapes (sidebar search 30 capsule, sheet search 24 capsule, name field 28 boxed inside a 36 fill row, composer 36 capsule), 3 paths. Fix: Input h32 / sm h28 radius 8; SearchField h32 full; composer per 2.17; delete `controls::TextField`. Rule: 1.8, 2.8.

**K05. One menu renderer.** Scenes: rail-topic-menu, group-menu, mention popup. Problem: 3 renderers; rows 26 / 24 / 40; topic menu text at x 156 (check column) and group menu at x 144. Fix: 2.11. Rule: 2.11.

**K06. One container, surface, separator, header and row set.** Problem: 6 group containers, 6 surfaces, 5 separators, 6 section headers, 8 row kinds. Fix: Card, GroupBox, right-pane section; float surface; `hairline`; SectionHeader + GroupLabel; ListRow, FormRow, menu item, table row. Rule: 1.9, 2.3-2.8.

**K07. One modal path.** Problem: 4 paths, widths 440/460/560/320/260 + gpui default 448. Fix: `native_sheet` 460/560 and `native_alert` 260 only. Rule: 2.9, 2.10.

**K08. One feedback system.** Problem: overlay Toast (44) / Banner (52), gpui Notification (382, text 11.4), connection_banner, banners. Fix: Toast + Alert. Rule: 2.15, 2.16.

**K09. One tabs control.** Problem: Segmented, `rows::tab()` (11 sites), members segmented measures 37 with the glass edge. Fix: Segmented h32. Rule: 1.8.

**K10. Adopt the missing components.** Problem: Table, DescriptionList, GroupBox, Skeleton, Progress have 0 uses; EmptyState 1 use. Fix: wrappers per 1.9 and use them in every fix below. Rule: 1.9.

**K11. Badge and avatar components.** Problem: 5 own badge kinds + 3 gpui; avatars in 6 sizes (16, 20, 28, 32, 57 + inspector 21) and 2 fallback styles (black "J" vs gray). Fix: Badge / StatusBadge (2.12), Avatar (1.8). Rule: 1.8, 2.12.

### 3.2 Screen fixes

**F01. Topic inspector is 2 loose rows.** Scenes: main, offline, window-inactive, long, topic-worst, cards-v2-topic, cards, refs-card-chart/metric/hover/markdown/sent/composer (light, dark, worst). Problem: "负责 Joye" and "提出 小王 · 昨天 08:21" then nothing: last ink y 82-83 of 900 (91% void); no header; value columns at x 1166 (left) and 1351-1422 (right-aligned) in one section; in `cards` one centered line with 430 px voids above and below; worst truncates the owner to "Sa…" while the role keeps 200 px. Fix: right-pane anatomy with the fixed topic sections (2.5), DescriptionList at one value x, compact empty per section, pinned footer. Template: refs-card-step / topic-v2. Rule: 2.5, 2.21.

**F02. No shared header line.** Scenes: all topic scenes, desk, group-v2, files, artifact-v2, artifact-pane-v2. Problem: 2-line title with no border-b; 36 glass circle buttons with a shadow band y 43-53; no luminance step at y 52 for x 700 or 1280; right-pane first ink at y 24 / 59 / 23 / 26; messages fade under the header so a half message sits at the top. Fix: 2.2. Rule: 2.2, D4.

**F03. Blank band above short threads.** Scenes: main (first ink 507, gap 455), topic-dm (645), topic-short (345), topic-worst (298), main-worst (383), cards-v2-topic (405), artifact-pane-v2 (265), live (357). Fix: thread-start block and top anchoring (2.17). Rule: 2.17, 2.21.

**F04. Sidebar ends in the middle.** Scenes: main (last ink 699, void 191), topic-core (262, void 628), empty (170, void 720), rail-member (582, void 308); footer only in offline. Fix: groups, `mt-auto` secondary items, pinned user footer h48 that also holds the offline state (2.1). Rule: 2.1.

**F05. Home is loose rows with a duplicate rail.** Scenes: desk, desk-answered, rail-work, rail-member (light, dark, worst). Problem: no frame, header or footer; the 3 "正在做" items repeat in the rail (main y 399-527, rail y 84-244) with two designs (dot + 3/5 vs bar); main void 685-900; rail last ink 333 (63% empty); the "Jojo" summary is a loose line. Fix: Home recipe (2.18). Rule: 2.18, 2.6, 2.21.

**F06. Empty Home is one line.** Scene: desk-empty (light, dark). Problem: one 13 px muted line at y 460-472 in a 1170 x 840 pane; the subtitle switches from a count to a date. Fix: Empty state (40 check icon box, "都处理完了", description of what Home shows, primary sm "开个话题" + outline sm "看产出"); keep the KPI row and the 正在做 / 做完了 sections when they have data; keep the count subtitle ("0 件等你"). Rule: 2.13, 2.18.

**F07. Artifacts grid stops at y 230.** Scene: artifacts-v2 (all). Problem: 3 tiles 136 x 84, 74% of the pane blank; no count, grouping or filter; in worst data 3 identical truncated titles. Fix: Artifacts recipe (table default, grouped icon view, tooltip, version + topic under the title). Rule: 2.7, 2.18.

**F08. Member detail is an island and clips.** Scene: members-v2 (all). Problem: 600 px column (x 712-1311) with 128 px margins in an 855 pane; light data cut at y 899 ("需要你批准的事"); worst stops at y 506 (394 void) and drops 3 sections; buttons h25 and "交代…" reads as truncated; 重试 h20. Fix: Member detail recipe (2.18). Rule: 2.18, 2.4, 2.21.

**F09. Process state screens are identical to their base.** Scenes: process-dev, stop-stopping (light, dark, worst). Problem: 0 changed pixels against process and stop; no stop control in the header or run card; no confirm, no stopping state, no dev view. Fix: Stop = ghost sm in the run card footer + destructive Alert confirm; Stopping = the same button in loading state ("正在停止…", disabled) + StatusBadge "停止中"; Dev view = a log panel (header h40 with title and copy, mono 13). Add a capture check that each state differs from its base in the state region. Not confirmed: the cause may be a capture-script gap; either way no designed state is visible. Rule: 2.4 working card, 2.10, 2.15.

**F10. Failed run has no error design.** Scene: process-failed (all). Problem: only red "卡住了" text in the subtitle; no card, cause or Retry; inspector "今天 11-01–11-01". Fix: run-failure Alert (2.15), StatusBadge "失败" in the header and inspector, same-day range format. Rule: 2.15, 2.12.

**F11. Login controls at the window corners.** Scenes: login, login-code, login-error, login-server, login-custom-server. Problem: 205 px form in 1440 x 900; "继续" at x 1356-1420, y 852-880 and "使用其他服务器" at x 20-132 (about 1000 px from the field); no label; field width 205 then 325; hint ink 10 px; no loading state. Fix: login card (2.20). Rule: 2.20.

**F12. Dark switch ON is invisible.** Scenes: settings-notifications-dark, settings-general-dark, settings-members-end-dark. Problem: knob #ffffff on track #f7f7fa (1.03:1). Fix: 1.8 switch, 2.22. Rule: 2.22.

**F13. Dark letter avatars are invisible.** Scene: settings-members-dark (also light: #e9e9ec on #f5f5f5). Problem: #2c2c30 on #2b2b2b (1.0:1). Fix: Avatar fallback with border (1.8). Rule: 1.8, 2.22.

**F14. Dark charts and artifact lines.** Scenes: interact_*-dark, cards-v2-data-dark, media-html-dark. Problem: bar value (52,52,58) darker than track (64,64,64), 1.07:1 on the card; HTML artifact light has 2/6/3 line transitions on the scan, dark 0/0/0, so options lose their boxes. Fix: D12 for bars; artifact border token mapped in dark. Rule: 2.22.

**F15. Search hit is not marked.** Scene: search-hit (light, dark). Problem: no highlight, no n/m, no way back; the top message is cut under the header; "回到最新" covers the last item. Fix: 2.17 search hit; "回到最新" pill sits 16 above the composer and does not overlap content (reserve space). Rule: 2.17.

**F16. Settings 账号 device list is a placeholder.** Scene: settings 账号 (all, worst with 12 devices). Problem: no separators (one fill run y 247-351), pitch 32, a 22 px "−" under the group plus a help sentence at x 55; no count, no scroll frame, no "这台 Mac". Fix: section "登录的设备 · 12" + description; rows h44 with icon box, name, meta 12 right, "这台 Mac" outline badge, "⋯" → "退出登录"; skeleton and Alert states; delete "−" and the help line. Rule: 2.19, 2.6.

**F17. Settings 机器 list and cloud row.** Scene: settings-machines (all). Problem: no description; free dot + text status; +/− h22 with help at x 77 (others x 23); no row action; 云端 row "还没有开通" with no button; Linux server uses the cloud icon; worst "1970年1月1日在线"; orange "异常" with no cause. Fix: 2.19 machine rows, header action "添加电脑…", 云端 trailing "开通…" sm, Empty / skeleton / Alert, "从未上线". Rule: 2.19, 2.12, 2.15.

**F18. Three member pickers.** Scenes: new-group, add-members, start-dm (all). Problem: search h24 / h31 / full width 484; checkbox column vs black filled row; list boxes 232 / 231 / 263; 7 of 12 rows with no scroll cue; green, red, black/white dots with no legend. Fix: one picker (2.9). Rule: 2.9, D10.

**F19. In-thread cards: 9 widths, no border, no anatomy.** Scenes: topic, topic-v2, topic-core, topic-short, cards, cards-v2-*, interact_*, compose-choice*, media-states, refs-card-chart, process, stop. Problem: widths 300, 380, 420, 480, 560, 600, 620, 640, 680, 720 (refs-card-chart right edges 888 / 948 / 1008); fill #f2f2f2 at 1.12:1 with no border; padding 14 / 16; the run card is title + empty 20 px band + meta; code clips at the edge. Fix: in-thread card, card header slots, working card (2.4); code scroll (2.17). Rule: 2.4.

**F20. KPI drawn 5 ways.** Scenes: cards-v2-data, refs-card-chart, refs-card-metric, media-html, web-embed. Problem: digit ink 13 / 20 / 12 / 12 / 13; unboxed, hero, mono tiles, number-first tiles; in the data card the number is smaller than the title. Fix: KPI block (2.4). Rule: 2.4, 1.4.

**F21. Tables do not fill their frames.** Scenes: refs-hover, refs-markdown, refs-sent, refs-composer, refs-side-chat, cards-v2-data(-worst). Problem: card table last column ends x 694 of 934 (39% blank); markdown table ends x 588 of 1008 (62% blank); no frame or header fill; total "100…" truncated with free space; deleted row is plain text. Fix: 2.7, tombstone. Rule: 2.7.

**F22. Failed message and offline state.** Scenes: main, topic, offline, main-worst. Problem: red 11 px label and plain-text "重试" (ink 10, no frame); offline only an 11 px sidebar footer line. Fix: 2.15 failed message and offline. Rule: 2.15.

**F23. Conversation scenes use 4 shells.** Scenes: diagram-*, web-embed*, media-*, thread scenes. Problem: thread header (title ink 13) vs breadcrumb (14, border) vs plain title (16) vs page title in the body (x 124); avatars 28 / 28 / 36 / none; composer in 1 of 4. Fix: one shell (2.1, 2.2, 2.17); viewers open as a sheet or the right pane inside it. Rule: 2.1, 2.2.

**F24. Side chat pane.** Scene: side-chat, refs-side-chat (all). Problem: width 380; void y 75-380 (side-chat) and y 67-760 (refs-side-chat, 77%); lock notice as a loose line; gray bubbles only here; quick action a 24 gray chip with an 11 px hint. Fix: 2.5 side chat, 2.17 suggested actions. Rule: 2.5, D13.

**F25. Live pane.** Scenes: live, live-takeover (all). Problem: thread void 357; plan shown twice with a 5 px blue bar and a 3 px segmented bar; right-pane plan card cut by the footer at y 852; acceptance status as blue dot + blue text; worst rows wrap to 4 lines. Fix: plan once in the right pane, a one-line summary card in the thread; Progress steps variant; pane body scroll with 24 bottom padding; StatusBadge; acceptance title line-clamp 2, value column 160 right-aligned. Rule: 2.5, D12, 2.12.

**F26. Toolbar rows mix heights.** Scenes: artifacts-v2 (segmented 32, popup 24, search 24), members-v2 (segmented 37, add button 47), artifact-v2 (popup 24 next to 36 buttons). Fix: all h32 on one center line (2.2). Rule: 2.2.

**F27. List columns and row alignment.** Scenes: desk, desk-answered, group-v2, rail-work, rail-member. Problem: desk time x 934-964, group-v2 time x 1050-1080; action-less rows leave a 180 px column; "看看" at x 1016 / 1027 / 1101; time on the title line (y 135) but buttons centered (y 142); buttons h20-21; blue dot + "3/5" at x 1098-1142. Fix: ListRow right cluster (2.6). Rule: 2.6.

**F28. Message column has 8 left edges.** Scenes: topic scenes, rail scenes, side-chat. Problem: x 280, 281, 283, 285, 288, 297, 324, 328. Fix: x 288 and x 332 only; composer "+" inside the composer as a 28 ghost button. Rule: 2.1, 2.17.

**F29. Secondary text too small, too many grays, flat hierarchy.** Scenes: all main-window and page scenes. Problem: subtitle, inspector labels, date, composer hint, offline footer at CJK ink 9-10 (11 px); 7 muted grays at 3.18-4.48:1; toolbar title 14-16 ink vs row titles 13-14; section counts as plain muted text. Fix: 1.2, 1.4; counts as count pills. Rule: 1.2, 1.4.

**F30. Status by color only.** Scenes: sidebar rows (orange "!" at x 27-39), header subtitles, desk / group / members lists (blue dots at x 1098-1114), members "等你审" orange text, artifact-pane "完成" green text, cards-v2 / interact status text, blue provenance fill, reaction chip blue outline for "mine". Fix: StatusBadge (2.12); provenance as a 12 muted label or border-l; "mine" reaction = muted fill + foreground border. Rule: 2.12, 1.3.

**F31. No loading design anywhere.** Scenes: topic-core ("正在加载…" at x 666-718 over a faded card), all lists (0 Skeleton uses). Fix: 2.14 per region. Rule: 2.14.

**F32. Two empty designs, neither finished.** Scenes: topic-dm inspector (15 px icon + one 12 line mid-pane), empty (57 mascot, 24 buttons, sidebar 3 items, no right pane). Fix: Empty (2.13): full in the no-groups page with primary + outline sm; compact under the inspector header with "指派". Rule: 2.13.

**F33. Sidebar member view.** Scene: rail-member (light, worst). Problem: no label for the agent group (no label between y 164 and 196); right column mixes text ("等你", "运营实习") and counts; collapsed rows look like placeholders; worst role text runs to x 256 and is cut mid-glyph. Fix: GroupLabels "AI 成员" / "同事", right column only count or badge, "More" item, truncate before the right column. Rule: 2.1, 2.21.

**F34. Group page.** Scene: group-v2 (all). Problem: 7 identical Forge avatars; a 42 px capsule composer at the top with an 11 px hint; list ends at y 572 (328 void); worst decisions one muted line, members truncate with a stray "." at x 1420, machine name without tooltip. Fix: Group recipe (2.18), media = topic-status icon box, compact empties, tooltips. Rule: 2.18, 2.6.

**F35. Files thread and inspector.** Scene: files (all). Problem: attachment widths 620 / 560 / 620 / image 360; failed card "预览失败 · … · 0 B" with an h20 "打开"; progress bars at two insets; inspector row 1 avatar+name left, row 2 label left / value right at x 1423; last ink y 147 of 880. Fix: attachment = ListRow-style item at the in-thread card width, p 12, 32 media, Progress under the meta at the same inset; failure as an inline Alert row with Retry; hide "0 B"; inspector per F01. Rule: 2.4, 2.5, 2.15.

**F36. Artifact pane: duplicate content, two header styles.** Scene: artifact-pane-v2 (all). Problem: embed (x 328-887) and full page side by side; left header green "完成" text, right header "网页 · v3" muted; worst URL and reaction chips wrap to 2-3 lines; void y 55-320. Fix: collapse the embed (2.17), StatusBadge in both headers, h52 headers (2.2), thread-start (F03), truncation. Rule: 2.17, 2.2, 2.21.

**F37. Artifact page bottom.** Scene: artifact-v2 (all). Problem: decision block cut at y 837 by the footer strip (y 848-870); the footer is 11-12 px muted text with an info icon; worst "做完 / 了 09:40" wraps; versions without a note show only version + date. Fix: 24 bottom padding above the footer; footer as card footer (border-t, 14 muted) or move to the inspector; one-line values with tooltip; "无说明" muted. Rule: 2.21, 2.4.

**F38. Settings 成员.** Scenes: settings-members, settings-members-end (all). Problem: owner "管理员" text at x 570 while select labels start at x 512; no filter; no pending invitations; removed member is a faded copy with no badge or action; worst no-role member shows only "在线"; add buttons float under the list. Fix: section "成员 · 13" with filter input h32 and header actions (primary "邀请成员…", outline "添加 AI 成员…"); owner = disabled select or "负责人" badge at the value edge; section "已停用 · 1" collapsed with badge and "⋯" (恢复, 移除); "未填写岗位" muted; skeleton / Alert. Rule: 2.19, 2.8, 2.12.

**F39. Settings 通用 and About.** Scenes: settings-general, settings-about, settings-about-dev. Problem: 3 groups and 0 headers; "当前版本 1.0.1 / 自动检查新版本" row with no control; server value with no copy; settings-about renders General; dev view rows titled "connection: online" in English; "发送诊断…" floats at the bottom left (h23). Fix: sections 启动, 提醒, 版本与服务器 ("检查更新" sm + auto-update switch row; server + copy), 开发者 (switch, DescriptionList, "发送诊断…" in its header). Not confirmed: settings-about may be a capture-script mistake; check before building a separate About surface. Rule: 2.19.

**F40. Settings spend limit has no field.** Scene: settings-members-end (all). Problem: "¥ 50" on #f6f6f6 inside a #f5f5f5 row, no border; worst "1000000" without separators; help under the group; "谁来批准" select wider than the role selects. Fix: number field (2.8), help as the section description, one select width. Rule: 2.8.

**F41. Sheet shell: padding and edges.** Scenes: new-topic, rename-group, rename-topic, new-group, add-members, start-dm, update*. Problem: padding left 40, right 36, top ~24 (to title box), bottom 20; buttons end 4 px past the content (913 vs 909, 963 vs 959); textarea ring 3 px outside the group; new-group name field ends 948 while search ends 959; light sheets have no border. Fix: 2.9, 1.7. Rule: 2.9.

**F42. Sheet label columns.** Scenes: new-topic, rename-group, rename-topic, new-group. Problem: fields at x 617 / 615 / 595 / 665 (label columns 86 / 84 / 64 / 184); new-topic mixes inline labels (x 544) with a label above the textarea (x 534); new-group "名称" inside the group (x 493) and "成员" outside (x 482). Fix: label column 112 in every sheet, textarea as a full-width row (2.8). Rule: 2.8, D8.

**F43. Sheet headers and primary state.** Scenes: settings-add-bot(-new), settings-invite, settings-add-machine, update-dl, all sheets. Problem: add-bot and invite have no description; "添加", "生成邀请码", "生成安装命令" enabled with an empty name; the name field sits in a filled row with a 2 px ring box (158-194); add-bot help line under the wrong row; avatar grid 9 + 9 + 8. Fix: 2.9 header, disabled primary, inline field, help as the row description, avatar grid. Rule: 2.9, 2.8.

**F44. Disabled primary looks like Cancel.** Scenes: all sheets, login. Problem: disabled "开始" #f6f6f6 / #f3f3f3 next to "取消" #f2f2f2 / #f5f5f5. Fix: Cancel outline, disabled primary at 50% (D11). Rule: D11, 1.8.

**F45. Edit-profile sheet uses its own form.** Scenes: settings-edit-profile, settings-profile-error (all). Problem: centered right-aligned labels, bare 222 px fields (x 228-450) in a 458 sheet; "更换头像…" h20; worst values truncate with space on both sides. Fix: header row avatar 64 + outline sm "更换头像…", then the grouped FormRows (2.8); error turns the label red too. Rule: 2.8, D8.

**F46. Command palette rows.** Scenes: search, switcher, switcher-query, ask, ask-empty (all). Problem: the ↩ glyph (x 988-1002) moves the selected time to x 980 vs 998-1000; inverted black selection; worst second row has no time; mixed leading glyphs ("#", blue dot, orange "!", check, avatars); no footer; no empty, loading or error state. Fix: palette (2.11). Rule: 2.11, D10.

**F47. Palette cuts its last row.** Scenes: switcher, switcher-worst. Problem: panel ends at y 658 through the last avatar (ink 639-657). Fix: whole-row height, 8.5-row max, footer (2.11). Rule: 2.11.

**F48. Text clipped at the start.** Scenes: new-topic-worst ("品设计与前端…", first glyph cut), ask-worst (chip "enachrichtigungseinstellun" clipped both ends, x 848-1007), search-worst (scope chip 320 wide pushes the query to x 800). Fix: pop-up max 220 and chips max 200 with end truncation and tooltips (2.11, 2.21). Rule: 2.21.

**F49. Update flow.** Scenes: update, update-dl, update-manual (light, dark). Problem: a 460 sheet with a 40 px empty band (description bottom 118, buttons top 158); no notes or "from → to"; "42%" at 8 px ink; only "隐藏"; three heights 154 / 146 / 190. Fix: alert anatomy (2.10): "TeamChat 0.2.0 可以更新了", "当前 0.1.9 · 更新时会重启", link "查看更新内容"; downloading: Progress + "42% · 18/43 MB" 12 muted tabular, "取消下载" + "在后台下载". Rule: 2.10, D12.

**F50. Card header and footer kinds.** Scenes: cards, cards-v2-*, interact_*, media-html, web-embed. Problem: 4 header kinds (first-row y 131 / 123 / 122 / strip 161-197); footers with or without hints; interact_pending "没有回应" puts "重试 / 不等了" above the description. Fix: card header slots and footer (2.4). Rule: 2.4.

**F51. In-card control heights.** Scenes: cards-v2-evidence, media-states, cards-v2-form, cards-v2-approve, compose-choice, refs-card-step, interact_conflict_toast. Problem: buttons 18 / 20 / 24 / 30; inputs 24 and 31 (4 px focus border); NPS 32; "要求修改…" truncated; 2 inputs with no label or placeholder. Fix: sm 28 for all in-card controls, focus ring outside, buttons sized to label, every input labeled. Not confirmed: the 18 button is inside a scaled preview. Rule: 1.8.

**F52. Media viewer.** Scenes: media-audio, media-code, media-html, media-pdf, media-table, media-video, media-images. Problem: floating 600 x 780 panel (x 736-1336) outside the shell; voids 272 (audio), 302 (code), 503 (html rail); header actions differ per type; image viewer has a double frame. Fix: open as a 560 sheet or in the right pane; header 16 icon + title 16 semibold + meta 12 + ghost 32 actions in a fixed order (copy, download, open, close); footer border-t; body fills; short rail collapses; one frame. Rule: 2.9, 2.21.

**F53. Toast is a bare box.** Scene: interact_conflict_toast (light, dark). Problem: white box ~42 tall with one 12 px line, no icon, title, action or close. Fix: Toast (2.16) with action "看新版". Not confirmed: toast size is approximate. Rule: 2.16.

**F54. Web-embed menu and side panel.** Scenes: web-embed-covered, web-embed, web-embed-scrolled. Problem: menu surface shows the subtitle through the "打开" row; side panel ends with a loose orange progress bar at y 510 then a 180 px blank card; orange accent and 36 avatars. Fix: opaque float surface; side panel per 2.5; Progress per D12; no orange; avatars 32. Not confirmed: translucency is visual only. Rule: 2.11, 1.3, 2.5.

**F55. Settings per-group notification sheet.** (Removed: the per-群 level moved to the 群's context menu, NATIVE.md §7.3.) Scene: settings-notify-groups (all). Problem: groups at x 78 (no icon) and people at x 106 (avatar); column headers ink 9; no sections, search or empty state; only "完成". Fix: sections 群 (24 icon box) and 私聊 (avatar 24), one media column; column headers 12 medium muted; filter when more than 8 rows; Empty; keep 560 width. Rule: 2.9, 2.6.

### 3.3 Polish fixes

**F56. Choice options and mention popup.** Scenes: topic (choice card, mention popup), topic-short. Problem: options are text-hugging chips (x 343-486 / 343-489 / 343-475, h24); popup selects with a black row (x 711-1019), anchored at x 705 over the working card. Fix: RadioGroup rows (2.17); mention popup (2.11). Rule: 2.17, 2.11, D10.

**F57. Date divider has no rule.** Scenes: main, topic, topic-worst. Problem: ink h10 at y 507-516, no line at x 300-640. Fix: 2.17 date divider. Rule: 2.17.

**F58. Error grows the sheet.** Scenes: rename-group-error, login-error. Problem: sheet bottom 242 → 262; error ink at x 533 outside the group (label x 544); login error under an unlabeled field. Fix: error in the footer left slot or as a dependent row; fixed height (2.8). Rule: 2.8, 2.9.

**F59. Destructive alerts.** Scenes: settings-device-remove, settings-sign-out, leave-group (all). Problem: black filled "取消" and red text on gray (#f2f2f2) for the destructive action; the checkbox label wraps to 2 lines. Fix: 2.10. Rule: 2.10.

**F60. Settings 通知.** Scene: settings-notifications (all). Problem: 每日摘要 has a time select (x 493-565) and a switch (x 576-607) in one row; 勿扰 group has no header; "按群设置…" floats at the bottom left. Fix: dependent "时间" row; sections 提醒我 and 勿扰 with descriptions; navigation row "按群设置 · 8 个群 ›" in a section 按群 (later removed: per-群 levels live in the 群's context menu). Rule: 2.8, 2.19.

**F61. Settings 快捷键.** Scene: settings-shortcuts (light, dark). Problem: pitch 31 (separators 169 / 200 / 231) vs 37 elsewhere; keys as gray text. Fix: rows h44, Kbd caps, filter input. Rule: 2.19, 1.8.

**F62. Settings 外观 is a stub.** Scene: settings-appearance. Problem: 265 tall window, one group; row label "外观" repeats the tab; slider without a value or preview. Fix: sections 主题 (3 thumbnails with labels) and 文字 (slider with "15 px" value and a preview message line), header + description each. Rule: 2.19.

**F63. Settings spacing and icons.** Scenes: all Settings tabs. Problem: gutter 20 (groups x 20-620); group gaps 20-22; first element 22-23 below y 95; two border colors (#e9e9e9 toolbar, #dfdfdf separators); groups without a frame; 机器 icon 22 x 10. Fix: 2.19, 1.2, 1.7. Rule: 2.19.

**F64. Diagram blocks.** Scenes: diagram-error, diagram-inline, diagram-viewer, refs-diagram-viewer, diagram-inline-worst. Problem: failures in 2-3 styles (red border vs neutral); a 150-node chart at ~4 px text; "展开" floats over a fade; viewer at 250% with no fit hint; "问这里" floats. Fix: one Alert per failure (destructive only when the user can fix the input) with "查看源码"; oversized = fit-to-width preview with a 12 px minimum text and an outline sm "打开查看器". Rule: 2.15, 1.8.

**F65. Long-text fixtures and truncation priority.** Scenes: compose-choice-worst, cards-light-worst, refs-composer-worst, media-states-worst. Problem: "bartholomew.fitzgerald…" stops ~30 px before the badge; name truncates before the role; media-states and option fixtures do not change in worst; composer chips wrap to 3 rows. Fix: truncation order (2.21); add long file names, long options and many attachments to the worst fixtures; chips max 2 rows + "+N". Rule: 2.21.

**F66. Gallery pages.** Scenes: cards-v2*, compose-choice*, interact_*, media-states. Problem: columns 600 and 560 (ends x 1296, not 1336); 3 x 380 ends x 1292; voids 218-495. Fix: 2 equal columns (gap 16) or one column at card width, same right edge; start a new section instead of a void. Rule: 2.4.

**F67. Caption contrast on card fill.** Scene: cards-v2-* light. Problem: (120-124) on (242) = 3.7-3.94:1. Fix: muted-foreground only, cards on card fill with a border (1.2, 1.7). Rule: 1.2.

**F68. Button and palette text sizes.** Scenes: new-topic, rename-*, new-group, add-members, start-dm, live, login, search. Problem: 2 CJK glyphs measure 20-21 px in buttons (11 px text) vs 24 in rows; palette query ink 23 (24-26 px); palette group headings ink 10. Fix: button 14 medium, palette input 16, group label 12 medium (1.4). Rule: 1.4.

fixCount: 11 foundation + 68 screen and polish fixes = **79**.

---

## 4. Completeness checklist

Run every check on the light, dark and worst-case 1x PNG of a scene before the scene is called done. Scripts: `audit-pages/m2.py` (ink runs against a sampled background), `main-window/m*.py` (pane voids, edges), `overlays/diff.py` (state differs from base), `settings/ink.py` (ink box), with `shadcn-bar/venv/bin/python -I`. Record each value in the PR. A check marked [visual] is a reviewer check.

### A. Fill
- A1. Per pane, the largest blank vertical band is < 24% of the pane height (216 at 900), or it holds a designed Empty state. Exception (owner decision, v14): a short thread stays top-anchored, so the band between its last message and the composer may be taller when the thread-start block (2.17) shows its suggested actions. On pages (首页, 群, 成员, 产出) a void is only a gap inside a frame or between the header and the content; space below the last frame is allowed, and frames hug their content (rows + footer row) instead of stretching to the window bottom (owner decision, v14).
- A2. Topic scenes: first ink below the header is at most 40 px below y 52.
- A3. Sidebar: the footer user row is present; last ink >= y 840.
- A4. Scroll content ends with >= 24 px before any pinned footer or the window edge; no block is cut.
- A5. No item appears twice on one screen.

### B. Structure
- B1. A luminance step (border token) at y 52 at x 700 and x 1280 (both panes open).
- B2. Right pane: width 320 (400 for the live and artifact panes, D13); first content ink at y 68 +- 2 in every right-pane scene.
- B3. Message column: every block starts at x 288 or x 332 (+- 1).
- B4. Every card has a header (title + description or action); in-thread cards in one screen share one width.
- B5. Every list or table sits in a frame or card and has a footer or a count.
- B6. Every Settings group has a section header (unless it is the only group); first element 24 below the toolbar border; gutters 24; group gaps 24.
- B7. Sheets: padding 24 +- 2 on all 4 sides; all body blocks share one left x and one right x (0 px deviation); primary right edge = content right edge.

### C. Scale
- C1. Every text size is in {12, 14, 16, 18, 24, 30} (13 mono). No CJK ink of 9 px or less.
- C2. Every layout gap is in {2, 4, 6, 8, 12, 16, 24, 32, 48}.
- C3. Every control height is 28, 32 or 36. All controls in one row share one height and one center line.
- C4. Every radius is in {4, 6, 8, 10, 14, full} (chrome 18 / 22).
- C5. Avatars are 16, 24, 32, 40 or 64; one fallback style.
- C6. Row pitch per kind: sidebar 32, menu and palette 32, form row 44 / 64, list row 56, table 48 / 40 (header 40).

### D. Color
- D1. One muted text color: sampled darkest ink of every secondary text is #737373 (light) / #a1a1a1 (dark) +- 3.
- D2. One border color: every 1 px line samples as the border token +- 3.
- D3. State colors appear only on badge icons, alerts, destructive buttons, invalid fields, the unread dot and presence dots. No colored text run longer than its icon; no blue progress.
- D4. Dark parity: line-transition count on the same scan line equals light; switch knob/track >= 3:1; avatar fill vs surface >= 1.5:1; value fill > track contrast.
- D5. Every bordered surface has a 1 px line in both themes (a fill step alone fails).

### E. States
- E1. Every list, table, pane and card region has a loading (skeleton at the real row height), empty (Empty component) and error (Alert with Retry) design, captured as its own scene.
- E2. Each named state capture differs from its base screen in the state region (`diff.py` npx > 0) and shows an element named for the state.
- E3. Every control shows default, hover, focus (3 px ring), disabled (50%), loading (spinner, width kept) and, for fields, error. F6 / ⌥Tab (⇧ back) move the focus between the window's regions: sidebar, thread, composer (v16, K-10). [visual; hover and focus are not in the current capture set]
- E4. A disabled primary differs from Cancel (outline vs primary at 50%).
- E5. Sheet height does not change when an error appears.

### F. Content
- F1. Status is always text + icon (StatusBadge); 0 free colored dots or "!" circles without text.
- F2. Times and numbers right-aligned at one x per column, tabular; thousands separators; no epoch dates; same-day ranges formatted.
- F3. Worst-case set: 0 strings clipped at the start or mid-glyph; 0 values wrapped mid-phrase in a DescriptionList; every truncated value has a tooltip.
- F4. Every icon column in a list aligns at one x; every right column aligns at one x.
- F5. No placeholder-looking text: no raw English debug labels, no "0 B", no trailing "…" on a button that opens nothing, no lone centered line.
- F6. Selected rows use the muted fill; no inverted black row except the primary button.

### G. Code
- G1. `rg 'Button::new\(' views` = 0.
- G2. `px(` literal count in `views/` is lower than the previous merge (ratchet test).
- G3. No `.rounded(px(`, `.text_size(px(` or `.gap(px(` literal in `views/`.
- G4. `theme::apply` writes ThemeColor once.

---

## 5. Not confirmed

- Hover, focus and keyboard states: the capture set holds only one hover (toolbar in topic-short). E3 needs new captures.
- process-dev and stop-stopping: identical to their base captures. A capture-script gap or missing UI; F09 applies either way.
- settings-about renders the General tab: possibly a capture-script mistake (F39).
- Dark token values in 1.2 come from the shadcn v4 neutral theme, not from a file in `shadcn-bar/reg/`.
- Font sizes come from CJK ink heights (about 0.85x the size, +- 1 px).
- Notification and Dialog text sizes on screen were not measured with them open; K01 is confirmed only for menus.
- The traffic lights are gray in the "active" captures too; not counted as a defect.
- Most dark-mode findings come from 5-10 dark scenes; the dark worst-case set was not opened image by image.
- The voids in short threads partly come from the seed data; F03 applies anyway.
