use std::error::Error;
use std::path::Path;

use landlock::{
    ABI, Access as _, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr as _,
    RulesetCreatedAttr as _,
};

// A kernel enforces only the Landlock ABI it supports, possibly none.
// https://docs.kernel.org/userspace-api/landlock.html
const ABI_VERSION: ABI = ABI::V3;
const ROOT: &str = "/";
const DEVICES: [&str; 3] = ["/dev/null", "/dev/dri", "/dev/snd"];

pub fn lock_in() -> Result<(), Box<dyn Error>> {
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(ABI_VERSION))?
        .create()?
        .add_rule(PathBeneath::new(
            PathFd::new(ROOT)?,
            AccessFs::from_read(ABI_VERSION),
        ))?;
    for device in DEVICES
        .iter()
        .map(Path::new)
        .filter(|device| device.exists())
    {
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(device)?,
            AccessFs::from_read(ABI_VERSION) | AccessFs::WriteFile | AccessFs::Execute,
        ))?;
    }
    ruleset.restrict_self()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, io, thread};

    use nova_files_test_folder::TestFolder;

    use super::*;

    fn kernel_has_landlock() -> bool {
        fs::read_to_string("/sys/kernel/security/lsm").is_ok_and(|lsms| lsms.contains("landlock"))
    }

    #[test]
    fn reads_anything_and_writes_only_devices() {
        let folder = TestFolder::new("player-sandbox");
        let outside = folder.join("outside");
        let result = thread::spawn(move || {
            lock_in().unwrap();
            (
                fs::read_dir(ROOT).is_ok(),
                fs::write("/dev/null", b"x").is_ok(),
                fs::write(&outside, b"x").map_err(|error| error.kind()),
            )
        })
        .join()
        .unwrap();
        assert!(result.0 && result.1);
        if kernel_has_landlock() {
            assert_eq!(result.2, Err(io::ErrorKind::PermissionDenied));
        }
    }
}
