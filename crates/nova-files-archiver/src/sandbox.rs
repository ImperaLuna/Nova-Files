//! Locks the helper in before it reads the archive: it may open no file,
//! reach no network and signal no process outside itself. What it needs
//! is already open (the archive, and its answers on standard output), so
//! a hostile archive that takes over a decoder finds nothing to read and
//! nowhere to send it.
//!
//! Landlock does the locking. A kernel without Landlock (or an older ABI)
//! gets as much of it as it supports; the helper still runs there, since a
//! missing sandbox must not cost the user their archives.

use std::error::Error;
use std::os::fd::AsFd;

use landlock::{
    ABI, Access as _, AccessFs, AccessNet, BitFlags, PathBeneath, RestrictionStatus, Ruleset,
    RulesetAttr as _, RulesetCreatedAttr as _, Scope,
};

/// The newest Landlock ABI the rules are written for: V6 adds the network
/// (V4) and signal (V6) scopes.
const ABI_VERSION: ABI = ABI::V6;

/// Locks the process in: every file, network and scope right handled,
/// none granted. Says how much of that this kernel enforces.
pub fn lock_in() -> Result<RestrictionStatus, Box<dyn Error>> {
    Ok(ruleset()?.create()?.restrict_self()?)
}

/// Locks the process in as [`lock_in`] does, except that inside the
/// folder `unpack` it may make, write and remove files, folders and
/// links, and open folders.
pub fn lock_in_unpacking(unpack: impl AsFd) -> Result<RestrictionStatus, Box<dyn Error>> {
    let unpacking: BitFlags<AccessFs> = [
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
    Ok(ruleset()?
        .create()?
        .add_rule(PathBeneath::new(unpack, unpacking))?
        .restrict_self()?)
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
        // Landlock binds the thread that asks, so the test locks its own.
        let result = thread::spawn(move || {
            let status = lock_in().unwrap();
            (
                status.ruleset,
                fs::read(inside.join("there.txt")).map_err(|error| error.kind()),
                fs::write(inside.join("escaped.txt"), b"x").map_err(|error| error.kind()),
                TcpStream::connect(address).is_ok(),
                // What was open before still reads.
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
        let folder = fs::File::open(&inside).unwrap();
        let (outside, into) = (root.clone(), inside);
        let result = thread::spawn(move || {
            let status = lock_in_unpacking(folder).unwrap();
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
