use std::cell::RefCell;
use std::path::Path;

use gpui::{Context, SharedString};
use nova_files_shell::{Host, NavEvent, PageKind, PageRef, Part, Parts, Slots, TabId};

use super::super::*;

thread_local! {
    static HEARD: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn heard(line: String) {
    HEARD.with_borrow_mut(|heard| heard.push(line));
}

fn take_heard() -> Vec<String> {
    HEARD.with_borrow_mut(std::mem::take)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Desk;

impl PageKind for Desk {
    fn title(&self) -> SharedString {
        "Desk".into()
    }
}

struct Turner;

impl Part for Turner {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&host.nav, |_, _, event: &NavEvent, _| {
            if let NavEvent::TabShown(TabId(tab)) = event {
                heard(format!("tab {tab}"));
            }
        })
        .detach();
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.page_shown::<Desk>(|_, _, TabId(tab), _| heard(format!("shown {tab}")));
        slots.page_left::<Desk>(|_, _, TabId(tab), _| heard(format!("left {tab}")));
        slots.used(|_, folder: &Path, _| heard(format!("used {}", folder.display())));
    }
}

fn turning(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let mut parts = Parts::default();
    parts.add::<Turner>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    take_heard();
    (view, cx, core)
}

#[gpui::test]
fn a_part_hears_its_page_shown_and_left_and_the_folders_used(cx: &mut TestAppContext) {
    let (view, cx, mut core) = turning(cx);

    view.update(cx, |view, cx| view.open_page(PageRef::new(Desk), cx));
    cx.run_until_parked();
    assert_eq!(take_heard(), ["shown 0"]);

    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    assert_eq!(take_heard(), [format!("used {OTHER}"), "left 0".to_owned()]);
}

#[gpui::test]
fn a_part_hears_its_page_left_and_shown_again_across_tabs(cx: &mut TestAppContext) {
    let (view, cx, _core) = turning(cx);
    view.update(cx, |view, cx| view.open_page(PageRef::new(Desk), cx));
    cx.run_until_parked();
    take_heard();

    view.update(cx, |view, cx| {
        view.new_tab(&nova_files_window::Location::start(), cx);
    });
    cx.run_until_parked();
    assert_eq!(take_heard(), ["left 0", "tab 1"]);

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    cx.run_until_parked();
    assert_eq!(take_heard(), ["shown 0", "tab 0"]);
}
