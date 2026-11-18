use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command};

use super::Setup;
use crate::jobs::Client;
use crate::ops::answer_drop_would;

pub(super) fn answer(
    id: ClientId,
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), ClientGone> {
    match command {
        Command::WatchJobs => setup.jobs.watch(id, client.events()),
        Command::StartJob { tag, operation } => {
            setup.jobs.audience().join(id, Arc::clone(client.events()));
            return setup.jobs.start(client, tag, operation);
        }
        Command::DropFiles { tag, sources, into } => {
            setup.jobs.audience().join(id, Arc::clone(client.events()));
            setup.jobs.start_drop(client.events(), tag, sources, into);
        }
        Command::DropWould { id, sources, into } => {
            answer_drop_would(client.events(), id, sources, into);
        }
        Command::PauseJob(job) => setup.jobs.pause(job),
        Command::ResumeJob(job) => setup.jobs.resume(job),
        Command::ForgetJob(job) => setup.jobs.forget(job),
        Command::CancelJob(job) => setup.jobs.cancel(job),
        Command::AnswerConflict {
            job,
            conflict,
            answer,
            scope,
        } => setup.jobs.answer(job, &conflict, answer, scope),
        Command::GivePassword {
            job,
            conflict,
            password,
        } => setup.jobs.give_password(job, &conflict, password),
        _ => {}
    }
    Ok(())
}
