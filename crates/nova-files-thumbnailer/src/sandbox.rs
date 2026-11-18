use std::error::Error;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::Path;

use landlock::{
    ABI, Access as _, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr as _, RulesetCreated,
    RulesetCreatedAttr as _,
};

// A kernel enforces only the Landlock ABI it supports, possibly none.
// https://docs.kernel.org/userspace-api/landlock.html
const ABI_VERSION: ABI = ABI::V3;
const ROOT: &str = "/";
const NULL_DEVICE: &str = "/dev/null";
pub const FOLDER_MODE: u32 = 0o700;

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

pub fn lock_in_read_only() -> Result<(), Box<dyn Error>> {
    read_only()?.restrict_self()?;
    Ok(())
}

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

    fn kernel_has_landlock() -> bool {
        fs::read_to_string("/sys/kernel/security/lsm").is_ok_and(|lsms| lsms.contains("landlock"))
    }

    #[test]
    fn writes_only_inside_the_folder() {
        let root = std::env::temp_dir().join(format!("nova-thumb-sandbox-{}", std::process::id()));
        let inside = root.join("thumbnails");
        fs::create_dir_all(&root).unwrap();
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
