//! The copy job: plan everything first, then make each item in order
//! (folders, files through part files, links as links), recording each in
//! the journal, and last give each folder made its source's mode, times
//! and attributes, deepest first, so a read-only folder can still be
//! filled. Before the job says it is done, every folder it added entries
//! to is synced, so what it reports made is on the disk.
//!
//! A name already taken at the target fails the job there; nothing is
//! replaced. What was made before a failure or a cancel stays, whole.

use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, File};
use std::os::unix::fs::{DirBuilderExt as _, symlink};
use std::path::{Path, PathBuf};

use nova_files_api::Outcome;

use super::Ended;
use super::attrs::{copy_mode_and_times, copy_xattrs};
use super::copy_file::copy_file;
use super::journal::Record;
use super::plan::{Item, Kind, plan};
use super::running::Running;
use super::stop::Stop;

/// A folder being filled is the user's alone; it gets its source's mode
/// once full.
const FILLING_MODE: u32 = 0o700;

/// Copies each of `sources` into `into`.
pub fn copy(running: &mut Running, sources: &[PathBuf], into: &Path) -> Ended {
    running.record(Record::Operation("copy"));
    running.record(Record::Into(into));
    for source in sources {
        running.record(Record::Source(source));
    }
    let plan = match plan(sources, into, &running.shared.control) {
        Ok(plan) => plan,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    let items = u64::try_from(plan.items.len()).unwrap_or(u64::MAX);
    running.reporter.planned(plan.bytes, items);
    let mut folders = Vec::new();
    let mut touched = BTreeSet::new();
    let mut result = Ok(());
    for (index, item) in plan.items.iter().enumerate() {
        result = make(running, item, index);
        if result.is_err() {
            break;
        }
        if let Some(parent) = item.to.parent() {
            touched.insert(parent);
        }
        if item.kind == Kind::Folder {
            folders.push(item);
        }
    }
    for folder in folders.iter().rev() {
        // Held while paused; a cancel leaves the rest as they are.
        if let Err(cancelled) = running.shared.control.checkpoint() {
            result = Err(cancelled.into());
            break;
        }
        finish_folder(folder);
    }
    // So what the job reports made is on the disk. A failure means only
    // that a crash right now could lose an entry, so it is logged.
    for folder in &touched {
        if let Err(error) = File::open(folder).and_then(|folder| folder.sync_all()) {
            log::warn!("can't sync the folder {}: {error}", folder.display());
        }
    }
    let made_any = !touched.is_empty();
    running.reporter.tell();
    let changed = if made_any {
        vec![into.to_path_buf()]
    } else {
        Vec::new()
    };
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        changed,
    )
}

/// Makes `item`, the `index`th of its job.
fn make(running: &mut Running, item: &Item, index: usize) -> Result<(), Stop> {
    running.shared.control.checkpoint()?;
    let mut left = 0;
    match item.kind {
        Kind::Folder => DirBuilder::new()
            .mode(FILLING_MODE)
            .create(&item.to)
            .map_err(|error| Stop::at(&item.to, &error))?,
        Kind::File { size } => {
            let part = item
                .to
                .with_file_name(format!(".nova-part-{}-{index}", running.job.0));
            running.record(Record::Part(&part));
            let mut copied = 0;
            let reporter = &mut running.reporter;
            copy_file(
                &item.from,
                &item.to,
                &part,
                &running.shared.control,
                &mut |bytes| {
                    copied += bytes;
                    reporter.bytes(bytes);
                },
            )?;
            left = size.saturating_sub(copied);
        }
        Kind::Link => {
            let target = fs::read_link(&item.from).map_err(|error| Stop::at(&item.from, &error))?;
            symlink(target, &item.to).map_err(|error| Stop::at(&item.to, &error))?;
        }
    }
    running.record(Record::Made(&item.to));
    running.reporter.item(left);
    Ok(())
}

/// Gives the folder made for `item` its source's attributes, mode and
/// times. The copy is whole without them, so a failure is only logged.
fn finish_folder(item: &Item) {
    let finished = File::open(&item.from).and_then(|source| {
        let target = File::open(&item.to)?;
        copy_xattrs(&source, &target)?;
        copy_mode_and_times(&source.metadata()?, &target)
    });
    if let Err(error) = finished {
        log::warn!(
            "can't give {} its source's mode and times: {error}",
            item.to.display()
        );
    }
}
