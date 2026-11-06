//! The program. Wiring only: it starts the core, the D-Bus service and the
//! window, and holds no logic of its own.

#![forbid(unsafe_code)]
// Under `cargo test` an empty `main` is an ordinary function, and Clippy asks
// for `const fn`; the program gains a body before that matters.
#![cfg_attr(test, expect(clippy::missing_const_for_fn, reason = "empty main"))]

fn main() {}
