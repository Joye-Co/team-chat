//! Our component layer on gpui-component (DESIGN.md v2.1 §7). Views build
//! controls, rows, avatars, chips, badges, the composer shell, overlays,
//! banners and empty states from here, so every screen looks the same.
//! Controls that gpui-component has (buttons, inputs, lists, menus, tabs,
//! dialogs, tooltips) are used from gpui-component and given the spec's
//! sizes and states here; this layer never re-implements them.
//!
//! | Module | Spec | What |
//! | --- | --- | --- |
//! | [`controls`] | §4.2, §7.3, §7.4 | buttons, icon buttons, text buttons, fields, selects |
//! | [`rows`] | §7.1, §7.6, §7.7 | the row helper, section headers, tabs, unread dot |
//! | [`chip`] | §7.8, §7.9 | status chips and labels, badges, role tag, kbd |
//! | [`avatar`] | §7.9 | people circles, bot tiles, the 群 glyph |
//! | [`composer`] | §7.2 | the composer shell, its tools and send button |
//! | [`overlay`] | §5, §7.12, §8.6 | popovers, menus, popup rows, toasts, banners |
//! | [`sheet`] | §7.12 | dialogs, alerts, the switcher |
//! | [`parts`] | §4.4, §8.5 | bands, cards, hairlines, empty states |
//! | [`truncate`] | §15 | one cut line with the full text in a tooltip |
//! | [`motion`] | §12 | durations, reduce motion, the busy spinner |
//! | [`gallery`] | §16 | every component in its states (self-test scene) |

pub mod avatar;
pub mod chip;
pub mod composer;
pub mod controls;
pub mod emoji;
pub mod gallery;
pub mod icons;
pub mod motion;
pub mod overlay;
pub mod parts;
pub mod rows;
pub mod sheet;
pub mod truncate;

// The screens build from these; some are not used by every screen yet.
#[allow(unused_imports)]
pub use avatar::{GroupGlyph, GroupTile, MemberAvatar};
#[allow(unused_imports)]
pub use chip::{BadgeKind, ChipStyle, CountBadge, Kbd, RoleTag, StatusChip};
#[allow(unused_imports)]
pub use composer::{ComposerShell, composer_tool, send_button};
#[allow(unused_imports)]
pub use controls::{
    ButtonKind, ButtonSize, ButtonSpec, FieldKind, TextField, button, field_help, field_label,
    icon_button, icon_button_sm, placeholder_input, primary_button, secondary_button,
    select_trigger, text_button,
};
pub use icons::{AppAssets, Lucide, button_glyph, icon};
pub use motion::spinner;
#[allow(unused_imports)]
pub use overlay::{
    Banner, Toast, Tone, float_surface, mention_surface, menu_row, menu_separator, menu_surface,
    overlay_surface, popup_row, return_hint,
};
#[allow(unused_imports)]
pub use parts::{
    Band, EmptyState, KbdHint, capsule, card, card_padding, column_edge, column_start_edge,
    floating, hairline, header_band, toolbar_band, vhairline,
};
#[allow(unused_imports)]
pub use rows::{
    RowKind, RowSurface, SectionHeader, disabled_row, list_row, panel_label, row_count, tab,
    unread_dot,
};
pub use sheet::SheetExt;
pub use truncate::{row_tip, truncating};
