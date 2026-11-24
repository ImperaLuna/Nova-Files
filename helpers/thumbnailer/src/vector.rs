use std::fs;
use std::path::Path;

use image::RgbaImage;
use resvg::{tiny_skia, usvg};

const SVG_EXTENSIONS: [&str; 2] = ["svg", "svgz"];

pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SVG_EXTENSIONS
                .iter()
                .any(|svg| extension.eq_ignore_ascii_case(svg))
        })
}

pub fn draw(path: &Path, width: u32, height: u32) -> Option<RgbaImage> {
    let data = fs::read(path).ok()?;
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &options).ok()?;
    let size = tree.size();
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let side = |side: f32| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a side scaled into the box, at least 1"
        )]
        let scaled = (side * scale).round().max(1.0) as u32;
        scaled
    };
    let (drawn_width, drawn_height) = (side(size.width()), side(size.height()));
    let mut pixmap = tiny_skia::Pixmap::new(drawn_width, drawn_height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    RgbaImage::from_raw(drawn_width, drawn_height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="5"><rect width="10" height="5" fill="#FD7014"/></svg>"##;

    #[test]
    fn a_drawing_grows_to_fill_the_box_and_keeps_its_shape() {
        let folder = TestFolder::new("vector-draw");
        let path = folder.join("bar.svg");
        fs::write(&path, SQUARE).unwrap();
        let image = draw(&path, 400, 400).unwrap();
        assert_eq!(image.dimensions(), (400, 200));
        assert_eq!(image.get_pixel(200, 100).0, [0xFD, 0x70, 0x14, 255]);
    }

    #[test]
    fn broken_drawings_and_other_names_are_refused() {
        let folder = TestFolder::new("vector-broken");
        let path = folder.join("bad.svg");
        fs::write(&path, "<svg").unwrap();
        assert!(draw(&path, 40, 40).is_none());
        assert!(is_svg(Path::new("/a/b.SVG")));
        assert!(!is_svg(Path::new("/a/b.png")));
    }
}
