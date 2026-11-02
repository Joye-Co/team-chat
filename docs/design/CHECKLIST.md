# Design checklist (GPUI)

Check every UI change against this list before merging. It distills the vendored skills in
[skills/emilkowalski](skills/emilkowalski/) (written for the web) into checks for this native
macOS app. Each item says **what** to check in this app, **how** in GPUI / gpui-component, and the
**source** section. What GPUI can and cannot do is in [GPUI-CAPABILITIES.md](GPUI-CAPABILITIES.md).

Paths: `EDE` = `skills/emil-design-eng/SKILL.md`, `AD` = `skills/apple-design/SKILL.md`,
`RA` = `skills/review-animations/SKILL.md`, `STD` = `skills/review-animations/STANDARDS.md`,
`BU` = `skills/break-ui/SKILL.md`, `CAT` = `skills/break-ui/CATALOG.md`,
`FAO` = `skills/find-animation-opportunities/SKILL.md`, `RM` = `skills/remove-animations/SKILL.md`,
`AP` = `skills/animation-performance/SKILL.md`, `UIC` = `ui-cheatsheet.md`,
`PC` = `performance-cheatsheet.md`.

## 0. How to look

| # | Check | How | Source |
| --- | --- | --- | --- |
| 0.1 | Every changed screen captured in light **and** dark | `desktop --screenshot <dir> --theme both --scenes <yours>`; read every PNG. Add a scene in `apps/desktop/src/selftest/scenes.rs` when your screen has none | AGENTS.md design bar; `BU` Phase 4 |
| 0.2 | Same screens with worst-case data | `desktop --screenshot <dir> --worst-case`; data changes at the provider, never in the view (`selftest/worst_case.rs`) | `BU` Hard Rules 2-3 |
| 0.3 | Compare with the baseline | Baseline PNGs in `~/.cache/team-chat/design/baseline/` (same names); open both side by side | `EDE` "Review your work the next day" |
| 0.4 | Narrow and wide windows | `--size 900x560` (the window minimum) and `--size 1440x900`; the side panel and topic list must not squeeze the timeline below `min_w` | `CAT` Environment |
| 0.5 | Review fresh, at 1x and 2x zoom of the PNG | Read the PNG again the next session; look at edges, baselines and spacing, not only "does it render" | `EDE` Core Philosophy ("Unseen details compound") |

## 1. Visual craft (calm, precise, Chinese-first)

| # | Check | How in GPUI | Source |
| --- | --- | --- | --- |
| 1.1 | Every color comes from the theme and works in both modes | Only `theme::colors(cx)` / `cx.theme()` slots; no `rgb(...)` in views. Status chip, presence and mention colors live in `apps/desktop/src/theme.rs` | `AD` 16 Craft ("colors that adapt to light/dark"); `CAT` Dark mode |
| 1.2 | Controls are gpui-component controls | `Button`, `Input`, `Tag`, `Badge`, `Avatar`, `List`, `Tooltip`, `Popover`, `PopupMenu`, `Command`, `Switch`, `Empty`, ... (catalog in GPUI-CAPABILITIES §1) | AGENTS.md design bar; `EDE` Sonner principles ("Good defaults") |
| 1.3 | System font with PingFang SC for Han, SF Pro for Latin and digits; one or two weights per screen | `.SystemUIFont` (theme default) with `FontFallbacks` if pinning; weight `NORMAL` for body, `MEDIUM`/`SEMIBOLD` for names and titles; never `BOLD` for body emphasis in lists | `AD` 15 Typography ("Default to the platform's system font", "hierarchy from weight + size + leading") |
| 1.4 | Type scale is small and steady | Body 13-14 px, secondary 12 px, titles 15-17 px; line height ≥ 1.5 for Chinese body text, tighter for one-line rows | `AD` 15 ("Leading tracks size inversely"); `UIC` "Body text looks awkward" |
| 1.5 | Message text lines are not too long | Cap message body width (about 40 Han characters / 70 Latin characters, e.g. `max_w(px(720.))`) on wide windows | `UIC` "max-width: 65ch"; `CAT` 2560px wide |
| 1.6 | Counts, times and sizes use tabular figures | `.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))` on badges, time stamps, the 进行中 count, file sizes | `UIC` tabular-nums; `CAT` "A value that changes live" |
| 1.7 | Old times show an absolute date | `views::time_text`: relative only within a week, then `11月1日`, with the year when not this year | `UIC` "Old timestamps"; `CAT` Time |
| 1.8 | No heavy borders where spacing or a soft surface separates | Prefer background steps (`sidebar` vs `background`) and spacing over `border_1`; keep at most one hairline between panes; borders use `border` with low contrast | Task bar ("no heavy borders"); `AD` 12 ("Scroll edge effects, not hard dividers") |
| 1.9 | No cards inside cards, no gradients or glow for decoration, no emoji as decoration | One surface level per region; `linear_gradient` only for scroll-edge fades | Task bar ("No generic AI look"); `AD` 12 ("Never stack a light material on a light material") |
| 1.10 | Radii are consistent and nest | Theme `radius` (6) for controls, `radius_lg` (8) for popovers/dialogs; an inner radius = outer radius - padding | `EDE` Cohesion matters |
| 1.11 | Depth through soft shadows only on floating surfaces | `shadow_md`/`shadow_lg` on popovers, menus, dialogs, toasts; no shadows on in-flow rows or panes | `AD` 12 ("Bigger surfaces should read as thicker") |
| 1.12 | Avatars never squish; bot mark is readable | `flex_none()` on avatar boxes; `Avatar` from gpui-component (initials + color, image fallback) | `UIC` "Avatar squished"; `BU` failure signatures |
| 1.13 | Alignment: rows share a left edge and baseline; icons sit on the text's optical center | `items_center` for one-line rows; `items_start` when a name can wrap; icon size 16 with text 13-14 | `BU` "Avatar centered against a three-line name" |
| 1.14 | Hit targets and hover states on everything clickable | Rows at least 28 px high; `.hover(|s| s.bg(list_hover))`, `.cursor_pointer()`; selected uses `list_active` | `AD` 1 Response; `AD` 10 ("hit padding") |
| 1.15 | Keyboard focus is visible and only for keyboard | gpui-component focus ring (`theme.focus_ring`); `track_focus` on custom focusable views | `AD` 14 Accessibility |
| 1.16 | Title bar and sidebar feel like a macOS app | Consider `TitlebarOptions { appears_transparent: true, traffic_light_position }` with a full-height sidebar (Mail, Linear, Things); optional `WindowBackgroundAppearance::Blurred` with a translucent `sidebar` color | `AD` 12 Materials & depth |
| 1.17 | Wayfinding: where am I, what's open, how do I get out | Selected 群 and 话题 clearly marked; window title or header names the open 话题; Esc closes popovers and dialogs | `AD` 16 ("Wayfinding") |
| 1.18 | Specific, short labels from the copy catalog | Only `copy::*`; the four nouns 成员 群 话题 产出; no developer words outside the developer view | `AD` 16 ("Direct, specific labels"); docs/PRODUCT.md |

## 2. Worst-case data (break-ui)

Run `--worst-case` and check each row. The fixture (`apps/desktop/src/selftest/worst_case.rs`)
covers them; add values there when a new field appears.

| # | Field in this app | Worst case | Expected | How in GPUI | Source |
| --- | --- | --- | --- | --- | --- |
| 2.1 | 成员 name (sidebar DM, message header, settings, side panel) | `Aleksandra Wiśniewska-Kowalczyk`, `王`, `bartholomew.fitzgerald@...example.com`, `👩🏽‍💻 Priya`, RTL `نور الهدى عبد الرحمن`, `  Sam   Lee ` | One line, truncated at the end, full name in a tooltip or `HoverCard`; role and time never pushed off | `min_w_0()` on the text column, `.truncate()` on the name, `flex_none()` on time and badges | `CAT` People and names; `BU` "Text overflows its box" |
| 2.2 | Avatar initial | Emoji-first, one-letter, space-first names | A real grapheme, never `�` or blank | Take the first grapheme of the trimmed name (or use `Avatar::name`) | `BU` "Wrong initials" |
| 2.3 | 群 name | 40-character Chinese name, unbroken URL, `群`, `🚀🚀🚀` | Truncates in the sidebar and in the topic-list header; never overlaps the 新话题 button | Header: `min_w_0()` + `.truncate()` on the title, `flex_none()` on the button | `CAT` Labels |
| 2.4 | 话题 title | 80 characters, `1`, empty, newline, `<script>`, file name, UUID | Clamp to 2 lines in the list (`line_clamp(2)`), one line in the header; empty shows a placeholder; newline is not rendered as a line break in rows; literal text, no markup | `line_clamp`, `.truncate()`, replace `\n` with a space for one-line display | `CAT` Labels; `BU` Truncate, wrap, or clamp |
| 2.5 | Unread / mention counts | 0, 1, 99, 100, 999, 1,000, 12,345, 1,000,000 | 0 hides the badge; caps at `99+` (`@99+`); badge never wraps; width stable | `Badge::count(..).max(99)` or `views::count_badge`; `flex_none()`; tabular figures | `CAT` Numbers; `BU` "Badge wraps" |
| 2.6 | Message count / `N 条消息` | 1, 1,000,000 | Thousands separator for ≥ 1,000 (`12,345` or `1.2万`) | Format in one helper | `CAT` "Needs a thousands separator" |
| 2.7 | Message body | Unbroken URL, very long code line, wide Markdown table, emoji-only, RTL mixed with Chinese, 2,000-character paragraph | Wraps inside the column; code and tables scroll horizontally inside their block; nothing pushes the side panel | `TextView::markdown`, parent `min_w_0()`, code block `overflow_x_scroll` | `CAT` Emails, URLs; `BU` "Email or URL runs past the edge" |
| 2.8 | Status chip with a name (`等{name}审`) | Long member name | Chip never wraps; the name inside truncates | `whitespace_nowrap`, max width on the chip, `.truncate()` | `BU` "Badge wraps onto two lines" |
| 2.9 | 产出 title and size | `Q3 Board Deck — FINAL (revised) v12 [approved by legal].pdf`, 0 bytes, 100 MB, version 999 | Middle truncation keeps the extension and version; sizes formatted (`100 MB`) | `.text_ellipsis_middle()` | `BU` "Truncate in the middle" |
| 2.10 | Empty lists | No 群, no 话题, no messages, no 产出, no 待办 | A designed empty state (`Empty` with a short line and the next action), not a blank pane | `scene empty`; gpui-component `Empty` | `CAT` Collections ("0 items"); `BU` Phase 2 Empty |
| 2.11 | One item | One 群, one 话题, `1 条消息` | Lists do not look broken; no "1 of 1" oddities | Check with a provider that returns one row | `BU` Phase 2 One |
| 2.12 | 10,000 messages | The `long` scene | Opens anchored at the bottom, scrolls at 60 fps | Virtualized list (`MessageScroller` / `List`); never render all rows | `PC` "Long list scrolls slowly"; `CAT` 1,000+ items |
| 2.13 | Times | Now, 12 days ago, 3 years ago, 0 (1970) | "刚刚", absolute date after a week, year for old dates, 1970 never shown as a real date | `views::time_text` | `CAT` Time |
| 2.14 | States | Pending send, failed send, streaming, deleted, edited, offline / reconnecting | Each has a quiet visible state; failed has a retry; nothing flashes | Check the fake provider's sample states in `main` | `CAT` States |
| 2.15 | Window size | 900x560 minimum | No clipped controls; panels collapse before text overlaps | `--size 900x560` | `CAT` Environment |

## 3. Motion

Decide first whether something animates; most things in a work chat used all day should not.

| # | Check | How in GPUI | Source |
| --- | --- | --- | --- |
| 3.1 | Frequency gate: keyboard actions and things seen 100+ times a day do not animate | Cmd+K switcher, switching 群 or 话题, sending a message, arrow-key list navigation: instant. No `with_animation` on these paths | `EDE` Animation Decision Framework 1; `RA` Standard 2; `RM` Where to Hunt (Keyboard) |
| 3.2 | Every animation has a purpose | Allowed: a new message arriving (state), streaming text (`TextView::stream_fade`), a toast entering and leaving (spatial), a popover appearing (prevent jarring change). Not allowed: decoration, pulsing badges, loops that are not loading indicators | `RA` Standard 1; `FAO` The Gate 2; `RM` Decoration |
| 3.3 | Durations under 300 ms | Use `cx.theme().motion`: `duration_fast` 120 ms (hover, press), `duration_normal` 180 ms (popover, menu), `duration_slow` 280 ms (dialog, sheet) | `EDE` 4 How fast; `STD` Duration |
| 3.4 | Ease-out for entering, never ease-in for UI | `motion.easing_enter` (0.16, 1, 0.3, 1) for enter; exits shorter than enters; `easing_move` for on-screen moves | `EDE` 3 Easing; `RA` Standard 3; `STD` Easing |
| 3.5 | Interruptible | Use gpui-base `transition(...)` or `spring(...)` (retarget from the current value), not `with_animation` (restarts from zero) for anything that can toggle quickly (panel open/close, hover) | `RA` Standard 6; `AD` 3 Interruptibility; `EDE` "Use CSS transitions over keyframes" |
| 3.6 | Springs critically damped for UI | `motion.spring_control` (no overshoot); bounce only for rare moments | `AD` 4 and Quick Reference; `EDE` Spring Animations |
| 3.7 | Asymmetric enter / exit | Exit faster (about 2/3 of enter) | `EDE` Asymmetric enter/exit timing; `RA` Standard 9 |
| 3.8 | No scale-from-zero, origin at the trigger | GPUI has no box transforms: use opacity plus a 4-8 px offset (`motion.distance_short`/`distance_medium`) in the direction of the trigger; modals stay centered | `RA` Standard 5; `STD` Physicality; GPUI-CAPABILITIES §8 |
| 3.9 | Reduced motion honored | gpui-base motion reads `cx.reduce_motion()`; custom `with_animation` must check it and fall back to an opacity change or none | `RA` Standard 8; `AD` 14; `EDE` Accessibility |
| 3.10 | Cheap to draw | Animate opacity and color; avoid animating widths/heights of large regions (each frame re-lays-out the window); never animate inside virtualized lists; spinners only for work that takes over 300 ms | `AP` Hard Rules 1 and What You Animate; `RM` Over-engineering; `PC` |
| 3.11 | No stagger on lists opened many times a day | Topic list, timeline and sidebar appear at once | `FAO` Group entrances (occasional only); `RM` Decoration |
| 3.12 | Tooltips: delay on first, instant after | `Tooltip` default delay; do not add enter animations to tooltips | `EDE` Tooltips: skip delay on subsequent hovers |
| 3.13 | Cohesion | One motion feel for the app, from the theme tokens; a calm work tool stays crisp, no bounce | `RA` Standard 10; `EDE` Cohesion matters |
| 3.14 | Opportunities worth having | New-topic composer focus, a message moving from pending to sent (opacity), a 产出 card arriving, the side panel opening (spring on width only if it stays smooth), success of a bot's result (rare: a short, quiet highlight) | `FAO` Where to Hunt (Teleporting state, Delight budget) |
| 3.15 | When removing motion, remove its start state and waits | Delete hidden-at-start opacity and timers sized to the animation together with it | `RM` Removal Hazards |

## 4. Responsiveness and feel (apple-design)

| # | Check | How in GPUI | Source |
| --- | --- | --- | --- |
| 4.1 | Feedback on mouse-down, not release | `.active(|s| ...)` style on pressable rows and buttons (gpui-component buttons do this); no press scale is possible, so darken (`button_active`, `list_active`) | `AD` 1 Response; `EDE` Buttons must feel responsive |
| 4.2 | No artificial latency | No timers or debounces on the input path except the composer typing signal (provider limits it); views read provider snapshots synchronously | `AD` 1 ("Be vigilant about every latency") |
| 4.3 | Never lock input during a transition | Views stay interactive while anything animates | `AD` 3 ("Never lock out input during a transition") |
| 4.4 | Scroll edges, not hard dividers, under floating headers | `MessageScroller::with_bottom_fade`, gradient overlays at scroll edges instead of a 1 px border | `AD` 12 ("Scroll edge effects") |
| 4.5 | Destructive actions are forgiving | Delete message: undo toast or `AlertDialog` only for irreversible actions | `AD` 16 Agency |
| 4.6 | Feedback in four kinds | Status (connection line), completion (sent), warning, error (failed send with retry) are visible and quiet | `AD` 16 ("Feedback comes in four kinds") |

## 5. Before merging

- [ ] 0.1-0.4 screenshots read (light, dark, worst case, narrow) and compared with the baseline.
- [ ] Every row of §1 that applies holds; colors only from the theme.
- [ ] Every row of §2 for the fields you render holds, or the break is listed in the PR as a
      decision (truncate vs wrap) with the PNG.
- [ ] Every animation passes §3.1-3.10, or it was removed.
- [ ] `cargo test -p desktop` (copy catalog check) passes.
