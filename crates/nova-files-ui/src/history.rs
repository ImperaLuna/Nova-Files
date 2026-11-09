//! Where a window has been, for Back and Forward.
//!
//! A place joins the history only once its listing is on screen
//! ([`History::arrived`]), so a folder that can't be read never does.
//! Going somewhere new clears Forward, as in a browser. Back and Forward
//! only say where to go ([`History::back`], [`History::forward`]); the
//! window asks the core for it and the history moves when the listing
//! arrives. If that place is refused, it is dropped from the history and
//! the window stays where it was; if that leaves the place on screen next
//! in line, it goes too, so the next step always moves. Each side holds at most [`MAX_PLACES`];
//! the oldest go first.
//!
//! A window owns one history now; each tab will own its own.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// The most places Back (or Forward) remembers.
pub const MAX_PLACES: usize = 100;

/// Which way a trip through the history goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Back,
    Forward,
}

/// A window's history.
#[derive(Debug, Default)]
pub struct History {
    /// Places before the one on screen, newest last.
    back: VecDeque<PathBuf>,
    /// The place on screen, once one has arrived.
    here: Option<PathBuf>,
    /// Places after the one on screen, nearest last.
    forward: Vec<PathBuf>,
    /// The trip asked for and not yet arrived or refused.
    going: Option<(Step, PathBuf)>,
}

impl History {
    /// Where Back goes, if anywhere.
    pub fn back_target(&self) -> Option<&Path> {
        self.back.back().map(PathBuf::as_path)
    }

    /// Where Forward goes, if anywhere.
    pub fn forward_target(&self) -> Option<&Path> {
        self.forward.last().map(PathBuf::as_path)
    }

    /// Starts a step back: returns where to go.
    pub fn back(&mut self) -> Option<PathBuf> {
        self.start(Step::Back)
    }

    /// Starts a step forward: returns where to go.
    pub fn forward(&mut self) -> Option<PathBuf> {
        self.start(Step::Forward)
    }

    fn start(&mut self, step: Step) -> Option<PathBuf> {
        let target = match step {
            Step::Back => self.back_target(),
            Step::Forward => self.forward_target(),
        }?
        .to_path_buf();
        self.going = Some((step, target.clone()));
        Some(target)
    }

    /// The window went somewhere without Back or Forward: forgets a trip
    /// still on its way, so its answer counts as a new place.
    pub fn leave(&mut self) {
        self.going = None;
    }

    /// The listing of `place` is now on screen.
    pub fn arrived(&mut self, place: &Path) {
        if self.here.as_deref() == Some(place) {
            // Read again in place.
            self.going = None;
            return;
        }
        let step = self
            .going
            .take()
            .filter(|(_, target)| target == place)
            .map(|(step, _)| step);
        let left = self.here.replace(place.to_path_buf());
        match step {
            Some(Step::Back) => {
                self.back.pop_back();
                if let Some(left) = left {
                    self.forward.push(left);
                }
            }
            Some(Step::Forward) => {
                self.forward.pop();
                if let Some(left) = left {
                    self.push_back(left);
                }
            }
            None => {
                self.forward.clear();
                if let Some(left) = left {
                    self.push_back(left);
                }
            }
        }
    }

    /// `place` couldn't be read. If Back or Forward was going there, it is
    /// dropped from that side.
    pub fn refused(&mut self, place: &Path) {
        match self.going.take() {
            Some((Step::Back, target)) if target == place => {
                self.back.pop_back();
                // Dropping it may leave the place on screen next in line.
                if self.back.back() == self.here.as_ref() {
                    self.back.pop_back();
                }
            }
            Some((Step::Forward, target)) if target == place => {
                self.forward.pop();
                if self.forward.last() == self.here.as_ref() {
                    self.forward.pop();
                }
            }
            other => self.going = other,
        }
    }

    fn push_back(&mut self, place: PathBuf) {
        if self.back.len() == MAX_PLACES {
            self.back.pop_front();
        }
        self.back.push_back(place);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walked(places: &[&str]) -> History {
        let mut history = History::default();
        for place in places {
            history.arrived(Path::new(place));
        }
        history
    }

    #[test]
    fn back_and_forward_retrace_the_walk() {
        let mut history = walked(&["/a", "/b", "/c"]);
        assert_eq!(history.forward_target(), None);
        assert_eq!(history.back(), Some(PathBuf::from("/b")));
        history.arrived(Path::new("/b"));
        assert_eq!(history.back(), Some(PathBuf::from("/a")));
        history.arrived(Path::new("/a"));
        assert_eq!(history.back_target(), None);
        assert_eq!(history.forward(), Some(PathBuf::from("/b")));
        history.arrived(Path::new("/b"));
        assert_eq!(history.forward_target(), Some(Path::new("/c")));
        assert_eq!(history.back_target(), Some(Path::new("/a")));
    }

    #[test]
    fn a_new_place_clears_forward() {
        let mut history = walked(&["/a", "/b"]);
        history.back();
        history.arrived(Path::new("/a"));
        history.arrived(Path::new("/x"));
        assert_eq!(history.forward_target(), None);
        assert_eq!(history.back_target(), Some(Path::new("/a")));
    }

    #[test]
    fn reading_the_same_place_again_adds_nothing() {
        let history = walked(&["/a", "/a"]);
        assert_eq!(history.back_target(), None);
    }

    #[test]
    fn a_refused_target_is_dropped_and_the_window_stays() {
        let mut history = walked(&["/a", "/gone", "/c"]);
        history.back();
        history.refused(Path::new("/gone"));
        assert_eq!(history.back_target(), Some(Path::new("/a")));
        assert_eq!(history.forward_target(), None);
        history.back();
        history.arrived(Path::new("/a"));
        history.forward();
        history.refused(Path::new("/c"));
        assert_eq!(history.forward_target(), None);
    }

    #[test]
    fn dropping_a_refused_place_never_leaves_this_one_next() {
        let mut history = walked(&["/a", "/b", "/gone", "/b"]);
        history.back();
        history.refused(Path::new("/gone"));
        assert_eq!(history.back_target(), Some(Path::new("/a")));

        // The same the other way: /gone was read once, then went away.
        let mut history = walked(&["/b", "/gone", "/b"]);
        history.back();
        history.arrived(Path::new("/gone"));
        history.back();
        history.arrived(Path::new("/b"));
        assert_eq!(history.forward_target(), Some(Path::new("/gone")));
        history.forward();
        history.refused(Path::new("/gone"));
        assert_eq!(history.forward_target(), None);
    }

    #[test]
    fn a_refusal_of_another_place_keeps_the_trip() {
        let mut history = walked(&["/a", "/b"]);
        history.back();
        history.refused(Path::new("/elsewhere"));
        history.arrived(Path::new("/a"));
        assert_eq!(history.forward_target(), Some(Path::new("/b")));

        let mut history = walked(&["/a", "/b", "/c"]);
        for place in ["/b", "/a"] {
            history.back();
            history.arrived(Path::new(place));
        }
        history.forward();
        history.refused(Path::new("/elsewhere"));
        history.arrived(Path::new("/b"));
        assert_eq!(history.forward_target(), Some(Path::new("/c")));
    }

    #[test]
    fn going_elsewhere_mid_trip_counts_as_a_new_place() {
        let mut history = walked(&["/a", "/b"]);
        history.back();
        history.leave();
        history.arrived(Path::new("/a"));
        // Not a step back: /b stays behind, nothing ahead.
        assert_eq!(history.back_target(), Some(Path::new("/b")));
        assert_eq!(history.forward_target(), None);
    }

    #[test]
    fn back_keeps_only_the_newest_places() {
        let names: Vec<String> = (0..=MAX_PLACES + 1).map(|n| format!("/{n}")).collect();
        let mut history = History::default();
        for name in &names {
            history.arrived(Path::new(name));
        }
        let mut steps = 0;
        while let Some(target) = history.back() {
            history.arrived(&target);
            steps += 1;
        }
        assert_eq!(steps, MAX_PLACES);
        assert_eq!(history.forward_target(), Some(Path::new("/2")));
    }
}
