use gpui::{Pixels, RenderImage};
use image::{Frame, ImageFormat};

// GPUI draws images in BGRA order.
// https://github.com/ImperaLuna/zed/tree/nova/v0.233.10/crates/gpui
const RED: usize = 0;
const BLUE: usize = 2;

pub fn device_side(side: Pixels, scale_factor: f32) -> u32 {
    let device = (f32::from(side) * scale_factor).ceil();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to a small positive size first"
    )]
    let device = device.clamp(1.0, f32::from(u16::MAX)) as u32;
    device
}

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

    #[test]
    fn device_sides_round_up() {
        assert_eq!(device_side(px(24.0), 1.0), 24);
        assert_eq!(device_side(px(24.0), 1.25), 30);
        assert_eq!(device_side(px(24.0), 1.1), 27);
    }
}
