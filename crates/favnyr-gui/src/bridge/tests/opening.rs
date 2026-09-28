use super::*;

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
