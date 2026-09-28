use super::*;

// Tab geometry -----

#[test]
fn tab_width_is_clamped_and_monotonic() {
    // Short title → min bound (90); very long → max bound (180).
    assert_eq!(estimate_tab_width("~"), 90.0);
    assert_eq!(
        estimate_tab_width("a-really-endless-folder-name-that-keeps-going"),
        180.0
    );
    // A longer title is never NARROWER (monotonicity).
    let w1 = estimate_tab_width("Documents");
    let w2 = estimate_tab_width("Documents-2024");
    assert!(w2 >= w1, "{w2} >= {w1}");
    // Intermediate zone: well within the bounds.
    assert!(w1 > 90.0 && w1 < 180.0, "{w1}");
}

#[test]
fn own_icon_only_for_self_iconed_types() {
    // Executables & co: EMBEDDED icon → "by path" route.
    for ext in ["exe", "scr", "cpl", "ico"] {
        assert!(has_own_icon(ext), "{ext} should carry its own icon");
    }
    // Ordinary types: "by extension" route (cached, no I/O).
    for ext in ["txt", "png", "pdf", "docx", "lnk", ""] {
        assert!(!has_own_icon(ext), "{ext} should NOT trigger per-file I/O");
    }
}

#[test]
fn preview_capability_and_compact_height_share_the_same_rules() {
    assert_eq!(
        thumbnail_kind_for_row(FileKind::Image.as_i32(), "svg"),
        Some((FileKind::Image, true))
    );
    for ext in ["af", "afdesign", "afphoto", "afpub"] {
        assert_eq!(
            thumbnail_kind_for_row(FileKind::Image.as_i32(), ext),
            Some((FileKind::Image, false)),
            "{ext}"
        );
    }
    assert_eq!(
        thumbnail_kind_for_row(FileKind::Document.as_i32(), "pdf"),
        Some((FileKind::Document, false))
    );
    assert_eq!(
        thumbnail_kind_for_row(FileKind::Audio.as_i32(), "mp3"),
        Some((FileKind::Audio, false))
    );
    assert_eq!(
        thumbnail_kind_for_row(FileKind::Audio.as_i32(), "FLAC"),
        Some((FileKind::Audio, false))
    );
    // Folders never request a preview, on any platform.
    assert!(thumbnail_kind_for_row(FileKind::Folder.as_i32(), "").is_none());
    // A document Favnyr can't render itself: on Windows it is handed to the
    // shell (which decides whether it has a thumbnail), elsewhere skipped.
    let docx = thumbnail_kind_for_row(FileKind::Document.as_i32(), "docx");
    #[cfg(windows)]
    assert_eq!(docx, Some((FileKind::Document, false)));
    #[cfg(not(windows))]
    assert!(docx.is_none());

    assert_eq!(effective_row_height(true, THUMB_DEFAULT_ZOOM, true), 76.0);
    assert_eq!(effective_row_height(false, THUMB_DEFAULT_ZOOM, true), 28.0);
    assert_eq!(effective_row_height(false, THUMB_DEFAULT_ZOOM, false), 76.0);
    // The setting never alters the list mode's levels.
    assert_eq!(effective_row_height(false, LIST_DEFAULT_ZOOM, true), 28.0);

    // Zoom scale: the LIST is a single-level floor and the
    // FIRST notch upward enters thumbnail mode. (The purely
    // constant invariants are verified at COMPILE TIME near the `const`s, see `_`.)
    assert_eq!(zoom_to_height(LIST_DEFAULT_ZOOM), 28.0); // list (floor)
    assert_eq!(zoom_to_height(THUMB_ZOOM), 52.0); // 1st thumbnail
    assert_eq!(zoom_to_height(MAX_ZOOM), 220.0); // sharp upper bound
}

#[test]
fn zoom_keeps_the_single_visible_selection_at_the_same_screen_position() {
    let panel = Panel::with_mode(PathBuf::from("gallery"), columns::default_columns(), 0);
    let mut rows: Vec<FileRow> = (0..80)
        .map(|index| FileRow {
            name: format!("image-{index:02}.png").into(),
            preview_capable: true,
            selected: index == 22,
            ..Default::default()
        })
        .collect();
    layout_rows(
        &mut rows,
        RowStyle {
            mode: ViewMode::Previews,
            zoom: THUMB_DEFAULT_ZOOM,
            compact_icon_rows: false,
            width: 0.0,
        },
    );
    let viewport = ZoomViewport {
        top: rows[20].visual_y + 10.0,
        height: 420.0,
        // Deliberately points at another row: the single visible
        // selection must take priority.
        pointer_y: 15.0,
    };
    let anchor =
        capture_zoom_anchor(&rows, viewport.top, viewport.height, viewport.pointer_y).unwrap();
    assert_eq!(anchor.row_index, 22);
    panel.viewport_top.set(viewport.top);
    panel.viewport_height.set(viewport.height);
    panel.replace_rows(rows);

    let style = RowStyle {
        mode: ViewMode::Previews,
        zoom: MAX_ZOOM,
        compact_icon_rows: false,
        width: 0.0,
    };
    let new_top = zoom_panel_visuals(&panel, style, false, viewport);
    let selected = panel.rows_model.row_data(anchor.row_index).unwrap();
    let selected_anchor_y = selected.visual_y + selected.visual_h * anchor.row_fraction - new_top;
    assert!((selected_anchor_y - anchor.viewport_y).abs() < 0.01);
    assert!(selected_anchor_y >= 0.0 && selected_anchor_y <= viewport.height);
    assert!((panel.viewport_top.get() - new_top).abs() < 0.01);
    assert_eq!(
        (panel.rendered_first.get(), panel.rendered_end.get()),
        row_range_for_content_span(
            &*panel.rows_model,
            (new_top - viewport.height).max(0.0),
            new_top + viewport.height * 2.0,
        )
    );
}

#[test]
fn zoom_uses_the_pointer_for_multiple_or_offscreen_selections() {
    let mut rows: Vec<FileRow> = (0..100)
        .map(|index| FileRow {
            preview_capable: index % 2 == 0,
            selected: matches!(index, 2 | 31),
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    let top = rows[30].visual_y;
    let height = 360.0;
    let pointer_y = 95.0;
    let old_content_y = top + pointer_y;
    let expected_row = row_index_at_content_y_slice(&rows, old_content_y).unwrap();
    let anchor = capture_zoom_anchor(&rows, top, height, pointer_y).unwrap();
    assert_eq!(anchor.row_index, expected_row);

    layout_at(&mut rows, MAX_ZOOM, true);
    let new_top = restore_zoom_viewport_top(&rows, Some(anchor), top, height);
    let row = &rows[anchor.row_index];
    let restored_pointer_y = row.visual_y + row.visual_h * anchor.row_fraction - new_top;
    assert!((restored_pointer_y - pointer_y).abs() < 0.01);

    // Same policy with a single OFF-screen selection: no jump to
    // it, the user keeps the zone they're actually looking at.
    for row in &mut rows {
        row.selected = false;
    }
    rows[2].selected = true;
    let anchor = capture_zoom_anchor(&rows, new_top, height, pointer_y).unwrap();
    assert_ne!(anchor.row_index, 2);
    assert_eq!(
        anchor.row_index,
        row_index_at_content_y_slice(&rows, new_top + pointer_y).unwrap()
    );
}

#[test]
fn zoom_falls_back_to_the_view_center_and_clamps_short_content() {
    let mut rows: Vec<FileRow> = (0..10)
        .map(|_| FileRow {
            preview_capable: true,
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, LIST_DEFAULT_ZOOM, false);
    let viewport_height = 500.0;
    // The pointer is in the blank space below the 280 px of content; the center
    // (250 px), however, stays on a valid row.
    let anchor = capture_zoom_anchor(&rows, 0.0, viewport_height, 450.0).unwrap();
    assert_eq!(anchor.viewport_y, viewport_height * 0.5);

    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, false);
    let top = restore_zoom_viewport_top(&rows, Some(anchor), 0.0, viewport_height);
    let row = &rows[anchor.row_index];
    let max_top = rows.last().unwrap().visual_y + rows.last().unwrap().visual_h - viewport_height;
    assert!((top - max_top).abs() < 0.01);
    let anchored_row_y = row.visual_y + row.visual_h * anchor.row_fraction - top;
    assert!(anchored_row_y >= 0.0 && anchored_row_y <= viewport_height);

    // If the content fits in the view again, no negative position or
    // one above the maximum can leak out to the Slint scrollbar.
    layout_at(&mut rows, LIST_DEFAULT_ZOOM, false);
    assert_eq!(
        restore_zoom_viewport_top(&rows, Some(anchor), 9_999.0, viewport_height),
        0.0
    );
}

#[test]
fn variable_row_geometry_drives_hit_test_and_rubber_band() {
    let mut rows = vec![
        FileRow {
            preview_capable: true,
            ..Default::default()
        },
        FileRow {
            preview_capable: false,
            ..Default::default()
        },
        FileRow {
            preview_capable: true,
            ..Default::default()
        },
    ];
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    let model = VecModel::from(rows);

    assert_eq!(row_index_at_content_y(&model, 75.0), 0);
    assert_eq!(row_index_at_content_y(&model, 76.0), 1);
    assert_eq!(row_index_at_content_y(&model, 103.0), 1);
    assert_eq!(row_index_at_content_y(&model, 104.0), 2);
    assert_eq!(row_index_at_content_y(&model, 180.0), -1);
    assert_eq!(row_band_for_content_range(&model, 70.0, 110.0), (0, 2));
}

#[test]
fn variable_row_binary_search_matches_a_linear_oracle_far_from_the_top() {
    let mut rows: Vec<FileRow> = (0..4_000)
        .map(|index| FileRow {
            preview_capable: index % 3 != 1,
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    let total_height = rows.last().map(|row| row.visual_y + row.visual_h).unwrap();
    let model = VecModel::from(rows.clone());

    // Samples every zone of the gallery, including exactly
    // at the heterogeneous boundaries where rounding errors appear.
    let mut samples = vec![-1.0, 0.0, total_height, total_height + 1.0];
    for row in rows.iter().step_by(37) {
        samples.extend([
            row.visual_y,
            row.visual_y + row.visual_h * 0.5,
            row.visual_y + row.visual_h,
        ]);
    }
    for y in samples {
        let expected = rows
            .iter()
            .position(|row| y >= row.visual_y && y < row.visual_y + row.visual_h)
            .map(|index| index as i32)
            .unwrap_or(-1);
        assert_eq!(row_index_at_content_y(&model, y), expected, "y={y}");
    }
}

#[test]
fn render_window_is_exact_bounded_and_tracks_a_far_scroll() {
    let mut rows: Vec<FileRow> = (0..10_000)
        .map(|index| FileRow {
            preview_capable: index % 2 == 0,
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    let top = rows[8_500].visual_y;
    let height = 720.0;
    let expected = row_range_for_slice(&rows, top - height, top + height * 2.0);
    let actual = mark_render_window(&mut rows, top, height);

    assert_eq!(actual, expected);
    assert!(actual.0 > 8_400, "the window must follow a distant scroll");
    assert!(
        actual.1 - actual.0 < 80,
        "the overscan must never materialize all 10,000 rows"
    );
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(
            row.rendered,
            index >= actual.0 && index < actual.1,
            "index={index}"
        );
    }

    let changed = render_window_changed_indices(0, 48, actual.0, actual.1);
    assert_eq!(
        changed.len(),
        48 + actual.1 - actual.0,
        "a distant jump must visit only the old and the new window"
    );
    assert_eq!(
        render_window_changed_indices(10, 20, 15, 25),
        vec![10, 11, 12, 13, 14, 20, 21, 22, 23, 24],
        "a normal scroll must not notify the shared area twice"
    );
}

#[test]
fn filtered_render_model_keeps_logical_indexes_and_selection_in_sync() {
    let (rows_model, rendered_model) = new_row_models();
    let mut rows: Vec<FileRow> = (0..120)
        .map(|index| FileRow {
            name: format!("row-{index}").into(),
            preview_capable: index % 2 == 0,
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    let top = rows[80].visual_y;
    let (first, end) = mark_render_window(&mut rows, top, 300.0);
    rows_model.set_vec(rows);

    assert_eq!(rendered_model.row_count(), end - first);
    for visible in 0..rendered_model.row_count() {
        let row = rendered_model.row_data(visible).unwrap();
        assert_eq!(row.model_index, (first + visible) as i32);
    }

    let logical = first + (end - first) / 2;
    let mut row = rows_model.row_data(logical).unwrap();
    row.selected = true;
    rows_model.set_row_data(logical, row);
    assert!(
        rendered_model
            .row_data(logical - first)
            .is_some_and(|row| row.selected)
    );
}

#[test]
fn changing_context_resets_the_exact_viewport_before_replacing_rows() {
    let panel = Panel::with_mode(PathBuf::from("old"), columns::default_columns(), 0);
    let mut rows: Vec<FileRow> = (0..2_000)
        .map(|index| FileRow {
            preview_capable: index % 2 == 0,
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    panel.viewport_top.set(rows[1_500].visual_y);
    panel.viewport_height.set(600.0);
    panel.replace_rows(rows.clone());
    assert!(panel.rendered_first.get() > 1_400);

    let previous_gen = panel.viewport_reset_gen.get();
    panel.reset_rows_viewport();
    panel.replace_rows(rows);
    assert_eq!(panel.viewport_top.get(), 0.0);
    assert_eq!(panel.rendered_first.get(), 0);
    assert_eq!(panel.viewport_reset_gen.get(), previous_gen.wrapping_add(1));
}

#[test]
fn heterogeneous_rows_do_not_change_multi_selection_semantics() {
    let mut rows: Vec<FileRow> = (0..8)
        .map(|index| FileRow {
            preview_capable: index % 2 == 0,
            selected: matches!(index, 1 | 5 | 7),
            ..Default::default()
        })
        .collect();
    layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
    let model = VecModel::from(rows);
    let base = snapshot_selection(&model);

    assert_eq!(selection_apply_band(&model, 2, 4, 1, &base), 6);
    assert_eq!(
        snapshot_selection(&model),
        vec![false, true, true, true, true, true, false, true]
    );
    assert_eq!(selection_apply_band(&model, 2, 5, 2, &base), 2);
    assert_eq!(
        snapshot_selection(&model),
        vec![false, true, false, false, false, false, false, true]
    );
}

/// A delegate can only be corrected by a row being written back, and a
/// write only happens for a row the rule accepts. On screen the rule must
/// therefore accept even an unchanged row — that re-push is the one chance
/// a display left out of step has of catching up. Off screen it must keep
/// refusing, or a hundred-thousand-entry folder would pay for every move.
#[test]
fn a_row_on_screen_is_pushed_back_even_when_nothing_changed() {
    let on_screen = FileRow {
        selected: false,
        rendered: true,
        ..Default::default()
    };
    let off_screen = FileRow {
        selected: false,
        rendered: false,
        ..Default::default()
    };

    assert!(
        selection_needs_write(&on_screen, false),
        "unchanged but visible: re-pushed so a stale delegate can catch up"
    );
    assert!(
        !selection_needs_write(&off_screen, false),
        "unchanged and invisible: nothing to correct, nothing to pay"
    );
    // A real change is always written, wherever the row sits.
    assert!(selection_needs_write(&off_screen, true));
    assert!(selection_needs_write(&on_screen, true));
}

#[test]
fn collapsing_a_multiple_selection_is_told_apart_from_re_clicking_one_row() {
    // The distinction the deferred rename hangs on. On the clicked row the
    // two cases look identical — selected before, selected after — so only
    // "did anything else change" separates them.
    let rows: Vec<FileRow> = (0..5)
        .map(|index| FileRow {
            selected: matches!(index, 1..=3),
            ..Default::default()
        })
        .collect();
    let model = VecModel::from(rows);

    // Clicking one member of a multiple selection DROPS the others: the
    // user is narrowing down, not asking to edit a name.
    let (count, was_alone) = selection_set_only(&model, 2);
    assert_eq!(count, 1);
    assert!(
        !was_alone,
        "the click removed rows 1 and 3 from the selection"
    );
    assert_eq!(
        snapshot_selection(&model),
        vec![false, false, true, false, false]
    );

    // Clicking it AGAIN changes nothing — that is the second click Explorer
    // treats as "rename this".
    let (count, was_alone) = selection_set_only(&model, 2);
    assert_eq!(count, 1);
    assert!(was_alone);

    // And clicking a different, unselected row is a plain new selection.
    let (_, was_alone) = selection_set_only(&model, 4);
    assert!(!was_alone);
}
