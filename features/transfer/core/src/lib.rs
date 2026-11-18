#![forbid(unsafe_code)]

mod ahead;
mod attrs;
mod copy_file;
mod dropped;
mod make;
mod transfer;

pub use dropped::{answer as answer_drop_would, dropped};
pub use make::How;
pub use transfer::{transfer, transfer_to};
