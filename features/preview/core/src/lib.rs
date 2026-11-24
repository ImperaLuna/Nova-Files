#![forbid(unsafe_code)]

mod code;

use std::fs::{self, File, Metadata};
use std::io::{self, Read as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use nova_files_api::{EventSink, Fit, Unreadable};
use nova_files_disk::ports::Mimes;
use nova_files_entries_core::list::{self, Stopped};
use nova_files_preview_api::{
    Event, PREVIEW_ENTRIES, PREVIEW_TEXT_BYTES, Preview, PreviewContent, PreviewId,
};
use nova_files_thumbnailer_launch::{self as launch, Running};
use rustix::fs::OFlags;

const THREAD_NAME: &str = "nova-files-preview";

const NUL: u8 = 0;

const PICTURE_SIGNATURES: [&[(usize, &[u8])]; 3] = [
    &[(0, &[0xFF, 0xD8, 0xFF])],
    &[(0, b"\x89PNG\r\n\x1a\n")],
    &[(0, b"RIFF"), (8, b"WEBP")],
];

#[derive(Debug, Default)]
pub struct Current {
    id: AtomicU64,
    decoding: Mutex<()>,
    running: Running,
}

const NONE: u64 = u64::MAX;

impl Current {
    pub const fn new() -> Self {
        Self {
            id: AtomicU64::new(NONE),
            decoding: Mutex::new(()),
            running: Running::new(),
        }
    }

    fn is(&self, id: PreviewId) -> bool {
        self.id.load(Ordering::SeqCst) == id.0
    }

    fn set(&self, id: PreviewId) {
        self.id.store(id.0, Ordering::SeqCst);
        self.running.stop();
    }

    pub fn cancel(&self) {
        self.id.store(NONE, Ordering::SeqCst);
        self.running.stop();
    }
}

pub struct Request {
    pub id: PreviewId,
    pub path: PathBuf,
    pub fit: Fit,
    pub current: Arc<Current>,
    pub types: Arc<dyn Mimes>,
    pub helper: Option<PathBuf>,
}

pub fn answer(events: &Arc<dyn EventSink>, request: Request) {
    request.current.set(request.id);
    let sink = Arc::clone(events);
    let (id, asked) = (request.id, request.path.clone());
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let current = Arc::clone(&request.current);
            if let Some(event) = event(&request)
                && current.is(request.id)
            {
                sink.deliver(event).ok();
            }
        });
    if spawned.is_err() {
        let reason = Unreadable::Failed;
        let path = asked;
        events.deliver(Event::Unreadable { id, path, reason }).ok();
    }
}

fn event(request: &Request) -> Option<Event> {
    let Request { id, path, .. } = request;
    let cancelled = || !request.current.is(*id);
    let unreadable = |reason| Event::Unreadable {
        id: *id,
        path: path.clone(),
        reason,
    };
    if cancelled() {
        return None;
    }
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return Some(unreadable(list::unreadable(&error))),
    };
    let content = if metadata.is_dir() {
        match folder(path, cancelled) {
            Ok(content) => content,
            Err(Stopped::Cancelled) => return None,
            Err(Stopped::Unreadable(reason)) => return Some(unreadable(reason)),
        }
    } else {
        if cancelled() {
            return None;
        }
        match (start(path, &metadata), &request.helper) {
            (Err(error), _) => return Some(unreadable(list::unreadable(&error))),
            (Ok(Some(start)), Some(helper)) if is_picture(&start) => picture(request, helper)?,
            (Ok(Some(start)), _) => text(start, metadata.len()),
            (Ok(None), _) => PreviewContent::Icon,
        }
    };
    let named = (!metadata.is_dir()).then(|| target(path));
    let content = match (content, named.as_deref().and_then(Path::file_name)) {
        (PreviewContent::Text { text, cut }, Some(name)) => {
            match code::runs(name, &text, cancelled) {
                Some(runs) => PreviewContent::Code {
                    text,
                    cut,
                    runs: runs.into(),
                },
                None if cancelled() => return None,
                None => PreviewContent::Text { text, cut },
            }
        }
        (content, _) => content,
    };
    let kind_name = named.as_deref().and_then(Path::file_name).and_then(|name| {
        let types = &request.types;
        types.of(name).and_then(|mime| types.name(mime))
    });
    let bytes = if metadata.is_dir() { 0 } else { metadata.len() };
    let preview = Preview {
        path: path.clone(),
        kind_name,
        bytes,
        content,
    };
    Some(Event::Previewed {
        id: *id,
        preview: Arc::new(preview),
    })
}

fn target(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());
    is_link
        .then(|| fs::canonicalize(path).ok())
        .flatten()
        .unwrap_or_else(|| path.to_owned())
}

fn picture(request: &Request, helper: &Path) -> Option<PreviewContent> {
    let _turn = request
        .current
        .decoding
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !request.current.is(request.id) {
        return None;
    }
    let current = &request.current;
    let wanted = || current.is(request.id);
    let picture = launch::picture(helper, &request.path, request.fit, &current.running, wanted);
    Some(picture.map_or(PreviewContent::Icon, PreviewContent::Picture))
}

fn folder(path: &Path, cancelled: impl Fn() -> bool) -> Result<PreviewContent, Stopped> {
    let mut first = list::read_folder(path, cancelled)?;
    let count = first.len();
    first.truncate(PREVIEW_ENTRIES);
    Ok(PreviewContent::Folder { first, count })
}

fn start(path: &Path, metadata: &Metadata) -> io::Result<Option<Vec<u8>>> {
    if !metadata.is_file() {
        return Ok(None);
    }
    let opened = File::options()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits().cast_signed())
        .open(path)?;
    let mut start = Vec::with_capacity(PREVIEW_TEXT_BYTES);
    let limit = u64::try_from(PREVIEW_TEXT_BYTES).unwrap_or(u64::MAX);
    opened.take(limit).read_to_end(&mut start)?;
    Ok(Some(start))
}

fn is_picture(start: &[u8]) -> bool {
    PICTURE_SIGNATURES.iter().any(|parts| {
        parts
            .iter()
            .all(|(at, bytes)| start.get(*at..).is_some_and(|rest| rest.starts_with(bytes)))
    })
}

fn text(start: Vec<u8>, length: u64) -> PreviewContent {
    if start.is_empty() || start.contains(&NUL) {
        return PreviewContent::Icon;
    }
    let cut = u64::try_from(start.len()).is_ok_and(|read| read < length);
    let text = match String::from_utf8(start) {
        Ok(text) => text,
        Err(error) if cut && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => return PreviewContent::Icon,
            }
        }
        Err(_) => return PreviewContent::Icon,
    };
    PreviewContent::Text {
        text: text.into(),
        cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

    fn shown(start: &[u8], length: u64) -> PreviewContent {
        text(start.to_vec(), length)
    }

    #[test]
    fn a_request_a_newer_one_replaced_never_answers() {
        let folder = TestFolder::new("preview-replaced");
        let dir = folder.path().to_path_buf();
        let current = Arc::new(Current::new());
        let request = |id| Request {
            id: PreviewId(id),
            path: dir.clone(),
            fit: Fit {
                width: 1,
                height: 1,
            },
            current: Arc::clone(&current),
            types: Arc::new(PlainText),
            helper: None,
        };
        let (old, new) = (request(1), request(2));
        current.set(new.id);
        assert!(event(&old).is_none());
        assert!(event(&new).is_some());
        current.cancel();
        assert!(event(&new).is_none());
    }

    #[derive(Debug)]
    struct PlainText;

    impl nova_files_api::FileTypes for PlainText {
        fn of(&self, name: &std::ffi::OsStr) -> Option<&str> {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "txt")
                .then_some("text/plain")
        }

        fn is_a(&self, mime: &str, wanted: &str) -> bool {
            mime == wanted
        }
    }

    impl Mimes for PlainText {
        fn ancestry(&self, mime: &str) -> Vec<String> {
            vec![mime.to_owned()]
        }

        fn name(&self, mime: &str) -> Option<String> {
            (mime == "text/plain").then(|| "Plain text".to_owned())
        }

        fn icon_names(&self, _: &str) -> Vec<String> {
            Vec::new()
        }

        fn icon_kind(&self, _: &str) -> nova_files_api::IconKind {
            nova_files_api::IconKind::Text
        }
    }

    fn previewed(path: &Path) -> Arc<Preview> {
        let current = Arc::new(Current::new());
        current.set(PreviewId(1));
        let request = Request {
            id: PreviewId(1),
            path: path.to_owned(),
            fit: Fit {
                width: 1,
                height: 1,
            },
            current,
            types: Arc::new(PlainText),
            helper: None,
        };
        match event(&request) {
            Some(Event::Previewed { preview, .. }) => preview,
            other => panic!("expected a preview, got {other:?}"),
        }
    }

    #[test]
    fn a_file_is_named_by_its_type_and_a_folder_is_not() {
        let folder = TestFolder::new("preview-named");
        let dir = folder.join("dir");
        let file = dir.join("a.txt");
        let folder = dir.join("b.txt");
        fs::create_dir_all(&folder).unwrap();
        fs::write(&file, "a".repeat(4096)).unwrap();
        let (file, folder) = (previewed(&file), previewed(&folder));
        assert_eq!(file.kind_name.as_deref(), Some("Plain text"));
        assert!(matches!(
            file.content,
            PreviewContent::Text { cut: false, .. }
        ));
        assert_eq!(folder.kind_name, None);
    }

    #[test]
    fn a_link_is_named_by_its_target() {
        let folder = TestFolder::new("preview-target");
        let dir = folder.path();
        let photo = dir.join("photo.png");
        fs::write(&photo, b"x").unwrap();
        let link = dir.join("current");
        std::os::unix::fs::symlink(&photo, &link).unwrap();
        let (to_link, to_photo) = (target(&link), target(&photo));
        assert_eq!(to_link.file_name(), photo.file_name());
        assert_eq!(to_photo, photo);
    }

    #[test]
    fn pictures_are_known_by_their_first_bytes() {
        assert!(is_picture(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(is_picture(b"\x89PNG\r\n\x1a\n\0\0"));
        assert!(is_picture(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!is_picture(b"RIFF\0\0\0\0WAVEfmt "));
        assert!(!is_picture(b"RIFF"));
        assert!(!is_picture(b"\x89PNG"));
        assert!(!is_picture(b"GIF89a"));
        assert!(!is_picture(b""));
    }

    #[test]
    fn utf8_without_nul_is_text() {
        assert_eq!(
            shown("héllo".as_bytes(), 6),
            PreviewContent::Text {
                text: "héllo".into(),
                cut: false
            }
        );
    }

    #[test]
    fn empty_binary_and_other_encodings_show_their_icon() {
        assert_eq!(shown(b"", 0), PreviewContent::Icon);
        assert_eq!(shown(b"PK\x03\x04\0\0", 6), PreviewContent::Icon);
        assert_eq!(shown(b"caf\xe9", 4), PreviewContent::Icon);
    }

    #[test]
    fn a_character_cut_at_the_end_of_a_longer_file_is_dropped() {
        assert_eq!(
            shown(b"caf\xc3", 100),
            PreviewContent::Text {
                text: "caf".into(),
                cut: true
            }
        );
        assert_eq!(shown(b"caf\xc3", 4), PreviewContent::Icon);
        assert_eq!(shown(b"ca\xc3f", 100), PreviewContent::Icon);
    }
}
