#![expect(
    clippy::derive_partial_eq_without_eq,
    reason = "GPUI's actions! macro derives PartialEq only"
)]

use gpui::{App, KeyBinding, KeyContext};

use crate::commands;

pub const KEY_CONTEXT: &str = "Filtering";

gpui::actions!(filtering, [EraseLetter, ClearFilter]);

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", EraseLetter, context),
        KeyBinding::new("escape", ClearFilter, context),
    ]);
}

pub fn keys(filtering: bool) -> KeyContext {
    let mut keys = KeyContext::default();
    keys.add(commands::KEY_CONTEXT);
    if filtering {
        keys.add(KEY_CONTEXT);
    }
    keys
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub text: String,
    pub total: usize,
    pub moves_cursor: bool,
}

impl Filter {
    pub const fn on(&self) -> bool {
        !self.text.is_empty()
    }

    pub fn typed(&mut self, typed: &str, total: usize) -> bool {
        if !self.on() {
            if typed.trim().is_empty() {
                return false;
            }
            self.total = total;
        }
        self.text.push_str(typed);
        self.moves_cursor = true;
        true
    }

    pub fn erase_letter(&mut self) -> bool {
        if self.text.pop().is_none() {
            return false;
        }
        if self.text.trim().is_empty() {
            self.text.clear();
        }
        self.moves_cursor = self.on();
        true
    }

    pub fn clear(&mut self) -> bool {
        let was_on = self.on();
        *self = Self::default();
        was_on
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_starts_with_a_letter_and_keeps_the_count_it_started_from() {
        let mut filter = Filter::default();
        assert!(!filter.typed(" ", 9));
        assert!(!filter.on());
        assert!(filter.typed("p", 9));
        assert!(filter.typed(" ", 4));
        assert!(filter.typed("h", 4));
        assert_eq!(filter.text, "p h");
        assert_eq!(filter.total, 9);
    }

    #[test]
    fn erasing_the_last_letter_turns_the_filter_off() {
        let mut filter = Filter::default();
        filter.typed("a", 3);
        filter.typed(" ", 3);
        assert!(filter.erase_letter());
        assert_eq!(filter.text, "a");
        assert!(filter.erase_letter());
        assert!(!filter.on());
        assert!(!filter.erase_letter());
    }

    #[test]
    fn clearing_says_whether_there_was_a_filter() {
        let mut filter = Filter::default();
        assert!(!filter.clear());
        filter.typed("x", 1);
        assert!(filter.clear());
        assert_eq!(filter, Filter::default());
    }
}
