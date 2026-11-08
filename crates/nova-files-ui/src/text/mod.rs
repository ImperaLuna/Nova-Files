//! The app's one text field (see [`TextField`]), and the keys it takes.
//!
//! The keys are bound under [`KEY_CONTEXT`], so they act only while a field
//! has focus; [`bind_keys`] binds them once per app.

mod edit;
mod element;
mod field;

use gpui::{App, KeyBinding};

pub use field::{TextField, TextFieldEvent};

/// The key context a focused field sets.
pub const KEY_CONTEXT: &str = "TextField";

#[expect(
    clippy::derive_partial_eq_without_eq,
    reason = "GPUI's actions! macro derives PartialEq only"
)]
mod actions {
    gpui::actions!(
        text_field,
        [
            Left,
            Right,
            WordLeft,
            WordRight,
            SelectLeft,
            SelectRight,
            SelectWordLeft,
            SelectWordRight,
            Home,
            End,
            SelectHome,
            SelectEnd,
            SelectAll,
            Backspace,
            Delete,
            DeleteWordLeft,
            DeleteWordRight,
            Copy,
            Cut,
            Paste,
            Submit,
            Cancel,
        ]
    );
}

pub use actions::{
    Backspace, Cancel, Copy, Cut, Delete, DeleteWordLeft, DeleteWordRight, End, Home, Left, Paste,
    Right, SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight, SelectWordLeft,
    SelectWordRight, Submit, WordLeft, WordRight,
};

/// Binds the field's keys, the Linux and Windows way.
pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("ctrl-left", WordLeft, context),
        KeyBinding::new("ctrl-right", WordRight, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("shift-home", SelectHome, context),
        KeyBinding::new("shift-end", SelectEnd, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, context),
        KeyBinding::new("ctrl-delete", DeleteWordRight, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("enter", Submit, context),
        KeyBinding::new("escape", Cancel, context),
    ]);
}
