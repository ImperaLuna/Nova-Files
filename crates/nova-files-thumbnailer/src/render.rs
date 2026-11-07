//! Makes one thumbnail: decode the file, scale it to fit the spec's
//! "normal" size, and write it into `normal/` with its URI and mtime. A
//! file that can't be decoded gets a failure marker in `fail/nova-files/`
//! instead, so nobody tries it again until it changes.
//!
//! The helper decodes JPEG, PNG and WebP itself; any other file is
//! `unsupported` and left alone. Each file is written to a private temp
//! file in the same folder, then renamed over the old one.

use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Write as _};
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};

use crate::batch::Job;
use crate::png_text;
use crate::sandbox::FOLDER_MODE;

/// The spec's "normal" thumbnail fits in this square.
const NORMAL_SIZE: u32 = 128;
const NORMAL_FOLDER: &str = "normal";
/// Failure markers, per application as the spec asks.
const FAIL_FOLDER: &str = "fail/nova-files";
/// Thumbnails are private to the user.
const FILE_MODE: u32 = 0o600;
/// The most a single decode may allocate, so one huge image can't take the
/// machine's memory.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const URI_KEY: &str = "Thumb::URI";
const MTIME_KEY: &str = "Thumb::MTime";
const TEMP_SUFFIX: &str = ".part";
/// A failure marker is a 1x1 image, as the spec suggests.
const MARKER_SIZE: u32 = 1;

pub enum Outcome {
    Ok,
    Failed,
    Unsupported,
}

impl Outcome {
    pub const fn word(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
        }
    }
}

pub fn render(folder: &Path, job: &Job) -> Outcome {
    let supported = matches!(
        ImageFormat::from_path(&job.source),
        Ok(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    );
    if !supported {
        return Outcome::Unsupported;
    }
    let Ok(image) = decode(&job.source) else {
        let marker = DynamicImage::ImageRgba8(RgbaImage::new(MARKER_SIZE, MARKER_SIZE));
        // Without a marker the file is simply tried again next time.
        write(&folder.join(FAIL_FOLDER), job, &marker).ok();
        return Outcome::Failed;
    };
    let thumb = image.thumbnail(NORMAL_SIZE, NORMAL_SIZE);
    // A write that fails (a full disk) leaves no marker: the file is fine.
    if write(&folder.join(NORMAL_FOLDER), job, &thumb).is_ok() {
        Outcome::Ok
    } else {
        Outcome::Failed
    }
}

fn decode(path: &Path) -> io::Result<DynamicImage> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    reader.decode().map_err(io::Error::other)
}

fn write(into: &Path, job: &Job, image: &DynamicImage) -> io::Result<()> {
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(io::Error::other)?;
    let png = png_text::with_text(&png, &[(URI_KEY, &job.uri), (MTIME_KEY, &job.mtime)])
        .ok_or_else(|| io::Error::other("the encoder wrote no PNG header"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(FOLDER_MODE)
        .create(into)?;
    let target = into.join(&job.name);
    let mut temp_name = job.name.clone();
    temp_name.push(TEMP_SUFFIX);
    let temp = into.join(temp_name);
    // A temp file left by a helper that was killed is replaced, not reused.
    fs::remove_file(&temp).ok();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(&temp)?;
    let saved = file
        .write_all(&png)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temp, &target));
    if saved.is_err() {
        fs::remove_file(&temp).ok();
    }
    saved
}
