#[path = "../../../legacy/core/tests/common/own_folders.rs"]
mod own_folders;

use own_folders::run_again_in;

use super::*;

const ROOT_VAR: &str = "NOVA_FILES_OPENS_WITH_TEST_ROOT";
const TEST_NAME: &str = "window_opens_with::a_files_default_app_changes_from_its_properties";
const TERMINAL_TEST: &str =
    "window_opens_with::a_default_that_runs_in_a_terminal_leaves_the_others_to_choose";
const MIME: &str = "text/x-nova-test";

#[gpui::test]
async fn a_files_default_app_changes_from_its_properties(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    if let Some(root) = std::env::var_os(ROOT_VAR) {
        through_the_window(Path::new(&root), cx).await;
        return;
    }
    run_with(&entry("One"), "plain", TEST_NAME);
}

#[gpui::test]
async fn a_default_that_runs_in_a_terminal_leaves_the_others_to_choose(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    if let Some(root) = std::env::var_os(ROOT_VAR) {
        past_a_terminal_default(Path::new(&root), cx).await;
        return;
    }
    let terminal = format!("{}Terminal=true\n", entry("One"));
    run_with(&terminal, "terminal", TERMINAL_TEST);
}

fn run_with(one_entry: &str, tag: &str, name: &str) {
    let root = std::env::temp_dir().join(format!(
        "nova-files-opens-with-{tag}-{}",
        std::process::id()
    ));
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
        ("data/applications/one.desktop", one_entry.to_owned()),
        ("data/applications/two.desktop", entry("Two")),
        ("files/a.novatest", String::new()),
    ];
    for (file, text) in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    run_again_in(&root, ROOT_VAR, name);
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
    with_parts(cx);
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

    let kept = fs::read_to_string(root.join("config/mimeapps.list")).unwrap();
    assert!(kept.contains(&format!("{MIME}=two.desktop")), "{kept}");
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    assert_eq!(default_app(&view, cx).await, "Two");
}

async fn past_a_terminal_default(root: &Path, cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let file = root.join("files/a.novatest");
    let core = nova_files_core::spawn_with_idle_exit(Duration::from_secs(5)).unwrap();
    let place = Location::Properties(vec![file]);
    let home = root.to_owned();
    with_parts(cx);
    let (view, cx) = cx.add_window_view(|_, cx| MainWindow::at(&core.connector, &place, home, cx));
    view.condition::<WindowEvent>(cx, |view, _| !view.apps_to_open_with().is_empty())
        .await;
    assert!(view.read_with(cx, |view, _| view.default_app().is_none()));

    cx.run_until_parked();
    let card = cx.debug_bounds("opens-with").unwrap();
    cx.simulate_mouse_down(card.center(), gpui::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let item = view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert_eq!(menu.items.len(), 1);
        let item = &menu.items[0];
        assert_eq!(item.label.to_string(), "Two");
        item.look
    });
    assert_ne!(item, Look::On);
    let bounds = cx.debug_bounds("menu-0").unwrap();
    cx.simulate_click(bounds.center(), Modifiers::none());
    assert_eq!(default_app(&view, cx).await, "Two");
}

async fn default_app(view: &Entity<MainWindow>, cx: &VisualTestContext) -> String {
    view.condition::<WindowEvent>(cx, |view, _| view.default_app().is_some())
        .await;
    view.read_with(cx, |view, _| view.default_app().unwrap().name.clone())
}
