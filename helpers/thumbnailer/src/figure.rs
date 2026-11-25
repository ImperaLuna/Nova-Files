use std::ffi::OsString;
use std::io::{self, Write as _};
use std::process::ExitCode;

use latex_rust::{Color, Dim, MathFont, SvgOptions};
use mermaid_svg::Theme;

use crate::{preview, sandbox, vector};

pub const FLAG: &str = "--figure";
pub const USAGE: &str = "usage: nova-files-thumbnailer --figure <diagram|formula> <width> <height> <zoom percent> <light|dark> <ink RRGGBB> <source>";
const DIAGRAM_WORD: &str = "diagram";
const FORMULA_WORD: &str = "formula";
const FORMULA_POINTS: i64 = 14;
const HEX_RADIX: u32 = 16;
const INK_DIGITS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Diagram,
    Formula,
}
pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
const PERCENT: f32 = 100.0;
const LIGHT_WORD: &str = "light";
const DARK_WORD: &str = "dark";

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
    pub light: bool,
    pub ink: (u8, u8, u8),
    pub source: String,
}

impl Request {
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let mut args = args.into_iter();
        let kind = match args.next()?.to_str()? {
            DIAGRAM_WORD => Kind::Diagram,
            FORMULA_WORD => Kind::Formula,
            _ => return None,
        };
        let number = |arg: Option<OsString>| {
            arg?.into_string()
                .ok()?
                .parse::<u16>()
                .ok()
                .filter(|number| *number > 0)
        };
        let width = u32::from(number(args.next())?);
        let height = u32::from(number(args.next())?);
        let zoom = f32::from(number(args.next())?) / PERCENT;
        let light = match args.next()?.to_str()? {
            LIGHT_WORD => true,
            DARK_WORD => false,
            _ => return None,
        };
        let ink = ink(args.next()?.to_str()?)?;
        let source = args.next()?.into_string().ok()?;
        (source.len() <= MAX_SOURCE_BYTES && args.next().is_none()).then_some(Self {
            kind,
            width,
            height,
            zoom,
            light,
            ink,
            source,
        })
    }
}

pub fn run(request: &Request) -> ExitCode {
    if let Err(error) = sandbox::lock_in_read_only() {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    let Some(drawing) = drawing(request) else {
        return ExitCode::FAILURE;
    };
    let room = (request.width, request.height);
    let Some(image) = vector::draw_data(drawing.as_bytes(), room, Some(request.zoom)) else {
        return ExitCode::FAILURE;
    };
    if preview::send_still(&image).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn drawing(request: &Request) -> Option<String> {
    match request.kind {
        Kind::Diagram => {
            let theme = if request.light {
                Theme::default()
            } else {
                Theme::dark()
            };
            mermaid_svg::render_with(&request.source, &theme).ok()
        }
        Kind::Formula => formula(&request.source, request.ink),
    }
}

fn ink(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() != INK_DIGITS {
        return None;
    }
    let ink = u32::from_str_radix(hex, HEX_RADIX).ok()?;
    let [_, red, green, blue] = ink.to_be_bytes();
    Some((red, green, blue))
}

fn formula(source: &str, (red, green, blue): (u8, u8, u8)) -> Option<String> {
    let font = MathFont::stix_two_math().ok()?;
    let options = SvgOptions {
        font_size_pt: Dim::from_i64(FORMULA_POINTS),
        color: Color::rgb(red, green, blue),
        display: true,
    };
    latex_rust::latex_to_svg(source, &font, &options).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_request_names_a_box_a_zoom_and_the_source() {
        let request = Request::from_args(args(&[
            "diagram",
            "800",
            "600",
            "200",
            "dark",
            "E0E0E0",
            "graph TD; A-->B",
        ]))
        .unwrap();
        assert_eq!((request.width, request.height), (800, 600));
        assert!((request.zoom - 2.0).abs() < f32::EPSILON);
        assert_eq!(request.source, "graph TD; A-->B");
        assert_eq!(request.kind, Kind::Diagram);
        assert_eq!(ink("FD7014"), Some((0xFD, 0x70, 0x14)));
        assert_eq!(ink("FD70"), None);
        assert!(
            Request::from_args(args(&["chart", "8", "6", "100", "dark", "E0E0E0", "x"])).is_none()
        );
        assert!(
            Request::from_args(args(&[
                "diagram", "800", "600", "0", "dark", "E0E0E0", "graph TD"
            ]))
            .is_none()
        );
        assert!(
            Request::from_args(args(&[
                "diagram", "800", "600", "100", "pink", "E0E0E0", "graph TD"
            ]))
            .is_none()
        );
        assert!(
            Request::from_args(args(&["diagram", "800", "600", "100", "dark", "E0E0E0"])).is_none()
        );
        let long = "a".repeat(MAX_SOURCE_BYTES + 1);
        assert!(
            Request::from_args(args(&[
                "formula", "8", "6", "100", "light", "161616", &long
            ]))
            .is_none()
        );
    }

    #[test]
    fn a_flowchart_is_drawn_no_bigger_than_its_zoomed_size() {
        let drawing = mermaid_svg::render_with("graph TD; A-->B", &Theme::dark()).unwrap();
        let small = vector::draw_data(drawing.as_bytes(), (4000, 4000), Some(1.0)).unwrap();
        let big = vector::draw_data(drawing.as_bytes(), (4000, 4000), Some(2.0)).unwrap();
        assert!(small.width() < 4000 && small.height() < 4000);
        assert!(big.width() > small.width());
        assert!(mermaid_svg::render_with("not a diagram ((", &Theme::dark()).is_err());
    }

    #[test]
    fn a_formula_is_drawn_and_broken_maths_is_refused() {
        let drawing = formula(r"\frac{a}{b} + \sqrt{x^2}", (0xE0, 0xE0, 0xE0)).unwrap();
        let image = vector::draw_data(drawing.as_bytes(), (4000, 4000), Some(1.0)).unwrap();
        assert!(image.width() > image.height());
        assert!(image.pixels().any(|pixel| pixel.0[3] > 0));
        assert!(formula(r"\frac{a}{", (0, 0, 0)).is_none());
    }
}
