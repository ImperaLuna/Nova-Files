use std::sync::Arc;

use gpui::{Pixels, RenderImage, SvgRenderer, px};
use image::{Frame, Rgba as Pixel, RgbaImage};

use super::*;

const THUMBNAIL_WIDTH: u32 = 8;
const THUMBNAIL_HEIGHT: u32 = 6;
const SHADE: Pixel<u8> = Pixel([10, 20, 30, 255]);
const LABEL_WIDTH: Pixels = px(20.0);

fn thumbnail() -> Arc<RenderImage> {
    let image = RgbaImage::from_pixel(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT, SHADE);
    Arc::new(RenderImage::new([Frame::new(image)]))
}

fn pile(faces: Vec<Face>, label: Label) -> Pile {
    Pile {
        faces,
        label,
        label_width: LABEL_WIDTH,
        colors: Colors::default(),
    }
}

fn glyph() -> Face {
    Face::Glyph {
        glyph: Icon::FileText,
        tint: Colors::default().text,
    }
}

#[test]
fn a_pile_shows_at_most_three_cards_and_the_count() {
    let faces = vec![
        Face::Thumbnail(thumbnail()),
        glyph(),
        glyph(),
        glyph(),
        glyph(),
    ];
    let svg = pile(faces, Label::Count("5".to_owned())).svg();

    assert_eq!(svg.matches("<clipPath").count(), 3);
    assert_eq!(svg.matches("data:image/png;base64,").count(), 1);
    assert!(svg.contains(">5</text>"));
    assert!(usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).is_ok());
}

#[test]
fn a_name_is_written_as_plain_text() {
    let svg = pile(vec![glyph()], Label::Name("<a & b>".to_owned())).svg();

    assert!(svg.contains(">&lt;a &amp; b&gt;</text>"));
    assert!(usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).is_ok());
}

#[test]
fn the_picture_is_drawn_at_the_window_scale() {
    let renderer = SvgRenderer::new(Arc::new(()));
    let shown = pile(
        vec![Face::Thumbnail(thumbnail())],
        Label::Count("1".to_owned()),
    );
    let picture = shown.draw(&renderer, 2).unwrap();

    let size = picture.image.size(0);
    assert_eq!(picture.scale, 2);
    assert_eq!((size.width.0, size.height.0), (176, 166));
}

#[test]
fn a_control_character_in_a_name_still_draws() {
    let svg = pile(vec![glyph()], Label::Name("report\u{1}.txt".to_owned())).svg();

    assert!(svg.contains(">report\u{fffd}.txt</text>"));
    assert!(usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).is_ok());
}
