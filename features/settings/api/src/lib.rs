//! The user's settings, as the core keeps them and every window applies
//! them.
//!
//! The core reads them from its settings file and sends the whole set as
//! [`Event::Settings`]: when a client asks, and from then on to that
//! client each time a value changes, whoever changed it. A client changes one value at
//! a time with [`Command::Change`], so two windows changing
//! different values never undo each other.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use nova_files_api::{FolderSizes, HiddenFiles};

/// What a client asks of the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask for the settings; the answer is [`Event::Settings`], and from
    /// then on the client gets [`Event::Settings`] again each time one
    /// changes. Clients that never ask never get them.
    Watch,
    /// Change one setting and save it. Every client that asked for the
    /// settings, this one included, then gets [`Event::Settings`]; nothing
    /// is sent when the value was already set.
    Change(SettingChange),
}

/// What the settings tell a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The settings, answering [`Command::Watch`], and sent again to
    /// each client that asked whenever one changes: from a client, or from
    /// the settings file.
    Settings(Arc<Settings>),
}

/// Every setting, each with its default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// Light or dark, or whichever the desktop prefers.
    pub appearance: Appearance,
    /// Whether rows show thumbnails of pictures and videos.
    pub thumbnails: Thumbnails,
    /// Whether folders list their hidden entries.
    pub hidden_files: HiddenFiles,
    /// Whether a folder sorted by size measures its folders.
    pub folder_sizes: FolderSizes,
    /// Key overrides: an action's id (owned by the UI, such as `go.back`)
    /// to the keys that run it, in GPUI's keystroke syntax
    /// (`ctrl-shift-a`). An empty list leaves the action without keys. The
    /// core keeps these as written; the UI checks them when it binds and
    /// keeps its default for any id or key it can't use.
    pub keys: BTreeMap<String, Vec<String>>,
}

/// Which colors the windows use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    /// Whichever the desktop prefers; dark when it says nothing.
    #[default]
    System,
    Light,
    Dark,
}

/// Whether rows show thumbnails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Thumbnails {
    #[default]
    Shown,
    Hidden,
}

/// One setting's new value, sent with [`Command::Change`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Appearance(Appearance),
    Thumbnails(Thumbnails),
    HiddenFiles(HiddenFiles),
    FolderSizes(FolderSizes),
    /// The keys for the action named `action`; `None` drops the override,
    /// so the action gets its default keys back.
    Keys {
        action: String,
        keys: Option<Vec<String>>,
    },
}

impl Settings {
    /// These settings with `change` made.
    #[must_use]
    pub fn with(mut self, change: SettingChange) -> Self {
        match change {
            SettingChange::Appearance(appearance) => self.appearance = appearance,
            SettingChange::Thumbnails(thumbnails) => self.thumbnails = thumbnails,
            SettingChange::HiddenFiles(hidden) => self.hidden_files = hidden,
            SettingChange::FolderSizes(sizes) => self.folder_sizes = sizes,
            SettingChange::Keys {
                action,
                keys: Some(keys),
            } => {
                self.keys.insert(action, keys);
            }
            SettingChange::Keys { action, keys: None } => {
                self.keys.remove(&action);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_sets_one_value_and_leaves_the_rest() {
        let dark = Settings::default().with(SettingChange::Appearance(Appearance::Dark));
        assert_eq!(dark.appearance, Appearance::Dark);
        assert_eq!(dark.thumbnails, Thumbnails::Shown);
        let hidden = dark.with(SettingChange::Thumbnails(Thumbnails::Hidden));
        assert_eq!(hidden.appearance, Appearance::Dark);
        assert_eq!(hidden.thumbnails, Thumbnails::Hidden);
        assert_eq!(hidden.hidden_files, HiddenFiles::Left);
        let shown = hidden.with(SettingChange::HiddenFiles(HiddenFiles::Shown));
        assert_eq!(shown.hidden_files, HiddenFiles::Shown);
        assert_eq!(shown.thumbnails, Thumbnails::Hidden);
        assert_eq!(shown.folder_sizes, FolderSizes::Measured);
        let left = shown.with(SettingChange::FolderSizes(FolderSizes::Left));
        assert_eq!(left.folder_sizes, FolderSizes::Left);
        assert_eq!(left.hidden_files, HiddenFiles::Shown);
    }

    #[test]
    fn keys_are_set_and_dropped_per_action() {
        let bound = Settings::default()
            .with(SettingChange::Keys {
                action: "go.back".to_owned(),
                keys: Some(vec!["alt-left".to_owned()]),
            })
            .with(SettingChange::Keys {
                action: "go.up".to_owned(),
                keys: Some(Vec::new()),
            });
        assert_eq!(bound.keys["go.back"], ["alt-left"]);
        assert_eq!(bound.keys["go.up"], Vec::<String>::new());
        let dropped = bound.with(SettingChange::Keys {
            action: "go.back".to_owned(),
            keys: None,
        });
        assert!(!dropped.keys.contains_key("go.back"));
        assert!(dropped.keys.contains_key("go.up"));
    }
}
