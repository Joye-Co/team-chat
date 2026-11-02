//! Our component layer on gpui-component (DESIGN.md). Views build member
//! avatars, chips, badges, keyboard hints, section headers, empty states,
//! bands, cards and toolbars from here, so every screen looks the same.
//! Controls that gpui-component has (buttons, inputs, lists, menus, tabs,
//! dialogs, tooltips) are used from gpui-component and styled through the
//! theme; this layer never re-implements them.

pub mod avatar;
pub mod chip;
pub mod icons;
pub mod motion;
pub mod parts;

pub use avatar::{GroupTile, MemberAvatar};
pub use chip::{BadgeKind, CountBadge, StatusChip};
pub use icons::{AppAssets, Lucide, icon};
pub use parts::{
    Band, EmptyState, KbdHint, SectionHeader, capsule, card, floating, hairline, icon_button,
    text_button, toolbar_band, vhairline,
};
