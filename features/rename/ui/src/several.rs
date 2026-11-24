use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

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
    Ending { from: String, to: String },
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

fn split(name: &str, is_folder: bool) -> (&str, &str) {
    let stem_end = if is_folder {
        name.len()
    } else {
        stem(OsStr::new(name)).len()
    };
    name.split_at(stem_end)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    Letters,
    Digits,
    Other,
}

fn run_of(character: char) -> Run {
    if character.is_alphabetic() {
        Run::Letters
    } else if character.is_numeric() {
        Run::Digits
    } else {
        Run::Other
    }
}

fn words(text: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let run = run_of(first);
        let end = rest
            .char_indices()
            .find(|(_, character)| run_of(*character) != run)
            .map_or(rest.len(), |(at, _)| at);
        let (word, after) = rest.split_at(end);
        words.push(word);
        rest = after;
    }
    words
}

fn stem_change(original: &str, typed: &str) -> Change {
    let (was, now) = (words(original), words(typed));
    let front = was.iter().zip(&now).take_while(|(a, b)| a == b).count();
    let room = was.len().min(now.len()) - front;
    let back = was
        .iter()
        .rev()
        .zip(now.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    if front == 0 && back == 0 {
        return Change::Whole(typed.to_owned());
    }
    let gone = was[front..was.len() - back].concat();
    let added = now[front..now.len() - back].concat();
    if gone.is_empty() {
        Change::After {
            start: was[..front].concat(),
            added,
        }
    } else {
        Change::Swap {
            find: gone,
            with: added,
        }
    }
}

pub fn change(original: &str, typed: &str, is_folder: bool) -> Option<Change> {
    if original == typed {
        return None;
    }
    let (stem_was, ending_was) = split(original, is_folder);
    let (stem_now, ending_now) = split(typed, is_folder);
    if let Some(added) = typed.strip_suffix(original) {
        return Some(Change::Front(added.to_owned()));
    }
    if let Some(added) = typed.strip_prefix(original) {
        return Some(Change::End(added.to_owned()));
    }
    if stem_was == stem_now {
        return Some(Change::Ending {
            from: ending_was.to_owned(),
            to: ending_now.to_owned(),
        });
    }
    if ending_was != ending_now {
        return Some(Change::Whole(stem_now.to_owned()));
    }
    if let Some(added) = stem_now.strip_prefix(stem_was) {
        return Some(Change::BeforeEnding(added.to_owned()));
    }
    Some(stem_change(stem_was, stem_now))
}

fn swapped(text: &[u8], find: &[u8], with: &[u8]) -> Vec<u8> {
    let mut swapped = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(find).filter(|_| !find.is_empty()) {
            swapped.extend_from_slice(with);
            rest = after;
        } else {
            let (kept, after) = rest.split_at(1);
            swapped.extend_from_slice(kept);
            rest = after;
        }
    }
    swapped
}

pub fn changed(change: &Change, name: &OsStr, is_folder: bool, number: u32) -> OsString {
    let bytes = name.as_bytes();
    let stem_end = if is_folder {
        bytes.len()
    } else {
        stem(name).len()
    };
    let (stem, ending) = bytes.split_at(stem_end);
    let joined = |parts: &[&[u8]]| OsString::from_vec(parts.concat());
    match change {
        Change::Whole(new) => {
            let naming = if is_folder {
                Naming::Folder
            } else {
                Naming::File
            };
            numbered(&joined(&[new.as_bytes(), ending]), number, naming)
        }
        Change::Front(added) => joined(&[added.as_bytes(), bytes]),
        Change::End(added) => joined(&[bytes, added.as_bytes()]),
        Change::BeforeEnding(added) => joined(&[stem, added.as_bytes(), ending]),
        Change::After { start, added } => stem.strip_prefix(start.as_bytes()).map_or_else(
            || name.to_owned(),
            |rest| joined(&[start.as_bytes(), added.as_bytes(), rest, ending]),
        ),
        Change::Swap { find, with } => {
            joined(&[&swapped(stem, find.as_bytes(), with.as_bytes()), ending])
        }
        Change::Ending { from, to } if ending == from.as_bytes() => joined(&[stem, to.as_bytes()]),
        Change::Ending { .. } => name.to_owned(),
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
            change.as_ref().map_or_else(
                || other.name.to_owned(),
                |change| changed(change, other.name, other.is_folder, number),
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
        let emptied = planned("a1", "1", &["a"], &[]);
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

    #[test]
    fn text_added_in_front_counts_even_when_it_starts_like_the_name() {
        let others = ["plan.md"];
        let plan = planned("notes.txt", "new notes.txt", &others, &[]);

        assert_eq!(names(&plan, &others), ["new plan.md"]);
    }

    #[test]
    fn text_added_before_the_ending_may_start_with_a_dot() {
        let others = ["plan.md"];
        let plan = planned("notes.txt", "notes.old.txt", &others, &[]);

        assert_eq!(names(&plan, &others), ["plan.old.md"]);
    }

    #[test]
    fn a_new_name_that_shares_letters_with_the_old_one_is_still_whole() {
        let others = ["beach.jpg"];

        let italy = planned("holiday.jpg", "Italy.jpg", &others, &[]);
        let resume = planned("report.jpg", "resume.jpg", &others, &[]);

        assert_eq!(names(&italy, &others), ["Italy (2).jpg"]);
        assert_eq!(names(&resume, &others), ["resume (2).jpg"]);
    }

    #[test]
    fn a_changed_word_changes_as_a_word_and_never_in_the_ending() {
        let years = ["IMG_2023_0413.jpg", "IMG_2023_3333.jpg"];
        let year = planned("IMG_2023_0412.jpg", "IMG_2024_0412.jpg", &years, &[]);
        let endings = ["txt notes.txt"];
        let word = planned("txt list.txt", "text list.txt", &endings, &[]);

        assert_eq!(
            names(&year, &years),
            ["IMG_2024_0413.jpg", "IMG_2024_3333.jpg"]
        );
        assert_eq!(names(&word, &endings), ["text notes.txt"]);
    }

    #[test]
    fn a_changed_ending_changes_only_the_same_ending() {
        let others = ["b.jpeg", "c.png"];
        let plan = planned("a.jpeg", "a.jpg", &others, &[]);

        assert_eq!(names(&plan, &others), ["b.jpg", "c.png"]);
    }

    #[test]
    fn bytes_that_are_not_text_stay_as_they_were() {
        let odd = OsStr::from_bytes(b"a\xFFa 1");
        let others = [Named {
            name: odd,
            is_folder: false,
        }];
        let plan = plan(file("a 1"), "b 1", &others, &HashSet::new());

        assert_eq!(plan.others[odd].name, OsStr::from_bytes(b"b\xFFb 1"));
    }
}
