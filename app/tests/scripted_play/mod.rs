use std::time::Duration;

use nova_files_preview_api::{
    Command as PreviewCommand, Control, Event as PreviewEvent, Played, Preview, PreviewContent,
    PreviewId,
};

use super::*;

const WATCHED: Duration = Duration::from_millis(83_500);

fn looking_at_a_video(
    cx: &mut TestAppContext,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core, PreviewId) {
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down space");
    cx.run_until_parked();
    let (id, path) = core
        .sent::<PreviewCommand>()
        .into_iter()
        .find_map(|command| match command {
            PreviewCommand::Preview { id, path, .. } => Some((id, path)),
            _ => None,
        })
        .unwrap();
    let preview = Preview {
        path,
        kind_name: None,
        bytes: 1,
        content: PreviewContent::Media { video: true },
    };
    core.send(PreviewEvent::Previewed {
        id,
        preview: Arc::new(preview),
    });
    cx.run_until_parked();
    (view, cx, core, id)
}

fn controls(core: &mut Core) -> Vec<Control> {
    core.sent::<PreviewCommand>()
        .into_iter()
        .filter_map(|command| match command {
            PreviewCommand::Control { control, .. } => Some(control),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn quick_look_plays_a_video_and_k_j_l_steer_it(cx: &mut TestAppContext) {
    let (_view, cx, mut core, id) = looking_at_a_video(cx);
    assert!(controls(&mut core).contains(&Control::Play));
    core.send(PreviewEvent::Played {
        id,
        played: Played::Paused(false),
    });
    core.send(PreviewEvent::Played {
        id,
        played: Played::At(WATCHED),
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("k l j");
    cx.run_until_parked();

    let later = WATCHED + Duration::from_secs(10);
    assert_eq!(
        controls(&mut core),
        [Control::Pause, Control::Seek(later), Control::Seek(WATCHED)]
    );
}

#[gpui::test]
fn enter_hands_the_video_to_the_default_app_where_it_was(cx: &mut TestAppContext) {
    let (view, cx, mut core, id) = looking_at_a_video(cx);
    core.send(PreviewEvent::Played {
        id,
        played: Played::At(WATCHED),
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let handed: Vec<_> = core
        .sent::<PreviewCommand>()
        .into_iter()
        .filter_map(|command| match command {
            PreviewCommand::OpenAt { path, at } => Some((path, at)),
            _ => None,
        })
        .collect();
    assert_eq!(handed, [(Path::new(SHOWN).join("0"), WATCHED)]);
    assert!(!view.read_with(cx, previewing));
}
