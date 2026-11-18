use std::path::PathBuf;

use nova_files_open_api::NotOpened;

use super::{FILE, FILES, LIST_GAP, PART_GAP, count, name_of};

const MOST_NAMES: usize = 3;

pub fn note(failed: &[(PathBuf, NotOpened)]) -> Option<String> {
    let mut reasons: Vec<(NotOpened, Vec<&PathBuf>)> = Vec::new();
    for (path, reason) in failed {
        match reasons.iter_mut().find(|(seen, _)| seen == reason) {
            Some((_, paths)) => paths.push(path),
            None => reasons.push((*reason, vec![path])),
        }
    }
    let parts: Vec<String> = reasons
        .into_iter()
        .map(|(reason, paths)| {
            let names = if paths.len() > MOST_NAMES {
                count(paths.len(), FILE, FILES)
            } else {
                let names: Vec<_> = paths.iter().map(|path| name_of(path)).collect();
                names.join(LIST_GAP)
            };
            format!("{names}: {}", why(reason, paths.len() > 1))
        })
        .collect();
    (!parts.is_empty()).then(|| format!("Can't open {}", parts.join(PART_GAP)))
}

const fn why(reason: NotOpened, several: bool) -> &'static str {
    match (reason, several) {
        (NotOpened::NoApp, false) => "no app opens it",
        (NotOpened::NoApp, true) => "no app opens them",
        (NotOpened::Failed, false) => "its app didn't start",
        (NotOpened::Failed, true) => "their apps didn't start",
        (NotOpened::Sandboxed, false) => "the sandbox keeps it from opening",
        (NotOpened::Sandboxed, true) => "the sandbox keeps them from opening",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(names: &[(&str, NotOpened)]) -> Vec<(PathBuf, NotOpened)> {
        names
            .iter()
            .map(|&(name, reason)| (PathBuf::from("/home/a").join(name), reason))
            .collect()
    }

    #[test]
    fn one_file_says_its_name_and_why() {
        assert_eq!(note(&[]), None);
        assert_eq!(
            note(&failed(&[("notes.txt", NotOpened::NoApp)])).as_deref(),
            Some("Can't open notes.txt: no app opens it")
        );
        assert_eq!(
            note(&failed(&[("a.pdf", NotOpened::Failed)])).as_deref(),
            Some("Can't open a.pdf: its app didn't start")
        );
    }

    #[test]
    fn several_files_each_say_why_grouped_by_reason() {
        let mixed = failed(&[
            ("a.txt", NotOpened::NoApp),
            ("c.pdf", NotOpened::Failed),
            ("b.txt", NotOpened::NoApp),
        ]);
        assert_eq!(
            note(&mixed).as_deref(),
            Some("Can't open a.txt, b.txt: no app opens them · c.pdf: its app didn't start")
        );
        let three = failed(&[
            ("a", NotOpened::Failed),
            ("b", NotOpened::Failed),
            ("c", NotOpened::Failed),
        ]);
        assert_eq!(
            note(&three).as_deref(),
            Some("Can't open a, b, c: their apps didn't start")
        );
        let many = failed(&[
            ("a", NotOpened::Sandboxed),
            ("b", NotOpened::Sandboxed),
            ("c", NotOpened::Sandboxed),
            ("d", NotOpened::Sandboxed),
        ]);
        assert_eq!(
            note(&many).as_deref(),
            Some("Can't open 4 files: the sandbox keeps them from opening")
        );
    }
}
