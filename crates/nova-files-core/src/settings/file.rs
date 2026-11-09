//! The settings file's text: reading settings out of it, and making one
//! change in it.
//!
//! The file is TOML that people edit by hand, so a mistake costs only the
//! value it is in: each known key is read on its own, and one with the
//! wrong type or an unknown value keeps its default and is reported.
//! Unknown keys are reported and left out. This is on purpose, unlike the
//! `deny_unknown_fields` rule for other files: one typo must not throw the
//! whole file away. Text that isn't TOML at all reads as all defaults.
//!
//! A change from the app edits the text in place (`toml_edit`), so the
//! user's comments, order and other keys stay as they were. A key the
//! file doesn't have is only added when its new value isn't the default.
//!
//! ```toml
//! appearance = "system"   # "system", "light" or "dark"
//! thumbnails = true
//!
//! [keys]                  # action id = keys, in GPUI's keystroke syntax
//! "go.back" = ["alt-left", "backspace"]
//! ```

use nova_files_api::{Appearance, SettingChange, Settings, Thumbnails};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

const APPEARANCE: &str = "appearance";
const THUMBNAILS: &str = "thumbnails";
const KEYS: &str = "keys";

const SYSTEM: &str = "system";
const LIGHT: &str = "light";
const DARK: &str = "dark";

/// The settings `text` holds, and what in it was left out and why.
pub fn read(text: &str) -> (Settings, Vec<String>) {
    let mut settings = Settings::default();
    let document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return (settings, vec![format!("not TOML, using defaults: {error}")]),
    };
    let mut problems = Vec::new();
    for (key, item) in document.iter() {
        match key {
            APPEARANCE => match item.as_str().and_then(appearance) {
                Some(appearance) => settings.appearance = appearance,
                None => problems.push(format!(
                    "`{APPEARANCE}` is \"{SYSTEM}\", \"{LIGHT}\" or \"{DARK}\""
                )),
            },
            THUMBNAILS => match item.as_bool() {
                Some(true) => settings.thumbnails = Thumbnails::Shown,
                Some(false) => settings.thumbnails = Thumbnails::Hidden,
                None => problems.push(format!("`{THUMBNAILS}` is true or false")),
            },
            KEYS => read_keys(item, &mut settings, &mut problems),
            unknown => problems.push(format!("unknown setting `{unknown}`")),
        }
    }
    (settings, problems)
}

/// The `[keys]` table's overrides, each read on its own.
fn read_keys(item: &Item, settings: &mut Settings, problems: &mut Vec<String>) {
    let Some(table) = item.as_table_like() else {
        problems.push(format!("`{KEYS}` is a table"));
        return;
    };
    for (action, keys) in table.iter() {
        if let Some(keys) = key_list(keys) {
            settings.keys.insert(action.to_owned(), keys);
        } else {
            problems.push(format!("`{KEYS}.{action}` is a list of key strings"));
        }
    }
}

/// `text` with `change` made, or `None` when `text` isn't TOML, so a file
/// the user is still fixing is never overwritten.
pub fn edit(text: &str, change: &SettingChange) -> Option<String> {
    let mut document = text.parse::<DocumentMut>().ok()?;
    let defaults = Settings::default();
    match change {
        SettingChange::Appearance(appearance) => set(
            &mut document,
            APPEARANCE,
            appearance_name(*appearance).into(),
            *appearance == defaults.appearance,
        ),
        SettingChange::Thumbnails(thumbnails) => set(
            &mut document,
            THUMBNAILS,
            (*thumbnails == Thumbnails::Shown).into(),
            *thumbnails == defaults.thumbnails,
        ),
        SettingChange::Keys {
            action,
            keys: Some(keys),
        } => {
            let list: Array = keys.iter().map(String::as_str).collect();
            if let Some(table) = keys_table(&mut document) {
                table.insert(action, Item::Value(Value::Array(list)));
            }
        }
        SettingChange::Keys { action, keys: None } => {
            if let Some(table) = document.get_mut(KEYS).and_then(Item::as_table_like_mut) {
                table.remove(action);
                if table.is_empty() {
                    document.remove(KEYS);
                }
            }
        }
    }
    Some(document.to_string())
}

/// Sets `key` to `value`, keeping the comment around it; leaves a missing
/// key out while the value is the default.
fn set(document: &mut DocumentMut, key: &str, value: Value, is_default: bool) {
    match document.get_mut(key) {
        Some(Item::Value(old)) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        Some(other) => *other = Item::Value(value),
        None if is_default => {}
        None => {
            document.insert(key, Item::Value(value));
        }
    }
}

/// The `[keys]` table, made when the file has none or has something else
/// under that name.
fn keys_table(document: &mut DocumentMut) -> Option<&mut dyn TableLike> {
    let item = document
        .entry(KEYS)
        .or_insert_with(|| Item::Table(Table::new()));
    if item.as_table_like().is_none() {
        *item = Item::Table(Table::new());
    }
    item.as_table_like_mut()
}

fn appearance(name: &str) -> Option<Appearance> {
    match name {
        SYSTEM => Some(Appearance::System),
        LIGHT => Some(Appearance::Light),
        DARK => Some(Appearance::Dark),
        _ => None,
    }
}

const fn appearance_name(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::System => SYSTEM,
        Appearance::Light => LIGHT,
        Appearance::Dark => DARK,
    }
}

/// A list of key strings, or `None` when `item` is anything else.
fn key_list(item: &Item) -> Option<Vec<String>> {
    item.as_array()?
        .iter()
        .map(|key| key.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests;
