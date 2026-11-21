#![forbid(unsafe_code)]

pub mod client;
pub mod details;
mod details_queue;
pub mod folder;
pub mod list;
pub mod memory;
pub mod order;
pub mod sizes;

pub use client::{Client, ListingJob, RowWatch};
pub use folder::{Inner, InnerFolders, list_folder};
pub use sizes::{FolderSizer, MeasureFolders};
