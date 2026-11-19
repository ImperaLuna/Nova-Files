use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, Entity};
use nova_files_messages::{Connection, CoreGone, Message};

use crate::entries::Entries;
use crate::island::Island;
use crate::line::Lines;
use crate::location::Location;
use crate::navigator::Navigator;
use crate::opening::Opening;
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

#[derive(Clone, Default)]
pub struct AskIds(Rc<Cell<u64>>);

impl AskIds {
    pub fn next(&self) -> u64 {
        let next = self.0.get() + 1;
        self.0.set(next);
        next
    }
}

#[derive(Clone)]
pub struct Host {
    pub link: Link,
    pub asks: AskIds,
    pub home: PathBuf,
    pub rules: RulesBook,
    pub entries: Entity<Entries>,
    pub nav: Entity<Navigator>,
    pub island: Entity<Island>,
    pub opening: Opening,
    pub lines: Lines,
}

impl Host {
    #[must_use]
    pub fn with_lines(mut self, lines: Lines) -> Self {
        self.lines = lines;
        self
    }

    pub fn new(link: Link, home: PathBuf, opening: Opening, cx: &mut App) -> Self {
        Self {
            link,
            asks: AskIds::default(),
            home,
            rules: RulesBook::default(),
            entries: cx.new(|_| Entries::default()),
            nav: cx.new(|_| Navigator::default()),
            island: cx.new(|_| Island::default()),
            opening,
            lines: Lines::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_holder_of_the_ask_ids_draws_from_one_count() {
        let window = AskIds::default();
        let part = window.clone();

        assert_eq!([window.next(), part.next(), window.next()], [1, 2, 3]);
    }
}
