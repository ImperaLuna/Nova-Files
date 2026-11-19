//! Gathered folders: Games and Projects, the places that gather folders
//! from several spots into one list.
//!
//! The core keeps them in `gathered.toml` in its config folder, which
//! people may also write by hand or from Nix. A client asks for them with
//! [`Command::Gathered`] and changes them with [`Command::ChangeGathered`];
//! every client that asked hears [`Event::Gathered`] after each change,
//! from any client or the file.
//!
//! [`Command::Gathered`]: crate::Command::Gathered
//! [`Command::ChangeGathered`]: crate::Command::ChangeGathered
//! [`Event::Gathered`]: crate::Event::Gathered

#![forbid(unsafe_code)]

mod gathered;

use std::sync::Arc;

use nova_files_api::{ListingId, Order};

pub use gathered::{Gathered, GatheredChange, GatheredFolder, Gathering, Holds};

/// What a client asks of Games and Projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask for the folders Games and Projects gather from: the answer is
    /// [`Event::Gathered`], and from then on the client gets it again
    /// whenever they change (from any client, or `gathered.toml` edited).
    Gathered,
    /// Add or remove a gathered folder and save it. Every client that
    /// asked, this one included, then gets [`Event::Gathered`]; nothing is
    /// sent when nothing changed.
    ChangeGathered(GatheredChange),
    /// List what `place` gathers in `order`: the folders inside each of
    /// its folders that holds many, and each one-project folder itself,
    /// folders only, as the hidden-files setting says. The answer is
    /// [`Event::FolderListed`] with `id`, each entry in its own folder
    /// (see [`Listing::folders`]) and an empty [`Listing::path`]. It is
    /// the client's listing like a [`Command::ListFolder`]: either one
    /// asked for later cancels it, and [`Command::Rows`] serves its rows.
    ///
    /// [`Event::FolderListed`]: nova_files_api::Event::FolderListed
    /// [`Listing::folders`]: nova_files_api::Listing::folders
    /// [`Listing::path`]: nova_files_api::Listing::path
    /// [`Command::ListFolder`]: nova_files_api::Command::ListFolder
    /// [`Command::Rows`]: nova_files_api::Command::Rows
    ListGathered {
        id: ListingId,
        place: Gathering,
        order: Order,
    },
}

/// What Games and Projects tell a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The folders Games and Projects gather from (see
    /// [`Command::Gathered`]).
    Gathered(Arc<Gathered>),
    /// What is inside `place`'s folders changed (a folder made, removed or
    /// renamed in one, or one of them appeared or went away): a client
    /// showing it lists it again. Sent to the clients that asked with
    /// [`Command::Gathered`].
    GatheredChanged(Gathering),
}
