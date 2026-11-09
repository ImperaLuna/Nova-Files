//! The file picker portal's options and results, as plain types.
//!
//! xdg-desktop-portal passes an app's request as a map of named values
//! (`multiple`, `directory`, `filters`, `current_filter`, `current_name`,
//! `current_folder`, `current_file`, `accept_label`) and takes the answer
//! as a response code plus a map (`uris`, `current_filter`). A value of the
//! wrong type counts as missing, as the portal's own backends treat it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;

use nova_files_api::{FileFilter, FilterRule, ParentWindow, PickAnswer, PickKind, uri};
use zbus::zvariant::{OwnedValue, Value};

/// The portal's response codes.
pub const SUCCESS: u32 = 0;
pub const CANCELLED: u32 = 1;
pub const FAILED: u32 = 2;

/// A filter as the portal sends it: a name and `(kind, pattern)` rules.
type WireFilter = (String, Vec<(u32, String)>);

/// The rule kinds in a [`WireFilter`].
const GLOB: u32 = 0;
const MIME_TYPE: u32 = 1;

pub type Options = HashMap<String, OwnedValue>;

/// The parts of a request that come from its options.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub title: String,
    pub kind: PickKind,
    pub accept_label: Option<String>,
    pub filters: Vec<FileFilter>,
    pub filter: Option<usize>,
    pub folder: Option<PathBuf>,
    /// The app's window, which comes beside the options.
    pub parent: Option<ParentWindow>,
}

/// An `OpenFile` request's title and options.
pub fn open(title: String, options: &Options) -> Parsed {
    let kind = PickKind::Open {
        multiple: get(options, "multiple").unwrap_or(false),
        folders: get(options, "directory").unwrap_or(false),
    };
    parsed(title, options, kind, get_path(options, "current_folder"))
}

/// A `SaveFile` request's title and options. A `current_file` names both
/// the folder and the name; otherwise they come from `current_folder` and
/// `current_name`.
pub fn save(title: String, options: &Options) -> Parsed {
    let file = get_path(options, "current_file");
    let name = file
        .as_ref()
        .and_then(|file| file.file_name())
        .map(ToOwned::to_owned)
        .or_else(|| get::<String>(options, "current_name").map(OsString::from))
        .unwrap_or_default();
    let folder = file
        .as_ref()
        .and_then(|file| file.parent())
        .map(PathBuf::from)
        .or_else(|| get_path(options, "current_folder"));
    parsed(title, options, PickKind::Save { name }, folder)
}

fn parsed(title: String, options: &Options, kind: PickKind, folder: Option<PathBuf>) -> Parsed {
    let mut filters: Vec<FileFilter> = get::<Vec<WireFilter>>(options, "filters")
        .unwrap_or_default()
        .into_iter()
        .map(filter_from_wire)
        .collect();
    // The current filter should be one of the list; one that isn't is
    // added, so it can still be shown and chosen.
    let filter = get::<WireFilter>(options, "current_filter")
        .map(filter_from_wire)
        .map(|current| {
            filters
                .iter()
                .position(|filter| *filter == current)
                .unwrap_or_else(|| {
                    filters.push(current);
                    filters.len() - 1
                })
        });
    Parsed {
        title,
        kind,
        accept_label: get(options, "accept_label"),
        filters,
        filter,
        folder,
        parent: None,
    }
}

/// The response code and results for `answer`.
pub fn results(answer: &PickAnswer, filters: &[FileFilter]) -> (u32, Options) {
    let PickAnswer::Picked { paths, filter } = answer else {
        return (CANCELLED, Options::new());
    };
    // Nothing picked is no answer an app can use.
    if paths.is_empty() {
        return (CANCELLED, Options::new());
    }
    let uris: Vec<String> = paths.iter().map(|path| uri::file_uri(path)).collect();
    let mut results = Options::new();
    if let Some(uris) = owned(Value::from(uris)) {
        results.insert("uris".to_owned(), uris);
    }
    let current = filter
        .and_then(|index| filters.get(index))
        .and_then(|filter| owned(Value::from(filter_to_wire(filter))));
    if let Some(current) = current {
        results.insert("current_filter".to_owned(), current);
    }
    (SUCCESS, results)
}

fn filter_from_wire((name, rules): WireFilter) -> FileFilter {
    let rules = rules
        .into_iter()
        .filter_map(|(kind, pattern)| match kind {
            GLOB => Some(FilterRule::Glob(pattern)),
            MIME_TYPE => Some(FilterRule::MimeType(pattern)),
            _ => None,
        })
        .collect();
    FileFilter { name, rules }
}

fn filter_to_wire(filter: &FileFilter) -> WireFilter {
    let rules = filter
        .rules
        .iter()
        .map(|rule| match rule {
            FilterRule::Glob(pattern) => (GLOB, pattern.clone()),
            FilterRule::MimeType(pattern) => (MIME_TYPE, pattern.clone()),
        })
        .collect();
    (filter.name.clone(), rules)
}

/// The option `key` as a `T`, or `None` when it is missing or of another
/// type.
fn get<T: TryFrom<OwnedValue>>(options: &Options, key: &str) -> Option<T> {
    T::try_from(options.get(key)?.try_clone().ok()?).ok()
}

/// A path option: bytes ending in a NUL, as the portal sends paths. Only
/// an absolute path counts; anything else is relative to nothing here.
fn get_path(options: &Options, key: &str) -> Option<PathBuf> {
    let mut bytes: Vec<u8> = get(options, key)?;
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    Some(PathBuf::from(OsString::from_vec(bytes))).filter(|path| path.is_absolute())
}

fn owned(value: Value<'_>) -> Option<OwnedValue> {
    OwnedValue::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::PathBuf;

    use nova_files_api::{FileFilter, FilterRule, PickAnswer, PickKind};
    use zbus::zvariant::{OwnedValue, Value};

    use super::{CANCELLED, Options, SUCCESS, open, results, save};

    fn options(pairs: Vec<(&str, Value<'_>)>) -> Options {
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_owned(), OwnedValue::try_from(value).unwrap()))
            .collect()
    }

    fn pdf() -> FileFilter {
        FileFilter {
            name: "PDF".to_owned(),
            rules: vec![
                FilterRule::Glob("*.pdf".to_owned()),
                FilterRule::MimeType("application/pdf".to_owned()),
            ],
        }
    }

    fn wire_pdf() -> (String, Vec<(u32, String)>) {
        (
            "PDF".to_owned(),
            vec![(0, "*.pdf".to_owned()), (1, "application/pdf".to_owned())],
        )
    }

    fn path_bytes(path: &str) -> Vec<u8> {
        let mut bytes = path.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }

    #[test]
    fn an_empty_open_asks_for_one_file_from_home() {
        let parsed = open("Open".to_owned(), &Options::new());
        assert_eq!(
            parsed.kind,
            PickKind::Open {
                multiple: false,
                folders: false
            }
        );
        assert_eq!(parsed.folder, None);
        assert_eq!(parsed.filters, []);
        assert_eq!(parsed.filter, None);
        assert_eq!(parsed.accept_label, None);
        assert_eq!(parsed.title, "Open");
    }

    #[test]
    fn open_reads_every_option() {
        let parsed = open(
            String::new(),
            &options(vec![
                ("multiple", Value::from(true)),
                ("directory", Value::from(true)),
                ("accept_label", Value::from("Upload")),
                ("filters", Value::from(vec![wire_pdf()])),
                ("current_filter", Value::from(wire_pdf())),
                ("current_folder", Value::from(path_bytes("/srv/docs"))),
            ]),
        );
        assert_eq!(
            parsed.kind,
            PickKind::Open {
                multiple: true,
                folders: true
            }
        );
        assert_eq!(parsed.accept_label.as_deref(), Some("Upload"));
        assert_eq!(parsed.filters, [pdf()]);
        assert_eq!(parsed.filter, Some(0));
        assert_eq!(parsed.folder, Some(PathBuf::from("/srv/docs")));
    }

    #[test]
    fn a_current_filter_not_in_the_list_is_added() {
        let image = ("Images".to_owned(), vec![(1_u32, "image/*".to_owned())]);
        let parsed = open(
            String::new(),
            &options(vec![
                ("filters", Value::from(vec![wire_pdf()])),
                ("current_filter", Value::from(image)),
            ]),
        );
        assert_eq!(parsed.filters.len(), 2);
        assert_eq!(parsed.filter, Some(1));
        assert_eq!(
            parsed.filters[1].rules,
            [FilterRule::MimeType("image/*".to_owned())]
        );
    }

    #[test]
    fn unknown_rule_kinds_and_wrong_types_are_left_out() {
        let odd = (
            "Odd".to_owned(),
            vec![(7_u32, "x".to_owned()), (0, "*.x".to_owned())],
        );
        let parsed = open(
            String::new(),
            &options(vec![
                ("filters", Value::from(vec![odd])),
                ("multiple", Value::from("yes")),
            ]),
        );
        assert_eq!(
            parsed.filters[0].rules,
            [FilterRule::Glob("*.x".to_owned())]
        );
        assert_eq!(
            parsed.kind,
            PickKind::Open {
                multiple: false,
                folders: false
            }
        );
    }

    #[test]
    fn save_takes_the_name_and_folder() {
        let parsed = save(
            String::new(),
            &options(vec![
                ("current_name", Value::from("invoice.pdf")),
                ("current_folder", Value::from(path_bytes("/srv/docs"))),
            ]),
        );
        assert_eq!(
            parsed.kind,
            PickKind::Save {
                name: "invoice.pdf".into()
            }
        );
        assert_eq!(parsed.folder, Some(PathBuf::from("/srv/docs")));
    }

    #[test]
    fn save_over_a_current_file_takes_both_from_it() {
        let parsed = save(
            String::new(),
            &options(vec![
                ("current_name", Value::from("ignored.txt")),
                ("current_folder", Value::from(path_bytes("/ignored"))),
                (
                    "current_file",
                    Value::from(path_bytes("/srv/docs/report.odt")),
                ),
            ]),
        );
        assert_eq!(
            parsed.kind,
            PickKind::Save {
                name: "report.odt".into()
            }
        );
        assert_eq!(parsed.folder, Some(PathBuf::from("/srv/docs")));
    }

    #[test]
    fn a_save_keeps_a_name_that_is_not_utf8() {
        let mut file = b"/srv/caf\xe9.txt".to_vec();
        file.push(0);
        let parsed = save(
            String::new(),
            &options(vec![("current_file", Value::from(file))]),
        );
        assert_eq!(
            parsed.kind,
            PickKind::Save {
                name: OsString::from_vec(b"caf\xe9.txt".to_vec())
            }
        );
    }

    #[test]
    fn an_empty_path_counts_as_none() {
        let parsed = save(
            String::new(),
            &options(vec![("current_folder", Value::from(vec![0_u8]))]),
        );
        assert_eq!(parsed.folder, None);
        assert_eq!(
            parsed.kind,
            PickKind::Save {
                name: OsString::new()
            }
        );
    }

    #[test]
    fn a_pick_answers_uris_and_the_filter_in_use() {
        let answer = PickAnswer::Picked {
            paths: vec![PathBuf::from("/srv/a b.pdf"), PathBuf::from("/srv/c.pdf")],
            filter: Some(0),
        };
        let (code, results) = results(&answer, &[pdf()]);
        assert_eq!(code, SUCCESS);
        let uris: Vec<String> = results["uris"].try_clone().unwrap().try_into().unwrap();
        assert_eq!(uris, ["file:///srv/a%20b.pdf", "file:///srv/c.pdf"]);
        let filter: (String, Vec<(u32, String)>) = results["current_filter"]
            .try_clone()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(filter, wire_pdf());
    }

    #[test]
    fn a_pick_without_a_filter_answers_only_uris() {
        let answer = PickAnswer::Picked {
            paths: vec![PathBuf::from("/srv/a")],
            filter: None,
        };
        let (code, results) = results(&answer, &[]);
        assert_eq!(code, SUCCESS);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn a_relative_current_file_gives_no_folder() {
        let parsed = save(
            String::new(),
            &options(vec![
                ("current_folder", Value::from(path_bytes("/srv/docs"))),
                ("current_file", Value::from(path_bytes("report.odt"))),
            ]),
        );
        assert_eq!(parsed.folder, Some(PathBuf::from("/srv/docs")));
    }

    #[test]
    fn picking_nothing_answers_cancelled() {
        let answer = PickAnswer::Picked {
            paths: Vec::new(),
            filter: None,
        };
        assert_eq!(results(&answer, &[]).0, CANCELLED);
    }

    #[test]
    fn a_cancel_answers_cancelled_and_nothing_else() {
        let (code, results) = results(&PickAnswer::Cancelled, &[pdf()]);
        assert_eq!(code, CANCELLED);
        assert!(results.is_empty());
    }
}
