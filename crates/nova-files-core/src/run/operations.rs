//! The commands about file operations: watching, starting, dropping
//! files, pausing, resuming, cancelling and forgetting jobs, and answering
//! their conflicts. `run` sends only these here.

use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command};

use super::Setup;
use crate::jobs::Client;

/// Answers one of the job commands for client `id`.
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
        // `run` sends the other commands elsewhere.
        _ => {}
    }
    Ok(())
}
