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

use landlock::{
    ABI, Access as _, AccessFs, AccessNet, RestrictionStatus, Ruleset, RulesetAttr as _, Scope,
};

/// The newest Landlock ABI the rules are written for: V6 adds the network
/// (V4) and signal (V6) scopes.
const ABI_VERSION: ABI = ABI::V6;

/// Locks the process in: every file, network and scope right handled,
/// none granted. Says how much of that this kernel enforces.
pub fn lock_in() -> Result<RestrictionStatus, Box<dyn Error>> {
    Ok(Ruleset::default()
        .handle_access(AccessFs::from_all(ABI_VERSION))?
        .handle_access(AccessNet::from_all(ABI_VERSION))?
        .scope(Scope::from_all(ABI_VERSION))?
        .create()?
        .restrict_self()?)
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
}
