use std::collections::VecDeque;

pub const MAX_PLACES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Back,
    Forward,
    Replace,
}

#[derive(Debug)]
pub struct History<L> {
    back: VecDeque<L>,
    here: Option<L>,
    forward: Vec<L>,
    going: Option<(Step, L)>,
}

impl<L> Default for History<L> {
    fn default() -> Self {
        Self {
            back: VecDeque::new(),
            here: None,
            forward: Vec::new(),
            going: None,
        }
    }
}

impl<L: Clone + PartialEq> History<L> {
    pub fn back_target(&self) -> Option<&L> {
        self.back.back()
    }

    pub fn forward_target(&self) -> Option<&L> {
        self.forward.last()
    }

    pub const fn here(&self) -> Option<&L> {
        self.here.as_ref()
    }

    pub fn back(&mut self) -> Option<L> {
        self.start(Step::Back)
    }

    pub fn forward(&mut self) -> Option<L> {
        self.start(Step::Forward)
    }

    pub fn replace_with(&mut self, place: L) {
        self.going = Some((Step::Replace, place));
    }

    fn start(&mut self, step: Step) -> Option<L> {
        let target = match step {
            Step::Back => self.back_target(),
            Step::Forward => self.forward_target(),
            Step::Replace => None,
        }?
        .clone();
        self.going = Some((step, target.clone()));
        Some(target)
    }

    pub fn leave(&mut self) {
        self.going = None;
    }

    pub fn arrived(&mut self, place: &L) {
        if self.here.as_ref() == Some(place) {
            self.going = None;
            return;
        }
        let step = self
            .going
            .take()
            .filter(|(_, target)| target == place)
            .map(|(step, _)| step);
        let left = self.here.replace(place.clone());
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
            Some(Step::Replace) => {}
            None => {
                self.forward.clear();
                if let Some(left) = left {
                    self.push_back(left);
                }
            }
        }
    }

    pub fn refused(&mut self, place: &L) {
        match self.going.take() {
            Some((Step::Back, target)) if target == *place => {
                self.back.pop_back();
                if self.back.back() == self.here.as_ref() {
                    self.back.pop_back();
                }
            }
            Some((Step::Forward, target)) if target == *place => {
                self.forward.pop();
                if self.forward.last() == self.here.as_ref() {
                    self.forward.pop();
                }
            }
            other => self.going = other,
        }
    }

    fn push_back(&mut self, place: L) {
        if self.back.len() == MAX_PLACES {
            self.back.pop_front();
        }
        self.back.push_back(place);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn at(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    fn walked(places: &[&str]) -> History<PathBuf> {
        let mut history = History::default();
        for place in places {
            history.arrived(&at(place));
        }
        history
    }

    #[test]
    fn back_and_forward_retrace_the_walk() {
        let mut history = walked(&["/a", "/b", "/c"]);
        assert_eq!(history.forward_target(), None);
        assert_eq!(history.back(), Some(at("/b")));
        history.arrived(&at("/b"));
        assert_eq!(history.back(), Some(at("/a")));
        history.arrived(&at("/a"));
        assert_eq!(history.back_target(), None);
        assert_eq!(history.forward(), Some(at("/b")));
        history.arrived(&at("/b"));
        assert_eq!(history.forward_target(), Some(&at("/c")));
        assert_eq!(history.back_target(), Some(&at("/a")));
    }

    #[test]
    fn a_new_place_clears_forward() {
        let mut history = walked(&["/a", "/b"]);
        history.back();
        history.arrived(&at("/a"));
        history.arrived(&at("/x"));
        assert_eq!(history.forward_target(), None);
        assert_eq!(history.back_target(), Some(&at("/a")));
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
        history.refused(&at("/gone"));
        assert_eq!(history.back_target(), Some(&at("/a")));
        assert_eq!(history.forward_target(), None);
        history.back();
        history.arrived(&at("/a"));
        history.forward();
        history.refused(&at("/c"));
        assert_eq!(history.forward_target(), None);
    }

    #[test]
    fn dropping_a_refused_place_never_leaves_this_one_next() {
        let mut history = walked(&["/a", "/b", "/gone", "/b"]);
        history.back();
        history.refused(&at("/gone"));
        assert_eq!(history.back_target(), Some(&at("/a")));

        let mut history = walked(&["/b", "/gone", "/b"]);
        history.back();
        history.arrived(&at("/gone"));
        history.back();
        history.arrived(&at("/b"));
        assert_eq!(history.forward_target(), Some(&at("/gone")));
        history.forward();
        history.refused(&at("/gone"));
        assert_eq!(history.forward_target(), None);
    }

    #[test]
    fn a_refusal_of_another_place_keeps_the_trip() {
        let mut history = walked(&["/a", "/b"]);
        history.back();
        history.refused(&at("/elsewhere"));
        history.arrived(&at("/a"));
        assert_eq!(history.forward_target(), Some(&at("/b")));

        let mut history = walked(&["/a", "/b", "/c"]);
        for place in ["/b", "/a"] {
            history.back();
            history.arrived(&at(place));
        }
        history.forward();
        history.refused(&at("/elsewhere"));
        history.arrived(&at("/b"));
        assert_eq!(history.forward_target(), Some(&at("/c")));
    }

    #[test]
    fn going_elsewhere_mid_trip_counts_as_a_new_place() {
        let mut history = walked(&["/a", "/b"]);
        history.back();
        history.leave();
        history.arrived(&at("/a"));
        assert_eq!(history.back_target(), Some(&at("/b")));
        assert_eq!(history.forward_target(), None);
    }

    #[test]
    fn back_keeps_only_the_newest_places() {
        let names: Vec<String> = (0..=MAX_PLACES + 1).map(|n| format!("/{n}")).collect();
        let mut history = History::default();
        for name in &names {
            history.arrived(&at(name));
        }
        let mut steps = 0;
        while let Some(target) = history.back() {
            history.arrived(&target);
            steps += 1;
        }
        assert_eq!(steps, MAX_PLACES);
        assert_eq!(history.forward_target(), Some(&at("/2")));
    }

    #[test]
    fn a_replaced_place_takes_no_step_back() {
        let mut history = History::default();
        let (first, second, third) = (at("/a"), at("/b"), at("/c"));
        history.arrived(&first);
        history.arrived(&second);
        history.replace_with(third.clone());
        history.arrived(&third);
        assert_eq!(history.here(), Some(&third));
        assert_eq!(history.back_target(), Some(&first));
    }
}
