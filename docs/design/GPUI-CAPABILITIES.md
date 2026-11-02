# What GPUI and gpui-component can do

For designers and implementers of the desktop app. It says what the UI stack in `apps/desktop`
can draw and animate, how to reach it, and what it cannot do, so a design does not ask for
something the stack cannot build.

Versions: `gpui-pre 0.3.10` (GPUI), `gpui-component 0.8.1`, `gpui-base 0.8.1` (the layer under
gpui-component: motion, scrollbars, focus), `gpui-kit 0.8.1` (window root, `init`) and
`gpui-kit-assets 0.8.1` (Lucide icons). Paths below are relative to
`~/.cargo/registry/src/index.crates.io-*/` (run `cargo fetch` in a worktree if they are missing).
Read the source when this file and the code disagree: the code wins.

Look at the result with the screenshot self-test (`desktop --screenshot <dir>`, see
`apps/desktop/README.md`) in light and dark mode and with `--worst-case`.

## 1. Components (gpui-component 0.8.1)

Rule (AGENTS.md): use the component when it exists; never hand-roll a control. Style it through
the theme (section 2), not per call site. Most components take `Sizable` (`.xsmall()`,
`.small()`, default medium, `.large()`, `.with_size(px)`; `gpui-component-0.8.1/src/sizing.rs`)
and `Disableable`. Builder names come from the source; the main ones are listed.

### Controls

| Component | Purpose | Key options | Source |
| --- | --- | --- | --- |
| `Button`, `ButtonGroup`, `DropdownButton`, `Toggle`, `ToggleGroup` | Push buttons, button rows, split button, on/off toggle | Variants `primary` `secondary` `danger` `info` `success` `warning` `ghost` `link` `text` `outline` `custom(ButtonCustomVariant)`; `icon`, `label`, `loading`, `selected`, `compact`, `dropdown_menu`, `tooltip`, `on_click` | `src/button/` |
| `Input` + `InputState` | Single-line field, also password (`mask_toggle`), number, OTP, search | `placeholder`, `prefix`/`suffix`, `cleanable`, `appearance(false)` (no border), `bordered`, `focus_bordered`, `loading`, `disabled`, IME support | `src/input/` |
| `Textarea` (`InputState::multi_line`) | Multi-line field (the composer) | `rows`, auto-grow, `soft_wrap` | `src/input/` |
| `Editor` (`InputState::code_editor`) | Code editor with syntax highlight | `line_number`, language | `src/input/`, `src/highlighter/` |
| `NumberInput`, `OtpInput` | Stepper number field, one-time code boxes | `min`/`max`/`step`; length, `masked` | `src/input/` |
| `Checkbox`, `Radio`, `RadioGroup`, `Switch` | Booleans and one-of-many | `checked`, `label`, `on_click`/`on_change`, `tooltip` | `src/checkbox.rs`, `src/radio.rs`, `src/switch.rs` |
| `Slider` | Value on a track | `horizontal`/`vertical`, `reverse`, range | `src/slider.rs` |
| `Select` + `SelectState` | Drop-down chooser | `searchable`, `placeholder`, `cleanable`, `menu_width`, `menu_max_h`, `icon` | `src/select.rs` |
| `Combobox` | Searchable select, single or `multiple` | `searchable`, `check_icon`, `menu_width` | `src/combobox.rs` |
| `ColorPicker` | Color swatch and picker | `featured_colors`, `anchor` | `src/color_picker.rs` |
| `Rating` | Stars | `max`, `value`, `color` | `src/rating.rs` |
| `Kbd` | A key binding as keycaps | `binding_for_action`, `outline` | `src/kbd.rs` |
| `Link` | Inline link | `href`, `on_click` | `src/link.rs` |

### Navigation and structure

| Component | Purpose | Key options | Source |
| --- | --- | --- | --- |
| `Sidebar`, `SidebarGroup`, `SidebarMenu`, `SidebarMenuItem`, `SidebarHeader/Footer`, `SidebarToggleButton` | A collapsible app sidebar with groups and items | `collapsible`, `collapsed`, `side`, item `icon`, `active`, `suffix` (badges), `context_menu`, `default_open` | `src/sidebar/` |
| `TabBar`, `Tab` | Tabs | `underline`, `pill`, `outline`, `segmented` (a macOS-style segmented control), `prefix`/`suffix`, `selected_index` | `src/tab/` |
| `Breadcrumb` | Path of places | items with `on_click` | `src/breadcrumb.rs` |
| `Pagination` | Page numbers | `compact`, `visible_pages` | `src/pagination.rs` |
| `Stepper` | Steps of a flow | `vertical`, `selected_index` | `src/stepper/` |
| `Accordion`, `Collapsible` | Disclosure sections | `multiple`, `bordered`, `open`, `on_toggle_click` | `src/accordion.rs`, `src/collapsible.rs` |
| `Resizable` (`h_resizable`, `v_resizable`, `resizable_panel`) | Split panes with drag handles | `size`, `size_range` | `src/resizable.rs` |
| `Dock`, `DockArea` | IDE-style docking panels | panel placement, tabs | `src/dock/` |
| `TitleBar` | Custom title bar drawn by the app (content beside the traffic lights) | `TitleBar::title_bar_options()` gives `appears_transparent: true`, traffic lights at (9, 9); height 34 px (`TITLE_BAR_HEIGHT`) | `src/title_bar.rs` |
| `StatusBar`, `Toolbar`, `ToolbarGroup` | Bottom status line, tool rows | `left`/`right`; groups | `src/status_bar.rs`, `src/toolbar.rs` |
| `Settings`, `SettingPage`, `SettingGroup`, `SettingItem`, `SettingField` | A whole settings window: sidebar of pages, groups of fields | `sidebar_width`, `group_variant`, `resettable`, field kinds (switch, input, select, ...) | `src/setting/` |
| `Form`, `Field` | Labeled form grid | `horizontal`/`vertical`, `label_width`, `columns`, `required`, `description` | `src/form/` |
| `GroupBox` | Titled group of content (grouped-table look) | `title`, `footer` | `src/group_box.rs` |
| `DescriptionList` | Label / value pairs | `columns`, `horizontal`, `bordered`, `label_width` | `src/description_list.rs` |
| `Separator` | Line, optionally with a label | `horizontal`/`vertical`, `dashed`, `label` | `src/separator.rs` |

### Lists, tables, text

| Component | Purpose | Key options | Source |
| --- | --- | --- | --- |
| `List` + `ListDelegate` + `ListItem` | Virtualized, keyboard-navigable list with optional search | `searchable`, `selectable`, `scroll_to_item`, sections, `Loading` rows, `ListItem::selected/disabled/suffix` | `src/list/` |
| `v_virtual_list`, `h_virtual_list` | Virtual list of rows with known sizes | item sizes, scroll handle | `src/virtual_list.rs` |
| `MessageScroller` + `MessageScrollerState` | Chat timeline: anchored at the bottom, follows the tail, `prepend` older pages without a jump, jump-to-latest button, bottom fade | `append`, `prepend`, `splice`, `remeasure`, `scroll_to_end`, `is_following_tail`, `with_jump_button`, `with_bottom_fade` | `src/message_scroller.rs` |
| `Message`, `MessageGroup`, `MessageAvatar`, `MessageHeader`, `MessageContent`, `MessageFooter`, `Bubble`, `BubbleReactions` | Chat message layout parts | `alignment`, `avatar`, `role`, stacked style | `src/message.rs`, `src/bubble.rs` |
| `Attachment` (+ `AttachmentMedia`, `Title`, `Description`, `Actions`) | File or image card with upload state | `status` (pending, uploading, processing, complete, failed), `progress`, `on_retry`, `on_remove`, shimmer | `src/attachment.rs` |
| `Table`, `DataTable` + `TableDelegate` | Virtualized data table | `col_resizable`, `col_movable`, `fixed_left`, `stripe`, `bordered`, sorting, selection | `src/table/` |
| `Tree` | Tree view | keyboard nav | `src/tree.rs` |
| `SearchableList` | List with filter and multi-select | `checked`, `selection` | `src/searchable_list/` |
| `TextView` (`TextView::markdown`, `::html`) | Markdown and HTML rendering (GFM tables, code blocks with highlight), selectable, streaming | `selectable`, `max_lines`, `paragraph_gap`, `stream_fade` (fades new streamed text), `TextViewStyle` for code, quotes, tables | `src/text/` |
| `Label` | Text with highlighted matches, secondary text | `highlights`, `secondary`, `masked` | `src/label.rs` |
| `Diff` | Text diff view | unified or split, collapse unchanged | `src/diff/` |
| `Chart` (`LineChart`, `BarChart`, `AreaChart`, `PieChart`, `CandlestickChart`), `plot` | Charts | series, axes, colors `chart.1-5` | `src/chart/`, `src/plot/` |

### Feedback and overlays

| Component | Purpose | Key options | Source |
| --- | --- | --- | --- |
| `Badge` | Count or dot on an element | `count`, `max` (shows `99+`), `dot`, `icon`, `color` | `src/badge.rs` |
| `Tag` | Small label chip (status chips) | `primary` `secondary` `danger` `success` `warning` `info` `custom`, `outline`, `rounded_full` | `src/tag.rs` |
| `Avatar`, `AvatarGroup` | Image avatar with initials fallback | `src`, `name` (initials + color), `placeholder`, `limit` | `src/avatar/` |
| `Alert` | Inline message box | `info` `success` `warning` `error`, `banner`, `title`, `on_close` | `src/alert.rs` |
| `Notification`, `NotificationList` (`window.push_notification`) | Toasts; also macOS system notifications | `info` `success` `warning` `error`, `autohide`, `placement`, `delivery` (in-app and/or system) | `src/notification.rs` |
| `Dialog`, `AlertDialog` (`window.open_dialog`) | Modal dialogs | `title`, `width`, `overlay`, `overlay_closable`, `keyboard` (Esc), `confirm`/`on_ok`/`on_cancel` | `src/dialog/` |
| `Sheet` (`window.open_sheet`, `open_sheet_at(Placement, ..)`) | Side drawer | placement via `open_sheet_at`, `size`, `resizable`, `overlay`, `overlay_closable` | `src/sheet.rs` |
| `Popover` | Floating panel at a trigger | `anchor` (corner), `offset`, `arrow`, `overlay_closable`, `track_focus` | `src/popover.rs` |
| `HoverCard` | Card on hover (member card) | `open_delay`, `close_delay`, `anchor` | `src/hover_card.rs` |
| `Tooltip` | Tooltip, with key binding | `Tooltip::new(text).action(&Action)`, `key_binding` | `src/tooltip.rs` |
| `PopupMenu`, `ContextMenu` (`.context_menu(...)`), `DropdownMenu` (`.dropdown_menu(...)`) | Menus | `menu`, `menu_with_check`, `menu_with_icon`, `separator`, `submenu`, `link`, `max_h` | `src/menu/` |
| `AppMenuBar`, `native_menu` | The app's menus | native NSMenu through GPUI | `src/menu/`, `src/native_menu/` |
| `Command`, `CommandGroup`, `CommandItem` | Command palette / quick switcher (Cmd+K) | `searchable`, `filterable`, groups with `heading`, `icon`, `max_h`, `set_loading` | `src/command/` |
| `Progress`, `ProgressCircle`, `Spinner`, `Skeleton`, `ShimmerText` | Progress, busy and loading states | `value`, `loading`; `Spinner::ease`; `Skeleton::secondary`; shimmer `duration`, `once` | `src/progress/`, `src/spinner.rs`, `src/skeleton.rs`, `src/shimmer.rs` |
| `Empty` (+ `EmptyHeader`, `EmptyMedia`, `EmptyTitle`, `EmptyDescription`, `EmptyContent`) | Empty states | variants, media icon, action content | `src/empty.rs` |
| `Questionnaire` | Multiple-choice question card | choices, shortcuts | `src/questionnaire/` |
| `Carousel` | Paged content | `looping`, axis | `src/carousel/` |
| `Scrollable` (`.overflow_y_scrollbar()`) | Scroll area with overlay scrollbar | `scroll_bounce`; scrollbar mode from the theme | `src/scroll/` |

Also: `WindowBorder` (client-side resize border and shadow for a frameless window,
`src/window_border.rs`), `clipboard`, `history` (undo), `inspector` (debug element inspector).

## 2. Theme

### Where values come from

- `Theme` is a GPUI global (`gpui-component-0.8.1/src/theme/mod.rs`). Read it in views with
  `cx.theme()` (trait `ActiveTheme`). In this app views read `crate::theme::colors(cx)`, which maps
  app roles to theme slots (`apps/desktop/src/theme.rs`).
- Fields besides colors: `font_family` (default `.SystemUIFont`), `font_size` (16), 
  `mono_font_family` (Menlo on macOS), `mono_font_size` (13), `radius` (6), `radius_lg` (8),
  `shadow` (bool, inputs and buttons draw a small shadow), `focus_ring` (bool),
  `scrollbar_mode` (`Scrolling` default, `Hover`, `Always`), `list` and `sheet` settings,
  `motion` (section 5), `highlight_theme` (code highlight).
- `ThemeMode::{Light, Dark}`. `Theme::change(mode, window, cx)` loads the registered theme for
  that mode and refreshes every window. `Theme::sync_system_appearance(window, cx)` follows the
  window's appearance; this app calls it from `theme::follow_system`.
- `cx.set_window_appearance(Some(WindowAppearance::Dark))` (GPUI, `gpui-pre-0.3.10/src/app.rs`)
  sets `NSApplication.appearance`, so native chrome (title bar, traffic lights, window border)
  matches. The screenshot self-test uses both.

### Color slots (`ThemeColor`, `src/theme/theme_color.rs`, defaults in `src/theme/default-theme.json`)

All are `Hsla`. JSON keys use dots (`primary.background`, `list.active.background`).

| Group | Slots |
| --- | --- |
| Base | `background`, `foreground`, `border`, `muted`, `muted_foreground`, `accent`, `accent_foreground`, `ring` (focus), `selection`, `caret`, `overlay`, `window_border`, `transparent` |
| Intent | `primary`, `secondary`, `danger`, `info`, `success`, `warning`, each with `_foreground`, `_hover`, `_active` |
| Buttons | `button`, `button_hover`, `button_active`, `button_foreground`, and `button_<intent>[_hover/_active/_foreground]` for every intent |
| Inputs | `input` (border), `switch`, `switch_thumb`, `slider_bar`, `slider_thumb`, `progress_bar`, `skeleton` |
| Lists and tables | `list`, `list_hover`, `list_active`, `list_active_border`, `list_even`, `list_head`; `table`, `table_hover`, `table_active`, `table_active_border`, `table_even`, `table_head`, `table_head_foreground`, `table_foot`, `table_foot_foreground`, `table_row_border` |
| Surfaces | `popover`, `popover_foreground`, `accordion`, `group_box`, `group_box_foreground`, `description_list_label`, `description_list_label_foreground` |
| Chrome | `sidebar`, `sidebar_foreground`, `sidebar_border`, `sidebar_accent`, `sidebar_accent_foreground`, `sidebar_primary`, `sidebar_primary_foreground`, `title_bar`, `title_bar_border`, `status_bar`, `status_bar_border`, `tab_bar`, `tab_bar_segmented`, `tab`, `tab_active`, `tab_foreground`, `tab_active_foreground` |
| Scroll, drag | `scrollbar`, `scrollbar_thumb`, `scrollbar_thumb_hover`, `drag_border`, `drop_target` |
| Links | `link`, `link_hover`, `link_active` |
| Palette | `red`, `green`, `blue`, `yellow`, `magenta`, `cyan`, each with `_light`; `chart_1`..`chart_5`, `chart_bullish`, `chart_bearish`, `chart_grid` |

There is also a newer semantic layer, `Theme::semantic_tokens()` (gpui-base `theme_tokens.rs`),
with radius `none sm md lg xl full` and shadow `sm ...` tokens settable from the JSON
(`schema.rs`, `radius`, `shadow` objects). New app code reads `cx.theme()` slots or
`theme::colors(cx)`; it does not extend the legacy `tokens` field.

### A custom theme for light and dark

Two ways; both keep every color in one place.

1. **JSON theme set** (preferred for the design pass). Write one file with two themes, `"mode":
   "light"` and `"mode": "dark"`, in the format of `src/theme/default-theme.json` (colors as
   `#rrggbb[aa]`, `hsl()`, or Tailwind names like `neutral-200/60`; also `font.family`,
   `font.size`, `mono_font.family`, `radius`, `radius.lg`, `shadow`). Load it at start with
   `ThemeRegistry::global_mut(cx).load_themes_from_str(include_str!(...))`, set
   `Theme::global_mut(cx).light_theme` / `.dark_theme` to the loaded configs (from
   `ThemeRegistry::themes()`), then `Theme::change(mode, None, cx)`. `ThemeRegistry::watch_dir`
   reloads a directory of theme files on change (good for live tuning in development).
2. **In code**: `Theme::update(cx, |t| { t.colors.primary = ...; t.radius = px(8.) })` after
   `Theme::change`. A later `Theme::change` reloads from the config and drops these edits, so
   re-apply them after every mode change (or use option 1).

App-only roles (status chip colors, presence, mention badge) stay in `apps/desktop/src/theme.rs`
as functions of the mode.

## 3. Fonts and text

- **Family.** The default `.SystemUIFont` is SF Pro on macOS (`gpui-pre-apple-0.3.10/src/text_system.rs`
  maps it to `.AppleSystemUIFont`). Han characters fall back through Core Text to PingFang SC, so
  Chinese already renders in PingFang with SF Pro for Latin and digits. To pin it, give a font
  explicit fallbacks: `Font { family: ".SystemUIFont".into(), fallbacks:
  Some(FontFallbacks::from_fonts(vec!["PingFang SC".into()])), .. }` or on an element
  `.font_family(...)` plus `TextStyle::font_fallbacks` (`gpui-pre-0.3.10/src/text_system/font_fallbacks.rs`;
  applied in `gpui-pre-apple-0.3.10/src/open_type.rs`). Custom fonts are embedded with
  `cx.text_system().add_fonts(...)`.
- **Mono.** `theme.mono_font_family` (Menlo); use SF Mono by naming `"SF Mono"` if installed.
- **Weights.** `FontWeight` 100-900 (`THIN` ... `BLACK`, `gpui-pre-0.3.10/src/text_system.rs:1161`):
  `.font_weight(FontWeight::MEDIUM)` (there are no `font_semibold()`-style helpers in GPUI).
- **Sizes.** `.text_xs()` (12) `.text_sm()` (14) `.text_base()` (16) `.text_lg()` ... or
  `.text_size(px(13.))`. Line height `.line_height(px(..))` or `relative(1.5)`.
- **OpenType features.** `FontFeatures(Arc<Vec<(String, u32)>>)`, applied by Core Text
  (`src/text_system/font_features.rs`). Tabular numbers: `.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))`
  on the element (Styled `font_features`). Also `"ss01"`, `"case"`, `"calt"` (0 disables ligatures).
- **Overflow.** `.truncate()` (one line + ellipsis), `.text_ellipsis()`, `.text_ellipsis_start()`,
  `.text_ellipsis_middle()` (for file names and ids), `.line_clamp(n)`, `.whitespace_nowrap()`,
  `.whitespace_normal()`. Long unbroken strings need `min_w_0()` on the flex parent and a width.
- **Decoration.** `.underline()`, `.line_through()`, `.italic()`, decoration color and wavy style.
- **Selection.** Plain `div` text is not selectable. `TextView` (`selectable(true)`) and
  `Input`/`Textarea` are.

## 4. Styling (GPUI `Styled`, `gpui-pre-0.3.10/src/styled.rs` and the `gpui-pre-macros` style helpers)

- **Layout.** Flexbox and CSS grid through Taffy: `flex`, `flex_col`, `gap_*`, `items_*`,
  `justify_*`, `grid_cols(n)`, `col_span`, `aspect_ratio`, `size`, `w`/`h`/`min_*`/`max_*`,
  `absolute`/`relative` with `top`/`left`, `overflow_hidden`, `overflow_y_scroll` (prefer
  gpui-component's `overflow_y_scrollbar()`). Units: `px()`, `rems()`, `relative()` (fraction).
- **Spacing.** `p_*`, `px_*`, `m_*`, `gap_*` on the Tailwind 4 px scale, or exact `px()`.
- **Radius.** `rounded_none/xs/sm/md/lg/xl/2xl/3xl/full`, `rounded(px(10.))`, per corner
  (`rounded_tl`, ...). No squircle (continuous) corners.
- **Borders.** `border_1`, `border_2`, per side (`border_b_1`), `border_color`, `border_dashed`.
  Hairlines: `border_1` is 1 logical px (2 device px on Retina); there is no 0.5 px helper, but
  `.border_b(px(0.5))` works.
- **Shadows.** `shadow_2xs/xs/sm/md/lg/xl/2xl`, `shadow_none`, or `.shadow(vec![BoxShadow {
  color, offset, blur_radius, spread_radius, inset }])` (`style.rs:349`). Several shadows stack;
  `inset` draws an inner shadow.
- **Backgrounds.** Solid `.bg(hsla)`; `linear_gradient(angle, linear_color_stop(c, 0.), ...)`
  (two stops, linear only), `pattern_slash(...)` (`gpui-pre-0.3.10/src/color.rs`). No radial or
  conic gradients, no images as CSS backgrounds (use `img()`).
- **Opacity.** `.opacity(f32)` on any element (and `Hsla::opacity`, `.alpha()` on colors).
- **Images and SVG.** `img(src)` (`elements/img.rs`, with object fit and grayscale), clipped by a rounded parent;
  `svg().path(...)` tinted with `text_color`. SVGs (icons) take `with_transformation(
  Transformation::scale/rotate/translate)` (`elements/svg.rs:214`).
- **Transforms on boxes.** None. A `div` cannot be scaled, rotated or translated at paint time.
  Move it with layout (`top`, `left`, margins, sizes) or opacity. gpui-base has a
  `MotionTransform` value type but nothing paints it on a `div` in 0.8.1.
- **Hover, active, focus.** `.hover(|s| s.bg(...))`, `.active(|s| ...)`, `.focus(|s| ...)`,
  `.group("row")` + `.group_hover("row", |s| ...)` to change a child when its row is hovered;
  `.cursor_pointer()`. These switch instantly (no transition) unless animated (section 5).
- **Container queries.** `elements/container_query.rs` changes layout by the element's width.

## 5. Animation

- **Theme motion tokens** (`gpui-component-0.8.1/src/theme/motion.rs`): `duration_fast` 120 ms,
  `duration_normal` 180 ms, `duration_slow` 280 ms; `easing_enter` cubic-bezier(0.16, 1, 0.3, 1),
  `easing_exit` (0.4, 0, 1, 1), `easing_move` (0.2, 0, 0, 1); `spring_control` (response 180 ms,
  critically damped), `spring_move` (280 ms, damping 0.85); distances 0.25 rem and 0.5 rem.
  Read them from `cx.theme().motion` so one place sets the app's motion feel. Note
  `easing_exit` is an ease-in curve: keep exits short (the skills ban ease-in on entering UI).
- **GPUI `with_animation`** (`gpui-pre-0.3.10/src/elements/animation.rs`):
  `el.with_animation(id, Animation::new(dur).with_easing(f), |el, t| el.opacity(t))`.
  `Animation::repeat()`, `with_animations` (a chain), `with_max_fps`. Easings: `linear`,
  `quadratic`, `ease_in_out`, `ease_out_quint()`, `bounce()`, `pulsating_between()`, or any
  `Fn(f32) -> f32`. It runs once per element id from mount; it is not a transition between
  states and cannot be interrupted.
- **GPUI springs.** `el.with_spring(...)` (same file) and `SpringConfig` (`src/spring.rs`):
  keyed by element id, keeps position and velocity when the target changes, so it is
  interruptible.
- **gpui-base motion** (`gpui-base-0.8.1/src/motion.rs`, `src/motion/`): `transition(id, target,
  Transition::new(d).easing(Easing::cubic_bezier(..)), window, cx)` returns the value to use this
  frame and retargets from the current value (interruptible, like a CSS transition); `spring(id,
  target, Spring::new(response).with_damping(..), ...)`; `animate_keyframes`; `Presence` (enter
  and exit for content that mounts and unmounts); `MotionReveal`; `Stagger`; `Sequence`.
  `Easing` supports `cubic_bezier`, `steps`, `linear_stops`. Anything that `Interpolate`s
  (`f32`, `Pixels`, `Hsla`, points, sizes) can animate.
- **Reduced motion.** `gpui_component::init` reads macOS "Reduce motion" into
  `cx.reduce_motion()` (`gpui-base-0.8.1/src/reduce_motion.rs`); gpui-base transitions, springs
  and presence honor it. Custom `with_animation` code must check `cx.reduce_motion()` itself.
- **Built-in component motion.** Dialogs have an `entrance`, `TextView::stream_fade` fades
  streamed text, `Collapsible` has a `motion_id`, `ShimmerText`, `Spinner`, `Skeleton`,
  `MessageScroller` jump-button transition, scrollbar fade.
- **Cost.** GPUI redraws the whole window each animation frame (immediate mode; cached views
  that did not change are reused). There is no compositor layer for "cheap" properties: an
  animated width re-runs layout for the window. Prefer opacity and color; keep animated regions
  small; never animate in long lists.

## 6. Windows (`WindowOptions`, `gpui-pre-0.3.10/src/platform.rs:2583`)

| Option | Use |
| --- | --- |
| `titlebar: Some(TitlebarOptions { title, appears_transparent, traffic_light_position })` | `appears_transparent: true` hides the title bar background so content runs to the top (full-height sidebar like Mail or Linear); `traffic_light_position: Some(point(px(x), px(y)))` moves the buttons. `None` gives no title bar. gpui-component's `TitleBar` sets (9, 9) and a 34 px bar |
| `window_background: WindowBackgroundAppearance::{Opaque, Transparent, Blurred}` | `Blurred` puts an `NSVisualEffectView` with material `UnderWindowBackground` behind the whole window (`gpui-pre-macos-0.3.10/src/window.rs`); paint a translucent `sidebar` color over it for a vibrant sidebar. The material is fixed, and the blur is per window, not per element |
| `window_bounds`, `window_min_size`, `display_id` | Size, position, minimum, screen |
| `kind: Normal / PopUp / Floating / AnchoredPopup` | Panels and native popups (`AnchoredPopup` for menus attached to a parent window) |
| `focus`, `show`, `is_movable`, `is_resizable`, `is_minimizable`, `app_owns_titlebar_drag` | Behavior; self-tests open with `focus: false` |
| `cx.set_window_appearance(...)`, `cx.set_activation_policy(...)` | Force light/dark chrome; Accessory = no Dock icon (menu bar mode) |

## 7. Icons (gpui-kit-assets 0.8.1, Lucide)

- `gpui_kit::assets::Assets` (the app's asset source) embeds the 101 default component icons
  (list in `gpui-kit-assets-0.8.1/default-icons.txt`), used as `Icon::new(IconName::Search)`
  (`gpui-component-0.8.1/src/icon.rs`). They include: arrows and chevrons, `Bell`, `Bot`,
  `BookOpen`, `Calendar`, `Check`, `CircleAlert/Check/User/X`, `Close`, `Copy`, `Cpu`,
  `Delete`, `Ellipsis` (and vertical), `ExternalLink`, `Eye`/`EyeOff`, `File`, `FileText`,
  `Folder*`, `Globe`, `HardDrive`, `Inbox`, `Info`, `Loader`, `LoaderCircle`, `Menu`, `Mic`,
  `Minus`, `Moon`, `Network`, `Palette`, `PanelLeft/Right/Bottom` (open/close), `Pause`, `Play`,
  `Plus`, `Redo`, `RefreshCw`, `Replace`, `Search`, `Settings`, `Settings2`, `SquareTerminal`,
  `Star*`, `Sun`, `ThumbsUp/Down`, `TriangleAlert`, `Undo`, `User`, window controls.
- The full Lucide 1.43 catalog (1,830 icons, `gpui_kit_assets::IconName`) is in the crate. Embed
  it all with `AllAssets` (+1 MiB), or better, add the few extra SVGs the app needs through its
  own `AssetSource` that falls back to `Assets` (about 2 KiB per icon; README of
  `gpui-kit-assets-0.8.1`). Icons draw at 16 px by default (`Sizable`), tinted by `text_color`,
  and can rotate (`Icon::rotate`) for spinners and chevrons.

## 8. What cannot be done (or not yet)

- No paint-time transforms on boxes: no `scale(0.97)` press effect, no slide by translate, no
  origin-aware scale for popovers. Use opacity, color and small layout offsets; SVG icons can
  scale and rotate.
- No per-element backdrop blur (`backdrop-filter`): only the window-level `Blurred` background.
  Translucent bars over scrolling content cannot blur what is under them.
- No blur filter, drop-shadow filter, `clip-path`, masks or blend modes on elements. Fades at a
  scroll edge are gradients (`linear_gradient` overlay) or `MessageScroller::with_bottom_fade`.
- Gradients are linear with two stops; no radial, conic or eased multi-stop gradients.
- No letter-spacing (tracking) control on text; no `font-variation-settings` (variable axes such
  as optical size are chosen by Core Text automatically for SF Pro).
- No continuous-corner (squircle) radii.
- No CSS-like transitions on `hover`/`active` styles; animate with gpui-base `transition`.
- Text selection only inside `TextView`, inputs and editors.
- One NSVisualEffect material; no vibrancy text (`NSAppearance` vibrant label colors).
- `WindowBounds` larger than the screen's visible frame are reduced by macOS (the self-test's
  `--size 1440x900` on a smaller screen gives a shorter window).

## 9. Verify

- `desktop --screenshot <dir> --theme both [--worst-case]` and read the PNGs.
- `docs/design/CHECKLIST.md` lists what to check in each screen.
