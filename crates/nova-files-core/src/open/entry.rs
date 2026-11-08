//! The parts of a desktop entry (`.desktop` file) that launching it needs:
//! its `[Desktop Entry]` group's `Type`, `Exec`, `Name`, `Icon`, `Path`,
//! `Hidden`, `Terminal` and `TryExec`.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

const GROUP: &str = "[Desktop Entry]";
const GROUP_START: char = '[';
const COMMENT: char = '#';
const KEY_END: char = '=';
const APPLICATION: &str = "Application";
const TRUE: &str = "true";
/// A real entry is a few KB.
/// 256 KiB.
const MAX_ENTRY_BYTES: u64 = 262_144;
/// Any of the execute bits.
const EXECUTABLE: u32 = 0o111;

/// A desktop entry that can open files.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub exec: String,
    pub name: String,
    pub icon: Option<String>,
    /// The folder to run it in.
    pub working_dir: Option<PathBuf>,
}

/// The entry at `path`, or `None` when it is missing, isn't an app, is
/// hidden, needs a terminal, has no `Exec`, or its `TryExec` program isn't
/// installed. `search_path` is `PATH`, for `TryExec`.
pub fn read(path: &Path, search_path: Option<&OsStr>) -> Option<Entry> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_ENTRY_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let entry = parse(&text)?;
    entry
        .try_exec
        .as_deref()
        .is_none_or(|program| installed(program, search_path))
        .then_some(entry.entry)
}

/// An entry and the program its `TryExec` names.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    entry: Entry,
    try_exec: Option<String>,
}

fn parse(text: &str) -> Option<Parsed> {
    let mut in_group = false;
    let (mut kind, mut exec, mut name, mut icon, mut working_dir, mut try_exec) =
        (None, None, None, None, None, None);
    let (mut hidden, mut terminal) = (false, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with(GROUP_START) {
            in_group = line == GROUP;
            continue;
        }
        if !in_group || line.starts_with(COMMENT) {
            continue;
        }
        let Some((key, value)) = line.split_once(KEY_END) else {
            continue;
        };
        let value = unescape(value.trim_start());
        match key.trim_end() {
            "Type" => kind = Some(value),
            "Exec" => exec = Some(value),
            "Name" => name = Some(value),
            "Icon" => icon = Some(value),
            "Path" => working_dir = Some(PathBuf::from(value)),
            "TryExec" => try_exec = Some(value),
            "Hidden" => hidden = value == TRUE,
            "Terminal" => terminal = value == TRUE,
            _ => {}
        }
    }
    if kind.as_deref() != Some(APPLICATION) || hidden || terminal {
        return None;
    }
    Some(Parsed {
        entry: Entry {
            exec: exec.filter(|exec| !exec.is_empty())?,
            name: name.unwrap_or_default(),
            icon: icon.filter(|icon| !icon.is_empty()),
            working_dir: working_dir.filter(|dir| dir.is_absolute()),
        },
        try_exec: try_exec.filter(|program| !program.is_empty()),
    })
}

/// A string value with its escapes (`\s`, `\n`, `\t`, `\r`, `\\`) undone.
/// Any other backslash stays, for the `Exec` quoting rules to read.
fn unescape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => text.push(' '),
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some('r') => text.push('\r'),
            Some('\\') | None => text.push('\\'),
            Some(other) => text.extend(['\\', other]),
        }
    }
    text
}

/// Whether `program`, a path or a name looked up in `search_path`, is an
/// executable file.
fn installed(program: &str, search_path: Option<&OsStr>) -> bool {
    let program = Path::new(program);
    if program.is_absolute() {
        return executable(program);
    }
    search_path.is_some_and(|dirs| {
        std::env::split_paths(dirs)
            .filter(|dir| dir.is_absolute())
            .any(|dir| executable(&dir.join(program)))
    })
}

fn executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|found| found.is_file() && found.permissions().mode() & EXECUTABLE != 0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn entry(exec: &str) -> Entry {
        Entry {
            exec: exec.to_owned(),
            name: String::new(),
            icon: None,
            working_dir: None,
        }
    }

    #[test]
    fn reads_the_launch_keys_of_its_own_group() {
        let text = "\
# comment
[Desktop Entry]
Type = Application
Name=Image Viewer
Name[de]=Bildbetrachter
Icon=viewer
Path=/opt/viewer
Exec=viewer\\s--open %f
# Exec=ignored

[Desktop Action new]
Exec=viewer --new
Name=New
";
        assert_eq!(
            parse(text),
            Some(Parsed {
                entry: Entry {
                    exec: "viewer --open %f".to_owned(),
                    name: "Image Viewer".to_owned(),
                    icon: Some("viewer".to_owned()),
                    working_dir: Some(PathBuf::from("/opt/viewer")),
                },
                try_exec: None,
            })
        );
    }

    #[test]
    fn only_a_shown_app_with_a_command_opens_files() {
        let app = "[Desktop Entry]\nType=Application\nExec=a %f\n";
        assert_eq!(parse(app).map(|p| p.entry), Some(entry("a %f")));
        for text in [
            "[Desktop Entry]\nType=Link\nExec=a %f\n",
            "[Desktop Entry]\nExec=a %f\n",
            "[Desktop Entry]\nType=Application\n",
            "[Desktop Entry]\nType=Application\nExec=\n",
            "[Desktop Entry]\nType=Application\nExec=a %f\nHidden=true\n",
            "[Desktop Entry]\nType=Application\nExec=a %f\nTerminal=true\n",
            "[Other]\nType=Application\nExec=a %f\n",
            "Type=Application\nExec=a %f\n",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
        let shown = "[Desktop Entry]\nType=Application\nExec=a %f\nHidden=false\nTerminal=false\n";
        assert_eq!(parse(shown).map(|p| p.entry), Some(entry("a %f")));
    }

    #[test]
    fn a_relative_working_folder_or_empty_icon_is_ignored() {
        let text = "[Desktop Entry]\nType=Application\nExec=a %f\nPath=rel\nIcon=\n";
        assert_eq!(parse(text).map(|p| p.entry), Some(entry("a %f")));
    }

    #[test]
    fn escapes_are_undone_but_others_stay_for_exec() {
        assert_eq!(unescape(r"a\sb\nc\td\re\\f"), "a b\nc\td\re\\f");
        assert_eq!(unescape(r#"say \"hi\" \"#), r#"say \"hi\" \"#);
    }

    #[test]
    fn try_exec_must_name_an_installed_program() {
        let root = std::env::temp_dir().join(format!("nova-entry-{}", std::process::id()));
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let program = bin.join("viewer");
        fs::write(&program, "").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let plain = bin.join("plain");
        fs::write(&plain, "").unwrap();
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
        let path = std::env::join_paths(["relative", bin.to_str().unwrap()]).unwrap();
        let write = |try_exec: &str| {
            let file = root.join("viewer.desktop");
            fs::write(
                &file,
                format!("[Desktop Entry]\nType=Application\nExec=viewer %f\nTryExec={try_exec}\n"),
            )
            .unwrap();
            read(&file, Some(&path)).map(|entry| entry.exec)
        };
        assert_eq!(write("viewer"), Some("viewer %f".to_owned()));
        assert_eq!(
            write(program.to_str().unwrap()),
            Some("viewer %f".to_owned())
        );
        assert_eq!(write("missing"), None);
        assert_eq!(write("plain"), None, "not executable");
        assert_eq!(write(bin.to_str().unwrap()), None, "a folder");
        assert_eq!(write(""), Some("viewer %f".to_owned()), "empty means none");
        write("viewer");
        let file = root.join("viewer.desktop");
        assert_eq!(read(&file, None), None, "no PATH to find it in");
        assert_eq!(read(&root.join("missing.desktop"), Some(&path)), None);
        fs::remove_dir_all(&root).unwrap();
    }
}
