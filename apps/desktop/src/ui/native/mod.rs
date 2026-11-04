//! The native component kit (docs/design/NATIVE.md §2–§5, §7): the controls
//! and surfaces of a macOS 26 (Tahoe) app, built from gpui-component and
//! styled through the theme. Screens build their toolbars, sidebars, forms,
//! sheets and lists from here so the whole app reads as a Mac client.
//!
//! | Module | Spec | What |
//! | --- | --- | --- |
//! | [`tokens`] | §3, §4 | colors (light, dark), text styles, metrics, shadows |
//! | [`toolbar`] | §2, §5.1 | toolbar row (title, subtitle), glass groups |
//! | [`badge`] | COMPLETENESS §2.12 | the outline status badge |
//! | [`buttons`] | §5.1–§5.3 | push, pop-up, pull-down, square pair, borderless, toolbar items |
//! | [`sidebar`] | §2, §9.1 | floating panel, section headers, 32 pt rows |
//! | [`form`] | §5.6, §8 | grouped forms, form rows, inline value field |
//! | [`fields`] | §5.6 | standalone text field, search field |
//! | [`toggles`] | §5.4 | switch, checkbox, radio |
//! | [`segmented`] | §5.5 | segmented control |
//! | [`menu`] | §7.3 | menu surface and rows, popover surface |
//! | [`sheet`] | §7.1, §7.2 | sheets on gpui-component's dialog, alerts |
//! | [`inspector`] | §9.3, §5.7 | inspector sections, label/value rows, list rows, determinate bar |
//! | [`scene`] | §11 | the `native-kit` self-test scenes |

pub mod badge;
pub mod blocks;
pub mod buttons;
pub mod feedback;
pub mod fields;
pub mod form;
pub mod inspector;
pub mod menu;
pub mod scene;
pub mod segmented;
pub mod sheet;
pub mod sidebar;
pub mod toggles;
pub mod tokens;
pub mod toolbar;

#[allow(unused_imports)]
pub use badge::{
    Badge, BadgeState, BadgeVariant, CountPill, KeyCap, StatusBadge, StatusKind, status_badge,
};
#[allow(unused_imports)]
pub use blocks::{
    Card, Column, LIST_TITLE_INSET, ListRow, SectionTitle, card_surface, description_row,
    group_label, list_frame, media_box, separator, table,
};
#[allow(unused_imports)]
pub use buttons::{
    Glyph, IconSize, PushSize, PushStyle, ToolbarItemKind, borderless_button, borderless_text,
    borderless_variant, chip_button, disabled_push, ghost_button, icon_button, icon_button_bare,
    icon_button_styled, icon_button_sym, loading_push, popup_button, pull_down, push_button,
    row_button, screen_button, select_button, square_button, square_pair, tile_button,
    toolbar_item, toolbar_item_sym, toolbar_tab,
};
#[allow(unused_imports)]
pub use feedback::{
    Alert, AlertTone, Bone, Empty, EmptyLine, SkeletonKind, icon_box, progress, progress_steps,
    skeleton, skeleton_rows, toast_surface,
};
#[allow(unused_imports)]
pub use fields::{FieldSize, NativeTextField, SearchField, field_edge, field_edge_inset};
#[allow(unused_imports)]
pub use form::{FormRow, GroupedForm, InlineValueField, inline_commit, inline_state};
#[allow(unused_imports)]
pub use inspector::{
    CompactRow, InspectorSection, determinate_bar, inspector_column, label_value, label_value_with,
    pane_fill, right_pane, steps_bar,
};
#[allow(unused_imports)]
pub use menu::{MenuRow, menu_label, menu_separator, menu_surface, popover_surface};
#[allow(unused_imports)]
pub use segmented::{Segment, Segmented};
#[allow(unused_imports)]
pub use sheet::{
    AlertLayout, SheetLayout, SheetWidth, alert_button, alert_surface, native_sheet,
    native_sheet_keyboard, sheet_buttons, sheet_status_error, sheet_surface,
};
#[allow(unused_imports)]
pub use sidebar::{
    RightColumn, SidebarFooter, SidebarGroup, SidebarMedia, SidebarRow, SidebarSection,
    floating_sidebar,
};
#[allow(unused_imports)]
pub use toggles::{NativeCheckbox, NativeRadio, NativeSwitch, SwitchSize};
#[allow(unused_imports)]
pub use tokens::{NativeColors, NativeType, emphasized, metrics, native_colors};
#[allow(unused_imports)]
pub use toolbar::{ToolbarGroup, ToolbarRow, toolbar_slot};

/// The one truncating label (T1-02): `…` at the end or in the middle, the
/// full text in a tooltip when cut.
#[allow(unused_imports)]
pub use crate::ui::truncate::{Cut, TruncText};

/// The busy spinner (§5.7): for unknown waits, never with "加载中…" text.
#[allow(unused_imports)]
pub use crate::ui::motion::spinner;

// --- track cards: the card parts the kit lacked (ui/native/cards.rs) ---
pub mod cards;
// --- end track cards ---
// --- v12/overlays ---
/// Overlay parts the kit lacks: the Tahoe alert dialog, the Spotlight panel
/// (§9.7), the bordered list of a sheet and the app icon.
pub mod overlays;
// --- end v12/overlays ---
// --- v13/ux-pages ---
/// Page parts (COMPLETENESS.md §2.3–§2.18): section headers, framed lists,
/// KPI cards, StatusBadge, Alert, Empty and skeleton states.
pub mod page;
// --- end v13/ux-pages ---
