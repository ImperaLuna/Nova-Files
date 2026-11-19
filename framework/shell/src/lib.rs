#![forbid(unsafe_code)]

pub mod assets;
mod command;
mod entries;
pub mod format;
mod host;
mod island;
pub mod jump;
pub mod kind;
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
pub mod text;
pub mod theme;
pub mod widgets;

pub use command::{CommandSpec, WINDOW_KEYS};
pub use entries::{Entries, EntriesAsked, Shown};
pub use host::{AskIds, Host, Link};
pub use island::{Island, IslandAsk, IslandAsked, NoteGone, NoteId, OnUndo, OnYes, Undo};
pub use location::{Location, PageKind, PageRef};
pub use menu::{
    Expander, Look, MenuAt, MenuGroup, MenuItem, OnRun, Placed, RowItem, SidebarAt, SidebarRow,
    arrange,
};
pub use navigator::{FolderPick, NavAsk, NavEvent, Navigator, OnPicked};
pub use part::{
    Applies, Card, Cell, Closing, Column, Columns, DropAim, DropSlot, DropTake, EmptyNote,
    HeadButton, Items, Listed, NameCell, Opens, Part, RowMenu, Rows, Slots, Typed, Watch,
};
pub use parts::{Parts, ShownColumn, WindowParts};
pub use per_tab::{PerTab, TabId};
pub use published::Published;
pub use rules::{FolderName, FolderRules, RulesBook, Views};
pub use scene::Scene;
pub use text::{TextField, TextFieldEvent};
