use super::*;

fn gallery(
    cx: &mut TestAppContext,
    entries: u32,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, entries);
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-3");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    (view, cx, core)
}

fn on_stage(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, cx| previewed(view, cx).map(|shown| shown.name))
}

fn lifted(view: &Entity<MainWindow>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, previewing)
}

#[gpui::test]
fn the_stage_shows_the_first_entry_selected(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 10);
    assert_eq!(on_stage(&view, cx).as_deref(), Some("0"));
    assert_eq!(view.read_with(cx, |view, _| view.selected_names()), ["0"]);
    assert!(!lifted(&view, cx));
    let stage = cx.debug_bounds("stage").unwrap();
    assert!(cx.debug_bounds("strip").is_some());
    assert_eq!(cx.debug_bounds("head-name"), None);
    let (first, second) = (cx.debug_bounds("entry-0"), cx.debug_bounds("entry-1"));
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_eq!(first.top(), second.top());
    assert!(first.top() >= stage.bottom(), "{first:?} {stage:?}");
    let island = cx.debug_bounds("island").unwrap();
    assert!(first.bottom() <= island.top(), "{first:?} {island:?}");
}

#[gpui::test]
fn the_arrows_step_the_stage(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 10);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(on_stage(&view, cx).as_deref(), Some("1"));
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_eq!(on_stage(&view, cx).as_deref(), Some("2"));
    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    assert_eq!(on_stage(&view, cx).as_deref(), Some("1"));
}

#[gpui::test]
fn space_lifts_the_stage_and_puts_it_back(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 10);
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(lifted(&view, cx));
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(!lifted(&view, cx));
    assert_eq!(on_stage(&view, cx).as_deref(), Some("0"));
    cx.simulate_keystrokes("space escape");
    cx.run_until_parked();
    assert!(!lifted(&view, cx));
    assert_eq!(on_stage(&view, cx).as_deref(), Some("0"));
}

#[gpui::test]
fn leaving_gallery_takes_the_stage_down(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 10);
    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    assert_eq!(on_stage(&view, cx), None);
    assert!(cx.debug_bounds("head-name").is_some());
}

#[gpui::test]
fn the_strip_draws_only_what_fits(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 400);
    assert!(cx.debug_bounds("entry-0").is_some());
    assert_eq!(cx.debug_bounds("entry-399"), None);
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert_eq!(on_stage(&view, cx).as_deref(), Some("399"));
    assert!(cx.debug_bounds("entry-399").is_some());
    assert_eq!(cx.debug_bounds("entry-0"), None);
}

#[gpui::test]
fn the_stage_asks_for_less_room_than_the_lifted_look(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 10);
    let fit = |cx: &mut VisualTestContext| view.read_with(cx, preview_fit);
    let docked = fit(cx).unwrap();
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    let lifted = fit(cx).unwrap();
    assert!(docked.width < lifted.width && docked.height < lifted.height);
}

#[gpui::test]
fn a_rename_shows_under_the_stage(cx: &mut TestAppContext) {
    let (view, cx, _) = gallery(cx, 10);
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.renaming()));
    let field = cx.debug_bounds("rename").unwrap();
    assert!(field.size.width > px(100.0), "{field:?}");
    assert_eq!(cx.debug_bounds("stage-name"), None);
}
