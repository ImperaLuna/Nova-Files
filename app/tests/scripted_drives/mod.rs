use std::ffi::OsString;

use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent};
use nova_files_drives_api::{
    Command as DrivesCommand, DiskSpace, Drive, DriveAction, DriveFailure, DriveId, DriveList,
    Event as DrivesEvent, Password, Volume, VolumeId,
};

use nova_files_drives_ui::{DrivesPart, Said};

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
        locked: false,
    }
}

fn drives_part(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Entity<DrivesPart> {
    view.read_with(cx, |view, _| view.part::<DrivesPart>().unwrap())
}

fn drives(volumes: Vec<Volume>) -> DrivesEvent {
    DrivesEvent::Drives(Arc::new(DriveList {
        drives: Vec::new(),
        volumes,
    }))
}

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

fn drive_actions(core: &mut Core) -> Vec<DriveAction> {
    let mut actions = Vec::new();
    while let Some(message) = core.try_recv() {
        if let Some(DrivesCommand::Drive(action)) = message.into_command::<DrivesCommand>() {
            actions.push(action);
        }
    }
    actions
}

#[gpui::test]
fn the_drives_the_core_lists_show_with_no_space_until_told(cx: &mut TestAppContext) {
    let (view, cx, _core) = with_drives(cx, vec![volume("stick", None)]);
    drives_part(&view, cx).read_with(cx, |part, _| {
        let names: Vec<_> = part.shown().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["System", "stick"]);
        assert_eq!(part.disk_space(Path::new("/")), None);
    });
}

#[gpui::test]
fn the_space_the_core_tells_is_kept_by_folder(cx: &mut TestAppContext) {
    let (view, cx, core) = with_drives(cx, Vec::new());
    let space = DiskSpace {
        total: 100,
        free: 40,
    };
    core.send(DrivesEvent::DiskSpace(vec![
        nova_files_drives_api::PathSpace {
            path: PathBuf::from("/"),
            space: Some(space),
        },
    ]));
    cx.run_until_parked();
    drives_part(&view, cx).read_with(cx, |part, _| {
        assert_eq!(part.disk_space(Path::new("/")), Some(space));
        assert_eq!(part.disk_space(Path::new(STICK)), None);
    });
}

#[gpui::test]
fn a_volume_clicked_is_mounted_then_opened(cx: &mut TestAppContext) {
    let (view, cx, mut core) = with_drives(cx, vec![volume("stick", None), volume("card", None)]);
    let part = drives_part(&view, cx);
    let stick = part.read_with(cx, |part, _| part.shown()[1].clone());
    part.update(cx, |part, cx| part.open_drive(stick.clone(), cx));
    part.update(cx, |part, cx| part.open_drive(stick, cx));
    let stick_id = VolumeId("stick".to_owned());
    assert_eq!(
        drive_actions(&mut core),
        [DriveAction::Mount(stick_id.clone())]
    );

    core.send(DrivesEvent::DriveDone {
        action: DriveAction::Mount(VolumeId("card".to_owned())),
        result: Ok(Some(PathBuf::from("/media/card"))),
    });
    cx.run_until_parked();
    assert_eq!(core.next_listing(), None);

    core.send(DrivesEvent::DriveDone {
        action: DriveAction::Mount(stick_id),
        result: Ok(Some(PathBuf::from(STICK))),
    });
    cx.run_until_parked();
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(STICK))
    );
}

const VAULT: &str = "/media/vault";

fn locked(id: &str) -> Volume {
    Volume {
        locked: true,
        filesystem: "crypto_LUKS".to_owned(),
        ..volume(id, None)
    }
}

fn unlock(password: &str) -> DriveAction {
    DriveAction::Unlock(VolumeId("vault".to_owned()), Password(password.to_owned()))
}

fn asking_for_the_vault(
    cx: &mut TestAppContext,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    cx.update(nova_files_window::bind_text_keys);
    let (view, cx, mut core) = with_drives(cx, vec![locked("vault")]);
    let part = drives_part(&view, cx);
    let vault = part.read_with(cx, |part, _| part.shown()[1].clone());
    assert!(vault.locked);
    part.update(cx, |part, cx| part.open_drive(vault, cx));
    cx.run_until_parked();
    assert_eq!(drive_actions(&mut core), []);
    assert!(cx.debug_bounds("island-password").is_some());
    (view, cx, core)
}

#[gpui::test]
fn a_locked_drive_asks_for_its_password_then_opens(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = asking_for_the_vault(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        drive_actions(&mut core),
        [],
        "an empty password is not sent"
    );

    cx.simulate_input("hunter2");
    cx.simulate_keystrokes("enter");
    assert_eq!(drive_actions(&mut core), [unlock("hunter2")]);

    core.send(DrivesEvent::DriveDone {
        action: unlock("hunter2"),
        result: Ok(Some(PathBuf::from(VAULT))),
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-password").is_none());
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(VAULT))
    );
}

#[gpui::test]
fn a_wrong_password_keeps_the_prompt_for_another_try(cx: &mut TestAppContext) {
    let (view, cx, mut core) = asking_for_the_vault(cx);
    cx.simulate_input("letmein");
    cx.simulate_keystrokes("enter");
    core.send(DrivesEvent::DriveDone {
        action: unlock("letmein"),
        result: Err(DriveFailure::WrongPassword),
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-password").is_some());
    assert_eq!(
        drives_part(&view, cx).read_with(cx, |part, _| part.says()),
        Some(Said::Wrong)
    );
    assert_eq!(core.next_listing(), None);

    drive_actions(&mut core);
    cx.simulate_input("hunter2");
    cx.simulate_keystrokes("enter");
    assert_eq!(drive_actions(&mut core), [unlock("hunter2")]);
}

#[gpui::test]
fn escape_or_an_unplugged_drive_takes_the_prompt_away(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = asking_for_the_vault(cx);
    cx.simulate_keystrokes("escape");
    assert!(cx.debug_bounds("island-password").is_none());
    assert_eq!(drive_actions(&mut core), []);

    let (_view, cx, core) = asking_for_the_vault(cx);
    core.send(drives(Vec::new()));
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-password").is_none());
}

#[gpui::test]
fn a_failure_other_than_a_wrong_password_says_it_can_t_be_unlocked(cx: &mut TestAppContext) {
    let (view, cx, core) = asking_for_the_vault(cx);
    cx.simulate_input("hunter2");
    cx.simulate_keystrokes("enter");
    core.send(DrivesEvent::DriveDone {
        action: unlock("hunter2"),
        result: Err(DriveFailure::NotAuthorized),
    });
    cx.run_until_parked();
    assert_eq!(
        drives_part(&view, cx).read_with(cx, |part, _| part.says()),
        Some(Said::Failed)
    );
    core.send(DrivesEvent::DriveDone {
        action: unlock("hunter2"),
        result: Err(DriveFailure::Gone),
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-password").is_none());
}

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
    let part = drives_part(&view, cx);
    let stick = part.read_with(cx, |part, _| part.shown()[1].clone());
    part.update(cx, |part, cx| part.open_drive(stick, cx));
    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(OTHER))
    );

    core.send(DrivesEvent::DriveDone {
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
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    let mut stick = volume("stick", Some(STICK));
    stick.drive = Some(DriveId("usb".to_owned()));
    core.send(DrivesEvent::Drives(Arc::new(DriveList {
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
    assert_eq!(
        view.read_with(cx, |view, _| view.menu().unwrap().lines.clone()),
        [2]
    );
    let mut marks = Vec::new();
    for key in ["down", "down", "down", "down", "up", "up", "up"] {
        cx.simulate_keystrokes(key);
        marks.push(view.read_with(cx, |view, _| view.menu().unwrap().active.unwrap()));
    }
    assert_eq!(marks, [0, 1, 2, 0, 2, 1, 0]);
    let over = cx.debug_bounds("menu-1").unwrap().center();
    cx.simulate_mouse_move(over, None, Modifiers::none());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert!(menu.marks(1) && !menu.marks(0));
    });
}
