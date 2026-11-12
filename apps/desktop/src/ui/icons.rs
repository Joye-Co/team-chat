//! The app's icons are SF Symbols (NATIVE.md rule 2, §6): views name a
//! concept ([`Sym`]) at a §6 size ([`SymSize`]); [`AppAssets`] serves the
//! symbol drawn by AppKit (`ui::sf_symbols`).
//!
//! gpui-component draws some glyphs of its own (select chevrons, checkbox
//! check, dialog close, input clear, menu check and submenu chevron …) from
//! Lucide asset paths: [`AppAssets`] answers those paths with the concept's
//! SF Symbol too, so no Lucide glyph reaches the screen.

use std::borrow::Cow;

use gpui::{AssetSource, Hsla, Pixels, SharedString, Styled as _};
use gpui_component::Icon;

#[allow(unused_imports)]
pub use super::sf_symbols::{Sym, SymSize, Weight};

/// Deprecated: name a [`Sym`] concept. Kept so code that still names Lucide
/// icons compiles; every Lucide icon with a concept draws its SF Symbol.
pub use gpui_kit::assets::IconName as Lucide;

/// Raw Lucide asset paths (`lucide/icons/<name>.svg`), only for the
/// `sf-symbols` comparison scene and its gate report.
pub const LUCIDE_PREFIX: &str = "lucide/";

// Lucide is no longer drawn by the app (NATIVE.md rule 2): these stay only
// for the Lucide column of the `sf-symbols` comparison scene, served under
// [`LUCIDE_PREFIX`]. gpui-kit's 101 default icons cover the rest of it.
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        SquarePen,
        MessageCircle,
        Layers,
        Monitor,
        GitPullRequest,
        House,
        Hash,
        Users,
        Server,
        Keyboard,
        Hand,
        X,
        CircleDot,
        Clock,
        Image,
        Lock,
        UserPlus,
        Download,
        SlidersHorizontal,
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
        if let Some(svg) = super::sf_symbols::asset::load(path) {
            return Ok(Some(Cow::Owned(svg.to_vec())));
        }
        if let Some(svg) = super::sf_symbols::asset::load_lucide(path) {
            return Ok(Some(Cow::Owned(svg.to_vec())));
        }
        let path = path.strip_prefix(LUCIDE_PREFIX).unwrap_or(path);
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

/// The icon of a gpui-component `Button` of side `button`, with the symbol
/// at the §6 size for a `glyph` pt slot. The button always draws its icon at
/// 75 % of its side, so the symbol comes centered in a slot of that size.
pub fn button_glyph(name: impl Into<Sym>, glyph: Pixels, button: Pixels) -> Icon {
    button_sym(name.into(), SymSize::for_slot(glyph), button)
}

/// The icon of a gpui-component `Button` of side `button`, with the symbol
/// at a §6 size ([`SymSize::TOOLBAR`] for toolbar items).
pub fn button_sym(sym: Sym, size: SymSize, button: Pixels) -> Icon {
    let drawn = f32::from(button) * 0.75;
    Icon::empty().path(super::sf_symbols::asset::path(
        sym,
        SymSize::new(size.point, drawn, size.weight),
    ))
}

/// An icon at a slot size and color: the concept's SF Symbol at the §6 size
/// for that slot ([`SymSize::for_slot`]: 18 sidebar, 20 toolbar, 16 inline,
/// 14 inline small, 24 tab).
pub fn icon(name: impl Into<Sym>, size: Pixels, color: Hsla) -> Icon {
    sym(name.into(), SymSize::for_slot(size), color)
}

/// A concept at a §6 size, in a color.
pub fn sym(sym: Sym, size: SymSize, color: Hsla) -> Icon {
    sym.icon(size).text_color(color)
}

/// Draws every concept at the §6 sizes on a background thread, for the
/// largest screen scale. Call once at launch, on the main thread.
pub fn prewarm() {
    // `TC_SF_NO_PREWARM=1`: draw every symbol on first use (gate numbers).
    if std::env::var_os("TC_SF_NO_PREWARM").is_some() {
        return;
    }
    let s = super::sf_symbols::asset::screens_scale();
    super::sf_symbols::asset::prewarm(s);
}

/// The Lucide icon itself, for the comparison scene only.
pub fn lucide_raw(name: Lucide, size: Pixels, color: Hsla) -> Icon {
    Icon::empty()
        .path(format!("{LUCIDE_PREFIX}{}", name.path()))
        .size(size)
        .text_color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_lucide_icons_load_thin() {
        for name in [
            Lucide::Search,
            Lucide::SquarePen,
            Lucide::Ellipsis,
            Lucide::Layers,
            Lucide::Hand,
        ] {
            let path = format!("{LUCIDE_PREFIX}{}", name.path());
            let svg = AppAssets.load(&path).unwrap().expect("icon loads");
            let svg = std::str::from_utf8(&svg).unwrap().to_string();
            assert!(svg.contains("stroke-width=\"1.75\""), "{path}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn lucide_paths_and_concepts_draw_sf_symbols() {
        // gpui-component's own chevron and an app concept both come as the
        // AppKit mask in an SVG.
        for path in [
            Lucide::ChevronDown.path().to_string(),
            Lucide::Check.path().to_string(),
            super::super::sf_symbols::asset::path(Sym::Home, SymSize::SIDEBAR),
        ] {
            let svg = AppAssets.load(&path).unwrap().expect("loads");
            let svg = std::str::from_utf8(&svg).unwrap().to_string();
            assert!(svg.contains("data:image/png;base64,"), "{path}");
        }
    }

    #[test]
    fn old_slot_sizes_map_to_the_native_contexts() {
        assert_eq!(SymSize::for_slot(gpui::px(18.)), SymSize::SIDEBAR);
        assert_eq!(SymSize::for_slot(gpui::px(16.)), SymSize::INLINE);
        assert_eq!(SymSize::for_slot(gpui::px(14.)), SymSize::INLINE_SMALL);
        assert_eq!(SymSize::for_slot(gpui::px(20.)), SymSize::TOOLBAR);
        assert_eq!(SymSize::for_slot(gpui::px(12.)).point, 9.75);
    }
}
