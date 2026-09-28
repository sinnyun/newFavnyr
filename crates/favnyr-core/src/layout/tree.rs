use super::*;

impl LayoutNode {
    /// Builds a default tree from a flat list of panels and their
    /// `stretch`: a left-leaning **chain of `Row` splits**, with all panels
    /// side by side. This shape is used as a fallback when a workspace
    /// doesn't contain an explicit tree.
    pub fn row_chain(stretches: &[f32]) -> LayoutNode {
        fn build(idx: usize, stretches: &[f32]) -> LayoutNode {
            let n = stretches.len();
            if idx + 1 >= n {
                return LayoutNode::Leaf { panel: idx };
            }
            let remaining: f32 = stretches[idx..].iter().filter(|s| s.is_finite()).sum();
            let here = stretches[idx].max(0.0);
            let ratio = if remaining > 0.0 {
                (here / remaining).clamp(0.05, 0.95)
            } else {
                0.5
            };
            LayoutNode::Split {
                dir: SplitDir::Row,
                ratio,
                first: Box::new(LayoutNode::Leaf { panel: idx }),
                second: Box::new(build(idx + 1, stretches)),
            }
        }
        if stretches.is_empty() {
            return LayoutNode::Leaf { panel: 0 };
        }
        build(0, stretches)
    }

    /// Panel indices of the leaves, in infix traversal order.
    pub fn leaf_indices(&self) -> Vec<usize> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<usize>) {
        match self {
            LayoutNode::Leaf { panel } => out.push(*panel),
            LayoutNode::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    /// Swaps the PLACES of two views: the leaves holding `a` and `b` exchange
    /// their panel index.
    ///
    /// Nothing else moves — not the shape of the tree, not a single ratio, not
    /// the panels themselves. Each view therefore takes the size of its new
    /// place, which is exactly what swapping two tiles means, and everything
    /// keyed by panel index (the active view, its tabs, its history) stays
    /// valid because no index is created or destroyed.
    ///
    /// Refused, with nothing written, when the two are the same or when either
    /// has no leaf: a half-applied swap would leave the tree holding one index
    /// twice, which `is_valid_for` would then reject.
    pub fn swap_panels(&mut self, a: usize, b: usize) -> bool {
        if a == b {
            return false;
        }
        let leaves = self.leaf_indices();
        if !leaves.contains(&a) || !leaves.contains(&b) {
            return false;
        }
        self.each_leaf_mut(&mut |panel| {
            if *panel == a {
                *panel = b;
            } else if *panel == b {
                *panel = a;
            }
        });
        true
    }

    fn each_leaf_mut(&mut self, visit: &mut impl FnMut(&mut usize)) {
        match self {
            LayoutNode::Leaf { panel } => visit(panel),
            LayoutNode::Split { first, second, .. } => {
                first.each_leaf_mut(visit);
                second.each_leaf_mut(visit);
            }
        }
    }

    /// True if the tree covers **exactly** the panels `0..panel_count`, each
    /// exactly once (no missing, duplicated, or out-of-bounds index). Used
    /// as a safeguard before using a tree loaded from disk.
    pub fn is_valid_for(&self, panel_count: usize) -> bool {
        if panel_count == 0 {
            return false;
        }
        let mut seen = vec![false; panel_count];
        for i in self.leaf_indices() {
            if i >= panel_count || seen[i] {
                return false;
            }
            seen[i] = true;
        }
        seen.iter().all(|&b| b)
    }

    /// Mutable reference to the node designated by `path` (root if empty).
    pub fn node_at_mut(&mut self, path: &[Side]) -> Option<&mut LayoutNode> {
        let mut node = self;
        for side in path {
            match node {
                LayoutNode::Split { first, second, .. } => {
                    node = match side {
                        Side::First => first,
                        Side::Second => second,
                    };
                }
                LayoutNode::Leaf { .. } => return None,
            }
        }
        Some(node)
    }

    /// Adjusts the `ratio` of the split located at `path`. No-op if the path
    /// doesn't point to a `Split`. The value is clamped to `[min, 1 - min]`.
    pub fn set_ratio(&mut self, path: &[Side], ratio: f32, min: f32) -> bool {
        if let Some(LayoutNode::Split { ratio: r, .. }) = self.node_at_mut(path) {
            *r = ratio.clamp(min, 1.0 - min);
            true
        } else {
            false
        }
    }

    /// Number of tracks the subtree occupies along `dir`: how many columns it
    /// shows across a [`SplitDir::Row`], how many bands down a
    /// [`SplitDir::Column`].
    ///
    /// A split ALONG `dir` puts its children end to end, so their tracks add
    /// up; a split ACROSS it lays them over the same tracks, so the wider of
    /// the two decides. A leaf is one track.
    fn tracks(&self, dir: SplitDir) -> usize {
        match self {
            LayoutNode::Leaf { .. } => 1,
            LayoutNode::Split {
                dir: d,
                first,
                second,
                ..
            } => {
                let (a, b) = (first.tracks(dir), second.tracks(dir));
                if *d == dir { a + b } else { a.max(b) }
            }
        }
    }

    /// Gives every track the same size, recursively.
    ///
    /// Each split hands a side the room its tracks are worth, separators
    /// included, so the panels come out the same WIDTH across a row and the
    /// same HEIGHT down a column. Two panels side by side next to a third
    /// therefore take a third each, not a half and two quarters — and the
    /// result does not depend on which way the tree happens to lean, which the
    /// user cannot see.
    ///
    /// Where a row and a column cross, the track count of the crossing side is
    /// the widest of its bands, so the sizing is as even as that shape allows.
    fn spread(&mut self, area: Rect, gap: f32, min: f32) {
        let LayoutNode::Split {
            dir,
            ratio,
            first,
            second,
        } = self
        else {
            return;
        };
        let d = *dir;
        let n1 = first.tracks(d) as f32;
        let n = n1 + second.tracks(d) as f32;
        let span = match d {
            SplitDir::Row => area.w,
            SplitDir::Column => area.h,
        };
        // Room the first side needs for every track in `area` to come out the
        // same size. Each of the `n` tracks gets an equal share of what the
        // `n - 1` separators leave, and the first side also keeps the
        // separators that fall inside it — which reduces to this.
        let first_span = n1 / n * (span + gap) - gap;
        let avail = span - gap;
        if avail > f32::EPSILON {
            *ratio = (first_span / avail).clamp(min, 1.0 - min);
        }
        let (a1, _, a2) = split_area(area, d, *ratio, gap);
        first.spread(a1, gap, min);
        second.spread(a2, gap, min);
    }

    /// Ratios of the subtree, a node before its children — a fixed order, so a
    /// copy taken now can be written back later.
    fn collect_ratios(&self, out: &mut Vec<f32>) {
        if let LayoutNode::Split {
            ratio,
            first,
            second,
            ..
        } = self
        {
            out.push(*ratio);
            first.collect_ratios(out);
            second.collect_ratios(out);
        }
    }

    fn write_ratios(&mut self, next: &mut impl Iterator<Item = f32>) {
        if let LayoutNode::Split {
            ratio,
            first,
            second,
            ..
        } = self
        {
            if let Some(r) = next.next() {
                *ratio = r;
            }
            first.write_ratios(next);
            second.write_ratios(next);
        }
    }

    /// Reference to the node designated by `path` (root if empty).
    fn node_at(&self, path: &[Side]) -> Option<&LayoutNode> {
        let mut node = self;
        for side in path {
            match node {
                LayoutNode::Split { first, second, .. } => {
                    node = match side {
                        Side::First => first,
                        Side::Second => second,
                    };
                }
                LayoutNode::Leaf { .. } => return None,
            }
        }
        Some(node)
    }

    /// Ratios of the subtree at `path`, in [`Self::collect_ratios`] order.
    /// Empty for a leaf or an invalid path.
    pub fn ratios(&self, path: &[Side]) -> Vec<f32> {
        let mut out = Vec::new();
        if let Some(node) = self.node_at(path) {
            node.collect_ratios(&mut out);
        }
        out
    }

    /// Evens out the subtree at `path` over `area`: every panel it holds ends
    /// up the same width across a row, the same height down a column.
    ///
    /// The root path evens out the whole tree; any other evens out only what
    /// that separator governs, leaving hand-set proportions elsewhere alone.
    ///
    /// `area` is the rectangle THAT SUBTREE occupies — the `area` field the
    /// flattening already records on every [`SplitterGeom`]. It is needed
    /// because separators take absolute pixels while ratios are relative: only
    /// with both can the tracks come out truly equal.
    ///
    /// Returns the ratios as they stood BEFORE **and** the ones written in
    /// their place, or `None` when nothing moved — there is then nothing to
    /// undo. Both are handed back together on purpose: a caller holding the
    /// tree through a lock would otherwise have to reach for it a second time
    /// just to read what it had itself just written.
    pub fn equalize(
        &mut self,
        path: &[Side],
        area: Rect,
        gap: f32,
        min: f32,
    ) -> Option<(Vec<f32>, Vec<f32>)> {
        let before = self.ratios(path);
        let node = self.node_at_mut(path)?;
        node.spread(area, gap, min);
        let mut after = Vec::new();
        node.collect_ratios(&mut after);
        let moved = before
            .iter()
            .zip(&after)
            .any(|(a, b)| (a - b).abs() > RATIO_EPSILON);
        moved.then_some((before, after))
    }

    /// Writes ratios back into the subtree at `path` — the way back from
    /// [`Self::equalize`].
    ///
    /// Refused, and nothing written, when the subtree no longer holds the same
    /// number of splits: the layout changed under the copy, and a half-applied
    /// restore would be worse than none.
    pub fn restore_ratios(&mut self, path: &[Side], ratios: &[f32]) -> bool {
        let Some(node) = self.node_at_mut(path) else {
            return false;
        };
        let mut here = Vec::new();
        node.collect_ratios(&mut here);
        if here.len() != ratios.len() {
            return false;
        }
        node.write_ratios(&mut ratios.iter().copied());
        true
    }

    /// Splits the leaf of panel `panel`: it becomes a `Split`.
    /// `new_first = false` → the existing panel stays first (left/top) and
    /// `new_panel` comes second (right/bottom); `new_first = true` → the
    /// reverse (for a drop on the west/north edge). Returns `false` if no
    /// leaf `panel` exists. First match only (indices are unique).
    pub fn split_leaf(
        &mut self,
        panel: usize,
        dir: SplitDir,
        new_panel: usize,
        ratio: f32,
        new_first: bool,
    ) -> bool {
        match self {
            LayoutNode::Leaf { panel: p } if *p == panel => {
                let kept = LayoutNode::Leaf { panel: *p };
                let fresh = LayoutNode::Leaf { panel: new_panel };
                let (first, second, r) = if new_first {
                    // The new panel takes the `ratio` share (first child).
                    (fresh, kept, ratio)
                } else {
                    (kept, fresh, ratio)
                };
                *self = LayoutNode::Split {
                    dir,
                    ratio: r.clamp(0.05, 0.95),
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            LayoutNode::Leaf { .. } => false,
            LayoutNode::Split { first, second, .. } => {
                first.split_leaf(panel, dir, new_panel, ratio, new_first)
                    || second.split_leaf(panel, dir, new_panel, ratio, new_first)
            }
        }
    }

    /// Removes the leaf of panel `idx`: its parent `Split` is replaced by
    /// the sibling leaf (merge), then all leaf indices `> idx` are
    /// **decremented** to stay consistent with the caller removing
    /// `panels[idx]`. Returns `false` if the tree is reduced to a single
    /// leaf (the last panel is never removed).
    pub fn remove_panel(&mut self, idx: usize) -> bool {
        if matches!(self, LayoutNode::Leaf { .. }) {
            return false;
        }
        if Self::collapse_leaf(self, idx) {
            self.reindex_after_removal(idx);
            true
        } else {
            false
        }
    }

    fn collapse_leaf(node: &mut LayoutNode, idx: usize) -> bool {
        // If one of the direct children is the target leaf, this node
        // becomes the other child.
        let replacement = match node {
            LayoutNode::Split { first, second, .. } => {
                if matches!(**first, LayoutNode::Leaf { panel } if panel == idx) {
                    Some(std::mem::replace(
                        &mut **second,
                        LayoutNode::Leaf { panel: 0 },
                    ))
                } else if matches!(**second, LayoutNode::Leaf { panel } if panel == idx) {
                    Some(std::mem::replace(
                        &mut **first,
                        LayoutNode::Leaf { panel: 0 },
                    ))
                } else {
                    None
                }
            }
            LayoutNode::Leaf { .. } => return false,
        };
        if let Some(repl) = replacement {
            *node = repl;
            return true;
        }
        // Otherwise, descend into the subtrees.
        if let LayoutNode::Split { first, second, .. } = node {
            Self::collapse_leaf(first, idx) || Self::collapse_leaf(second, idx)
        } else {
            false
        }
    }

    fn reindex_after_removal(&mut self, removed: usize) {
        match self {
            LayoutNode::Leaf { panel } => {
                if *panel > removed {
                    *panel -= 1;
                }
            }
            LayoutNode::Split { first, second, .. } => {
                first.reindex_after_removal(removed);
                second.reindex_after_removal(removed);
            }
        }
    }

    /// Flattens the tree over the area `area`, reserving `gap` pixels for
    /// each separator. Returns the absolute position of each panel and each
    /// handle.
    pub fn compute(&self, area: Rect, gap: f32) -> Layout {
        let mut out = Layout::default();
        let mut path = Vec::new();
        self.compute_into(area, gap, &mut path, &mut out);
        out
    }

    fn compute_into(&self, area: Rect, gap: f32, path: &mut NodePath, out: &mut Layout) {
        match self {
            LayoutNode::Leaf { panel } => out.panels.push(PanelGeom {
                panel: *panel,
                rect: area,
            }),
            LayoutNode::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let (a1, sep, a2) = split_area(area, *dir, *ratio, gap);
                out.splitters.push(SplitterGeom {
                    path: path.clone(),
                    dir: *dir,
                    rect: sep,
                    area,
                });
                path.push(Side::First);
                first.compute_into(a1, gap, path, out);
                path.pop();
                path.push(Side::Second);
                second.compute_into(a2, gap, path, out);
                path.pop();
            }
        }
    }
}
