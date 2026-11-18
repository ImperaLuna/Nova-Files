use nova_files_ui::Location;

use super::*;

#[gpui::test]
fn leaving_a_properties_page_stops_its_scan(cx: &mut TestAppContext) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    core.commands();

    let place = Location::Properties(vec![PathBuf::from(SHOWN).join("0")]);
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    cx.run_until_parked();
    let id = core
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::Measure { id, .. } => Some(id),
            _ => None,
        })
        .unwrap();

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    cx.run_until_parked();
    let commands = core.commands();
    assert!(commands.contains(&Command::StopMeasure(id)), "{commands:?}");
}
