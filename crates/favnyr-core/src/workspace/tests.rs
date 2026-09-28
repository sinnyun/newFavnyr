use super::*;

fn sample() -> WorkspaceState {
    WorkspaceState {
        active_panel: 1,
        panels: vec![
            PanelState {
                stretch: 1.5,
                active_tab: 0,
                tabs: vec![TabState {
                    path: "/home/u/Documents".into(),
                    sort_column: SortColumn::Size,
                    sort_order: SortOrder::Desc,
                    preview: false,
                    show_hidden: false,
                    group_mode: GroupMode::FoldersFirst,
                    zoom: None,
                    view_mode: None,
                    subfolders: false,
                    collapsed: Vec::new(),
                }],
                columns: crate::columns::default_columns(),
                tab_bar_mode: 0,
                vbar_width: 0.0,
            },
            PanelState {
                stretch: 1.0,
                active_tab: 1,
                tabs: vec![
                    TabState {
                        path: "/tmp".into(),
                        sort_column: SortColumn::Name,
                        sort_order: SortOrder::Asc,
                        preview: true,
                        show_hidden: false,
                        group_mode: GroupMode::FoldersFirst,
                        zoom: None,
                        view_mode: None,
                        subfolders: false,
                        collapsed: Vec::new(),
                    },
                    TabState {
                        path: "/etc".into(),
                        sort_column: SortColumn::Modified,
                        sort_order: SortOrder::Desc,
                        preview: false,
                        show_hidden: false,
                        group_mode: GroupMode::FoldersFirst,
                        zoom: None,
                        view_mode: None,
                        subfolders: false,
                        collapsed: Vec::new(),
                    },
                ],
                columns: crate::columns::default_columns(),
                tab_bar_mode: 0,
                vbar_width: 0.0,
            },
        ],
        layout: None,
        workspace_name: None,
        closed_tabs: Vec::new(),
        sidebar_sections: SidebarSectionsState::default(),
    }
}

#[test]
fn round_trip_toml() {
    let ws = sample();
    let s = toml::to_string_pretty(&ws).unwrap();
    let back: WorkspaceState = toml::from_str(&s).unwrap();
    assert_eq!(back, ws);
}

#[test]
fn a_workspace_saved_before_the_zoom_existed_still_loads() {
    // Exactly what older files contain: a tab with no `zoom` key at all.
    // It must parse, and the absence has to stay distinguishable from a
    // stored level so the interface can fall back to the default for the
    // mode instead of snapping the view to some arbitrary size.
    let older = r#"
            active_panel = 0
            [[panels]]
            stretch = 1.0
            active_tab = 0
            [[panels.tabs]]
            path = "/tmp"
            sort_column = "name"
            sort_order = "asc"
            preview = true
            show_hidden = false
            group_mode = "foldersfirst"
        "#;
    let ws: WorkspaceState = toml::from_str(older).unwrap();
    assert_eq!(ws.panels[0].tabs[0].zoom, None);
    assert!(ws.panels[0].tabs[0].preview);

    // A file written today keeps the level through a full round trip.
    let mut current = ws.clone();
    current.panels[0].tabs[0].zoom = Some(5);
    let text = toml::to_string_pretty(&current).unwrap();
    let back: WorkspaceState = toml::from_str(&text).unwrap();
    assert_eq!(back.panels[0].tabs[0].zoom, Some(5));
}

/// The display mode, the subfolder sections and the folded sections travel
/// with the tab. A file written before any of them existed still loads: the
/// interface then reads the legacy `preview` flag, which stands for the two
/// modes its author knew.
#[test]
fn view_mode_subfolders_and_folds_travel_with_the_tab() {
    let older = r#"
            active_panel = 0
            [[panels]]
            stretch = 1.0
            active_tab = 0
            [[panels.tabs]]
            path = "/tmp"
            sort_column = "name"
            sort_order = "asc"
            preview = true
            show_hidden = false
            group_mode = "foldersfirst"
        "#;
    let ws: WorkspaceState = toml::from_str(older).unwrap();
    let tab = &ws.panels[0].tabs[0];
    assert_eq!(tab.view_mode, None);
    assert!(!tab.subfolders);
    assert!(tab.collapsed.is_empty());

    let mut current = ws.clone();
    current.panels[0].tabs[0].view_mode = Some("grid".into());
    current.panels[0].tabs[0].subfolders = true;
    current.panels[0].tabs[0].collapsed =
        vec!["cat:image".to_string(), "sub:/tmp/album".to_string()];
    let text = toml::to_string_pretty(&current).unwrap();
    let back: WorkspaceState = toml::from_str(&text).unwrap();
    let tab = &back.panels[0].tabs[0];
    assert_eq!(tab.view_mode.as_deref(), Some("grid"));
    assert!(tab.subfolders);
    assert_eq!(tab.collapsed, ["cat:image", "sub:/tmp/album"]);
}

#[test]
fn closed_tabs_are_backward_compatible_persisted_and_capped() {
    let serialized_without_history = toml::to_string_pretty(&sample()).unwrap();
    assert!(!serialized_without_history.contains("closed_tabs"));
    let restored: WorkspaceState = toml::from_str(&serialized_without_history).unwrap();
    assert!(restored.closed_tabs.is_empty());

    let mut ws = sample();
    ws.closed_tabs = (0..25)
        .map(|index| TabState {
            path: format!("/closed/{index}"),
            sort_column: SortColumn::Name,
            sort_order: SortOrder::Asc,
            preview: false,
            show_hidden: false,
            group_mode: GroupMode::FoldersFirst,
            zoom: None,
            view_mode: None,
            subfolders: false,
            collapsed: Vec::new(),
        })
        .collect();

    let ws = ws.sanitized();
    assert_eq!(ws.closed_tabs.len(), MAX_CLOSED_TABS);
    assert_eq!(ws.closed_tabs.first().unwrap().path, "/closed/5");
    assert_eq!(ws.closed_tabs.last().unwrap().path, "/closed/24");

    let serialized = toml::to_string_pretty(&ws).unwrap();
    let restored: WorkspaceState = toml::from_str(&serialized).unwrap();
    assert_eq!(restored.closed_tabs, ws.closed_tabs);
}

#[test]
fn sidebar_sections_are_backward_compatible_and_persisted() {
    // The historical "all expanded" state stays absent from the TOML and
    // comes back by default: no old workspace needs migration.
    let without_sidebar = toml::to_string_pretty(&sample()).unwrap();
    assert!(!without_sidebar.contains("sidebar_sections"));
    let restored: WorkspaceState = toml::from_str(&without_sidebar).unwrap();
    assert_eq!(restored.sidebar_sections, SidebarSectionsState::default());

    let mut ws = sample();
    ws.sidebar_sections = SidebarSectionsState {
        shortcuts_collapsed: true,
        favorites_collapsed: false,
        drives_collapsed: true,
        network_collapsed: true,
    };
    let serialized = toml::to_string_pretty(&ws).unwrap();
    assert!(serialized.contains("sidebar_sections"));
    assert!(!serialized.contains("\norder ="));
    let restored: WorkspaceState = toml::from_str(&serialized).unwrap();
    assert_eq!(restored.sidebar_sections, ws.sidebar_sections);

    // Compatibility with the short-lived version that persisted the order
    // per workspace: Serde ignores this old field, the collapse states remain intact.
    let legacy = serialized.replacen(
        "[sidebar_sections]\n",
        "[sidebar_sections]\norder = [\"network\", \"drives\", \"shortcuts\", \"favorites\"]\n",
        1,
    );
    let restored_legacy: WorkspaceState = toml::from_str(&legacy).unwrap();
    assert_eq!(restored_legacy.sidebar_sections, ws.sidebar_sections);

    // Same guarantee through the real envelope of a named workspace
    // (`name` + `saved_at` + `[state]`), whose TOML structure is more
    // nested than the session file.
    let dir = temp_dir("sidebar-sections");
    let id = save_named_workspace(&dir, "Sidebar", &ws).unwrap();
    let (_, restored) = load_named_workspace(&dir, &id).unwrap();
    assert_eq!(restored.sidebar_sections, ws.sidebar_sections);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn save_load_file() {
    let dir = std::env::temp_dir().join(format!(
        "favnyr-ws-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("workspace.toml");

    // Missing file → None.
    assert!(WorkspaceState::load(&path).unwrap().is_none());

    sample().save(&path).unwrap();
    let loaded = WorkspaceState::load(&path).unwrap().unwrap();
    // `load` sanitizes, which materializes the migrated `layout`.
    assert_eq!(loaded, sample().sanitized());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn finds_named_workspace_by_trimmed_case_insensitive_label() {
    let dir = std::env::temp_dir().join(format!(
        "favnyr-ws-find-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let id = save_named_workspace(&dir, "Café Project", &sample()).unwrap();

    let found = find_named_workspace(&dir, "  CAFÉ PROJECT  ").unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.name, "Café Project");
    assert!(find_named_workspace(&dir, "workspace.toml").is_none());

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn sanitized_clamps_out_of_bounds() {
    let ws = WorkspaceState {
        active_panel: 99,
        panels: vec![PanelState {
            stretch: -3.0,  // invalid
            active_tab: 42, // out of bounds
            tabs: vec![TabState {
                path: "/x".into(),
                sort_column: SortColumn::Name,
                sort_order: SortOrder::Asc,
                preview: false,
                show_hidden: false,
                group_mode: GroupMode::FoldersFirst,
                zoom: None,
                view_mode: None,
                subfolders: false,
                collapsed: Vec::new(),
            }],
            columns: crate::columns::default_columns(),
            tab_bar_mode: 0,
            vbar_width: 0.0,
        }],
        layout: None,
        workspace_name: None,
        closed_tabs: Vec::new(),
        sidebar_sections: SidebarSectionsState::default(),
    }
    .sanitized();
    assert_eq!(ws.active_panel, 0);
    assert_eq!(ws.panels[0].active_tab, 0);
    assert_eq!(ws.panels[0].stretch, 1.0);
}

#[test]
fn sanitized_drops_empty_panels() {
    let ws = WorkspaceState {
        active_panel: 0,
        panels: vec![
            PanelState {
                stretch: 1.0,
                active_tab: 0,
                tabs: vec![],
                columns: crate::columns::default_columns(),
                tab_bar_mode: 0,
                vbar_width: 0.0,
            },
            PanelState {
                stretch: 1.0,
                active_tab: 0,
                tabs: vec![TabState {
                    path: "/y".into(),
                    sort_column: SortColumn::Name,
                    sort_order: SortOrder::Asc,
                    preview: false,
                    show_hidden: false,
                    group_mode: GroupMode::FoldersFirst,
                    zoom: None,
                    view_mode: None,
                    subfolders: false,
                    collapsed: Vec::new(),
                }],
                columns: crate::columns::default_columns(),
                tab_bar_mode: 0,
                vbar_width: 0.0,
            },
        ],
        layout: None,
        workspace_name: None,
        closed_tabs: Vec::new(),
        sidebar_sections: SidebarSectionsState::default(),
    }
    .sanitized();
    assert_eq!(ws.panels.len(), 1);
    assert_eq!(ws.panels[0].tabs[0].path, "/y");
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "favnyr-named-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn named_workspace_crud() {
    let dir = temp_dir("crud");

    // Empty list at the start.
    assert!(list_named_workspaces(&dir).is_empty());

    // Save → appears in the list.
    let id = save_named_workspace(&dir, "  My workspace  ", &sample()).unwrap();
    let list = list_named_workspaces(&dir);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "My workspace"); // trim applied
    assert_eq!(list[0].id, id);
    // Counters: sample() = 2 panels, 1+2 = 3 tabs.
    assert_eq!(list[0].panels, 2);
    assert_eq!(list[0].tabs, 3);

    // Load → name + state returned (sanitized: migrated layout materialized).
    let (name, state) = load_named_workspace(&dir, &id).unwrap();
    assert_eq!(name, "My workspace");
    assert_eq!(state, sample().sanitized());

    // Rename → name changed, id stable.
    rename_named_workspace(&dir, &id, "Renamed").unwrap();
    let list = list_named_workspaces(&dir);
    assert_eq!(list[0].name, "Renamed");
    assert_eq!(list[0].id, id);

    // Overwrite → state replaced, name kept.
    let other = WorkspaceState {
        active_panel: 0,
        panels: vec![PanelState {
            stretch: 1.0,
            active_tab: 0,
            tabs: vec![TabState {
                path: "/var".into(),
                sort_column: SortColumn::Name,
                sort_order: SortOrder::Asc,
                preview: false,
                show_hidden: false,
                group_mode: GroupMode::FoldersFirst,
                zoom: None,
                view_mode: None,
                subfolders: false,
                collapsed: Vec::new(),
            }],
            columns: crate::columns::default_columns(),
            tab_bar_mode: 0,
            vbar_width: 0.0,
        }],
        layout: None,
        workspace_name: None,
        closed_tabs: Vec::new(),
        sidebar_sections: SidebarSectionsState::default(),
    };
    overwrite_named_workspace(&dir, &id, &other).unwrap();
    let (name, state) = load_named_workspace(&dir, &id).unwrap();
    assert_eq!(name, "Renamed"); // name preserved
    assert_eq!(state.panels.len(), 1);
    assert_eq!(state.panels[0].tabs[0].path, "/var");

    // Delete → empty list.
    delete_named_workspace(&dir, &id).unwrap();
    assert!(list_named_workspaces(&dir).is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

/// Was this refused BECAUSE the id is unusable, rather than merely failing
/// because nothing was there?
///
/// The distinction carries the test. The paths these ids build do not
/// exist, so a plain `is_err()` holds just as well with the guard removed —
/// it would then be reading "file not found" and calling it a rejection.
/// Only the cause tells the two apart: the guard answers `Workspace`, a
/// missing file answers `Io`.
fn refused_as_invalid_id<T>(result: crate::error::Result<T>) -> bool {
    match result {
        Err(crate::error::Error::Workspace(message)) => message.starts_with("invalid workspace id"),
        _ => false,
    }
}

#[test]
fn rejects_unsafe_id() {
    let dir = temp_dir("unsafe");
    // An id becomes a file path, so one that walks out of the folder — or
    // simply carries a separator — must be turned away before it reaches
    // the filesystem.
    assert!(refused_as_invalid_id(load_named_workspace(
        &dir,
        "../outside"
    )));
    assert!(refused_as_invalid_id(delete_named_workspace(&dir, "a/b")));
    assert!(!id_is_safe(""));
    assert!(!id_is_safe("a.b"));
    assert!(id_is_safe("ws-12345"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn list_sorted_newest_first() {
    let dir = temp_dir("sort");
    // Small sleep between each save → strictly increasing `saved_at`
    // (deterministic, no nanosecond collision).
    save_named_workspace(&dir, "Zeta", &sample()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(3));
    save_named_workspace(&dir, "alpha", &sample()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(3));
    let beta_id = save_named_workspace(&dir, "Beta", &sample()).unwrap();
    // By default: the most recently saved comes first.
    let names: Vec<_> = list_named_workspaces(&dir)
        .into_iter()
        .map(|m| m.name)
        .collect();
    assert_eq!(names, ["Beta", "alpha", "Zeta"]);

    // An update (overwrite) moves the workspace to the top.
    std::thread::sleep(std::time::Duration::from_millis(3));
    overwrite_named_workspace(&dir, &beta_id, &sample()).unwrap(); // Beta already at the top
    save_named_workspace(&dir, "Gamma", &sample()).unwrap();
    let top = list_named_workspaces(&dir)
        .into_iter()
        .next()
        .map(|m| m.name)
        .unwrap();
    assert_eq!(top, "Gamma");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_rejects_empty_panels_as_none() {
    let dir = std::env::temp_dir().join(format!(
        "favnyr-ws-empty-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("workspace.toml");
    std::fs::write(&path, "active_panel = 0\npanels = []\n").unwrap();
    assert!(WorkspaceState::load(&path).unwrap().is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn layout_migrated_from_flat_panels() {
    // A workspace without `layout` produces a valid chain covering all
    // the panels.
    let ws = sample(); // layout: None, 2 panels
    let tree = ws.layout_tree();
    assert!(tree.is_valid_for(2));
    assert_eq!(tree.leaf_indices(), vec![0, 1]);

    // After sanitize, the layout is materialized and valid.
    let s = ws.sanitized();
    assert!(s.layout.is_some());
    assert!(s.layout.as_ref().unwrap().is_valid_for(s.panels.len()));
}

#[test]
fn stored_valid_layout_survives_round_trip() {
    let mut ws = sample();
    ws.layout = Some(LayoutNode::Split {
        dir: crate::layout::SplitDir::Column,
        ratio: 0.4,
        first: Box::new(LayoutNode::Leaf { panel: 0 }),
        second: Box::new(LayoutNode::Leaf { panel: 1 }),
    });
    let text = toml::to_string_pretty(&ws).unwrap();
    assert!(text.contains("layout"));
    let back: WorkspaceState = toml::from_str(&text).unwrap();
    assert_eq!(back.layout, ws.layout);
    // The stored tree (Column) is kept by layout_tree (it is valid).
    assert!(matches!(
        back.layout_tree(),
        LayoutNode::Split {
            dir: crate::layout::SplitDir::Column,
            ..
        }
    ));
}

#[test]
fn invalid_stored_layout_falls_back_to_chain() {
    let mut ws = sample(); // 2 panels
    // Layout referencing only panel 0 → invalid for 2 panels.
    ws.layout = Some(LayoutNode::Leaf { panel: 0 });
    let tree = ws.layout_tree();
    assert!(tree.is_valid_for(2)); // rebuilt into a valid chain
    assert_eq!(tree.leaf_indices(), vec![0, 1]);
}
