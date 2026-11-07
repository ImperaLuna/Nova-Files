//! Makes one thumbnail: decode the file, scale it to fit the spec's
//! "normal" size, and write it into `normal/` with its URI and mtime. A
//! file whose data can't be decoded gets a failure marker in
//! `fail/nova-files/` instead, so nobody tries it again until it changes.
//! A file that couldn't be read (no permission, say) or was too big to
//! decode gets no marker: that can change without the file changing.
//!
//! The helper decodes JPEG, PNG and WebP itself, known by their first
//! bytes (or name); any other file is `unsupported` and left alone. Each
//! file is written to a temp file of this process's own in the same
//! folder, then renamed over the old one.

use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Write as _};
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::process;

use image::metadata::Orientation;
use image::{
    DynamicImage, ImageDecoder as _, ImageError, ImageFormat, ImageReader, ImageResult, Limits,
    RgbaImage,
};

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
const TEMP_SUFFIX: &str = "part";
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
    if !built_in(&job.source) {
        return Outcome::Unsupported;
    }
    let image = match decode(&job.source) {
        Ok(image) => image,
        Err(error) => {
            if is_bad_data(&error) {
                let marker = DynamicImage::ImageRgba8(RgbaImage::new(MARKER_SIZE, MARKER_SIZE));
                // Without a marker the file is simply tried again next time.
                write(&folder.join(FAIL_FOLDER), job, &marker).ok();
            }
            return Outcome::Failed;
        }
    };
    let thumb = image.thumbnail(NORMAL_SIZE, NORMAL_SIZE);
    // A write that fails (a full disk) leaves no marker: the file is fine.
    if write(&folder.join(NORMAL_FOLDER), job, &thumb).is_ok() {
        Outcome::Ok
    } else {
        Outcome::Failed
    }
}

/// Whether the helper decodes the file itself: by its first bytes, or by
/// its name when they say nothing (or it can't be opened).
fn built_in(path: &Path) -> bool {
    let format = ImageReader::open(path)
        .and_then(ImageReader::with_guessed_format)
        .ok()
        .and_then(|reader| reader.format())
        .or_else(|| ImageFormat::from_path(path).ok());
    matches!(
        format,
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    )
}

fn decode(path: &Path) -> ImageResult<DynamicImage> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    // `into_decoder` leaves the pixel buffer itself to the caller.
    limits.reserve(decoder.total_bytes())?;
    // Phones store a photo's rotation apart from its pixels; the thumbnail
    // has none, so it is turned upright here.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Whether `error` says the file's data is broken, rather than that it
/// couldn't be read or was too big for the decode limit.
const fn is_bad_data(error: &ImageError) -> bool {
    matches!(
        error,
        ImageError::Decoding(_) | ImageError::Unsupported(_) | ImageError::Parameter(_)
    )
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
    // Two helpers may write the same thumbnail at once (two windows on one
    // folder), so the temp file is this process's own.
    let mut temp_name = job.name.clone();
    temp_name.push(format!(".{}.{TEMP_SUFFIX}", process::id()));
    let temp = into.join(temp_name);
    // One left by a killed helper that had this process id is replaced.
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
