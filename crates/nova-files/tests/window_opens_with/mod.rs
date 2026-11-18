//! The Properties page's Opens with on a real core: a lone file's card
//! names its default app, and choosing another in its menu makes that the
//! default, which a page opened again shows and the user's
//! `mimeapps.list` keeps.
//!
//! The core reads the MIME database and the app lists from the
//! environment, so the test runs itself again with every XDG folder
//! pointed into a temp folder of its own.

#[path = "../../../nova-files-core/tests/common/own_folders.rs"]
mod own_folders;

use own_folders::run_again_in;

use super::*;

/// Set in the run that talks to the core: the folder it was given.
const ROOT_VAR: &str = "NOVA_FILES_OPENS_WITH_TEST_ROOT";
const TEST_NAME: &str = "window_opens_with::a_files_default_app_changes_from_its_properties";
const MIME: &str = "text/x-nova-test";

#[gpui::test]
async fn a_files_default_app_changes_from_its_properties(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    if let Some(root) = std::env::var_os(ROOT_VAR) {
        through_the_window(Path::new(&root), cx).await;
        return;
    }
    let root = std::env::temp_dir().join(format!("nova-files-opens-with-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let files = [
        ("data/mime/globs2", format!("50:{MIME}:*.novatest\n")),
        (
            "config/mimeapps.list",
            format!("[Default Applications]\n{MIME}=one.desktop\n"),
        ),
        (
            "data/applications/mimeinfo.cache",
            format!("[MIME Cache]\n{MIME}=one.desktop;two.desktop;\n"),
        ),
        ("data/applications/one.desktop", entry("One")),
        ("data/applications/two.desktop", entry("Two")),
        ("files/a.novatest", String::new()),
    ];
    for (file, text) in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    run_again_in(&root, ROOT_VAR, TEST_NAME);
}

fn entry(name: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=true %f\n")
}

async fn through_the_window(root: &Path, cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let file = root.join("files/a.novatest");
    let core = nova_files_core::spawn_with_idle_exit(Duration::from_secs(5)).unwrap();
    let place = Location::Properties(vec![file.clone()]);
    let home = root.to_owned();
    let (view, cx) = cx.add_window_view(|_, cx| MainWindow::at(&core.connector, &place, home, cx));
    assert_eq!(default_app(&view, cx).await, "One");

    cx.run_until_parked();
    let card = cx.debug_bounds("opens-with").unwrap();
    cx.simulate_mouse_down(card.center(), gpui::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let (labels, two) = view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        let labels: Vec<String> = menu
            .items
            .iter()
            .map(|item| item.label.to_string())
            .collect();
        let two = labels.iter().position(|label| label == "Two").unwrap();
        (labels, two)
    });
    assert_eq!(labels.len(), 2);
    let item: &'static str = ["menu-0", "menu-1"][two];
    let bounds = cx.debug_bounds(item).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::none());
    view.condition::<WindowEvent>(cx, |view, _| {
        view.default_app().is_some_and(|app| app.name == "Two")
    })
    .await;

    // A page opened again asks the system, which keeps the new default.
    let kept = fs::read_to_string(root.join("config/mimeapps.list")).unwrap();
    assert!(kept.contains(&format!("{MIME}=two.desktop")), "{kept}");
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    assert_eq!(default_app(&view, cx).await, "Two");
}

/// Waits until the page on screen knows its file's default app.
async fn default_app(view: &Entity<MainWindow>, cx: &VisualTestContext) -> String {
    view.condition::<WindowEvent>(cx, |view, _| view.default_app().is_some())
        .await;
    view.read_with(cx, |view, _| view.default_app().unwrap().name.clone())
}
