use std::ffi::OsStr;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{EntryId, EntryKind, FileIcon, IconKind};
use nova_files_disk::ports::Icons;

use super::*;

const WAIT: Duration = Duration::from_secs(5);
const KNOWN: &str = "/no-such-folder-for-nova-files/a.zip/in";
const BROKEN: &str = "/no-such-folder-for-nova-files/b.zip/in";

struct Plain;

impl Icons for Plain {
    fn of(&self, _: &OsStr, _: EntryKind) -> FileIcon {
        FileIcon::BuiltIn(IconKind::Folder)
    }

    fn kind_name(&self, _: &OsStr, _: EntryKind) -> Option<Arc<str>> {
        None
    }

    fn follow_desktop(&self, _: &[String]) {}
}

struct OneArchive;

impl InnerFolders for OneArchive {
    fn list(&self, path: &Path, _: Order, _: HiddenFiles) -> Option<Inner> {
        let entry = Entry {
            id: EntryId(0),
            name: "inside.txt".into(),
            kind: EntryKind::File,
        };
        match path.to_str() {
            Some(KNOWN) => Some(Ok((vec![entry], vec![Details::default()]))),
            Some(BROKEN) => Some(Err(Unreadable::Failed)),
            _ => None,
        }
    }
}

fn listed(path: &str) -> Event {
    let (sender, events): (_, Receiver<Event>) = mpsc::channel();
    let icons: Arc<dyn Icons> = Arc::new(Plain);
    let client = Client::new(Arc::new(sender), None, &icons);
    let inner: Arc<dyn InnerFolders> = Arc::new(OneArchive);
    let asked = Shown {
        hidden: HiddenFiles::Shown,
        ..Shown::default()
    };

    list_folder(&client, (ListingId(1), path.into()), asked, &inner, None);

    loop {
        match events.recv_timeout(WAIT).unwrap() {
            Event::DetailsRead { .. } => {}
            event => return event,
        }
    }
}

#[test]
fn a_path_no_folder_holds_is_listed_from_the_inner_folders_that_know_it() {
    let Event::FolderListed(listing) = listed(KNOWN) else {
        panic!("expected the inner listing");
    };
    let names: Vec<_> = listing.entries.iter().map(|entry| &entry.name).collect();
    assert_eq!(names, ["inside.txt"]);

    assert_eq!(
        listed(BROKEN),
        Event::FolderUnreadable {
            id: ListingId(1),
            path: BROKEN.into(),
            reason: Unreadable::Failed,
        }
    );
    let nowhere = "/no-such-folder-for-nova-files/plain";
    assert_eq!(
        listed(nowhere),
        Event::FolderUnreadable {
            id: ListingId(1),
            path: nowhere.into(),
            reason: Unreadable::Missing,
        }
    );
}
