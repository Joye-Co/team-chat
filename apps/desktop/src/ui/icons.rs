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
        Laptop,
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
        CornerDownLeft,
        Type,
        FaceSlightlySmiling,
        FaceSlightlySmilingPlus,
        Reply,
        FileImage,
        BellRing,
        Bold,
        Italic,
        Strikethrough,
        Link,
        ListOrdered,
        Quote,
        // v2 shell (1.1.0): 首页, 单独问, 改动, the rail toggle.
        House,
        MessageCircle,
        GitPullRequest,
        PanelRightOpen,
        // --- rail track (1.1.0): rail options, the 话题 destination of mode C,
        // ⌘K 交代或搜索 keys and 产出 thumbnails.
        SlidersHorizontal,
        TextAlignStart,
        Command,
        ChartLine,
        // --- end rail track
        // Desk track (1.1.0 首页): the 录屏 proof chip.
        Play,
        // panes track (1.1.0): 单独问 and 实时查看.
        CornerDownRight,
        MousePointer2,
        Maximize2,
        Minimize2,
        ArrowUpRight,
        ScreenShare,
        // --- cards v2 track ---
        Hourglass,
        Ban,
        GitMerge,
        FileX,
        Target,
        // --- end cards v2 track ---
        // --- v11/media: 产出 media previews ---
        AppWindow,
        AudioLines,
        Video,
        Pause,
        Volume2,
        Maximize,
        PictureInPicture2,
        Send,
        Upload,
        ZoomIn,
        ZoomOut,
        PanelLeft,
        RotateCw,
        Shield,
        Images,
        ListTree,
        ArrowUpDown,
        // --- end v11/media ---
        // --- v11/inc-diagrams: diagrams ---
        Workflow,
        Scan,
        Maximize2,
        // --- end v11/inc-diagrams ---
        // --- inc-refs: 引用提问 chips ---
        FileDiff,
        // --- end inc-refs ---
        // --- v11/pl-sf-symbols: the Lucide side of the SF Symbols spike ---
        Palette,
        Keyboard,
        Hand,
        // --- end v11/pl-sf-symbols ---
    ]
);

/// The app icon at the login size (88 pt, drawn from the 2x PNG).
pub const APP_ICON_88: &str = "brand/app-icon-88.png";
const APP_ICON_88_BYTES: &[u8] =
    include_bytes!("../../../../assets/brand/login/app-icon-88@2x.png");

/// The bare app icon at the login and 关于 sizes (72 pt, from the 2x PNG):
/// cropped to its squircle, with no baked shadow (DESIGN.md §8.3).
pub const APP_ICON_72: &str = "brand/app-icon-72.png";
const APP_ICON_72_BYTES: &[u8] =
    include_bytes!("../../../../assets/brand/login/app-icon-72@2x.png");
/// The dark-mode variant of [`APP_ICON_72`].
pub const APP_ICON_DARK_72: &str = "brand/app-icon-dark-72.png";
const APP_ICON_DARK_72_BYTES: &[u8] =
    include_bytes!("../../../../assets/brand/login/app-icon-dark-72@2x.png");

/// The asset source of the app: brand images, the extra icons, then
/// gpui-kit's defaults.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON_88 {
            return Ok(Some(Cow::Borrowed(APP_ICON_88_BYTES)));
        }
        if path == APP_ICON_72 {
            return Ok(Some(Cow::Borrowed(APP_ICON_72_BYTES)));
        }
        if path == APP_ICON_DARK_72 {
            return Ok(Some(Cow::Borrowed(APP_ICON_DARK_72_BYTES)));
        }
        if let Some(bytes) = crate::characters::tile::load(path) {
            return Ok(Some(Cow::Owned(bytes)));
        }
        if let Some(bytes) = crate::characters::assets::load(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(rest) = path.strip_prefix(FIT_PREFIX) {
            return self.load_fitted(rest);
        }
        let bytes = match ExtraIcons.load(path)? {
            Some(bytes) => Some(bytes),
            None => gpui_kit::assets::Assets.load(path)?,
        };
        Ok(bytes.map(|b| match path.starts_with("icons/") {
            true => Cow::Owned(thin_stroke(&b)),
            false => b,
        }))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.extend(crate::characters::assets::list(path).map(SharedString::from));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

impl AppAssets {
    /// `fit/<glyph>/<drawn>/icons/<name>.svg`: the icon drawn in a box of
    /// `drawn` pt with its 24-unit glyph at `glyph` pt (sizes in tenths).
    fn load_fitted(&self, rest: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let mut parts = rest.splitn(3, '/');
        let (Some(glyph), Some(drawn), Some(source)) = (parts.next(), parts.next(), parts.next())
        else {
            return Ok(None);
        };
        let (Ok(glyph), Ok(drawn)) = (glyph.parse::<f32>(), drawn.parse::<f32>()) else {
            return Ok(None);
        };
        let Some(bytes) = self.load(source)? else {
            return Ok(None);
        };
        let svg = String::from_utf8_lossy(&bytes);
        // The 24-unit glyph takes `glyph` of `drawn`: widen the viewBox
        // around its center. The stroke scales with it, as in the mockups.
        let side = 24. * drawn / glyph;
        let at = -(side - 24.) / 2.;
        let fitted = svg.replacen(
            "viewBox=\"0 0 24 24\"",
            &format!("viewBox=\"{at:.3} {at:.3} {side:.3} {side:.3}\""),
            1,
        );
        Ok(Some(Cow::Owned(fitted.into_bytes())))
    }
}

const FIT_PREFIX: &str = "fit/";

/// Lucide draws at stroke 2; the design uses 1.75 (§4.3), in viewBox units
/// like the mockups (`kit.mjs`), so an 18 pt glyph has a 1.31 pt line.
fn thin_stroke(svg: &[u8]) -> Vec<u8> {
    String::from_utf8_lossy(svg)
        .replacen("stroke-width=\"2\"", "stroke-width=\"1.75\"", 1)
        .into_bytes()
}

/// The icon of a gpui-component `Button` of side `button`, with its glyph
/// at `glyph` pt. The button always draws its icon at 75 % of its side
/// (24 in a 32 pt button), so the glyph comes in a widened viewBox that
/// puts the 24-unit Lucide glyph at `glyph` (18 in a 32 pt button, §4.2).
pub fn button_glyph(name: Lucide, glyph: Pixels, button: Pixels) -> Icon {
    Icon::empty().path(button_glyph_path(name, glyph, button))
}

fn button_glyph_path(name: Lucide, glyph: Pixels, button: Pixels) -> String {
    let drawn = button * 0.75;
    format!(
        "{FIT_PREFIX}{:.1}/{:.1}/{}",
        f32::from(glyph),
        f32::from(drawn),
        name.path()
    )
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
            let svg = AppAssets.load(&path).unwrap().expect("icon loads");
            let svg = std::str::from_utf8(&svg).unwrap().to_string();
            assert!(svg.contains("stroke-width=\"1.75\""), "{path}");
        }
    }

    #[test]
    fn button_glyphs_put_the_glyph_at_its_size() {
        let path = button_glyph_path(Lucide::Bell, gpui::px(18.), gpui::px(32.));
        let svg = AppAssets.load(&path).unwrap().unwrap();
        let svg = std::str::from_utf8(&svg).unwrap().to_string();
        // Drawn at 24 pt: the glyph is 18 pt when the viewBox is 32 units.
        assert!(
            svg.contains("viewBox=\"-4.000 -4.000 32.000 32.000\""),
            "{svg}"
        );
        assert!(svg.contains("stroke-width=\"1.75\""));
    }
}
