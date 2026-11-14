//! Looking into a file or folder for [`Command::Preview`], the quick look.
//!
//! A folder shows its first entries, as a listing orders them, and how
//! many it holds. A regular file is read from its start, at most
//! [`PREVIEW_TEXT_BYTES`]: when that is UTF-8 text (allowing a character
//! cut at the end) with no NUL byte, it shows as text; anything else, and
//! anything that isn't a regular file (a device, pipe or socket, which is
//! never opened), shows as its icon. A link shows what it points to. The
//! kind's name comes from the file's name, through the MIME database.
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
use std::sync::{Arc, OnceLock};
use std::thread;

use nova_files_api::{
    Event, EventSink, FileTypes as _, PREVIEW_ENTRIES, PREVIEW_TEXT_BYTES, Preview, PreviewContent,
    PreviewId, Unreadable,
};
use rustix::fs::OFlags;

use crate::list::{self, Stopped};
use crate::mime::MimeTypes;

const THREAD_NAME: &str = "nova-files-preview";

/// A NUL byte marks a file as binary.
const NUL: u8 = 0;

/// The preview that counts for one client, or none.
#[derive(Debug, Default)]
pub struct Current(AtomicU64);

/// No preview counts: the client never asked, or left.
const NONE: u64 = u64::MAX;

impl Current {
    pub const fn new() -> Self {
        Self(AtomicU64::new(NONE))
    }

    fn is(&self, id: PreviewId) -> bool {
        self.0.load(Ordering::SeqCst) == id.0
    }

    /// Stops the preview running, if any.
    pub fn cancel(&self) {
        self.0.store(NONE, Ordering::SeqCst);
    }
}

/// What one request needs.
pub struct Request {
    pub id: PreviewId,
    pub path: PathBuf,
    pub current: Arc<Current>,
    pub types: Arc<OnceLock<MimeTypes>>,
}

/// Looks into `request`'s path on a thread of its own and hands the answer
/// to `events`, unless a newer request replaced it first.
pub fn answer(events: &Arc<dyn EventSink>, request: Request) {
    request.current.0.store(request.id.0, Ordering::SeqCst);
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
        match file(path, &metadata) {
            Ok(content) => content,
            Err(error) => return Some(unreadable(list::unreadable(&error))),
        }
    };
    let kind_name = (!metadata.is_dir())
        .then(|| path.file_name())
        .flatten()
        .and_then(|name| {
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

/// A folder's first entries and how many it holds.
fn folder(path: &Path, cancelled: impl Fn() -> bool) -> Result<PreviewContent, Stopped> {
    let mut first = list::read_folder(path, cancelled)?;
    let count = first.len();
    first.truncate(PREVIEW_ENTRIES);
    Ok(PreviewContent::Folder { first, count })
}

/// What a file that isn't a folder shows: its text, or its icon.
fn file(path: &Path, metadata: &Metadata) -> io::Result<PreviewContent> {
    if !metadata.is_file() {
        return Ok(PreviewContent::Icon);
    }
    // Swapped for a pipe since the look, it still won't wait for a writer.
    let opened = File::options()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits().cast_signed())
        .open(path)?;
    let mut start = Vec::with_capacity(PREVIEW_TEXT_BYTES);
    let limit = u64::try_from(PREVIEW_TEXT_BYTES).unwrap_or(u64::MAX);
    opened.take(limit).read_to_end(&mut start)?;
    Ok(text(start, metadata.len()))
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
            current: Arc::clone(&current),
            types: Arc::new(OnceLock::new()),
        };
        let (old, new) = (request(1), request(2));
        current.0.store(new.id.0, Ordering::SeqCst);
        assert!(event(&old).is_none());
        assert!(event(&new).is_some());
        current.cancel();
        assert!(event(&new).is_none());
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
