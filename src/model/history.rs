const DEFAULT_HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct History<T> {
    past: Vec<T>,
    present: T,
    future: Vec<T>,
    limit: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(initial: T) -> Self {
        Self {
            past: Vec::new(),
            present: initial,
            future: Vec::new(),
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }

    pub fn current(&self) -> &T {
        &self.present
    }

    pub fn edit(&mut self) -> &mut T {
        &mut self.present
    }

    pub fn commit_from(&mut self, previous: T) -> bool {
        if previous == self.present {
            return false;
        }
        self.record(previous);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.past.pop() else {
            return false;
        };
        self.future
            .push(std::mem::replace(&mut self.present, previous));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        self.past.push(std::mem::replace(&mut self.present, next));
        true
    }

    pub fn reset(&mut self, initial: T) {
        self.past.clear();
        self.future.clear();
        self.present = initial;
    }

    fn record(&mut self, previous: T) {
        if self.limit > 0 {
            if self.past.len() == self.limit {
                self.past.remove(0);
            }
            self.past.push(previous);
        }
        self.future.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_branching_are_consistent() {
        let mut history = History::new(0);
        let previous = *history.current();
        *history.edit() = 1;
        assert!(history.commit_from(previous));
        let previous = *history.current();
        *history.edit() = 2;
        assert!(history.commit_from(previous));
        assert!(history.undo());
        assert_eq!(*history.current(), 1);
        assert!(history.redo());
        assert_eq!(*history.current(), 2);
        assert!(history.undo());
        let previous = *history.current();
        *history.edit() = 9;
        assert!(history.commit_from(previous));
        assert!(!history.can_redo());
    }

    #[test]
    fn in_place_gesture_records_its_baseline_once() {
        let mut history = History::new(vec![1]);
        let baseline = history.current().clone();
        history.edit().push(2);
        history.edit().push(3);
        assert!(history.commit_from(baseline));
        assert!(history.undo());
        assert_eq!(history.current(), &vec![1]);
    }

    #[test]
    fn reset_discards_both_directions() {
        let mut history = History::new(0);
        let previous = *history.current();
        *history.edit() = 1;
        history.commit_from(previous);
        history.undo();
        history.reset(7);
        assert_eq!(*history.current(), 7);
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }
}
