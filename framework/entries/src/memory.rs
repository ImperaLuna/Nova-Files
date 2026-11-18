use std::sync::OnceLock;

static GIVE_BACK: OnceLock<fn()> = OnceLock::new();

pub fn set_give_back(give_back: fn()) {
    GIVE_BACK.set(give_back).ok();
}

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

    #[test]
    fn give_back_runs_what_the_program_set() {
        give_back();
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
        set_give_back(count);
        give_back();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }
}
