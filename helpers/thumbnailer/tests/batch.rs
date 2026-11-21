#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use image::codecs::jpeg::JpegEncoder;
use image::{ImageEncoder as _, ImageFormat, RgbImage};
use nova_files_test_folder::TestFolder;

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const SOURCE_WIDTH: u32 = 400;
const SOURCE_HEIGHT: u32 = 200;
const NORMAL_SIZE: u32 = 128;
const LARGE_SIZE: u32 = 256;
const NORMAL: &str = "normal";
const LARGE: &str = "large";

fn run(thumbnails: &Path, jobs: &[(&Path, &str)]) -> Vec<String> {
    run_with_data(thumbnails, jobs, &thumbnails.join("no-data"))
}

fn run_with_data(thumbnails: &Path, jobs: &[(&Path, &str)], data: &Path) -> Vec<String> {
    run_sized(thumbnails, NORMAL, jobs, data)
}

fn run_sized(thumbnails: &Path, size: &str, jobs: &[(&Path, &str)], data: &Path) -> Vec<String> {
    let mut command = Command::new(HELPER);
    command
        .arg(thumbnails)
        .arg(size)
        .env("XDG_DATA_DIRS", data)
        .env("XDG_DATA_HOME", data);
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
    let folder = TestFolder::new("thumbnailer-images");
    let thumbnails = folder.join("thumbnails");
    let image = RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT);
    let mut jobs = Vec::new();
    for (file, format) in [
        ("a.jpg", ImageFormat::Jpeg),
        ("b.png", ImageFormat::Png),
        ("c.webp", ImageFormat::WebP),
    ] {
        let path = folder.join(file);
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
fn a_large_batch_makes_large_thumbnails_in_their_own_folder() {
    let folder = TestFolder::new("thumbnailer-large");
    let thumbnails = folder.join("thumbnails");
    let file = folder.join("a.png");
    RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT)
        .save_with_format(&file, ImageFormat::Png)
        .unwrap();

    let answers = run_sized(&thumbnails, LARGE, &[(&file, "a.png")], folder.path());

    assert_eq!(answers, ["ok"]);
    assert!(!thumbnails.join(NORMAL).exists());
    let thumb = image::open(thumbnails.join(LARGE).join("a.png")).unwrap();
    assert_eq!(
        (thumb.width(), thumb.height()),
        (LARGE_SIZE, LARGE_SIZE / 2)
    );
}

#[test]
fn a_picture_smaller_than_the_size_keeps_its_own() {
    let folder = TestFolder::new("thumbnailer-small");
    let thumbnails = folder.join("thumbnails");
    let small = folder.join("b.png");
    RgbImage::new(LARGE_SIZE / 2, LARGE_SIZE / 4)
        .save_with_format(&small, ImageFormat::Png)
        .unwrap();
    let wide = folder.join("c.png");
    RgbImage::new(LARGE_SIZE * 2, LARGE_SIZE / 4)
        .save_with_format(&wide, ImageFormat::Png)
        .unwrap();
    let jobs: [(&Path, &str); 2] = [(&small, "b.png"), (&wide, "c.png")];

    assert_eq!(
        run_sized(&thumbnails, LARGE, &jobs, folder.path()),
        ["ok", "ok"]
    );

    let side = |name: &str| {
        let thumb = image::open(thumbnails.join(LARGE).join(name)).unwrap();
        (thumb.width(), thumb.height())
    };
    assert_eq!(side("b.png"), (LARGE_SIZE / 2, LARGE_SIZE / 4));
    assert_eq!(side("c.png"), (LARGE_SIZE, LARGE_SIZE / 8));
}

const PHOTO_WIDTH: u32 = 1600;
const PHOTO_HEIGHT: u32 = 1200;

#[test]
fn a_photo_sized_image_is_within_the_decode_limit() {
    let folder = TestFolder::new("thumbnailer-photo");
    let thumbnails = folder.join("thumbnails");
    let source = folder.join("big.png");
    RgbImage::new(PHOTO_WIDTH, PHOTO_HEIGHT)
        .save_with_format(&source, ImageFormat::Png)
        .unwrap();

    assert_eq!(run(&thumbnails, &[(&source, "b.png")]), ["ok"]);
}

const HUGE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 78, 32, 0, 0, 78, 32, 8, 6,
    0, 0, 0, 227, 112, 70, 57, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 64, 5, 0, 0, 16, 0,
    1, 57, 189, 143, 101, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[test]
fn an_image_too_big_to_decode_is_refused_without_a_failure_marker() {
    let folder = TestFolder::new("thumbnailer-huge");
    let thumbnails = folder.join("thumbnails");
    let source = folder.join("huge.png");
    fs::write(&source, HUGE_PNG).unwrap();

    assert_eq!(run(&thumbnails, &[(&source, "h.png")]), ["failed"]);
    assert!(!thumbnails.join("fail/nova-files/h.png").exists());
}

#[test]
fn an_image_is_known_by_its_content_not_its_name() {
    let folder = TestFolder::new("thumbnailer-content");
    let thumbnails = folder.join("thumbnails");
    let source = folder.join("photo");
    RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT)
        .save_with_format(&source, ImageFormat::Png)
        .unwrap();

    assert_eq!(run(&thumbnails, &[(&source, "p.png")]), ["ok"]);
}

#[test]
fn a_broken_image_gets_a_failure_marker_and_other_files_are_left_alone() {
    let folder = TestFolder::new("thumbnailer-broken");
    let thumbnails = folder.join("thumbnails");
    let broken = folder.join("broken.jpg");
    fs::write(&broken, b"not a jpeg").unwrap();
    let video = folder.join("clip.mp4");
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
    let folder = TestFolder::new("thumbnailer-unreadable");
    let thumbnails = folder.join("thumbnails");
    let missing = folder.join("missing.jpg");

    assert_eq!(run(&thumbnails, &[(&missing, "x.png")]), ["failed"]);
    assert!(!thumbnails.join("fail/nova-files/x.png").exists());
}

#[test]
fn no_temp_files_are_left_behind() {
    let folder = TestFolder::new("thumbnailer-temp");
    let thumbnails = folder.join("thumbnails");
    let source = folder.join("a.png");
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

const EXIF_ROTATE_90: &[u8] = &[
    b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
];

#[test]
fn a_rotated_photo_gets_an_upright_thumbnail() {
    let folder = TestFolder::new("thumbnailer-rotated");
    let thumbnails = folder.join("thumbnails");
    let source = folder.join("portrait.jpg");
    let mut jpeg = Vec::new();
    let mut encoder = JpegEncoder::new(&mut jpeg);
    encoder.set_exif_metadata(EXIF_ROTATE_90.to_vec()).unwrap();
    encoder
        .encode_image(&RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT))
        .unwrap();
    fs::write(&source, jpeg).unwrap();

    assert_eq!(run(&thumbnails, &[(&source, "p.png")]), ["ok"]);
    let thumb = image::open(thumbnails.join("normal/p.png")).unwrap();
    assert_eq!(
        (thumb.width(), thumb.height()),
        (NORMAL_SIZE / 2, NORMAL_SIZE)
    );
}

#[test]
fn bad_arguments_are_refused() {
    let folder = TestFolder::new("thumbnailer-refused");
    let status = Command::new(HELPER)
        .arg(folder.path())
        .args([NORMAL, "/a.jpg", "file:///a.jpg", "1", "../escape.png"])
        .status()
        .unwrap();
    assert!(!status.success());
}

#[test]
fn other_formats_go_to_the_system_thumbnailer_for_their_type() {
    let folder = TestFolder::new("thumbnailer-system");
    let thumbnails = folder.join("thumbnails");
    let data = folder.join("data");
    fs::create_dir_all(data.join("mime")).unwrap();
    fs::create_dir_all(data.join("thumbnailers")).unwrap();
    fs::write(
        data.join("mime/globs2"),
        "50:image/heif:*.heic\n50:video/mp4:*.mp4\n50:video/x-matroska:*.mkv\n",
    )
    .unwrap();
    let made = folder.join("made.png");
    RgbImage::new(SOURCE_WIDTH, SOURCE_HEIGHT)
        .save_with_format(&made, ImageFormat::Png)
        .unwrap();
    let script = folder.join("thumbnail.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nexec >&-\nsleep 0.2\ntest -r \"$1\" && exec cp \"{}\" \"$2\"\n",
            made.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        data.join("thumbnailers/copy.thumbnailer"),
        format!(
            "[Thumbnailer Entry]\nExec={} %i %o\nMimeType=image/heif;\n",
            script.display()
        ),
    )
    .unwrap();
    fs::write(
        data.join("thumbnailers/broken.thumbnailer"),
        "[Thumbnailer Entry]\nExec=false %i %o\nMimeType=video/mp4;\n",
    )
    .unwrap();
    let heic = folder.join("photo.heic");
    fs::write(&heic, b"\0\0\0\x18ftypheic").unwrap();
    let missing = folder.join("missing.heic");
    let (mp4, mkv) = (folder.join("clip.mp4"), folder.join("film.mkv"));
    fs::write(&mp4, b"").unwrap();
    fs::write(&mkv, b"").unwrap();

    let answers = run_with_data(
        &thumbnails,
        &[
            (&heic, "h.png"),
            (&mp4, "m.png"),
            (&mkv, "k.png"),
            (&missing, "x.png"),
        ],
        &data,
    );

    assert_eq!(answers, ["ok", "failed", "unsupported", "failed"]);
    let thumb = image::open(thumbnails.join("normal/h.png")).unwrap();
    assert_eq!(
        (thumb.width(), thumb.height()),
        (NORMAL_SIZE, NORMAL_SIZE / 2)
    );
    assert!(thumbnails.join("fail/nova-files/m.png").exists());
    assert!(!thumbnails.join("fail/nova-files/x.png").exists());
    let left: Vec<_> = fs::read_dir(thumbnails.join("normal"))
        .unwrap()
        .map(|file| file.unwrap().file_name())
        .collect();
    assert_eq!(left, ["h.png"], "the thumbnailer's own output is removed");
}

#[test]
fn a_thumbnailer_killed_by_a_signal_leaves_no_failure_marker() {
    let folder = TestFolder::new("thumbnailer-system-killed");
    let thumbnails = folder.join("thumbnails");
    let data = folder.join("data");
    fs::create_dir_all(data.join("mime")).unwrap();
    fs::create_dir_all(data.join("thumbnailers")).unwrap();
    fs::write(data.join("mime/globs2"), "50:video/mp4:*.mp4\n").unwrap();
    let script = folder.join("killed.sh");
    fs::write(&script, "#!/bin/sh\nkill -KILL $$\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        data.join("thumbnailers/killed.thumbnailer"),
        format!(
            "[Thumbnailer Entry]\nExec={} %i %o\nMimeType=video/mp4;\n",
            script.display()
        ),
    )
    .unwrap();
    let mp4 = folder.join("clip.mp4");
    fs::write(&mp4, b"").unwrap();

    let answers = run_with_data(&thumbnails, &[(&mp4, "m.png")], &data);

    assert_eq!(answers, ["failed"]);
    assert!(!thumbnails.join("fail/nova-files/m.png").exists());
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
