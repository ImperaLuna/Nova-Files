use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const FILE_MODE: u32 = 0o600;
pub const FOLDER_MODE: u32 = 0o700;

static SAVES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum Read {
    Bytes(Vec<u8>),
    Missing,
    TooBig,
    Failed(io::Error),
}

pub fn read(path: &Path, max_bytes: usize) -> Read {
    let mut bytes = Vec::new();
    let cap = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    match fs::File::open(path).and_then(|file| file.take(cap).read_to_end(&mut bytes)) {
        Ok(_) if bytes.len() > max_bytes => Read::TooBig,
        Ok(_) => Read::Bytes(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Read::Missing,
        Err(error) => Read::Failed(error),
    }
}

pub fn replace(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(folder)?;
    }
    let save = SAVES.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{save}", std::process::id()));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)
        .and_then(|file| {
            file.set_permissions(Permissions::from_mode(mode))
                .map(|()| file)
        })
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        fs::remove_file(&temporary).ok();
    }
    written?;
    path.parent().map_or(Ok(()), sync_folder)
}

pub fn sync_folder(folder: &Path) -> io::Result<()> {
    fs::File::open(folder)?.sync_all()
}

const PERMISSION_BITS: u32 = 0o7777;

pub fn mode_to_keep(path: &Path) -> u32 {
    fs::metadata(path).map_or(FILE_MODE, |metadata| {
        metadata.permissions().mode() & PERMISSION_BITS
    })
}

const MAX_LINKS: u32 = 40;

pub fn saved_at(path: &Path) -> io::Result<PathBuf> {
    let mut at = path.to_owned();
    for _ in 0..MAX_LINKS {
        let points_to = match fs::read_link(&at) {
            Ok(points_to) => points_to,
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => return Ok(at),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let reached = at == path || at.parent().is_some_and(Path::is_dir);
                return if reached { Ok(at) } else { Err(error) };
            }
            Err(error) => return Err(error),
        };
        at = at
            .parent()
            .map_or_else(|| points_to.clone(), |folder| folder.join(&points_to));
    }
    Err(rustix::io::Errno::LOOP.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

    const ALL_MODE_BITS: u32 = 0o777;
    const CAP: usize = 16;

    fn temp(name: &str) -> (TestFolder, PathBuf) {
        let folder = TestFolder::new(&format!("private-{name}"));
        let path = folder.join(name);
        (folder, path)
    }

    fn bytes(read: Read) -> Vec<u8> {
        match read {
            Read::Bytes(bytes) => bytes,
            other => panic!("expected bytes, got {other:?}"),
        }
    }

    #[test]
    fn a_replace_swaps_the_file_whole_and_leaves_no_temporary() {
        let (_home_folder, home) = temp("replace");
        let path = home.join("folder/file");
        replace(&path, b"picker 900x600", FILE_MODE).unwrap();
        replace(&path, b"picker 1000x700", FILE_MODE).unwrap();
        assert_eq!(bytes(read(&path, CAP)), b"picker 1000x700");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & ALL_MODE_BITS;
        assert_eq!(mode(&path), FILE_MODE);
        assert_eq!(mode(path.parent().unwrap()), FOLDER_MODE);
        let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
    }

    #[test]
    fn a_replace_keeps_bits_the_umask_would_take() {
        let (_home_folder, home) = temp("umask");
        let path = home.join("folder/file");
        let shared = 0o664;
        replace(&path, b"shared", shared).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & ALL_MODE_BITS;
        assert_eq!(mode, shared);
    }

    #[test]
    fn a_read_tells_missing_oversized_and_unreadable_apart() {
        let (_home_folder, home) = temp("read");
        let path = home.join("file");
        assert!(matches!(read(&path, CAP), Read::Missing));
        replace(&path, &[b'x'; CAP], FILE_MODE).unwrap();
        assert_eq!(bytes(read(&path, CAP)).len(), CAP);
        replace(&path, &[b'x'; CAP + 1], FILE_MODE).unwrap();
        assert!(matches!(read(&path, CAP), Read::TooBig));
        assert!(matches!(read(&home, CAP), Read::Failed(_)));
    }

    #[test]
    fn a_save_through_a_link_to_nowhere_keeps_the_link() {
        let (_home_folder, home) = temp("dangling");
        fs::create_dir_all(&home).unwrap();
        let link = home.join("settings.toml");
        std::os::unix::fs::symlink("dotfiles.toml", &link).unwrap();

        let target = saved_at(&link).unwrap();
        replace(&target, b"saved", FILE_MODE).unwrap();

        assert_eq!(target, home.join("dotfiles.toml"));
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(bytes(read(&link, CAP)), b"saved");
    }

    #[test]
    fn a_link_into_a_missing_folder_is_not_saved_over() {
        let (_home_folder, home) = temp("unmounted");
        fs::create_dir_all(&home).unwrap();
        let link = home.join("bookmarks");
        std::os::unix::fs::symlink(home.join("drive/bookmarks"), &link).unwrap();
        let chained = home.join("chained");
        std::os::unix::fs::symlink("bookmarks", &chained).unwrap();
        let looped = home.join("looped");
        std::os::unix::fs::symlink("looped", &looped).unwrap();

        assert_eq!(saved_at(&link).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(
            saved_at(&chained).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(saved_at(&looped).is_err());
        assert_eq!(saved_at(&home.join("new")).unwrap(), home.join("new"));
    }

    #[test]
    fn a_failed_replace_is_an_error() {
        let (_home_folder, home) = temp("blocked");
        fs::write(&home, b"").unwrap();
        assert!(replace(&home.join("file"), b"x", FILE_MODE).is_err());
    }
}
