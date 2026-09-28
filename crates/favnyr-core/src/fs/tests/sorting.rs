use super::*;

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
