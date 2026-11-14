//! Looking into a file or folder for [`Command::Preview`], the quick look.
//!
//! A folder shows its first entries, as a listing orders them, and how
//! many it holds. A regular file is read from its start, at most
//! [`PREVIEW_TEXT_BYTES`]. When its first bytes are those of a JPEG, PNG
//! or WebP picture, it goes to the sandboxed thumbnail helper, which
//! decodes it scaled down to the client's box (see `thumbs`); one client's
//! pictures decode one at a time, a newer request stops the helper still
//! decoding an older one, and a request replaced while it waited its turn
//! never starts. Otherwise, when the start is UTF-8 text
//! (allowing a character cut at the end) with no NUL byte, it shows as
//! text. Anything else (a picture that wouldn't decode, a binary file, or
//! anything that isn't a regular file: a device, pipe or socket, which is
//! never opened) shows as its icon. A link shows what it points to, named
//! by its target's name too. The kind's name comes from the file's name,
//! through the MIME database.
//!
//! Each request reads on a thread of its own, so a drive that stopped
//! answering never holds up the core. A client's newer request cancels
//! the older one: the worker checks before each read from the disk and
//! before it answers, so a late answer never lands. The client also drops
//! answers whose id isn't its newest.
//!
//! [`Command::Preview`]: nova_files_api::Command::Preview

use std::fs::{self, File, Metadata};
use std::io::{self, Read as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;

use nova_files_api::{
    Event, EventSink, FileTypes as _, Fit, PREVIEW_ENTRIES, PREVIEW_TEXT_BYTES, Preview,
    PreviewContent, PreviewId, Unreadable,
};
use rustix::fs::OFlags;

use crate::list::{self, Stopped};
use crate::mime::MimeTypes;
use crate::thumbs::{self, Running};

const THREAD_NAME: &str = "nova-files-preview";

/// A NUL byte marks a file as binary.
const NUL: u8 = 0;

/// How the pictures the helper decodes begin: the bytes each holds at
/// which place.
const PICTURE_SIGNATURES: [&[(usize, &[u8])]; 3] = [
    // JPEG
    &[(0, &[0xFF, 0xD8, 0xFF])],
    // PNG
    &[(0, b"\x89PNG\r\n\x1a\n")],
    // WebP: a RIFF file of WEBP data.
    &[(0, b"RIFF"), (8, b"WEBP")],
];

/// The preview that counts for one client, or none, and the turn its
/// pictures take to decode.
#[derive(Debug, Default)]
pub struct Current {
    id: AtomicU64,
    decoding: Mutex<()>,
    /// The helper decoding the client's picture, if one is.
    running: Running,
}

/// No preview counts: the client never asked, or left.
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

    /// Request `id` counts from now on: an older one still decoding stops.
    fn set(&self, id: PreviewId) {
        self.id.store(id.0, Ordering::SeqCst);
        self.running.stop();
    }

    /// Stops the preview running, if any.
    pub fn cancel(&self) {
        self.id.store(NONE, Ordering::SeqCst);
        self.running.stop();
    }
}

/// What one request needs.
pub struct Request {
    pub id: PreviewId,
    pub path: PathBuf,
    pub fit: Fit,
    pub current: Arc<Current>,
    pub types: Arc<OnceLock<MimeTypes>>,
    /// The thumbnail helper, which decodes pictures; without one, a
    /// picture shows its icon.
    pub helper: Option<PathBuf>,
}

/// Looks into `request`'s path on a thread of its own and hands the answer
/// to `events`, unless a newer request replaced it first.
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
        // Nothing could be read; the client can ask again.
        let reason = Unreadable::Failed;
        let path = asked;
        events
            .deliver(Event::PreviewUnreadable { id, path, reason })
            .ok();
    }
}

/// The answer to `request`, or `None` once it no longer counts.
fn event(request: &Request) -> Option<Event> {
    let Request { id, path, .. } = request;
    let cancelled = || !request.current.is(*id);
    let unreadable = |reason| Event::PreviewUnreadable {
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
    let kind_name = named.as_deref().and_then(Path::file_name).and_then(|name| {
        let types = request.types.get_or_init(MimeTypes::from_env);
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

/// What `path` leads to: the path itself, or a link's final target.
fn target(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());
    is_link
        .then(|| fs::canonicalize(path).ok())
        .flatten()
        .unwrap_or_else(|| path.to_owned())
}

/// `request`'s picture, decoded by `helper` once the client's previous
/// picture is done, or its icon when it wouldn't decode. `None` when the
/// request was replaced meanwhile.
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
    let picture = thumbs::picture(helper, &request.path, request.fit, &current.running, wanted);
    Some(picture.map_or(PreviewContent::Icon, PreviewContent::Picture))
}

/// A folder's first entries and how many it holds.
fn folder(path: &Path, cancelled: impl Fn() -> bool) -> Result<PreviewContent, Stopped> {
    let mut first = list::read_folder(path, cancelled)?;
    let count = first.len();
    first.truncate(PREVIEW_ENTRIES);
    Ok(PreviewContent::Folder { first, count })
}

/// The first bytes of a regular file; `None` for anything else.
fn start(path: &Path, metadata: &Metadata) -> io::Result<Option<Vec<u8>>> {
    if !metadata.is_file() {
        return Ok(None);
    }
    // Swapped for a pipe since the look, it still won't wait for a writer.
    let opened = File::options()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits().cast_signed())
        .open(path)?;
    let mut start = Vec::with_capacity(PREVIEW_TEXT_BYTES);
    let limit = u64::try_from(PREVIEW_TEXT_BYTES).unwrap_or(u64::MAX);
    opened.take(limit).read_to_end(&mut start)?;
    Ok(Some(start))
}

/// Whether a file starting with `start` is a picture the helper decodes.
fn is_picture(start: &[u8]) -> bool {
    PICTURE_SIGNATURES.iter().any(|parts| {
        parts
            .iter()
            .all(|(at, bytes)| start.get(*at..).is_some_and(|rest| rest.starts_with(bytes)))
    })
}

/// `start`, the first bytes of a file `length` long, as text when it is
/// text, else the file's icon.
fn text(start: Vec<u8>, length: u64) -> PreviewContent {
    if start.is_empty() || start.contains(&NUL) {
        return PreviewContent::Icon;
    }
    let cut = u64::try_from(start.len()).is_ok_and(|read| read < length);
    let text = match String::from_utf8(start) {
        Ok(text) => text,
        // A character cut at the end of what was read is still text.
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

    fn shown(start: &[u8], length: u64) -> PreviewContent {
        text(start.to_vec(), length)
    }

    #[test]
    fn a_request_a_newer_one_replaced_never_answers() {
        let dir = std::env::temp_dir();
        let current = Arc::new(Current::new());
        let request = |id| Request {
            id: PreviewId(id),
            path: dir.clone(),
            fit: Fit {
                width: 1,
                height: 1,
            },
            current: Arc::clone(&current),
            types: Arc::new(OnceLock::new()),
            helper: None,
        };
        let (old, new) = (request(1), request(2));
        current.set(new.id);
        assert!(event(&old).is_none());
        assert!(event(&new).is_some());
        current.cancel();
        assert!(event(&new).is_none());
    }

    #[test]
    fn a_link_is_named_by_its_target() {
        let dir =
            std::env::temp_dir().join(format!("nova-files-preview-target-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("photo.png");
        fs::write(&photo, b"x").unwrap();
        let link = dir.join("current");
        std::os::unix::fs::symlink(&photo, &link).unwrap();
        let (to_link, to_photo) = (target(&link), target(&photo));
        fs::remove_dir_all(&dir).ok();
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
        // Latin-1 "é", whole: not UTF-8.
        assert_eq!(shown(b"caf\xe9", 4), PreviewContent::Icon);
    }

    #[test]
    fn a_character_cut_at_the_end_of_a_longer_file_is_dropped() {
        // "é" is 0xC3 0xA9; the read stopped between them.
        assert_eq!(
            shown(b"caf\xc3", 100),
            PreviewContent::Text {
                text: "caf".into(),
                cut: true
            }
        );
        // The file ends there: it isn't text.
        assert_eq!(shown(b"caf\xc3", 4), PreviewContent::Icon);
        // A bad byte before the end isn't a cut character.
        assert_eq!(shown(b"ca\xc3f", 100), PreviewContent::Icon);
    }
}
