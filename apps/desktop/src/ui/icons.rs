//! Lucide icons (DESIGN.md §9). gpui-kit embeds 101 component icons; the
//! app's own [`AppAssets`] adds the rest of the set the design uses and falls
//! back to `gpui_kit::assets::Assets` (never `AllAssets`).

use std::borrow::Cow;

use gpui::{AssetSource, Hsla, Pixels, SharedString, Styled as _};
use gpui_component::Icon;

pub use gpui_kit::assets::IconName as Lucide;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        SquarePen,
        BellOff,
        Paperclip,
        Image,
        AtSign,
        TextQuote,
        FileCode,
        GitBranch,
        CircleDot,
        CircleQuestionMark,
        CircleDashed,
        Circle,
        WifiOff,
        Monitor,
        Users,
        UsersRound,
        User,
        UserPlus,
        Server,
        ChartColumn,
        Activity,
        Code,
        LogOut,
        MessageSquarePlus,
        Clock,
        X,
        Lock,
        Download,
        Layers,
        List,
        ListChecks,
        Table,
        Terminal,
        RotateCcw,
        Pencil,
        Trash,
        SearchX,
        Hash,
    ]
);

/// The app icon at the login size (88 pt, drawn from the 2x PNG).
pub const APP_ICON_88: &str = "brand/app-icon-88.png";
const APP_ICON_88_BYTES: &[u8] =
    include_bytes!("../../../../assets/brand/login/app-icon-88@2x.png");

/// The asset source of the app: brand images, the extra icons, then
/// gpui-kit's defaults.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON_88 {
            return Ok(Some(Cow::Borrowed(APP_ICON_88_BYTES)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

/// A Lucide icon at a size and color.
pub fn icon(name: Lucide, size: Pixels, color: Hsla) -> Icon {
    Icon::new(name).size(size).text_color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_design_icon_loads() {
        for name in [
            Lucide::Search,
            Lucide::SquarePen,
            Lucide::CircleQuestionMark,
            Lucide::Eye,
            Lucide::Bot,
            Lucide::WifiOff,
            Lucide::MessageSquarePlus,
            Lucide::Settings,
            Lucide::PanelRight,
            Lucide::Ellipsis,
        ] {
            let path = name.path();
            assert!(
                AppAssets.load(&path).unwrap().is_some(),
                "missing icon {path}"
            );
        }
    }
}
