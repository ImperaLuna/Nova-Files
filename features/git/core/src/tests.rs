use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

use nova_files_git_api::{Head, Repo};
use nova_files_test_folder::TestFolder;

use super::*;
use crate::status::parse;

fn git(folder: &Path, args: &[&str]) {
    let ran = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .unwrap();
    assert!(ran.status.success(), "git {args:?}: {ran:?}");
}

fn committed(folder: &Path) {
    fs::create_dir_all(folder).unwrap();
    git(folder, &["init", "--quiet"]);
    fs::write(folder.join("kept.txt"), "one").unwrap();
    git(folder, &["add", "kept.txt"]);
    git(folder, &["commit", "--quiet", "-m", "first"]);
}

fn branch(name: &str, changed: bool, (ahead, behind): (u32, u32)) -> Repo {
    Repo {
        head: Head::Branch(name.to_owned()),
        changed,
        ahead,
        behind,
    }
}

#[test]
fn a_clean_repo_shows_its_branch_and_no_changes() {
    let root = TestFolder::new("git-clean");
    let repo = root.join("repo");
    committed(&repo);
    assert_eq!(repo_at(&repo), Some(branch("main", false, (0, 0))));
}

#[test]
fn edited_new_and_deleted_files_count_as_changes_but_ignored_ones_do_not() {
    let root = TestFolder::new("git-changes");
    let repo = root.join("repo");
    committed(&repo);
    fs::write(repo.join(".gitignore"), "build/\n").unwrap();
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "--quiet", "-m", "ignore"]);
    fs::create_dir_all(repo.join("build")).unwrap();
    fs::write(repo.join("build").join("out.o"), "x").unwrap();
    assert_eq!(repo_at(&repo), Some(branch("main", false, (0, 0))));
    fs::write(repo.join("kept.txt"), "two").unwrap();
    assert_eq!(repo_at(&repo), Some(branch("main", true, (0, 0))));
    git(&repo, &["checkout", "--quiet", "kept.txt"]);
    fs::write(repo.join("new.txt"), "new").unwrap();
    assert_eq!(repo_at(&repo), Some(branch("main", true, (0, 0))));
    fs::remove_file(repo.join("new.txt")).unwrap();
    fs::remove_file(repo.join("kept.txt")).unwrap();
    assert_eq!(repo_at(&repo), Some(branch("main", true, (0, 0))));
}

#[test]
fn commits_not_pushed_and_not_pulled_are_counted_against_the_upstream() {
    let root = TestFolder::new("git-ahead");
    let upstream = root.join("upstream");
    committed(&upstream);
    let clone = root.join("clone");
    let (from, to) = (upstream.to_string_lossy(), clone.to_string_lossy());
    git(root.path(), &["clone", "--quiet", &from, &to]);
    fs::write(upstream.join("theirs.txt"), "theirs").unwrap();
    git(&upstream, &["add", "theirs.txt"]);
    git(&upstream, &["commit", "--quiet", "-m", "theirs"]);
    git(&clone, &["fetch", "--quiet"]);
    for name in ["a.txt", "b.txt"] {
        fs::write(clone.join(name), name).unwrap();
        git(&clone, &["add", name]);
        git(&clone, &["commit", "--quiet", "-m", name]);
    }
    assert_eq!(repo_at(&clone), Some(branch("main", false, (2, 1))));
}

#[test]
fn a_detached_checkout_shows_its_short_commit_id() {
    let root = TestFolder::new("git-detached");
    let repo = root.join("repo");
    committed(&repo);
    git(&repo, &["checkout", "--quiet", "--detach"]);
    let Some(Repo {
        head: Head::Detached(id),
        ..
    }) = repo_at(&repo)
    else {
        panic!("not detached: {:?}", repo_at(&repo));
    };
    assert_eq!(id.len(), "0123456".len());
}

#[test]
fn a_new_repo_without_commits_shows_its_branch() {
    let root = TestFolder::new("git-unborn");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    assert_eq!(repo_at(&repo), Some(branch("main", false, (0, 0))));
}

#[test]
fn only_the_top_of_a_repo_is_a_repo() {
    let root = TestFolder::new("git-top");
    let repo = root.join("repo");
    committed(&repo);
    let inner = repo.join("inner");
    fs::create_dir_all(&inner).unwrap();
    assert_eq!(repo_at(&inner), None);
    assert_eq!(repo_at(root.path()), None);
}

#[test]
fn a_status_header_that_names_no_branch_is_no_repo() {
    assert_eq!(parse(&b"# branch.oid 0123456789abcdef\n"[..]), None);
    let unknown = b"# branch.oid 0123456789\n# branch.head main\n# stash 3\n";
    assert_eq!(parse(&unknown[..]), Some(branch("main", false, (0, 0))));
}

fn reading(folders: Vec<PathBuf>) -> (Reader, mpsc::Receiver<Event>) {
    let (sender, answers) = mpsc::channel::<Event>();
    let events: Arc<dyn EventSink> = Arc::new(sender);
    let reader = Reader::default();
    reader.read(ClientId(1), &events, folders);
    (reader, answers)
}

#[test]
fn each_asked_folder_is_answered_in_turn_and_forgetting_ends_the_reading() {
    let root = TestFolder::new("git-reader");
    let repo = root.join("repo");
    committed(&repo);
    let plain = root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let (reader, answers) = reading(vec![repo.clone(), plain.clone()]);
    let first: Vec<Event> = answers.iter().take(2).collect();
    assert_eq!(
        first,
        [
            Event::Repo {
                folder: repo,
                repo: Some(branch("main", false, (0, 0))),
            },
            Event::Repo {
                folder: plain,
                repo: None,
            },
        ]
    );
    reader.forget(ClientId(1));
    assert_eq!(answers.iter().count(), 0);
}

#[test]
fn a_commit_or_a_new_branch_is_read_again() {
    let root = TestFolder::new("git-follow");
    let repo = root.join("repo");
    committed(&repo);
    fs::write(repo.join("kept.txt"), "two").unwrap();
    let (reader, answers) = reading(vec![repo.clone()]);
    let at = |repo: Option<Repo>| Event::Repo {
        folder: root.join("repo"),
        repo,
    };
    assert_eq!(
        answers.recv().unwrap(),
        at(Some(branch("main", true, (0, 0))))
    );
    git(&repo, &["commit", "--quiet", "-am", "second"]);
    let clean = at(Some(branch("main", false, (0, 0))));
    assert!(answers.iter().any(|event| event == clean));
    git(&repo, &["checkout", "--quiet", "-b", "other"]);
    let other = at(Some(branch("other", false, (0, 0))));
    assert!(answers.iter().any(|event| event == other));
    reader.forget(ClientId(1));
}

#[test]
fn a_repo_whose_config_can_run_programs_is_not_read() {
    let root = TestFolder::new("git-config-runs");
    let repo = root.join("repo");
    committed(&repo);
    let ran = root.join("ran");
    let config = repo.join(".git").join("config");
    let mut text = fs::read_to_string(&config).unwrap();
    let line = format!("[core]\n\tfsmonitor = \"touch {}\"\n", ran.display());
    text.push_str(&line);
    fs::write(&config, text).unwrap();
    assert_eq!(repo_at(&repo), None);
    assert!(!ran.exists());
}

#[test]
fn config_that_names_a_program_is_found_however_it_is_written() {
    let root = TestFolder::new("git-config-spellings");
    let repo = root.join("repo");
    committed(&repo);
    let config = repo.join(".git").join("config");
    let kept = fs::read(&config).unwrap();
    let ran = root.join("ran");
    fs::write(repo.join(".gitattributes"), "* filter=evil\n").unwrap();
    let clean = format!("clean = touch {}; cat", ran.display());
    let spellings = [
        format!("\u{feff}[filter \"evil\"]\n\t{clean}\n"),
        format!("[user] [filter \"evil\"] {clean}\n"),
        format!("[Filter.evil]\n{clean}\n"),
        "[include]\npath = ../x\n".to_owned(),
        "[includeIf \"gitdir:/x\"]\npath = y\n".to_owned(),
        "[core]\n  FSMonitor=true\n".to_owned(),
    ];
    for spelling in spellings {
        let mut text = spelling.clone().into_bytes();
        text.extend_from_slice(&kept);
        fs::write(&config, &text).unwrap();
        assert_eq!(repo_at(&repo), None, "{spelling:?}");
        assert!(!ran.exists(), "{spelling:?}");
    }
    fs::write(&config, &kept).unwrap();
    assert!(repo_at(&repo).is_some());
}

#[test]
fn a_push_of_a_branch_with_a_slash_in_its_name_is_read_again() {
    let root = TestFolder::new("git-push");
    let upstream = root.join("upstream");
    committed(&upstream);
    let clone = root.join("clone");
    let (from, to) = (upstream.to_string_lossy(), clone.to_string_lossy());
    git(root.path(), &["clone", "--quiet", &from, &to]);
    git(&clone, &["checkout", "--quiet", "-b", "feature/x"]);
    let (reader, answers) = reading(vec![clone.clone()]);
    let at = |ahead| Event::Repo {
        folder: root.join("clone"),
        repo: Some(Repo {
            head: Head::Branch("feature/x".to_owned()),
            changed: false,
            ahead,
            behind: 0,
        }),
    };
    assert_eq!(answers.recv().unwrap(), at(0));
    git(&clone, &["push", "--quiet", "-u", "origin", "feature/x"]);
    fs::write(clone.join("a.txt"), "a").unwrap();
    git(&clone, &["add", "a.txt"]);
    git(&clone, &["commit", "--quiet", "-m", "a"]);
    assert!(answers.iter().any(|event| event == at(1)));
    git(&clone, &["push", "--quiet"]);
    assert!(answers.iter().any(|event| event == at(0)));
    reader.forget(ClientId(1));
}

fn marked(entries: &[(&str, Mark)]) -> BTreeMap<OsString, Mark> {
    entries
        .iter()
        .map(|(name, mark)| ((*name).into(), *mark))
        .collect()
}

#[test]
fn each_entry_is_marked_by_what_git_sees_in_it() {
    let root = TestFolder::new("git-marks");
    let repo = root.join("repo");
    committed(&repo);
    for folder in ["src/deep", "docs", "target/debug"] {
        fs::create_dir_all(repo.join(folder)).unwrap();
    }
    fs::write(repo.join("src/deep/lib.rs"), "one").unwrap();
    fs::write(repo.join("docs/guide.md"), "one").unwrap();
    fs::write(repo.join(".gitignore"), "target/\n*.log\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "tree"]);
    fs::write(repo.join("kept.txt"), "edited").unwrap();
    fs::write(repo.join("src/deep/lib.rs"), "edited").unwrap();
    fs::write(repo.join("new.txt"), "new").unwrap();
    fs::create_dir_all(repo.join("fresh")).unwrap();
    fs::write(repo.join("fresh/a.txt"), "a").unwrap();
    fs::write(repo.join("run.log"), "log").unwrap();
    fs::write(repo.join("target/debug/app"), "bin").unwrap();

    let top = marks_at(&repo).unwrap();
    assert_eq!(top.repo.head, Head::Branch("main".to_owned()));
    assert!(top.repo.changed);
    assert_eq!(
        top.entries,
        marked(&[
            ("fresh", Mark::New),
            ("kept.txt", Mark::Changed),
            ("new.txt", Mark::New),
            ("run.log", Mark::Ignored),
            ("src", Mark::Changed),
            ("target", Mark::Ignored),
        ])
    );
    let src = marks_at(&repo.join("src")).unwrap();
    assert_eq!(src.entries, marked(&[("deep", Mark::Changed)]));
    let docs = marks_at(&repo.join("docs")).unwrap();
    assert!(docs.entries.is_empty());
    assert!(!docs.repo.changed);
    assert_eq!(marks_at(root.path()), None);
}

struct Following {
    reader: Reader,
    events: Arc<dyn EventSink>,
    answers: mpsc::Receiver<Event>,
}

impl Following {
    fn marks_of(repo: &Path) -> Self {
        let (sender, answers) = mpsc::channel::<Event>();
        let events: Arc<dyn EventSink> = Arc::new(sender);
        let reader = Reader::default();
        reader.mark(ClientId(1), &events, Some(repo.to_path_buf()));
        let following = Self {
            reader,
            events,
            answers,
        };
        assert_eq!(following.next(), Some(marked(&[])));
        following
    }

    fn next(&self) -> Option<BTreeMap<OsString, Mark>> {
        entries_of(self.answers.recv().unwrap())
    }

    fn until(&self, entries: &[(&str, Mark)]) {
        let wanted = Some(marked(entries));
        assert!(self.answers.iter().any(|event| entries_of(event) == wanted));
    }

    fn stop(self) {
        self.reader.mark(ClientId(1), &self.events, None);
        drop(self.events);
        assert_eq!(self.answers.iter().count(), 0);
    }
}

fn entries_of(event: Event) -> Option<BTreeMap<OsString, Mark>> {
    match event {
        Event::Marks { marks, .. } => marks.map(|marks| marks.entries),
        Event::Repo { .. } => None,
    }
}

#[test]
fn marks_follow_an_edit_in_the_folder_and_a_commit() {
    let root = TestFolder::new("git-marks-follow");
    let repo = root.join("repo");
    committed(&repo);
    let following = Following::marks_of(&repo);
    fs::write(repo.join("kept.txt"), "edited").unwrap();
    following.until(&[("kept.txt", Mark::Changed)]);
    git(&repo, &["commit", "--quiet", "-am", "edit"]);
    following.until(&[]);
    following.stop();
}

#[test]
fn a_subfolder_is_marked_when_a_file_inside_it_is_edited() {
    let root = TestFolder::new("git-marks-subfolder");
    let repo = root.join("repo");
    committed(&repo);
    fs::create_dir(repo.join("src")).unwrap();
    fs::write(repo.join("src").join("main.rs"), "one").unwrap();
    git(&repo, &["add", "src"]);
    git(&repo, &["commit", "--quiet", "-m", "src"]);
    let following = Following::marks_of(&repo);
    fs::write(repo.join("src").join("main.rs"), "edited").unwrap();
    following.until(&[("src", Mark::Changed)]);
    following.stop();
}
