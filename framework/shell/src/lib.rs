#![forbid(unsafe_code)]

pub mod assets;
mod command;
mod entries;
pub mod format;
mod host;
mod island;
pub mod jump;
pub mod kind;
mod line;
mod location;
mod menu;
mod navigator;
mod opening;
mod part;
mod parts;
pub mod paths;
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
pub use line::{Line, Lines};
pub use location::{Location, PageKind, PageRef, StartPage};
pub use menu::{
    Expander, Look, MenuAt, MenuGroup, MenuItem, OnRun, Placed, RowItem, SidebarAt, SidebarRow,
    arrange,
};
pub use navigator::{FolderPick, NavAsk, NavEvent, Navigator, OnPicked, PopUp};
pub use opening::Opening;
pub use part::{
    Applies, Busy, Card, CardRank, Cell, Closing, Column, Columns, Dims, DropAim, DropOver,
    DropSlot, DropTake, EmptyNote, Focus, HeadButton, Items, Listed, NameCell, NameField, Opens,
    Part, Refused, Rescaled, RowIcon, RowMenu, Rows, RowsDrawn, Selectable, Several, Slots, Stage,
    Typed, Used, Watch,
};
pub use parts::{Parts, ShownColumn, Turn, WindowParts};
pub use per_tab::{PerTab, TabId};
pub use published::{KnownFolder, KnownFolders, Published, ThumbnailsShown};
pub use rules::{FolderName, FolderRules, RulesBook, Views};
pub use scene::Scene;
pub use text::{TextField, TextFieldEvent};
