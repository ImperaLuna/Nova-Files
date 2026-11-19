//! A file or folder's properties, and how much space files and folders
//! take: what the properties feature's two halves say to each other.

#![forbid(unsafe_code)]

mod properties;
mod size;

use std::path::PathBuf;
use std::sync::Arc;

use nova_files_api::Unreadable;

pub use properties::{Account, PathKind, Properties, PropertiesId};
pub use size::{MeasureId, ScanState, Share, Size};

/// What a client asks of the properties feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Read the properties of what is at `path`, without following a link.
    /// The answer is [`Event::Properties`] or [`Event::PropertiesUnreadable`]
    /// with the same `id`.
    Properties { id: PropertiesId, path: PathBuf },
    /// Measure how much space everything under `paths` (absolute paths)
    /// takes, together: what two paths share counts once. The answer is
    /// [`Event::SizeMeasured`] with the same `id`, a few times a second
    /// while the scan runs, then once more when it ends. A client has one
    /// scan at a time: a newer `Measure` from it stops this one, which then
    /// says nothing more.
    Measure { id: MeasureId, paths: Vec<PathBuf> },
    /// Measure what each entry of `folder` (an absolute path) takes. The
    /// answer is [`Event::SpaceMeasured`] with the same `id`, a few times a
    /// second while the scan runs, then once more when it ends; or
    /// [`Event::SpaceUnreadable`]. A client has one space scan at a time,
    /// beside its one `Measure`: a newer `Space` from it stops this one,
    /// which then says nothing more.
    Space { id: MeasureId, folder: PathBuf },
    /// Stop the client's scan `id`, a `Measure` or a `Space`, if it still
    /// runs (both, when the client gave them the same id). It says nothing
    /// more.
    StopMeasure(MeasureId),
}

/// What the properties feature tells a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The properties of [`Command::Properties`] `id`'s path.
    Properties {
        id: PropertiesId,
        properties: Arc<Properties>,
    },
    /// [`Command::Properties`] `id`'s path couldn't be looked at.
    PropertiesUnreadable {
        id: PropertiesId,
        path: PathBuf,
        reason: Unreadable,
    },
    /// What [`Command::Measure`] `id` counted so far, and whether it ended.
    SizeMeasured {
        id: MeasureId,
        size: Size,
        state: ScanState,
    },
    /// What the entries of [`Command::Space`] `id`'s folder take so far:
    /// the first answer names every entry, later ones only those whose
    /// totals changed since. `state` says whether it ended.
    SpaceMeasured {
        id: MeasureId,
        shares: Vec<Share>,
        state: ScanState,
    },
    /// [`Command::Space`] `id`'s folder couldn't be listed.
    SpaceUnreadable { id: MeasureId, reason: Unreadable },
}
