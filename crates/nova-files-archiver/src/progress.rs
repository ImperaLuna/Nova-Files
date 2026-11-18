use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unheard;

pub struct Told<'a, W> {
    out: &'a mut W,
}

impl<'a, W: Write> Told<'a, W> {
    pub const fn new(out: &'a mut W) -> Self {
        Self { out }
    }

    pub fn done(&mut self, bytes: u64) -> Result<(), Unheard> {
        writeln!(self.out, "{bytes}")
            .and_then(|()| self.out.flush())
            .map_err(|_| Unheard)
    }
}
