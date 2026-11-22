use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::own_folders;

use nova_files_api::{
    Command, EntryId, Event, FileIcon, IconKind, ListingId, Order, ThumbnailSize,
};
use nova_files_test_folder::TestFolder;

use crate::common::Core;
use own_folders::run_again_in;

const ROOT_VAR: &str = "NOVA_FILES_ICON_THEME_TEST_ROOT";
const TEST_NAME: &str = "file_icons_follow_the_desktops_icon_theme";
const IDLE_EXIT: Duration = Duration::from_secs(5);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const THEME: &str = "Desk";

#[test]
fn file_icons_follow_the_desktops_icon_theme() {
    match std::env::var_os(ROOT_VAR) {
        Some(root) => icons_through_the_core(Path::new(&root)),
        None => run_again_in_folders_of_its_own(),
    }
}

fn run_again_in_folders_of_its_own() {
    let folder = TestFolder::new("icon-theme");
    let root = folder.path();
    write(root, "data/mime/globs2", "50:image/png:*.png\n");
    write(
        root,
        &format!("data/icons/{THEME}/index.theme"),
        "[Icon Theme]\nDirectories=m\n[m]\nSize=48\nType=Scalable\nMinSize=8\nMaxSize=256\n",
    );
    write(
        root,
        &format!("data/icons/{THEME}/m/image-png.svg"),
        "<svg/>",
    );
    write(root, "files/a.png", "");
    run_again_in(root, ROOT_VAR, TEST_NAME);
}

fn icons_through_the_core(root: &Path) {
    let core = Core(nova_files::spawn_with_idle_exit(IDLE_EXIT).unwrap());
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    let icon_of_the_file = |id: u64| {
        connection
            .send(Command::ListFolder {
                id: ListingId(id),
                path: root.join("files"),
                order: Order::default(),
            })
            .unwrap();
        wait_for(
            &events,
            |event| matches!(event, Event::FolderListed(listing) if listing.id == ListingId(id)),
        );
        connection
            .send(Command::Rows {
                id: ListingId(id),
                first: EntryId(0),
                count: 1,
                size: ThumbnailSize::Normal,
            })
            .unwrap();
        match wait_for(&events, |event| matches!(event, Event::DetailsRead { .. })) {
            Event::DetailsRead { icons, .. } => icons[0].clone(),
            _ => unreachable!(),
        }
    };

    assert_eq!(icon_of_the_file(1), FileIcon::BuiltIn(IconKind::Picture));

    connection
        .send(nova_files_look_api::Command::DesktopIconThemes(vec![
            "Missing".to_owned(),
            THEME.to_owned(),
        ]))
        .unwrap();
    let FileIcon::Theme { icon, .. } = icon_of_the_file(2) else {
        panic!("no theme icon");
    };
    assert_eq!(
        icon.path,
        root.join(format!("data/icons/{THEME}/m/image-png.svg"))
    );
}

fn wait_for(events: &Receiver<Event>, wanted: impl Fn(&Event) -> bool) -> Event {
    loop {
        let event = events.recv_timeout(ANSWER_TIMEOUT).unwrap();
        if wanted(&event) {
            return event;
        }
    }
}

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
