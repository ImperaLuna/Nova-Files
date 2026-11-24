use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, ImageFormat, ImageReader, RgbaImage};

const MAX_BYTES: u64 = 192 * 1024 * 1024;
const MAX_FRAMES: usize = 2000;
const MIN_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_DELAY: Duration = Duration::from_millis(100);

pub struct Frame {
    pub image: RgbaImage,
    pub delay: Duration,
}

pub fn frames(path: &Path, width: u32, height: u32) -> Option<Vec<Frame>> {
    let format = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .format()?;
    let reader = || File::open(path).ok().map(BufReader::new);
    let frames = match format {
        ImageFormat::Gif => GifDecoder::new(reader()?).ok()?.into_frames(),
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(reader()?).ok()?;
            if !decoder.has_animation() {
                return None;
            }
            decoder.into_frames()
        }
        ImageFormat::Png => {
            let decoder = PngDecoder::new(reader()?).ok()?;
            if !decoder.is_apng().ok()? {
                return None;
            }
            decoder.apng().ok()?.into_frames()
        }
        _ => return None,
    };
    let mut kept = Vec::new();
    let mut bytes = 0_u64;
    for frame in frames.take(MAX_FRAMES) {
        let Ok(frame) = frame else {
            break;
        };
        let delay = Duration::from(frame.delay());
        let image = fitted(frame.into_buffer(), width, height);
        bytes += u64::from(image.width()) * u64::from(image.height()) * 4;
        if bytes > MAX_BYTES {
            break;
        }
        kept.push(Frame {
            image,
            delay: if delay < MIN_DELAY {
                DEFAULT_DELAY
            } else {
                delay
            },
        });
    }
    (kept.len() > 1).then_some(kept)
}

fn fitted(image: RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.width() <= width && image.height() <= height {
        return image;
    }
    let scale = (f64::from(width) / f64::from(image.width()))
        .min(f64::from(height) / f64::from(image.height()));
    let side = |side: u32| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a scaled-down side, between 1 and the original"
        )]
        let scaled = (f64::from(side) * scale).round().max(1.0) as u32;
        scaled
    };
    image::imageops::resize(
        &image,
        side(image.width()),
        side(image.height()),
        FilterType::Triangle,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Rgba};
    use nova_files_test_folder::TestFolder;

    fn gif(path: &Path, frames: u32, side: u32) {
        let file = File::create(path).unwrap();
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for index in 0..frames {
            let shade = u8::try_from(index * 40 % 256).unwrap();
            let image = RgbaImage::from_pixel(side, side, Rgba([shade, 0, 0, 255]));
            let frame = image::Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(70, 1));
            encoder.encode_frame(frame).unwrap();
        }
    }

    #[test]
    fn a_moving_gif_gives_every_frame_scaled_to_fit() {
        let folder = TestFolder::new("moving-gif");
        let path = folder.join("spin.gif");
        gif(&path, 3, 40);
        let frames = frames(&path, 20, 30).unwrap();
        assert_eq!(frames.len(), 3);
        assert!(
            frames
                .iter()
                .all(|frame| frame.image.dimensions() == (20, 20))
        );
        assert_eq!(frames[0].delay, Duration::from_millis(70));
    }

    #[test]
    fn a_still_gif_is_not_moving() {
        let folder = TestFolder::new("still-gif");
        let path = folder.join("one.gif");
        gif(&path, 1, 8);
        assert!(frames(&path, 20, 20).is_none());
    }
}
