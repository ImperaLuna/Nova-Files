use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

use crate::sandbox;

pub const FLAG: &str = "--document";
pub const USAGE: &str = "usage: nova-files-thumbnailer --document <file>";
const LOCKED: &str = "locked";
const PAGES: &str = "pages";
const NONE: &str = "none";
const MAX_SIDE: f32 = 8192.0;
const MAX_PAGES: usize = 100_000;
const MAX_POINTS: f32 = 99_999.0;

pub fn file_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut args = args.into_iter();
    let file = PathBuf::from(args.next()?);
    (file.is_absolute() && args.next().is_none()).then_some(file)
}

pub fn run(file: &PathBuf) -> ExitCode {
    if let Err(error) = sandbox::lock_in_read_only() {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    let Ok(data) = fs::read(file) else {
        return ExitCode::FAILURE;
    };
    let pdf = match Pdf::new(data) {
        Ok(pdf) => pdf,
        Err(LoadPdfError::Decryption(DecryptionError::PasswordProtected)) => {
            return finish(&writeln!(io::stdout(), "{LOCKED}"));
        }
        Err(_) => return ExitCode::FAILURE,
    };
    let pages = pdf.pages();
    if pages.is_empty() || pages.len() > MAX_PAGES {
        return ExitCode::FAILURE;
    }
    let mut out = io::stdout().lock();
    let listed = writeln!(out, "{PAGES} {}", pages.len()).and_then(|()| {
        pages.iter().try_for_each(|page| {
            let (width, height) = page.render_dimensions();
            let side = |side: f32| side.round().clamp(1.0, MAX_POINTS);
            writeln!(out, "{} {}", side(width), side(height))
        })
    });
    if finish(&listed.and_then(|()| out.flush())) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            return ExitCode::FAILURE;
        };
        let asked = line.split_once(' ').and_then(|(index, width)| {
            Some((index.parse::<usize>().ok()?, width.parse::<u32>().ok()?))
        });
        let Some((page, width)) = asked.and_then(|(index, width)| Some((pages.get(index)?, width)))
        else {
            if writeln!(out, "{NONE}").and_then(|()| out.flush()).is_err() {
                return ExitCode::FAILURE;
            }
            continue;
        };
        let (page_width, page_height) = page.render_dimensions();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a width in device pixels, a few thousand at most"
        )]
        let scale = (width as f32 / page_width)
            .min(MAX_SIDE / page_width)
            .min(MAX_SIDE / page_height);
        if !scale.is_finite() || scale <= 0.0 {
            if writeln!(out, "{NONE}").and_then(|()| out.flush()).is_err() {
                return ExitCode::FAILURE;
            }
            continue;
        }
        let pixmap_settings = PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        };
        let pixmap = render(
            page,
            &cache,
            &settings,
            &RenderSettings::default(),
            &pixmap_settings,
        );
        let sent = writeln!(out, "{} {}", pixmap.width(), pixmap.height())
            .and_then(|()| out.write_all(pixmap.data_as_u8_slice()))
            .and_then(|()| out.flush());
        if sent.is_err() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

const fn finish(written: &io::Result<()>) -> ExitCode {
    if written.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
