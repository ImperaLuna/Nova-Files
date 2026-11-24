use std::fs;
use std::path::Path;

use image::RgbaImage;
use resvg::{tiny_skia, usvg};

use crate::render::scaled_into;

pub fn draw(path: &Path, width: u32, height: u32) -> Option<RgbaImage> {
    let data = fs::read(path).ok()?;
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &options).ok()?;
    let size = tree.size().to_int_size();
    let (drawn_width, drawn_height) = scaled_into(size.width(), size.height(), width, height);
    #[expect(
        clippy::cast_precision_loss,
        reason = "sides of a few thousand pixels at most"
    )]
    let scale = drawn_width as f32 / tree.size().width();
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
    fn broken_drawings_and_other_files_are_refused() {
        let folder = TestFolder::new("vector-broken");
        let path = folder.join("bad.svg");
        fs::write(&path, "<svg").unwrap();
        assert!(draw(&path, 40, 40).is_none());
        let words = folder.join("notes.txt");
        fs::write(&words, "plain words").unwrap();
        assert!(draw(&words, 40, 40).is_none());
    }
}
