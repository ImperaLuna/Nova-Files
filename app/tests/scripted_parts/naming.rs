use std::cell::Cell;
use std::ffi::OsStr;
use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _, Pixels, Size,
    Styled as _, div, px,
};
use nova_files_api::Entry;
use nova_files_shell::{Host, Part, Parts, Slots, TextField, Undo};

use super::super::*;

const NAMED: &str = "1";

thread_local! {
    static UNDONE: Cell<usize> = const { Cell::new(0) };
}

struct Namer {
    field: Option<Entity<TextField>>,
    room: Option<Size<Pixels>>,
}

impl Part for Namer {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self {
            field: None,
            room: None,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.name_field(|namer, entry: &Entry, _| {
            (entry.name == NAMED).then(|| namer.field.clone()).flatten()
        });
        slots.busy(|namer, _| namer.field.is_some());
        slots.stage(|namer, scene, room, _, _| {
            namer.room = Some(room);
            (scene.selected == 1).then(|| {
                div()
                    .debug_selector(|| "namer-stage".to_owned())
                    .w(room.width / 2.0)
                    .h(px(10.0))
                    .into_any_element()
            })
        });
        slots.layer(|namer, _, _, _| {
            namer.room.map(|_| {
                div()
                    .debug_selector(|| "namer-layer".to_owned())
                    .absolute()
                    .inset_0()
                    .into_any_element()
            })
        });
    }
}

fn named(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext) {
    UNDONE.set(0);
    let mut parts = Parts::default();
    parts.add::<nova_files_folders_ui::FoldersPart>();
    parts.add::<Namer>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx)
}

fn namer(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Entity<Namer> {
    view.read_with(cx, |view, _| view.part::<Namer>()).unwrap()
}

fn open_field(view: &Entity<MainWindow>, cx: &mut VisualTestContext) {
    let namer = namer(view, cx);
    namer.update(cx, |namer, cx| {
        namer.field = Some(cx.new(TextField::new));
        cx.notify();
    });
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
}

#[gpui::test]
fn a_part_name_field_draws_in_its_row_inside_the_rename_box(cx: &mut TestAppContext) {
    let (view, cx) = named(cx);
    assert!(cx.debug_bounds("rename").is_none());
    open_field(&view, cx);
    assert!(cx.debug_bounds("rename").is_some());
    assert!(cx.debug_bounds("name-1").is_none());
    assert!(cx.debug_bounds("name-0").is_some());
}

#[gpui::test]
fn the_entries_say_which_rows_are_drawn(cx: &mut TestAppContext) {
    let (view, cx) = named(cx);
    let entries = view.read_with(cx, |view, _| view.host().entries.clone());
    entries.read_with(cx, |entries, _| {
        assert!(entries.drawn(OsStr::new(NAMED)));
        assert!(!entries.drawn(OsStr::new("gone")));
    });
}

#[gpui::test]
fn a_busy_part_holds_undo(cx: &mut TestAppContext) {
    let (view, cx) = named(cx);
    open_field(&view, cx);
    let island = view.read_with(cx, |view, _| view.host().island.clone());
    let undo = Undo::Run(Rc::new(|_| UNDONE.set(UNDONE.get() + 1)));
    island.update(cx, |island, cx| island.note("Named", Some(undo), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(UNDONE.get(), 0);
}

#[gpui::test]
fn a_part_draws_the_gallery_stage_in_the_room_it_is_given(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (_view, cx) = named(cx);
    cx.simulate_keystrokes("ctrl-3");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert!(cx.debug_bounds("stage-name").is_none());
    let drawn = cx.debug_bounds("namer-stage").unwrap();
    let stage = cx.debug_bounds("stage").unwrap();
    assert!(drawn.size.width > px(0.0));
    assert!(drawn.size.width < stage.size.width);
}

#[gpui::test]
fn a_part_layer_covers_the_whole_window_once_its_stage_knows_the_room(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = named(cx);
    assert!(cx.debug_bounds("namer-layer").is_none());
    cx.simulate_keystrokes("ctrl-3");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let room = namer(&view, cx)
        .read_with(cx, |namer, _| namer.room)
        .unwrap();
    assert!(room.width > px(0.0));
    let layer = cx.debug_bounds("namer-layer").unwrap();
    let viewport = cx.update(|window, _| window.viewport_size());
    assert_eq!(layer.size, viewport);
}

#[gpui::test]
fn a_part_focuses_its_own_field(cx: &mut TestAppContext) {
    let (view, cx) = named(cx);
    open_field(&view, cx);
    let field = namer(&view, cx).read_with(cx, |namer, _| namer.field.clone().unwrap());
    let handle = field.read_with(cx, gpui::Focusable::focus_handle);
    let nav = view.read_with(cx, |view, _| view.host().nav.clone());
    nav.update(cx, |nav, cx| nav.focus(handle.clone(), cx));
    cx.run_until_parked();
    assert!(cx.update(|window, _| handle.is_focused(window)));
}
