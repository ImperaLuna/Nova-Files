use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext as _, Entity};
use nova_files_messages::{Connection, CoreGone, Message};

use crate::entries::Entries;
use crate::island::Island;
use crate::location::Location;
use crate::navigator::Navigator;
use crate::rules::RulesBook;
use crate::scene::Scene;

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

#[derive(Clone)]
pub struct Host {
    pub link: Link,
    pub home: PathBuf,
    pub rules: RulesBook,
    pub entries: Entity<Entries>,
    pub nav: Entity<Navigator>,
    pub island: Entity<Island>,
}

impl Host {
    pub fn new(link: Link, home: PathBuf, cx: &mut App) -> Self {
        Self {
            link,
            home,
            rules: RulesBook::default(),
            entries: cx.new(|_| Entries::default()),
            nav: cx.new(|_| Navigator::default()),
            island: cx.new(|_| Island::default()),
        }
    }

    pub fn scene(&self, cx: &App) -> Scene {
        let entries = self.entries.read(cx);
        let location = entries.location().cloned();
        let rules = location
            .as_ref()
            .and_then(Location::folder)
            .map(|folder| self.rules.for_folder(folder))
            .unwrap_or_default();
        Scene {
            location,
            rules,
            selected: entries.selected_count(),
            picker: entries.picker(),
        }
    }
}
