use nova_files_properties_api::Command as PropertiesCommand;
use nova_files_properties_ui::PropertiesPage;
use nova_files_shell::PageRef;
use nova_files_window::Location;

use super::*;

#[gpui::test]
fn leaving_a_properties_page_stops_its_scan(cx: &mut TestAppContext) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    core.commands();

    let place = Location::Page(PageRef::new(PropertiesPage::new(vec![
        PathBuf::from(SHOWN).join("0"),
    ])));
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    cx.run_until_parked();
    let id = core
        .sent()
        .into_iter()
        .find_map(|command| match command {
            PropertiesCommand::Measure { id, .. } => Some(id),
            _ => None,
        })
        .unwrap();

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    cx.run_until_parked();
    let commands: Vec<PropertiesCommand> = core.sent();
    assert!(
        commands.contains(&PropertiesCommand::StopMeasure(id)),
        "{commands:?}"
    );
}
