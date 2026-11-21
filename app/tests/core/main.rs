#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[path = "common/answers.rs"]
mod answers;
mod common;
mod feature;
#[path = "common/files.rs"]
mod files;
#[path = "common/own_folders.rs"]
mod own_folders;

mod archive_names;
mod conflicts;
mod copy;
mod deletes;
mod disk_space;
mod file_icons;
mod folder_views;
mod gathered;
mod icon_theme;
mod jobs;
mod lifecycle;
mod listing;
mod measure;
mod moves;
mod open;
mod open_with;
mod places;
mod preview;
mod properties;
mod recent;
mod restoring;
mod resume;
mod search;
mod settings;
mod space;
mod trashing;
mod undoing;
