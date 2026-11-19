use gpui::Context;
use nova_files_api::MessageType;
use nova_files_shell::{FolderRules, Host, Navigator, Part, Parts, Slots, Views};
use nova_files_ui::CoreStatus;

use super::*;

mod drawn;
mod hooks;
mod kept;
mod listing;
mod naming;
mod picking;
mod preview;
mod rows;
mod shelf;
mod turns;
mod undoing;

#[derive(Debug, Clone)]
struct Claim(PathBuf);

#[derive(Debug)]
struct Unknown;

struct Claimer {
    host: Host,
}

impl Part for Claimer {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(|claimer: &mut Self, Claim(under), _| {
            claimer.host.rules.claim(
                "claimer",
                under,
                FolderRules {
                    read_only: true,
                    views: Views::DetailsOnly,
                    replaces_menu: false,
                    trashed: false,
                },
            );
        });
    }
}

thread_local! {
    static WITNESSED: Cell<usize> = const { Cell::new(0) };
}

struct Witness;

impl Part for Witness {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(|_: &mut Self, Claim(_), _| WITNESSED.set(WITNESSED.get() + 1));
    }
}

fn with_parts(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    WITNESSED.set(0);
    let mut parts = Parts::default();
    parts.add::<Claimer>();
    parts.add::<nova_files_folders_ui::FoldersPart>();
    parts.add::<Witness>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, gpui::Modifiers::none());
    cx.run_until_parked();
}

fn rules(view: &Entity<MainWindow>, cx: &VisualTestContext) -> FolderRules {
    view.read_with(cx, |view, _| view.folder_rules())
}

#[gpui::test]
fn a_part_takes_its_own_events_and_its_rules_reach_the_window(cx: &mut TestAppContext) {
    let (view, cx, core) = with_parts(cx);
    assert_eq!(rules(&view, cx), FolderRules::default());

    core.events
        .as_ref()
        .unwrap()
        .deliver(Claim(PathBuf::from(SHOWN)))
        .unwrap();
    cx.run_until_parked();

    assert!(rules(&view, cx).read_only);
    assert_eq!(
        WITNESSED.get(),
        1,
        "every part that takes an event hears it"
    );
    assert_eq!(rules(&view, cx).views, Views::DetailsOnly);
    assert_eq!(
        view.read_with(cx, MainWindow::folder_view),
        nova_files_folders_api::FolderView::default()
    );
}

#[gpui::test]
fn an_event_no_part_takes_leaves_the_window_connected(cx: &mut TestAppContext) {
    let (view, cx, core) = with_parts(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.status()),
        CoreStatus::Connecting
    );

    core.events.as_ref().unwrap().deliver(Unknown).unwrap();
    core.send(Event::Pong);
    cx.run_until_parked();

    assert_eq!(
        view.read_with(cx, |view, _| view.status()),
        CoreStatus::Connected
    );
}

#[test]
fn parts_list_the_event_types_they_take() {
    let mut parts = Parts::default();
    parts.add::<Claimer>();
    parts.add::<Witness>();
    assert_eq!(parts.takes(), [MessageType::of::<Claim>()]);
}

#[gpui::test]
fn a_part_selects_a_name_once_a_later_listing_holds_it(cx: &mut TestAppContext) {
    let (view, cx, mut core) = with_parts(cx);
    let (entries, nav) = view.read_with(cx, |view, _| {
        (view.host().entries.clone(), view.host().nav.clone())
    });
    let new = ENTRIES.to_string();
    entries.update(cx, |entries, cx| {
        entries.select_when_listed(SHOWN.into(), vec![new.clone().into()], cx);
    });
    cx.run_until_parked();
    assert_ne!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [Path::new(SHOWN).join(&new)]
    );

    nav.update(cx, Navigator::reload);
    cx.run_until_parked();
    answer_listing(&mut core, ENTRIES + 1);
    cx.run_until_parked();

    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [Path::new(SHOWN).join(&new)]
    );
}
