//! The permission bits of a file or folder.

/// The permission bits: read, write and run for owner, group and others,
/// plus set-user, set-group and sticky.
pub const MODE_BITS: u32 = 0o7777;
