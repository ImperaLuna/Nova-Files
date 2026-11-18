// glibc returns only the free memory at the top of its heap by itself; malloc_trim releases the rest.
// https://man7.org/linux/man-pages/man3/malloc_trim.3.html
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

#[cfg(not(target_env = "gnu"))]
pub const fn give_back() {}

#[cfg(all(test, target_env = "gnu"))]
mod tests {
    use std::hint::black_box;

    use super::*;

    #[test]
    fn freed_memory_under_a_live_block_goes_back() {
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
