//! The helper as the core runs it: a real process, a batch on its command
//! line, real image files in a temp folder, and the thumbnails it leaves in
//! the thumbnails folder.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use image::{ImageFormat, RgbImage};

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
/// Bigger than a thumbnail, so the helper has to scale it down.
const SOURCE_WIDTH: u32 = 400;
const SOURCE_HEIGHT: u32 = 200;
const NORMAL_SIZE: u32 = 128;

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "nova-files-thumbnailer-{test}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Runs one batch of `(file, name)` and returns the helper's answers.
fn run(thumbnails: &Path, jobs: &[(&Path, &str)]) -> Vec<String> {
    let mut command = Command::new(HELPER);
    command.arg(thumbnails);
    for (file, name) in jobs {
        command
            .arg(file)
            .arg(format!("file://{}", file.display()))
            .arg("1700000000")
            .arg(name);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn images_get_scaled_thumbnails_with_their_uri_and_mtime() {
    let folder = TempFolder::new("images");
    let thumbnails = folder.0.join("thumbnails");
    let image = RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT);
    let mut jobs = Vec::new();
    for (file, format) in [
        ("a.jpg", ImageFormat::Jpeg),
        ("b.png", ImageFormat::Png),
        ("c.webp", ImageFormat::WebP),
    ] {
        let path = folder.0.join(file);
        image.save_with_format(&path, format).unwrap();
        jobs.push(path);
    }
    let names = ["a.png", "b.png", "c.png"];
    let batch: Vec<(&Path, &str)> = jobs.iter().map(PathBuf::as_path).zip(names).collect();

    assert_eq!(run(&thumbnails, &batch), ["ok", "ok", "ok"]);

    for (file, name) in batch {
        let thumb = thumbnails.join("normal").join(name);
        let bytes = fs::read(&thumb).unwrap();
        let uri = format!("Thumb::URI\0file://{}", file.display());
        assert!(contains(&bytes, uri.as_bytes()), "{name} has its URI");
        assert!(
            contains(&bytes, b"Thumb::MTime\x001700000000"),
            "{name} has its mtime"
        );
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (NORMAL_SIZE, NORMAL_SIZE / 2)
        );
    }
}

#[test]
fn a_broken_image_gets_a_failure_marker_and_other_files_are_left_alone() {
    let folder = TempFolder::new("broken");
    let thumbnails = folder.0.join("thumbnails");
    let broken = folder.0.join("broken.jpg");
    fs::write(&broken, b"not a jpeg").unwrap();
    let video = folder.0.join("clip.mp4");
    fs::write(&video, b"").unwrap();

    let answers = run(&thumbnails, &[(&broken, "x.png"), (&video, "y.png")]);

    assert_eq!(answers, ["failed", "unsupported"]);
    assert!(thumbnails.join("fail/nova-files/x.png").exists());
    assert!(!thumbnails.join("normal/x.png").exists());
    assert!(!thumbnails.join("normal/y.png").exists());
    assert!(!thumbnails.join("fail/nova-files/y.png").exists());
}

#[test]
fn a_file_that_cant_be_read_gets_no_failure_marker() {
    let folder = TempFolder::new("unreadable");
    let thumbnails = folder.0.join("thumbnails");
    let missing = folder.0.join("missing.jpg");

    assert_eq!(run(&thumbnails, &[(&missing, "x.png")]), ["failed"]);
    assert!(!thumbnails.join("fail/nova-files/x.png").exists());
}

#[test]
fn no_temp_files_are_left_behind() {
    let folder = TempFolder::new("temp");
    let thumbnails = folder.0.join("thumbnails");
    let source = folder.0.join("a.png");
    RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT)
        .save_with_format(&source, ImageFormat::Png)
        .unwrap();

    assert_eq!(run(&thumbnails, &[(&source, "a.png")]), ["ok"]);
    let left: Vec<_> = fs::read_dir(thumbnails.join("normal"))
        .unwrap()
        .map(|file| file.unwrap().file_name())
        .collect();
    assert_eq!(left, ["a.png"]);
}

#[test]
fn bad_arguments_are_refused() {
    let folder = TempFolder::new("refused");
    let status = Command::new(HELPER)
        .arg(&folder.0)
        .args(["/a.jpg", "file:///a.jpg", "1", "../escape.png"])
        .status()
        .unwrap();
    assert!(!status.success());
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
