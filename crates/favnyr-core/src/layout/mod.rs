//! Panel layout tree and nested split management.
//!
//! Favnyr displays an arbitrary number of panels organized into nested
//! horizontal/vertical splits, VSCode-style. Slint cannot instantiate
//! components **recursively**, so the representation is kept separate from
//! rendering:
//!
//!  1. the layout lives here, in Rust, as a **binary tree** ([`LayoutNode`])
//!     — easy to manipulate (split / merge / resize);
//!  2. at render time, the tree is **flattened** into a list of absolutely
//!     positioned rectangles ([`Layout`]) that the GUI places directly. This
//!     approach avoids any recursion in Slint repeaters.
//!
//! **Leaves** reference a panel by index into
//! [`crate::workspace::WorkspaceState::panels`]: the tree describes *where*
//! each panel goes, while the `Vec<PanelState>` describes *what* each panel
//! contains. This separation lets the GUI consume a flat list of panels.

use serde::{Deserialize, Serialize};

/// Two ratios closer than this are the same ratio: past a pixel or so of a
/// panel, a difference no one can see and no one asked for.
const RATIO_EPSILON: f32 = 1e-4;

/// Orientation of a split.
///
/// - [`SplitDir::Row`]: children **side by side** (**vertical** separator) —
///   this is the UI's "◧ Side by side".
/// - [`SplitDir::Column`]: children **stacked** (**horizontal** separator) —
///   this is the "⊟ One below another".
///
/// We deliberately ban "horizontal / vertical" from the exposed vocabulary
/// (ambiguous); these internal names describe the **axis along which
/// children are arranged**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    /// Children arranged horizontally (left → right).
    Row,
    /// Children stacked vertically (top → bottom).
    Column,
}

/// Node of the layout tree (recursive, **in-memory** representation).
///
/// Directly serializable to TOML: the scalar fields (`dir`, `ratio`) are
/// declared **before** the subtrees (`first`, `second`) to comply with the
/// TOML "values before tables" rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayoutNode {
    /// Leaf: an actual panel, designated by its index in `panels`.
    Leaf { panel: usize },
    /// Internal node: two children separated according to `dir`, `ratio`
    /// being the share (∈ ]0,1[) allotted to the **first** child.
    Split {
        dir: SplitDir,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

/// Step of a leaf's `panel` index during a traversal (see [`Side`]).
///
/// Path to a node from the root: a sequence of [`Side`]. The root has an
/// empty path.
pub type NodePath = Vec<Side>;

/// Side of a split, used to designate a child within a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// `first` child (left or top).
    First,
    /// `second` child (right or bottom).
    Second,
}

/// Rectangle in pixels (absolute coordinates within the panel container).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Geometry of a leaf panel after flattening.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelGeom {
    /// Index of the panel in `panels`.
    pub panel: usize,
    pub rect: Rect,
}

/// Geometry of a separator (resize handle) after flattening. `path`
/// identifies the corresponding `Split` node, to adjust its `ratio` during a
/// resize drag; `area` is the **total** area that this split divides
/// (needed to convert a pointer position into a ratio local to this split).
#[derive(Debug, Clone, PartialEq)]
pub struct SplitterGeom {
    pub path: NodePath,
    pub dir: SplitDir,
    pub rect: Rect,
    pub area: Rect,
}

/// Result of flattening a [`LayoutNode`] over a given area.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub panels: Vec<PanelGeom>,
    pub splitters: Vec<SplitterGeom>,
}

mod geom;
mod tree;

#[cfg(test)]
mod tests;

use geom::*;
