use std::cell::{Cell, RefCell};

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, Pixels, Size, Styled as _,
    Window, div, px, size,
};
use nova_files_api::{Entry, ListingId};
use nova_files_properties_api::{self as properties_api, PropertiesId};
use nova_files_shell::{Click, ClickAnswer, Host, Part, Parts, Scene, Slots};

use super::super::*;

const TAKEN: &str = "1";
const PICKED: &str = "2";
const FOOTER: &str = "framer-footer";
const PEEKED: PropertiesId = PropertiesId(4);

thread_local! {
    static CLICKS: RefCell<Vec<(String, Click)>> = const { RefCell::new(Vec::new()) };
    static SIZES: RefCell<Vec<Size<Pixels>>> = const { RefCell::new(Vec::new()) };
    static PEEKS: RefCell<Vec<PropertiesId>> = const { RefCell::new(Vec::new()) };
    static FOOTED: Cell<bool> = const { Cell::new(false) };
}

struct Framer;

fn clicked(
    _: &mut Framer,
    _: ListingId,
    entry: &Entry,
    click: Click,
    _: &mut Context<Framer>,
) -> Option<ClickAnswer> {
    let name = entry.name.to_string_lossy().into_owned();
    CLICKS.with_borrow_mut(|clicks| clicks.push((name.clone(), click)));
    match name.as_str() {
        TAKEN => Some(ClickAnswer::Done),
        PICKED => Some(ClickAnswer::Select),
        _ => None,
    }
}

fn footer(
    _: &mut Framer,
    _: &Scene,
    _: &mut Window,
    _: &mut Context<Framer>,
) -> Option<AnyElement> {
    FOOTED.get().then(|| {
        div()
            .debug_selector(|| FOOTER.to_owned())
            .size(px(1.0))
            .into_any_element()
    })
}

impl Part for Framer {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.clicked(clicked);
        slots.footer(footer);
        slots.resized(|_, size, _| SIZES.with_borrow_mut(|sizes| sizes.push(size)));
        slots.peek(|_, event: &properties_api::Event, _| {
            if let properties_api::Event::PropertiesUnreadable { id, .. } = event {
                PEEKS.with_borrow_mut(|peeks| peeks.push(*id));
            }
        });
    }
}

fn framed(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    CLICKS.with_borrow_mut(Vec::clear);
    SIZES.with_borrow_mut(Vec::clear);
    PEEKS.with_borrow_mut(Vec::clear);
    FOOTED.set(false);
    let mut parts = Parts::default();
    parts.add::<Framer>();
    cx.update(|cx| {
        cx.set_global(parts);
        nova_files_window::bind_window_keys(cx);
    });
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn click_entry(cx: &mut VisualTestContext, index: u32) {
    let at = cx.debug_bounds(entry_selector(index)).unwrap().center();
    cx.simulate_click(at, gpui::Modifiers::none());
    cx.run_until_parked();
}

const fn entry_selector(index: u32) -> &'static str {
    match index {
        0 => "entry-0",
        1 => "entry-1",
        _ => "entry-2",
    }
}

#[gpui::test]
fn a_part_answers_a_click_on_an_entry(cx: &mut TestAppContext) {
    let (view, cx, _core) = framed(cx);
    click_entry(cx, 1);
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        Vec::<PathBuf>::new()
    );
    click_entry(cx, 2);
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [Path::new(SHOWN).join(PICKED)]
    );
    click_entry(cx, 0);
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [Path::new(SHOWN).join("0")]
    );
    let only = Click {
        only: true,
        double: false,
    };
    CLICKS.with_borrow(|clicks| {
        let names: Vec<(&str, Click)> = clicks
            .iter()
            .map(|(name, click)| (name.as_str(), *click))
            .collect();
        assert_eq!(names, [(TAKEN, only), (PICKED, only), ("0", only)]);
    });
}

#[gpui::test]
fn a_part_draws_a_footer_under_the_list(cx: &mut TestAppContext) {
    let (view, cx, _core) = framed(cx);
    assert!(cx.debug_bounds(FOOTER).is_none());
    FOOTED.set(true);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let footer = cx.debug_bounds(FOOTER).unwrap();
    let entry = cx.debug_bounds("entry-0").unwrap();
    assert!(footer.top() > entry.bottom());
}

#[gpui::test]
fn a_part_hears_the_window_size(cx: &mut TestAppContext) {
    let (view, cx, _core) = framed(cx);
    view.update_in(cx, MainWindow::watch_size);
    let resized = size(px(700.0), px(500.0));
    cx.simulate_resize(resized);
    cx.run_until_parked();
    SIZES.with_borrow(|sizes| {
        assert!(sizes.len() >= 2, "{sizes:?}");
        assert_eq!(sizes.last(), Some(&resized));
    });
}

#[gpui::test]
fn a_part_peeks_at_an_event_the_window_still_gets(cx: &mut TestAppContext) {
    let (_view, cx, core) = framed(cx);
    core.send(properties_api::Event::PropertiesUnreadable {
        id: PEEKED,
        path: PathBuf::from(MISSING),
        reason: Unreadable::Missing,
    });
    cx.run_until_parked();
    PEEKS.with_borrow(|peeks| assert_eq!(peeks, &[PEEKED]));
}
