//! Our component layer on gpui-component (DESIGN.md v2.1 §7). Views build
//! controls, rows, avatars, chips, badges, the composer shell, overlays,
//! banners and empty states from here, so every screen looks the same.
//! Controls that gpui-component has (buttons, inputs, lists, menus, tabs,
//! dialogs, tooltips) are used from gpui-component and given the spec's
//! sizes and states here; this layer never re-implements them.
//!
//! | Module | Spec | What |
//! | --- | --- | --- |
//! | [`controls`] | §1.9 | icon button names, input placeholders, field label and help |
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
//! | [`web_embed`] | — | a live 产出 page inside the window (native web view) |

pub mod avatar;
pub mod chip;
pub mod composer;
pub mod controls;
pub mod emoji;
pub mod gallery;
pub mod icons;
pub mod motion;
// --- v12/kit: the native component kit (docs/design/NATIVE.md) ---
pub mod native;
// --- end v12/kit ---
pub mod overlay;
pub mod parts;
pub mod rows;
pub mod sheet;
// --- v11/pl-sf-symbols: SF Symbols spike (POLISH-SPEC §10.2) ---
pub mod sf_symbols;
// --- end v11/pl-sf-symbols ---
pub mod truncate;
pub mod web_embed;

// The screens build from these; some are not used by every screen yet.
#[allow(unused_imports)]
pub use avatar::{GroupGlyph, GroupTile, MemberAvatar, Ring};
#[allow(unused_imports)]
pub use chip::{BadgeKind, ChipStyle, CountBadge, JobTag, Kbd, RoleTag, StatusChip};
#[allow(unused_imports)]
pub use composer::{ComposerShell, composer_tool, send_button};
#[allow(unused_imports)]
pub use controls::{NamedButton, field_help, field_label, placeholder_input};
pub use icons::{AppAssets, Sym, button_glyph, icon};
/// The one Button's variant and size (`ui::native::buttons`).
#[allow(unused_imports)]
pub use native::{PushSize, PushStyle};
// --- v12/symbols: the SF Symbols API (NATIVE.md §6) ---
#[allow(unused_imports)]
pub use icons::{Lucide, SymSize, sym};
// --- end v12/symbols ---
pub use motion::spinner;
#[allow(unused_imports)]
pub use overlay::{
    Banner, Toast, Tone, float_surface, mention_surface, menu_row, menu_separator, menu_surface,
    overlay_surface, popup_row, return_hint,
};
#[allow(unused_imports)]
pub use parts::{
    Band, EmptyState, KbdHint, capsule, card, card_footer, card_padding, column_edge,
    column_start_edge, divided, floating, hairline, header_band, inset_row, rail_card,
    rail_section, rail_stack, toolbar_band, vhairline,
};
#[allow(unused_imports)]
pub use rows::{
    RowKind, RowSurface, SectionHeader, disabled_row, list_row, panel_label, row_count, unread_dot,
};
pub use sheet::SheetExt;
#[allow(unused_imports)]
pub use truncate::{Cut, TruncText, cut_line, row_tip, tip_when_cut, truncating};
