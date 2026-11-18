#![forbid(unsafe_code)]

pub mod assets;
mod command;
mod entries;
mod host;
mod island;
mod location;
mod menu;
mod navigator;
mod part;
mod parts;
mod per_tab;
mod published;
mod rules;
mod scene;
mod slot_view;
pub mod theme;

pub use command::{CommandSpec, WINDOW_KEYS};
pub use entries::{Entries, EntriesAsked, Shown};
pub use host::{Host, Link};
pub use island::{Island, IslandAsk, IslandAsked, OnYes};
pub use location::{Location, PageKind, PageRef};
pub use menu::{Look, MenuAt, MenuItem, SidebarAt, SidebarRow};
pub use navigator::{NavAsk, NavEvent, Navigator};
pub use part::{
    Applies, Card, Cell, Column, Columns, DropSlot, EmptyNote, Items, Listed, NameCell, Part, Rows,
    Slots, Watch,
};
pub use parts::{Parts, ShownColumn, WindowParts};
pub use per_tab::{PerTab, TabId};
pub use published::Published;
pub use rules::{FolderName, FolderRules, RulesBook, Views};
pub use scene::Scene;
