//! The icon theme's icons a window draws, each made once at the size it
//! is drawn and shared by every row of its type.
//!
//! The core sends the icon file as it is (see `FileIcon`); a PNG is
//! decoded and shrunk as a thumbnail is, an SVG is drawn by GPUI's SVG
//! renderer straight at the row's size in device pixels. Both happen off
//! the UI thread. A window draws a few dozen types at most, so the images
//! are kept while it draws at one scale; a new scale hands them all back,
//! so the window drops them from GPUI's image atlas and makes them again.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{RenderImage, SvgRenderer};
use nova_files_api::{IconFormat, ThemeIcon};

use crate::thumbnail;

/// GPUI's renderer draws at twice the scale asked for; a first draw at
/// this scale gives the SVG's own size, doubled.
const PROBE_SCALE: f32 = 1.0;

/// The theme icons made so far, `side` device pixels across.
#[derive(Default)]
pub struct ThemeIcons {
    side: u32,
    made: HashMap<PathBuf, Arc<RenderImage>>,
    /// Icons being made.
    pending: HashSet<PathBuf>,
}

impl ThemeIcons {
    /// The image of the icon at `path`, once made.
    pub fn get(&self, path: &Path) -> Option<&Arc<RenderImage>> {
        self.made.get(path)
    }

    /// Whether `icon` still has to be made; if so, it counts as being made
    /// from now on.
    pub fn start(&mut self, icon: &ThemeIcon) -> bool {
        !self.made.contains_key(&icon.path) && self.pending.insert(icon.path.clone())
    }

    /// The side images are made at.
    pub const fn side(&self) -> u32 {
        self.side
    }

    /// Keeps `image`, made for `path` at `side`, if the window still
    /// draws at that size; otherwise hands it back. An image it replaces
    /// is handed back too.
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

    /// Starts again at `side`, handing back every image made so far.
    #[must_use]
    pub fn resize(&mut self, side: u32) -> Vec<Arc<RenderImage>> {
        self.side = side;
        self.pending.clear();
        self.made.drain().map(|(_, image)| image).collect()
    }
}

/// `icon` made to fit a square `side` device pixels across; `None` when
/// its file doesn't decode.
pub fn make(icon: &ThemeIcon, side: u32, svg: &SvgRenderer) -> Option<Arc<RenderImage>> {
    match icon.format {
        IconFormat::Png => thumbnail::shrink(&icon.bytes, side).map(Arc::new),
        IconFormat::Svg => {
            let probe = svg.render_single_frame(&icon.bytes, PROBE_SCALE).ok()?;
            let drawn = probe.size(0);
            let longest = drawn.width.0.max(drawn.height.0);
            let longest = u32::try_from(longest).ok().filter(|longest| *longest > 0)?;
            if longest == side {
                return Some(probe);
            }
            #[expect(
                clippy::cast_precision_loss,
                reason = "icon sides are a few hundred pixels at most"
            )]
            let scale = PROBE_SCALE * side as f32 / longest as f32;
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
        // GPUI draws in BGRA: red is the third byte.
        assert_eq!(&drawn.as_bytes(0).unwrap()[..4], [0, 0, 255, 255]);
        let exact = make(&icon(IconFormat::Svg, SQUARE.into()), 32, &svg).unwrap();
        assert_eq!(side_of(&exact), (32, 32));

        let mut png = Vec::new();
        RgbaImage::from_pixel(48, 48, Rgba([1, 2, 3, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let shrunk = make(&icon(IconFormat::Png, png), 22, &svg).unwrap();
        assert_eq!(side_of(&shrunk), (22, 22));

        assert!(make(&icon(IconFormat::Svg, b"not svg".to_vec()), 22, &svg).is_none());
        assert!(make(&icon(IconFormat::Png, b"not png".to_vec()), 22, &svg).is_none());
    }

    #[test]
    fn an_icon_is_made_once_per_size() {
        let mut icons = ThemeIcons::default();
        assert_eq!(icons.resize(44).len(), 0);
        let first = icon(IconFormat::Svg, Vec::new());

        assert!(icons.start(&first));
        assert!(!icons.start(&first));
        let image = Arc::new(RenderImage::new([image::Frame::new(RgbaImage::new(1, 1))]));
        // Made for a size the window left meanwhile: handed back.
        let stale = icons.finish(first.path.clone(), 22, Some(Arc::clone(&image)));
        assert!(stale.is_some());
        assert!(
            icons
                .finish(first.path.clone(), 44, Some(Arc::clone(&image)))
                .is_none()
        );
        assert_eq!(icons.get(&first.path).map(|made| made.id), Some(image.id));
        assert!(!icons.start(&first));

        let gone = icons.resize(22);
        assert_eq!(gone.len(), 1);
        assert!(icons.get(&first.path).is_none());
        assert_eq!(icons.side(), 22);
        assert!(icons.start(&first));
        // One that doesn't decode can be tried again.
        assert!(icons.finish(first.path.clone(), 22, None).is_none());
        assert!(icons.start(&first));
    }
}
