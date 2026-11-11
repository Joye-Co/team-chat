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

/// The side of the taskbar badge's bitmap. Windows scales it to the overlay size (16 px at
/// 100 %), so it is drawn at twice that.
pub const BADGE_SIDE: u32 = 32;

/// 3 x 5 digits and a plus sign, one row per byte, the left column in bit 2.
const GLYPHS: [[u8; 5]; 11] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
    [0b000, 0b010, 0b111, 0b010, 0b000],
];

/// What the taskbar badge says for `unread`: the count up to 99, then `99+` (the overlay
/// is 16 px, three digits do not fit). Nothing when nothing is unread.
pub fn badge_text(unread: u32) -> Option<String> {
    match unread {
        0 => None,
        1..=99 => Some(unread.to_string()),
        _ => Some("99+".into()),
    }
}

/// The taskbar badge for `unread` (the Windows twin of the Dock badge): a disc in `fill`
/// (B, G, R; the theme's red) with the count in white, as [`BADGE_SIDE`] square BGRA rows
/// from the top, straight alpha.
pub fn badge_bgra(unread: u32, fill: [u8; 3]) -> Option<Vec<u8>> {
    let text = badge_text(unread)?;
    let side = BADGE_SIDE as i32;
    let mut px = vec![0u8; (side * side * 4) as usize];
    // The disc, with its edge smoothed over 4 x 4 samples per pixel.
    let (center, radius) = (side as f32 / 2., side as f32 / 2.);
    for y in 0..side {
        for x in 0..side {
            let mut inside = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let dx = x as f32 + (sx as f32 + 0.5) / 4. - center;
                    let dy = y as f32 + (sy as f32 + 0.5) / 4. - center;
                    inside += (dx * dx + dy * dy <= radius * radius) as u32;
                }
            }
            let at = ((y * side + x) * 4) as usize;
            px[at..at + 3].copy_from_slice(&fill);
            px[at + 3] = (inside * 255 / 16) as u8;
        }
    }
    // The count: one or two glyphs at 4 px per cell, three at 2 px wide cells.
    let glyphs: Vec<usize> = text
        .bytes()
        .map(|b| if b == b'+' { 10 } else { (b - b'0') as usize })
        .collect();
    let (cell_w, cell_h, gap) = match glyphs.len() {
        1 => (4, 4, 0),
        2 => (3, 4, 2),
        _ => (2, 3, 2),
    };
    let n = glyphs.len() as i32;
    let width = n * 3 * cell_w + (n - 1) * gap;
    let (left, top) = ((side - width) / 2, (side - 5 * cell_h) / 2);
    for (i, glyph) in glyphs.iter().enumerate() {
        let x0 = left + i as i32 * (3 * cell_w + gap);
        for (row, bits) in GLYPHS[*glyph].iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) == 0 {
                    continue;
                }
                for y in 0..cell_h {
                    for x in 0..cell_w {
                        let (px_x, px_y) = (x0 + col * cell_w + x, top + row as i32 * cell_h + y);
                        let at = ((px_y * side + px_x) * 4) as usize;
                        px[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    Some(px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_says_the_count_up_to_99() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(7).as_deref(), Some("7"));
        assert_eq!(badge_text(99).as_deref(), Some("99"));
        assert_eq!(badge_text(100).as_deref(), Some("99+"));
        assert_eq!(badge_text(u32::MAX).as_deref(), Some("99+"));
        assert!(badge_bgra(0, [0, 0, 255]).is_none());
    }

    /// A red disc that is clear in the corners, with white digits that stay inside it and
    /// differ from count to count.
    #[test]
    fn the_badge_is_a_disc_with_the_count_in_white() {
        let side = BADGE_SIDE as usize;
        let white = |px: &[u8]| -> Vec<usize> {
            px.chunks(4)
                .enumerate()
                .filter(|(_, p)| p[..3] == [255, 255, 255])
                .map(|(i, _)| i)
                .collect()
        };
        let mut seen = Vec::new();
        for unread in [1, 8, 12, 99, 100] {
            let px = badge_bgra(unread, [0x30, 0x3b, 0xff]).expect("a badge");
            assert_eq!(px.len(), side * side * 4);
            assert_eq!(px[3], 0, "the corner is clear");
            let center = (side / 2 * side + 2) * 4;
            assert_eq!(
                px[center..center + 4],
                [0x30, 0x3b, 0xff, 255],
                "the disc is solid"
            );
            let ink = white(&px);
            assert!(!ink.is_empty(), "{unread} draws digits");
            for i in &ink {
                assert_eq!(
                    px[i * 4 + 3],
                    255,
                    "digits sit on the solid part of the disc"
                );
                let (x, y) = ((i % side) as i32, (i / side) as i32);
                assert!(
                    (2..30).contains(&x) && (6..26).contains(&y),
                    "{unread}: {x},{y}"
                );
            }
            assert!(!seen.contains(&ink), "{unread} looks like another count");
            seen.push(ink);
        }
    }

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
