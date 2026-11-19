use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command, EventSink, Operation};

use crate::runner::{JobKind, Jobs};

impl<K: JobKind<Op = Operation>> Jobs<K> {
    pub fn command(
        &self,
        client: ClientId,
        events: &Arc<dyn EventSink>,
        command: Command,
    ) -> Result<(), ClientGone> {
        match command {
            Command::WatchJobs => self.watch(client, events),
            Command::StartJob { tag, operation } => {
                self.audience().join(client, Arc::clone(events));
                return self.start(events.as_ref(), tag, operation);
            }
            Command::PauseJob(job) => self.pause(job),
            Command::ResumeJob(job) => self.resume(job),
            Command::ForgetJob(job) => self.forget(job),
            Command::CancelJob(job) => self.cancel(job),
            Command::AnswerConflict {
                job,
                conflict,
                answer,
                scope,
            } => self.answer(job, &conflict, answer, scope),
            Command::GivePassword {
                job,
                conflict,
                password,
            } => self.give_password(job, &conflict, password),
            _ => {}
        }
        Ok(())
    }
}
