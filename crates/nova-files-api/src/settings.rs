//! The user's settings, as the core keeps them and every window applies
//! them.
//!
//! The core reads them from its settings file and sends the whole set as
//! [`crate::Event::Settings`]: when a client asks, and from then on to that
//! client each time a value changes, whoever changed it. A client changes one value at
//! a time with [`crate::Command::ChangeSetting`], so two windows changing
//! different values never undo each other.

use std::collections::BTreeMap;

/// Every setting, each with its default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// Light or dark, or whichever the desktop prefers.
    pub appearance: Appearance,
    /// Whether rows show thumbnails of pictures and videos.
    pub thumbnails: Thumbnails,
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

/// One setting's new value, sent with [`crate::Command::ChangeSetting`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Appearance(Appearance),
    Thumbnails(Thumbnails),
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
