use std::ffi::OsString;
use std::io::{self, Write as _};
use std::process::ExitCode;

use mermaid_svg::Theme;

use crate::{preview, sandbox, vector};

pub const FLAG: &str = "--diagram";
pub const USAGE: &str = "usage: nova-files-thumbnailer --diagram <width> <height> <zoom percent> <light|dark> <mermaid source>";
pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
const PERCENT: f32 = 100.0;
const LIGHT_WORD: &str = "light";
const DARK_WORD: &str = "dark";

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
    pub light: bool,
    pub source: String,
}

impl Request {
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let mut args = args.into_iter();
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
        let source = args.next()?.into_string().ok()?;
        (source.len() <= MAX_SOURCE_BYTES && args.next().is_none()).then_some(Self {
            width,
            height,
            zoom,
            light,
            source,
        })
    }
}

pub fn run(request: &Request) -> ExitCode {
    if let Err(error) = sandbox::lock_in_read_only() {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    let theme = if request.light {
        Theme::default()
    } else {
        Theme::dark()
    };
    let Ok(drawing) = mermaid_svg::render_with(&request.source, &theme) else {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_request_names_a_box_a_zoom_and_the_source() {
        let request =
            Request::from_args(args(&["800", "600", "200", "dark", "graph TD; A-->B"])).unwrap();
        assert_eq!((request.width, request.height), (800, 600));
        assert!((request.zoom - 2.0).abs() < f32::EPSILON);
        assert_eq!(request.source, "graph TD; A-->B");
        assert!(Request::from_args(args(&["800", "600", "0", "dark", "graph TD"])).is_none());
        assert!(Request::from_args(args(&["800", "600", "100", "pink", "graph TD"])).is_none());
        assert!(Request::from_args(args(&["800", "600", "100", "dark"])).is_none());
        let long = "a".repeat(MAX_SOURCE_BYTES + 1);
        assert!(Request::from_args(args(&["8", "6", "100", "light", &long])).is_none());
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
}
