//! Games and Projects on a real core: Places lists both before the trash;
//! a click shows what each gathers, with each project's Folder or each
//! game's Library beside it, and the list follows what is made in its
//! folders. Back leaves it as it leaves a folder; a tab keeps it; a
//! search from it comes back to it. There is no folder there to make
//! files in.

use gpui::point;
use nova_files_api::Gathering;

use super::*;
use crate::window_sidebar::right_click_at;

/// Waits until `place` is on screen with `names`, in order.
async fn wait_for_place(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
    place: Gathering,
    names: &'static [&'static str],
) {
    view.condition::<WindowEvent>(cx, move |view, _| {
        view.gathered_shown() == Some(place) && view.entry_names() == names
    })
    .await;
}

#[gpui::test]
async fn projects_lists_the_folders_it_gathers_and_follows_them(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-projects");
    // A fresh install gathers `~/Projects`, the tree's.
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "place-projects");

    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), None, "no folder to make files in");
        assert_eq!(
            view.location(),
            Some(&Location::Gathered(Gathering::Projects))
        );
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("folder-header").is_some());
    assert!(cx.debug_bounds("place-projects-here").is_some());

    fs::create_dir(tree.path().join("Projects/bench")).unwrap();
    // A file is no project.
    fs::write(tree.path().join("Projects/README.md"), "").unwrap();
    wait_for_place(&view, cx, Gathering::Projects, &["bench", "nova"]).await;

    view.update_in(cx, |view, _, cx| view.go_back(cx));
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn games_names_each_library(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-games");
    let library = tree.path().join("SteamLibrary");
    fs::create_dir_all(library.join("Celeste")).unwrap();
    let config = tree.path().join(".config/nova-files");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("gathered.toml"),
        format!(
            "[[games]]\npath = \"{}\"\nname = \"Steam\"\n",
            library.display()
        ),
    )
    .unwrap();
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "place-games");

    wait_for_place(&view, cx, Gathering::Games, &["Celeste"]).await;
    view.condition::<WindowEvent>(cx, |view, _| {
        view.library_of_row(0).as_deref() == Some("Steam")
    })
    .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("head-library").is_some());
    assert!(cx.debug_bounds("place-games-here").is_some());
}

#[gpui::test]
async fn a_tab_on_projects_keeps_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-tab");
    let (view, cx) = open_on(cx, tree.path()).await;

    view.update_in(cx, |view, _, cx| {
        view.new_tab(&Location::Gathered(Gathering::Projects), cx);
    });
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.update_in(cx, |view, _, cx| view.switch_tab(0, cx));
    wait_for_folder(&view, cx, tree.path()).await;
    view.update_in(cx, |view, _, cx| view.switch_tab(1, cx));

    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_names()[1], "Projects");
    });
}

#[gpui::test]
async fn a_search_from_projects_comes_back_to_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(|cx| {
        nova_files_ui::bind_window_keys(cx);
        nova_files_ui::bind_text_keys(cx);
    });
    let tree = TempFolder::tree("gathered-search");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;

    cx.simulate_keystrokes("/");
    cx.run_until_parked();
    cx.simulate_input("nova");
    // This core has no index: the find can't run and the list empties.
    view.condition::<WindowEvent>(cx, |view, _| {
        view.searching() && view.entry_names().is_empty()
    })
    .await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.gathered_shown(), None, "a search's files show");
        assert_eq!(
            view.location(),
            Some(&Location::Gathered(Gathering::Projects))
        );
    });

    cx.simulate_keystrokes("escape");

    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
}

/// Writes `text` as the gathered folders file of the home at `tree`.
fn write_gathered(tree: &Path, text: &str) {
    let config = tree.join(".config/nova-files");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("gathered.toml"), text).unwrap();
}

#[gpui::test]
async fn each_game_shows_the_library_that_brought_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-libraries");
    let steam = tree.path().join("SteamLibrary");
    let other = tree.path().join("Other");
    let solo = tree.path().join("Solo");
    fs::create_dir_all(steam.join("Celeste")).unwrap();
    fs::create_dir_all(other.join("Hades")).unwrap();
    fs::create_dir_all(&solo).unwrap();
    write_gathered(
        tree.path(),
        &format!(
            "[[games]]\npath = \"{}\"\nname = \"Steam\"\n\n\
             [[games]]\npath = \"{}\"\nholds = \"one\"\nname = \"Mine\"\n\n\
             [[games]]\npath = \"{}\"\n",
            steam.display(),
            solo.display(),
            other.display()
        ),
    );
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "place-games");

    wait_for_place(&view, cx, Gathering::Games, &["Celeste", "Hades", "Solo"]).await;
    view.read_with(cx, |view, _| {
        let libraries: Vec<_> = (0..3).map(|row| view.library_of_row(row)).collect();
        assert_eq!(
            libraries,
            [
                Some("Steam".to_owned()),
                Some("~/Other".to_owned()),
                Some("Mine".to_owned())
            ]
        );
    });
}

#[gpui::test]
async fn a_place_offers_no_new_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-no-new");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;

    view.update(cx, |view, _| view.new_folder());

    view.read_with(cx, |view, _| {
        assert_eq!(view.jobs_pending(), 0);
        assert!(!view.renaming());
    });
    cx.run_until_parked();
    let below = cx.debug_bounds("entry-0").unwrap();
    right_click_at(
        cx,
        point(below.center().x, below.bottom() + below.size.height * 2.0),
    );
    view.read_with(cx, |view, _| {
        let labels: Vec<_> = view
            .menu()
            .unwrap()
            .items
            .iter()
            .map(|item| item.label.to_string())
            .collect();
        assert_eq!(labels, ["Select all"]);
    });
}

#[gpui::test]
async fn a_folder_added_to_the_file_lists_the_place_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-file-change");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;

    let work = tree.path().join("Work");
    fs::create_dir_all(work.join("zeta")).unwrap();
    write_gathered(
        tree.path(),
        &format!(
            "[[projects]]\npath = \"{}\"\n\n[[projects]]\npath = \"{}\"\n",
            tree.path().join("Projects").display(),
            work.display()
        ),
    );

    wait_for_place(&view, cx, Gathering::Projects, &["nova", "zeta"]).await;
}

#[gpui::test]
async fn a_place_that_changes_while_going_elsewhere_keeps_the_way(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-change-away");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;

    view.update(cx, |view, _| {
        view.open(tree.path());
        view.gathered_changed(Gathering::Projects);
    });

    wait_for_folder(&view, cx, tree.path()).await;
}
