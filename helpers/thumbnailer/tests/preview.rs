#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use nova_files::{IDLE_EXIT, RunningCore, Thumbnails};
use nova_files_api::{Connection, Fit, Picture};
use nova_files_preview_api::{Command, Event, Preview, PreviewContent, PreviewId};
use nova_files_test_folder::TestFolder;

const HELPER: &str = env!("CARGO_BIN_EXE_nova-files-thumbnailer");
const WAIT: Duration = Duration::from_secs(10);
const WIDTH: u32 = 400;
const HEIGHT: u32 = 200;
const ORANGE: [u8; 3] = [253, 112, 20];
const OPAQUE: u8 = 255;

struct Client {
    _core: RunningCore,
    connection: Connection,
    events: Receiver<Event>,
}

impl Client {
    fn new(temp: &Path) -> Self {
        let setup = Thumbnails {
            folder: temp.join("thumbnails"),
            helper: PathBuf::from(HELPER),
        };
        let core = nova_files::spawn_with(IDLE_EXIT, None, Some(setup), None).unwrap();
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(sink).unwrap();
        Self {
            _core: core,
            connection,
            events,
        }
    }

    fn preview(&self, id: u64, path: &Path, fit: Fit) -> Preview {
        self.connection
            .send(Command::Preview {
                id: PreviewId(id),
                path: path.to_owned(),
                fit,
            })
            .unwrap();
        match self.events.recv_timeout(WAIT).unwrap() {
            Event::Previewed { id: got, preview } if got == PreviewId(id) => (*preview).clone(),
            other => panic!("expected a preview, got {other:?}"),
        }
    }
}

fn picture(preview: &Preview) -> &Picture {
    match &preview.content {
        PreviewContent::Picture(picture) => picture,
        other => panic!("expected a picture, got {other:?}"),
    }
}

fn orange_picture(path: &Path, format: ImageFormat) {
    RgbImage::from_pixel(WIDTH, HEIGHT, Rgb(ORANGE))
        .save_with_format(path, format)
        .unwrap();
}

#[test]
fn a_big_picture_is_scaled_down_to_the_box() {
    let temp = TestFolder::new("preview-down");
    let path = temp.join("photo.txt");
    orange_picture(&path, ImageFormat::Png);
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (100, 50));
    assert_eq!(picture.pixels.len(), 100 * 50 * 4);
    assert_eq!(
        &picture.pixels[..4],
        [ORANGE[0], ORANGE[1], ORANGE[2], OPAQUE]
    );
    assert_eq!(shown.bytes, fs::metadata(&path).unwrap().len());
}

#[test]
fn a_small_picture_keeps_its_size() {
    let temp = TestFolder::new("preview-keeps");
    let path = temp.join("photo.jpg");
    orange_picture(&path, ImageFormat::Jpeg);
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: WIDTH + 10,
            height: HEIGHT + 10,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (WIDTH, HEIGHT));
}

#[test]
fn a_broken_picture_shows_its_icon() {
    let temp = TestFolder::new("preview-broken");
    let path = temp.join("photo.png");
    fs::write(&path, b"\x89PNG\r\n\x1a\nnot really").unwrap();
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert_eq!(shown.content, PreviewContent::Icon);
}

#[test]
fn stepping_through_pictures_answers_the_last() {
    let temp = TestFolder::new("preview-stepping");
    let paths: Vec<PathBuf> = (0..5)
        .map(|n| {
            let path = temp.join(format!("photo-{n}.png"));
            orange_picture(&path, ImageFormat::Png);
            path
        })
        .collect();
    let client = Client::new(temp.path());
    let fit = Fit {
        width: 100,
        height: 100,
    };
    for (id, path) in (1..).zip(&paths) {
        client
            .connection
            .send(Command::Preview {
                id: PreviewId(id),
                path: path.clone(),
                fit,
            })
            .unwrap();
    }
    let last = PreviewId(5);
    loop {
        match client.events.recv_timeout(WAIT).unwrap() {
            Event::Previewed { id, preview } if id == last => {
                assert_eq!(preview.path, paths[4]);
                break;
            }
            Event::Previewed { id, .. } => assert!(id < last),
            other @ (Event::Unreadable { .. }
            | Event::Paged { .. }
            | Event::Played { .. }
            | Event::Drawn { .. }) => {
                panic!("expected a preview, got {other:?}")
            }
        }
    }
}

#[test]
fn a_picture_too_wide_for_the_box_is_scaled_down() {
    let temp = TestFolder::new("preview-wide");
    let path = temp.join("photo.png");
    orange_picture(&path, ImageFormat::Png);
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 4000,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (100, 50));
}

#[test]
fn a_text_file_is_not_sent_to_the_helper() {
    let temp = TestFolder::new("preview-text");
    let path = temp.join("notes.png");
    fs::write(&path, "plain words").unwrap();
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert_eq!(
        shown.content,
        PreviewContent::Text {
            text: "plain words".into(),
            cut: false
        }
    );
}

#[test]
fn a_moving_gif_comes_with_every_frame() {
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame, Rgba, RgbaImage};
    let temp = TestFolder::new("preview-gif");
    let path = temp.join("spin.gif");
    let mut encoder = GifEncoder::new(fs::File::create(&path).unwrap());
    for shade in [0, 120, 240] {
        let image = RgbaImage::from_pixel(8, 8, Rgba([shade, 0, 0, OPAQUE]));
        let frame = Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(60, 1));
        encoder.encode_frame(frame).unwrap();
    }
    drop(encoder);
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    let PreviewContent::Moving(frames) = &shown.content else {
        panic!("expected a moving picture, got {:?}", shown.content);
    };
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].delay, Duration::from_millis(60));
    assert_eq!((frames[2].picture.width, frames[2].picture.height), (8, 8));
}

#[test]
fn a_drawing_is_drawn_to_fill_the_box() {
    let temp = TestFolder::new("preview-svg");
    let path = temp.join("bar.svg");
    fs::write(
        &path,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#FD7014"/></svg>"##,
    )
    .unwrap();
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    let picture = picture(&shown);
    assert_eq!((picture.width, picture.height), (100, 50));
    assert_eq!(
        &picture.pixels[..4],
        [ORANGE[0], ORANGE[1], ORANGE[2], OPAQUE]
    );
}

fn pdf(pages: &[(u32, u32)], encrypt: bool) -> Vec<u8> {
    let kids: Vec<String> = (0..pages.len()).map(|n| format!("{} 0 R", n + 3)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        ),
    ];
    for (width, height) in pages {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] >>"
        ));
    }
    if encrypt {
        let hex = "AB".repeat(32);
        objects.push(format!(
            "<< /Filter /Standard /V 1 /R 2 /O <{hex}> /U <{hex}> /P -4 >>"
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (n, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", n + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    let encryption = if encrypt {
        format!(
            " /Encrypt {} 0 R /ID [<{id}> <{id}>]",
            objects.len(),
            id = "01".repeat(16)
        )
    } else {
        String::new()
    };
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{encryption} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

impl Client {
    fn page(&self, id: u64, index: usize, width: u32) -> Option<Picture> {
        self.connection
            .send(Command::Page {
                id: PreviewId(id),
                index,
                width,
            })
            .unwrap();
        match self.events.recv_timeout(WAIT).unwrap() {
            Event::Paged {
                id: got,
                index: page,
                picture,
            } if got == PreviewId(id) && page == index => picture,
            other => panic!("expected a page, got {other:?}"),
        }
    }
}

#[test]
fn a_pdf_names_its_pages_and_draws_the_one_asked_for() {
    let temp = TestFolder::new("preview-pdf");
    let path = temp.join("letter.pdf");
    fs::write(&path, pdf(&[(612, 792), (842, 595), (100, 200)], false)).unwrap();
    let client = Client::new(temp.path());
    let fit = Fit {
        width: 100,
        height: 100,
    };

    let shown = client.preview(1, &path, fit);

    let PreviewContent::Document { pages } = &shown.content else {
        panic!("expected a document, got {:?}", shown.content);
    };
    let sizes: Vec<_> = pages.iter().map(|page| (page.width, page.height)).collect();
    assert_eq!(sizes, vec![(612, 792), (842, 595), (100, 200)]);
    let page = client.page(1, 1, 842).unwrap();
    assert_eq!((page.width, page.height), (842, 595));
    assert_eq!(&page.pixels[..4], [OPAQUE; 4]);
    assert_eq!(client.page(1, 3, 100), None);
}

#[test]
fn a_pdf_with_a_password_says_it_is_locked() {
    let temp = TestFolder::new("preview-locked");
    let path = temp.join("secret.pdf");
    fs::write(&path, pdf(&[(612, 792)], true)).unwrap();
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert_eq!(shown.content, PreviewContent::Locked);
}

#[test]
fn a_broken_pdf_shows_its_icon() {
    let temp = TestFolder::new("preview-broken-pdf");
    let path = temp.join("broken.pdf");
    fs::write(&path, b"%PDF-1.4\n\x00\xFFbroken stream").unwrap();
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert_eq!(shown.content, PreviewContent::Icon);
}

#[test]
fn a_text_that_mentions_the_pdf_signature_stays_text() {
    let temp = TestFolder::new("preview-pdf-text");
    let path = temp.join("notes.txt");
    fs::write(&path, "a PDF starts with %PDF-1.7 and then its objects").unwrap();
    let client = Client::new(temp.path());

    let shown = client.preview(
        1,
        &path,
        Fit {
            width: 100,
            height: 100,
        },
    );

    assert!(
        matches!(&shown.content, PreviewContent::Text { text, .. } if text.starts_with("a PDF")),
        "{:?}",
        shown.content
    );
}

impl Client {
    fn draw(&self, id: u64, document: &Path, picture: &Path) -> Option<Picture> {
        self.connection
            .send(Command::Draw {
                id: PreviewId(id),
                document: document.to_owned(),
                picture: picture.to_owned(),
                fit: Fit {
                    width: WIDTH,
                    height: HEIGHT,
                },
            })
            .unwrap();
        match self.events.recv_timeout(WAIT).unwrap() {
            Event::Drawn {
                id: got,
                picture: asked,
                drawn,
            } if got == PreviewId(id) && asked == picture => drawn,
            other => panic!("expected a drawn picture, got {other:?}"),
        }
    }
}

#[test]
fn a_note_draws_only_the_pictures_inside_its_own_folder() {
    let temp = TestFolder::new("preview-note-pictures");
    let notes = temp.join("notes");
    fs::create_dir_all(&notes).unwrap();
    let note = notes.join("a.md");
    fs::write(&note, "![x](x.png)\n").unwrap();
    orange_picture(&notes.join("x.png"), ImageFormat::Png);
    orange_picture(&temp.join("outside.png"), ImageFormat::Png);
    let client = Client::new(temp.path());
    let fit = Fit {
        width: WIDTH,
        height: HEIGHT,
    };
    client.preview(1, &note, fit);

    let inside = client.draw(1, &note, &notes.join("x.png")).unwrap();
    assert_eq!((inside.width, inside.height), (WIDTH, HEIGHT));
    assert_eq!(client.draw(1, &note, &notes.join("../outside.png")), None);
}
