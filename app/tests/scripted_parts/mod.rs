use gpui::Context;
use nova_files_api::MessageType;
use nova_files_shell::{FolderRules, Host, Part, Parts, Slots, Views};
use nova_files_ui::CoreStatus;

use super::*;

#[derive(Debug)]
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
                },
            );
        });
    }
}

fn with_parts(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let mut parts = Parts::default();
    parts.add::<Claimer>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
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
    assert_eq!(rules(&view, cx).views, Views::DetailsOnly);
    assert_eq!(
        view.read_with(cx, |view, _| view.folder_view()),
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
    assert_eq!(parts.takes(), [MessageType::of::<Claim>()]);
}
