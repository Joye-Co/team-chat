//! Pictures on the clipboard, where Windows differs from the Mac.
//!
//! - A copied picture: GPUI's Windows clipboard writes only the registered `PNG` format.
//!   Programs that read bitmaps only (Paint's older versions, several chat apps) then
//!   paste nothing, while on the Mac a copied picture pastes anywhere. [`write_image`]
//!   adds the bitmap (`CF_DIB`) beside it.
//! - A pasted picture: a clipboard that holds only a bitmap (PrintScreen, many apps)
//!   arrives as an uncompressed BMP, about 8 MB for a 1920 × 1080 screenshot.
//!   [`png_of_bmp`] makes it the PNG the Mac's screenshot is.

use gpui::{App, ClipboardItem, Image, ImageFormat};

/// Puts `image` on the clipboard so that every program can paste it.
pub fn write_image(image: &Image, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_image(image));
    #[cfg(target_os = "windows")]
    if let Some(dib) = dib_of(image.bytes(), image.format) {
        windows::add_bitmap(&dib);
    }
}

fn decoder(format: ImageFormat) -> Option<image::ImageFormat> {
    Some(match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        _ => return None,
    })
}

/// A BMP file's picture as PNG bytes; `None` when it cannot be read.
pub fn png_of_bmp(bmp: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory_with_format(bmp, image::ImageFormat::Bmp).ok()?;
    let mut png = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// The picture as a device-independent bitmap (the content of `CF_DIB`): a
/// `BITMAPINFOHEADER` for 32-bit pixels, then the rows bottom-up as B, G, R, 0. A
/// bitmap has no transparency every reader agrees on, so see-through pixels are laid on
/// white, as a pasted picture shows on a page.
#[cfg(any(target_os = "windows", test))]
fn dib_of(bytes: &[u8], format: ImageFormat) -> Option<Vec<u8>> {
    let picture = image::load_from_memory_with_format(bytes, decoder(format)?)
        .ok()?
        .into_rgba8();
    let (width, height) = picture.dimensions();
    let pixels = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let mut dib = Vec::with_capacity(40 + pixels);
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes()); // positive: bottom-up
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    dib.extend_from_slice(&(pixels as u32).to_le_bytes());
    dib.extend_from_slice(&[0u8; 16]); // resolution and palette: none
    for row in picture.rows().rev() {
        for pixel in row {
            let [r, g, b, a] = pixel.0;
            let on_white = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
            dib.extend_from_slice(&[on_white(b), on_white(g), on_white(r), 0]);
        }
    }
    Some(dib)
}

#[cfg(target_os = "windows")]
pub mod windows {
    use ::windows::Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL},
        System::{
            DataExchange::{
                CloseClipboard, GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };

    /// `CF_DIB`.
    const BITMAP: u32 = 8;

    /// A number that changes whenever the clipboard's content does.
    pub fn sequence() -> u32 {
        // SAFETY: reads a counter of the window station.
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Adds `dib` to what is on the clipboard now, as its bitmap. Call it right after the
    /// picture's other formats were written.
    pub(super) fn add_bitmap(dib: &[u8]) {
        // SAFETY: copies `dib` into movable global memory, which the clipboard owns once
        // `SetClipboardData` took it and which is freed here when it did not.
        unsafe {
            if OpenClipboard(None).is_err() {
                return;
            }
            if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, dib.len()) {
                let to = GlobalLock(memory);
                let taken = !to.is_null() && {
                    std::ptr::copy_nonoverlapping(dib.as_ptr(), to.cast::<u8>(), dib.len());
                    let _ = GlobalUnlock(memory);
                    SetClipboardData(BITMAP, Some(HANDLE(memory.0))).is_ok()
                };
                if !taken {
                    let _ = GlobalFree(Some(HGLOBAL(memory.0)));
                }
            }
            let _ = CloseClipboard();
        }
    }

    #[cfg(test)]
    mod tests {
        use ::windows::Win32::System::{
            DataExchange::{EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable},
            Memory::GlobalSize,
        };

        use super::*;

        /// The bitmap lands on the real clipboard beside what is there, and counts as a
        /// change of its content.
        #[test]
        fn a_bitmap_is_added_to_the_clipboard() {
            let dib = vec![7u8; 40 + 16];
            // SAFETY: empties, reads and closes the clipboard of the test's desktop.
            unsafe {
                if OpenClipboard(None).is_err() {
                    eprintln!("no clipboard in this session");
                    return;
                }
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
                let before = sequence();
                add_bitmap(&dib);
                assert_ne!(sequence(), before);
                assert!(IsClipboardFormatAvailable(BITMAP).is_ok());
                OpenClipboard(None).expect("open the clipboard");
                let data = GetClipboardData(BITMAP).expect("the bitmap");
                assert_eq!(GlobalSize(HGLOBAL(data.0)), dib.len());
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let picture = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));
        let mut bytes = Vec::new();
        picture
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn a_bitmap_has_its_header_and_bottom_up_bgr_rows() {
        let mut picture = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        picture.put_pixel(0, 1, image::Rgba([200, 100, 50, 255]));
        let mut bytes = Vec::new();
        picture
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let dib = dib_of(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(dib.len(), 40 + 16);
        assert_eq!(&dib[0..4], &40u32.to_le_bytes());
        assert_eq!(&dib[4..8], &2i32.to_le_bytes());
        assert_eq!(&dib[8..12], &2i32.to_le_bytes());
        assert_eq!(&dib[14..16], &32u16.to_le_bytes());
        // The first stored row is the picture's last one: its left pixel as B, G, R.
        assert_eq!(&dib[40..44], &[50, 100, 200, 0]);
        assert_eq!(&dib[48..52], &[30, 20, 10, 0]);
    }

    #[test]
    fn see_through_pixels_of_a_bitmap_lie_on_white() {
        let dib = dib_of(&png(1, 1, [0, 0, 0, 0]), ImageFormat::Png).unwrap();
        assert_eq!(&dib[40..44], &[255, 255, 255, 0]);
        let dib = dib_of(&png(1, 1, [0, 0, 0, 128]), ImageFormat::Png).unwrap();
        assert_eq!(&dib[40..44], &[127, 127, 127, 0]);
    }

    #[test]
    fn a_pasted_bmp_becomes_a_png_of_the_same_picture() {
        let picture = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
        let mut bmp = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .unwrap();
        let png = png_of_bmp(&bmp).unwrap();
        let back = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(back.dimensions(), (3, 2));
        assert_eq!(back.get_pixel(2, 1).0, [1, 2, 3, 255]);
        assert!(png_of_bmp(b"not a picture").is_none());
    }
}
