/// The environment is handed in, so these assert the PARSING rather than
/// whatever the machine running them happens to define.
fn fake_env(name: &str) -> Option<String> {
    match name {
        "APPDATA" => Some(r"C:\Users\someone\AppData\Roaming".to_owned()),
        "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
        "HOME" => Some("/home/someone".to_owned()),
        "USER" => Some("someone".to_owned()),
        _ => None,
    }
}

#[cfg(windows)]
#[test]
fn windows_variables_expand_only_when_they_exist() {
    let expand = |text: &str| expand_variables_with(text, fake_env);

    assert_eq!(
        expand(r"%APPDATA%\Favnyr"),
        r"C:\Users\someone\AppData\Roaming\Favnyr"
    );
    // Two real variables carry parentheses, so a letters-only name would
    // have missed them.
    assert_eq!(
        expand(r"%ProgramFiles(x86)%\Tool"),
        r"C:\Program Files (x86)\Tool"
    );
    // Anywhere in the path, not just at the start.
    assert_eq!(
        expand(r"C:\x\%APPDATA%"),
        r"C:\x\C:\Users\someone\AppData\Roaming"
    );

    // Untouched: an unknown name reaches the caller so it can say so, and a
    // percent sign that names nothing is just a character in a folder name.
    assert_eq!(expand("%NOT_A_VARIABLE%"), "%NOT_A_VARIABLE%");
    assert_eq!(expand("Report 100% final"), "Report 100% final");
    assert_eq!(expand("50%-75%"), "50%-75%");
    assert_eq!(expand("%%"), "%%");
    assert_eq!(expand("%unterminated"), "%unterminated");
}

#[cfg(not(windows))]
#[test]
fn unix_variables_expand_in_both_spellings() {
    let expand = |text: &str| expand_variables_with(text, fake_env);

    assert_eq!(expand("$HOME/Documents"), "/home/someone/Documents");
    assert_eq!(expand("${HOME}/Documents"), "/home/someone/Documents");
    // Mid-path, which is the form a shell user reaches for.
    assert_eq!(expand("/home/$USER/Documents"), "/home/someone/Documents");
    // The braces are what allow a name to be followed by a letter.
    assert_eq!(expand("${USER}name"), "someonename");
    assert_eq!(expand("$USERname"), "$USERname");

    // Untouched: unknown names, and a dollar that opens nothing.
    assert_eq!(expand("$NOT_A_VARIABLE/x"), "$NOT_A_VARIABLE/x");
    assert_eq!(expand("price $ 5"), "price $ 5");
    assert_eq!(expand("${unterminated"), "${unterminated");
}

#[test]
fn a_plain_path_is_returned_unchanged() {
    // Nothing to expand must mean nothing altered, on either platform.
    let typed = if cfg!(windows) {
        r"C:\Users\someone\Documents"
    } else {
        "/home/someone/Documents"
    };
    assert_eq!(expand_typed_path(typed), PathBuf::from(typed));
}

#[test]
fn the_home_shorthand_still_works() {
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    assert_eq!(expand_typed_path("~"), home);
    assert_eq!(expand_typed_path("~/Documents"), home.join("Documents"));
    #[cfg(windows)]
    assert_eq!(expand_typed_path(r"~\Documents"), home.join("Documents"));
    // Only at the very start: a tilde inside a name is a name.
    assert_eq!(expand_typed_path("backup~1"), PathBuf::from("backup~1"));
}
use super::*;

/// Wording the interface supplies for English and French. Mirrored here so
/// the formatting assertions keep testing the exact rendered strings.
const EN: SizeUnits<'static> = SizeUnits {
    steps: ["B", "KB", "MB", "GB", "TB"],
    decimal: '.',
};
const FR: SizeUnits<'static> = SizeUnits {
    steps: ["o", "Ko", "Mo", "Go", "To"],
    decimal: ',',
};
const FR_AGE: AgeUnits<'static> = AgeUnits {
    minute: "min",
    day: "j",
    month: "mois",
    year: "an",
    now: "à l'instant",
};

#[test]
fn classify_folder_overrides_extension() {
    assert_eq!(classify_kind(Some("zip"), true), FileKind::Folder);
}

#[test]
fn file_kind_code_round_trips() {
    // Every variant must survive a trip through its wire code: the UI row
    // carries only the integer, and it is decoded back to a `FileKind`.
    for kind in [
        FileKind::Folder,
        FileKind::File,
        FileKind::Application,
        FileKind::Archive,
        FileKind::Audio,
        FileKind::Document,
        FileKind::Image,
        FileKind::Video,
        FileKind::Config,
    ] {
        assert_eq!(FileKind::from_code(kind.as_i32()), Some(kind));
    }
    // A code no variant maps to is rejected rather than defaulting to a
    // variant (a foreign/stale value must never become `Folder`).
    assert_eq!(FileKind::from_code(-1), None);
    assert_eq!(FileKind::from_code(9), None);
    assert_eq!(FileKind::from_code(i32::MAX), None);
}

#[test]
fn unc_server_root_detects_bare_server_only() {
    use std::path::Path;
    // Server root (no share) → host name.
    assert_eq!(unc_server_root(Path::new(r"\\NAS")), Some("NAS".into()));
    assert_eq!(unc_server_root(Path::new(r"\\NAS\")), Some("NAS".into()));
    assert_eq!(
        unc_server_root(Path::new(r"\\192.168.1.50")),
        Some("192.168.1.50".into())
    );
    // With a share OR a local path OR just "\\" → not a server root.
    assert_eq!(unc_server_root(Path::new(r"\\NAS\media")), None);
    assert_eq!(unc_server_root(Path::new(r"\\NAS\media\sub")), None);
    assert_eq!(unc_server_root(Path::new(r"C:\Users")), None);
    assert_eq!(unc_server_root(Path::new(r"\\")), None);
}

#[cfg(windows)]
#[test]
fn unc_share_parent_only_at_share_root() {
    use std::path::Path;
    // Share root → server root.
    assert_eq!(
        unc_share_parent(Path::new(r"\\NAS\media")),
        Some(PathBuf::from(r"\\NAS"))
    );
    assert_eq!(
        unc_share_parent(Path::new(r"\\NAS\media\")),
        Some(PathBuf::from(r"\\NAS"))
    );
    // Share subfolder → None (Path::parent() is enough).
    assert_eq!(unc_share_parent(Path::new(r"\\NAS\media\sub")), None);
    // Local paths / server root: None.
    assert_eq!(unc_share_parent(Path::new(r"C:\Users")), None);
    assert_eq!(unc_share_parent(Path::new(r"\\NAS")), None);
}

#[test]
fn is_unc_path_only_true_for_network_shares() {
    use std::path::Path;
    assert!(is_unc_path(Path::new(r"\\NAS\media")));
    assert!(is_unc_path(Path::new(r"\\NAS")));
    // LOCAL namespaces (verbatim / device) → not network.
    assert!(!is_unc_path(Path::new(r"\\?\C:\x")));
    assert!(!is_unc_path(Path::new(r"\\.\PhysicalDrive0")));
    assert!(!is_unc_path(Path::new(r"C:\Users")));
}

#[test]
fn classify_known_extensions() {
    // Both lists are searched by bisection: unsorted, they would silently
    // stop matching some of their own entries.
    assert!(IMAGE_EXTENSIONS.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(ARCHIVE_EXTENSIONS.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(classify_kind(Some("PNG"), false), FileKind::Image);
    for ext in ["af", "afdesign", "afphoto", "afpub"] {
        assert_eq!(classify_kind(Some(ext), false), FileKind::Image, "{ext}");
    }
    // Every archive extension still classifies as one after the move out of
    // the match arm into a bisected list.
    for ext in ARCHIVE_EXTENSIONS {
        assert_eq!(classify_kind(Some(ext), false), FileKind::Archive, "{ext}");
    }
    assert_eq!(classify_kind(Some("toml"), false), FileKind::Config);
    assert_eq!(classify_kind(Some("mp3"), false), FileKind::Audio);
    assert_eq!(classify_kind(Some("mp4"), false), FileKind::Video);
    assert_eq!(
        classify_kind(Some("AppImage"), false),
        FileKind::Application
    );
    assert_eq!(classify_kind(Some("zip"), false), FileKind::Archive);
    assert_eq!(classify_kind(Some("pdf"), false), FileKind::Document);
}

#[test]
fn classify_unknown_extension_is_file() {
    assert_eq!(classify_kind(Some("xyz123"), false), FileKind::File);
    assert_eq!(classify_kind(None, false), FileKind::File);
}

#[test]
fn the_capacity_warning_weighs_the_share_against_the_amount() {
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * GIB;

    // Roomy.
    assert_eq!(free_space_level(38 * GIB, 64 * GIB), 0);
    assert_eq!(free_space_level(180 * GIB, 460 * GIB), 0);
    // A share alone would condemn this one: 8.75% free, but 700 GiB is
    // nobody's emergency. The ceiling is what keeps a big archive quiet.
    assert_eq!(free_space_level(700 * GIB, 8 * TIB), 0);
    // Small removable drives, mostly EMPTY, must read as roomy — the whole
    // point of dropping the absolute floors. A 3 GB stick two-thirds free
    // never reaches 4 GiB free; a 16 GB stick a quarter free never reaches
    // 16 GiB. Both used to light up red / amber.
    assert_eq!(free_space_level(2 * GIB, 3 * GIB), 0); // 67% free
    assert_eq!(free_space_level(4 * GIB, 16 * GIB), 0); // 25% free

    // Low: the share, not any floor, is what flags these.
    assert_eq!(free_space_level(12 * GIB, 64 * GIB), 1); // 18.75%
    assert_eq!(free_space_level(200 * GIB, 8 * TIB), 1);

    // Critical.
    assert_eq!(free_space_level(5 * GIB, 64 * GIB), 2); // 7.8%
    assert_eq!(free_space_level(50 * GIB, TIB), 2); // 5%
    assert_eq!(free_space_level(100 * GIB, 8 * TIB), 2); // under the ceiling
    // A large disk with almost nothing left: the share alone (0.6%) is well
    // under a tenth, so it is caught without any floor.
    assert_eq!(free_space_level(3 * GIB, 500 * GIB), 2);
    // A genuinely full SMALL drive is still flagged — the share scales down
    // with it: 0.5 GB on an 8 GB stick is 6%.
    assert_eq!(free_space_level(GIB / 2, 8 * GIB), 2);

    // Unmeasurable volume: no capacity, so nothing to warn about.
    assert_eq!(free_space_level(0, 0), 0);
}

#[test]
fn a_capacity_pair_shares_one_unit_so_the_two_figures_compare() {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const TIB: u64 = 1024 * GIB;

    // Both figures in the total's unit — calling `format_size` twice would
    // have picked a unit per number.
    assert_eq!(format_used_total(26 * GIB, 64 * GIB, EN), "26.0 / 64.0 GB");
    assert_eq!(format_used_total(26 * GIB, 64 * GIB, FR), "26,0 / 64,0 Go");

    // A barely used volume keeps the total's unit and reads near zero,
    // which is what it means. No special case is needed now that the
    // figure counts UP alongside the bar instead of down against it.
    assert_eq!(format_used_total(900 * MIB, 2 * TIB, EN), "0.0 / 2.0 TB");

    // Kept honest against the size column: same base-1024 convention, so a
    // drive sold as "64 GB" reads the same here as everywhere else.
    assert_eq!(format_size(64_000_000_000, EN), "59.6 GB");
    assert_eq!(
        format_used_total(32_000_000_000, 64_000_000_000, EN),
        "29.8 / 59.6 GB"
    );

    // Tiny volume: bytes carry no decimal.
    assert_eq!(format_used_total(200, 900, EN), "200 / 900 B");
}

#[test]
fn only_programs_and_untyped_files_can_be_launched() {
    // Real candidates for "launch with these files".
    assert!(FileKind::Application.can_be_program()); // .sh, .appimage, binary
    assert!(FileKind::File.can_be_program()); // binary/script WITHOUT an extension
    // Data: never a program, EVEN with the executable bit (FAT/NTFS
    // 0777 mount) → fixes the incorrect "Open with this program" label
    // on an image/document/etc. on Linux.
    assert!(!FileKind::Image.can_be_program());
    assert!(!FileKind::Video.can_be_program());
    assert!(!FileKind::Audio.can_be_program());
    assert!(!FileKind::Document.can_be_program());
    assert!(!FileKind::Archive.can_be_program());
    assert!(!FileKind::Config.can_be_program());
    assert!(!FileKind::Folder.can_be_program());
}

#[test]
fn format_age_buckets_and_text() {
    const H: i64 = 3600;
    const D: i64 = 86_400;
    let now = 1_000_000_000;
    // < 1 min → "à l'instant".
    assert_eq!(format_age(now - 30, now, FR_AGE), "à l'instant");
    assert_eq!(age_bucket(now - 30, now), 0);
    // minutes.
    assert_eq!(format_age(now - 42 * 60, now, FR_AGE), "42 min");
    // hours + minutes ("2 h 05'").
    assert_eq!(format_age(now - (2 * H + 5 * 60), now, FR_AGE), "2 h 05'");
    // days.
    assert_eq!(format_age(now - 25 * D, now, FR_AGE), "25j");
    assert_eq!(age_bucket(now - 25 * D, now), 4);
    // mtime in the future (clock) → clamped to 0 → "à l'instant".
    assert_eq!(format_age(now + 9999, now, FR_AGE), "à l'instant");
}

#[test]
fn age_bucket_is_monotonic_cold() {
    const D: i64 = 86_400;
    let now = 1_000_000_000;
    // The older it is, the bigger the bucket (colder).
    assert!(age_bucket(now - 30, now) < age_bucket(now - 10 * D, now));
    assert_eq!(age_bucket(now - 400 * D, now), 6);
}

#[test]
fn format_size_units() {
    assert_eq!(format_size(0, EN), "0 B");
    assert_eq!(format_size(512, EN), "512 B");
    assert_eq!(format_size(1024, EN), "1.0 KB");
    assert_eq!(format_size(1536, EN), "1.5 KB");
    assert_eq!(format_size(1024 * 1024, EN), "1.0 MB");
}

#[test]
fn format_size_fr_uses_o_and_comma() {
    assert_eq!(format_size(1024, FR), "1,0 Ko");
    assert_eq!(format_size(1_500_000_000, FR), "1,4 Go");
}

#[test]
fn format_mtime_iso() {
    // Known boundaries (offset 0 = UTC):
    assert_eq!(format_mtime(0, 0), "1970-01-01 00:00");
    assert_eq!(format_mtime(86_400, 0), "1970-01-02 00:00");
    // 2000-01-01 00:00:00 UTC = 946_684_800
    assert_eq!(format_mtime(946_684_800, 0), "2000-01-01 00:00");
    // Format is always `YYYY-MM-DD HH:MM` (16 chars).
    assert_eq!(format_mtime(1_780_948_604, 0).len(), 16);
    // Local offset applied: +2 h → same clock shifted by 2 h.
    assert_eq!(format_mtime(0, 2 * 3600), "1970-01-01 02:00");
    // Negative offset crossing midnight.
    assert_eq!(format_mtime(0, -3600), "1969-12-31 23:00");
}

#[test]
fn sort_folders_first_then_name_asc() {
    let mut v = vec![
        make_entry("zeta.txt", false),
        make_entry("alpha-dir", true),
        make_entry("Bravo.png", false),
        make_entry("aaa-dir", true),
    ];
    sort(
        &mut v,
        SortColumn::Name,
        SortOrder::Asc,
        GroupMode::FoldersFirst,
    );
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["aaa-dir", "alpha-dir", "Bravo.png", "zeta.txt"]);
}

#[test]
fn sort_files_first_then_name_asc() {
    let mut v = vec![
        make_entry("zeta.txt", false),
        make_entry("alpha-dir", true),
        make_entry("Bravo.png", false),
        make_entry("aaa-dir", true),
    ];
    sort(
        &mut v,
        SortColumn::Name,
        SortOrder::Asc,
        GroupMode::FilesFirst,
    );
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    // Files (alpha) then folders (alpha).
    assert_eq!(names, ["Bravo.png", "zeta.txt", "aaa-dir", "alpha-dir"]);
}

#[test]
fn sort_mixed_interleaves_by_name_asc() {
    let mut v = vec![
        make_entry("zeta.txt", false),
        make_entry("alpha-dir", true),
        make_entry("Bravo.png", false),
        make_entry("aaa-dir", true),
    ];
    sort(&mut v, SortColumn::Name, SortOrder::Asc, GroupMode::Mixed);
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    // No grouping: everything is sorted together (case-insensitive).
    assert_eq!(names, ["aaa-dir", "alpha-dir", "Bravo.png", "zeta.txt"]);
}

/// An entry whose `kind` comes from its extension, the way a listing
/// classifies it.
fn typed_entry(name: &str, is_dir: bool) -> Entry {
    let ext = Path::new(name).extension().and_then(|s| s.to_str());
    Entry {
        kind: classify_kind(ext, is_dir),
        ..make_entry(name, is_dir)
    }
}

#[test]
fn category_groups_folders_then_media_then_everything_else() {
    let mut v = vec![
        typed_entry("notes.txt", false),
        typed_entry("clip.mp4", false),
        typed_entry("song.mp3", false),
        typed_entry("archive.zip", false),
        typed_entry("setup.exe", false),
        typed_entry("photo.png", false),
        typed_entry("zeta-dir", true),
        typed_entry("alpha-dir", true),
    ];
    sort(
        &mut v,
        SortColumn::Name,
        SortOrder::Asc,
        GroupMode::Category,
    );
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "alpha-dir",
            "zeta-dir",
            "photo.png",
            "clip.mp4",
            "song.mp3",
            "notes.txt",
            "archive.zip",
            "setup.exe",
        ]
    );
}

#[test]
fn category_mode_keeps_the_criterion_inside_a_section() {
    let mut v = vec![
        typed_entry("b.png", false),
        typed_entry("a.png", false),
        typed_entry("m.mp3", false),
    ];
    sort(
        &mut v,
        SortColumn::Name,
        SortOrder::Desc,
        GroupMode::Category,
    );
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    // The section order is never reversed (like folders-first): images
    // stay above audio, while the criterion orders names inside a section.
    assert_eq!(names, ["b.png", "a.png", "m.mp3"]);
}

#[test]
fn category_maps_kinds_and_unknown_codes() {
    assert_eq!(Category::of(FileKind::Folder), Category::Folder);
    assert_eq!(Category::of(FileKind::Image), Category::Image);
    assert_eq!(Category::of(FileKind::Video), Category::Video);
    assert_eq!(Category::of(FileKind::Audio), Category::Audio);
    assert_eq!(Category::of(FileKind::Document), Category::Document);
    for kind in [
        FileKind::Archive,
        FileKind::Application,
        FileKind::Config,
        FileKind::File,
    ] {
        assert_eq!(Category::of(kind), Category::Other);
    }
    assert_eq!(Category::of_code(FileKind::Image.as_i32()), Category::Image);
    // A code no variant maps to lands in Other instead of failing.
    assert_eq!(Category::of_code(-1), Category::Other);
    assert_eq!(Category::from_code("video"), Some(Category::Video));
    assert_eq!(Category::from_code("nope"), None);
}

#[test]
fn sort_name_desc_keeps_folders_on_top() {
    let mut v = vec![
        make_entry("zeta.txt", false),
        make_entry("alpha-dir", true),
        make_entry("Bravo.png", false),
        make_entry("aaa-dir", true),
    ];
    sort(
        &mut v,
        SortColumn::Name,
        SortOrder::Desc,
        GroupMode::FoldersFirst,
    );
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    // Descending reverses the names WITHIN each block, but not the grouping:
    // folders stay on top.
    assert_eq!(names, ["alpha-dir", "aaa-dir", "zeta.txt", "Bravo.png"]);
}

#[test]
fn sort_ext_groups_by_extension_then_name() {
    let mut v = vec![
        make_entry("b.txt", false),
        make_entry("a.txt", false),
        make_entry("C.png", false),
        make_entry("a.png", false),
    ];
    sort(&mut v, SortColumn::Ext, SortOrder::Asc, GroupMode::Mixed);
    let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
    // Grouped by extension (png before txt), then by name (case-insensitive).
    assert_eq!(names, ["a.png", "C.png", "a.txt", "b.txt"]);
}

#[test]
fn sort_size_desc_keeps_folders_on_top() {
    let mut v = vec![
        sized_entry("big.bin", 10_000_000),
        sized_entry("small.bin", 10),
        make_entry("dir", true),
    ];
    sort(
        &mut v,
        SortColumn::Size,
        SortOrder::Desc,
        GroupMode::FoldersFirst,
    );
    assert!(v[0].is_dir, "folder must remain on top regardless of size");
    assert_eq!(v[1].name, "big.bin");
    assert_eq!(v[2].name, "small.bin");
}

#[test]
fn list_dir_reads_temp() {
    let tmp = std::env::temp_dir().join(format!("favnyr-fs-test-{}", nano()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("a.txt"), b"hello").unwrap();
    std::fs::create_dir_all(tmp.join("sub")).unwrap();
    std::fs::write(tmp.join(".hidden"), b"x").unwrap();

    let mut entries = list_dir(&tmp, false).unwrap();
    sort(
        &mut entries,
        SortColumn::Name,
        SortOrder::Asc,
        GroupMode::FoldersFirst,
    );
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["sub", "a.txt"]);

    let entries_hidden = list_dir(&tmp, true).unwrap();
    assert_eq!(entries_hidden.len(), 3);

    // Hidden counter: returned regardless of `include_hidden`.
    let (visible, hidden_n) = list_dir_counted(&tmp, false).unwrap();
    assert_eq!(visible.len(), 2); // sub + a.txt
    assert_eq!(hidden_n, 1); // .hidden
    let (all, hidden_n2) = list_dir_counted(&tmp, true).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(hidden_n2, 1);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn list_dir_follows_symlink_to_dir() {
    // A symbolic link to a FOLDER must be classified `is_dir = true`
    // (otherwise Favnyr would open it in the file manager instead of navigating).
    let tmp = std::env::temp_dir().join(format!("favnyr-symlink-{}", nano()));
    std::fs::create_dir_all(tmp.join("realdir")).unwrap();
    let link = tmp.join("linkdir");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(tmp.join("realdir"), &link).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(tmp.join("realdir"), &link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let made = false;
    // Link creation is often refused without privilege (Windows) → only
    // assert if the link was actually created (keeps the test non-flaky in restricted CI).
    if made {
        let (entries, _) = list_dir_counted(&tmp, false).unwrap();
        let e = entries
            .iter()
            .find(|e| e.name == "linkdir")
            .expect("linkdir listed");
        assert!(e.is_dir, "a link to a directory must have is_dir = true");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn list_dir_reports_target_size_and_date_for_a_symlink() {
    // A link stores a path, so its own length is a handful of bytes and its
    // own timestamp is when it was created. Showing those would describe
    // the link instead of the file the user sees listed.
    let tmp = std::env::temp_dir().join(format!("favnyr-symlink-size-{}", nano()));
    std::fs::create_dir_all(&tmp).unwrap();
    let target = tmp.join("payload.bin");
    let payload = vec![7u8; 4096];
    std::fs::write(&target, &payload).unwrap();
    let link = tmp.join("payload-link.bin");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let made = false;

    // Creating a link is refused without privilege on Windows; asserting
    // only when one exists keeps the test meaningful and non-flaky.
    if made {
        let (entries, _) = list_dir_counted(&tmp, false).unwrap();
        let linked = entries
            .iter()
            .find(|e| e.name == "payload-link.bin")
            .expect("link listed");
        let real = entries
            .iter()
            .find(|e| e.name == "payload.bin")
            .expect("target listed");
        assert!(linked.is_symlink, "the entry is still marked as a link");
        assert_eq!(
            linked.size_bytes,
            Some(payload.len() as u64),
            "a link must report the size of its target"
        );
        assert_eq!(
            linked.mtime_unix, real.mtime_unix,
            "a link must report the date of its target"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn list_dir_reports_target_executable_bits_including_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let tmp = std::env::temp_dir().join(format!("favnyr-executable-{}", nano()));
    std::fs::create_dir_all(&tmp).unwrap();
    let executable = tmp.join("tool");
    let plain = tmp.join("plain");
    std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
    std::fs::write(&plain, b"text\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&executable, tmp.join("tool-link")).unwrap();
    symlink(&plain, tmp.join("plain-link")).unwrap();
    symlink(tmp.join("missing"), tmp.join("broken-link")).unwrap();

    let entries = list_dir(&tmp, false).unwrap();
    let executable_of = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.executable)
            .unwrap()
    };
    assert!(executable_of("tool"));
    assert!(executable_of("tool-link"));
    assert!(!executable_of("plain"));
    assert!(!executable_of("plain-link"));
    assert!(!executable_of("broken-link"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn recursive_max_mtime_respects_depth() {
    use std::time::{Duration, UNIX_EPOCH};
    let tmp = std::env::temp_dir().join(format!("favnyr-rmtime-{}", nano()));
    std::fs::create_dir_all(tmp.join("sub")).unwrap();
    std::fs::write(tmp.join("a.txt"), b"x").unwrap();
    let deep = tmp.join("sub").join("deep.txt");
    std::fs::write(&deep, b"y").unwrap();

    // Explicit FUTURE mtime on the deep file (level 2).
    let future: i64 = 4_000_000_000; // ~2096, > any "current" mtime
    std::fs::File::options()
        .write(true)
        .open(&deep)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(future as u64))
        .unwrap();

    // depth 2 reaches deep.txt; depth 1 (direct children) does not.
    assert_eq!(recursive_max_mtime(&tmp, 2), Some(future));
    let d1 = recursive_max_mtime(&tmp, 1).unwrap();
    assert!(d1 < future, "depth 1 must NOT reach level 2");
    // depth 0 = the folder's own mtime only (≤ depth 1).
    let d0 = recursive_max_mtime(&tmp, 0).unwrap();
    assert!(d0 <= d1);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn recursive_folder_stats_sums_size_by_depth() {
    let tmp = std::env::temp_dir().join(format!("favnyr-fsize-{}", nano()));
    std::fs::create_dir_all(tmp.join("sub").join("deep")).unwrap();
    std::fs::write(tmp.join("a.txt"), [0u8; 10]).unwrap(); // level 1
    std::fs::write(tmp.join("sub").join("b.txt"), [0u8; 20]).unwrap(); // level 2
    std::fs::write(tmp.join("sub").join("deep").join("c.txt"), [0u8; 30]).unwrap(); // level 3

    // Size accumulates by depth; folders themselves add nothing.
    assert_eq!(recursive_folder_stats(&tmp, 0, 1).1, Some(10));
    assert_eq!(recursive_folder_stats(&tmp, 0, 2).1, Some(30));
    assert_eq!(recursive_folder_stats(&tmp, 0, 3).1, Some(60));
    // A depth of 0 disables that metric.
    assert_eq!(recursive_folder_stats(&tmp, 0, 0).1, None);
    assert_eq!(recursive_folder_stats(&tmp, 0, 2).0, None);
    // Unified: one walk returns both, and each half matches its single-metric call.
    let (mtime, size) = recursive_folder_stats(&tmp, 2, 2);
    assert!(mtime.is_some(), "mtime computed");
    assert_eq!(size, Some(30), "size at depth 2");

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Benchmark for listing 10,000 entries with a 500 ms target. Creates the
/// folder on the fly, measures `list_dir + sort`, prints the time, and
/// checks the target. Ignored by default, since creating the files is slow:
/// run with `cargo test -p favnyr-core bench_list_10k -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_list_10k() {
    let tmp = std::env::temp_dir().join(format!("favnyr-bench-{}", nano()));
    std::fs::create_dir_all(&tmp).unwrap();
    for i in 0..10_000 {
        std::fs::write(tmp.join(format!("file_{i}.txt")), b"").unwrap();
    }

    let t0 = std::time::Instant::now();
    let mut entries = list_dir(&tmp, false).unwrap();
    let listed = t0.elapsed();
    let t1 = std::time::Instant::now();
    sort(
        &mut entries,
        SortColumn::Name,
        SortOrder::Asc,
        GroupMode::FoldersFirst,
    );
    let sorted = t1.elapsed();
    let total = t0.elapsed();

    println!(
        "bench_list_10k: {} entries | list_dir={:?} sort={:?} total={:?}",
        entries.len(),
        listed,
        sorted,
        total
    );
    assert_eq!(entries.len(), 10_000);
    assert!(
        total.as_millis() < 500,
        "listing 10k should be < 500ms, measured: {total:?}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

fn make_entry(name: &str, is_dir: bool) -> Entry {
    Entry {
        name: name.into(),
        path: PathBuf::from(name),
        size_bytes: if is_dir { None } else { Some(0) },
        mtime_unix: None,
        is_dir,
        kind: if is_dir {
            FileKind::Folder
        } else {
            FileKind::File
        },
        hidden: false,
        is_symlink: false,
        executable: false,
    }
}

fn sized_entry(name: &str, size: u64) -> Entry {
    Entry {
        name: name.into(),
        path: PathBuf::from(name),
        size_bytes: Some(size),
        mtime_unix: None,
        is_dir: false,
        kind: FileKind::File,
        hidden: false,
        is_symlink: false,
        executable: false,
    }
}

fn nano() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
