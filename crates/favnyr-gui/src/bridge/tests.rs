use super::*;

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

/// Files sharing an application are handed over together, so an editor
/// opens ONE window holding all of them.
#[test]
fn plan_open_groups_files_that_share_an_application() {
    let plan = plan_open(&files(&["a.txt", "b.txt", "c.md"]), opener_for);
    assert_eq!(
        plan,
        vec![Launch::Opener {
            id: "editor".to_string(),
            paths: files(&["a.txt", "b.txt", "c.md"]),
        }]
    );
}

/// Each file is resolved by ITS OWN extension: a text file and a picture
/// chosen together reach two applications, not the first one's twice.
#[test]
fn plan_open_sends_each_kind_to_its_own_application() {
    let plan = plan_open(&files(&["a.txt", "b.png"]), opener_for);
    assert_eq!(
        plan,
        vec![
            Launch::Opener {
                id: "editor".to_string(),
                paths: files(&["a.txt"]),
            },
            Launch::Opener {
                id: "viewer".to_string(),
                paths: files(&["b.png"]),
            },
        ]
    );
}

/// No default of its own → the system opens it, one launch per file. This
/// is what makes a selection of plain text files start as many editors.
#[test]
fn plan_open_hands_unclaimed_files_to_the_system_one_by_one() {
    let plan = plan_open(&files(&["a.log", "b.log"]), opener_for);
    assert_eq!(
        plan,
        vec![
            Launch::System(PathBuf::from("/somewhere/a.log")),
            Launch::System(PathBuf::from("/somewhere/b.log")),
        ]
    );
}

/// A group appears where its FIRST file did, so what opens first is what
/// the user sees first in the list.
#[test]
fn plan_open_keeps_the_selection_order() {
    let plan = plan_open(&files(&["a.png", "b.log", "c.png"]), opener_for);
    assert_eq!(
        plan,
        vec![
            Launch::Opener {
                id: "viewer".to_string(),
                paths: files(&["a.png", "c.png"]),
            },
            Launch::System(PathBuf::from("/somewhere/b.log")),
        ]
    );
}

/// A file with no extension has no default to look up, and is opened the
/// ordinary way rather than dropped.
#[test]
fn plan_open_covers_a_file_without_extension() {
    let plan = plan_open(&files(&["README"]), opener_for);
    assert_eq!(
        plan,
        vec![Launch::System(PathBuf::from("/somewhere/README"))]
    );
}

#[test]
fn monochrome_shell_icon_detection() {
    // Monochrome glyph (grey, opaque) → should be re-tinted.
    let mono = vec![40, 40, 40, 255, 200, 200, 200, 255];
    assert!(shell_icon_monochrome(&Some((mono, 2, 1))));
    // Coloured icon (one saturated red pixel) → left intact.
    let color = vec![220, 20, 20, 255, 30, 30, 30, 255];
    assert!(!shell_icon_monochrome(&Some((color, 2, 1))));
    // Fully transparent / empty → no tint (nothing to colour).
    let empty = vec![10, 10, 10, 0, 200, 200, 200, 10];
    assert!(!shell_icon_monochrome(&Some((empty, 2, 1))));
    assert!(!shell_icon_monochrome(&None));
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
fn op_registry_tracks_each_operation_under_its_own_id() {
    let ops = OpRegistry::default();
    assert!(!ops.in_flight());

    let first = ops.register(op_handle(None));
    let second = ops.register(op_handle(None));
    assert_ne!(first, second, "ids must distinguish concurrent operations");
    assert!(ops.in_flight());

    // Finishing one leaves the other running.
    assert!(ops.finish(first).is_some());
    assert!(ops.in_flight());
    assert!(ops.finish(second).is_some());
    assert!(!ops.in_flight());
}

#[test]
fn op_registry_ignores_a_completion_delivered_twice() {
    let ops = OpRegistry::default();
    let id = ops.register(op_handle(None));
    assert!(ops.finish(id).is_some());
    // A second delivery must not release an unrelated operation.
    let other = ops.register(op_handle(None));
    assert!(ops.finish(id).is_none());
    assert!(ops.in_flight());
    assert!(ops.finish(other).is_some());
}

#[test]
fn op_registry_returns_the_focus_target_of_that_operation_only() {
    let ops = OpRegistry::default();
    let with_focus = ops.register(OpHandle {
        cancel: None,
        pending_focus: Some(PathBuf::from("/tmp/copied.txt")),
        targets: Vec::new(),
        thumbnail_invalidations: Vec::new(),
        transient_cleanup: None,
    });
    let without = ops.register(op_handle(None));

    assert_eq!(ops.finish(without).and_then(|h| h.pending_focus), None);
    assert_eq!(
        ops.finish(with_focus).and_then(|h| h.pending_focus),
        Some(PathBuf::from("/tmp/copied.txt"))
    );
}

#[test]
fn a_running_operation_holds_its_destinations_until_it_ends() {
    let ops = OpRegistry::default();
    let copying = ops.register(op_handle_writing(&["/dst/photo.jpg", "/dst/notes.txt"]));

    // Claimed from the start — well before the files exist on disk, which is
    // exactly when a second paste would otherwise pick the same name.
    assert!(ops.is_reserved(Path::new("/dst/photo.jpg")));
    assert!(ops.is_reserved(Path::new("/dst/notes.txt")));
    assert!(!ops.is_reserved(Path::new("/dst/other.jpg")));

    // Released together with the operation, so the names free up again.
    assert!(ops.finish(copying).is_some());
    assert!(!ops.is_reserved(Path::new("/dst/photo.jpg")));
    assert!(!ops.is_reserved(Path::new("/dst/notes.txt")));
}

#[test]
fn concurrent_operations_release_only_their_own_destinations() {
    let ops = OpRegistry::default();
    let first = ops.register(op_handle_writing(&["/dst/a.txt"]));
    let _second = ops.register(op_handle_writing(&["/dst/b.txt"]));

    assert!(ops.finish(first).is_some());
    assert!(!ops.is_reserved(Path::new("/dst/a.txt")));
    // The operation still running keeps its own claim.
    assert!(ops.is_reserved(Path::new("/dst/b.txt")));
}

#[test]
fn a_deletion_claims_nothing_since_it_writes_nowhere() {
    let ops = OpRegistry::default();
    let deleting = ops.register(op_handle(None));
    assert!(ops.in_flight());
    assert!(!ops.is_reserved(Path::new("/dst/anything")));
    assert!(ops.finish(deleting).is_some());
}

#[test]
fn a_free_name_steps_over_the_destination_of_a_running_operation() {
    let ops = OpRegistry::default();
    // Nothing exists on disk here, so only the reservation can push the
    // name forward — a plain `exists()` check would hand back the same path.
    let taken_path = std::env::temp_dir().join("favnyr-op-reservation-test.txt");
    ops.register(op_handle_writing(&[taken_path.to_str().unwrap()]));

    let picked = ops::unique_sibling_where(&taken_path, |p| p.exists() || ops.is_reserved(p));
    assert_ne!(picked, taken_path, "must not reuse a claimed destination");
    assert_eq!(picked.parent(), taken_path.parent());
}

#[test]
fn the_extension_field_accepts_the_spellings_users_actually_type() {
    // Plain, dotted and shell-style all reach the same stored form. The
    // shell one used to be kept verbatim and matched nothing, so the entry
    // silently disappeared from the menu.
    assert_eq!(parse_exts("zip"), ["zip"]);
    assert_eq!(parse_exts(".zip"), ["zip"]);
    assert_eq!(parse_exts("*.zip"), ["zip"]);
    assert_eq!(parse_exts("ZIP"), ["zip"]);
    // A lone star is the wildcard and must survive untouched.
    assert_eq!(parse_exts("*"), [openers::CTX_EXT_ALL]);
    // Mixed list, separators and spacing.
    assert_eq!(parse_exts("*.zip, .rar ; 7z"), ["zip", "rar", "7z"]);
    assert!(parse_exts("   ").is_empty());
}

#[test]
fn the_context_entry_filter_only_narrows_files_and_needs_every_one_to_match() {
    let archive_only = openers::Opener {
        id: "x".into(),
        label: "Extract".into(),
        program: "7z".into(),
        assoc: None,
        icon: openers::OpenerIcon::None,
        args: vec![],
        default_exts: vec![],
        used_exts: vec![],
        use_count: 0,
        last_used: 0,
        elevated: false,
        ctx_menu: openers::CTX_FILE,
        ctx_exts: vec!["zip".into(), "7z".into()],
    };
    let zip = PathBuf::from("/tmp/a.zip");
    let txt = PathBuf::from("/tmp/a.txt");

    assert!(ctx_entry_applies(
        &archive_only,
        openers::CTX_FILE,
        std::slice::from_ref(&zip)
    ));
    assert!(!ctx_entry_applies(
        &archive_only,
        openers::CTX_FILE,
        std::slice::from_ref(&txt)
    ));
    // A mixed selection hides it: the command would run on the odd one out
    // and fail there.
    assert!(!ctx_entry_applies(
        &archive_only,
        openers::CTX_FILE,
        &[zip.clone(), txt.clone()]
    ));

    // Folders and the view background have no extension — never filtered.
    assert!(ctx_entry_applies(
        &archive_only,
        openers::CTX_DIR,
        std::slice::from_ref(&txt)
    ));
    assert!(ctx_entry_applies(
        &archive_only,
        openers::CTX_BACKGROUND,
        &[]
    ));

    // An unrestricted command shows up on anything, as before.
    let anything = openers::Opener {
        ctx_exts: vec![],
        ..archive_only.clone()
    };
    assert!(ctx_entry_applies(&anything, openers::CTX_FILE, &[txt]));
}

#[test]
fn extract_recipes_are_limited_to_archives_and_the_others_are_not() {
    for r in RECIPES {
        let extracts = r.label_key.starts_with("ow_recipe_extract");
        if extracts {
            assert_eq!(
                r.ctx_exts,
                rfs::ARCHIVE_EXTENSIONS,
                "{} should only show on archives",
                r.label_key
            );
        } else {
            assert_eq!(
                r.ctx_exts,
                [openers::CTX_EXT_ALL],
                "{} should show on every file",
                r.label_key
            );
        }
    }
    // The filter is only consulted for the FILE entry, so a recipe that
    // narrows extensions without claiming that menu would be inert.
    for r in RECIPES
        .iter()
        .filter(|r| r.ctx_exts != [openers::CTX_EXT_ALL])
    {
        assert!(
            r.ctx & openers::CTX_FILE != 0,
            "{} restricts extensions but is not pinned to files",
            r.label_key
        );
    }
}

#[test]
fn every_recipe_has_one_of_the_four_builtin_icon_families() {
    let mut kinds = std::collections::BTreeSet::new();
    for recipe in RECIPES {
        assert_ne!(recipe.icon, openers::OpenerIcon::None);
        kinds.insert(recipe.icon.as_i32());
    }
    assert_eq!(kinds, [1, 2, 3, 4].into_iter().collect());
}

#[test]
fn every_recipe_is_translated_and_expresses_its_intended_mode() {
    // Actions that must cover the whole selection in a single run; every
    // other one runs per selected item.
    const RUN_ONCE: &[&str] = &[
        "ow_recipe_compress_zip",
        "ow_recipe_compress_targz",
        "ow_recipe_share_bluetooth",
    ];

    for r in RECIPES {
        // A recipe whose label falls back to its own key would show the raw
        // key in the settings. Its visible label must remain exactly the
        // translated action in every bundled language: the icon identifies
        // the tool, so adding a textual prefix would be redundant.
        for lang in [Lang::En, Lang::Fr, Lang::Es, Lang::De, Lang::It, Lang::Zh] {
            let action = i18n::tr(lang, r.label_key);
            assert_ne!(action, r.label_key, "missing translation: {}", r.label_key);
            assert_eq!(r.label(lang), action);
        }
        assert!(r.ctx != 0, "a recipe nobody can reach: {}", r.label_key);
        assert!(
            !r.programs.is_empty(),
            "a recipe with no program can never appear: {}",
            r.label_key
        );

        // Each recipe must land in the mode it was written for; the args
        // alone decide that, so a typo would silently flip it.
        let o = openers::Opener {
            id: String::new(),
            label: String::new(),
            program: r.programs[0].into(),
            assoc: None,
            // Simulates an opener saved before recipe icons were persisted.
            icon: openers::OpenerIcon::None,
            args: split_args(r.args),
            default_exts: Vec::new(),
            used_exts: Vec::new(),
            use_count: 0,
            last_used: 0,
            elevated: false,
            ctx_menu: r.ctx,
            ctx_exts: Vec::new(),
        };
        assert_eq!(effective_opener_icon(&o), r.icon);
        if RUN_ONCE.contains(&r.label_key) {
            assert!(o.expands_list(), "{} must run once", r.label_key);
        } else {
            assert!(
                !o.expands_list() && o.has_tag(),
                "{} must run once per selected item",
                r.label_key
            );
        }
        // No recipe may fall through to the historical mode that silently
        // appends every path at the end — each states what it wants.
        assert!(
            o.expands_list() || o.has_tag(),
            "{} would fall back to appending paths blindly",
            r.label_key
        );
    }

    // One action key is shared by several tools ("Extract here" serves 7z
    // and tar). They must agree on the mode, otherwise the same wording
    // would behave differently depending on which chip was picked.
    for r in RECIPES {
        for other in RECIPES.iter().filter(|o| o.label_key == r.label_key) {
            assert_eq!(
                split_args(r.args).iter().any(|a| a == openers::LIST_TAG),
                split_args(other.args)
                    .iter()
                    .any(|a| a == openers::LIST_TAG),
                "{} runs differently for {} and {}",
                r.label_key,
                r.tool,
                other.tool
            );
        }
    }
}

#[test]
fn the_preview_states_how_many_processes_will_start() {
    let en = Lang::En;
    // No tag: one process receives the whole selection, whatever its size.
    assert_eq!(run_count_text(en, false, 5), "Will run once");
    // A tag makes it one process per item — the rule this line exists to expose.
    assert!(run_count_text(en, true, 3).contains('3'));
    assert_eq!(run_count_text(en, true, 1), "Will run once");
    // Nothing selected (from Settings): state the rule, not a count of zero.
    assert_eq!(
        run_count_text(en, true, 0),
        "Runs once per selected item",
        "an empty selection must not read as 'will run 0 times'"
    );
}

#[test]
fn quoted_arguments_survive_the_split_and_stay_one_argument() {
    // The plain case is unchanged.
    assert_eq!(
        split_args("a {dir}/x.7z {file}"),
        ["a", "{dir}/x.7z", "{file}"]
    );
    // Quotes group a run: without this, an archive name containing a space
    // reached the program as two mangled arguments.
    assert_eq!(
        split_args(r#"a "{dir}/My Archive.7z" {file}"#),
        ["a", "{dir}/My Archive.7z", "{file}"]
    );
    // Single quotes work too, and may open mid-argument.
    assert_eq!(split_args("-o'My Folder'"), ["-oMy Folder"]);
    assert_eq!(split_args(r#"--out="a b" c"#), ["--out=a b", "c"]);
    // A quote of the other kind inside a quoted run is literal.
    assert_eq!(split_args(r#""it's here""#), ["it's here"]);
    // Unbalanced input is tolerated: the field is unbalanced while typing.
    assert_eq!(split_args(r#"a "unterminated"#), ["a", "unterminated"]);
    // Runs of whitespace collapse, and an empty field yields no argument.
    assert_eq!(split_args("  a   b  "), ["a", "b"]);
    assert!(split_args("   ").is_empty());
    // An explicitly empty argument is preserved.
    assert_eq!(split_args(r#"a "" b"#), ["a", "", "b"]);
}

/// Reopening a saved command must not change what it runs. The arguments
/// are stored split, so putting them back in the field means writing a
/// command line again — and a plain join silently turned one quoted path
/// into as many arguments as it had spaces.
#[test]
fn an_argument_list_survives_the_trip_through_the_field() {
    let cases: Vec<Vec<String>> = vec![
        // The case that broke: a wrapper taking a quoted path with spaces,
        // then the file.
        vec!["run".into(), "folder 01/my app.exe".into(), "{file}".into()],
        // A tag standing alone must stay alone, or the command stops
        // running once for the whole selection.
        vec!["a".into(), "{dir}/{dirname}.7z".into(), "{files}".into()],
        // Each quote character on its own, then both at once — the last
        // one cannot be wrapped and takes the piecewise path.
        vec!["it's here".into()],
        vec![r#"say "hi""#.into()],
        vec![r#"it's "quoted" here"#.into()],
        // An empty argument is a real argument.
        vec!["a".into(), String::new(), "b".into()],
        vec!["plain".into()],
    ];
    for args in cases {
        assert_eq!(
            split_args(&join_args(&args)),
            args,
            "round trip changed {args:?} (written as {:?})",
            join_args(&args)
        );
    }
}

/// The common cases stay readable: quoting only what needs it is what
/// makes the field editable by hand afterwards.
#[test]
fn quoting_is_added_only_where_it_is_needed() {
    assert_eq!(join_args(&["a".into(), "b".into()]), "a b");
    assert_eq!(join_args(&["{file}".into()]), "{file}");
    assert_eq!(join_args(&["b c".into()]), r#""b c""#);
    assert_eq!(join_args(&["it's".into()]), r#""it's""#);
    assert_eq!(join_args(&[r#"say "hi""#.into()]), r#"'say "hi"'"#);
}

#[test]
fn the_preview_keeps_argument_boundaries_visible() {
    // A single path containing spaces must not read as two arguments.
    let argv = vec![
        "a".to_string(),
        "/tmp/My Archive.7z".to_string(),
        "/tmp/x.txt".to_string(),
    ];
    assert_eq!(display_argv(&argv), r#"a "/tmp/My Archive.7z" /tmp/x.txt"#);
    // Nothing is quoted when nothing needs it.
    assert_eq!(display_argv(&["a".into(), "b".into()]), "a b");
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

fn thumb_test_request(path: &str, panel: usize, row: usize, row_count: usize) -> ThumbRequest {
    ThumbRequest {
        job: ThumbJob {
            path: PathBuf::from(path),
            kind: FileKind::Image,
            svg: false,
        },
        locations: vec![ThumbLocation {
            panel,
            row,
            row_count,
        }],
    }
}

#[test]
fn thumbnail_scroll_reprioritizes_the_next_job() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(
        (0..20)
            .map(|row| thumb_test_request(&format!("image-{row:02}.png"), 0, row, 20))
            .collect(),
    );
    scheduler.update_viewport(0, 0, 1);

    // The first decode has already started when the user jumps to the bottom.
    let first = scheduler.try_take_next().unwrap();
    assert_eq!(first.path, PathBuf::from("image-00.png"));
    // Several intermediate positions may be published during a
    // fast scroll: only the most recent one should drive the next choice.
    scheduler.update_viewport(0, 7, 8);
    scheduler.update_viewport(0, 12, 13);
    scheduler.update_viewport(0, 18, 19);
    scheduler.complete(&first.path, first.serial);

    // The next one comes from the new visible zone, not the old FIFO.
    let next = scheduler.try_take_next().unwrap();
    assert_eq!(next.path, PathBuf::from("image-18.png"));
    scheduler.complete(&next.path, next.serial);
    let next = scheduler.try_take_next().unwrap();
    assert_eq!(next.path, PathBuf::from("image-19.png"));
    scheduler.complete(&next.path, next.serial);
}

#[test]
fn thumbnail_pool_takes_each_job_exactly_once_under_concurrency() {
    use std::sync::Mutex;
    // Invariant that makes the pool safe: several workers pull in
    // parallel, but `take_next` removes the path from `pending` UNDER LOCK →
    // no path is decoded twice, none is lost. We stress it
    // with lots of jobs and more threads than the real pool.
    const JOBS: usize = 500;
    let scheduler = Arc::new(ThumbScheduler::new());
    scheduler.replace_pending(
        (0..JOBS)
            .map(|row| thumb_test_request(&format!("img-{row:04}.png"), 0, row, JOBS))
            .collect(),
    );

    let taken: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::with_capacity(JOBS)));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let sched = scheduler.clone();
        let taken = taken.clone();
        handles.push(std::thread::spawn(move || {
            // `try_take_next` returns `None` on an empty queue (clean test exit);
            // in production it's `take_next`, blocking, that waits for the next job.
            while let Some(job) = sched.try_take_next() {
                taken
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(job.path.clone());
                sched.complete(&job.path, job.serial);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let mut paths = taken.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(paths.len(), JOBS, "each job taken exactly once");
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), JOBS, "no duplicate decode");
}

#[test]
fn thumbnail_requests_are_deduplicated_and_stale_paths_are_removed() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(vec![
        thumb_test_request("shared.png", 0, 3, 10),
        thumb_test_request("shared.png", 1, 1, 10),
        thumb_test_request("obsolete.png", 0, 4, 10),
    ]);
    scheduler.replace_pending(vec![
        thumb_test_request("shared.png", 0, 3, 10),
        thumb_test_request("shared.png", 1, 1, 10),
    ]);
    scheduler.update_viewport(1, 1, 2);

    let only = scheduler.try_take_next().unwrap();
    assert_eq!(only.path, PathBuf::from("shared.png"));
    // A refresh during decoding must not create a duplicate.
    scheduler.replace_pending(vec![thumb_test_request("shared.png", 1, 1, 10)]);
    scheduler.complete(&only.path, only.serial);
    assert!(scheduler.try_take_next().is_none());
}

#[test]
fn invalidated_thumbnail_generation_cannot_complete_a_new_request() {
    let scheduler = ThumbScheduler::new();
    let path = PathBuf::from("gallery").join("image-02.jpg");
    scheduler.replace_pending(vec![thumb_test_request(
        path.to_string_lossy().as_ref(),
        0,
        0,
        1,
    )]);
    let stale = scheduler.try_take_next().expect("stale generation");

    scheduler.invalidate_paths(std::slice::from_ref(&path));
    scheduler.merge_pending(vec![thumb_test_request(
        path.to_string_lossy().as_ref(),
        0,
        0,
        1,
    )]);
    let current = scheduler.try_take_next().expect("current generation");
    assert_ne!(stale.serial, current.serial);
    assert!(
        scheduler
            .in_flight_locations(&path, stale.serial)
            .is_empty(),
        "an invalidated decode must lose ownership of the path"
    );
    assert_eq!(
        scheduler.in_flight_locations(&path, current.serial).len(),
        1
    );

    // A late callback from the old decode cannot acknowledge/remove the
    // replacement job that now owns the same path.
    scheduler.complete(&path, stale.serial);
    assert_eq!(
        scheduler.in_flight_locations(&path, current.serial).len(),
        1
    );
    scheduler.complete(&path, current.serial);
}

#[test]
fn thumbnail_lru_removes_only_the_reused_path() {
    let image = image_from_thumb(&Thumbnail {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 255],
    });
    let reused = PathBuf::from("gallery").join("image-02.jpg");
    let untouched = PathBuf::from("gallery").join("image-03.jpg");
    let mut cache = ThumbLru::new(8);
    cache.put(reused.to_string_lossy().into_owned(), image.clone());
    cache.put(untouched.to_string_lossy().into_owned(), image);

    cache.remove_path(&reused);

    assert!(cache.get(reused.to_string_lossy().as_ref()).is_none());
    assert!(cache.get(untouched.to_string_lossy().as_ref()).is_some());
}

#[test]
fn thumbnail_idempotent_merge_keeps_the_ready_heap_intact() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(vec![thumb_test_request("stable.png", 0, 4, 10)]);
    {
        let mut queue = scheduler.queue.lock().unwrap_or_else(|e| e.into_inner());
        scheduler.ensure_ready(&mut queue);
        assert_eq!(queue.ready.len(), 1);
        assert!(!scheduler.priorities_dirty.load(Ordering::Acquire));
    }

    // Hot path of the viewport callback: the visible row is already known at the
    // same location. No O(n) rebuild, no worker wake-up needed.
    scheduler.merge_pending(vec![thumb_test_request("stable.png", 0, 4, 10)]);
    let queue = scheduler.queue.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(queue.ready.len(), 1);
    assert!(!scheduler.priorities_dirty.load(Ordering::Acquire));
}

#[test]
fn thumbnail_navigation_keeps_new_queue_after_stale_job_finishes() {
    let scheduler = ThumbScheduler::new();
    scheduler.replace_pending(vec![thumb_test_request("old-folder.png", 0, 0, 1)]);
    let stale = scheduler.try_take_next().unwrap();

    // Navigation during decoding: the new request replaces the whole
    // old queue, but the already-started job can finish without polluting it.
    scheduler.replace_pending(
        (0..6)
            .map(|row| thumb_test_request(&format!("new-{row}.png"), 0, row, 6))
            .collect(),
    );
    scheduler.update_viewport(0, 5, 5);
    scheduler.complete(&stale.path, stale.serial);

    let next = scheduler.try_take_next().unwrap();
    assert_eq!(next.path, PathBuf::from("new-5.png"));
    scheduler.complete(&next.path, next.serial);
    assert_ne!(next.path, stale.path);
}

#[test]
fn visible_thumbnail_holes_are_reconciled_when_render_window_is_unchanged() {
    let gallery = PathBuf::from("gallery");
    let state = AppState::new_at(Config::default(), gallery.clone(), 0);
    let (top, height, expected) = {
        let mut panels = state.panels.borrow_mut();
        let panel = &mut panels[0];
        panel.tabs.tabs[0].mode = ViewMode::Previews;
        panel.tabs.tabs[0].zoom = THUMB_DEFAULT_ZOOM;

        let mut rows: Vec<FileRow> = (0..100)
            .map(|index| FileRow {
                name: format!("image-{index:03}.png").into(),
                // A row carries its parent: the thumbnail key is
                // `row.path`, so a subfolder section can hold the entries
                // of another folder.
                path: gallery.display().to_string().into(),
                ext: "png".into(),
                kind: FileKind::Image.as_i32(),
                preview_capable: true,
                ..Default::default()
            })
            .collect();
        layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
        let top = rows[50].visual_y;
        let height = rows[50].visual_h * 2.0;
        let visible = row_range_for_slice(&rows, top, top + height);
        let expected = (visible.0..visible.1)
            .map(|index| gallery.join(format!("image-{index:03}.png")))
            .collect::<Vec<_>>();
        panel.viewport_top.set(top);
        panel.viewport_height.set(height);
        panel.replace_rows(rows);
        // Reproduces the zoom relayout: `replace_rows` has already pre-marked
        // exactly the window that the callback is going to republish.
        assert_eq!(
            (panel.rendered_first.get(), panel.rendered_end.get()),
            row_range_for_content_span(
                &*panel.rows_model,
                (top - height).max(0.0),
                top + height * 2.0,
            )
        );
        (top, height, expected)
    };

    // Same range twice: the second reconciliation must neither lose nor
    // duplicate the already-pending requests.
    update_panel_render_window(&state, 0, top, height);
    update_panel_render_window(&state, 0, top, height);

    let mut actual = Vec::new();
    while let Some(job) = state.thumb_scheduler.try_take_next() {
        actual.push(job.path.clone());
        state.thumb_scheduler.complete(&job.path, job.serial);
    }
    actual.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn cached_offscreen_thumbnails_are_not_redecoded_by_a_global_refresh() {
    let gallery = PathBuf::from("gallery-cache");
    let state = AppState::new_at(Config::default(), gallery.clone(), 0);
    let cached_image = image_from_thumb(&Thumbnail {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 255],
    });
    {
        let mut panels = state.panels.borrow_mut();
        let panel = &mut panels[0];
        panel.tabs.tabs[0].mode = ViewMode::Previews;
        panel.tabs.tabs[0].zoom = THUMB_DEFAULT_ZOOM;
        panel.viewport_height.set(76.0);
        let mut rows: Vec<FileRow> = (0..40)
            .map(|index| FileRow {
                name: format!("cached-{index:02}.png").into(),
                path: gallery.display().to_string().into(),
                ext: "png".into(),
                kind: FileKind::Image.as_i32(),
                preview_capable: true,
                ..Default::default()
            })
            .collect();
        layout_at(&mut rows, THUMB_DEFAULT_ZOOM, true);
        panel.replace_rows(rows);
    }
    for index in 0..40 {
        let key = gallery
            .join(format!("cached-{index:02}.png"))
            .to_string_lossy()
            .into_owned();
        state
            .thumb_cache
            .borrow_mut()
            .put(key, cached_image.clone());
    }

    request_thumbnails(&state);
    assert!(
        state.thumb_scheduler.try_take_next().is_none(),
        "a texture already in the LRU must not be re-decoded off-screen"
    );
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

/// Adding an open tab constitutes a workspace modification.
#[test]
fn differ_detects_added_tab() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let b = ws(vec![panel(vec![tab(r"C:\a"), tab(r"C:\b")])]);
    assert!(workspaces_differ(&a, &b));
}

/// A per-tab display setting (hidden files, previews, sort) counts.
#[test]
fn differ_detects_per_tab_settings() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let mut b = ws(vec![panel(vec![tab(r"C:\a")])]);
    b.panels[0].tabs[0].show_hidden = true;
    assert!(workspaces_differ(&a, &b));

    let mut c = ws(vec![panel(vec![tab(r"C:\a")])]);
    c.panels[0].tabs[0].preview = true;
    assert!(workspaces_differ(&a, &c));

    // The thumbnail zoom is saved with the view, so changing it has to
    // raise the asterisk like any other per-tab setting — the title and
    // the Update button both read this one comparison.
    let mut d = ws(vec![panel(vec![tab(r"C:\a")])]);
    d.panels[0].tabs[0].zoom = Some(4);
    assert!(workspaces_differ(&a, &d));

    // Two levels that differ are two different states, even inside
    // preview mode where `preview` alone cannot tell them apart.
    let mut e = ws(vec![panel(vec![tab(r"C:\a")])]);
    e.panels[0].tabs[0].zoom = Some(2);
    let mut f = ws(vec![panel(vec![tab(r"C:\a")])]);
    f.panels[0].tabs[0].zoom = Some(5);
    assert!(workspaces_differ(&e, &f));
}

/// Tab bar position (per-view): counted.
#[test]
fn differ_detects_tab_bar_mode() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let mut b = ws(vec![panel(vec![tab(r"C:\a")])]);
    b.panels[0].tab_bar_mode = 1;
    assert!(workspaces_differ(&a, &b));
}

/// A left-bar section's collapse state goes through the same signature as
/// the tabs: the asterisk and the Update button can never diverge.
#[test]
fn differ_detects_sidebar_section_state() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let mut b = a.clone();
    b.sidebar_sections.favorites_collapsed = true;
    assert!(workspaces_differ(&a, &b));

    b.sidebar_sections.favorites_collapsed = false;
    b.sidebar_sections.network_collapsed = true;
    assert!(workspaces_differ(&a, &b));
}

#[test]
fn sidebar_section_state_survives_app_state_capture_and_reset() {
    let mut saved = ws(vec![panel(vec![tab(r"C:\a")])]);
    saved.sidebar_sections = SidebarSectionsState {
        shortcuts_collapsed: true,
        favorites_collapsed: true,
        drives_collapsed: false,
        network_collapsed: true,
    };
    let config = Config {
        sidebar_section_order: [
            SidebarSection::Favorites,
            SidebarSection::Network,
            SidebarSection::Drives,
            SidebarSection::Shortcuts,
        ],
        ..Config::default()
    };
    let state = AppState::from_workspace(config.clone(), saved.clone());
    assert_eq!(
        state.capture_workspace().sidebar_sections,
        saved.sidebar_sections
    );

    state.reset_to_blank();
    assert_eq!(
        state.capture_workspace().sidebar_sections,
        SidebarSectionsState::default()
    );
    // Reset and loading a workspace never touch the
    // global section order preference.
    assert_eq!(
        state.snapshot_config().sidebar_section_order,
        config.sidebar_section_order
    );
}

/// Volatile fields (split ratios, column widths, active tab or panel,
/// and `workspace_name`) don't affect the workspace's persistent state.
#[test]
fn differ_ignores_volatile_fields() {
    let base = ws(vec![panel(vec![tab(r"C:\a")]), panel(vec![tab(r"C:\b")])]);
    let mut other = base.clone();
    // Split ratios.
    other.panels[0].stretch = 2.5;
    // Active tab / panel (simple focus).
    other.active_panel = 1;
    other.panels[1].active_tab = 0;
    // Vertical bar width + attached name.
    other.panels[0].vbar_width = 220.0;
    other.workspace_name = Some("My Workspace".into());
    // Columns (resized widths).
    if let Some(c) = other.panels[0].columns.first_mut() {
        c.width += 40.0;
    }
    assert!(!workspaces_differ(&base, &other));
}

/// Idempotence: a workspace never differs from itself.
#[test]
fn differ_reflexive() {
    let a = ws(vec![panel(vec![tab(r"C:\a"), tab(r"C:\b")])]);
    assert!(!workspaces_differ(&a, &a.clone()));
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
