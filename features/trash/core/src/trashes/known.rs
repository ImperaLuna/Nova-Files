use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use nova_files_disk::private_file::{self, FILE_MODE, Read};

const LIST: &str = "nova-files/drive-trashes";
const MAX_BYTES: usize = 256 * 1024;
const END: u8 = b'\n';

pub fn list_of(home_trash: &Path) -> PathBuf {
    home_trash.with_file_name(LIST)
}

pub fn read(home_trash: &Path) -> Vec<PathBuf> {
    match private_file::read(&list_of(home_trash), MAX_BYTES) {
        Read::Bytes(bytes) => bytes
            .split(|&byte| byte == END)
            .filter(|line| !line.is_empty())
            .map(|line| PathBuf::from(OsString::from_vec(line.to_vec())))
            .filter(|root| root.is_absolute())
            .collect(),
        Read::Missing => Vec::new(),
        Read::TooBig => {
            log::warn!("{} is too big to read", list_of(home_trash).display());
            Vec::new()
        }
        Read::Failed(error) => {
            log::warn!("can't read {}: {error}", list_of(home_trash).display());
            Vec::new()
        }
    }
}

pub fn remember(home_trash: &Path, root: &Path) -> io::Result<()> {
    let bytes = root.as_os_str().as_bytes();
    if !root.is_absolute() || bytes.contains(&END) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if read(home_trash).iter().any(|known| known == root) {
        return Ok(());
    }
    let list = list_of(home_trash);
    if let Some(folder) = list.parent() {
        std::fs::DirBuilder::new().recursive(true).create(folder)?;
    }
    let mut line = bytes.to_vec();
    line.push(END);
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(FILE_MODE)
        .open(&list)?
        .write_all(&line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

    #[test]
    fn a_drive_trash_is_remembered_once_and_read_back() {
        let folder = TestFolder::new("trash-known");
        let home = folder.path().join("share/Trash");
        let root = Path::new("/srv/sub/.Trash-1000");

        remember(&home, root).unwrap();
        remember(&home, root).unwrap();
        remember(&home, Path::new("/srv/other/.Trash-1000")).unwrap();

        assert_eq!(
            read(&home),
            [root.to_path_buf(), PathBuf::from("/srv/other/.Trash-1000")]
        );
        assert!(remember(&home, Path::new("relative")).is_err());
        assert!(remember(&home, Path::new("/a\nb")).is_err());
        assert_eq!(read(&home).len(), 2);
    }
}
