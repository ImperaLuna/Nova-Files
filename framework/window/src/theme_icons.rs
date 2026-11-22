use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{RenderImage, SvgRenderer};
use nova_files_api::{IconFormat, ThemeIcon};

use nova_files_shell::shrink::shrink;

// GPUI's renderer draws at twice the scale it is asked for.
// https://github.com/ImperaLuna/zed/tree/915fb0d79b03de29a8a15beb0fef11e0e465d8ec/crates/gpui
pub const RENDERER_SCALE: f32 = 2.0;
const ROUNDING: f32 = 0.5;

#[derive(Default)]
pub struct ThemeIcons {
    side: u32,
    made: HashMap<PathBuf, Arc<RenderImage>>,
    pending: HashSet<PathBuf>,
}

impl ThemeIcons {
    pub fn get(&self, path: &Path) -> Option<&Arc<RenderImage>> {
        self.made.get(path)
    }

    pub fn start(&mut self, icon: &ThemeIcon) -> bool {
        !self.made.contains_key(&icon.path) && self.pending.insert(icon.path.clone())
    }

    #[must_use]
    pub fn finish(
        &mut self,
        path: PathBuf,
        side: u32,
        image: Option<Arc<RenderImage>>,
    ) -> Option<Arc<RenderImage>> {
        if side != self.side {
            return image;
        }
        self.pending.remove(&path);
        let image = image?;
        self.made.insert(path, image)
    }

    #[must_use]
    pub fn resize(&mut self, side: u32) -> Option<Vec<Arc<RenderImage>>> {
        if side == self.side {
            return None;
        }
        self.side = side;
        self.pending.clear();
        Some(self.made.drain().map(|(_, image)| image).collect())
    }
}

pub fn make(icon: &ThemeIcon, side: u32, svg: &SvgRenderer) -> Option<Arc<RenderImage>> {
    match icon.format {
        IconFormat::Png => shrink(&icon.bytes, side).map(Arc::new),
        IconFormat::Svg => {
            let tree = usvg::Tree::from_data(&icon.bytes, &usvg::Options::default()).ok()?;
            let longest = tree.size().width().max(tree.size().height());
            if !longest.is_normal() {
                return None;
            }
            #[expect(
                clippy::cast_precision_loss,
                reason = "an icon's side is a few hundred pixels at most"
            )]
            let wanted = side as f32 + ROUNDING;
            let scale = wanted / (longest * RENDERER_SCALE);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the renderer rounds the side down the same way"
            )]
            let drawn = (longest * scale * RENDERER_SCALE) as u32;
            if drawn != side {
                return None;
            }
            svg.render_single_frame(&icon.bytes, scale).ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{ImageFormat, Rgba, RgbaImage};

    use super::*;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#f00"/></svg>"##;

    fn icon(format: IconFormat, bytes: Vec<u8>) -> ThemeIcon {
        ThemeIcon {
            path: PathBuf::from("/icons/a"),
            format,
            bytes: bytes.into(),
        }
    }

    fn side_of(image: &RenderImage) -> (i32, i32) {
        let size = image.size(0);
        (size.width.0, size.height.0)
    }

    #[test]
    fn svgs_and_pngs_are_made_at_the_side_asked_for() {
        let svg = SvgRenderer::new(Arc::new(()));
        let drawn = make(&icon(IconFormat::Svg, SQUARE.into()), 44, &svg).unwrap();
        assert_eq!(side_of(&drawn), (44, 44));
        assert_eq!(&drawn.as_bytes(0).unwrap()[..4], [0, 0, 255, 255]);
        let big = SQUARE.replace("16", "48");
        let shrunk = make(&icon(IconFormat::Svg, big.clone().into()), 44, &svg).unwrap();
        assert_eq!(side_of(&shrunk), (44, 44));
        let odd = make(&icon(IconFormat::Svg, big.into()), 28, &svg).unwrap();
        assert_eq!(side_of(&odd), (28, 28));
        let huge = SQUARE.replace("16", "30000");
        let made = make(&icon(IconFormat::Svg, huge.into()), 22, &svg).unwrap();
        assert_eq!(side_of(&made), (22, 22));

        let mut png = Vec::new();
        RgbaImage::from_pixel(48, 48, Rgba([1, 2, 3, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let png = make(&icon(IconFormat::Png, png), 22, &svg).unwrap();
        assert_eq!(side_of(&png), (22, 22));

        assert!(make(&icon(IconFormat::Svg, b"not svg".to_vec()), 22, &svg).is_none());
        assert!(make(&icon(IconFormat::Png, b"not png".to_vec()), 22, &svg).is_none());
    }

    #[test]
    fn an_icon_is_made_once_per_size() {
        let mut icons = ThemeIcons::default();
        assert_eq!(icons.resize(44).map(|gone| gone.len()), Some(0));
        let first = icon(IconFormat::Svg, Vec::new());

        assert!(icons.start(&first));
        assert!(!icons.start(&first));
        let image = Arc::new(RenderImage::new([image::Frame::new(RgbaImage::new(1, 1))]));
        let stale = icons.finish(first.path.clone(), 22, Some(Arc::clone(&image)));
        assert!(stale.is_some());
        assert!(
            icons
                .finish(first.path.clone(), 44, Some(Arc::clone(&image)))
                .is_none()
        );
        assert_eq!(icons.get(&first.path).map(|made| made.id), Some(image.id));
        assert!(!icons.start(&first));

        assert!(icons.resize(44).is_none());
        assert_eq!(icons.get(&first.path).map(|made| made.id), Some(image.id));

        let gone = icons.resize(22);
        assert_eq!(gone.map(|gone| gone.len()), Some(1));
        assert!(icons.get(&first.path).is_none());
        assert!(icons.resize(22).is_none());
        assert!(icons.start(&first));
        assert!(icons.finish(first.path.clone(), 22, None).is_none());
        assert!(icons.start(&first));
    }
}
