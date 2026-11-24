use std::ffi::OsStr;
use std::path::Path;

use super::terms::{Met, Terms};
use crate::fold;

pub struct Placed {
    pub met: Met,
    pub length: usize,
    pub depth: usize,
}

pub fn meet(terms: &Terms, root: &Path, path: &Path, loose: bool) -> Option<Placed> {
    let name = path.file_name().map(OsStr::as_encoded_bytes)?;
    let mut folded = Vec::new();
    let mut reach = vec![0; terms.len()];
    let above = path
        .parent()
        .and_then(|folder| folder.strip_prefix(root).ok());
    let depth = above.map_or(0, |folders| {
        folders
            .iter()
            .inspect(|folder| {
                fold::fold_into(folder.as_encoded_bytes(), &mut folded);
                terms.enter(&mut reach, &folded);
            })
            .count()
    });
    fold::fold_into(name, &mut folded);
    let starts_word = |at| fold::starts_word(name, &folded, at);
    let met = terms.meet(&reach, &folded, loose, starts_word)?;
    (!loose || met.loosely).then_some(Placed {
        met,
        length: folded.len(),
        depth,
    })
}
