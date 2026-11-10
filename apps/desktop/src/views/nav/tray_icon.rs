//! The pixels of the Windows notification-area icon, from the brand's template images
//! (black with alpha, as the macOS menu bar wants them). No system API here, so the tests
//! run on every platform; the icon itself is made in `status_item_win.rs`.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

/// Decodes a template PNG and paints it white (for a dark taskbar) or black, keeping its
/// alpha: (width, height, BGRA rows from the top, straight alpha).
pub fn tinted_bgra(png: &[u8], white: bool) -> Option<(u32, u32, Vec<u8>)> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    let level = if white { 255 } else { 0 };
    let bgra = image
        .pixels()
        .flat_map(|p| [level, level, level, p.0[3]])
        .collect();
    Some((width, height, bgra))
}

/// The byte length of a 1-bit mask bitmap: every row is padded to 16 bits.
pub fn and_mask_len(width: u32, height: u32) -> usize {
    (width as usize).div_ceil(16) * 2 * height as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &[u8] =
        include_bytes!("../../../../../assets/brand/menubar/StatusItemTemplate@2x.png");
    const WAITING: &[u8] =
        include_bytes!("../../../../../assets/brand/menubar/StatusItemWaitingTemplate@2x.png");

    #[test]
    fn the_template_keeps_its_shape_in_either_color() {
        let (w, h, white) = tinted_bgra(ICON, true).unwrap();
        let (_, _, black) = tinted_bgra(ICON, false).unwrap();
        assert_eq!((w, h), (32, 32));
        assert_eq!(white.len(), (w * h * 4) as usize);
        let alpha = |px: &[u8]| px.chunks(4).map(|p| p[3]).collect::<Vec<_>>();
        assert_eq!(alpha(&white), alpha(&black));
        assert!(alpha(&white).iter().any(|a| *a > 200), "something is drawn");
        assert!(
            alpha(&white).iter().any(|a| *a == 0),
            "and something is clear"
        );
        assert!(white.chunks(4).all(|p| p[..3] == [255, 255, 255]));
        assert!(black.chunks(4).all(|p| p[..3] == [0, 0, 0]));
    }

    #[test]
    fn the_waiting_image_draws_more_than_the_plain_one() {
        let drawn = |png| {
            let (_, _, px) = tinted_bgra(png, true).unwrap();
            px.chunks(4).filter(|p| p[3] > 0).count()
        };
        assert_ne!(drawn(ICON), drawn(WAITING));
        assert!(tinted_bgra(b"not a png", true).is_none());
    }

    #[test]
    fn mask_rows_are_padded_to_words() {
        assert_eq!(and_mask_len(32, 32), 4 * 32);
        assert_eq!(and_mask_len(16, 16), 2 * 16);
        assert_eq!(and_mask_len(20, 2), 4 * 2);
    }
}
