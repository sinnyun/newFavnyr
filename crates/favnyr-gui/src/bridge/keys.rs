use super::*;

// Configurable shortcuts ----------

/// Display label of a canonical key (arrows handled separately via `kind`).
///
/// The canonical name is the STORAGE form and never changes; only what the
/// user reads does. It matters beyond politeness: a German keyboard prints
/// "Strg" where an English one prints "Ctrl", so a hardcoded label would name
/// a key that is not on the reader's keyboard.
pub(super) fn cap_label(lang: Lang, key: &str) -> String {
    match key {
        // Glyphs printed identically on every keyboard.
        "Backslash" => "\\".to_string(),
        "Comma" => ",".to_string(),
        "PageUp" => i18n::tr(lang, "key_pageup"),
        "PageDown" => i18n::tr(lang, "key_pagedown"),
        other => other.to_string(),
    }
}

/// Display "caps" of a serialized chord (empty if unassigned/unreadable).
pub(super) fn chord_to_caps(lang: Lang, chord: &str) -> Vec<ShortcutCap> {
    let mut caps = Vec::new();
    let Some(c) = Chord::parse(chord) else {
        return caps;
    };
    if c.ctrl {
        caps.push(ShortcutCap {
            label: i18n::tr(lang, "key_ctrl").into(),
            kind: 0,
        });
    }
    if c.alt {
        caps.push(ShortcutCap {
            label: i18n::tr(lang, "key_alt").into(),
            kind: 0,
        });
    }
    if c.shift {
        caps.push(ShortcutCap {
            label: i18n::tr(lang, "key_shift").into(),
            kind: 0,
        });
    }
    let (label, kind) = match c.key.as_str() {
        "ArrowUp" => (String::new(), 1),
        "ArrowDown" => (String::new(), 2),
        "ArrowLeft" => (String::new(), 3),
        "ArrowRight" => (String::new(), 4),
        k => (cap_label(lang, k), 0),
    };
    caps.push(ShortcutCap {
        label: label.into(),
        kind,
    });
    caps
}

/// Builds the shortcut groups (filtered by the current search) +
/// a boolean "at least one override exists" (enables "Reset all").
pub(super) fn build_shortcut_groups(state: &AppState) -> (Vec<ShortcutGroup>, bool) {
    let lang = state.config.borrow().language;
    let overrides = state.config.borrow().shortcut_overrides.clone();
    let has_overrides = !overrides.is_empty();
    let km = state.keymap.borrow();
    let filter = state.shortcut_filter.borrow().to_lowercase();

    let mut groups: Vec<ShortcutGroup> = Vec::new();
    let mut cur_code: Option<&'static str> = None;
    let mut cur_name = String::new();
    let mut cur_rows: Vec<ShortcutRow> = Vec::new();
    for a in shortcuts::ACTIONS {
        let code = a.group.code();
        if cur_code != Some(code) {
            if !cur_rows.is_empty() {
                groups.push(ShortcutGroup {
                    name: cur_name.as_str().into(),
                    rows: ModelRc::new(VecModel::from(std::mem::take(&mut cur_rows))),
                });
            }
            cur_code = Some(code);
            cur_name = i18n::shortcut_group_name(lang, code);
        }
        let name = i18n::shortcut_action_name(lang, a.id);
        let chord = km.chord_of(a.id);
        if !filter.is_empty()
            && !name.to_lowercase().contains(&filter)
            && !chord.to_lowercase().contains(&filter)
        {
            continue;
        }
        cur_rows.push(ShortcutRow {
            action_id: a.id.into(),
            name: name.into(),
            caps: ModelRc::new(VecModel::from(chord_to_caps(lang, chord))),
            assigned: !chord.is_empty(),
            overridden: overrides.contains_key(a.id),
        });
    }
    if !cur_rows.is_empty() {
        groups.push(ShortcutGroup {
            name: cur_name.as_str().into(),
            rows: ModelRc::new(VecModel::from(cur_rows)),
        });
    }
    (groups, has_overrides)
}

/// Pushes the shortcut list + the overrides state to the UI.
pub(super) fn push_shortcuts_ui(window: &MainWindow, state: &AppState) {
    let (groups, has_overrides) = build_shortcut_groups(state);
    window
        .global::<crate::SettingsApi>()
        .set_shortcut_groups(ModelRc::new(VecModel::from(groups)));
    window
        .global::<crate::SettingsApi>()
        .set_shortcut_has_overrides(has_overrides);
    // Context menus reflect the SAME effective map (dynamic).
    push_menu_shortcuts(window, state);
}

/// Renders a serialized chord ("Ctrl+Shift+N") into a SHORT label for a context
/// menu. Empty if the chord is empty (unassigned action → no shortcut
/// displayed). The serialized form is the storage one and stays canonical;
/// what is rendered uses each keyboard's own key names, so the same chord
/// reads "Ctrl+Shift+N" in English and "Strg+Umschalt+N" in German.
pub(super) fn chord_display(lang: Lang, chord: &str) -> String {
    let Some(c) = shortcuts::Chord::parse(chord) else {
        return String::new();
    };
    let mut s = String::new();
    if c.ctrl {
        s.push_str(&i18n::tr(lang, "key_ctrl"));
        s.push('+');
    }
    if c.alt {
        s.push_str(&i18n::tr(lang, "key_alt"));
        s.push('+');
    }
    if c.shift {
        s.push_str(&i18n::tr(lang, "key_shift"));
        s.push('+');
    }
    // Arrows and punctuation are drawn the same on every keyboard; the named
    // keys come from the catalogue.
    s.push_str(&match c.key.as_str() {
        "Delete" => i18n::tr(lang, "key_delete"),
        "Backslash" => "\\".to_string(),
        "Comma" => ",".to_string(),
        "ArrowUp" => "↑".to_string(),
        "ArrowDown" => "↓".to_string(),
        "ArrowLeft" => "←".to_string(),
        "ArrowRight" => "→".to_string(),
        "PageUp" => i18n::tr(lang, "key_pageup"),
        "PageDown" => i18n::tr(lang, "key_pagedown"),
        k => k.to_string(),
    });
    s
}

/// Pushes the EFFECTIVE shortcut labels displayed in the menus and the
/// rail tooltips — recomputed on every map change (rebind,
/// reset, unassignment) via `push_shortcuts_ui`.
pub(super) fn push_menu_shortcuts(window: &MainWindow, state: &AppState) {
    let lang = state.snapshot_config().language;
    let km = state.keymap.borrow();
    let d = |id: &str| SharedString::from(chord_display(lang, km.chord_of(id)));
    window
        .global::<crate::PanelsApi>()
        .set_menu_shortcuts(MenuShortcuts {
            open: d("open"),
            terminal: d("terminal"),
            copy: d("copy"),
            cut: d("cut"),
            paste: d("paste"),
            rename: d("rename"),
            delete: d("delete"),
            properties: d("properties"),
            new_folder: d("new-folder"),
            new_file: d("new-file"),
            split_side: d("split-side"),
            split_stack: d("split-stack"),
            equalize: d("equalize-views"),
            tab_reopen_closed: d("tab-reopen-closed"),
            open_settings: d("open-settings"),
            open_workspaces: d("open-workspaces"),
        });
}

/// Applies an override (or removes it if it equals the default) + rebuilds the map.
pub(super) fn apply_shortcut_override(state: &AppState, action_id: &str, chord: &str) {
    let default = shortcuts::ACTIONS
        .iter()
        .find(|a| a.id == action_id)
        .map(|a| a.default)
        .unwrap_or("");
    let id = action_id.to_string();
    let chord = chord.to_string();
    state.persist_config(|c| {
        if chord == default {
            c.shortcut_overrides.remove(&id);
        } else {
            c.shortcut_overrides.insert(id.clone(), chord.clone());
        }
    });
    state.rebuild_keymap();
}

/// Updates the active panel's footer ("N items · M selected") without
/// rebuilding the whole UI. The logical model and its rendered sub-model
/// remain the same `Rc`s, so no geometry or scroll position
/// is lost. `selected` is the counter already computed by the operation.
pub(super) fn push_active_footer(window: &MainWindow, state: &AppState, selected: i32) {
    let lang = state.config.borrow().language;
    let idx = *state.active_panel.borrow();
    let (total, hidden) = {
        let panels = state.panels.borrow();
        match panels.get(idx) {
            Some(p) => {
                let show_hidden = p.tabs.tabs[p.tabs.active].show_hidden;
                (
                    p.rows_model.row_count(),
                    if show_hidden { 0 } else { p.hidden_count },
                )
            }
            None => return,
        }
    };
    let footer = i18n::footer_text(lang, total, selected.max(0) as usize, hidden);
    // We write ONLY into the parallel view-footers model: NEVER
    // touch the `panels` model here. Rewriting a `PanelView` (even just for the
    // label) would invalidate the Repeater's `rendered-rows` model property and,
    // on every rubber-band step, selection `row_changed`s would be
    // deferred until a re-list (scroll) — resulting in "skipped" entries.
    let footers = window.global::<crate::PanelsApi>().get_panel_footers();
    if let Some(cur) = footers.row_data(idx)
        && cur.as_str() != footer.as_str()
    {
        footers.set_row_data(idx, footer.into());
    }
}
