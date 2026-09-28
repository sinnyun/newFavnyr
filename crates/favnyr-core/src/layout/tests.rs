use super::*;

fn approx(a: f32, b: f32) {
    assert!((a - b).abs() < 0.5, "expected ~{b}, got {a}");
}

fn area() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        w: 1000.0,
        h: 600.0,
    }
}

fn leaf(panel: usize) -> LayoutNode {
    LayoutNode::Leaf { panel }
}

fn split(dir: SplitDir, ratio: f32, first: LayoutNode, second: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        dir,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

/// Panel sizes over the reference area, indexed by panel number.
fn sizes(tree: &LayoutNode, gap: f32) -> Vec<(f32, f32)> {
    let l = tree.compute(area(), gap);
    let mut out = vec![(0.0, 0.0); l.panels.len()];
    for p in &l.panels {
        out[p.panel] = (p.rect.w, p.rect.h);
    }
    out
}

/// The area a given separator governs, as the GUI reads it.
fn area_of(tree: &LayoutNode, path: &[Side], gap: f32) -> Rect {
    tree.compute(area(), gap)
        .splitters
        .iter()
        .find(|s| s.path == path)
        .expect("separator at that path")
        .area
}

#[test]
fn single_leaf_fills_area() {
    let tree = LayoutNode::Leaf { panel: 0 };
    let l = tree.compute(area(), 8.0);
    assert_eq!(l.panels.len(), 1);
    assert!(l.splitters.is_empty());
    assert_eq!(l.panels[0].rect, area());
}

#[test]
fn row_split_halves_minus_gap() {
    let tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Leaf { panel: 1 }),
    };
    let l = tree.compute(area(), 8.0);
    assert_eq!(l.panels.len(), 2);
    assert_eq!(l.splitters.len(), 1);
    // avail = 992, half = 496
    approx(l.panels[0].rect.w, 496.0);
    approx(l.panels[1].rect.w, 496.0);
    approx(l.panels[0].rect.h, 600.0);
    // no overlap, separator between the two
    approx(l.splitters[0].rect.x, 496.0);
    approx(l.splitters[0].rect.w, 8.0);
    approx(l.panels[1].rect.x, 504.0);
    assert_eq!(l.splitters[0].dir, SplitDir::Row);
}

#[test]
fn column_split_stacks() {
    let tree = LayoutNode::Split {
        dir: SplitDir::Column,
        ratio: 0.25,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Leaf { panel: 1 }),
    };
    let l = tree.compute(area(), 8.0);
    // avail = 592, first = 148
    approx(l.panels[0].rect.h, 148.0);
    approx(l.panels[1].rect.h, 444.0);
    approx(l.panels[0].rect.w, 1000.0);
    approx(l.splitters[0].rect.y, 148.0);
    approx(l.splitters[0].rect.h, 8.0);
    assert_eq!(l.splitters[0].dir, SplitDir::Column);
}

#[test]
fn nested_split_geometry() {
    // Root Row 0.5; right child re-split into Column 0.5.
    let tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Split {
            dir: SplitDir::Column,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf { panel: 1 }),
            second: Box::new(LayoutNode::Leaf { panel: 2 }),
        }),
    };
    let l = tree.compute(area(), 8.0);
    assert_eq!(l.panels.len(), 3);
    assert_eq!(l.splitters.len(), 2);
    // panel 0 = left half, full height
    let p0 = l.panels.iter().find(|p| p.panel == 0).unwrap();
    approx(p0.rect.w, 496.0);
    approx(p0.rect.h, 600.0);
    // panels 1 and 2 = right half, stacked (≈ (600-8)/2)
    let p1 = l.panels.iter().find(|p| p.panel == 1).unwrap();
    let p2 = l.panels.iter().find(|p| p.panel == 2).unwrap();
    approx(p1.rect.w, 496.0);
    approx(p1.rect.h, 296.0);
    approx(p2.rect.h, 296.0);
    approx(p1.rect.x, 504.0);
}

#[test]
fn row_chain_matches_stretches() {
    // 3 panels with stretch 2,1,1 → the first one takes half.
    let tree = LayoutNode::row_chain(&[2.0, 1.0, 1.0]);
    assert_eq!(tree.leaf_indices(), vec![0, 1, 2]);
    let l = tree.compute(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 408.0,
            h: 100.0,
        },
        8.0,
    );
    // total stretch = 4; gaps = 2*8 = 16; remainder 392.
    // p0 = 2/4 ≈ 196 (at the first level ratio=0.5 over 400 available)
    let p0 = l.panels.iter().find(|p| p.panel == 0).unwrap();
    approx(p0.rect.w, 200.0);
}

#[test]
fn validity_checks() {
    let good = LayoutNode::row_chain(&[1.0, 1.0, 1.0]);
    assert!(good.is_valid_for(3));
    assert!(!good.is_valid_for(2)); // index 2 out of bounds
    assert!(!good.is_valid_for(4)); // panel 3 missing

    let dup = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Leaf { panel: 0 }),
    };
    assert!(!dup.is_valid_for(1));
}

#[test]
fn set_ratio_via_path() {
    let mut tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Split {
            dir: SplitDir::Column,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf { panel: 1 }),
            second: Box::new(LayoutNode::Leaf { panel: 2 }),
        }),
    };
    // root
    assert!(tree.set_ratio(&[], 0.7, 0.05));
    // Column node = Second child of the root
    assert!(tree.set_ratio(&[Side::Second], 0.3, 0.05));
    // path to a leaf → failure
    assert!(!tree.set_ratio(&[Side::First], 0.5, 0.05));

    if let LayoutNode::Split { ratio, second, .. } = &tree {
        approx(*ratio, 0.7);
        if let LayoutNode::Split { ratio: r2, .. } = second.as_ref() {
            approx(*r2, 0.3);
        } else {
            panic!("expected Split");
        }
    } else {
        panic!("expected Split");
    }
}

#[test]
fn ratio_clamped() {
    let mut tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Leaf { panel: 1 }),
    };
    tree.set_ratio(&[], 0.001, 0.05);
    if let LayoutNode::Split { ratio, .. } = &tree {
        approx(*ratio, 0.05);
    }
    tree.set_ratio(&[], 0.999, 0.05);
    if let LayoutNode::Split { ratio, .. } = &tree {
        approx(*ratio, 0.95);
    }
}

#[test]
fn split_leaf_creates_split() {
    let mut tree = LayoutNode::Leaf { panel: 0 };
    assert!(tree.split_leaf(0, SplitDir::Row, 1, 0.5, false));
    assert!(tree.is_valid_for(2));
    assert_eq!(tree.leaf_indices(), vec![0, 1]);
    assert!(matches!(
        tree,
        LayoutNode::Split {
            dir: SplitDir::Row,
            ..
        }
    ));

    // Re-split panel 1 (nested leaf) into Column.
    assert!(tree.split_leaf(1, SplitDir::Column, 2, 0.5, false));
    assert!(tree.is_valid_for(3));
    assert_eq!(tree.leaf_indices(), vec![0, 1, 2]);

    // Non-existent panel → false.
    assert!(!tree.split_leaf(9, SplitDir::Row, 3, 0.5, false));
}

#[test]
fn split_leaf_new_first_puts_new_panel_first() {
    let mut tree = LayoutNode::Leaf { panel: 0 };
    assert!(tree.split_leaf(0, SplitDir::Row, 1, 0.5, true));
    // new_first → the new panel (1) is the FIRST child.
    assert_eq!(tree.leaf_indices(), vec![1, 0]);
}

#[test]
fn remove_panel_collapses_and_reindexes() {
    // Chain of 3 panels: leaves [0,1,2].
    let mut tree = LayoutNode::row_chain(&[1.0, 1.0, 1.0]);
    // Remove panel 1 → [0, 2] remain, reindexed as [0, 1].
    assert!(tree.remove_panel(1));
    assert!(tree.is_valid_for(2));
    assert_eq!(tree.leaf_indices(), vec![0, 1]);

    // Remove again → 1 leaf.
    assert!(tree.remove_panel(0));
    assert!(tree.is_valid_for(1));
    assert_eq!(tree.leaf_indices(), vec![0]);

    // Last panel: refused.
    assert!(!tree.remove_panel(0));
}

#[test]
fn remove_panel_keeps_sibling_subtree() {
    // Row { 0, Column { 1, 2 } }; removing 0 → only Column{1,2} remains,
    // reindexed as Column{0,1}.
    let mut tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Split {
            dir: SplitDir::Column,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf { panel: 1 }),
            second: Box::new(LayoutNode::Leaf { panel: 2 }),
        }),
    };
    assert!(tree.remove_panel(0));
    assert!(tree.is_valid_for(2));
    assert_eq!(tree.leaf_indices(), vec![0, 1]);
    assert!(matches!(
        tree,
        LayoutNode::Split {
            dir: SplitDir::Column,
            ..
        }
    ));
}

#[test]
fn splitter_carries_area() {
    let tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Leaf { panel: 1 }),
    };
    let l = tree.compute(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        0.0,
    );
    // Full fractional area, separator halfway.
    approx(l.splitters[0].area.w, 1.0);
    approx(l.splitters[0].rect.x, 0.5);
}

#[test]
fn toml_round_trip() {
    let tree = LayoutNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Split {
            dir: SplitDir::Column,
            ratio: 0.6,
            first: Box::new(LayoutNode::Leaf { panel: 1 }),
            second: Box::new(LayoutNode::Leaf { panel: 2 }),
        }),
    };
    let s = toml::to_string_pretty(&tree).expect("serialize");
    let back: LayoutNode = toml::from_str(&s).expect("parse");
    assert_eq!(back, tree);
}

/// One view split in two, then the right half split again: the three come
/// out the same width, not a half and two quarters.
#[test]
fn equalize_gives_every_panel_the_same_width() {
    let mut tree = split(
        SplitDir::Row,
        0.5,
        leaf(0),
        split(SplitDir::Row, 0.5, leaf(1), leaf(2)),
    );
    assert!(tree.equalize(&[], area(), 8.0, 0.08).is_some());
    let s = sizes(&tree, 8.0);
    approx(s[0].0, s[1].0);
    approx(s[1].0, s[2].0);
    // Nothing lost on the way: three panels and two separators fill the area.
    approx(s[0].0 + s[1].0 + s[2].0 + 16.0, area().w);
}

/// Three panels in a row can be held by a tree leaning either way, and the
/// user cannot tell which. Both must even out to the same thing.
#[test]
fn equalize_ignores_which_way_the_tree_leans() {
    let mut right = split(
        SplitDir::Row,
        0.5,
        leaf(0),
        split(SplitDir::Row, 0.5, leaf(1), leaf(2)),
    );
    let mut left = split(
        SplitDir::Row,
        0.5,
        split(SplitDir::Row, 0.5, leaf(0), leaf(1)),
        leaf(2),
    );
    right.equalize(&[], area(), 8.0, 0.08);
    left.equalize(&[], area(), 8.0, 0.08);
    for (a, b) in sizes(&right, 8.0).iter().zip(sizes(&left, 8.0)) {
        approx(a.0, b.0);
    }
}

/// A panel beside two stacked ones: the reading kept is "same width", so
/// the three are equally wide and the stacked pair shares the height.
#[test]
fn equalize_keeps_widths_equal_across_a_stack() {
    let mut tree = split(
        SplitDir::Row,
        0.8,
        leaf(0),
        split(SplitDir::Column, 0.3, leaf(1), leaf(2)),
    );
    tree.equalize(&[], area(), 8.0, 0.08);
    let s = sizes(&tree, 8.0);
    approx(s[0].0, s[1].0);
    approx(s[1].0, s[2].0);
    approx(s[1].1, s[2].1);
    approx(s[0].1, area().h);
}

/// A separator evens out only what it governs: proportions set by hand
/// elsewhere in the tree stay put.
#[test]
fn equalize_below_the_root_leaves_the_rest_alone() {
    let mut tree = split(
        SplitDir::Row,
        0.8,
        leaf(0),
        split(SplitDir::Row, 0.2, leaf(1), leaf(2)),
    );
    let nested = area_of(&tree, &[Side::Second], 8.0);
    assert!(tree.equalize(&[Side::Second], nested, 8.0, 0.08).is_some());
    let s = sizes(&tree, 8.0);
    approx(s[1].0, s[2].0);
    approx(s[0].0, (area().w - 8.0) * 0.8);
}

/// Nothing moved means nothing to put back: the caller is told so and
/// leaves no stale way back armed.
#[test]
fn equalize_reports_nothing_when_already_even() {
    let mut tree = split(SplitDir::Row, 0.5, leaf(0), leaf(1));
    assert!(tree.equalize(&[], area(), 8.0, 0.08).is_none());
}

#[test]
fn restore_ratios_puts_back_what_equalize_took() {
    let mut tree = split(
        SplitDir::Row,
        0.8,
        leaf(0),
        split(SplitDir::Row, 0.2, leaf(1), leaf(2)),
    );
    let (before, _) = tree.equalize(&[], area(), 8.0, 0.08).expect("layout moved");
    assert!(tree.restore_ratios(&[], &before));
    approx(sizes(&tree, 8.0)[0].0, (area().w - 8.0) * 0.8);
}

/// A view closed under the copy leaves one split fewer: the saved ratios no
/// longer describe this tree, and a half-applied restore is refused whole.
#[test]
fn restore_ratios_refuses_a_shape_that_changed() {
    let mut tree = split(
        SplitDir::Row,
        0.8,
        leaf(0),
        split(SplitDir::Row, 0.2, leaf(1), leaf(2)),
    );
    let (before, _) = tree.equalize(&[], area(), 8.0, 0.08).expect("layout moved");
    assert!(tree.remove_panel(2));
    assert!(!tree.restore_ratios(&[], &before));
}

/// The canonical case: four views in a 2x2, the two on top change places.
/// Their sizes are equal, so what moves is the content, not the geometry.
#[test]
fn swap_panels_exchanges_two_places() {
    let mut tree = split(
        SplitDir::Column,
        0.5,
        split(SplitDir::Row, 0.5, leaf(0), leaf(1)),
        split(SplitDir::Row, 0.5, leaf(2), leaf(3)),
    );
    let before = tree.compute(area(), 8.0);
    assert!(tree.swap_panels(0, 1));
    let after = tree.compute(area(), 8.0);
    let place = |l: &Layout, panel: usize| {
        l.panels
            .iter()
            .find(|p| p.panel == panel)
            .expect("panel")
            .rect
    };
    assert_eq!(place(&after, 0), place(&before, 1));
    assert_eq!(place(&after, 1), place(&before, 0));
    // The two left untouched really are untouched.
    assert_eq!(place(&after, 2), place(&before, 2));
    assert_eq!(place(&after, 3), place(&before, 3));
}

/// Views of different sizes: each takes the size of its new place. That is
/// the whole point of swapping places rather than contents.
#[test]
fn swap_panels_hands_over_the_size_of_the_new_place() {
    let mut tree = split(SplitDir::Row, 0.8, leaf(0), leaf(1));
    let wide = sizes(&tree, 8.0)[0].0;
    let narrow = sizes(&tree, 8.0)[1].0;
    assert!(tree.swap_panels(0, 1));
    approx(sizes(&tree, 8.0)[0].0, narrow);
    approx(sizes(&tree, 8.0)[1].0, wide);
}

/// The shape and the proportions are untouched: only two indices move.
#[test]
fn swap_panels_leaves_the_tree_and_its_ratios_alone() {
    let mut tree = split(
        SplitDir::Row,
        0.7,
        leaf(0),
        split(SplitDir::Column, 0.3, leaf(1), leaf(2)),
    );
    let ratios = tree.ratios(&[]);
    assert!(tree.swap_panels(0, 2));
    assert_eq!(tree.ratios(&[]), ratios);
    assert!(tree.is_valid_for(3));
}

/// Doing it twice puts everything back — the operation is its own undo.
#[test]
fn swap_panels_twice_restores() {
    let mut tree = split(
        SplitDir::Row,
        0.7,
        leaf(0),
        split(SplitDir::Column, 0.3, leaf(1), leaf(2)),
    );
    let before = tree.clone();
    assert!(tree.swap_panels(0, 2));
    assert!(tree.swap_panels(0, 2));
    assert_eq!(tree, before);
}

/// A view dropped on itself, and an index with no leaf: refused, and above
/// all nothing written — a half-applied swap would hold one index twice.
#[test]
fn swap_panels_refuses_what_it_cannot_do_whole() {
    let mut tree = split(SplitDir::Row, 0.5, leaf(0), leaf(1));
    let before = tree.clone();
    assert!(!tree.swap_panels(1, 1));
    assert_eq!(tree, before);
    assert!(!tree.swap_panels(0, 9));
    assert_eq!(tree, before);
    assert!(tree.is_valid_for(2));
}
