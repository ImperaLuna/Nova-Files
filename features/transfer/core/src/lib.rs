#![forbid(unsafe_code)]

mod ahead;
mod attrs;
mod copy_file;
mod dropped;
mod kind;
mod make;
mod transfer;
mod undo;

pub use dropped::{answer as answer_drop_would, dropped};
pub use kind::Transfers;
pub use make::How;
pub use transfer::{transfer, transfer_to};
