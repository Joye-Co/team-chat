//! Back and forward through the places of a window (⌘[ / ⌘]), like a
//! browser: every move to another place keeps the one it left, going back
//! keeps the one it leaves for forward, and a new move drops the forward
//! list.

/// Most places kept behind; the oldest go first.
pub const MAX: usize = 50;

#[derive(Debug)]
pub struct History<T> {
    back: Vec<T>,
    forward: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
        }
    }
}

impl<T: PartialEq> History<T> {
    /// A move away from `left`: keep it to go back to.
    pub fn push(&mut self, left: T) {
        self.forward.clear();
        if self.back.last() == Some(&left) {
            return;
        }
        self.back.push(left);
        if self.back.len() > MAX {
            self.back.remove(0);
        }
    }

    /// The place to go back to from `here`, skipping copies of `here`.
    pub fn back(&mut self, here: T) -> Option<T> {
        Self::step(&mut self.back, &mut self.forward, here)
    }

    /// The place to go forward to from `here`.
    pub fn forward(&mut self, here: T) -> Option<T> {
        Self::step(&mut self.forward, &mut self.back, here)
    }

    /// The place going back leads to (copies of `here` skipped).
    pub fn peek_back(&self, here: &T) -> Option<&T> {
        self.back.iter().rev().find(|p| *p != here)
    }

    #[cfg(test)]
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    #[cfg(test)]
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }

    /// Keeps only the places `keep` accepts (the others are gone).
    pub fn retain(&mut self, keep: impl Fn(&T) -> bool) {
        self.back.retain(&keep);
        self.forward.retain(&keep);
    }

    fn step(from: &mut Vec<T>, to: &mut Vec<T>, here: T) -> Option<T> {
        while let Some(next) = from.pop() {
            if next != here {
                to.push(here);
                return Some(next);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_and_forward_walk_the_places() {
        let mut h = History::default();
        h.push("desk");
        h.push("topic a");
        // Now at "topic b".
        assert_eq!(h.back("topic b"), Some("topic a"));
        assert_eq!(h.back("topic a"), Some("desk"));
        assert_eq!(h.back("desk"), None);
        assert_eq!(h.forward("desk"), Some("topic a"));
        assert_eq!(h.forward("topic a"), Some("topic b"));
        assert!(!h.can_go_forward());
    }

    #[test]
    fn a_new_move_drops_the_forward_places() {
        let mut h = History::default();
        h.push("desk");
        assert_eq!(h.back("topic a"), Some("desk"));
        assert!(h.can_go_forward());
        h.push("desk");
        assert!(!h.can_go_forward());
    }

    #[test]
    fn the_same_place_is_kept_once() {
        let mut h = History::default();
        h.push("desk");
        h.push("desk");
        assert_eq!(h.back("topic"), Some("desk"));
        assert!(!h.can_go_back());
    }

    #[test]
    fn going_back_skips_the_place_it_is_at() {
        let mut h = History::default();
        h.push("topic");
        h.forward.clear();
        h.back.push("desk");
        assert_eq!(h.back("desk"), Some("topic"));
    }

    #[test]
    fn only_the_newest_places_are_kept() {
        let mut h = History::default();
        for i in 0..MAX + 10 {
            h.push(i);
        }
        let mut here = MAX + 10;
        let mut steps = 0;
        while let Some(prev) = h.back(here) {
            here = prev;
            steps += 1;
        }
        assert_eq!(steps, MAX);
        assert_eq!(here, 10);
    }

    #[test]
    fn retain_drops_places_that_are_gone() {
        let mut h = History::default();
        h.push("desk");
        h.push("gone");
        h.retain(|p| *p != "gone");
        assert_eq!(h.back("topic"), Some("desk"));
    }
}
