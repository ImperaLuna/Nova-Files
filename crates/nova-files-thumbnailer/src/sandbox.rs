//! Locks the helper in before it reads any file: it may read anywhere, but
//! write, create or remove files only inside the thumbnails folder. It may
//! also write to `/dev/null`, where the system thumbnailers it starts send
//! their output. For a preview it may write nothing at all: the picture
//! goes out on its standard output, which it already holds.
//!
//! Landlock does the locking. A kernel without Landlock (or an older ABI)
//! gets as much of it as it supports; the helper still runs there, since a
//! missing sandbox must not cost the user their thumbnails.

use std::error::Error;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::Path;

use landlock::{
    ABI, Access as _, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr as _, RulesetCreated,
    RulesetCreatedAttr as _,
};

/// The newest Landlock ABI the rules are written for.
const ABI_VERSION: ABI = ABI::V3;
const ROOT: &str = "/";
const NULL_DEVICE: &str = "/dev/null";
/// The thumbnails folder is private to the user, as the spec asks.
pub const FOLDER_MODE: u32 = 0o700;

/// Creates `writable` if needed, then locks the process in.
pub fn lock_in(writable: &Path) -> Result<(), Box<dyn Error>> {
    DirBuilder::new()
        .recursive(true)
        .mode(FOLDER_MODE)
        .create(writable)?;
    let all = AccessFs::from_all(ABI_VERSION);
    read_only()?
        .add_rule(PathBeneath::new(PathFd::new(writable)?, all))?
        .add_rule(PathBeneath::new(
            PathFd::new(NULL_DEVICE)?,
            AccessFs::WriteFile,
        ))?
        .restrict_self()?;
    Ok(())
}

/// Locks the process in to reading alone.
pub fn lock_in_read_only() -> Result<(), Box<dyn Error>> {
    read_only()?.restrict_self()?;
    Ok(())
}

/// Rules that let the process read anywhere and change nothing.
fn read_only() -> Result<RulesetCreated, Box<dyn Error>> {
    Ok(Ruleset::default()
        .handle_access(AccessFs::from_all(ABI_VERSION))?
        .create()?
        .add_rule(PathBeneath::new(
            PathFd::new(ROOT)?,
            AccessFs::from_read(ABI_VERSION),
        ))?)
}

#[cfg(test)]
mod tests {
    use std::{fs, io, thread};

    use super::*;

    /// Whether this kernel enforces Landlock at all.
    fn kernel_has_landlock() -> bool {
        fs::read_to_string("/sys/kernel/security/lsm").is_ok_and(|lsms| lsms.contains("landlock"))
    }

    #[test]
    fn writes_only_inside_the_folder() {
        let root = std::env::temp_dir().join(format!("nova-thumb-sandbox-{}", std::process::id()));
        let inside = root.join("thumbnails");
        fs::create_dir_all(&root).unwrap();
        // Landlock binds the thread that asks and what it starts, so the
        // test locks a thread of its own.
        let outside = root.clone();
        let result = thread::spawn(move || {
            lock_in(&inside).unwrap();
            let inside_write = fs::write(inside.join("ok.png"), b"x");
            let outside_write = fs::write(outside.join("escaped.png"), b"x");
            (
                inside_write.is_ok(),
                outside_write.map_err(|error| error.kind()),
            )
        })
        .join()
        .unwrap();
        fs::remove_dir_all(&root).ok();
        assert!(result.0);
        if kernel_has_landlock() {
            assert_eq!(result.1, Err(io::ErrorKind::PermissionDenied));
        }
    }

    #[test]
    fn read_only_writes_nowhere() {
        let root =
            std::env::temp_dir().join(format!("nova-thumb-read-only-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("there.txt"), b"x").unwrap();
        let inside = root.clone();
        let result = thread::spawn(move || {
            lock_in_read_only().unwrap();
            (
                fs::read(inside.join("there.txt")).is_ok(),
                fs::write(inside.join("escaped.txt"), b"x").map_err(|error| error.kind()),
            )
        })
        .join()
        .unwrap();
        fs::remove_dir_all(&root).ok();
        assert!(result.0);
        if kernel_has_landlock() {
            assert_eq!(result.1, Err(io::ErrorKind::PermissionDenied));
        }
    }
}
