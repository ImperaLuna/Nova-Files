use std::fs;
use std::path::Path;
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

#[test]
fn each_asked_folder_is_answered_in_turn() {
    let root = TestFolder::new("git-reader");
    let repo = root.join("repo");
    committed(&repo);
    let plain = root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let (sender, answers) = mpsc::channel::<Event>();
    let events: Arc<dyn EventSink> = Arc::new(sender);
    let reader = Reader::default();
    reader.read(ClientId(1), &events, vec![repo.clone(), plain.clone()]);
    drop(events);
    let got: Vec<Event> = answers.iter().collect();
    assert_eq!(
        got,
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
}
