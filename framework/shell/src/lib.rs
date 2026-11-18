#![forbid(unsafe_code)]

pub mod assets;
mod command;
mod host;
mod part;
mod parts;
mod per_tab;
mod published;
mod rules;
pub mod theme;

pub use command::{CommandSpec, WINDOW_KEYS};
pub use host::{Host, Link};
pub use part::{Part, Slots};
pub use parts::{Parts, WindowParts};
pub use per_tab::{PerTab, TabId};
pub use published::Published;
pub use rules::{FolderRules, RulesBook, Views};
