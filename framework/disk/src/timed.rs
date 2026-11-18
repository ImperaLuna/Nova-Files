use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub fn within<T: Send + 'static>(
    wait: Duration,
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (done, result) = mpsc::channel();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            done.send(work()).ok();
        })
        .ok()?;
    result.recv_timeout(wait).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_read_answers_and_a_stuck_one_is_given_up_on() {
        assert_eq!(within(Duration::from_secs(10), "test", || 7), Some(7));
        let (_hold, stuck) = mpsc::channel::<()>();
        let never = within(Duration::from_millis(10), "test", move || stuck.recv().ok());
        assert_eq!(never, None);
    }
}
