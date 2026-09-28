use super::*;

/// Adding an open tab constitutes a workspace modification.
#[test]
fn differ_detects_added_tab() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let b = ws(vec![panel(vec![tab(r"C:\a"), tab(r"C:\b")])]);
    assert!(workspaces_differ(&a, &b));
}

/// A per-tab display setting (hidden files, previews, sort) counts.
#[test]
fn differ_detects_per_tab_settings() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let mut b = ws(vec![panel(vec![tab(r"C:\a")])]);
    b.panels[0].tabs[0].show_hidden = true;
    assert!(workspaces_differ(&a, &b));

    let mut c = ws(vec![panel(vec![tab(r"C:\a")])]);
    c.panels[0].tabs[0].preview = true;
    assert!(workspaces_differ(&a, &c));

    // The thumbnail zoom is saved with the view, so changing it has to
    // raise the asterisk like any other per-tab setting — the title and
    // the Update button both read this one comparison.
    let mut d = ws(vec![panel(vec![tab(r"C:\a")])]);
    d.panels[0].tabs[0].zoom = Some(4);
    assert!(workspaces_differ(&a, &d));

    // Two levels that differ are two different states, even inside
    // preview mode where `preview` alone cannot tell them apart.
    let mut e = ws(vec![panel(vec![tab(r"C:\a")])]);
    e.panels[0].tabs[0].zoom = Some(2);
    let mut f = ws(vec![panel(vec![tab(r"C:\a")])]);
    f.panels[0].tabs[0].zoom = Some(5);
    assert!(workspaces_differ(&e, &f));
}

/// Tab bar position (per-view): counted.
#[test]
fn differ_detects_tab_bar_mode() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let mut b = ws(vec![panel(vec![tab(r"C:\a")])]);
    b.panels[0].tab_bar_mode = 1;
    assert!(workspaces_differ(&a, &b));
}

/// A left-bar section's collapse state goes through the same signature as
/// the tabs: the asterisk and the Update button can never diverge.
#[test]
fn differ_detects_sidebar_section_state() {
    let a = ws(vec![panel(vec![tab(r"C:\a")])]);
    let mut b = a.clone();
    b.sidebar_sections.favorites_collapsed = true;
    assert!(workspaces_differ(&a, &b));

    b.sidebar_sections.favorites_collapsed = false;
    b.sidebar_sections.network_collapsed = true;
    assert!(workspaces_differ(&a, &b));
}

#[test]
fn sidebar_section_state_survives_app_state_capture_and_reset() {
    let mut saved = ws(vec![panel(vec![tab(r"C:\a")])]);
    saved.sidebar_sections = SidebarSectionsState {
        shortcuts_collapsed: true,
        favorites_collapsed: true,
        drives_collapsed: false,
        network_collapsed: true,
    };
    let config = Config {
        sidebar_section_order: [
            SidebarSection::Favorites,
            SidebarSection::Network,
            SidebarSection::Drives,
            SidebarSection::Shortcuts,
        ],
        ..Config::default()
    };
    let state = AppState::from_workspace(config.clone(), saved.clone());
    assert_eq!(
        state.capture_workspace().sidebar_sections,
        saved.sidebar_sections
    );

    state.reset_to_blank();
    assert_eq!(
        state.capture_workspace().sidebar_sections,
        SidebarSectionsState::default()
    );
    // Reset and loading a workspace never touch the
    // global section order preference.
    assert_eq!(
        state.snapshot_config().sidebar_section_order,
        config.sidebar_section_order
    );
}

/// Volatile fields (split ratios, column widths, active tab or panel,
/// and `workspace_name`) don't affect the workspace's persistent state.
#[test]
fn differ_ignores_volatile_fields() {
    let base = ws(vec![panel(vec![tab(r"C:\a")]), panel(vec![tab(r"C:\b")])]);
    let mut other = base.clone();
    // Split ratios.
    other.panels[0].stretch = 2.5;
    // Active tab / panel (simple focus).
    other.active_panel = 1;
    other.panels[1].active_tab = 0;
    // Vertical bar width + attached name.
    other.panels[0].vbar_width = 220.0;
    other.workspace_name = Some("My Workspace".into());
    // Columns (resized widths).
    if let Some(c) = other.panels[0].columns.first_mut() {
        c.width += 40.0;
    }
    assert!(!workspaces_differ(&base, &other));
}

/// Idempotence: a workspace never differs from itself.
#[test]
fn differ_reflexive() {
    let a = ws(vec![panel(vec![tab(r"C:\a"), tab(r"C:\b")])]);
    assert!(!workspaces_differ(&a, &a.clone()));
}
