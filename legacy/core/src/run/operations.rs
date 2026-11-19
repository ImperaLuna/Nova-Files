use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command};

use super::Setup;
use crate::jobs::Client;
use nova_files_transfer_api as transfer_api;
use nova_files_transfer_core::answer_drop_would;

use crate::ops;

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
            return setup.jobs.start(client.events().as_ref(), tag, operation);
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

pub(super) fn drop(id: ClientId, client: &Client, command: transfer_api::Command, setup: &Setup) {
    match command {
        transfer_api::Command::Drop { tag, sources, into } => {
            setup.jobs.audience().join(id, Arc::clone(client.events()));
            ops::start_drop(&setup.jobs, client.events(), tag, sources, into);
        }
        transfer_api::Command::DropWould { id, sources, into } => {
            answer_drop_would(client.events(), id, sources, into);
        }
    }
}
