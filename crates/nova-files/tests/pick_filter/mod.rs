//! The type filter in a picker window: the app's types dim the files they
//! leave out, the menu switches type or shows all files, MIME rules use a
//! real database, and a folder picker dims every file.

use std::sync::Arc;

use nova_files_api::{FileFilter, FilterRule};

use super::*;

fn filter(name: &str, rules: Vec<FilterRule>) -> FileFilter {
    FileFilter {
        name: name.to_owned(),
        rules,
    }
}

fn glob(pattern: &str) -> FilterRule {
    FilterRule::Glob(pattern.to_owned())
}

/// Which rows on screen the picker dims, of the first `rows`.
fn dimmed(view: &Entity<MainWindow>, cx: &VisualTestContext, rows: usize) -> Vec<usize> {
    view.read_with(cx, |view, _| {
        (0..rows).filter(|&row| view.pick_dims(row)).collect()
    })
}

fn shown(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.run_until_parked();
    cx.debug_bounds(selector).is_some()
}

fn picked_with(path: PathBuf, filter: Option<usize>) -> PickAnswer {
    PickAnswer::Picked {
        paths: vec![path],
        filter,
    }
}

/// Text first and PDFs second, as an app lists them.
fn text_and_pdfs(request: &mut PickRequest) {
    request.filters = vec![
        filter("Text", vec![glob("*.TXT")]),
        filter("PDF documents", vec![glob("*.pdf")]),
    ];
}

// With `report.pdf` added, rows on screen: 0 `Projects`, 1 `notes (2).txt`,
// 2 `notes.txt`, 3 `report.pdf`.

#[gpui::test]
async fn the_first_type_dims_the_files_it_leaves_out(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(tree.path().join("report.pdf"), "").unwrap();
    let (view, cx, answers) = picker_with(cx, &tree, tree.path(), FILES, text_and_pdfs).await;
    assert_eq!(view.read_with(cx, |view, _| view.pick_filter()), Some(0));
    assert_eq!(dimmed(&view, cx, 4), [3]);

    // A dimmed file can still be picked.
    click(cx, "entry-3");
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv(),
        Ok(picked_with(tree.path().join("report.pdf"), Some(0)))
    );
}

#[gpui::test]
async fn the_menu_chooses_another_type_or_all_files(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(tree.path().join("report.pdf"), "").unwrap();
    let (view, cx, answers) = picker_with(cx, &tree, tree.path(), FILES, text_and_pdfs).await;
    assert!(!shown(cx, "pick-filter-1"));

    click(cx, "pick-filter");
    click(cx, "pick-filter-1");
    assert!(!shown(cx, "pick-filter-1"));
    assert_eq!(dimmed(&view, cx, 4), [1, 2]);

    click(cx, "pick-filter");
    click(cx, "pick-filter-all");
    assert_eq!(view.read_with(cx, |view, _| view.pick_filter()), None);
    assert_eq!(dimmed(&view, cx, 4), [0_usize; 0]);
    double_click(cx, "entry-2");
    assert_eq!(
        answers.try_recv(),
        Ok(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn a_click_elsewhere_closes_the_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, _answers) = picker_with(cx, &tree, tree.path(), FILES, text_and_pdfs).await;
    click(cx, "pick-filter");
    assert!(shown(cx, "pick-filter-all"));

    click(cx, "entry-1");
    assert!(!shown(cx, "pick-filter-all"));
    assert_eq!(view.read_with(cx, |view, _| view.pick_filter()), Some(0));
}

#[gpui::test]
async fn a_mime_type_admits_its_kinds_and_files_it_cant_judge(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(tree.path().join("photo.png"), "").unwrap();
    fs::write(tree.path().join("README"), "").unwrap();
    let data = tree.path().join("Projects").join("share");
    fs::create_dir_all(data.join("mime")).unwrap();
    fs::write(
        data.join("mime").join("globs2"),
        "50:text/x-notes:*.txt\n50:image/png:*.png\n",
    )
    .unwrap();
    fs::write(
        data.join("mime").join("subclasses"),
        "text/x-notes text/plain\n",
    )
    .unwrap();
    let types = Arc::new(nova_files_core::MimeTypes::load(&[data]));
    let (view, cx, _answers) = picker_with(cx, &tree, tree.path(), FILES, |request| {
        request.filters = vec![filter(
            "Plain text",
            vec![FilterRule::MimeType("text/plain".to_owned())],
        )];
        request.types = Some(types);
    })
    .await;

    // Rows: 0 `Projects`, 1 `notes (2).txt`, 2 `notes.txt`, 3 `photo.png`,
    // 4 `README`.
    assert_eq!(dimmed(&view, cx, 5), [3]);
}

#[gpui::test]
async fn a_folder_picker_dims_every_file_and_offers_no_types(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, _answers) = picker_with(cx, &tree, tree.path(), FOLDERS, text_and_pdfs).await;
    assert_eq!(dimmed(&view, cx, 3), [1, 2]);
    assert!(!shown(cx, "pick-filter"));
}

#[gpui::test]
async fn no_types_means_no_menu_and_nothing_dimmed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, _answers) = picker(cx, &tree, tree.path(), FILES).await;
    assert_eq!(dimmed(&view, cx, 3), [0_usize; 0]);
    assert!(!shown(cx, "pick-filter"));
}

#[gpui::test]
async fn an_app_that_offers_every_file_gets_no_second_all_files(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, _answers) = picker_with(cx, &tree, tree.path(), FILES, |request| {
        request.filters = vec![
            filter("All Files", vec![glob("*")]),
            filter("Text", vec![glob("*.txt")]),
        ];
    })
    .await;
    click(cx, "pick-filter");
    assert!(shown(cx, "pick-filter-1"));
    assert!(!shown(cx, "pick-filter-all"));
}

#[gpui::test]
async fn a_long_type_name_leaves_room_for_the_buttons(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, _answers) = picker_with(cx, &tree, tree.path(), FILES, |request| {
        request.filters = vec![filter(&"Very long type name ".repeat(40), vec![glob("*")])];
    })
    .await;
    let width = cx.update(|window, _| window.viewport_size().width);
    let name = cx.debug_bounds("pick-name").unwrap();
    let accept = cx.debug_bounds("pick-accept").unwrap();
    assert!(name.size.width > gpui::Pixels::ZERO, "{name:?}");
    assert!(accept.right() <= width, "{accept:?} past {width:?}");
}

#[gpui::test]
async fn many_types_scroll_inside_the_window(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, _answers) = picker_with(cx, &tree, tree.path(), FILES, |request| {
        request.filters = (0..60)
            .map(|at| filter(&format!("Type {at}"), vec![glob("*")]))
            .collect();
    })
    .await;
    click(cx, "pick-filter");
    let height = cx.update(|window, _| window.viewport_size().height);
    let menu = cx.debug_bounds("pick-filter-menu").unwrap();
    assert!(menu.top() >= gpui::Pixels::ZERO, "{menu:?}");
    assert!(menu.bottom() <= height, "{menu:?} past {height:?}");
}
