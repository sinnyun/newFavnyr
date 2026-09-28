use super::*;

// ---------- Navigation history ----------

#[derive(Debug, Default)]
pub(in crate::bridge) struct NavHistory {
    pub(in crate::bridge) stack: Vec<PathBuf>,
    pub(in crate::bridge) cursor: usize,
}

impl NavHistory {
    pub(in crate::bridge) fn current(&self) -> Option<&Path> {
        self.stack.get(self.cursor).map(|p| p.as_path())
    }

    /// Pushes a new path, truncating any "forward" entries.
    pub(in crate::bridge) fn push(&mut self, path: PathBuf) {
        if self.current().map(|c| c == path).unwrap_or(false) {
            return;
        }
        if !self.stack.is_empty() {
            self.stack.truncate(self.cursor + 1);
        }
        self.stack.push(path);
        self.cursor = self.stack.len() - 1;
    }

    pub(in crate::bridge) fn can_back(&self) -> bool {
        self.cursor > 0
    }
    pub(in crate::bridge) fn can_forward(&self) -> bool {
        self.cursor + 1 < self.stack.len()
    }
    pub(in crate::bridge) fn back(&mut self) -> Option<PathBuf> {
        if !self.can_back() {
            return None;
        }
        self.cursor -= 1;
        Some(self.stack[self.cursor].clone())
    }
    pub(in crate::bridge) fn forward(&mut self) -> Option<PathBuf> {
        if !self.can_forward() {
            return None;
        }
        self.cursor += 1;
        Some(self.stack[self.cursor].clone())
    }
}

// ---------- Sort state ----------

#[derive(Debug, Clone, Copy)]
pub(in crate::bridge) struct SortState {
    pub(in crate::bridge) column: SortColumn,
    pub(in crate::bridge) order: SortOrder,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            column: SortColumn::Name,
            order: SortOrder::Asc,
        }
    }
}
