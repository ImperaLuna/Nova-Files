use std::path::{Path, PathBuf};
use std::sync::Arc;

use nova_files_api::{Gathered, GatheredFolder, Gathering, Holds};
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike, Value};

const PROJECTS: &str = "projects";
const GAMES: &str = "games";
const PATH: &str = "path";
const HOLDS: &str = "holds";
const NAME: &str = "name";
const MANY: &str = "many";
const ONE: &str = "one";
const HOME_PREFIX: &str = "~/";
const HOME_ALONE: &str = "~";

const fn key(place: Gathering) -> &'static str {
    match place {
        Gathering::Projects => PROJECTS,
        Gathering::Games => GAMES,
    }
}

pub fn read(text: &str, home: &Path) -> Result<(Gathered, Vec<String>), String> {
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| format!("not TOML: {error}"))?;
    let mut problems = Vec::new();
    for (name, _) in document.iter() {
        if name != PROJECTS && name != GAMES {
            problems.push(format!("unknown place `{name}`"));
        }
    }
    let mut place = |gathering| -> Arc<[GatheredFolder]> {
        let mut folders: Vec<GatheredFolder> = Vec::new();
        for (index, entry) in entries(&document, gathering, &mut problems) {
            let at = format!("`{}` entry {}", key(gathering), index + 1);
            let Some(entry) = entry else {
                problems.push(format!("{at}: is a table with a `{PATH}`"));
                continue;
            };
            if let Some(folder) = folder(entry, home, &at, &mut problems)
                && !folders.iter().any(|kept| kept.path == folder.path)
            {
                folders.push(folder);
            }
        }
        folders.into()
    };
    let gathered = Gathered {
        projects: place(Gathering::Projects),
        games: place(Gathering::Games),
    };
    Ok((gathered, problems))
}

fn entries<'a>(
    document: &'a DocumentMut,
    place: Gathering,
    problems: &mut Vec<String>,
) -> Vec<(usize, Option<&'a dyn TableLike>)> {
    let name = key(place);
    let tables: Vec<Option<&dyn TableLike>> = match document.get(name) {
        None => Vec::new(),
        Some(Item::ArrayOfTables(tables)) => tables.iter().map(|table| Some(table as _)).collect(),
        Some(Item::Value(Value::Array(array))) => array
            .iter()
            .map(|entry| entry.as_inline_table().map(|table| table as _))
            .collect(),
        Some(_) => {
            problems.push(format!("`{name}` is a list of folders"));
            Vec::new()
        }
    };
    tables.into_iter().enumerate().collect()
}

fn folder(
    entry: &dyn TableLike,
    home: &Path,
    at: &str,
    problems: &mut Vec<String>,
) -> Option<GatheredFolder> {
    for (name, _) in entry.iter() {
        if ![PATH, HOLDS, NAME].contains(&name) {
            problems.push(format!("{at}: unknown key `{name}`"));
        }
    }
    let Some(path) = entry.get(PATH).and_then(Item::as_str) else {
        problems.push(format!("{at}: `{PATH}` is a folder's path"));
        return None;
    };
    let Some(path) = expand(path, home) else {
        problems.push(format!("{at}: `{path}` starts with `/` or `~/`"));
        return None;
    };
    let holds = match entry.get(HOLDS) {
        None => Holds::Many,
        Some(item) => match item.as_str() {
            Some(MANY) => Holds::Many,
            Some(ONE) => Holds::One,
            _ => {
                problems.push(format!("{at}: `{HOLDS}` is \"{MANY}\" or \"{ONE}\""));
                return None;
            }
        },
    };
    let name = entry.get(NAME).and_then(|item| {
        let name = item.as_str().map(str::to_owned);
        if name.is_none() {
            problems.push(format!("{at}: `{NAME}` is text"));
        }
        name
    });
    Some(GatheredFolder { path, holds, name })
}

fn expand(text: &str, home: &Path) -> Option<PathBuf> {
    if text == HOME_ALONE {
        return Some(home.to_owned());
    }
    if let Some(inside) = text.strip_prefix(HOME_PREFIX) {
        return Some(home.join(inside.trim_start_matches('/')));
    }
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

fn written(path: &Path, home: &Path) -> Option<String> {
    match path.strip_prefix(home) {
        Ok(inside) if inside.as_os_str().is_empty() => Some(HOME_ALONE.to_owned()),
        Ok(inside) => inside
            .to_str()
            .map(|inside| format!("{HOME_PREFIX}{inside}")),
        Err(_) => path.to_str().map(str::to_owned),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    Changed(String),
    Unchanged,
    Refused(String),
}

pub fn add(
    text: &str,
    home: &Path,
    place: Gathering,
    folder: &GatheredFolder,
    at: Option<usize>,
) -> Edit {
    let mut document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return Edit::Refused(format!("not TOML: {error}")),
    };
    let positions = match kept_positions(&document, home, place) {
        Ok(positions) => positions,
        Err(problem) => return Edit::Refused(problem),
    };
    if positions.iter().any(|(_, path)| *path == folder.path) {
        return Edit::Unchanged;
    }
    if !folder.path.is_absolute() {
        return Edit::Refused(format!("{} isn't a full path", folder.path.display()));
    }
    let Some(path) = written(&folder.path, home) else {
        return Edit::Refused(format!(
            "{} isn't UTF-8 text, which the file can't hold",
            folder.path.display()
        ));
    };
    let mut entry = InlineTable::new();
    entry.insert(PATH, path.into());
    match folder.holds {
        Holds::Many => {}
        Holds::One => {
            entry.insert(HOLDS, ONE.into());
        }
    }
    if let Some(name) = &folder.name {
        entry.insert(NAME, name.as_str().into());
    }
    let index = at.and_then(|at| positions.get(at)).map(|&(index, _)| index);
    match document.get_mut(key(place)) {
        Some(Item::Value(Value::Array(array))) => {
            let index = index.unwrap_or(array.len());
            array.insert(index, entry);
        }
        Some(Item::ArrayOfTables(tables)) => insert_table(tables, index, entry.into_table()),
        Some(_) => return Edit::Refused(format!("{} isn't a list", key(place))),
        None => {
            let mut tables = ArrayOfTables::new();
            tables.push(entry.into_table());
            document.insert(key(place), Item::ArrayOfTables(tables));
        }
    }
    Edit::Changed(document.to_string())
}

pub fn remove(text: &str, home: &Path, place: Gathering, path: &Path) -> Edit {
    let mut document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return Edit::Refused(format!("not TOML: {error}")),
    };
    let names = |entry: &dyn TableLike| {
        entry
            .get(PATH)
            .and_then(Item::as_str)
            .and_then(|text| expand(text, home))
            .is_some_and(|named| named == path)
    };
    let removed = match document.get_mut(key(place)) {
        Some(Item::Value(Value::Array(array))) => {
            let before = array.len();
            array.retain(|entry| !entry.as_inline_table().is_some_and(|entry| names(entry)));
            before != array.len()
        }
        Some(Item::ArrayOfTables(tables)) => {
            let before = tables.len();
            tables.retain(|entry| !names(entry));
            before != tables.len()
        }
        _ => false,
    };
    if removed {
        Edit::Changed(document.to_string())
    } else {
        Edit::Unchanged
    }
}

pub fn new_file(home: &Path, folders: &[(Gathering, GatheredFolder)]) -> String {
    let mut text = String::new();
    for (place, folder) in folders {
        if let Edit::Changed(more) = add(&text, home, *place, folder, None) {
            text = more;
        }
    }
    text
}

fn kept_positions(
    document: &DocumentMut,
    home: &Path,
    place: Gathering,
) -> Result<Vec<(usize, PathBuf)>, String> {
    let mut problems = Vec::new();
    let found = entries(document, place, &mut problems);
    if let Some(problem) = problems.pop() {
        return Err(problem);
    }
    let mut kept: Vec<(usize, PathBuf)> = Vec::new();
    for (index, entry) in found {
        if let Some(entry) = entry
            && let Some(folder) = folder(entry, home, "", &mut Vec::new())
            && !kept.iter().any(|(_, path)| *path == folder.path)
        {
            kept.push((index, folder.path));
        }
    }
    Ok(kept)
}

fn insert_table(tables: &mut ArrayOfTables, index: Option<usize>, table: Table) {
    let index = index.map_or(tables.len(), |index| index.min(tables.len()));
    let mut after = Vec::new();
    while tables.len() > index {
        let last = tables.len() - 1;
        if let Some(moved) = tables.get(last) {
            after.push(moved.clone());
        }
        tables.remove(last);
    }
    tables.push(table);
    for moved in after.into_iter().rev() {
        tables.push(moved);
    }
}

#[cfg(test)]
mod tests;
