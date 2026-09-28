use super::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::sync::Mutex;

static FILE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn incoming_format_classification_preserves_each_route() {
    assert_eq!(
        classify_formats(true, false, true, false),
        IncomingDataKind::ShellPaths
    );
    assert_eq!(
        classify_formats(true, false, false, false),
        IncomingDataKind::ApplicationPaths
    );
    assert_eq!(
        classify_formats(true, false, false, true),
        IncomingDataKind::ApplicationPaths
    );
    assert_eq!(
        classify_formats(true, true, true, false),
        IncomingDataKind::ApplicationPaths
    );
    assert_eq!(
        classify_formats(false, false, false, true),
        IncomingDataKind::VirtualFiles
    );
    assert_eq!(
        classify_formats(false, false, true, false),
        IncomingDataKind::None
    );
}

#[test]
fn copy_effect_never_exceeds_the_source_mask() {
    use windows::Win32::System::Ole::{DROPEFFECT_LINK, DROPEFFECT_MOVE};

    assert_eq!(
        FavnyrDropTarget::choose_copy_effect(DROPEFFECT_COPY | DROPEFFECT_MOVE, true),
        DROPEFFECT_COPY
    );
    assert_eq!(
        FavnyrDropTarget::choose_copy_effect(DROPEFFECT_MOVE | DROPEFFECT_LINK, true),
        DROPEFFECT_NONE
    );
    assert_eq!(
        FavnyrDropTarget::choose_copy_effect(DROPEFFECT_COPY, false),
        DROPEFFECT_NONE
    );
}

#[test]
fn application_capture_survives_source_removal_and_preserves_trees() {
    let _lock = FILE_TEST_LOCK.lock().expect("file test lock poisoned");
    let source_root = create_drop_dir("favnyr-path-dnd").expect("create source root");
    let first_root = source_root.join("first");
    let second_root = source_root.join("second");
    std::fs::create_dir_all(first_root.join("album")).expect("create first tree");
    std::fs::create_dir_all(&second_root).expect("create second tree");
    std::fs::write(first_root.join("album").join("same.txt"), b"first")
        .expect("write first source");
    std::fs::write(second_root.join("same.txt"), b"second").expect("write second source");

    let captured =
        capture_application_paths(&[first_root.join("album"), second_root.join("same.txt")])
            .expect("capture application paths");
    std::fs::remove_dir_all(&source_root).expect("remove source tree");

    assert_eq!(captured.paths.len(), 2);
    assert_eq!(
        std::fs::read(captured.paths[0].join("same.txt")).expect("read captured tree"),
        b"first"
    );
    assert_eq!(
        std::fs::read(&captured.paths[1]).expect("read captured file"),
        b"second"
    );
    std::fs::remove_dir_all(&captured.temp_dir).expect("remove captured tree");
}

#[test]
fn failed_application_capture_rolls_back_its_staging_tree() {
    let _lock = FILE_TEST_LOCK.lock().expect("file test lock poisoned");
    let source_root = create_drop_dir("favnyr-path-dnd").expect("create source root");
    let valid = source_root.join("valid.txt");
    std::fs::write(&valid, b"valid").expect("write valid source");
    let before = application_drop_directories();

    assert!(capture_application_paths(&[valid, source_root.join("missing.txt")]).is_none());
    assert_eq!(application_drop_directories(), before);
    std::fs::remove_dir_all(source_root).expect("remove source root");
}

fn application_drop_directories() -> HashSet<OsString> {
    let prefix = format!("favnyr-path-dnd-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .expect("read temp directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&prefix)
                .then(|| entry.file_name())
        })
        .collect()
}
