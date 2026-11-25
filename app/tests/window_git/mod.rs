use nova_files_git_api::Mark;
use nova_files_git_ui::{BRANCH_CHIP, CHIP_CHANGES, GitPart};

use super::*;

pub fn git(folder: &Path, args: &[&str]) {
    let ran = std::process::Command::new("git")
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

fn git_part<'a>(view: &MainWindow, cx: &'a App) -> &'a GitPart {
    view.part::<GitPart>().unwrap().read(cx)
}

#[gpui::test]
async fn rows_in_a_repository_show_what_changed_and_dim_what_is_ignored(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("git-marks");
    let repo = tree.path().join("Projects/nova");
    git(&repo, &["init", "--quiet"]);
    fs::write(repo.join("kept.txt"), "one").unwrap();
    fs::write(repo.join(".gitignore"), "target/\n.config/\n.local/\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "first"]);
    fs::write(repo.join("kept.txt"), "two").unwrap();
    fs::write(repo.join("fresh.txt"), "new").unwrap();
    fs::create_dir_all(repo.join("target")).unwrap();
    let (view, cx) = open_on(cx, &repo).await;

    view.condition::<WindowEvent>(cx, |view, cx| {
        let part = git_part(view, cx);
        part.mark_of("kept.txt") == Some(Mark::Changed)
            && part.mark_of("fresh.txt") == Some(Mark::New)
            && part.mark_of("target") == Some(Mark::Ignored)
    })
    .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("git-mark-kept.txt").is_some());
    assert!(cx.debug_bounds("git-mark-fresh.txt").is_some());
    assert!(cx.debug_bounds("git-mark-target").is_none());
    assert!(cx.debug_bounds("git-mark-.gitignore").is_none());
    assert!(cx.debug_bounds(BRANCH_CHIP).is_some());
    assert!(cx.debug_bounds(CHIP_CHANGES).is_some());

    git(&repo, &["add", "--all"]);
    git(&repo, &["commit", "--quiet", "-am", "second"]);
    view.condition::<WindowEvent>(cx, |view, cx| {
        git_part(view, cx).mark_of("kept.txt").is_none()
    })
    .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds(BRANCH_CHIP).is_some());
    assert!(cx.debug_bounds(CHIP_CHANGES).is_none());

    view.update(cx, |view, cx| view.new_tab(&Location::start(), cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds(BRANCH_CHIP).is_none());
}

#[gpui::test]
async fn a_folder_outside_any_repository_has_no_branch_chip(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("git-no-chip");
    let (_, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds(BRANCH_CHIP).is_none());
}
