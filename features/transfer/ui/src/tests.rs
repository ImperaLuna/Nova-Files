use super::*;

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

#[test]
fn keys_held_pick_copy_or_move_and_otherwise_the_drive_does() {
    let both = Modifiers {
        shift: true,
        ..Modifiers::control()
    };
    assert_eq!(picked(Modifiers::control()), Some(DropKind::Copy));
    assert_eq!(picked(both), Some(DropKind::Link));
    assert_eq!(picked(Modifiers::shift()), Some(DropKind::Move));
    assert_eq!(picked(Modifiers::none()), None);
}

#[test]
fn the_chip_names_the_drop_and_its_folder() {
    assert_eq!(label(DropKind::Move, "Backup"), "Move to Backup");
    assert_eq!(label(DropKind::Copy, "Backup"), "Copy to Backup");
    assert_eq!(label(DropKind::Link, "Backup"), "Link in Backup");
}

#[test]
fn until_the_drive_answers_the_chip_names_what_is_dragged() {
    assert_eq!(what(&paths(&["/a/notes.txt"])), "notes.txt");
    assert_eq!(what(&paths(&["/"])), "/");
    assert_eq!(what(&paths(&["/a/x", "/a/y"])), "2 items");
}

#[test]
fn each_folder_is_asked_once_per_drag_and_remembers_the_answer() {
    let mut drives = Drives::default();
    let (sources, into) = (paths(&["/a/x"]), PathBuf::from("/b"));
    assert_eq!(drives.kind(&sources, &into), Lookup::Unasked);
    assert_eq!(drives.kind(&sources, &into), Lookup::Asked);
    drives
        .asked
        .insert(DropAskId(1), (sources.clone(), into.clone()));
    assert_eq!(
        drives.answered(DropAskId(1), DropKind::Copy),
        Some((sources.clone(), into.clone()))
    );
    assert_eq!(drives.answered(DropAskId(1), DropKind::Copy), None);
    assert_eq!(drives.kind(&sources, &into), Lookup::Known(DropKind::Copy));
    assert_eq!(drives.kind(&paths(&["/a/y"]), &into), Lookup::Unasked);
}

#[test]
fn an_answer_for_an_earlier_drag_is_not_kept() {
    let mut drives = Drives::default();
    let (first, into) = (paths(&["/a/x"]), PathBuf::from("/b"));
    drives.kind(&first, &into);
    drives
        .asked
        .insert(DropAskId(1), (first.clone(), into.clone()));
    drives.kind(&paths(&["/a/y"]), &into);
    assert!(drives.answered(DropAskId(1), DropKind::Move).is_some());
    assert_eq!(drives.kind(&paths(&["/a/y"]), &into), Lookup::Asked);
}

#[test]
fn duplicates_go_beside_their_originals_one_folder_at_a_time() {
    assert_eq!(
        by_folder(paths(&["/a/x", "/b/y", "/a/z", "/"])),
        vec![
            (PathBuf::from("/a"), paths(&["/a/x", "/a/z"])),
            (PathBuf::from("/b"), paths(&["/b/y"])),
        ]
    );
}
