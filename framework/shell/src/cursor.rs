#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    First,
    Last,
}

impl Move {
    pub fn to(
        self,
        from: Option<usize>,
        len: usize,
        (page, per_line): (usize, usize),
    ) -> Option<usize> {
        let last = len.checked_sub(1)?;
        let per_line = per_line.max(1);
        let page = (page / per_line).max(1) * per_line;
        Some(match (self, from) {
            (Self::First, _) | (Self::Down | Self::Right | Self::PageDown, None) => 0,
            (Self::Last, _) | (Self::Up | Self::Left | Self::PageUp, None) => last,
            (Self::Up, Some(from)) => from.checked_sub(per_line).unwrap_or(from),
            (Self::Left, Some(from)) => from.saturating_sub(1),
            (Self::PageUp, Some(from)) => from.saturating_sub(page),
            (Self::Down, Some(from)) if from / per_line == last / per_line => from,
            (Self::Down, Some(from)) => from.saturating_add(per_line).min(last),
            (Self::Right, Some(from)) => from.saturating_add(1).min(last),
            (Self::PageDown, Some(from)) => from.saturating_add(page).min(last),
        })
    }
}
