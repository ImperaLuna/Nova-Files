use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

pub const FILES: &str = "files";
pub const INFO: &str = "info";
pub const INFO_SUFFIX: &str = ".trashinfo";

pub fn item_of_info(info: &Path) -> Option<PathBuf> {
    let name = info.file_name()?.as_bytes();
    let stored = name.strip_suffix(INFO_SUFFIX.as_bytes())?;
    let folder = info.parent()?;
    if stored.is_empty() || folder.file_name()? != INFO {
        return None;
    }
    let root = folder.parent()?;
    let stored = OsStr::from_bytes(stored);
    Some(root.join(FILES).join(stored))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_info_file_names_its_entry_in_the_same_trash() {
        assert_eq!(
            item_of_info(Path::new("/t/Trash/info/a.txt.trashinfo")),
            Some(PathBuf::from("/t/Trash/files/a.txt"))
        );
    }

    #[test]
    fn only_info_files_in_an_info_folder_name_an_entry() {
        for path in [
            "/t/Trash/info/a.txt",
            "/t/Trash/files/a.trashinfo",
            "/t/Trash/info/.trashinfo",
            "a.trashinfo",
        ] {
            assert_eq!(item_of_info(Path::new(path)), None, "{path}");
        }
    }
}
