use crate::common;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use nova_files_api::archive::ending;
use nova_files_api::{Entry, EntryId, EntryKind, IconKind, Operation, Outcome, Packing};
use nova_files_archives_ui::archive_name;
use nova_files_shell::kind::icon_kind;
use nova_files_test_folder::TestFolder;

use common::Bench;

const MAKES_AN_EMPTY_ARCHIVE: &str = "#!/bin/sh\n[ \"$1\" = --seven ] && shift 2\n: > \"$4\"\n";
const SOURCES: [(&str, EntryKind); 3] = [
    ("docs", EntryKind::Folder),
    ("notes.txt", EntryKind::File),
    ("backup.tar.gz", EntryKind::File),
];

fn fake_archiver(helper: PathBuf) -> nova_files::Archiver {
    fs::write(&helper, MAKES_AN_EMPTY_ARCHIVE).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    nova_files::Archiver::at(helper.clone()).with_seven(Some(helper))
}

#[test]
fn every_format_gets_the_same_name_from_the_window_and_the_core() {
    let scripts = TestFolder::new("archive-names-helper");
    let helper = scripts.join("archiver");
    let bench = Bench::with_archiver("archive-names", fake_archiver(helper));
    let client = bench.client();
    for packing in Packing::ALL {
        for (source, kind) in SOURCES {
            let into = bench.at(&format!("{packing:?}-{source}"));
            fs::create_dir(&into).unwrap();
            let path = bench.at(source);
            match kind {
                EntryKind::Folder => fs::create_dir_all(&path).unwrap(),
                EntryKind::File => fs::write(&path, "").unwrap(),
            }
            let first = Entry {
                id: EntryId(0),
                name: source.into(),
                kind,
            };
            let wanted = archive_name(&first, packing, |_| false).unwrap();
            fs::write(into.join(&wanted), "mine").unwrap();
            let predicted = archive_name(&first, packing, |name| into.join(name).exists()).unwrap();

            let (_, outcome, _) = client.run(Operation::Compress {
                sources: vec![path],
                into: into.clone(),
                name: wanted.clone(),
                packing,
                level: 6,
                locked: false,
            });

            assert_eq!(outcome, Outcome::Done, "{packing:?} {source}");
            assert!(
                into.join(&predicted).exists(),
                "{packing:?} {source}: {predicted:?}"
            );
            for name in [&wanted, &predicted] {
                assert_eq!(ending(name), Some(packing.ending()), "{name:?}");
                assert_eq!(
                    icon_kind(name, EntryKind::File),
                    IconKind::Archive,
                    "{name:?}"
                );
            }
            assert!(
                predicted
                    .to_str()
                    .unwrap()
                    .ends_with(&format!(" (2){}", packing.ending())),
                "{predicted:?}"
            );
        }
    }
}
