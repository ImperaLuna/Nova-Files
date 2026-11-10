//! A desktop entry's `Exec` line, turned into the program and arguments
//! that open one file (freedesktop Desktop Entry spec, "The Exec key").
//!
//! The line is split on spaces; a double-quoted argument keeps its spaces,
//! and in it a backslash escapes `"`, `` ` ``, `$` and `\`. Field codes
//! stand for the file: `%f` its path, `%u` its `file://` URI, and `%F` and
//! `%U` the same as whole arguments. `%i` becomes `--icon` and the entry's
//! icon, `%c` its name, `%k` the entry's own path and `%%` a percent sign.
//! The deprecated codes are dropped. Codes inside quotes stay as written.
//! A line with `%F` or `%U` opens many files at once; any other opens one
//! per run.

use std::ffi::OsString;
use std::path::Path;

use nova_files_api::uri::file_uri;

const QUOTE: char = '"';
const ESCAPE: char = '\\';
const FIELD: char = '%';
/// What a backslash may escape inside quotes.
const QUOTED_ESCAPES: [char; 4] = ['"', '`', '$', '\\'];
/// Codes the spec deprecates; they expand to nothing.
const DEPRECATED: [char; 6] = ['d', 'D', 'n', 'N', 'v', 'm'];
const ICON_OPTION: &str = "--icon";
/// Codes that take every file, as whole arguments.
const LIST_CODES: [&str; 2] = ["%F", "%U"];

/// What the field codes besides the file stand for.
#[derive(Debug, Clone, Copy)]
pub struct Fields<'a> {
    pub name: &'a str,
    pub icon: Option<&'a str>,
    /// The desktop entry file.
    pub entry: &'a Path,
}

/// One argument as written: its text, and whether it was quoted.
#[derive(Debug, PartialEq, Eq)]
struct Word {
    text: String,
    quoted: bool,
}

/// The command lines that open `files`: one taking them all when the line
/// has `%F` or `%U` as a whole argument, else one per file. `None` when
/// there are no files, or when the line is malformed, names no program,
/// or takes no file at all.
pub fn command_lines(
    exec: &str,
    files: &[&Path],
    fields: Fields<'_>,
) -> Option<Vec<Vec<OsString>>> {
    let words = split(exec)?;
    if files.is_empty() {
        return None;
    }
    let takes_many = words
        .iter()
        .any(|word| !word.quoted && LIST_CODES.contains(&word.text.as_str()));
    if takes_many {
        return Some(vec![line_for(&words, files, fields)?]);
    }
    files
        .iter()
        .map(|file| line_for(&words, &[file], fields))
        .collect()
}

/// The line for `words` with `files`: all of them for `%F` and `%U` as a
/// whole argument, the first for any other file code.
fn line_for(words: &[Word], files: &[&Path], fields: Fields<'_>) -> Option<Vec<OsString>> {
    let first = files.first()?;
    let mut line = Vec::with_capacity(words.len() + files.len());
    let mut took_file = false;
    for word in words {
        if word.quoted {
            line.push(OsString::from(&word.text));
            continue;
        }
        match word.text.as_str() {
            "%i" => {
                if let Some(icon) = fields.icon {
                    line.extend([OsString::from(ICON_OPTION), OsString::from(icon)]);
                }
            }
            "%F" => {
                line.extend(files.iter().map(|file| file.as_os_str().to_owned()));
                took_file = true;
            }
            "%U" => {
                line.extend(files.iter().map(|file| OsString::from(file_uri(file))));
                took_file = true;
            }
            text => {
                let (expanded, file_in_word) = expand(text, first, fields)?;
                took_file |= file_in_word;
                if !expanded.is_empty() {
                    line.push(expanded);
                }
            }
        }
    }
    (took_file && !line.is_empty()).then_some(line)
}

/// Splits the line into words, or `None` for an unclosed quote or a bad
/// escape.
fn split(exec: &str) -> Option<Vec<Word>> {
    let mut words = Vec::new();
    let mut chars = exec.chars();
    let mut current: Option<Word> = None;
    while let Some(c) = chars.next() {
        match c {
            ' ' => words.extend(current.take()),
            QUOTE => {
                let word = current.get_or_insert_with(|| Word {
                    text: String::new(),
                    quoted: true,
                });
                word.quoted = true;
                quoted(&mut chars, &mut word.text)?;
            }
            _ => current
                .get_or_insert_with(|| Word {
                    text: String::new(),
                    quoted: false,
                })
                .text
                .push(c),
        }
    }
    words.extend(current);
    Some(words)
}

/// Reads a quoted run up to its closing quote into `text`.
fn quoted(chars: &mut std::str::Chars<'_>, text: &mut String) -> Option<()> {
    loop {
        match chars.next()? {
            QUOTE => return Some(()),
            ESCAPE => {
                let escaped = chars.next()?;
                if !QUOTED_ESCAPES.contains(&escaped) {
                    return None;
                }
                text.push(escaped);
            }
            c => text.push(c),
        }
    }
}

/// Expands the field codes inside one unquoted word, and says whether the
/// file went in. `None` for an unknown code or a lone `%` at the end.
fn expand(word: &str, file: &Path, fields: Fields<'_>) -> Option<(OsString, bool)> {
    let mut expanded = OsString::new();
    let mut took_file = false;
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != FIELD {
            expanded.push(c.encode_utf8(&mut [0; char::MAX_LEN_UTF8]));
            continue;
        }
        match chars.next()? {
            FIELD => expanded.push("%"),
            'f' | 'F' => {
                expanded.push(file.as_os_str());
                took_file = true;
            }
            'u' | 'U' => {
                expanded.push(file_uri(file));
                took_file = true;
            }
            'c' => expanded.push(fields.name),
            'k' => expanded.push(fields.entry.as_os_str()),
            code if DEPRECATED.contains(&code) => {}
            _ => return None,
        }
    }
    Some((expanded, took_file))
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStrExt as _;

    use super::*;

    const ENTRY: &str = "/usr/share/applications/viewer.desktop";

    fn fields() -> Fields<'static> {
        Fields {
            name: "Viewer",
            icon: Some("viewer"),
            entry: Path::new(ENTRY),
        }
    }

    fn command_line(exec: &str, file: &Path, fields: Fields<'_>) -> Option<Vec<OsString>> {
        command_lines(exec, &[file], fields).map(|mut lines| lines.remove(0))
    }

    fn line(exec: &str, file: &str) -> Option<Vec<String>> {
        command_line(exec, Path::new(file), fields()).map(|line| {
            line.into_iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        })
    }

    #[test]
    fn each_file_code_passes_the_path_or_its_uri() {
        assert_eq!(
            line("view %f", "/h/a b.png"),
            Some(vec!["view".into(), "/h/a b.png".into()])
        );
        assert_eq!(
            line("view %F", "/h/a.png"),
            Some(vec!["view".into(), "/h/a.png".into()])
        );
        assert_eq!(
            line("view %u", "/h/a b.png"),
            Some(vec!["view".into(), "file:///h/a%20b.png".into()])
        );
        assert_eq!(
            line("view --new-window %U", "/h/a.png"),
            Some(vec![
                "view".into(),
                "--new-window".into(),
                "file:///h/a.png".into()
            ])
        );
    }

    fn lines(exec: &str, files: &[&str]) -> Option<Vec<Vec<String>>> {
        let files: Vec<&Path> = files.iter().map(Path::new).collect();
        command_lines(exec, &files, fields()).map(|lines| {
            lines
                .into_iter()
                .map(|line| {
                    line.into_iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect()
                })
                .collect()
        })
    }

    #[test]
    fn a_list_code_takes_every_file_in_one_line() {
        assert_eq!(
            lines("view %F", &["/h/a", "/h/b c"]),
            Some(vec![vec!["view".into(), "/h/a".into(), "/h/b c".into()]])
        );
        assert_eq!(
            lines("view %U --x", &["/h/a", "/h/b c"]),
            Some(vec![vec![
                "view".into(),
                "file:///h/a".into(),
                "file:///h/b%20c".into(),
                "--x".into()
            ]])
        );
        // Inside quotes it is only text.
        assert_eq!(
            lines(r#"view "%F" %f"#, &["/h/a", "/h/b"]),
            Some(vec![
                vec!["view".into(), "%F".into(), "/h/a".into()],
                vec!["view".into(), "%F".into(), "/h/b".into()],
            ])
        );
    }

    #[test]
    fn a_single_file_code_runs_once_per_file() {
        assert_eq!(
            lines("view %u", &["/h/a", "/h/b"]),
            Some(vec![
                vec!["view".into(), "file:///h/a".into()],
                vec!["view".into(), "file:///h/b".into()],
            ])
        );
        assert_eq!(
            lines("view %i %f", &["/h/a"]),
            Some(vec![vec![
                "view".into(),
                "--icon".into(),
                "viewer".into(),
                "/h/a".into()
            ]])
        );
    }

    #[test]
    fn no_files_or_a_bad_line_gives_no_lines() {
        assert_eq!(lines("view %F", &[]), None);
        assert_eq!(lines("view %f", &[]), None);
        assert_eq!(lines("view", &["/h/a"]), None);
        assert_eq!(lines("view %F %x", &["/h/a"]), None);
        assert_eq!(lines(r#"view "open %f"#, &["/h/a"]), None);
    }

    #[test]
    fn a_file_code_inside_a_word_is_replaced_there() {
        assert_eq!(
            line("view --file=%f", "/h/a.png"),
            Some(vec!["view".into(), "--file=/h/a.png".into()])
        );
        assert_eq!(
            line("view --uri=%u", "/h/a.png"),
            Some(vec!["view".into(), "--uri=file:///h/a.png".into()])
        );
    }

    #[test]
    fn other_codes_stand_for_the_entry() {
        assert_eq!(
            line("view %i --class %c --from %k %% %f", "/h/a"),
            Some(vec![
                "view".into(),
                "--icon".into(),
                "viewer".into(),
                "--class".into(),
                "Viewer".into(),
                "--from".into(),
                "/usr/share/applications/viewer.desktop".into(),
                "%".into(),
                "/h/a".into(),
            ])
        );
        let no_icon = Fields {
            icon: None,
            ..fields()
        };
        let line = command_line("view %i %f", Path::new("/h/a"), no_icon);
        assert_eq!(
            line,
            Some(vec![OsString::from("view"), OsString::from("/h/a")])
        );
    }

    #[test]
    fn deprecated_codes_vanish() {
        assert_eq!(
            line("view %d %D %n %N %v %m %f", "/h/a"),
            Some(vec!["view".into(), "/h/a".into()])
        );
        assert_eq!(
            line("view -x%m %f", "/h/a"),
            Some(vec!["view".into(), "-x".into(), "/h/a".into()])
        );
    }

    #[test]
    fn quotes_keep_spaces_and_their_escapes() {
        assert_eq!(
            line(r#""/opt/My App/view" "say \"hi\" \$HOME \\ \`" %f"#, "/h/a"),
            Some(vec![
                "/opt/My App/view".into(),
                r#"say "hi" $HOME \ `"#.into(),
                "/h/a".into(),
            ])
        );
        assert_eq!(
            line(r#"view "%f" %f"#, "/h/a"),
            Some(vec!["view".into(), "%f".into(), "/h/a".into()])
        );
        assert_eq!(
            line(r#"view a"b c"d %f"#, "/h/a"),
            Some(vec!["view".into(), "ab cd".into(), "/h/a".into()])
        );
        assert_eq!(
            line(r#"view "" %f"#, "/h/a"),
            Some(vec!["view".into(), String::new(), "/h/a".into()])
        );
    }

    #[test]
    fn many_spaces_make_no_empty_words() {
        assert_eq!(
            line("  view   %f  ", "/h/a"),
            Some(vec!["view".into(), "/h/a".into()])
        );
    }

    #[test]
    fn a_bad_line_or_one_without_a_file_opens_nothing() {
        assert_eq!(line(r#"view "open %f"#, "/h/a"), None, "unclosed quote");
        assert_eq!(line(r#"view "\n" %f"#, "/h/a"), None, "bad escape");
        assert_eq!(line(r#"view "\"#, "/h/a"), None, "escape at the end");
        assert_eq!(line("view %x %f", "/h/a"), None, "unknown code");
        assert_eq!(line("view %f %", "/h/a"), None, "lone percent");
        assert_eq!(line("view", "/h/a"), None, "takes no file");
        assert_eq!(line("view %c", "/h/a"), None, "takes no file");
        assert_eq!(line("", "/h/a"), None);
    }

    #[test]
    fn a_path_that_is_not_text_passes_as_its_bytes() {
        let file = Path::new(std::ffi::OsStr::from_bytes(b"/h/\xff.png"));
        let line = command_line("view %f --as=%f", file, fields());
        assert_eq!(
            line.map(|line| line
                .iter()
                .map(|arg| arg.as_bytes().to_vec())
                .collect::<Vec<_>>()),
            Some(vec![
                b"view".to_vec(),
                b"/h/\xff.png".to_vec(),
                b"--as=/h/\xff.png".to_vec()
            ])
        );
    }
}
