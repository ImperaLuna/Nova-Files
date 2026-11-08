//! Gives the allocator's free pages back to the system; the core calls it
//! after heavy work (see `nova_files_core::set_give_back`).
//!
//! glibc only gives back what sits free at the very top of its heap on its
//! own. `malloc_trim` also releases free pages under live blocks, in every
//! pool. Other C libraries (musl) have no such call, so it does nothing
//! there.

/// Returns the allocator's free pages to the system. It walks every pool,
/// so the core calls it on a worker thread, never on the UI's.
#[cfg(target_env = "gnu")]
#[expect(
    unsafe_code,
    reason = "malloc_trim is a C call; it only releases pages glibc holds free"
)]
pub fn give_back() {
    // SAFETY: malloc_trim takes a plain integer, locks each pool itself and
    // releases only free pages; any thread may call it.
    unsafe { libc::malloc_trim(0) };
}

/// Other C libraries have nothing to give back.
#[cfg(not(target_env = "gnu"))]
pub const fn give_back() {}

#[cfg(all(test, target_env = "gnu"))]
mod tests {
    use std::hint::black_box;

    use super::*;

    /// Frees 32 MiB of small blocks under one that stays, so glibc can't
    /// give it back on its own; [`give_back`] must. nextest runs each test
    /// in a fresh process, so nothing else frees or allocates meanwhile.
    #[test]
    fn freed_memory_under_a_live_block_goes_back() {
        // KiB this process has in RAM right now.
        let resident_kib = || -> usize {
            let status = std::fs::read_to_string("/proc/self/status").unwrap();
            let line = status
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .unwrap();
            line.split_whitespace().nth(1).unwrap().parse().unwrap()
        };
        let blocks: Vec<Box<[u8; 4096]>> = (0..8192)
            .map(|_| black_box(Box::new([1_u8; 4096])))
            .collect();
        let stays = black_box(Box::new([1_u8; 64]));
        drop(blocks);
        let before = resident_kib();
        give_back();
        assert!(resident_kib() + 16 * 1024 < before);
        drop(stays);
    }
}
