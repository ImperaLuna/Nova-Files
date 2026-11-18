use std::error::Error;
use std::os::fd::OwnedFd;
use std::path::Path;

use landlock::{
    ABI, Access as _, AccessFs, AccessNet, BitFlags, PathBeneath, RestrictionStatus, Ruleset,
    RulesetAttr as _, RulesetCreatedAttr as _, Scope,
};
use rustix::fs::{Mode, OFlags};

// A kernel enforces only the Landlock ABI it supports, possibly none.
// https://docs.kernel.org/userspace-api/landlock.html
const ABI_VERSION: ABI = ABI::V6;
const PATH_ONLY: OFlags = OFlags::PATH.union(OFlags::CLOEXEC);
const SYSTEM: [&str; 4] = ["/nix/store", "/usr", "/lib", "/lib64"];
const RANDOM: &str = "/dev/urandom";

pub struct Grant {
    fd: OwnedFd,
    rights: BitFlags<AccessFs>,
}

pub fn lock_in(grants: Vec<Grant>) -> Result<RestrictionStatus, Box<dyn Error>> {
    let mut created = ruleset()?.create()?;
    for grant in grants {
        created = created.add_rule(PathBeneath::new(grant.fd, grant.rights))?;
    }
    Ok(created.restrict_self()?)
}

pub fn unpacking(unpack: OwnedFd) -> Grant {
    let rights = [
        AccessFs::ReadDir,
        AccessFs::WriteFile,
        AccessFs::Truncate,
        AccessFs::MakeDir,
        AccessFs::MakeReg,
        AccessFs::MakeSym,
        AccessFs::RemoveDir,
        AccessFs::RemoveFile,
        AccessFs::Refer,
    ]
    .into_iter()
    .collect();
    Grant { fd: unpack, rights }
}

pub fn reading(fd: OwnedFd, folder: bool) -> Grant {
    let rights = if folder {
        [AccessFs::ReadFile, AccessFs::ReadDir]
            .into_iter()
            .collect()
    } else {
        AccessFs::ReadFile.into()
    };
    Grant { fd, rights }
}

pub fn naming(fd: OwnedFd) -> Grant {
    Grant {
        fd,
        rights: AccessFs::ReadDir.into(),
    }
}

pub fn making_in(fd: OwnedFd) -> Grant {
    let rights = [AccessFs::MakeReg, AccessFs::WriteFile]
        .into_iter()
        .collect();
    Grant { fd, rights }
}

pub fn running(program: &Path) -> Vec<Grant> {
    let folder = program
        .canonicalize()
        .ok()
        .and_then(|program| program.parent().map(Path::to_path_buf));
    let rights: BitFlags<AccessFs> = [AccessFs::Execute, AccessFs::ReadFile, AccessFs::ReadDir]
        .into_iter()
        .collect();
    folder
        .into_iter()
        .chain(SYSTEM.iter().map(Into::into))
        .filter_map(|folder| rustix::fs::open(&folder, PATH_ONLY, Mode::empty()).ok())
        .map(|fd| Grant { fd, rights })
        .chain(
            rustix::fs::open(RANDOM, PATH_ONLY, Mode::empty())
                .ok()
                .map(|fd| reading(fd, false)),
        )
        .collect()
}

fn ruleset() -> Result<Ruleset, Box<dyn Error>> {
    Ok(Ruleset::default()
        .handle_access(AccessFs::from_all(ABI_VERSION))?
        .handle_access(AccessNet::from_all(ABI_VERSION))?
        .scope(Scope::from_all(ABI_VERSION))?)
}

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};
    use std::{fs, io, thread};

    use landlock::RulesetStatus;

    use super::*;

    #[test]
    fn opens_nothing_and_connects_nowhere() {
        let root =
            std::env::temp_dir().join(format!("nova-archiver-locked-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("there.txt"), b"x").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let inside = root.clone();
        let held = fs::File::open(root.join("there.txt")).unwrap();
        let result = thread::spawn(move || {
            let status = lock_in(Vec::new()).unwrap();
            (
                status.ruleset,
                fs::read(inside.join("there.txt")).map_err(|error| error.kind()),
                fs::write(inside.join("escaped.txt"), b"x").map_err(|error| error.kind()),
                TcpStream::connect(address).is_ok(),
                io::read_to_string(held).is_ok(),
            )
        })
        .join()
        .unwrap();
        fs::remove_dir_all(&root).ok();
        let (status, read, write, connected, held_reads) = result;
        assert!(held_reads);
        if status != RulesetStatus::NotEnforced {
            assert_eq!(read, Err(io::ErrorKind::PermissionDenied));
            assert_eq!(write, Err(io::ErrorKind::PermissionDenied));
        }
        if status == RulesetStatus::FullyEnforced {
            assert!(!connected);
        }
    }

    #[test]
    fn unpacking_writes_inside_its_folder_only() {
        let root =
            std::env::temp_dir().join(format!("nova-archiver-unpacking-{}", std::process::id()));
        let inside = root.join("into");
        fs::create_dir_all(&inside).unwrap();
        fs::write(inside.join("there.txt"), b"x").unwrap();
        let folder = OwnedFd::from(fs::File::open(&inside).unwrap());
        let (outside, into) = (root.clone(), inside);
        let result = thread::spawn(move || {
            let status = lock_in(vec![unpacking(folder)]).unwrap();
            (
                status.ruleset,
                fs::write(into.join("new.txt"), b"x").map_err(|error| error.kind()),
                fs::create_dir(into.join("made")).map_err(|error| error.kind()),
                fs::read(into.join("there.txt")).map_err(|error| error.kind()),
                fs::write(outside.join("escaped.txt"), b"x").map_err(|error| error.kind()),
            )
        })
        .join()
        .unwrap();
        fs::remove_dir_all(&root).ok();
        let (status, write, made, read, escaped) = result;
        assert_eq!(write, Ok(()));
        assert_eq!(made, Ok(()));
        if status != RulesetStatus::NotEnforced {
            assert_eq!(read, Err(io::ErrorKind::PermissionDenied));
            assert_eq!(escaped, Err(io::ErrorKind::PermissionDenied));
        }
    }
}
