//! Drives against a scripted core: the list the core sends shows, a click
//! on a volume not mounted mounts it and opens it once mounted (and only
//! that one), and a volume unmounted under the window sends it home.

use std::ffi::OsString;

use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent};
use nova_files_api::{DiskSpace, Drive, DriveAction, DriveId, DriveList, Volume, VolumeId};

use super::*;

const STICK: &str = "/media/stick";

fn volume(id: &str, mount_point: Option<&str>) -> Volume {
    Volume {
        id: VolumeId(id.to_owned()),
        drive: None,
        label: id.to_owned(),
        size: 1 << 30,
        filesystem: "vfat".to_owned(),
        mount_point: mount_point.map(PathBuf::from),
        device: OsString::from(format!("/dev/{id}")),
        read_only: false,
    }
}

fn drives(volumes: Vec<Volume>) -> Event {
    Event::Drives(Arc::new(DriveList {
        drives: Vec::new(),
        volumes,
    }))
}

/// A window on [`SHOWN`], its home, shown, with the core's drive list
/// `volumes` arrived.
fn with_drives(
    cx: &mut TestAppContext,
    volumes: Vec<Volume>,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    core.send(drives(volumes));
    cx.run_until_parked();
    (view, cx, core)
}

/// The drive actions the window sent since last asked.
fn drive_actions(core: &mut Core) -> Vec<DriveAction> {
    let mut actions = Vec::new();
    while let Some(message) = core.try_recv() {
        if let CoreMessage::Command {
            command: Command::Drive(action),
            ..
        } = message
        {
            actions.push(action);
        }
    }
    actions
}

#[gpui::test]
fn the_drives_the_core_lists_show_with_no_space_until_told(cx: &mut TestAppContext) {
    let (view, cx, _core) = with_drives(cx, vec![volume("stick", None)]);
    view.read_with(cx, |view, _| {
        let names: Vec<_> = view.shown_drives().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["System", "stick"]);
        assert_eq!(view.disk_space(Path::new("/")), None);
    });
}

#[gpui::test]
fn the_space_the_core_tells_is_kept_by_folder(cx: &mut TestAppContext) {
    let (view, cx, core) = with_drives(cx, Vec::new());
    let space = DiskSpace {
        total: 100,
        free: 40,
    };
    core.send(Event::DiskSpace(vec![nova_files_api::PathSpace {
        path: PathBuf::from("/"),
        space: Some(space),
    }]));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.disk_space(Path::new("/")), Some(space));
        assert_eq!(view.disk_space(Path::new(STICK)), None);
    });
}

#[gpui::test]
fn a_volume_clicked_is_mounted_then_opened(cx: &mut TestAppContext) {
    let (view, cx, mut core) = with_drives(cx, vec![volume("stick", None), volume("card", None)]);
    let stick = view.read_with(cx, |view, _| view.shown_drives()[1].clone());
    // Clicked twice while it mounts: one mount.
    view.update(cx, |view, _| view.open_drive(stick.clone()));
    view.update(cx, |view, _| view.open_drive(stick));
    let stick_id = VolumeId("stick".to_owned());
    assert_eq!(
        drive_actions(&mut core),
        [DriveAction::Mount(stick_id.clone())]
    );

    // Another volume mounted meanwhile isn't opened.
    core.send(Event::DriveDone {
        action: DriveAction::Mount(VolumeId("card".to_owned())),
        result: Ok(Some(PathBuf::from("/media/card"))),
    });
    cx.run_until_parked();
    assert_eq!(core.next_listing(), None);

    core.send(Event::DriveDone {
        action: DriveAction::Mount(stick_id),
        result: Ok(Some(PathBuf::from(STICK))),
    });
    cx.run_until_parked();
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(STICK))
    );
}

/// A window shown in the stick, mounted at [`STICK`].
fn in_the_stick(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (view, cx, mut core) = with_drives(cx, vec![volume("stick", Some(STICK))]);
    view.update(cx, |view, _| view.open(Path::new(STICK)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

#[gpui::test]
fn a_volume_unmounted_under_the_window_sends_it_home(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = in_the_stick(cx);
    core.send(drives(vec![volume("stick", None)]));
    cx.run_until_parked();
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(SHOWN))
    );
}

#[gpui::test]
fn a_volume_still_mounted_keeps_the_window_in_it(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = in_the_stick(cx);
    core.send(drives(vec![
        volume("stick", Some(STICK)),
        volume("card", None),
    ]));
    cx.run_until_parked();
    assert_eq!(core.next_listing(), None);
}

#[gpui::test]
fn going_elsewhere_while_a_volume_mounts_keeps_the_window_there(cx: &mut TestAppContext) {
    let (view, cx, mut core) = with_drives(cx, vec![volume("stick", None)]);
    let stick = view.read_with(cx, |view, _| view.shown_drives()[1].clone());
    view.update(cx, |view, _| view.open_drive(stick));
    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(OTHER))
    );

    core.send(Event::DriveDone {
        action: DriveAction::Mount(VolumeId("stick".to_owned())),
        result: Ok(Some(PathBuf::from(STICK))),
    });
    cx.run_until_parked();
    assert_eq!(core.next_listing(), None);
}

#[gpui::test]
fn the_drive_you_are_on_is_marked(cx: &mut TestAppContext) {
    let (_view, cx, _core) = in_the_stick(cx);
    assert!(cx.debug_bounds("drive-1-here").is_some());
    assert!(cx.debug_bounds("drive-0-here").is_none());
}

#[gpui::test]
fn the_keys_go_round_a_drive_menu(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    let mut stick = volume("stick", Some(STICK));
    stick.drive = Some(DriveId("usb".to_owned()));
    core.send(Event::Drives(Arc::new(DriveList {
        drives: vec![Drive {
            id: DriveId("usb".to_owned()),
            name: String::new(),
            removable: true,
            ejectable: true,
        }],
        volumes: vec![stick],
    })));
    cx.run_until_parked();
    let position = cx.debug_bounds("drive-1").unwrap().center();
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
    });
    cx.run_until_parked();
    // Open and Open in new tab, a line, then Eject.
    assert_eq!(
        view.read_with(cx, |view, _| view.menu().unwrap().lines.clone()),
        [2]
    );
    let mut marks = Vec::new();
    for key in ["down", "down", "down", "down", "up", "up", "up"] {
        cx.simulate_keystrokes(key);
        marks.push(view.read_with(cx, |view, _| view.menu().unwrap().active.unwrap()));
    }
    // Open, Open in new tab, Eject: round past each end.
    assert_eq!(marks, [0, 1, 2, 0, 2, 1, 0]);
    // The pointer marks what it is over.
    let over = cx.debug_bounds("menu-1").unwrap().center();
    cx.simulate_mouse_move(over, None, Modifiers::none());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert!(menu.marks(1) && !menu.marks(0));
    });
}
