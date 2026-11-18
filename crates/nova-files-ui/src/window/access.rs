//! The Properties page's Access: who may read and change what it shows,
//! and changing that.
//!
//! The permission bits read as one level each for the owner, the group
//! and everyone else: Read and write, Read only, or No access, from the
//! read and write bits. Several items show a level where they all have
//! the same one. Choosing a level starts one of the core's `ChangeAccess`
//! jobs on the page's paths; a folder gets its search bit along with
//! read, so a folder that can be read can also be opened. "Run as a
//! program" turns a file's run bits on for whoever may read it, or off for
//! all. "Also everything inside" makes the next changes reach what the
//! folders hold too. When the job ends the page asks for its paths again,
//! so it shows what the disk now says. A page whose paths the user can't
//! change, or on a drive mounted read-only, offers no changes.

use std::sync::Arc;

use gpui::{Context, Pixels, Point};
use nova_files_api::{AccessChange, ModeChange, Operation, Properties, Reach};

use super::MainWindow;
use crate::assets::Icon;
use crate::menu::{Look, MenuAction, MenuItem};

/// Read and write, for one of owner, group and everyone else, and the
/// bits that go together.
const READ: u32 = 0o4;
const WRITE: u32 = 0o2;
const READ_WRITE: u32 = 0o6;
const READ_RUN: u32 = 0o5;
const EVERY_BIT: u32 = 0o7;
/// The run bit of all three.
const RUN_ALL: u32 = 0o111;
/// The read bits of all three, and how far each run bit sits after its
/// read bit.
const READ_ALL: u32 = 0o444;
const READ_TO_RUN: u32 = 2;
/// The bits each of owner, group and everyone else has.
const CLASS_BITS: u32 = 3;
/// Where the group's and the owner's bits start.
const GROUP_SHIFT: u32 = CLASS_BITS;
const OWNER_SHIFT: u32 = GROUP_SHIFT + CLASS_BITS;

/// Whose access a row of the card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Owner,
    Group,
    Others,
}

/// How much one of them may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    ReadWrite,
    ReadOnly,
    NoAccess,
}

impl Who {
    pub const ALL: [Self; 3] = [Self::Owner, Self::Group, Self::Others];

    /// How far its bits sit from the lowest.
    const fn shift(self) -> u32 {
        match self {
            Self::Owner => OWNER_SHIFT,
            Self::Group => GROUP_SHIFT,
            Self::Others => 0,
        }
    }

    /// Its level in `mode`.
    pub const fn level(self, mode: u32) -> Level {
        let bits = mode >> self.shift();
        if bits & READ == 0 {
            Level::NoAccess
        } else if bits & WRITE == 0 {
            Level::ReadOnly
        } else {
            Level::ReadWrite
        }
    }

    /// What giving it `level` does to files and to folders.
    pub const fn change_to(self, level: Level) -> (ModeChange, ModeChange) {
        let shift = self.shift();
        match level {
            Level::ReadWrite => (
                ModeChange {
                    set: READ_WRITE << shift,
                    clear: 0,
                },
                ModeChange {
                    set: EVERY_BIT << shift,
                    clear: 0,
                },
            ),
            Level::ReadOnly => (
                ModeChange {
                    set: READ << shift,
                    clear: WRITE << shift,
                },
                ModeChange {
                    set: READ_RUN << shift,
                    clear: WRITE << shift,
                },
            ),
            Level::NoAccess => {
                let none = ModeChange {
                    set: 0,
                    clear: EVERY_BIT << shift,
                };
                (none, none)
            }
        }
    }
}

impl Level {
    pub const ALL: [Self; 3] = [Self::ReadWrite, Self::ReadOnly, Self::NoAccess];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadWrite => "Read and write",
            Self::ReadOnly => "Read only",
            Self::NoAccess => "No access",
        }
    }

    const fn icon(self) -> Icon {
        match self {
            Self::ReadWrite => Icon::Rename,
            Self::ReadOnly => Icon::Eye,
            Self::NoAccess => Icon::Close,
        }
    }
}

/// Whether a file with `mode` runs as a program: anyone may run it.
pub const fn runs(mode: u32) -> bool {
    mode & RUN_ALL != 0
}

/// The change that makes files run as programs for whoever may read them,
/// given `mode`, or for none of them.
pub const fn run_change(run: bool, mode: u32) -> ModeChange {
    if run {
        ModeChange {
            set: (mode & READ_ALL) >> READ_TO_RUN,
            clear: 0,
        }
    } else {
        ModeChange {
            set: 0,
            clear: RUN_ALL,
        }
    }
}

/// The level `who` has on every item of `read`, when they share one.
pub fn shared_level(read: &[Arc<Properties>], who: Who) -> Option<Level> {
    let mut levels = read.iter().map(|one| who.level(one.mode));
    let first = levels.next()?;
    levels.all(|level| level == first).then_some(first)
}

impl MainWindow {
    /// Whether the user may change the access of everything on the page.
    pub fn may_change_access(&self) -> bool {
        self.properties_page().is_some_and(|page| {
            let read = page.read();
            !read.is_empty()
                && read
                    .iter()
                    .all(|one| one.may_change && one.read_only != Some(true))
        })
    }

    /// Whether "Also everything inside" is ticked on the page on screen.
    pub fn reaches_inside(&self) -> bool {
        self.properties_page().is_some_and(|page| page.inside)
    }

    /// Ticks or clears "Also everything inside".
    pub fn toggle_inside(&mut self, cx: &mut Context<Self>) {
        if self.properties_page().is_none() {
            return;
        }
        if let Some(page) = self.properties.as_mut() {
            page.inside = !page.inside;
            cx.notify();
        }
    }

    /// Opens the menu of levels for `who` at `at`, a tick on the one it
    /// has.
    pub(crate) fn open_access_menu(&mut self, who: Who, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.may_change_access() {
            return;
        }
        let shown = self
            .properties_page()
            .and_then(|page| shared_level(&page.read(), who));
        let items = Level::ALL
            .into_iter()
            .map(|level| {
                let action = MenuAction::SetAccess(who, level);
                if shown == Some(level) {
                    MenuItem::new(Icon::Check, level.label(), action).with_look(Look::On)
                } else {
                    MenuItem::new(level.icon(), level.label(), action)
                }
            })
            .collect();
        self.open_menu(at, items, cx);
    }

    /// Gives `who` `level` on everything on the page.
    pub(crate) fn set_access(&mut self, who: Who, level: Level) {
        let (files, folders) = who.change_to(level);
        self.change_access(files, folders);
    }

    /// Makes the page's lone file run as a program, or stop.
    pub fn toggle_runs(&mut self) {
        let Some(mode) = self
            .properties_page()
            .and_then(|page| page.read().first().map(|one| one.mode))
        else {
            return;
        };
        self.change_access(run_change(!runs(mode), mode), ModeChange::default());
    }

    /// Starts a job changing the page's paths as `files` and `folders`
    /// say, reaching inside when the page says so.
    fn change_access(&mut self, files: ModeChange, folders: ModeChange) {
        if !self.may_change_access() {
            return;
        }
        let Some(page) = self.properties.as_ref() else {
            return;
        };
        let reach = if page.inside {
            Reach::Inside
        } else {
            Reach::Chosen
        };
        let paths = page.paths();
        self.start_job(Operation::ChangeAccess {
            paths,
            change: AccessChange {
                owner: None,
                group: None,
                files,
                folders,
                reach,
            },
        });
    }

    /// A job changing access ended: the page shows what the disk says now.
    pub(super) fn access_changed(&mut self) {
        if self.properties_page().is_some() {
            self.ask_properties();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run, for one of owner, group and everyone else.
    const RUN: u32 = 0o1;

    #[test]
    fn each_level_reads_from_the_read_and_write_bits() {
        let mode = 0o640;
        assert_eq!(Who::Owner.level(mode), Level::ReadWrite);
        assert_eq!(Who::Group.level(mode), Level::ReadOnly);
        assert_eq!(Who::Others.level(mode), Level::NoAccess);
        // Write without read is no access.
        assert_eq!(Who::Others.level(0o002), Level::NoAccess);
    }

    #[test]
    fn a_level_given_reads_back_and_folders_open_with_read() {
        let modes = [0o000, 0o777, 0o640, 0o751];
        let cases = Who::ALL.into_iter().flat_map(|who| {
            Level::ALL
                .into_iter()
                .flat_map(move |level| modes.map(|mode| (who, level, mode)))
        });
        for (who, level, mode) in cases {
            let (files, folders) = who.change_to(level);
            assert_eq!(who.level(files.applied_to(mode)), level);
            let folder = folders.applied_to(mode);
            assert_eq!(who.level(folder), level);
            let opens = folder >> who.shift() & RUN != 0;
            assert_eq!(opens, level != Level::NoAccess, "{who:?} {level:?}");
            let others = Who::ALL.into_iter().filter(|other| *other != who);
            assert!(
                others
                    .into_iter()
                    .all(|other| other.level(folder) == other.level(mode))
            );
        }
    }

    #[test]
    fn the_joined_bits_are_their_parts() {
        assert_eq!(READ_WRITE, READ | WRITE);
        assert_eq!(READ_RUN, READ | RUN);
        assert_eq!(EVERY_BIT, READ | WRITE | RUN);
    }

    #[test]
    fn several_items_share_a_level_only_when_all_have_it() {
        let with = |mode| {
            let nobody = nova_files_api::Account { id: 0, name: None };
            Arc::new(Properties {
                path: std::path::PathBuf::from("/a"),
                kind: nova_files_api::PathKind::File,
                bytes: 0,
                on_disk: 0,
                modified: None,
                accessed: None,
                changed: None,
                created: None,
                mode,
                owner: nobody.clone(),
                group: nobody,
                may_change: true,
                groups: Vec::new(),
                read_only: None,
            })
        };
        let same = [with(0o640), with(0o600)];
        assert_eq!(shared_level(&same, Who::Owner), Some(Level::ReadWrite));
        assert_eq!(shared_level(&same, Who::Group), None);
        assert_eq!(shared_level(&[], Who::Owner), None);
    }

    #[test]
    fn a_program_runs_for_whoever_may_read_it() {
        assert!(!runs(0o644));
        assert!(runs(0o744));
        assert!(runs(0o001));
        assert_eq!(run_change(true, 0o640).applied_to(0o640), 0o750);
        assert_eq!(run_change(true, 0o644).applied_to(0o644), 0o755);
        assert_eq!(run_change(false, 0o755).applied_to(0o755), 0o644);
    }
}
