//! The gathered folders store against a real config folder.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{Gathering, Holds};

use super::*;

/// How long a test waits for the store's threads.
const WAIT: Duration = Duration::from_secs(10);

/// A home with nothing in it, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-gathered-store-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn home(&self) -> Home {
        Home {
            folder: self.0.clone(),
            config: self.0.join("config"),
            state: self.0.join("state"),
            data: self.0.join("share"),
            drive_trashes: false,
        }
    }

    fn file(&self) -> PathBuf {
        file_of(&self.home())
    }

    fn write(&self, text: &str) {
        fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        fs::write(self.file(), text).unwrap();
    }

    /// A store with a client listening, and that client's events.
    fn listening(&self) -> (Store, Receiver<Event>) {
        let store = Store::open(Some(self.home()));
        let (sink, events) = mpsc::channel();
        store.listen(ClientId(1), &(Arc::new(sink) as Arc<dyn EventSink>));
        (store, events)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The next gathered folders the client hears.
fn next(events: &Receiver<Event>) -> Arc<Gathered> {
    match events.recv_timeout(WAIT) {
        Ok(Event::Gathered(gathered)) => gathered,
        other => panic!("no gathered folders: {other:?}"),
    }
}

fn paths(folders: &[GatheredFolder]) -> Vec<&Path> {
    folders.iter().map(|folder| folder.path.as_path()).collect()
}

#[test]
fn a_new_user_starts_with_the_usual_folders_that_exist() {
    let scratch = Scratch::new("usual");
    let home = scratch.home();
    fs::create_dir_all(home.folder.join("Projects")).unwrap();
    let (_store, events) = scratch.listening();
    let gathered = next(&events);
    assert_eq!(paths(&gathered.projects), [home.folder.join("Projects")]);
    assert!(gathered.games.is_empty(), "Steam isn't installed");
    // The file is written, so a folder removed later stays removed.
    let text = fs::read_to_string(scratch.file()).unwrap();
    assert!(text.contains("~/Projects"), "{text}");
}

#[test]
fn a_new_user_without_the_usual_folders_gets_an_empty_file() {
    let scratch = Scratch::new("empty");
    let (_store, events) = scratch.listening();
    assert_eq!(*next(&events), Gathered::default());
    assert_eq!(fs::read_to_string(scratch.file()).unwrap(), "");
}

#[test]
fn a_change_before_the_first_read_keeps_the_usual_folders() {
    let scratch = Scratch::new("early");
    fs::create_dir_all(scratch.home().folder.join("Projects")).unwrap();
    let store = Store::open(Some(scratch.home()));
    store.change(GatheredChange::Add {
        place: Gathering::Games,
        path: PathBuf::from("/mnt/games"),
        holds: None,
        at: None,
        name: None,
    });
    let (sink, events) = mpsc::channel();
    store.listen(ClientId(1), &(Arc::new(sink) as Arc<dyn EventSink>));
    let mut gathered = next(&events);
    while gathered.games.is_empty() {
        gathered = next(&events);
    }
    assert_eq!(paths(&gathered.projects).len(), 1);
    assert_eq!(paths(&gathered.games), [Path::new("/mnt/games")]);
}

#[test]
fn a_file_that_isnt_utf8_is_never_rewritten() {
    let scratch = Scratch::new("latin1");
    let bytes = b"# Jeux caf\xe9\n[[games]]\npath = \"/g\"\n";
    fs::create_dir_all(scratch.file().parent().unwrap()).unwrap();
    fs::write(scratch.file(), bytes).unwrap();
    let (store, events) = scratch.listening();
    assert_eq!(*next(&events), Gathered::default());
    store.change(GatheredChange::Remove {
        place: Gathering::Games,
        path: PathBuf::from("/g"),
    });
    assert!(events.recv_timeout(Duration::from_millis(500)).is_err());
    assert_eq!(fs::read(scratch.file()).unwrap(), bytes);
}

#[test]
fn a_file_there_is_never_seeded() {
    let scratch = Scratch::new("kept");
    fs::create_dir_all(scratch.home().folder.join("Projects")).unwrap();
    scratch.write("# nothing gathered\n");
    let (_store, events) = scratch.listening();
    assert_eq!(*next(&events), Gathered::default());
    assert_eq!(
        fs::read_to_string(scratch.file()).unwrap(),
        "# nothing gathered\n"
    );
}

#[test]
fn a_link_to_nowhere_is_left_alone() {
    let scratch = Scratch::new("dangling");
    fs::create_dir_all(scratch.file().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(scratch.0.join("nowhere.toml"), scratch.file()).unwrap();
    let (_store, events) = scratch.listening();
    assert_eq!(*next(&events), Gathered::default());
    assert!(!scratch.0.join("nowhere.toml").exists());
}

#[test]
fn a_change_is_saved_and_told_to_every_client() {
    let scratch = Scratch::new("change");
    let project = scratch.0.join("one");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("flake.nix"), "").unwrap();
    let (store, events) = scratch.listening();
    assert_eq!(*next(&events), Gathered::default());
    let (sink, other) = mpsc::channel();
    store.listen(ClientId(2), &(Arc::new(sink) as Arc<dyn EventSink>));
    assert_eq!(*next(&other), Gathered::default());

    store.change(GatheredChange::Add {
        place: Gathering::Projects,
        path: project.clone(),
        holds: None,
        at: None,
        name: None,
    });
    let gathered = next(&events);
    assert_eq!(paths(&gathered.projects), [project.as_path()]);
    assert_eq!(gathered.projects[0].holds, Holds::One, "guessed");
    assert_eq!(next(&other), gathered);

    store.change(GatheredChange::Remove {
        place: Gathering::Projects,
        path: project,
    });
    assert!(next(&events).projects.is_empty());
    let text = fs::read_to_string(scratch.file()).unwrap();
    assert!(!text.contains("one"), "{text}");
}

#[test]
fn a_folder_said_to_hold_many_is_not_guessed() {
    let scratch = Scratch::new("said");
    let project = scratch.0.join("repo");
    fs::create_dir_all(project.join(".git")).unwrap();
    let (store, events) = scratch.listening();
    next(&events);
    store.change(GatheredChange::Add {
        place: Gathering::Projects,
        path: project,
        holds: Some(Holds::Many),
        at: None,
        name: None,
    });
    assert_eq!(next(&events).projects[0].holds, Holds::Many);
}

#[test]
fn an_edit_by_hand_is_seen() {
    let scratch = Scratch::new("by-hand");
    scratch.write("");
    let (_store, events) = scratch.listening();
    next(&events);
    scratch.write("[[games]]\npath = \"/mnt/games\"\n");
    assert_eq!(paths(&next(&events).games), [Path::new("/mnt/games")]);
}

#[test]
fn a_file_being_fixed_keeps_the_folders_and_takes_no_change() {
    let scratch = Scratch::new("broken");
    scratch.write("[[games]]\npath = \"/mnt/games\"\n");
    let (store, events) = scratch.listening();
    assert_eq!(next(&events).games.len(), 1);

    scratch.write("[[games]\npath = ");
    store.shared.reload();
    store.change(GatheredChange::Remove {
        place: Gathering::Games,
        path: PathBuf::from("/mnt/games"),
    });
    store.change(GatheredChange::Add {
        place: Gathering::Projects,
        path: PathBuf::from("/x"),
        holds: Some(Holds::Many),
        at: None,
        name: None,
    });
    // Nothing changed, so nothing is told; the text stays the user's.
    assert!(events.recv_timeout(Duration::from_millis(500)).is_err());
    assert_eq!(
        fs::read_to_string(scratch.file()).unwrap(),
        "[[games]\npath = "
    );
}

#[test]
fn a_forgotten_client_hears_nothing_more() {
    let scratch = Scratch::new("forget");
    scratch.write("");
    let (store, events) = scratch.listening();
    next(&events);
    store.forget(ClientId(1));
    store.change(GatheredChange::Add {
        place: Gathering::Games,
        path: PathBuf::from("/g"),
        holds: None,
        at: None,
        name: None,
    });
    assert!(events.recv_timeout(Duration::from_millis(500)).is_err());
}

#[test]
fn without_a_home_nothing_is_gathered_or_saved() {
    let store = Store::open(None);
    let (sink, events) = mpsc::channel();
    store.listen(ClientId(1), &(Arc::new(sink) as Arc<dyn EventSink>));
    assert_eq!(*next(&events), Gathered::default());
    store.change(GatheredChange::Add {
        place: Gathering::Games,
        path: PathBuf::from("/g"),
        holds: None,
        at: None,
        name: None,
    });
    assert!(events.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn a_file_up_to_256_kib_is_read_and_a_bigger_one_is_not() {
    let scratch = Scratch::new("limit");
    let line = "#".repeat(256 * 1024 - 1) + "\n";
    scratch.write(&line);
    assert_eq!(read_text(&scratch.file()).as_deref(), Some(line.as_str()));
    scratch.write(&format!("{line}\n"));
    assert_eq!(read_text(&scratch.file()), None);
}

/// Waits for `place` to change, skipping the folders being sent again.
fn changed(events: &Receiver<Event>, place: Gathering) {
    loop {
        match events.recv_timeout(WAIT) {
            Ok(Event::GatheredChanged(got)) if got == place => return,
            Ok(Event::Gathered(_)) => {}
            other => panic!("expected {place:?} to change, got {other:?}"),
        }
    }
}

#[test]
fn a_one_project_folder_changes_only_when_it_comes_or_goes() {
    let scratch = Scratch::new("one-watch");
    let project = scratch.0.join("Skills");
    fs::create_dir_all(&project).unwrap();
    scratch.write(&format!(
        "[[projects]]\npath = \"{}\"\nholds = \"one\"\n",
        project.display()
    ));
    let (_store, events) = scratch.listening();
    next(&events);
    fs::write(project.join("notes.md"), "saved").unwrap();
    fs::write(scratch.0.join("beside.txt"), "").unwrap();
    assert!(events.recv_timeout(Duration::from_millis(600)).is_err());
    fs::remove_dir_all(&project).unwrap();
    changed(&events, Gathering::Projects);
    fs::create_dir(&project).unwrap();
    changed(&events, Gathering::Projects);
}

#[test]
fn a_folder_of_projects_changes_when_one_is_made_and_stops_when_removed() {
    let scratch = Scratch::new("many-watch");
    let games = scratch.0.join("games");
    scratch.write(&format!("[[games]]\npath = \"{}\"\n", games.display()));
    let (store, events) = scratch.listening();
    next(&events);
    // Not there yet: it counts once it appears.
    fs::create_dir(&games).unwrap();
    changed(&events, Gathering::Games);
    fs::create_dir(games.join("Celeste")).unwrap();
    changed(&events, Gathering::Games);
    // A loose file saved there is no change.
    fs::write(games.join("steam.log"), "").unwrap();
    assert!(events.recv_timeout(Duration::from_millis(600)).is_err());

    store.change(GatheredChange::Remove {
        place: Gathering::Games,
        path: games.clone(),
    });
    assert!(next(&events).games.is_empty());
    fs::create_dir(games.join("Hades")).unwrap();
    assert!(events.recv_timeout(Duration::from_millis(600)).is_err());
}

#[test]
fn listing_before_any_client_asked_still_follows_the_file() {
    let scratch = Scratch::new("list-first");
    scratch.write("");
    let store = Store::open(Some(scratch.home()));
    assert_eq!(store.folders_of(Gathering::Games)().len(), 0);
    // The listing started the watch; asking now starts nothing more.
    let (sink, events) = mpsc::channel();
    store.listen(ClientId(1), &(Arc::new(sink) as Arc<dyn EventSink>));
    assert_eq!(*next(&events), Gathered::default());
    scratch.write("[[games]]\npath = \"/mnt/games\"\n");
    assert_eq!(paths(&next(&events).games), [Path::new("/mnt/games")]);
    assert_eq!(store.folders_of(Gathering::Games)().len(), 1);
}

#[test]
fn an_added_folder_keeps_its_name_and_a_file_is_never_added() {
    let scratch = Scratch::new("named");
    let (store, events) = scratch.listening();
    assert_eq!(*next(&events), Gathered::default());
    let file = scratch.0.join("notes.txt");
    fs::write(&file, "").unwrap();
    store.change(GatheredChange::Add {
        place: Gathering::Games,
        path: file,
        holds: None,
        at: None,
        name: None,
    });
    // Missing: on a drive not plugged in.
    store.change(GatheredChange::Add {
        place: Gathering::Games,
        path: PathBuf::from("/mnt/games"),
        holds: Some(Holds::Many),
        at: None,
        name: Some("Games SSD".to_owned()),
    });
    let gathered = next(&events);
    assert_eq!(paths(&gathered.games), [Path::new("/mnt/games")]);
    assert_eq!(gathered.games[0].name.as_deref(), Some("Games SSD"));
}
