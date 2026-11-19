//! A picture scaled down to fit a box: what a quick look and a thumbnail
//! show of a picture file.

use std::fmt;
use std::sync::Arc;

/// The longest side a [`Fit`] counts; a bigger one is taken as this.
pub const MAX_FIT_SIDE: u32 = 8192;

/// The box a picture is scaled down to fit, in device pixels: what the
/// client has room to draw. A smaller picture keeps its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fit {
    pub width: u32,
    pub height: u32,
}

impl Fit {
    /// This box with each side at most [`MAX_FIT_SIDE`] and at least 1.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            width: self.width.clamp(1, MAX_FIT_SIDE),
            height: self.height.clamp(1, MAX_FIT_SIDE),
        }
    }
}

/// A picture, decoded and scaled to fit.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// RGBA, four bytes a pixel, row by row from the top.
    pub pixels: Arc<[u8]>,
}

/// Only the size: the pixels would fill a test's failure message.
impl fmt::Debug for Picture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Picture")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_shows_its_size_but_not_its_pixels() {
        let picture = Picture {
            width: 2,
            height: 1,
            pixels: Arc::from([7; 8].as_slice()),
        };
        assert_eq!(
            format!("{picture:?}"),
            "Picture { width: 2, height: 1, .. }"
        );
    }
}
