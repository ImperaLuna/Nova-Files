//! Gives memory freed after heavy work back to the system.
//!
//! The C library keeps memory a program frees, ready for its next
//! allocation. Loading a big folder frees most of what it built along the
//! way, and glibc keeps about 7 MiB of that after a 100k-file folder. The
//! program hands the core a way to give it back ([`set_give_back`]), since
//! that is a C call and the core stays free of `unsafe`; the core calls it
//! once a listing is done.

use std::sync::OnceLock;

/// What the program set to give free memory back.
static GIVE_BACK: OnceLock<fn()> = OnceLock::new();

/// Sets how the core gives free memory back after heavy work. The first
/// call wins; without one, the core gives nothing back.
pub fn set_give_back(give_back: fn()) {
    GIVE_BACK.set(give_back).ok();
}

/// Gives free memory back, if the program said how.
pub fn give_back() {
    if let Some(give_back) = GIVE_BACK.get() {
        give_back();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn count() {
        CALLS.fetch_add(1, Ordering::SeqCst);
    }

    /// nextest runs each test in a fresh process, so nothing set a hook
    /// before this one.
    #[test]
    fn give_back_runs_what_the_program_set() {
        give_back();
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
        set_give_back(count);
        give_back();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }
}
