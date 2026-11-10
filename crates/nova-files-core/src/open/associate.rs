//! Making an app the user's default for a MIME type, the way `gio mime`
//! does: in `$XDG_CONFIG_HOME/mimeapps.list`, the app becomes the type's
//! `Default Applications` entry, goes first in its `Added Associations`,
//! and leaves its `Removed Associations`. A desktop's own list in the
//! config home (`gnome-mimeapps.list`) is read before that one, so where
//! it names a default for the type or removes the app, it gets the same
//! change, or the new default wouldn't show.
//!
//! A file is edited as text: only the lines for that type change, and
//! every other line, comment and group stays as it was. A group may appear
//! more than once; the first line for the type in any of them is the one
//! that counts, as the reader takes it, and the app leaves every removal.
//! Each file is saved whole through a temporary file with its own mode,
//! and through a symlink to where it points. The association lists are
//! read fresh for every request, so the next Open With list shows the
//! change.

use std::io;
use std::path::Path;

use super::defaults::{
    ADDED, DEFAULTS, DESKTOP_LIST_SUFFIX, ID_SEPARATOR, LIST, MAX_LIST_BYTES, Places, REMOVED,
    installed,
};
use crate::private_file::{self, Read};

const GROUP_START: char = '[';
const GROUP_END: char = ']';
const KEY_END: char = '=';
const TYPE_SEPARATOR: char = '/';
/// What a MIME type's two halves may hold besides letters and digits
/// (RFC 6838 "restricted-name").
const TYPE_PUNCTUATION: &str = "!#$&-^_.+";
/// What a desktop ID ends in.
const DESKTOP_SUFFIX: &str = ".desktop";

/// Why the default wasn't set.
#[derive(Debug)]
pub enum NotSet {
    /// Not a MIME type, or not an installed app's desktop ID.
    Invalid,
    /// No config home to save it in.
    NoHome,
    /// A list couldn't be read or saved.
    Failed(io::Error),
}

/// Makes the app with desktop ID `app` the default for `mime`.
///
/// # Errors
/// [`NotSet`] says why nothing changed.
pub fn set_default(mime: &str, app: &str, places: &Places) -> Result<(), NotSet> {
    if !valid_type(mime) || !valid_id(app) || installed(app, places).is_none() {
        return Err(NotSet::Invalid);
    }
    let home = places.config_home.as_deref().ok_or(NotSet::NoHome)?;
    for desktop in &places.desktops {
        let list = home.join(format!("{desktop}{DESKTOP_LIST_SUFFIX}"));
        save(&list, mime, app, Only::WhereItOverrides)?;
    }
    save(&home.join(LIST), mime, app, Only::Always)
}

/// Which lists [`save`] changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Only {
    Always,
    /// Only a list that names a default for the type or removes the app.
    WhereItOverrides,
}

fn save(path: &Path, mime: &str, app: &str, only: Only) -> Result<(), NotSet> {
    let target = private_file::saved_at(path);
    let max = usize::try_from(MAX_LIST_BYTES).unwrap_or(usize::MAX);
    let text = match private_file::read(&target, max) {
        Read::Bytes(bytes) => String::from_utf8(bytes)
            .map_err(|_| NotSet::Failed(io::Error::from(io::ErrorKind::InvalidData)))?,
        Read::Missing => String::new(),
        Read::TooBig => return Err(NotSet::Failed(io::Error::from(io::ErrorKind::FileTooLarge))),
        Read::Failed(error) => return Err(NotSet::Failed(error)),
    };
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let overrides = key_line(&lines, DEFAULTS, mime).is_some()
        || values(&lines, REMOVED, mime).iter().any(|id| id == app);
    if only == Only::WhereItOverrides && !overrides {
        return Ok(());
    }
    let edited = edit(&text, mime, app);
    if edited == text {
        return Ok(());
    }
    let mode = private_file::mode_to_keep(&target);
    private_file::replace(&target, edited.as_bytes(), mode).map_err(NotSet::Failed)
}

/// `text` with `app` made the default for `mime`.
fn edit(text: &str, mime: &str, app: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    set(&mut lines, DEFAULTS, mime, &[app]);
    let added = values(&lines, ADDED, mime);
    let added: Vec<&str> = std::iter::once(app)
        .chain(added.iter().map(String::as_str).filter(|id| *id != app))
        .collect();
    set(&mut lines, ADDED, mime, &added);
    // Each pass takes the app out of one removal line; there are at most
    // as many as there are lines.
    for _ in 0..lines.len() {
        let Some(index) = removal_of(&lines, mime, app) else {
            break;
        };
        let kept: Vec<String> = ids_on(&lines[index])
            .into_iter()
            .filter(|id| id != app)
            .collect();
        if kept.is_empty() {
            lines.remove(index);
        } else {
            lines[index] = line_for(mime, &kept.iter().map(String::as_str).collect::<Vec<_>>());
        }
    }
    let mut edited = lines.join("\n");
    if !edited.is_empty() {
        edited.push('\n');
    }
    edited
}

/// The group a header line opens, if it is one.
fn header(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix(GROUP_START)
        .and_then(|line| line.strip_suffix(GROUP_END))
}

/// Whether `line` sets `key`.
fn sets(line: &str, key: &str) -> bool {
    line.split_once(KEY_END)
        .is_some_and(|(name, _)| name.trim() == key)
}

/// The IDs a key line lists.
fn ids_on(line: &str) -> Vec<String> {
    line.split_once(KEY_END)
        .map(|(_, ids)| {
            ids.split(ID_SEPARATOR)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The line setting `key` to `ids`.
fn line_for(key: &str, ids: &[&str]) -> String {
    let mut line = format!("{key}{KEY_END}{}", ids.join(&ID_SEPARATOR.to_string()));
    line.push(ID_SEPARATOR);
    line
}

/// Each run of `group`'s lines, header included, in file order.
fn groups(lines: &[String], group: &str) -> Vec<(usize, usize)> {
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| header(line).is_some())
        .map(|(index, _)| index)
        .chain([lines.len()])
        .collect();
    starts
        .iter()
        .zip(starts.iter().skip(1))
        .filter(|&(&start, _)| header(&lines[start]) == Some(group))
        .map(|(&start, &end)| (start, end))
        .collect()
}

/// The first line setting `key` in any run of `group`.
fn key_line(lines: &[String], group: &str, key: &str) -> Option<usize> {
    groups(lines, group)
        .into_iter()
        .find_map(|(start, end)| (start..end).skip(1).find(|&index| sets(&lines[index], key)))
}

/// The IDs `group` lists for `key`.
fn values(lines: &[String], group: &str, key: &str) -> Vec<String> {
    key_line(lines, group, key).map_or_else(Vec::new, |index| ids_on(&lines[index]))
}

/// A removal line for `key` that lists `app`, in any run of the group.
fn removal_of(lines: &[String], key: &str, app: &str) -> Option<usize> {
    groups(lines, REMOVED).into_iter().find_map(|(start, end)| {
        (start..end).skip(1).find(|&index| {
            sets(&lines[index], key) && ids_on(&lines[index]).iter().any(|id| id == app)
        })
    })
}

/// Makes `group` set `key` to `ids`: the line that counts is changed in
/// place, or one is added after the first run's last line that isn't
/// blank; a missing group is added at the end.
fn set(lines: &mut Vec<String>, group: &str, key: &str, ids: &[&str]) {
    let value = line_for(key, ids);
    if let Some(index) = key_line(lines, group, key) {
        lines[index] = value;
        return;
    }
    let Some(&(start, end)) = groups(lines, group).first() else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.extend([format!("{GROUP_START}{group}{GROUP_END}"), value]);
        return;
    };
    let last = (start..end)
        .rev()
        .find(|&index| !lines[index].trim().is_empty())
        .unwrap_or(start);
    lines.insert(last + 1, value);
}

/// Whether `mime` is a MIME type (`type/subtype`), so it can't break the
/// file's lines or keys.
fn valid_type(mime: &str) -> bool {
    let half = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || TYPE_PUNCTUATION.contains(c))
    };
    mime.split_once(TYPE_SEPARATOR)
        .is_some_and(|(kind, sub)| half(kind) && half(sub))
}

/// Whether `id` is a desktop ID that can't break the file's lines or
/// lists.
fn valid_id(id: &str) -> bool {
    id.ends_with(DESKTOP_SUFFIX)
        && !id.chars().any(|c| {
            c.is_whitespace() || c.is_control() || [ID_SEPARATOR, KEY_END, GROUP_START].contains(&c)
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    use super::*;

    #[test]
    fn a_new_list_gets_all_three_groups_it_needs() {
        assert_eq!(
            edit("", "image/png", "view.desktop"),
            "[Default Applications]\nimage/png=view.desktop;\n\n[Added Associations]\nimage/png=view.desktop;\n"
        );
    }

    #[test]
    fn only_the_types_lines_change() {
        let text = "\
# my apps
[Added Associations]
image/png=old.desktop;view.desktop;
text/plain=edit.desktop;

[Default Applications]
image/png=old.desktop
image/gif=old.desktop;

[Removed Associations]
text/plain=view.desktop;
image/png=view.desktop;bad.desktop;
";
        assert_eq!(
            edit(text, "image/png", "view.desktop"),
            "\
# my apps
[Added Associations]
image/png=view.desktop;old.desktop;
text/plain=edit.desktop;

[Default Applications]
image/png=view.desktop;
image/gif=old.desktop;

[Removed Associations]
text/plain=view.desktop;
image/png=bad.desktop;
"
        );
    }

    #[test]
    fn a_key_goes_after_the_groups_last_line_and_an_emptied_removal_goes() {
        let text = "[Default Applications]\ntext/plain=a.desktop;\n\n[Removed Associations]\nimage/png=view.desktop\n";
        assert_eq!(
            edit(text, "image/png", "view.desktop"),
            "[Default Applications]\ntext/plain=a.desktop;\nimage/png=view.desktop;\n\n[Removed Associations]\n\n[Added Associations]\nimage/png=view.desktop;\n"
        );
    }

    #[test]
    fn a_repeated_group_counts_where_the_reader_looks() {
        let text = "\
[Removed Associations]
text/plain=a.desktop;
[Default Applications]
text/plain=b.desktop;
[Removed Associations]
image/png=view.desktop;
[Default Applications]
image/png=old.desktop;
";
        assert_eq!(
            edit(text, "image/png", "view.desktop"),
            "\
[Removed Associations]
text/plain=a.desktop;
[Default Applications]
text/plain=b.desktop;
[Removed Associations]
[Default Applications]
image/png=view.desktop;

[Added Associations]
image/png=view.desktop;
"
        );
    }

    #[test]
    fn a_type_or_id_that_could_break_the_file_is_refused() {
        assert!(valid_type("image/png"));
        assert!(valid_type("application/vnd.oasis.opendocument.text"));
        assert!(valid_type("image/svg+xml"));
        for bad in [
            "image",
            "/png",
            "image/",
            "image/png\n",
            "a=b/c",
            "a/b/c",
            "[x]/y",
        ] {
            assert!(!valid_type(bad), "{bad:?}");
        }
        assert!(valid_id("org.gnome.Loupe.desktop"));
        for bad in [
            "view",
            "a;b.desktop",
            "a b.desktop",
            "a=b.desktop",
            "[a.desktop",
            "a\n.desktop",
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    use crate::open::tree::Tree;

    fn tree(name: &str) -> (Tree, Places) {
        let tree = Tree::new(&format!("associate-{name}"));
        tree.write(
            "share/applications/view.desktop",
            "[Desktop Entry]\nType=Application\nExec=view %f\n",
        );
        let places = tree.places();
        (tree, places)
    }

    #[test]
    fn set_default_saves_the_users_list_keeping_its_mode_and_link() {
        let (tree, places) = tree("save");
        set_default("image/png", "view.desktop", &places).unwrap();
        let list = tree.0.join("config/mimeapps.list");
        assert!(
            fs::read_to_string(&list)
                .unwrap()
                .starts_with("[Default Applications]\nimage/png=view.desktop;\n")
        );
        assert_eq!(
            fs::metadata(&list).unwrap().permissions().mode() & 0o777,
            private_file::FILE_MODE
        );

        // A list kept elsewhere and linked in stays linked.
        let kept = tree.0.join("dotfiles-mimeapps.list");
        fs::write(&kept, "# mine\n").unwrap();
        fs::set_permissions(&kept, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(&list).unwrap();
        symlink(&kept, &list).unwrap();
        set_default("image/gif", "view.desktop", &places).unwrap();
        assert!(
            fs::symlink_metadata(&list)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&kept).unwrap().starts_with("# mine\n"));
        assert_eq!(
            fs::metadata(&kept).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn a_desktops_own_list_changes_only_where_it_would_win() {
        let (tree, mut places) = tree("desktop");
        places.desktops = vec!["gnome".to_owned(), "kde".to_owned()];
        let config = tree.0.join("config");
        fs::create_dir_all(&config).unwrap();
        let gnome = "[Default Applications]\nimage/png=old.desktop\n";
        // Removing another app for the type doesn't make it win.
        let kde = "[Default Applications]\ntext/plain=kate.desktop\n[Removed Associations]\nimage/png=other.desktop\n";
        fs::write(config.join("gnome-mimeapps.list"), gnome).unwrap();
        fs::write(config.join("kde-mimeapps.list"), kde).unwrap();

        set_default("image/png", "view.desktop", &places).unwrap();

        assert!(
            fs::read_to_string(config.join("gnome-mimeapps.list"))
                .unwrap()
                .starts_with("[Default Applications]\nimage/png=view.desktop;\n")
        );
        assert_eq!(
            fs::read_to_string(config.join("kde-mimeapps.list")).unwrap(),
            kde
        );
        assert!(config.join("mimeapps.list").exists());
        // A desktop without a list of its own gets none.
        places.desktops.push("sway".to_owned());
        set_default("image/gif", "view.desktop", &places).unwrap();
        assert!(!config.join("sway-mimeapps.list").exists());
    }

    #[test]
    fn set_default_refuses_what_it_cant_save() {
        let (tree, places) = tree("refuse");
        assert!(matches!(
            set_default("image/png", "missing.desktop", &places),
            Err(NotSet::Invalid)
        ));
        assert!(matches!(
            set_default("image\npng", "view.desktop", &places),
            Err(NotSet::Invalid)
        ));
        let homeless = Places {
            config_home: None,
            ..places.clone()
        };
        assert!(matches!(
            set_default("image/png", "view.desktop", &homeless),
            Err(NotSet::NoHome)
        ));
        fs::create_dir_all(tree.0.join("config")).unwrap();
        fs::write(tree.0.join("config/mimeapps.list"), b"\xff\n").unwrap();
        assert!(matches!(
            set_default("image/png", "view.desktop", &places),
            Err(NotSet::Failed(_))
        ));
        assert!(
            !tree
                .0
                .join("config")
                .read_dir()
                .unwrap()
                .any(|entry| { entry.unwrap().file_name().to_string_lossy().contains("tmp") })
        );
    }
}
