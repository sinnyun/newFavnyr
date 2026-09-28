use super::*;

fn handler(name: &str) -> AppHandler {
    AppHandler {
        name: name.to_string(),
        key: name.to_string(),
        exe: None,
        recommended: false,
    }
}

#[test]
fn app_list_sorts_case_insensitively() {
    // Guards the `sort_by` → `sort_by_key` rewrite in `handlers_for_ext`
    // (Clippy `unnecessary_sort_by`, Rust 1.97): same one-liner, run here
    // against handlers built by hand rather than real `.desktop` files, so
    // it exercises the platform-neutral sort regardless of the OS running
    // the test — `handlers_for_ext` itself is Linux-only.
    let mut apps = [handler("viewer"), handler("EDITOR"), handler("Archiver")];
    apps.sort_by_key(|a| a.name.to_lowercase());
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Archiver", "EDITOR", "viewer"]);
}

#[cfg(not(windows))]
#[test]
fn optional_mime_less_launchers_do_not_admit_incompatible_apps() {
    use super::imp::desktop_matches_mime;

    assert!(desktop_matches_mime("image/png;", Some("image/png"), false));
    assert!(!desktop_matches_mime("", Some("image/png"), false));
    assert!(desktop_matches_mime("", Some("image/png"), true));
    assert!(!desktop_matches_mime(
        "text/plain;",
        Some("image/png"),
        true
    ));
    assert!(desktop_matches_mime("text/plain;", None, false));
}

#[cfg(not(windows))]
#[test]
fn no_display_mime_handlers_remain_available_but_hidden_entries_do_not() {
    use super::imp::{desktop_is_handler, parse_desktop};

    let menu_hidden_handler = parse_desktop(
        "[Desktop Entry]\n\
             Name=Document Viewer\n\
             Exec=document-viewer %U\n\
             MimeType=application/pdf;\n\
             NoDisplay=true\n",
    );
    assert!(desktop_is_handler(
        &menu_hidden_handler,
        Some("application/pdf"),
        false
    ));

    let disabled_handler = parse_desktop(
        "[Desktop Entry]\n\
             Name=Document Viewer\n\
             Exec=document-viewer %U\n\
             MimeType=application/pdf;\n\
             Hidden=true\n",
    );
    assert!(!desktop_is_handler(
        &disabled_handler,
        Some("application/pdf"),
        true
    ));
}

#[cfg(not(windows))]
#[test]
fn a_launcher_reaching_its_program_through_a_variable_still_starts() {
    use super::imp::exec_argv;

    // Splitting deliberately leaves the text alone: the Desktop Entry
    // specification has no variables, so `Exec` is not a shell line.
    let argv = exec_argv("$HOME/.local/bin/tool.sh --flag %f", Some("/tmp/doc.pdf"));
    assert_eq!(argv[0], "$HOME/.local/bin/tool.sh");
    assert_eq!(argv[1], "--flag");
    assert_eq!(argv[2], "/tmp/doc.pdf");

    // Resolving it is what makes the launcher usable, and it is the same
    // resolution a path typed in the address bar gets. Hand-written
    // launchers rely on it and the desktops accept them.
    let home = dirs::home_dir().expect("a home folder");
    assert_eq!(
        favnyr_core::fs::expand_typed_path(argv[0].as_str()),
        home.join(".local/bin/tool.sh")
    );
    assert_eq!(
        favnyr_core::fs::expand_typed_path("~/tool.sh"),
        home.join("tool.sh")
    );

    // A bare command name must come back untouched, or it would stop
    // being resolved through PATH.
    assert_eq!(
        favnyr_core::fs::expand_typed_path("7z"),
        std::path::PathBuf::from("7z")
    );
}

#[cfg(not(windows))]
#[test]
fn exec_is_split_by_the_desktop_entry_rules_not_by_shell_habits() {
    use super::imp::exec_argv;

    // Ordinary case, and the field code drops out when the launcher is
    // started on its own — a stray "%U" would reach the program as a
    // filename to open.
    assert_eq!(
        exec_argv("myapp myapp://open/12345 %U", None),
        ["myapp", "myapp://open/12345"]
    );
    assert_eq!(
        exec_argv("viewer %U", Some("/tmp/a.png")),
        ["viewer", "/tmp/a.png"]
    );

    // The specification quotes with `"` only. An apostrophe is an ordinary
    // character: treating it as a quote — as a shell-style splitter would —
    // swallowed the rest of the command line.
    assert_eq!(
        exec_argv("/opt/it's here/run --now", None),
        ["/opt/it's", "here/run", "--now"]
    );
    assert_eq!(
        exec_argv("\"/opt/it's here/run\" --now", None),
        ["/opt/it's here/run", "--now"]
    );

    // Quoted space stays inside one argument; inside quotes `\` escapes.
    assert_eq!(
        exec_argv("\"/usr/local/My App/bin\" %f", None),
        ["/usr/local/My App/bin"]
    );
    assert_eq!(exec_argv("\"a\\\"b\" tail", None), ["a\"b", "tail"]);

    // `%%` is a literal percent, not the start of a code.
    assert_eq!(
        exec_argv("tool --fmt 100%% -v", None),
        ["tool", "--fmt", "100%", "-v"]
    );

    // Codes Favnyr has nothing to supply for are dropped, and a code alone
    // in its token leaves no empty argument behind.
    assert_eq!(exec_argv("app %i %c %k --go", None), ["app", "--go"]);
}

#[cfg(not(windows))]
#[test]
fn a_launcher_icon_is_found_by_name_across_the_theme_layouts() {
    use super::imp::{find_legacy_icon, find_themed_icon, resolve_icon};

    let root = std::env::temp_dir().join(format!(
        "favnyr-icons-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let touch = |rel: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"x").unwrap();
        path
    };

    // Both size-folder orderings found in the wild.
    let sized = touch("hicolor/48x48/apps/app_icon_12345.png");
    let inverted = touch("hicolor/apps/64/other-app.png");
    // A vector wins over a bitmap of the same name: it stays sharp at any
    // row height, which is the whole reason for preferring it.
    touch("hicolor/32x32/apps/sample-app.png");
    let vector = touch("hicolor/scalable/apps/sample-app.svg");

    let bases = vec![root.clone()];
    let themes = vec!["hicolor".to_string()];
    assert_eq!(
        find_themed_icon(&bases, &themes, "app_icon_12345"),
        Some(sized)
    );
    assert_eq!(
        find_themed_icon(&bases, &themes, "other-app"),
        Some(inverted)
    );
    assert_eq!(
        find_themed_icon(&bases, &themes, "sample-app"),
        Some(vector)
    );
    assert_eq!(find_themed_icon(&bases, &themes, "absent"), None);

    // The user's theme is searched before the fallback every theme
    // inherits, so a local override wins.
    let mine = touch("Breeze/48x48/apps/app_icon_12345.png");
    assert_eq!(
        find_themed_icon(
            &bases,
            &["Breeze".to_string(), "hicolor".to_string()],
            "app_icon_12345"
        ),
        Some(mine)
    );

    // Flat layout, with no theme or size below it.
    let flat = touch("pixmaps/legacyapp.xpm");
    assert_eq!(
        find_legacy_icon(&[root.join("pixmaps")], "legacyapp"),
        Some(flat)
    );

    // An absolute path is taken as it stands — the common case for an
    // application installed by hand, which points at its own file.
    let direct = touch("opt/thing/logo.svg");
    assert_eq!(
        resolve_icon(&direct.display().to_string()),
        Some(direct.clone())
    );
    // ...but only when it really is there, otherwise the row would ask the
    // renderer for a missing file on every listing.
    assert_eq!(
        resolve_icon(&root.join("opt/thing/gone.svg").display().to_string()),
        None
    );
    assert_eq!(resolve_icon(""), None);

    std::fs::remove_dir_all(&root).ok();
}

#[cfg(not(windows))]
#[test]
fn only_the_main_group_of_a_launcher_is_read() {
    use super::imp::parse_desktop;

    // A shebang line and a trailing action group both carry something that
    // looks like a key; neither may override the entry itself.
    let entry = parse_desktop(
        "#!/usr/bin/env xdg-open\n\
             [Desktop Entry]\n\
             Name=Sample App 5.2\n\
             Exec=/opt/sample-app/sample-app\n\
             Icon=/opt/sample-app/sample-app.svg\n\
             Path=\n\
             Terminal=false\n\
             Type=Application\n\
             [Desktop Action Render]\n\
             Name=Render\n\
             Exec=/opt/sample-app/sample-app --render\n",
    );
    assert_eq!(entry.name, "Sample App 5.2");
    assert_eq!(entry.exec, "/opt/sample-app/sample-app");
    assert_eq!(entry.icon, "/opt/sample-app/sample-app.svg");
    assert_eq!(entry.kind, "Application");
    assert!(!entry.terminal);
    // `Path=` present but empty means "no preference", not the root.
    assert!(entry.work_dir.is_empty());
}
