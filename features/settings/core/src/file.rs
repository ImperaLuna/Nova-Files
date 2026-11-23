use nova_files_api::{FolderSizes, HiddenFiles};
use nova_files_settings_api::{Appearance, SettingChange, Settings, Thumbnails};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

const APPEARANCE: &str = "appearance";
const THUMBNAILS: &str = "thumbnails";
const HIDDEN_FILES: &str = "hidden_files";
const FOLDER_SIZES: &str = "folder_sizes";
const KEYS: &str = "keys";
const WIDTHS: &str = "widths";

const SYSTEM: &str = "system";
const LIGHT: &str = "light";
const DARK: &str = "dark";

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
            HIDDEN_FILES => match item.as_bool() {
                Some(true) => settings.hidden_files = HiddenFiles::Shown,
                Some(false) => settings.hidden_files = HiddenFiles::Left,
                None => problems.push(format!("`{HIDDEN_FILES}` is true or false")),
            },
            FOLDER_SIZES => match item.as_bool() {
                Some(true) => settings.folder_sizes = FolderSizes::Measured,
                Some(false) => settings.folder_sizes = FolderSizes::Left,
                None => problems.push(format!("`{FOLDER_SIZES}` is true or false")),
            },
            KEYS => read_keys(item, &mut settings, &mut problems),
            WIDTHS => read_widths(item, &mut settings, &mut problems),
            unknown => problems.push(format!("unknown setting `{unknown}`")),
        }
    }
    (settings, problems)
}

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

fn read_widths(item: &Item, settings: &mut Settings, problems: &mut Vec<String>) {
    let Some(table) = item.as_table_like() else {
        problems.push(format!("`{WIDTHS}` is a table"));
        return;
    };
    for (name, width) in table.iter() {
        match width
            .as_integer()
            .and_then(|width| u16::try_from(width).ok())
        {
            Some(width) => {
                settings.widths.insert(name.to_owned(), width);
            }
            None => problems.push(format!("`{WIDTHS}.{name}` is a width in pixels")),
        }
    }
}

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
        SettingChange::HiddenFiles(hidden) => set(
            &mut document,
            HIDDEN_FILES,
            (*hidden == HiddenFiles::Shown).into(),
            *hidden == defaults.hidden_files,
        ),
        SettingChange::FolderSizes(sizes) => set(
            &mut document,
            FOLDER_SIZES,
            (*sizes == FolderSizes::Measured).into(),
            *sizes == defaults.folder_sizes,
        ),
        SettingChange::Keys {
            action,
            keys: Some(keys),
        } => {
            let list: Array = keys.iter().map(String::as_str).collect();
            if let Some(table) = table_at(&mut document, KEYS) {
                table.insert(action, Item::Value(Value::Array(list)));
            }
        }
        SettingChange::Keys { action, keys: None } => drop_from(&mut document, KEYS, action),
        SettingChange::Width {
            name,
            width: Some(width),
        } => {
            if let Some(table) = table_at(&mut document, WIDTHS) {
                table.insert(name, Item::Value(i64::from(*width).into()));
            }
        }
        SettingChange::Width { name, width: None } => drop_from(&mut document, WIDTHS, name),
    }
    Some(document.to_string())
}

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

fn drop_from(document: &mut DocumentMut, table: &str, key: &str) {
    if let Some(entries) = document.get_mut(table).and_then(Item::as_table_like_mut) {
        entries.remove(key);
        if entries.is_empty() {
            document.remove(table);
        }
    }
}

fn table_at<'a>(document: &'a mut DocumentMut, key: &str) -> Option<&'a mut dyn TableLike> {
    let item = document
        .entry(key)
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

fn key_list(item: &Item) -> Option<Vec<String>> {
    item.as_array()?
        .iter()
        .map(|key| key.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests;
