//! The thumbnail sizes of the freedesktop thumbnail spec. A client asks for
//! the smallest one that fills the slot it draws a thumbnail in, so a row
//! gets the light "normal" thumbnail and a big icon the sharp "large" one.

/// The spec's "normal" thumbnails fit in this square.
const NORMAL_SIDE: u32 = 128;
/// The spec's "large" thumbnails fit in this square.
const LARGE_SIDE: u32 = 256;

/// One of the spec's sizes: its thumbnails fit in a square this many
/// pixels across, and live in a folder of the shared cache named after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThumbnailSize {
    /// 128 pixels, in `normal/`.
    #[default]
    Normal,
    /// 256 pixels, in `large/`.
    Large,
}

impl ThumbnailSize {
    /// Every size, smallest first.
    pub const ALL: [Self; 2] = [Self::Normal, Self::Large];

    /// The square its thumbnails fit in, in pixels.
    #[must_use]
    pub const fn side(self) -> u32 {
        match self {
            Self::Normal => NORMAL_SIDE,
            Self::Large => LARGE_SIDE,
        }
    }

    /// Its folder in the shared cache, and the word the helper takes for it.
    #[must_use]
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
        }
    }

    /// The smallest size whose thumbnails fill a slot `device_side` pixels
    /// across; the largest when none does.
    #[must_use]
    pub fn to_fill(device_side: u32) -> Self {
        let largest = Self::ALL[Self::ALL.len() - 1];
        Self::ALL
            .into_iter()
            .find(|size| size.side() >= device_side)
            .unwrap_or(largest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_gets_the_smallest_size_that_fills_it() {
        assert_eq!(ThumbnailSize::to_fill(1), ThumbnailSize::Normal);
        assert_eq!(ThumbnailSize::to_fill(128), ThumbnailSize::Normal);
        assert_eq!(ThumbnailSize::to_fill(129), ThumbnailSize::Large);
        assert_eq!(ThumbnailSize::to_fill(256), ThumbnailSize::Large);
        assert_eq!(ThumbnailSize::to_fill(4000), ThumbnailSize::Large);
    }

    #[test]
    fn sizes_follow_the_spec() {
        assert_eq!(ThumbnailSize::Normal.side(), 128);
        assert_eq!(ThumbnailSize::Large.side(), 256);
        assert_eq!(ThumbnailSize::Normal.folder(), "normal");
        assert_eq!(ThumbnailSize::Large.folder(), "large");
    }
}
