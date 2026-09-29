//! Back/forward locations for note navigation.

use std::path::PathBuf;

const MAX_JUMPS: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Jump {
    pub rel: PathBuf,
    pub cursor: usize,
    pub scroll: usize,
    pub scroll_off: usize,
}

#[derive(Clone, Debug, Default)]
pub struct JumpList {
    back: Vec<Jump>,
    forward: Vec<Jump>,
}

impl JumpList {
    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }

    pub fn peek_back(&self) -> Option<&Jump> {
        self.back.last()
    }

    pub fn peek_forward(&self) -> Option<&Jump> {
        self.forward.last()
    }

    pub fn push(&mut self, jump: Jump) {
        if self.back.last() == Some(&jump) {
            return;
        }
        self.back.push(jump);
        if self.back.len() > MAX_JUMPS {
            self.back.remove(0);
        }
        self.forward.clear();
    }

    pub fn back(&mut self, current: Jump) -> Option<Jump> {
        let dest = self.back.pop()?;
        if self.forward.last() != Some(&current) {
            self.forward.push(current);
            if self.forward.len() > MAX_JUMPS {
                self.forward.remove(0);
            }
        }
        Some(dest)
    }

    pub fn forward(&mut self, current: Jump) -> Option<Jump> {
        let dest = self.forward.pop()?;
        if self.back.last() != Some(&current) {
            self.back.push(current);
            if self.back.len() > MAX_JUMPS {
                self.back.remove(0);
            }
        }
        Some(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(name: &str, cursor: usize) -> Jump {
        Jump {
            rel: PathBuf::from(name),
            cursor,
            scroll: 0,
            scroll_off: 0,
        }
    }

    #[test]
    fn back_and_forward() {
        let mut list = JumpList::default();
        list.push(j("a.md", 0));
        let dest = list.back(j("b.md", 3)).expect("back");
        assert_eq!(dest.rel, PathBuf::from("a.md"));
        let dest = list.forward(j("a.md", 0)).expect("fwd");
        assert_eq!(dest.rel, PathBuf::from("b.md"));
        assert_eq!(dest.cursor, 3);
        assert!(list.peek_back().is_some());
        assert!(list.peek_forward().is_none());
    }
}
