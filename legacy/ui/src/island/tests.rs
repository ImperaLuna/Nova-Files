use nova_files_api::JobId;

use super::*;

#[test]
fn a_note_loses_only_its_own_undo() {
    let mut note = Some(Note {
        label: "Moved a to Trash".into(),
        undo: Some(Undo::Job(JobId(1))),
    });
    assert!(!drop_undo(&mut note, JobId(2)));
    assert!(note.as_ref().is_some_and(|note| note.undo.is_some()));
    assert!(drop_undo(&mut note, JobId(1)));
    assert!(note.is_some_and(|note| note.undo.is_none()));
    assert!(!drop_undo(&mut None, JobId(1)));
}

#[test]
fn the_for_all_tick_holds_only_for_its_own_question() {
    let mut island = Island::default();
    assert!(!island.ticked_for_all(JobId(1), 1));
    island.for_all = Some((JobId(1), 1));
    assert!(island.ticked_for_all(JobId(1), 1));
    assert!(!island.ticked_for_all(JobId(1), 2));
    assert!(!island.ticked_for_all(JobId(2), 1));
}
