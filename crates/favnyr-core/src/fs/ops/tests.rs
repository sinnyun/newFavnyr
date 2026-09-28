use super::*;
use std::io::Write;

fn tempdir() -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "favnyr-ops-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn path_equality_follows_platform_case_rules_without_io() {
    #[cfg(windows)]
    assert!(paths_equal(
        Path::new(r"C:\Users\User\Pictures"),
        Path::new(r"c:\users\user\pictures")
    ));
    #[cfg(not(windows))]
    assert!(!paths_equal(
        Path::new("/home/User/Pictures"),
        Path::new("/home/user/pictures")
    ));
}

fn write_file(p: &Path, content: &[u8]) {
    let mut f = std::fs::File::create(p).unwrap();
    f.write_all(content).unwrap();
}

#[test]
fn copy_path_handles_directory_symlink() {
    // Depending on link-creation privilege, the copy produces either a
    // link or materialized content. In both cases, `dst` must be an
    // existing folder containing the target file.
    let dir = tempdir();
    let real = dir.join("realdir");
    std::fs::create_dir_all(&real).unwrap();
    write_file(&real.join("a.txt"), b"hello");
    let link = dir.join("linkdir");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&real, &link).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&real, &link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let made = false;
    if made {
        let dst = dir.join("copied");
        copy_path(&link, &dst).expect("copying a directory link must not fail");
        // `dst` exists and behaves like a folder (link followed or copied).
        assert!(dst.exists(), "the copy must exist");
        assert!(std::fs::metadata(&dst).map(|m| m.is_dir()).unwrap_or(false));
        assert!(
            dst.join("a.txt").exists(),
            "the target's contents must be reachable"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn permanent_delete_removes_a_directory_symlink_without_touching_its_target() {
    // A folder symlink/junction must be deleted as a LINK: the link goes, the
    // target and its contents stay. Guards both the Windows error (remove_file
    // on a directory symlink) and the data-loss trap (recursing into the
    // target). Skipped where link creation needs a privilege we don't have.
    let dir = tempdir();
    let real = dir.join("realdir");
    std::fs::create_dir_all(&real).unwrap();
    write_file(&real.join("keep.txt"), b"keep");
    let link = dir.join("linkdir");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&real, &link).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&real, &link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let made = false;
    if made {
        permanent_delete(&link).expect("deleting a folder symlink must succeed");
        assert!(
            std::fs::symlink_metadata(&link).is_err(),
            "the symlink itself must be gone"
        );
        assert!(
            real.join("keep.txt").exists(),
            "the target's contents must be untouched"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn split_name_handles_dotfiles_and_extensions() {
    assert_eq!(split_name("notes.txt"), ("notes".into(), "txt".into()));
    assert_eq!(
        split_name("archive.tar.gz"),
        ("archive.tar".into(), "gz".into())
    );
    assert_eq!(split_name(".bashrc"), (".bashrc".into(), "".into()));
    assert_eq!(split_name("no_ext"), ("no_ext".into(), "".into()));
    // A purely numeric suffix is not an extension.
    assert_eq!(split_name("MyFile 5.7"), ("MyFile 5.7".into(), "".into()));
    assert_eq!(split_name("data.2024"), ("data.2024".into(), "".into()));
    // But a suffix containing a letter remains an extension.
    assert_eq!(split_name("archive.7z"), ("archive".into(), "7z".into()));
}

#[test]
fn ext_of_types_dotfiles_and_lowercases() {
    // Typing (column/sort/filter): a dotfile's suffix IS an extension.
    assert_eq!(ext_of(".gitignore"), "gitignore");
    assert_eq!(ext_of(".config.json"), "json");
    assert_eq!(ext_of("Photo.JPG"), "jpg"); // lowercased
    assert_eq!(ext_of("archive.tar.gz"), "gz");
    // Consistent with split_name: purely numeric suffix / no dot → empty.
    assert_eq!(ext_of("data.2024"), "");
    assert_eq!(ext_of("README"), "");
    assert_eq!(ext_of("archive."), "");
}

#[test]
fn strip_copy_suffix_removes_trailing_marker() {
    assert_eq!(strip_copy_suffix("notes"), "notes");
    assert_eq!(strip_copy_suffix("notes - Copy01"), "notes");
    assert_eq!(strip_copy_suffix("notes - Copy12"), "notes");
    // Not a valid marker → unchanged.
    assert_eq!(strip_copy_suffix("notes - Copy"), "notes - Copy");
    assert_eq!(strip_copy_suffix("my - Copyright"), "my - Copyright");
}

#[test]
fn unique_sibling_uses_copy_scheme() {
    let dir = tempdir();
    let orig = dir.join("notes.txt");
    // Not created yet → returns the path unchanged.
    assert_eq!(unique_sibling(&orig), orig);
    write_file(&orig, b"hello");

    let p1 = unique_sibling(&orig);
    assert_eq!(p1.file_name().unwrap(), "notes - Copy01.txt");
    write_file(&p1, b"copy1");

    let p2 = unique_sibling(&orig);
    assert_eq!(p2.file_name().unwrap(), "notes - Copy02.txt");

    // Copying a copy again does not stack suffixes.
    let p3 = unique_sibling(&p1);
    assert_eq!(p3.file_name().unwrap(), "notes - Copy02.txt");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unique_sibling_numeric_suffix_not_extension() {
    let dir = tempdir();
    let orig = dir.join("MyFile 5.7");
    write_file(&orig, b"x");
    let p1 = unique_sibling(&orig);
    assert_eq!(p1.file_name().unwrap(), "MyFile 5.7 - Copy01");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn copy_path_file_and_dir_recursive() {
    let dir = tempdir();
    let src_dir = dir.join("src");
    std::fs::create_dir(&src_dir).unwrap();
    write_file(&src_dir.join("a.txt"), b"A");
    std::fs::create_dir(src_dir.join("sub")).unwrap();
    write_file(&src_dir.join("sub").join("b.txt"), b"B");

    let dst_dir = dir.join("dst");
    copy_path(&src_dir, &dst_dir).unwrap();

    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"A");
    assert_eq!(
        std::fs::read(dst_dir.join("sub").join("b.txt")).unwrap(),
        b"B"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn path_size_sums_recursively() {
    let dir = tempdir();
    let sub = dir.join("d");
    std::fs::create_dir(&sub).unwrap();
    write_file(&dir.join("a.bin"), &[0u8; 100]);
    write_file(&sub.join("b.bin"), &[0u8; 250]);
    assert_eq!(path_size(&dir), 350);
    assert_eq!(path_size(&dir.join("a.bin")), 100);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_within_matches_only_real_containment() {
    let root = Path::new("/folder01");
    assert!(is_within(Path::new("/folder01"), root), "itself counts");
    assert!(is_within(Path::new("/folder01/sub"), root));
    assert!(is_within(Path::new("/folder01/sub/deep"), root));
    // A shared spelling prefix is not containment: this is the case a
    // plain string comparison gets wrong.
    assert!(!is_within(Path::new("/folder0123"), root));
    assert!(!is_within(Path::new("/folder02"), root));
    // The other way round, and unrelated branches.
    assert!(!is_within(root, Path::new("/folder01/sub")));
    assert!(!is_within(Path::new("/other"), root));
}

#[cfg(windows)]
#[test]
fn is_within_ignores_case_on_windows() {
    // Paths reaching an operation can come from another application's
    // clipboard, which spells them however it likes.
    assert!(is_within(
        Path::new(r"C:\Folder01\Sub"),
        Path::new(r"c:\folder01")
    ));
}

/// A directory link pointing back into the tree being copied used to make
/// the walk re-enter itself forever, each turn adding one level to the
/// destination. Windows only: there, a link that cannot be recreated is
/// materialised by following it, which is what opens the cycle. Unix
/// recreates the link and never walks through it.
///
/// The junction is built with the system tool, because `symlink_dir` needs
/// a privilege that depends on the account: an administrator or a machine
/// in developer mode has it, a plain account does not. Both outcomes are
/// correct and the test asserts whichever one applies — with the
/// privilege the link is recreated and the cycle never opens; without it
/// the copy materialises the target and must catch itself.
#[cfg(windows)]
#[test]
fn copy_refuses_a_directory_link_that_loops_back() {
    let dir = tempdir();
    let src = dir.join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    write_file(&src.join("a.bin"), &[7u8; 10]);

    let loop_link = src.join("sub").join("back");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&loop_link)
        .arg(&src)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !made {
        std::fs::remove_dir_all(&dir).ok();
        return; // junctions unavailable (filesystem without reparse points)
    }

    // Which branch of the copy this account reaches. The probe is the very
    // call the copy makes, so it cannot disagree with it.
    let probe = dir.join("probe");
    let recreates_links = std::os::windows::fs::symlink_dir(&src, &probe).is_ok();
    std::fs::remove_dir(&probe).ok(); // a directory link, never its target

    let mut skipped: Vec<String> = Vec::new();
    let status = copy_tree_progress(
        &src,
        &dir.join("dst"),
        &mut |_| {},
        &mut |path, _| skipped.push(path.file_name().unwrap().to_string_lossy().into_owned()),
        &|| false,
    )
    .unwrap();

    assert_eq!(status, OpStatus::Done);
    assert_eq!(
        std::fs::read(dir.join("dst").join("a.bin")).unwrap().len(),
        10,
        "everything outside the loop is still copied"
    );

    let copied_link = dir.join("dst").join("sub").join("back");
    if recreates_links {
        // The link is put back as a link, so nothing is ever walked
        // through and nothing is lost.
        assert!(skipped.is_empty(), "a recreated link costs nothing");
        assert!(
            std::fs::symlink_metadata(&copied_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link is recreated, not materialised"
        );
        assert!(copy_path(&src, &dir.join("dst2")).is_ok());
    } else {
        assert_eq!(skipped, ["back"], "the looping link is the only casualty");
        // The strict variant has no way to report, so it refuses outright
        // rather than walking forever.
        assert!(copy_path(&src, &dir.join("dst2")).is_err());
    }

    // Every reparse point is removed as a link before the tree goes, so a
    // deletion can never reach through one to the source.
    for link in [
        loop_link,
        copied_link,
        dir.join("dst2").join("sub").join("back"),
    ] {
        std::fs::remove_dir(&link).ok();
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn copy_tree_progress_reports_bytes() {
    let dir = tempdir();
    let src = dir.join("src");
    std::fs::create_dir(&src).unwrap();
    write_file(&src.join("a.bin"), &[7u8; 500]);
    write_file(&src.join("b.bin"), &[7u8; 300]);

    let dst = dir.join("dst");
    let mut total = 0u64;
    let status =
        copy_tree_progress(&src, &dst, &mut |d| total += d, &mut |_, _| {}, &|| false).unwrap();
    assert_eq!(status, OpStatus::Done);
    assert_eq!(total, 800);
    assert_eq!(std::fs::read(dst.join("a.bin")).unwrap().len(), 500);
    std::fs::remove_dir_all(&dir).ok();
}

/// The point of the skip callback: one file another program holds must
/// cost that file alone. Windows only, because only there can the
/// condition be staged — opening a handle with no sharing is exactly what
/// a service doing the same to a user's file produces. Elsewhere an open
/// file stays perfectly copyable, so there is nothing to reproduce.
#[cfg(windows)]
#[test]
fn copy_tree_progress_skips_a_held_file_and_keeps_going() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempdir();
    let src = dir.join("src");
    std::fs::create_dir(&src).unwrap();
    write_file(&src.join("a.bin"), &[7u8; 40]);
    let held = src.join("held.bin");
    write_file(&held, &[7u8; 50]);
    write_file(&src.join("z.bin"), &[7u8; 60]);

    // `share_mode(0)` = no other open is granted, the way a program that
    // keeps a file for itself leaves it.
    let _handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held)
        .unwrap();

    let dst = dir.join("dst");
    let mut skipped: Vec<String> = Vec::new();
    let status = copy_tree_progress(
        &src,
        &dst,
        &mut |_| {},
        &mut |path, _| skipped.push(path.file_name().unwrap().to_string_lossy().into_owned()),
        &|| false,
    )
    .unwrap();

    assert_eq!(status, OpStatus::Done);
    assert_eq!(skipped, ["held.bin"], "only the held file is reported");
    // Read order is not guaranteed, so both siblings are checked: whichever
    // side of the held file they fell on, they went through.
    assert_eq!(std::fs::read(dst.join("a.bin")).unwrap().len(), 40);
    assert_eq!(std::fs::read(dst.join("z.bin")).unwrap().len(), 60);
    assert!(!dst.join("held.bin").exists(), "no stub left behind");

    drop(_handle);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn copy_tree_progress_cancels_and_cleans() {
    let dir = tempdir();
    let src = dir.join("big.bin");
    write_file(&src, &[1u8; 1_000_000]); // > chunk size → cancellation mid-flight
    let dst = dir.join("big-copy.bin");
    // cancel() true right away → nothing is copied.
    let status = copy_tree_progress(&src, &dst, &mut |_| {}, &mut |_, _| {}, &|| true).unwrap();
    assert_eq!(status, OpStatus::Cancelled);
    assert!(!dst.exists(), "the partial file must be cleaned up");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn duplicate_creates_sibling() {
    let dir = tempdir();
    let src = dir.join("file.bin");
    write_file(&src, b"data");
    let copy = duplicate(&src).unwrap();
    assert_eq!(copy.file_name().unwrap(), "file - Copy01.bin");
    assert_eq!(std::fs::read(&copy).unwrap(), b"data");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_in_place_rejects_path_separator() {
    let dir = tempdir();
    let src = dir.join("file.txt");
    write_file(&src, b"x");
    let bad = rename_in_place(&src, "sub/file.txt");
    assert!(bad.is_err());
    let bad2 = rename_in_place(&src, "..");
    assert!(bad2.is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_in_place_ok() {
    let dir = tempdir();
    let src = dir.join("file.txt");
    write_file(&src, b"x");
    let new = rename_in_place(&src, "renamed.txt").unwrap();
    assert!(new.exists());
    assert!(!src.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_replacing_file_keeps_source_contents() {
    let dir = tempdir();
    let source = dir.join("source.txt");
    let target = dir.join("target.txt");
    write_file(&source, b"new");
    write_file(&target, b"old");

    let renamed = rename_in_place_replacing_file(&source, "target.txt").unwrap();
    assert_eq!(renamed, target);
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    assert!(!source.exists());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rename_replacing_file_refuses_directories() {
    let dir = tempdir();
    let source = dir.join("source");
    let target = dir.join("target");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&target).unwrap();
    assert!(rename_in_place_replacing_file(&source, "target").is_err());
    assert!(source.is_dir());
    assert!(target.is_dir());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn create_entry_dir_file_and_guards() {
    let dir = tempdir();
    // Folder.
    let d = create_entry(&dir, "sub", true).unwrap();
    assert!(d.is_dir());
    // File (with extension).
    let f = create_entry(&dir, "notes.txt", false).unwrap();
    assert!(f.is_file());
    // Rejected: name already taken.
    assert!(create_entry(&dir, "sub", true).is_err());
    std::fs::write(&f, b"keep me").unwrap();
    assert!(create_entry(&dir, "notes.txt", false).is_err());
    assert_eq!(std::fs::read(&f).unwrap(), b"keep me");
    // Rejected: invalid names (separator, ..).
    assert!(create_entry(&dir, "a/b", false).is_err());
    assert!(create_entry(&dir, "..", true).is_err());
    assert!(create_entry(&dir, "   ", false).is_err()); // empty after trim
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_name_the_filesystem_refuses_is_told_apart_from_a_name_already_taken() {
    // The distinction this function exists for: these are rejected on their
    // spelling alone, with no folder to compare against, so reporting them
    // as a duplicate would point at an entry that does not exist.
    assert_eq!(check_file_name(""), Err(NameRejection::Empty));
    assert_eq!(check_file_name("."), Err(NameRejection::DotEntry));
    assert_eq!(check_file_name(".."), Err(NameRejection::DotEntry));
    // Both separators everywhere: a name is never a path, see
    // `rename_in_place`.
    assert_eq!(check_file_name("a/b"), Err(NameRejection::ForbiddenChar));
    assert_eq!(check_file_name("a\\b"), Err(NameRejection::ForbiddenChar));
    // Invisible in the field, unusable on disk.
    assert_eq!(check_file_name("a\tb"), Err(NameRejection::ForbiddenChar));

    // Ordinary names stay accepted, including the ones whose punctuation
    // only looks suspicious.
    for ok in [
        "notes.txt",
        "Report 2026",
        ".clang-format",
        "a-b_c(1)[2]{3}",
        "café & co",
        "archive.tar.gz",
    ] {
        assert_eq!(check_file_name(ok), Ok(()), "{ok} should be accepted");
    }

    // Every character the interface lists must actually be refused: the
    // message and the rule are held to the same source of truth.
    for c in FORBIDDEN_NAME_CHARS.chars().filter(|c| !c.is_whitespace()) {
        assert_eq!(
            check_file_name(&format!("a{c}b")),
            Err(NameRejection::ForbiddenChar),
            "{c} is advertised as forbidden but was accepted"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_only_naming_rules_are_enforced() {
    // Win32 reserves these whatever the folder, extension included.
    assert_eq!(check_file_name("NUL"), Err(NameRejection::ReservedDevice));
    assert_eq!(
        check_file_name("com1.txt"),
        Err(NameRejection::ReservedDevice)
    );
    // Windows drops these silently, so the entry would not carry the name
    // that was typed.
    assert_eq!(
        check_file_name("report."),
        Err(NameRejection::TrailingDotOrSpace)
    );
    assert_eq!(
        check_file_name("report "),
        Err(NameRejection::TrailingDotOrSpace)
    );
    // Reserved as a whole name only — a longer name merely starting with
    // one is a perfectly ordinary file.
    assert_eq!(check_file_name("NULL.txt"), Ok(()));
    assert_eq!(check_file_name("console.log"), Ok(()));
}

#[cfg(not(windows))]
#[test]
fn windows_only_rules_do_not_leak_onto_unix() {
    // These are valid names on a Unix filesystem and must stay creatable.
    for ok in [
        "NUL", "com1.txt", "report.", "report ", "a:b", "what?", "a*",
    ] {
        assert_eq!(check_file_name(ok), Ok(()), "{ok} should be accepted");
    }
}

#[test]
fn move_into_uses_unique_name_on_collision() {
    let dir = tempdir();
    let from = dir.join("src");
    std::fs::create_dir(&from).unwrap();
    write_file(&from.join("a.txt"), b"A");

    let into = dir.join("into");
    std::fs::create_dir(&into).unwrap();
    write_file(&into.join("src"), b"existing"); // collision

    let dst = move_into(&from, &into).unwrap();
    // The "existing" content is intact, the move generated "src - Copy01".
    assert_eq!(dst.file_name().unwrap(), "src - Copy01");
    assert!(dst.is_dir());
    assert_eq!(std::fs::read(into.join("src")).unwrap(), b"existing");
    std::fs::remove_dir_all(&dir).ok();
}

// Trash restoration compares the CANONICALIZED original path (as returned
// by the FreeDesktop trash) to the path recorded by Favnyr. `resolve_parent_
// symlinks` must resolve the PARENT's links even if the leaf no longer exists
// (file already in the trash) — e.g. `/home` → `/var/home`, mounted volumes, etc.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn resolve_parent_symlinks_resolves_symlinked_parent_even_if_leaf_gone() {
    use std::os::unix::fs::symlink;
    let dir = tempdir();
    let real = dir.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = dir.join("link");
    symlink(&real, &link).unwrap();

    // Path via the link, NONEXISTENT leaf (simulates an already-deleted file).
    let via_link = link.join("gone.txt");
    let resolved = resolve_parent_symlinks(&via_link);
    let expected = std::fs::canonicalize(&real).unwrap().join("gone.txt");
    assert_eq!(resolved, expected, "the parent (link) must be resolved");
    assert_ne!(
        resolved, via_link,
        "the path must have changed (link resolved)"
    );

    std::fs::remove_dir_all(&dir).ok();
}
