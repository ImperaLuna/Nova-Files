//! Deleting for good through the real core: whole trees go, links go
//! without what they point to, and bad paths delete nothing.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use nova_files_api::{Command, Event, Failure, Operation, Outcome};

use common::Bench;

const fn delete(paths: Vec<PathBuf>) -> Operation {
    Operation::Delete { paths }
}

#[test]
fn deletes_trees_and_links_but_not_what_links_point_to() {
    let bench = Bench::new("delete-tree");
    let folder = bench.at("doomed");
    fs::create_dir_all(folder.join("sub/deeper")).unwrap();
    fs::write(folder.join("sub/deeper/a"), "a").unwrap();
    fs::write(bench.at("kept"), "kept").unwrap();
    symlink(bench.at("kept"), folder.join("link")).unwrap();
    symlink(bench.at("kept"), bench.at("loose link")).unwrap();
    let client = bench.client();

    let (_, outcome, changed) = client.run(delete(vec![folder.clone(), bench.at("loose link")]));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, vec![bench.root.clone()]);
    assert!(!folder.exists());
    assert!(bench.at("loose link").symlink_metadata().is_err());
    assert_eq!(fs::read_to_string(bench.at("kept")).unwrap(), "kept");
}

#[test]
fn a_path_that_is_not_absolute_deletes_nothing() {
    let bench = Bench::new("delete-relative");
    fs::write(bench.at("a"), "a").unwrap();
    let client = bench.client();

    let (_, outcome, changed) = client.run(delete(vec![bench.at("a"), "a".into()]));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: "a".into(),
            reason: Failure::InvalidName
        }
    );
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert!(bench.at("a").exists());
}

#[test]
fn a_path_already_gone_fails_before_anything_is_deleted() {
    let bench = Bench::new("delete-gone");
    fs::write(bench.at("a"), "a").unwrap();
    let client = bench.client();

    let (_, outcome, _) = client.run(delete(vec![bench.at("a"), bench.at("gone")]));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: bench.at("gone"),
            reason: Failure::NotFound
        }
    );
    assert!(bench.at("a").exists());
}

#[test]
fn a_window_watching_jobs_hears_which_folders_a_delete_changed() {
    let bench = Bench::new("delete-watched");
    fs::write(bench.at("a"), "a").unwrap();
    let watcher = bench.client();
    watcher.send(Command::WatchJobs);
    let client = bench.client();

    let (job, _, _) = client.run(delete(vec![bench.at("a")]));

    let changed = loop {
        match watcher.next() {
            Event::JobFinished {
                job: ended,
                changed,
                ..
            } if ended == job => break changed,
            Event::JobProgress { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(changed, vec![bench.root.clone()]);
}

#[test]
fn a_file_chosen_through_a_chosen_link_goes_too() {
    for (index, file_first) in [false, true].into_iter().enumerate() {
        let bench = Bench::new(&format!("delete-through-link-{index}"));
        let real = bench.at("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("f"), "f").unwrap();
        let alias = bench.at("alias");
        symlink(&real, &alias).unwrap();
        let client = bench.client();
        let mut paths = vec![alias.clone(), alias.join("f")];
        if file_first {
            paths.reverse();
        }

        let (_, outcome, _) = client.run(delete(paths));

        assert_eq!(outcome, Outcome::Done);
        assert!(alias.symlink_metadata().is_err());
        assert!(!real.join("f").exists(), "file first: {file_first}");
        assert!(real.exists());
    }
}
