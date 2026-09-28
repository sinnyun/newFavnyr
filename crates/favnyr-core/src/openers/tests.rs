use super::*;
use std::path::PathBuf;

fn ctx(p: &str) -> TagContext {
    TagContext::from_path(&PathBuf::from(p))
}

#[test]
fn suggested_sorts_by_recency_and_keeps_unused_in_insertion_order() {
    // Guards the `sort_by` → `sort_by_key(Reverse(..))` rewrite (Clippy
    // `unnecessary_sort_by`, Rust 1.97): most-recent first, and — the part
    // an unstable or wrongly-directed sort would silently break — openers
    // never used (`last_used == 0`) keep their ORIGINAL relative order.
    let mut s = OpenerStore::default();
    let old = s.add("Old", "/bin/old", vec![]);
    let never1 = s.add("Never1", "/bin/n1", vec![]);
    let recent = s.add("Recent", "/bin/recent", vec![]);
    let never2 = s.add("Never2", "/bin/n2", vec![]);
    // Set directly rather than via `record_use` (which stamps the current
    // second): a deterministic gap removes any dependence on how fast the
    // two calls would run.
    s.openers
        .iter_mut()
        .find(|o| o.id == old)
        .unwrap()
        .last_used = 100;
    s.openers
        .iter_mut()
        .find(|o| o.id == recent)
        .unwrap()
        .last_used = 200;

    let ids: Vec<&str> = s.suggested(10).iter().map(|o| o.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            recent.as_str(),
            old.as_str(),
            never1.as_str(),
            never2.as_str()
        ],
        "recent first, then the never-used pair in the order they were added"
    );
}

#[test]
fn elevated_defaults_false_and_round_trips() {
    // New opener: not elevated by default.
    let mut s = OpenerStore::default();
    let id = s.add("A", "/bin/a", vec![]);
    assert!(!s.get(&id).unwrap().elevated);
    // set_elevated toggles the flag (and fails on an unknown id).
    assert!(s.set_elevated(&id, true));
    assert!(s.get(&id).unwrap().elevated);
    assert!(!s.set_elevated("unknown-id", true));
    // TOML round-trip preserved.
    let toml = toml::to_string_pretty(&s).unwrap();
    let back: OpenerStore = toml::from_str(&toml).unwrap();
    assert!(back.get(&id).unwrap().elevated);
    // Backward compat: an opener serialized WITHOUT the field → false.
    let legacy = "[[openers]]\nid = \"x\"\nlabel = \"X\"\nprogram = \"/bin/x\"\n";
    let parsed: OpenerStore = toml::from_str(legacy).unwrap();
    assert!(!parsed.get("x").unwrap().elevated);
}

#[test]
fn recipe_icon_is_backward_compatible_and_follows_duplicates() {
    let mut store = OpenerStore::default();
    let id = store.add("Compress", "/usr/bin/7z", vec!["a".into()]);
    assert_eq!(store.get(&id).unwrap().icon, OpenerIcon::None);
    assert!(store.set_icon(&id, OpenerIcon::SevenZip));

    let duplicate = store.duplicate(&id).unwrap();
    assert_eq!(store.get(&duplicate).unwrap().icon, OpenerIcon::SevenZip);

    let serialized = toml::to_string_pretty(&store).unwrap();
    let restored: OpenerStore = toml::from_str(&serialized).unwrap();
    assert_eq!(restored.get(&id).unwrap().icon, OpenerIcon::SevenZip);

    let legacy = "[[openers]]\nid = \"x\"\nlabel = \"X\"\nprogram = \"/bin/x\"\n";
    let restored_legacy: OpenerStore = toml::from_str(legacy).unwrap();
    assert_eq!(restored_legacy.get("x").unwrap().icon, OpenerIcon::None);
    assert_eq!(OpenerIcon::from_i32(99), OpenerIcon::None);
}

#[test]
fn the_extension_filter_treats_star_and_empty_as_every_file() {
    let mut o = Opener {
        id: "x".into(),
        label: "Extract".into(),
        program: "/usr/bin/7z".into(),
        assoc: None,
        icon: OpenerIcon::None,
        args: vec![],
        default_exts: vec![],
        used_exts: vec![],
        use_count: 0,
        last_used: 0,
        elevated: false,
        ctx_menu: CTX_FILE,
        ctx_exts: vec![],
    };
    // Empty: no restriction. This is what every opener saved before the
    // field existed carries, so they must keep appearing everywhere.
    assert!(o.matches_ctx_ext("zip"));
    assert!(o.matches_ctx_ext(""));

    // The wildcard means the same thing, explicitly.
    o.ctx_exts = vec![CTX_EXT_ALL.to_string()];
    assert!(o.matches_ctx_ext("zip"));
    assert!(o.matches_ctx_ext("txt"));
    assert!(o.matches_ctx_ext(""), "an extensionless file still matches");

    // A real list restricts, and comparison ignores case.
    o.ctx_exts = vec!["zip".into(), "7z".into()];
    assert!(o.matches_ctx_ext("zip"));
    assert!(o.matches_ctx_ext("ZIP"));
    assert!(o.matches_ctx_ext("7z"));
    assert!(!o.matches_ctx_ext("txt"));
    assert!(!o.matches_ctx_ext(""), "no extension is not in the list");

    // The wildcard wins even when mixed with specific entries.
    o.ctx_exts = vec!["zip".into(), CTX_EXT_ALL.to_string()];
    assert!(o.matches_ctx_ext("txt"));
}

#[test]
fn the_extension_filter_survives_a_round_trip_and_a_duplicate() {
    let mut s = OpenerStore::default();
    let id = s.add("Extract", "/usr/bin/7z", vec!["x".into()]);
    assert!(s.set_ctx_menu(&id, CTX_FILE));
    assert!(s.set_ctx_exts(&id, &["zip".to_string(), "rar".to_string()]));

    let toml = toml::to_string_pretty(&s).unwrap();
    let back: OpenerStore = toml::from_str(&toml).unwrap();
    assert_eq!(back.get(&id).unwrap().ctx_exts, ["zip", "rar"]);

    // A duplicate keeps the filter: it would otherwise silently widen to
    // every file, which is the opposite of what the copy was made for.
    let dup = s.duplicate(&id).unwrap();
    assert_eq!(s.get(&dup).unwrap().ctx_exts, ["zip", "rar"]);
}

/// An archive takes the name of what it holds. One item names it itself;
/// past that there is nothing better than the folder they share.
#[test]
fn an_archive_is_named_after_a_lone_item_and_after_their_folder_otherwise() {
    // Real entries: telling a folder from a file is a filesystem question,
    // and a fictional path would answer "file" for both.
    let root = std::env::temp_dir().join(format!("favnyr-setname-{}", std::process::id()));
    let folder = root.join("folder 01");
    std::fs::create_dir_all(&folder).unwrap();
    let file = root.join("my_file.pdf");
    std::fs::write(&file, b"x").unwrap();

    let zip = Opener {
        id: "x".into(),
        label: "zip".into(),
        program: "/usr/bin/7z".into(),
        assoc: None,
        icon: OpenerIcon::None,
        args: vec!["a".into(), "{dir}/{setname}.zip".into(), "{files}".into()],
        default_exts: vec![],
        used_exts: vec![],
        use_count: 0,
        last_used: 0,
        elevated: false,
        ctx_menu: 0,
        ctx_exts: Vec::new(),
    };
    let name = |args: &[String]| args[1].clone();

    // A single file: its own name, without the extension — an archive of
    // `my_file.pdf` is `my_file.zip`, not `my_file.pdf.zip`.
    let one_file = [file.as_path()];
    assert_eq!(
        name(&zip.render_for_batch(&TagContext::from_path(&file), &one_file)),
        format!("{}/my_file.zip", root.display())
    );

    // A single folder keeps its WHOLE name: dropping an extension here
    // would truncate a folder that merely has a dot in its name.
    let dotted = root.join("archive.old");
    std::fs::create_dir_all(&dotted).unwrap();
    let one_dir = [dotted.as_path()];
    assert_eq!(
        name(&zip.render_for_batch(&TagContext::from_path(&dotted), &one_dir)),
        format!("{}/archive.old.zip", root.display())
    );

    // Several items: the first no longer names the whole, the folder does.
    let many = [file.as_path(), folder.as_path()];
    assert_eq!(
        name(&zip.render_for_batch(&TagContext::from_path(&file), &many)),
        format!(
            "{}/{}.zip",
            root.display(),
            root.file_name().unwrap().to_string_lossy()
        )
    );

    // `{dirname}` keeps its own meaning, whatever the count: commands
    // written before this tag existed must not start behaving differently.
    let by_folder = Opener {
        args: vec!["a".into(), "{dir}/{dirname}.zip".into(), "{files}".into()],
        ..zip.clone()
    };
    let expected = format!(
        "{}/{}.zip",
        root.display(),
        root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(
        name(&by_folder.render_for_batch(&TagContext::from_path(&file), &one_file)),
        expected
    );
    assert_eq!(
        name(&by_folder.render_for_batch(&TagContext::from_path(&file), &many)),
        expected
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_list_tag_runs_once_and_expands_where_it_stands() {
    let o = Opener {
        id: "x".into(),
        label: "7z".into(),
        program: "/usr/bin/7z".into(),
        assoc: None,
        icon: OpenerIcon::None,
        args: vec!["a".into(), "{dir}/{dirname}.7z".into(), "{files}".into()],
        default_exts: vec![],
        used_exts: vec![],
        use_count: 0,
        last_used: 0,
        elevated: false,
        ctx_menu: 0,
        ctx_exts: Vec::new(),
    };
    // `{files}` means ONE run, so it must not be mistaken for a per-file tag.
    assert!(o.expands_list());
    // `{dir}`/`{dirname}` are per-file tags, hence `has_tag` is true here;
    // what matters is that `{files}` alone never makes it true.
    let list_only = Opener {
        args: vec!["a".into(), "out.7z".into(), "{files}".into()],
        ..o.clone()
    };
    assert!(
        !list_only.has_tag(),
        "{{files}} must not be counted as a per-file tag"
    );

    let ctx = TagContext::from_path(Path::new("/home/u/Photos/a.png"));
    let paths = [
        Path::new("/home/u/Photos/a.png"),
        Path::new("/home/u/Photos/b.png"),
    ];
    let files = &paths[..];
    // The archive is named after the FOLDER, which is what archivers do for
    // a multiple selection, and both paths land where the tag stood.
    assert_eq!(
        o.render_for_batch(&ctx, files),
        [
            "a",
            "/home/u/Photos/Photos.7z",
            "/home/u/Photos/a.png",
            "/home/u/Photos/b.png"
        ]
    );

    // The list keeps its position: it can precede a trailing option, which
    // the historical "append at the end" fallback could never express.
    let trailing = Opener {
        args: vec!["-r".into(), "{files}".into(), "--verbose".into()],
        ..o.clone()
    };
    assert_eq!(
        trailing.render_for_batch(&ctx, files),
        [
            "-r",
            "/home/u/Photos/a.png",
            "/home/u/Photos/b.png",
            "--verbose"
        ]
    );
}

#[test]
fn the_names_tag_archives_the_files_without_their_tree() {
    let o = Opener {
        id: "x".into(),
        label: "tar".into(),
        program: "tar".into(),
        assoc: None,
        icon: OpenerIcon::None,
        args: vec![
            "-czf".into(),
            "{dir}/{dirname}.tar.gz".into(),
            "-C".into(),
            "{dir}".into(),
            "{names}".into(),
        ],
        default_exts: vec![],
        used_exts: vec![],
        use_count: 0,
        last_used: 0,
        elevated: false,
        ctx_menu: 0,
        ctx_exts: vec![],
    };
    // Like `{files}`, it means a single run over the whole selection.
    assert!(o.expands_list());

    let ctx = TagContext::from_path(Path::new("/home/u/Docs/a.txt"));
    let paths = [
        Path::new("/home/u/Docs/a.txt"),
        Path::new("/home/u/Docs/b.txt"),
    ];
    assert_eq!(
        o.render_for_batch(&ctx, &paths),
        [
            "-czf",
            "/home/u/Docs/Docs.tar.gz",
            "-C",
            "/home/u/Docs",
            "a.txt",
            "b.txt"
        ],
        "bare names, so the archive holds a.txt and not home/u/Docs/a.txt"
    );

    // `{files}` still yields full paths — the two are not interchangeable.
    let with_paths = Opener {
        args: vec!["-czf".into(), "out.tar.gz".into(), "{files}".into()],
        ..o.clone()
    };
    assert_eq!(
        with_paths.render_for_batch(&ctx, &paths),
        [
            "-czf",
            "out.tar.gz",
            "/home/u/Docs/a.txt",
            "/home/u/Docs/b.txt"
        ]
    );
}

#[test]
fn dirname_is_the_folder_name_while_dir_is_its_path() {
    let ctx = TagContext::from_path(Path::new("/home/u/Photos/a.png"));
    assert_eq!(ctx.dir, "/home/u/Photos");
    assert_eq!(ctx.dirname, "Photos");
    assert_eq!(ctx.name, "a.png");
    assert_eq!(ctx.stem, "a");
    // A path with no parent leaves it empty rather than guessing.
    let bare = TagContext::from_path(Path::new("a.png"));
    assert_eq!(bare.dirname, "");
}

#[test]
fn ctx_menu_mask_round_trips_and_filters() {
    let mut s = OpenerStore::default();
    let a = s.add("Shell Here", "/bin/sh", vec!["--cd={dir}".into()]);
    let b = s.add("Compare", "/bin/compare", vec![]);
    // Default: not pinned.
    assert!(s.for_context(CTX_BACKGROUND).is_empty());
    // Mask set + filter by bit.
    assert!(s.set_ctx_menu(&a, CTX_DIR | CTX_BACKGROUND));
    assert!(s.set_ctx_menu(&b, CTX_FILE));
    assert_eq!(s.for_context(CTX_BACKGROUND).len(), 1);
    assert_eq!(s.for_context(CTX_FILE)[0].id, b);
    assert_eq!(s.for_context(CTX_DIR)[0].id, a);
    // TOML round-trip + backward compat (field absent → 0).
    let toml = toml::to_string_pretty(&s).unwrap();
    let back: OpenerStore = toml::from_str(&toml).unwrap();
    assert_eq!(back.get(&a).unwrap().ctx_menu, CTX_DIR | CTX_BACKGROUND);
    let legacy = "[[openers]]\nid = \"x\"\nlabel = \"X\"\nprogram = \"/bin/x\"\n";
    let parsed: OpenerStore = toml::from_str(legacy).unwrap();
    assert_eq!(parsed.get("x").unwrap().ctx_menu, 0);
    // Duplication keeps the pinning.
    let dup = s.duplicate(&a).unwrap();
    assert_eq!(s.get(&dup).unwrap().ctx_menu, CTX_DIR | CTX_BACKGROUND);
}

#[test]
fn duplicate_inserts_after_and_resets_default_exts() {
    let mut s = OpenerStore::default();
    let a = s.add("A", "/bin/a", vec!["{file}".into()]);
    let _b = s.add("B", "/bin/b", vec![]);
    s.set_elevated(&a, true);
    s.set_default_ext(&a, "txt", true);
    let dup = s.duplicate(&a).unwrap();
    // Inserted RIGHT AFTER the original.
    assert_eq!(s.openers[1].id, dup);
    let d = s.get(&dup).unwrap();
    assert_eq!(d.label, "A");
    assert_eq!(d.program, "/bin/a");
    assert_eq!(d.args, vec!["{file}".to_string()]);
    assert!(d.elevated); // kept
    assert!(d.default_exts.is_empty()); // doesn't steal the "default" status
    // The original keeps its default extension.
    assert_eq!(s.default_for("txt").map(|o| o.id.clone()), Some(a));
}

#[test]
fn suggested_puts_last_used_first() {
    let mut s = OpenerStore::default();
    let a = s.add("A", "/bin/a", vec![]);
    let b = s.add("B", "/bin/b", vec![]);
    let _c = s.add("C", "/bin/c", vec![]);
    let labels = |s: &OpenerStore| {
        s.suggested(10)
            .iter()
            .map(|o| o.label.clone())
            .collect::<Vec<_>>()
    };
    // No usage → the Vec's manual order.
    assert_eq!(labels(&s), ["A", "B", "C"]);
    // A is used then B (same second possible) → B on top, then A, then C.
    s.record_use(&a, None);
    s.record_use(&b, None);
    assert_eq!(labels(&s), ["B", "A", "C"]);
    // Using A again moves it back to the top.
    s.record_use(&a, None);
    assert_eq!(labels(&s), ["A", "B", "C"]);
}

#[test]
fn suggested_for_ext_filters_and_learns() {
    let mut s = OpenerStore::default();
    let player = s.add("Media Player", "/bin/mediaplayer", vec![]);
    let text = s.add("Text Editor", "/bin/textedit", vec![]);
    let _image = s.add("Image Editor", "/bin/imageedit", vec![]);
    // The text editor was used on a .docx, the player on a .mp4 (learning).
    s.record_use(&text, Some("docx"));
    s.record_use(&player, Some("mp4"));
    let labels = |v: Vec<&Opener>| v.iter().map(|o| o.label.clone()).collect::<Vec<_>>();
    // Flyout for a .mp4: ONLY the player (no editor was ever used on one).
    assert_eq!(labels(s.suggested_for_ext("mp4", 8)), ["Media Player"]);
    // Flyout for a .docx: only the text editor.
    // (dot + case normalized)
    assert_eq!(labels(s.suggested_for_ext(".DOCX", 8)), ["Text Editor"]);
    // Extension never opened → empty list (→ "Choose an application…").
    assert!(s.suggested_for_ext("png", 8).is_empty());
    // An explicit `default_ext` also counts.
    s.set_default_ext(&_image, "png", true);
    assert_eq!(labels(s.suggested_for_ext("png", 8)), ["Image Editor"]);
}

#[test]
fn tag_context_splits_path() {
    let c = ctx("/home/u/photos/image.PNG");
    assert_eq!(c.name, "image.PNG");
    assert_eq!(c.stem, "image");
    assert_eq!(c.ext, "png"); // lowercase
    assert_eq!(c.dir, "/home/u/photos");
}

#[test]
fn substitute_tags_and_escapes() {
    let c = ctx("/a b/x.txt");
    assert_eq!(substitute("--in={file}", &c), "--in=/a b/x.txt");
    assert_eq!(substitute("{stem}.{ext}", &c), "x.txt");
    // Escaping + unknown literal tag.
    assert_eq!(substitute("{{lit}} {nope}", &c), "{lit} {nope}");
}

#[test]
fn render_appends_file_when_no_tag() {
    let mut s = OpenerStore::default();
    let id = s.add("ed", "/usr/bin/ed", vec!["-n".into()]);
    let o = s.get(&id).unwrap();
    assert!(!o.has_tag());
    assert_eq!(
        o.render_for_file(&ctx("/a b/x.txt")),
        vec!["-n".to_string(), "/a b/x.txt".to_string()]
    );
}

#[test]
fn render_uses_tags_when_present() {
    let mut s = OpenerStore::default();
    let id = s.add("g", "/usr/bin/g", vec!["--file".into(), "{file}".into()]);
    let o = s.get(&id).unwrap();
    assert!(o.has_tag());
    assert_eq!(
        o.render_for_file(&ctx("/p/y.jpg")),
        vec!["--file".to_string(), "/p/y.jpg".to_string()]
    );
}

#[test]
fn default_ext_is_unique_and_normalized() {
    let mut s = OpenerStore::default();
    let a = s.add("A", "/a", vec![]);
    let b = s.add("B", "/b", vec![]);
    s.set_default_ext(&a, ".PNG", true);
    assert_eq!(s.default_for("png").map(|o| o.id.clone()), Some(a.clone()));
    // B takes png → A loses it (uniqueness).
    s.set_default_ext(&b, "png", true);
    assert_eq!(s.default_for("png").map(|o| o.id.clone()), Some(b.clone()));
    // Removal.
    s.set_default_ext(&b, "png", false);
    assert!(s.default_for("png").is_none());
}

#[test]
fn set_used_exts_normalizes_and_lists_in_flyout() {
    let mut s = OpenerStore::default();
    let a = s.add("A", "/a", vec![]);
    // Manual edit: ".PNG, jpg,, png" → normalized + deduplicated.
    s.set_used_exts(&a, &[".PNG".into(), "jpg".into(), "".into(), "png".into()]);
    assert_eq!(s.get(&a).unwrap().used_exts, vec!["png", "jpg"]);
    // The opener is offered for each normalized extension.
    let labels = |v: Vec<&Opener>| v.iter().map(|o| o.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(s.suggested_for_ext("png", 8)), ["A"]);
    assert_eq!(labels(s.suggested_for_ext("jpg", 8)), ["A"]);
    // Re-editing to empty → no longer offered.
    s.set_used_exts(&a, &[]);
    assert!(s.suggested_for_ext("png", 8).is_empty());
}

#[test]
fn move_reorders() {
    let mut s = OpenerStore::default();
    let a = s.add("A", "/a", vec![]);
    let b = s.add("B", "/b", vec![]);
    s.move_before(&b, Some(&a)); // B before A
    assert_eq!(s.openers[0].id, b);
    assert_eq!(s.openers[1].id, a);
}

#[test]
fn save_load_round_trip() {
    let mut s = OpenerStore::default();
    let id = s.add(
        "Text Editor",
        "/usr/bin/textedit",
        vec!["-n".into(), "{file}".into()],
    );
    s.set_default_ext(&id, "rs", true);
    let dir = std::env::temp_dir().join(format!(
        "favnyr-op-test-{}",
        now_secs() + COUNTER.load(Ordering::Relaxed)
    ));
    let path = dir.join("openers.toml");
    s.save(&path).unwrap();
    assert_eq!(OpenerStore::load(&path), s);
    let _ = std::fs::remove_dir_all(&dir);
}
