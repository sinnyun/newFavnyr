use super::*;

#[test]
fn the_grid_packs_a_line_per_band_and_a_header_opens_a_new_one() {
    let style = grid_style(400.0);
    let mut rows: Vec<FileRow> = (0..7).map(|i| named_row(&format!("f{i}"))).collect();
    rows.insert(3, header_row("cat:image", "Images"));
    let cols = layout_rows(&mut rows, style);
    assert_eq!(cols, 3);

    // First line: three tiles sharing one band.
    assert_eq!(rows[0].visual_x, GRID_PAD);
    assert_eq!(rows[0].visual_y, 0.0);
    assert_eq!(
        rows[0].visual_h,
        zoom_to_height(THUMB_DEFAULT_ZOOM) + GRID_NAME_BAND
    );
    for i in 1..3 {
        assert_eq!(rows[i].visual_y, rows[0].visual_y, "a line shares one band");
        assert!(rows[i].visual_x > rows[i - 1].visual_x);
    }
    // The header spans the whole width and restarts the packing under it.
    assert_eq!((rows[3].visual_x, rows[3].visual_w), (0.0, 400.0));
    assert_eq!(rows[3].visual_h, SECTION_HEADER_H);
    assert!(rows[4].visual_y >= rows[3].visual_y + SECTION_HEADER_H);
    assert_eq!(rows[4].visual_x, GRID_PAD);
    // The invariant every binary search over the geometry rests on.
    assert!(
        rows.windows(2)
            .all(|pair| pair[1].visual_y >= pair[0].visual_y)
    );
    // A tile never sticks out of the list area, however many share a line.
    for width in [180.0_f32, 320.0, 640.0, 1280.0] {
        let mut probe: Vec<FileRow> = (0..12).map(|_| named_row("x")).collect();
        let cols = layout_rows(&mut probe, grid_style(width));
        assert!(cols >= 1);
        for row in &probe {
            assert!(
                row.visual_x + row.visual_w <= width + 0.01,
                "width={width} x={} w={}",
                row.visual_x,
                row.visual_w
            );
        }
    }
}

#[test]
fn the_grid_cursor_walks_lines_and_stops_at_a_header() {
    let mut rows = vec![header_row("cat:image", "Images")];
    rows.extend((0..6).map(|i| named_row(&format!("f{i}"))));
    rows.push(header_row("cat:other", "Other"));
    rows.push(named_row("f6"));
    layout_rows(&mut rows, grid_style(400.0));
    let model = VecModel::from(rows);
    // 0 = header, 1..=6 = f0..f5 (two lines of three), 7 = header, 8 = f6.
    assert_eq!(grid_neighbour(&model, 1, 1, 0), Some(2)); // right, same line
    assert_eq!(grid_neighbour(&model, 1, -1, 0), None); // the line's left edge
    assert_eq!(grid_neighbour(&model, 3, 1, 0), None); // the line's right edge
    assert_eq!(grid_neighbour(&model, 1, 0, 1), Some(4)); // down keeps the column
    assert_eq!(grid_neighbour(&model, 4, 0, -1), Some(1)); // and up comes back
    assert_eq!(grid_neighbour(&model, 4, 0, 1), None); // a header borders the line
    assert_eq!(grid_neighbour(&model, 8, 0, -1), None);
    assert_eq!(grid_neighbour(&model, 8, 0, 1), None);
}

#[test]
fn the_keyboard_cursor_never_rests_on_a_header() {
    let mut rows = vec![header_row("cat:image", "Images")];
    rows.extend((0..2).map(|i| named_row(&format!("f{i}"))));
    rows.push(header_row("cat:other", "Other"));
    rows.push(named_row("f2"));
    let model = VecModel::from(rows);
    assert_eq!(walk_entries(&model, 0, 1), Some(1)); // the header is stepped over
    assert_eq!(walk_entries(&model, 3, 1), Some(4)); // and so is the second one
    assert_eq!(walk_entries(&model, 2, 1), Some(2)); // an entry stays where it is
    assert_eq!(walk_entries(&model, 3, -1), Some(2));
    assert_eq!(walk_entries(&model, 4, 1), Some(4)); // the last entry
    assert_eq!(walk_entries(&model, 5, 1), None); // out of the model
    assert_eq!(walk_entries(&model, -1, 1), None);
}

#[test]
fn a_section_header_is_never_part_of_a_selection() {
    let mut rows = vec![header_row("cat:image", "Images")];
    rows.extend((0..2).map(|i| named_row(&format!("f{i}"))));
    rows.push(header_row("cat:other", "Other"));
    rows.push(named_row("f2"));
    let model = VecModel::from(rows);

    assert_eq!(selection_set_all(&model, true), 3);
    assert_eq!(count_selected(&model), 3);
    assert!(!model.row_data(0).unwrap().selected);
    assert!(!model.row_data(3).unwrap().selected);

    // A range or a band that covers a header selects the entries around it.
    assert_eq!(selection_set_range(&model, 0, 4), 3);
    let base = snapshot_selection(&model);
    assert_eq!(selection_apply_band(&model, 0, 4, 2, &base), 0); // Ctrl: remove
    assert_eq!(count_selected(&model), 0);
}

#[test]
fn a_subfolder_section_waits_pending_and_only_one_level_is_spanned() {
    let root = PathBuf::from("/gallery");
    let own = vec![
        test_entry("album", true),
        test_entry("zeta", true),
        test_entry("photo.png", false),
    ];
    let dirs = pending_subfolders(&root, &own);
    assert_eq!(dirs.len(), 2, "one section per DIRECT subfolder");
    assert!(dirs.iter().all(|dir| dir.pending && dir.entries.is_empty()));
    assert_eq!(dirs[0].path, root.join("album"));
    assert_eq!(
        sub_section_key(&dirs[0].path),
        sub_section_key(&root.join("album"))
    );
    assert!(sub_section_key(&dirs[0].path).starts_with("sub:"));
}

#[test]
fn the_category_grouping_heads_every_bucket_it_finds() {
    let mut own = vec![
        test_entry("notes.txt", false),
        test_entry("zulu", true),
        test_entry("photo.png", false),
        test_entry("misc.bin", false),
        test_entry("alpha", true),
    ];
    rfs::sort(
        &mut own,
        SortColumn::Name,
        SortOrder::Asc,
        GroupMode::Category,
    );
    let annotations = favnyr_core::annotations::AnnotationStore::default();
    let ctx = RowContext {
        lang: Lang::En,
        now_unix: 0,
        style: plain_style(LIST_DEFAULT_ZOOM, false),
        annotations: &annotations,
        subfolders: false,
    };
    let (rows, _) = build_rows(
        &RowsSource {
            root: PathBuf::from("/gallery"),
            own,
            dirs: Vec::new(),
        },
        GroupMode::Category,
        &ctx,
        &[],
    );
    let heads: Vec<String> = rows
        .iter()
        .filter(|row| row.role == ROW_ROLE_SECTION)
        .map(|row| row.section.to_string())
        .collect();
    assert_eq!(
        heads,
        ["cat:folder", "cat:image", "cat:document", "cat:other"]
    );
    // The count of a header is the localized "N items", and the entries
    // follow their header in the ranked order the sorter produced.
    let names: Vec<&str> = rows
        .iter()
        .filter(|row| row.role == ROW_ROLE_ENTRY)
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["alpha", "zulu", "photo.png", "notes.txt", "misc.bin"]
    );
    assert_eq!(
        rows[0].section_count_text,
        i18n::footer_items_text(Lang::En, 2)
    );
    assert_eq!(rows[0].section_label, i18n::tr(Lang::En, "category_folder"));
}
