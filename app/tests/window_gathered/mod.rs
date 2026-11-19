use gpui::point;
use nova_files_gathered_api::Gathering;
use nova_files_gathered_ui::{GatheredPage, GatheredPart};
use nova_files_shell::PageRef;

use super::*;
use crate::window_sidebar::right_click_at;

fn gathered_shown(view: &MainWindow) -> Option<Gathering> {
    view.listed_page()
        .and_then(PageRef::get::<GatheredPage>)
        .map(|page| page.place)
}

fn gathered_at(place: Gathering) -> Location {
    Location::Page(PageRef::new(GatheredPage::new(place)))
}

fn gathered<'a>(view: &MainWindow, cx: &'a App) -> &'a GatheredPart {
    view.part::<GatheredPart>().unwrap().read(cx)
}

async fn wait_for_place(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
    place: Gathering,
    names: &'static [&'static str],
) {
    view.condition::<WindowEvent>(cx, move |view, _| {
        gathered_shown(view) == Some(place) && view.entry_names() == names
    })
    .await;
}

#[gpui::test]
async fn projects_lists_the_folders_it_gathers_and_follows_them(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-projects");
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "place-projects");

    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), None, "no folder to make files in");
        assert_eq!(view.location(), Some(&gathered_at(Gathering::Projects)));
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("folder-header").is_some());
    assert!(cx.debug_bounds("place-projects-here").is_some());

    fs::create_dir(tree.path().join("Projects/bench")).unwrap();
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
    view.condition::<WindowEvent>(cx, |view, cx| {
        gathered(view, cx).library_of_row(0, cx).as_deref() == Some("Steam")
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
        view.new_tab(&gathered_at(Gathering::Projects), cx);
    });
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.update_in(cx, |view, _, cx| view.switch_tab(0, cx));
    wait_for_folder(&view, cx, tree.path()).await;
    view.update_in(cx, |view, _, cx| view.switch_tab(1, cx));

    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.read_with(cx, |view, cx| {
        assert_eq!(view.tab_names(cx)[1], "Projects");
    });
}

#[gpui::test]
async fn a_search_from_projects_comes_back_to_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(|cx| {
        nova_files_window::bind_window_keys(cx);
        nova_files_window::bind_text_keys(cx);
    });
    let tree = TempFolder::tree("gathered-search");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;

    cx.simulate_keystrokes("/");
    cx.run_until_parked();
    cx.simulate_input("nova");
    view.condition::<WindowEvent>(cx, |view, _| {
        view.searching() && view.entry_names().is_empty()
    })
    .await;
    view.read_with(cx, |view, _| {
        assert_eq!(gathered_shown(view), None, "a search's files show");
        assert_eq!(view.folder(), None, "no folder to paste into");
        assert_eq!(view.location(), Some(&gathered_at(Gathering::Projects)));
    });

    cx.simulate_keystrokes("escape");

    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
}

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
    view.read_with(cx, |view, cx| {
        let part = gathered(view, cx);
        let libraries: Vec<_> = (0..3).map(|row| part.library_of_row(row, cx)).collect();
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
async fn a_place_offers_adding_a_folder_not_making_one(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-no-new");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;

    cx.dispatch_action(nova_files_rename_ui::NewFolder);
    cx.run_until_parked();

    view.read_with(cx, |view, cx| {
        assert_eq!(view.jobs_pending(), 0);
        assert!(!view.renaming(cx));
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
        assert_eq!(labels, ["Add folder…", "Select all"]);
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

    view.update(cx, |view, cx| {
        view.open(tree.path());
        let part = view.part::<GatheredPart>().unwrap();
        part.update(cx, |part, cx| part.changed(Gathering::Projects, cx));
    });

    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn a_fresh_read_keeps_the_folder_selected_not_its_namesake(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-namesakes");
    let (mine, work) = (tree.path().join("Mine"), tree.path().join("Work"));
    fs::create_dir_all(mine.join("app")).unwrap();
    fs::create_dir_all(work.join("app")).unwrap();
    write_gathered(
        tree.path(),
        &format!(
            "[[projects]]\npath = \"{}\"\n\n[[projects]]\npath = \"{}\"\n",
            mine.display(),
            work.display()
        ),
    );
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["app", "app"]).await;
    click(cx, "name-0");
    let picked = view.read_with(cx, |view, _| view.selected_paths());
    assert_eq!(picked.len(), 1);

    fs::create_dir(mine.join("beta")).unwrap();

    wait_for_place(&view, cx, Gathering::Projects, &["app", "app", "beta"]).await;
    view.read_with(cx, |view, _| assert_eq!(view.selected_paths(), picked));
}

#[gpui::test]
async fn the_chip_lists_its_folders_and_undo_puts_one_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(nova_files_window::bind_window_keys);
    let tree = TempFolder::tree("gathered-chip");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    view.condition::<WindowEvent>(cx, |view, cx| {
        gathered(view, cx).count(Gathering::Projects) == "1 folder"
    })
    .await;

    click(cx, "gathered-folders");
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        let labels: Vec<&str> = menu.items.iter().map(|item| item.label.as_ref()).collect();
        assert_eq!(labels, ["~/Projects", "Add folder…"]);
        assert_eq!(menu.items[0].detail, Some("Projects inside".into()));
    });
    click(cx, "menu-0");

    wait_for_place(&view, cx, Gathering::Projects, &[]).await;
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.note().map(|note| note.label.to_string()).as_deref(),
            Some("Removed ~/Projects from Projects")
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
}

#[gpui::test]
async fn a_folder_dropped_on_games_is_added_to_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    cx.update(nova_files_window::bind_window_keys);
    let tree = TempFolder::tree("gathered-drop");
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    let (start, end) = (
        cx.debug_bounds("name-0").unwrap().center(),
        cx.debug_bounds("place-games").unwrap().center(),
    );
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();

    view.condition::<WindowEvent>(cx, |view, _| {
        view.note()
            .is_some_and(|note| note.label == "Added Projects to Games")
    })
    .await;
    click(cx, "place-games");
    wait_for_place(&view, cx, Gathering::Games, &["nova"]).await;

    cx.simulate_keystrokes("ctrl-z");
    wait_for_place(&view, cx, Gathering::Games, &[]).await;
}

#[gpui::test]
async fn a_file_dropped_on_games_is_not_added(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-drop-file");
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.entry_names())[1],
        "notes.txt"
    );
    let (start, end) = (
        cx.debug_bounds("name-1").unwrap().center(),
        cx.debug_bounds("place-games").unwrap().center(),
    );
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();

    click(cx, "place-games");
    wait_for_place(&view, cx, Gathering::Games, &[]).await;
    view.read_with(cx, |view, cx| {
        assert_eq!(gathered(view, cx).count(Gathering::Games), "0 folders");
        assert!(view.note().is_none());
    });
}

#[gpui::test]
async fn add_folder_opens_a_folder_picker(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("gathered-pick");
    let (view, cx) = open_on(cx, tree.path()).await;
    click(cx, "place-projects");
    wait_for_place(&view, cx, Gathering::Projects, &["nova"]).await;
    let windows = cx.windows().len();

    click(cx, "gathered-folders");
    click(cx, "menu-1");
    cx.run_until_parked();

    assert_eq!(cx.windows().len(), windows + 1);
}
