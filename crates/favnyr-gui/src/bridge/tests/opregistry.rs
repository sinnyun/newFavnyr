use super::*;

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
