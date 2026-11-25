use gpui::RenderImage;
use image::{Frame, ImageFormat};

// GPUI draws images in BGRA order.
// https://github.com/ImperaLuna/zed/tree/2033650502b1d0ec524b8913bf5a01b9463bed5d/crates/gpui
const RED: usize = 0;
const BLUE: usize = 2;

pub fn shrink(png: &[u8], side: u32) -> Option<RenderImage> {
    let decoded = image::load_from_memory_with_format(png, ImageFormat::Png).ok()?;
    let mut pixels = decoded.thumbnail(side, side).into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(RED, BLUE);
    }
    Some(RenderImage::new([Frame::new(pixels)]))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::device::device_side;
    use gpui::px;
    use image::{Rgba, RgbaImage};

    use super::*;

    #[test]
    fn shrinks_to_the_slot_and_swaps_to_bgra() {
        let mut png = Vec::new();
        RgbaImage::from_pixel(128, 64, Rgba([10, 20, 30, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();

        let image = shrink(&png, device_side(px(24.0), 2.0)).unwrap();

        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (48, 24));
        assert_eq!(&image.as_bytes(0).unwrap()[..4], [30, 20, 10, 255]);
        assert!(shrink(b"not a png", 48).is_none());
    }
}
