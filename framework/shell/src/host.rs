use std::sync::Arc;

use nova_files_messages::{Connection, CoreGone, Message};

use crate::rules::RulesBook;

#[derive(Clone, Default)]
pub struct Link(Option<Arc<Connection>>);

impl Link {
    pub const fn new(connection: Option<Arc<Connection>>) -> Self {
        Self(connection)
    }

    pub fn send<M: Message>(&self, message: M) -> Result<(), CoreGone> {
        self.0.as_ref().ok_or(CoreGone)?.send(message)
    }
}

#[derive(Clone, Default)]
pub struct Host {
    pub link: Link,
    pub rules: RulesBook,
}
