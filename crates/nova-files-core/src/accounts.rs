//! Who a user or group number names, the way the system says: through NSS,
//! so users from LDAP, systemd-homed and the like are named as well as
//! those in `/etc/passwd`. And the groups the core's user is in, which are
//! the groups it may give its own files.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "properties (FIL-32) names owners with it")
)]

use std::ffi::OsString;

use rustix::process::{Gid, getegid, getgroups};

/// The name of user `uid`, if the system knows one.
pub fn user_name(uid: u32) -> Option<OsString> {
    uzers::get_user_by_uid(uid).map(|user| user.name().to_owned())
}

/// The name of group `gid`, if the system knows one.
pub fn group_name(gid: u32) -> Option<OsString> {
    uzers::get_group_by_gid(gid).map(|group| group.name().to_owned())
}

/// The groups the core's user is in, each once, sorted.
pub fn own_groups() -> Vec<u32> {
    // The supplementary groups may leave out the user's own.
    let mut groups: Vec<u32> = getgroups()
        .unwrap_or_default()
        .into_iter()
        .chain([getegid()])
        .map(Gid::as_raw)
        .collect();
    groups.sort_unstable();
    groups.dedup();
    groups
}

#[cfg(test)]
mod tests {
    use rustix::process::geteuid;
    use rustix::thread::set_thread_groups;

    use super::*;

    /// No system hands out this number.
    const NOBODY_HAS: u32 = u32::MAX - 7;

    #[test]
    fn the_superuser_and_its_group_are_named() {
        assert_eq!(user_name(0), Some("root".into()));
        assert_eq!(group_name(0).map(|name| name.is_empty()), Some(false));
    }

    #[test]
    fn the_user_running_the_core_is_named() {
        assert!(user_name(geteuid().as_raw()).is_some());
    }

    #[test]
    fn a_number_no_one_has_has_no_name() {
        assert_eq!(user_name(NOBODY_HAS), None);
        assert_eq!(group_name(NOBODY_HAS), None);
    }

    #[test]
    fn own_groups_hold_the_user_s_group_once_and_in_order() {
        let groups = own_groups();

        assert!(groups.contains(&getegid().as_raw()));
        assert!(groups.is_sorted_by(|a, b| a < b), "{groups:?}");
    }

    #[test]
    fn own_groups_are_every_group_the_user_is_in() {
        let (one, other) = (4242, 4343);
        let given = [other, getegid().as_raw(), one].map(Gid::from_raw);
        // Only root may change its groups; the change holds for this
        // test's thread alone.
        if set_thread_groups(&given).is_err() {
            return;
        }

        assert_eq!(own_groups(), vec![getegid().as_raw(), one, other]);
    }
}
