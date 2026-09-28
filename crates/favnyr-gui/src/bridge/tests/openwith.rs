use super::*;

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
