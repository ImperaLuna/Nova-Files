use nova_files_api::{Answer, Command, Conflict, Event, JobId, Scope};

use super::common::Client;

pub fn held(client: &Client, job: JobId) -> Conflict {
    loop {
        match client.next() {
            Event::JobConflict { job: at, conflict } if at == job => return conflict,
            Event::JobProgress { .. } => {}
            other => panic!("expected job {job:?} to hold at a conflict, got {other:?}"),
        }
    }
}

pub fn answer(client: &Client, job: JobId, conflict: &Conflict, answer: Answer, scope: Scope) {
    client.send(Command::AnswerConflict {
        job,
        conflict: conflict.clone(),
        answer,
        scope,
    });
    loop {
        match client.next() {
            Event::ConflictAnswered {
                job: at,
                answer: heard,
            } if at == job => {
                assert_eq!(heard, answer);
                return;
            }
            Event::JobProgress { .. } => {}
            other => panic!("expected the answer to be heard, got {other:?}"),
        }
    }
}
