//! What the windows keep between runs, as named values: the picker's size
//! today, and whatever a window's parts save as they close.

use std::collections::BTreeMap;

const HEADER: &str = "nova-files window-state 1";
const SPLIT: char = ' ';
const END: char = '\n';

/// Named values, each a key without spaces or line breaks and a value
/// without line breaks. A save sends only the keys it changed; the core
/// keeps every other key it already had.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowState {
    values: BTreeMap<String, String>,
}

impl WindowState {
    /// Reads saved bytes; anything not written by [`Self::to_bytes`] reads
    /// as no state at all.
    pub fn parse(bytes: &[u8]) -> Self {
        let Ok(text) = str::from_utf8(bytes) else {
            return Self::default();
        };
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Self::default();
        }
        let mut state = Self::default();
        for line in lines.filter(|line| !line.is_empty()) {
            match line.split_once(SPLIT) {
                Some((key, value)) if !key.is_empty() => state.set(key, value),
                _ => return Self::default(),
            }
        }
        state
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = String::from(HEADER);
        text.push(END);
        for (key, value) in &self.values {
            text.push_str(key);
            text.push(SPLIT);
            text.push_str(value);
            text.push(END);
        }
        text.into_bytes()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Sets `key` to `value`; a key or value that could not be read back
    /// is left out.
    pub fn set(&mut self, key: &str, value: &str) {
        let fits = !key.is_empty() && !key.contains([SPLIT, END]) && !value.contains(END);
        if fits {
            self.values.insert(key.to_owned(), value.to_owned());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// This state with `changed`'s keys written over it.
    #[must_use]
    pub fn merged(mut self, changed: Self) -> Self {
        self.values.extend(changed.values);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pairs: &[(&str, &str)]) -> WindowState {
        let mut state = WindowState::default();
        for (key, value) in pairs {
            state.set(key, value);
        }
        state
    }

    #[test]
    fn a_state_reads_back_what_it_wrote() {
        let written = state(&[("picker", "900x600"), ("trash", "shown")]).to_bytes();
        assert_eq!(
            written,
            b"nova-files window-state 1\npicker 900x600\ntrash shown\n"
        );
        let read = WindowState::parse(&written);
        assert_eq!(read.get("picker"), Some("900x600"));
        assert_eq!(read.get("trash"), Some("shown"));
        assert_eq!(
            WindowState::default().to_bytes(),
            b"nova-files window-state 1\n"
        );
    }

    #[test]
    fn anything_else_reads_as_no_state() {
        for bytes in [
            &b""[..],
            b"picker 900x600\n",
            b"nova-files window-state 2\npicker 900x600\n",
            b"nova-files window-state 1\npicker\n",
            b"nova-files window-state 1\n 900x600\n",
            b"\xff\xfe",
        ] {
            assert_eq!(
                WindowState::parse(bytes),
                WindowState::default(),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn a_key_or_value_that_would_not_read_back_is_left_out() {
        let state = state(&[
            ("", "a"),
            ("two words", "a"),
            ("line", "a\nb"),
            ("kept", "a b"),
        ]);
        assert_eq!(state, WindowState::parse(&state.to_bytes()));
        assert_eq!(state.get("kept"), Some("a b"));
        assert_eq!(state.get("line"), None);
    }

    #[test]
    fn a_merge_keeps_the_keys_the_change_leaves_out() {
        let saved = state(&[("picker", "900x600"), ("trash", "shown")]);
        let merged = saved.merged(state(&[("trash", "hidden")]));
        assert_eq!(merged, state(&[("picker", "900x600"), ("trash", "hidden")]));
    }
}
