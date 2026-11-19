use std::fmt;
use std::rc::Rc;

use nova_files_api::ListingId;
use nova_files_messages::Envelope;

use crate::location::Location;

pub type OverAsk = Rc<dyn Fn(ListingId, &Location) -> Envelope>;

#[derive(Clone)]
pub struct LaidOver {
    pub ask: OverAsk,
    pub marks: String,
}

impl fmt::Debug for LaidOver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaidOver")
            .field("marks", &self.marks)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub enum OverWant {
    Lay(LaidOver),
    TakeBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverLeft;
