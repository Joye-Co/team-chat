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
pub mod buttons;
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
pub use badge::{StatusKind, status_badge};
#[allow(unused_imports)]
pub use buttons::{
    PushSize, PushStyle, ToolbarItemKind, borderless_button, borderless_text, borderless_variant,
    disabled_push, popup_button, pull_down, push_button, square_button, square_pair, toolbar_item,
};
#[allow(unused_imports)]
pub use fields::{FieldSize, NativeTextField, SearchField};
#[allow(unused_imports)]
pub use form::{FormRow, GroupedForm, InlineValueField, inline_commit, inline_state};
#[allow(unused_imports)]
pub use inspector::{
    InspectorSection, ListRow, determinate_bar, inspector_column, label_value, label_value_with,
    steps_bar,
};
#[allow(unused_imports)]
pub use menu::{MenuRow, menu_separator, menu_surface, popover_surface};
#[allow(unused_imports)]
pub use segmented::{Segment, Segmented};
#[allow(unused_imports)]
pub use sheet::{
    AlertLayout, SheetLayout, SheetWidth, alert_button, alert_surface, native_sheet, sheet_buttons,
    sheet_surface,
};
#[allow(unused_imports)]
pub use sidebar::{SidebarRow, SidebarSection, floating_sidebar};
#[allow(unused_imports)]
pub use toggles::{NativeCheckbox, NativeRadio, NativeSwitch, SwitchSize};
#[allow(unused_imports)]
pub use tokens::{NativeColors, NativeType, emphasized, metrics, native_colors};
#[allow(unused_imports)]
pub use toolbar::{ToolbarGroup, ToolbarRow};

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
