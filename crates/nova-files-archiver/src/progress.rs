//! Tells the core how far a run has got: a line for each entry done,
//! the bytes it held, in decimal, sent on at once.

use std::io::Write;

/// The core stopped reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unheard;

pub struct Told<'a, W> {
    out: &'a mut W,
}

impl<'a, W: Write> Told<'a, W> {
    pub const fn new(out: &'a mut W) -> Self {
        Self { out }
    }

    /// One more entry is done; it held `bytes`.
    pub fn done(&mut self, bytes: u64) -> Result<(), Unheard> {
        writeln!(self.out, "{bytes}")
            .and_then(|()| self.out.flush())
            .map_err(|_| Unheard)
    }
}
