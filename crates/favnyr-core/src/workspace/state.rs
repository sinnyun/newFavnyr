use super::*;

/// Maximum number of closed tabs kept per workspace, oldest to most
/// recent. The last element is the one "Reopen" will restore.
pub const MAX_CLOSED_TABS: usize = 20;

/// Serializable state of a tab: current path + sort + display mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabState {
    pub path: String,
    #[serde(default = "default_sort_column")]
    pub sort_column: SortColumn,
    #[serde(default = "default_sort_order")]
    pub sort_order: SortOrder,
    /// Preview mode (thumbnails) active for this tab. When the field is
    /// absent, `#[serde(default)]` selects list mode.
    #[serde(default)]
    pub preview: bool,
    /// Display of hidden files for this tab. When the field is
    /// absent, `#[serde(default)]` hides hidden files.
    #[serde(default)]
    pub show_hidden: bool,
    /// Grouping by type for this tab. When the field is absent, the
    /// default value is "folders first".
    #[serde(default = "default_group_mode")]
    pub group_mode: GroupMode,
    /// Thumbnail/row zoom level for this tab (Ctrl+wheel).
    ///
    /// `None` when the field is absent, which is what every workspace saved
    /// before this was persisted looks like. The caller then derives a level
    /// from `preview`, so an older file reopens exactly as it used to instead
    /// of snapping to a zoom it never chose. The valid range belongs to the
    /// interface, which clamps on restore — the core only carries the value.
    #[serde(default)]
    pub zoom: Option<i32>,
    /// Display mode of this tab: `"list"`, `"previews"` or `"grid"`.
    ///
    /// `None` for a workspace written before the grid existed: the interface
    /// then reads the legacy `preview` flag, which stands for the two modes it
    /// knew. Kept as a string so the core never has to know the mode set.
    #[serde(default)]
    pub view_mode: Option<String>,
    /// "Show subfolder contents": the listing carries one section per direct
    /// subfolder, holding its own entries (one level down).
    #[serde(default)]
    pub subfolders: bool,
    /// Sections folded away by the user, by key ("cat:image", "sub:C:\dir").
    /// Folding only changes the rows on screen, never the listing.
    #[serde(default)]
    pub collapsed: Vec<String>,
}

fn default_group_mode() -> GroupMode {
    GroupMode::FoldersFirst
}

fn default_sort_column() -> SortColumn {
    SortColumn::Name
}
fn default_sort_order() -> SortOrder {
    SortOrder::Asc
}

/// Serializable state of a panel: width ratio, active tab, tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelState {
    /// Width ratio relative to the other panels (see `panel_stretches`).
    #[serde(default = "default_stretch")]
    pub stretch: f32,
    #[serde(default)]
    pub active_tab: usize,
    pub tabs: Vec<TabState>,
    /// Columns of the view (order + visibility + width). When the field is
    /// absent, the full set is restored.
    #[serde(default = "crate::columns::default_columns")]
    pub columns: Vec<crate::columns::ColumnSpec>,
    /// Position of the view's tab bar: 0 = horizontal at the top,
    /// 1 = vertical on the left, 2 = vertical on the right. The default value is 0.
    #[serde(default)]
    pub tab_bar_mode: u8,
    /// USER width of the vertical tab bar (logical px), set by the
    /// resize handle. `0` = automatic (clamped 40% formula). Persisted per
    /// view.
    #[serde(default)]
    pub vbar_width: f32,
}

fn default_stretch() -> f32 {
    1.0
}

/// State of the four collapsible sections of the left sidebar. All
/// fields default to `false` so that an old workspace without this table
/// keeps its sections expanded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarSectionsState {
    #[serde(default)]
    pub shortcuts_collapsed: bool,
    #[serde(default)]
    pub favorites_collapsed: bool,
    #[serde(default)]
    pub drives_collapsed: bool,
    #[serde(default)]
    pub network_collapsed: bool,
}

impl SidebarSectionsState {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Complete state of the workspace.
///
/// `panels` is the **flat** list of panel contents (each with its
/// tabs); `layout` describes their **arrangement** as a tree of splits
/// (leaves reference an index in `panels`). If `layout` is absent or
/// invalid, it is rebuilt as a horizontal chain covering all
/// panels (see [`Self::layout_tree`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    #[serde(default)]
    pub active_panel: usize,
    pub panels: Vec<PanelState>,
    /// Layout tree. `None` triggers its reconstruction on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutNode>,
    /// Name of the named workspace this state comes from. Shown in the
    /// window title ("{workspace} — Favnyr"). `None` = ad hoc state,
    /// never attached to a saved workspace → title "Favnyr".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// History of tabs actually closed by the user. When the field is
    /// absent from the TOML, the list is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed_tabs: Vec<TabState>,
    /// Collapse state of the sidebar sections. The table is omitted when
    /// everything is expanded; `default` keeps old files perfectly readable.
    #[serde(default, skip_serializing_if = "SidebarSectionsState::is_default")]
    pub sidebar_sections: SidebarSectionsState,
}

impl WorkspaceState {
    /// Loads from `path`. Returns `Ok(None)` if the file does not exist
    /// (first launch) — the caller will then use a default workspace.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let ws: WorkspaceState = toml::from_str(&content).map_err(|e| {
                    // A default workspace takes over and will be saved in its
                    // place: the unreadable one is kept aside so the panels and
                    // tabs it described are not lost with it.
                    crate::paths::preserve_unreadable(path);
                    crate::error::Error::Workspace(format!("parse {}: {e}", path.display()))
                })?;
                // Guard: a workspace without a panel is invalid.
                if ws.panels.is_empty() {
                    return Ok(None);
                }
                Ok(Some(ws.sanitized()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Writes the workspace to disk (creates parent folders as needed).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| {
            crate::error::Error::Workspace(format!("serialize {}: {e}", path.display()))
        })?;
        crate::paths::write_atomic(path, &text)?;
        Ok(())
    }

    /// Returns the validated layout tree: the stored tree if it covers
    /// exactly `0..panels.len()`, otherwise a horizontal chain rebuilt
    /// from the available `stretch` values.
    pub fn layout_tree(&self) -> LayoutNode {
        if let Some(node) = &self.layout
            && node.is_valid_for(self.panels.len())
        {
            return node.clone();
        }
        let stretches: Vec<f32> = self.panels.iter().map(|p| p.stretch).collect();
        LayoutNode::row_chain(&stretches)
    }

    /// Normalizes out-of-bounds indices (robustness against a hand-edited
    /// or corrupted file): clamps `active_panel` and each `active_tab`,
    /// removes empty panels, and guarantees a `layout` consistent with the
    /// remaining panels.
    pub fn sanitized(mut self) -> Self {
        if self.closed_tabs.len() > MAX_CLOSED_TABS {
            self.closed_tabs
                .drain(..self.closed_tabs.len() - MAX_CLOSED_TABS);
        }
        self.panels.retain(|p| !p.tabs.is_empty());
        if self.panels.is_empty() {
            // Rebuild a minimal panel — the caller will replace the empty
            // path with $HOME via `effective_*`.
            self.panels.push(PanelState {
                stretch: 1.0,
                active_tab: 0,
                tabs: vec![TabState {
                    path: String::new(),
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
            });
        }
        // Unknown tab bar mode / invalid width (hand-edited file) → defaults.
        for p in &mut self.panels {
            if p.tab_bar_mode > 2 {
                p.tab_bar_mode = 0;
            }
            if !p.vbar_width.is_finite() || p.vbar_width < 0.0 {
                p.vbar_width = 0.0;
            }
        }
        for p in &mut self.panels {
            if p.active_tab >= p.tabs.len() {
                p.active_tab = p.tabs.len() - 1;
            }
            if !(p.stretch.is_finite() && p.stretch > 0.0) {
                p.stretch = 1.0;
            }
            // Normalize the columns (anchor `name`, widths, completion).
            p.columns = crate::columns::sanitize(std::mem::take(&mut p.columns));
        }
        if self.active_panel >= self.panels.len() {
            self.active_panel = self.panels.len() - 1;
        }
        // Guarantees a valid tree for the remaining panels (the removals
        // above may have reindexed the panels).
        let tree = self.layout_tree();
        self.layout = Some(tree);
        self
    }
}
