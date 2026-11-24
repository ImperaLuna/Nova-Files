use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};

use nova_files_api::numbered::{Naming, numbered, stem};
use nova_files_rename_api::is_plain_name;

use crate::name::renamed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Whole(String),
    Front(String),
    End(String),
    BeforeEnding(String),
    After { start: String, added: String },
    Swap { find: String, with: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Named<'a> {
    pub name: &'a OsStr,
    pub is_folder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub name: OsString,
    pub clashes: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    pub others: HashMap<OsString, Planned>,
    pub clashes: bool,
}

fn stem_len(name: &str, is_folder: bool) -> usize {
    if is_folder {
        name.len()
    } else {
        stem(OsStr::new(name)).len()
    }
}

fn shared(original: &str, typed: &str) -> (usize, usize) {
    let front: usize = original
        .chars()
        .zip(typed.chars())
        .take_while(|(was, now)| was == now)
        .map(|(was, _)| was.len_utf8())
        .sum();
    let back = original[front..]
        .chars()
        .rev()
        .zip(typed[front..].chars().rev())
        .take_while(|(was, now)| was == now)
        .map(|(was, _)| was.len_utf8())
        .sum();
    (front, back)
}

pub fn change(original: &str, typed: &str, is_folder: bool) -> Option<Change> {
    if original == typed {
        return None;
    }
    let (front, back) = shared(original, typed);
    let gone = &original[front..original.len() - back];
    let added = &typed[front..typed.len() - back];
    let stem_end = stem_len(original, is_folder);
    let whole = front == 0 && !gone.is_empty() && original.len() - back >= stem_end;
    Some(if whole {
        Change::Whole(typed[..stem_len(typed, is_folder)].to_owned())
    } else if !gone.is_empty() {
        Change::Swap {
            find: gone.to_owned(),
            with: added.to_owned(),
        }
    } else if front == 0 {
        Change::Front(added.to_owned())
    } else if front == original.len() {
        Change::End(added.to_owned())
    } else if front == stem_end {
        Change::BeforeEnding(added.to_owned())
    } else {
        Change::After {
            start: original[..front].to_owned(),
            added: added.to_owned(),
        }
    })
}

pub fn changed(change: &Change, name: &str, is_folder: bool, number: u32) -> String {
    let at = |at: usize, added: &str| format!("{}{added}{}", &name[..at], &name[at..]);
    match change {
        Change::Whole(new) => {
            let first = format!("{new}{}", &name[stem_len(name, is_folder)..]);
            let naming = if is_folder {
                Naming::Folder
            } else {
                Naming::File
            };
            numbered(OsStr::new(&first), number, naming)
                .to_string_lossy()
                .into_owned()
        }
        Change::Front(added) => at(0, added),
        Change::End(added) => at(name.len(), added),
        Change::BeforeEnding(added) => at(stem_len(name, is_folder), added),
        Change::After { start, added } if name.starts_with(start.as_str()) => {
            at(start.len(), added)
        }
        Change::After { .. } => name.to_owned(),
        Change::Swap { find, with } => name.replace(find.as_str(), with),
    }
}

pub const FIRST_NUMBER: u32 = 2;

pub fn plan(
    edited: Named<'_>,
    typed: &str,
    others: &[Named<'_>],
    unselected: &HashSet<&OsStr>,
) -> Plan {
    let original = edited.name.to_string_lossy();
    let change = change(&original, typed, edited.is_folder);
    let edited_name = renamed(edited.name, typed);
    let new_names: Vec<OsString> = others
        .iter()
        .zip(FIRST_NUMBER..)
        .map(|(other, number)| {
            let shown = other.name.to_string_lossy();
            change.as_ref().map_or_else(
                || other.name.to_owned(),
                |change| {
                    renamed(
                        other.name,
                        &changed(change, &shown, other.is_folder, number),
                    )
                },
            )
        })
        .collect();
    let mut times: HashMap<&OsStr, usize> = HashMap::new();
    for name in new_names.iter().chain([&edited_name]) {
        *times.entry(name.as_os_str()).or_default() += 1;
    }
    let clashing = |name: &OsStr| {
        !is_plain_name(name) || unselected.contains(name) || times.get(name) != Some(&1)
    };
    let planned: HashMap<OsString, Planned> = others
        .iter()
        .zip(&new_names)
        .filter(|(other, name)| other.name != name.as_os_str() || clashing(name))
        .map(|(other, name)| {
            let planned = Planned {
                name: name.clone(),
                clashes: clashing(name),
            };
            (other.name.to_owned(), planned)
        })
        .collect();
    Plan {
        clashes: clashing(&edited_name) || planned.values().any(|planned| planned.clashes),
        others: planned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> Named<'_> {
        Named {
            name: OsStr::new(name),
            is_folder: false,
        }
    }

    fn names(plan: &Plan, others: &[&str]) -> Vec<String> {
        others
            .iter()
            .map(|other| {
                plan.others.get(OsStr::new(other)).map_or_else(
                    || (*other).to_owned(),
                    |planned| planned.name.to_string_lossy().into_owned(),
                )
            })
            .collect()
    }

    fn planned(edited: &str, typed: &str, others: &[&str], unselected: &[&str]) -> Plan {
        let others: Vec<Named<'_>> = others.iter().map(|name| file(name)).collect();
        let unselected = unselected.iter().map(OsStr::new).collect();
        plan(file(edited), typed, &others, &unselected)
    }

    #[test]
    fn a_changed_part_changes_in_every_name_that_has_it() {
        let others = ["IMG_0413.jpg", "IMG_0414_IMG_.jpg", "notes.txt"];
        let plan = planned("IMG_0412.jpg", "Rome 0412.jpg", &others, &[]);

        assert_eq!(
            names(&plan, &others),
            ["Rome 0413.jpg", "Rome 0414_Rome .jpg", "notes.txt"]
        );
        assert!(!plan.clashes);
        assert!(!plan.others.contains_key(OsStr::new("notes.txt")));
    }

    #[test]
    fn text_added_at_an_end_goes_to_the_same_end_of_every_name() {
        let others = ["plan.md", "backup.tar.gz", "LICENSE"];

        let front = planned("notes.txt", "old notes.txt", &others, &[]);
        let stem_end = planned("notes.txt", "notes draft.txt", &others, &[]);
        let end = planned("notes.txt", "notes.txt.bak", &others, &[]);

        assert_eq!(
            names(&front, &others),
            ["old plan.md", "old backup.tar.gz", "old LICENSE"]
        );
        assert_eq!(
            names(&stem_end, &others),
            ["plan draft.md", "backup draft.tar.gz", "LICENSE draft"]
        );
        assert_eq!(
            names(&end, &others),
            ["plan.md.bak", "backup.tar.gz.bak", "LICENSE.bak"]
        );
    }

    #[test]
    fn text_added_inside_goes_after_the_same_start() {
        let others = ["IMG_0413.jpg", "DSC_0001.jpg"];
        let plan = planned("IMG_0412.jpg", "IMG_x0412.jpg", &others, &[]);

        assert_eq!(names(&plan, &others), ["IMG_x0413.jpg", "DSC_0001.jpg"]);
    }

    #[test]
    fn a_whole_new_name_numbers_the_others_and_keeps_their_endings() {
        let others = ["b.png", "c.tar.gz"];
        let plan = planned("a.jpg", "Rome.jpg", &others, &[]);

        assert_eq!(names(&plan, &others), ["Rome (2).png", "Rome (3).tar.gz"]);
        assert!(!plan.clashes);
    }

    #[test]
    fn folders_count_their_whole_name_as_the_part_before_the_ending() {
        let folder = |name: &'static str| Named {
            name: OsStr::new(name),
            is_folder: true,
        };
        let plan = plan(
            folder("v1.2"),
            "v1.2 old",
            &[folder("v1.3")],
            &HashSet::new(),
        );

        assert_eq!(names(&plan, &["v1.3"]), ["v1.3 old"]);
    }

    #[test]
    fn names_that_meet_or_are_taken_or_leave_the_folder_clash() {
        let same = planned("a1.txt", "a.txt", &["a11.txt"], &[]);
        let met = planned("x1", "y1", &["x2"], &["y2"]);
        let emptied = planned("ab", "b", &["a"], &[]);
        let free = planned("x1", "y1", &["x2"], &["y3"]);

        assert!(same.clashes);
        assert!(met.clashes && met.others[OsStr::new("x2")].clashes);
        assert!(emptied.clashes && emptied.others[OsStr::new("a")].clashes);
        assert!(!free.clashes);
    }

    #[test]
    fn an_untouched_name_plans_nothing() {
        let plan = planned("a.txt", "a.txt", &["b.txt"], &[]);

        assert_eq!(plan, Plan::default());
    }

    #[test]
    fn letters_of_several_bytes_stay_whole() {
        let others = ["café 2.txt"];
        let plan = planned("café 1.txt", "caffè 1.txt", &others, &[]);

        assert_eq!(names(&plan, &others), ["caffè 2.txt"]);
    }
}
