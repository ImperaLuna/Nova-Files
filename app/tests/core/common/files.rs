use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

pub fn hidden(root: &Path) -> Vec<PathBuf> {
    tree(root)
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.as_bytes().starts_with(b".nova-"))
        })
        .collect()
}

pub fn tree(root: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let kind = path.symlink_metadata().unwrap().file_type();
            let what = if kind.is_symlink() {
                format!("link {}", fs::read_link(&path).unwrap().display())
            } else if kind.is_dir() {
                pending.push(path.clone());
                "folder".to_owned()
            } else {
                format!("file {:?}", fs::read(&path).unwrap())
            };
            found.push((path.strip_prefix(root).unwrap().to_path_buf(), what));
        }
    }
    found.sort();
    found
}
