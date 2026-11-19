use std::cell::RefCell;

use gpui::{Context, Entity};
use nova_files_api::WindowState;
use nova_files_shell::{Host, Kept, Part, Parts, Slots};

use super::super::*;

thread_local! {
    static READ: RefCell<Vec<Option<String>>> = const { RefCell::new(Vec::new()) };
}

const KEY: &str = "trash";

struct Keeper {
    kept: Entity<Kept>,
}

impl Part for Keeper {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        cx.observe(&host.kept, |_, kept, cx| {
            let value = kept.read(cx).get(KEY).map(str::to_owned);
            READ.with_borrow_mut(|read| read.push(value));
        })
        .detach();
        Self {
            kept: host.kept.clone(),
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.closing(|keeper, cx| {
            keeper.kept.update(cx, |kept, _| kept.set(KEY, "hidden"));
        });
    }
}

fn state(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut state = WindowState::default();
    for (key, value) in pairs {
        state.set(key, value);
    }
    state.to_bytes()
}

#[gpui::test]
fn a_part_reads_what_the_window_kept_and_saves_only_its_own_key(cx: &mut TestAppContext) {
    let mut parts = Parts::default();
    parts.add::<Keeper>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    cx.run_until_parked();
    assert!(core.sent::<Command>().contains(&Command::WindowState));

    core.send(Event::WindowState(state(&[
        ("picker", "900x600"),
        (KEY, "shown"),
    ])));
    cx.run_until_parked();
    assert_eq!(READ.with_borrow(Clone::clone), [Some("shown".to_owned())]);

    drop(view);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let saves: Vec<Vec<u8>> = core
        .sent::<Command>()
        .into_iter()
        .filter_map(|command| match command {
            Command::SaveWindowState(bytes) => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(saves, [state(&[(KEY, "hidden")])]);
}
