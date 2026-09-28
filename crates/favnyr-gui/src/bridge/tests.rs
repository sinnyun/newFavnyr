use super::*;

mod geometry;
mod grid;
mod opening;
mod openwith;
mod opregistry;
mod thumbnails;
mod workspace;

/// A list/previews style: the two modes lay their rows out identically, so
/// the geometry tests only name the zoom and the compact flag.
fn plain_style(zoom: i32, compact_icon_rows: bool) -> RowStyle {
    RowStyle {
        mode: ViewMode::List,
        zoom,
        compact_icon_rows,
        width: 0.0,
    }
}

/// Lays rows out for the geometry tests (see `plain_style`).
fn layout_at(rows: &mut [FileRow], zoom: i32, compact_icon_rows: bool) -> i32 {
    layout_rows(rows, plain_style(zoom, compact_icon_rows))
}

/// Stand-in for the user's per-extension defaults.
fn opener_for(ext: &str) -> Option<String> {
    match ext {
        "txt" | "md" => Some("editor".to_string()),
        "png" => Some("viewer".to_string()),
        _ => None,
    }
}

fn files(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|n| PathBuf::from("/somewhere").join(n))
        .collect()
}

fn op_handle(cancel: Option<Arc<AtomicBool>>) -> OpHandle {
    OpHandle {
        cancel,
        pending_focus: None,
        targets: Vec::new(),
        thumbnail_invalidations: Vec::new(),
        transient_cleanup: None,
    }
}

fn op_handle_writing(targets: &[&str]) -> OpHandle {
    OpHandle {
        cancel: None,
        pending_focus: None,
        targets: targets.iter().map(PathBuf::from).collect(),
        thumbnail_invalidations: Vec::new(),
        transient_cleanup: None,
    }
}

#[test]
fn a_batch_never_sends_two_items_to_the_same_destination() {
    // `x` and `x - Copy01` both reduce to the base `x`, so picking their
    // names independently hands both `x - Copy02` — one result would
    // overwrite the other. Nothing exists on disk here, so only the
    // batch's own bookkeeping can separate them.
    let dir = std::env::temp_dir().join("favnyr-batch-target-test");
    let sources = vec![dir.join("notes.txt"), dir.join("notes - Copy01.txt")];

    let picked = plan_unique_targets(&sources, |_| false);
    assert_eq!(picked.len(), 2);
    assert_ne!(
        picked[0], picked[1],
        "two items of one batch must not share a destination"
    );

    // Names already spoken for are stepped over as well.
    let blocked = plan_unique_targets(&sources[..1], |p| {
        p.file_name().is_some_and(|n| n == "notes.txt")
    });
    assert_ne!(blocked[0], sources[0]);
}

#[test]
fn a_reservation_only_takes_a_name_that_was_otherwise_free() {
    use EntryNameAvailability as A;
    // Nothing on disk, but a running operation will write there.
    assert_eq!(with_reservation(A::Available, true), A::Reserved);
    assert_eq!(with_reservation(A::Available, false), A::Available);
    // A real entry keeps its own kind: it is what the dialogs report on,
    // and masking it would hide the "replace this file" choice.
    assert_eq!(
        with_reservation(A::ExistingNonDirectory, true),
        A::ExistingNonDirectory
    );
    assert_eq!(
        with_reservation(A::ExistingDirectory, true),
        A::ExistingDirectory
    );
    // A syntactically invalid name stays invalid whatever is reserved.
    assert_eq!(with_reservation(A::Invalid, true), A::Invalid);
}

#[test]
fn renaming_onto_a_reserved_name_never_offers_force_replace() {
    use RenameNameStatus as R;
    // No entry exists there to replace, and the running operation would
    // overwrite the renamed item as soon as it reached that path.
    assert_eq!(rename_with_reservation(R::Valid, true), R::Conflict);
    assert_eq!(rename_with_reservation(R::Valid, false), R::Valid);
    // A real file conflict keeps offering the replace route.
    assert_eq!(
        rename_with_reservation(R::ReplaceableFile, false),
        R::ReplaceableFile
    );
    assert_eq!(rename_with_reservation(R::Invalid, true), R::Invalid);
}

#[test]
fn toast_stack_summarises_only_what_it_cannot_show() {
    // Up to the cap every operation gets its own card, so no chip.
    assert_eq!(hidden_toast_count(0), 0);
    assert_eq!(hidden_toast_count(MAX_VISIBLE_TOASTS), 0);
    // Past it, the chip accounts for exactly the ones left out.
    assert_eq!(hidden_toast_count(MAX_VISIBLE_TOASTS + 1), 1);
    assert_eq!(hidden_toast_count(MAX_VISIBLE_TOASTS + 7), 7);
}

#[test]
fn op_registry_cancels_only_the_targeted_operation() {
    let ops = OpRegistry::default();
    let first_flag = Arc::new(AtomicBool::new(false));
    let second_flag = Arc::new(AtomicBool::new(false));
    let first = ops.register(op_handle(Some(first_flag.clone())));
    let _second = ops.register(op_handle(Some(second_flag.clone())));

    ops.cancel(first);
    assert!(first_flag.load(Ordering::Relaxed));
    assert!(!second_flag.load(Ordering::Relaxed));

    // An unknown id, and an operation without a cancel flag, are no-ops.
    ops.cancel(first + 1000);
    let uninterruptible = ops.register(op_handle(None));
    ops.cancel(uninterruptible);
    assert!(!second_flag.load(Ordering::Relaxed));
}

#[test]
#[cfg(windows)]
fn image_gallery_is_windows_only_and_image_only() {
    assert!(should_try_image_gallery("jpg"));
    assert!(should_try_image_gallery("PNG"));
    assert!(!should_try_image_gallery("txt"));
    assert!(!should_try_image_gallery("mp4"));
}

#[test]
fn contextual_open_and_duplicate_share_adjacent_tab_insertion() {
    let mut book = TabBook {
        tabs: vec![Tab::new(PathBuf::from("A"))],
        active: 0,
    };
    book.open(PathBuf::from("B"));
    book.open(PathBuf::from("C"));
    assert!(book.select(0));

    let opened = book.open_after_active(PathBuf::from("X"));
    assert_eq!(opened, 1);
    assert_eq!(book.active, 1);
    assert_eq!(
        book.tabs
            .iter()
            .map(|tab| tab.current_path.as_path())
            .collect::<Vec<_>>(),
        ["A", "X", "B", "C"].map(Path::new)
    );

    assert!(book.duplicate(2));
    assert_eq!(book.active, 3);
    assert_eq!(
        book.tabs
            .iter()
            .map(|tab| tab.current_path.as_path())
            .collect::<Vec<_>>(),
        ["A", "X", "B", "B", "C"].map(Path::new)
    );
}

#[test]
fn an_external_location_is_inserted_at_the_claimed_tab_gap() {
    let mut book = TabBook {
        tabs: vec![Tab::new(PathBuf::from("A"))],
        active: 0,
    };
    book.open(PathBuf::from("C"));

    let inserted = book.insert_tab_at(1, Tab::new(PathBuf::from("B")));
    assert_eq!(inserted, 1);
    assert_eq!(book.active, 1);
    assert_eq!(
        book.tabs
            .iter()
            .map(|tab| tab.current_path.as_path())
            .collect::<Vec<_>>(),
        ["A", "B", "C"].map(Path::new)
    );

    let appended = book.insert_tab_at(usize::MAX, Tab::new(PathBuf::from("D")));
    assert_eq!(appended, 3);
    assert_eq!(book.active, 3);
}

#[test]
fn favorite_rows_distinguish_directories_files_and_missing_targets() {
    let root = std::env::temp_dir().join(format!("favnyr-favorite-kind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("folder01");
    let file = root.join("file01.bin");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(&file, b"data").unwrap();
    let row = |path: &Path| FlatFav {
        id: "favorite01".into(),
        name: "Favorite".into(),
        path: path.display().to_string(),
        is_container: false,
        depth: 0,
        expanded: false,
        has_children: false,
    };

    let directory = flat_to_favnode(&row(&folder));
    assert!(directory.available);
    assert!(directory.is_dir);

    let regular_file = flat_to_favnode(&row(&file));
    assert!(regular_file.available);
    assert!(!regular_file.is_dir);

    let missing = flat_to_favnode(&row(&root.join("missing")));
    assert!(!missing.available);
    assert!(!missing.is_dir);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_sidebar_directory_split_creates_a_new_view_with_target_chrome() {
    let state = AppState::new_at(Config::default(), PathBuf::from("folder01"), 2);
    let expected_columns = {
        let mut panels = state.panels.borrow_mut();
        panels[0].vbar_user_w = 184.0;
        panels[0].columns.clone()
    };

    assert!(split_with_path(
        &state,
        PathBuf::from("folder02"),
        0,
        SplitDir::Row,
        true,
    ));

    let panels = state.panels.borrow();
    assert_eq!(panels.len(), 2);
    assert_eq!(*state.active_panel.borrow(), 1);
    assert_eq!(
        panels[1].tabs.tabs[0].current_path,
        PathBuf::from("folder02")
    );
    assert_eq!(panels[1].tab_bar_mode, 2);
    assert_eq!(panels[1].vbar_user_w, 184.0);
    assert_eq!(panels[1].columns, expected_columns);
    drop(panels);
    assert_eq!(state.layout.borrow().leaf_indices(), vec![1, 0]);
}

#[test]
fn a_favorite_drop_outside_its_visible_tree_has_no_reorder_target() {
    let clamped = fav_drag_target(-1000.0, 100.0, 1, 3);
    assert_eq!(
        clamped.0, 0,
        "raw geometry deliberately clamps to the first row"
    );
    assert_eq!(fav_drop_target(false, -1000.0, 100.0, 1, 3), None);
    assert_eq!(fav_drop_target(true, -1000.0, 100.0, 1, 3), Some(clamped));
}

#[test]
fn filename_caret_stops_before_a_real_extension() {
    assert_eq!(filename_caret_offset("test.txt", false), 4);
    // Also the span the rename popup's "select name" button highlights.
    assert_eq!(
        filename_caret_offset("my_file01.txt", false),
        "my_file01".len() as i32
    );
    assert_eq!(
        filename_caret_offset("résumé.txt", false),
        "résumé".len() as i32
    );
}

#[test]
fn filename_caret_keeps_folders_dotfiles_and_numeric_suffixes_whole() {
    for (name, is_dir) in [
        ("folder.with.dot", true),
        // A folder has no extension to set aside: the button selects it whole.
        ("my_dir01", true),
        (".bashrc", false),
        ("data.2024", false),
        ("README", false),
    ] {
        assert_eq!(
            filename_caret_offset(name, is_dir),
            name.len() as i32,
            "{name}"
        );
    }
}

#[test]
fn only_normal_windows_persist_their_dimensions() {
    assert!(should_persist_window_dimensions(false, false, false));
    assert!(!should_persist_window_dimensions(true, false, false));
    assert!(!should_persist_window_dimensions(false, true, false));
    assert!(!should_persist_window_dimensions(false, false, true));
}

#[test]
fn window_persistence_ignores_favnyr_ui_zoom() {
    // 150% OS DPI and 110% Favnyr zoom produce an effective Slint scale
    // of 1.65. Persistence must divide by 1.5, not by 1.65.
    assert_eq!(
        persisted_logical_window_size((1650, 1050), 1.5, 1.65),
        (1100, 700)
    );
}

#[test]
fn window_persistence_has_a_pre_zoom_startup_fallback() {
    assert_eq!(
        persisted_logical_window_size((1375, 875), 0.0, 1.25),
        (1100, 700)
    );
}

/// Reduces a `Vec<Crumb>` to comparable `(label, path)` pairs.
fn pairs(path: &Path) -> Vec<(String, String)> {
    breadcrumbs(path)
        .iter()
        .map(|c| (c.label.to_string(), c.path.to_string()))
        .collect()
}

#[cfg(unix)]
#[test]
fn breadcrumbs_unix_absolute() {
    assert_eq!(
        pairs(Path::new("/usr/lib")),
        vec![
            ("/".into(), "/".into()),
            ("usr".into(), "/usr".into()),
            ("lib".into(), "/usr/lib".into()),
        ]
    );
}

#[cfg(unix)]
#[test]
fn breadcrumbs_unix_root() {
    assert_eq!(pairs(Path::new("/")), vec![("/".into(), "/".into())]);
}

/// Windows: the drive prefix and the root merge into "C:\", and
/// each segment accumulates a valid navigable path (no incorrect "/").
#[cfg(windows)]
#[test]
fn breadcrumbs_windows_drive() {
    assert_eq!(
        pairs(Path::new(r"C:\Users\user")),
        vec![
            (r"C:\".into(), r"C:\".into()),
            ("Users".into(), r"C:\Users".into()),
            ("user".into(), r"C:\Users\user".into()),
        ]
    );
}

#[cfg(windows)]
#[test]
fn breadcrumbs_windows_drive_root() {
    assert_eq!(
        pairs(Path::new(r"C:\")),
        vec![(r"C:\".into(), r"C:\".into())]
    );
}

#[test]
fn upward_navigation_reveals_only_the_folder_just_left() {
    let parent = PathBuf::from("folder_01").join("folder_02");
    let child = parent.join("folder_03");

    assert_eq!(
        upward_navigation_child_name(&parent, &child).as_deref(),
        Some("folder_03")
    );
    assert_eq!(
        upward_navigation_child_name(&parent, &child.join("deeper")),
        None
    );
    assert_eq!(
        upward_navigation_child_name(&parent, &PathBuf::from("folder_01").join("folder_04")),
        None
    );
}

#[test]
fn workspace_names_are_compared_trimmed_and_case_insensitive() {
    assert_eq!(
        normalized_workspace_name("  Café Project  "),
        "café project"
    );
    assert_eq!(
        normalized_workspace_name("CAFÉ PROJECT"),
        normalized_workspace_name("café project")
    );
}

#[test]
fn type_ahead_filter_matches_any_name_substring() {
    assert!(name_contains_filter("user handbook", "han"));
    assert!(name_contains_filter("user handbook", "book"));
    assert!(name_contains_filter("HANDBOOK Notes", "book"));
    assert!(!name_contains_filter("user handbook", "report"));
}

#[test]
fn rename_status_only_offers_force_replace_for_two_files() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "favnyr-rename-status-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.txt");
    std::fs::write(&source, b"source").unwrap();

    assert_eq!(
        rename_name_status(&source, "source.txt"),
        RenameNameStatus::Valid
    );
    assert_eq!(
        rename_name_status(&source, "free.txt"),
        RenameNameStatus::Valid
    );
    assert_eq!(
        entry_name_availability(&dir, "free.txt"),
        EntryNameAvailability::Available
    );
    assert_eq!(
        rename_name_status(&source, "../bad"),
        RenameNameStatus::Invalid
    );
    assert_eq!(
        entry_name_availability(&dir, "../bad"),
        EntryNameAvailability::Invalid
    );

    std::fs::write(dir.join("target.txt"), b"target").unwrap();
    assert_eq!(
        entry_name_availability(&dir, "target.txt"),
        EntryNameAvailability::ExistingNonDirectory
    );
    assert_eq!(
        rename_name_status(&source, "target.txt"),
        RenameNameStatus::ReplaceableFile
    );

    std::fs::create_dir(dir.join("target-dir")).unwrap();
    assert_eq!(
        entry_name_availability(&dir, "target-dir"),
        EntryNameAvailability::ExistingDirectory
    );
    assert_eq!(
        rename_name_status(&source, "target-dir"),
        RenameNameStatus::Conflict
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_depth_separates_color_bits_from_alpha() {
    assert_eq!(
        format_img_meta(1920, 1080, 24, true, Lang::En),
        ("1920x1080".to_string(), "24-bit + alpha".to_string())
    );
    assert_eq!(
        format_img_meta(800, 600, 24, false, Lang::En),
        ("800x600".to_string(), "24-bit".to_string())
    );
    // The alpha bits are excluded from `bits`, and the wording follows the
    // catalogue: an RGBA8 image reads as colour depth, not as 32 bits.
    assert_eq!(
        format_img_meta(800, 600, 24, true, Lang::Fr).1,
        "24 bits + alpha".to_string()
    );
}

#[test]
fn opener_filter_searches_label_program_arguments_and_extensions() {
    let opener = openers::Opener {
        id: "editor".into(),
        label: "Text Editor".into(),
        program: r"C:\Apps\Editor.exe".into(),
        assoc: Some("Applications\\Editor.exe".into()),
        icon: openers::OpenerIcon::None,
        args: vec!["--wait".into(), "{file}".into()],
        default_exts: vec!["rs".into()],
        used_exts: vec!["txt".into()],
        use_count: 0,
        last_used: 0,
        elevated: false,
        ctx_menu: 0,
        ctx_exts: Vec::new(),
    };
    for needle in ["text", "editor.exe", "--wait", "rs", "txt"] {
        assert!(opener_matches_filter(&opener, needle), "{needle}");
    }
    assert!(!opener_matches_filter(&opener, "archiver"));
}

#[test]
fn picker_filter_is_trimmed_case_insensitive_and_non_destructive() {
    let item = |id: &str, label: &str| OpenerItem {
        id: id.into(),
        label: label.into(),
        available: true,
        icon: Image::default(),
        icon_kind: 0,
    };
    let cached = vec![
        item("image", "Image Editor"),
        item("reader", "Document Reader"),
        item("browser", "Web Browser"),
    ];

    let visible = filtered_ow_picker_items(&cached, "  EDITOR ");
    assert_eq!(
        visible
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["image"]
    );
    assert!(filtered_ow_picker_items(&cached, "missing").is_empty());
    assert_eq!(filtered_ow_picker_items(&cached, "").len(), cached.len());
    assert_eq!(
        cached.len(),
        3,
        "filtering must leave the full cache intact"
    );
}

#[test]
fn async_listing_only_accepts_the_latest_matching_navigation() {
    let path = PathBuf::from("network-a");
    let mut panel = Panel::with_mode(path.clone(), columns::default_columns(), 0);
    panel.pending_listing = true;
    panel.listing_gen = 7;

    assert!(async_listing_is_current(&panel, 7, &path));
    assert!(!async_listing_is_current(&panel, 6, &path));
    assert!(!async_listing_is_current(&panel, 7, Path::new("network-b")));

    panel.pending_listing = false;
    assert!(!async_listing_is_current(&panel, 7, &path));
}

#[cfg(windows)]
#[test]
fn dropped_files_can_launch_windows_programs_and_batch_scripts() {
    for candidate in ["tool.exe", "tool.COM", "tool.cmd", "tool.BaT"] {
        assert!(is_drop_runnable(Path::new(candidate)), "{candidate}");
    }
    for candidate in ["notes.txt", "archive.zip", "folder"] {
        assert!(!is_drop_runnable(Path::new(candidate)), "{candidate}");
    }
}

/// Two items of one paste must never aim at the same destination. Nothing
/// is on disk while the job is being arbitrated, so the filesystem check
/// beside this one cannot answer: a name typed in the popup differing only
/// in case from one already resolved would be accepted, and the second copy
/// would land on the first.
#[test]
fn a_paste_never_lets_two_items_claim_one_destination() {
    let job = PasteJob {
        op: ClipOp::Copy,
        dst_dir: PathBuf::from("/dst"),
        pending: std::collections::VecDeque::new(),
        resolved: vec![(
            PathBuf::from("/src/a.txt"),
            PathBuf::from("/dst/Merged.txt"),
            false,
        )],
        current: None,
        transient_cleanup: None,
    };

    assert!(job.claims(Path::new("/dst/Merged.txt")), "the exact name");
    assert!(
        !job.claims(Path::new("/dst/Other.txt")),
        "an unrelated name stays free"
    );
    // The platform decides whether a different spelling is the same file.
    assert_eq!(
        job.claims(Path::new("/dst/merged.txt")),
        cfg!(windows),
        "case follows the filesystem's own rule"
    );
}

#[test]
fn file_drop_rejects_the_source_itself_and_folder_descendants() {
    let album = PathBuf::from("gallery").join("album");
    let sources = vec![album.clone()];

    assert!(paths_conflict_with_drop_target(&sources, &album, true));
    assert!(paths_conflict_with_drop_target(
        &sources,
        &album.join("subfolder"),
        true
    ));
    assert!(!paths_conflict_with_drop_target(
        &sources,
        &PathBuf::from("gallery").join("other"),
        true
    ));

    let file = PathBuf::from("gallery").join("notes.txt");
    assert!(paths_conflict_with_drop_target(
        std::slice::from_ref(&file),
        &file,
        false
    ));
}

#[test]
fn tab_layout_accumulates_offsets_with_spacing() {
    let titles = vec!["~".to_string(), "src".to_string(), "target".to_string()];
    let geo = tab_layout(&titles);
    assert_eq!(geo.len(), 3);
    // First tab at offset 0.
    assert_eq!(geo[0].1, 0.0);
    // Each offset = previous offset + previous width + 2 spacing.
    assert_eq!(geo[1].1, geo[0].1 + geo[0].0 + 2.0);
    assert_eq!(geo[2].1, geo[1].1 + geo[1].0 + 2.0);
    // All widths within the design bounds.
    assert!(geo.iter().all(|(w, _)| (90.0..=180.0).contains(w)));
}

// Workspace "modified" detection -----

fn tab(path: &str) -> TabState {
    TabState {
        path: path.into(),
        sort_column: SortColumn::Name,
        sort_order: SortOrder::Asc,
        preview: false,
        show_hidden: false,
        group_mode: GroupMode::FoldersFirst,
        zoom: None,
        view_mode: None,
        subfolders: false,
        collapsed: Vec::new(),
    }
}

fn panel(tabs: Vec<TabState>) -> PanelState {
    PanelState {
        stretch: 1.0,
        active_tab: 0,
        tabs,
        columns: columns::default_columns(),
        tab_bar_mode: 0,
        vbar_width: 0.0,
    }
}

fn ws(panels: Vec<PanelState>) -> WorkspaceState {
    WorkspaceState {
        active_panel: 0,
        panels,
        layout: None,
        workspace_name: None,
        closed_tabs: Vec::new(),
        sidebar_sections: SidebarSectionsState::default(),
    }
}

// ----- Grid, sections and subfolder contents -----

/// Grid style of the tests: zoom 2 (76px tiles) in a 400px list area,
/// which packs exactly THREE 104px cells per line.
fn grid_style(width: f32) -> RowStyle {
    RowStyle {
        mode: ViewMode::Grid,
        zoom: THUMB_DEFAULT_ZOOM,
        compact_icon_rows: false,
        width,
    }
}

fn named_row(name: &str) -> FileRow {
    FileRow {
        name: name.into(),
        ..Default::default()
    }
}

/// A section header row, as `section_row` builds it.
fn header_row(key: &str, label: &str) -> FileRow {
    FileRow {
        kind: -1,
        visual_h: SECTION_HEADER_H,
        role: ROW_ROLE_SECTION,
        section: key.into(),
        section_label: label.into(),
        ..Default::default()
    }
}

fn test_entry(name: &str, is_dir: bool) -> Entry {
    Entry {
        kind: rfs::classify_kind(
            Path::new(name).extension().and_then(|ext| ext.to_str()),
            is_dir,
        ),
        name: name.to_string(),
        path: PathBuf::from("/gallery").join(name),
        size_bytes: None,
        mtime_unix: None,
        is_dir,
        hidden: false,
        is_symlink: false,
        executable: false,
    }
}

#[test]
fn a_tab_carries_its_display_state_through_a_workspace_round_trip() {
    let tab = Tab::restored(
        PathBuf::from("/gallery"),
        SortState {
            column: SortColumn::Name,
            order: SortOrder::Asc,
        },
        ViewMode::Grid,
        Some(4),
        true,
        GroupMode::Category,
        true,
        vec!["cat:image".to_string()],
    );
    let state = tab_to_state(&tab);
    assert_eq!(state.view_mode.as_deref(), Some("grid"));
    assert!(state.preview, "grid asks its rows for thumbnails");
    assert_eq!(state.zoom, Some(4));
    assert!(state.subfolders);
    assert_eq!(state.collapsed, ["cat:image"]);

    let back = tab_from_state(state);
    assert_eq!(back.mode, ViewMode::Grid);
    assert_eq!(back.zoom, 4);
    assert!(back.subfolders);
    assert_eq!(back.collapsed, ["cat:image"]);

    // A workspace older than the grid has no `view_mode`; its `preview`
    // flag still names the mode the tab was left in.
    let legacy = TabState {
        view_mode: None,
        preview: true,
        ..tab_to_state(&tab)
    };
    assert_eq!(tab_mode_of(&legacy), ViewMode::Previews);
}
