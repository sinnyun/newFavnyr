use super::*;

// Named workspaces ----------
//
// Each named workspace is a `<id>.toml` file in `paths::workspaces_dir()`.
// `id` is a stable generated identifier (timestamp), distinct from the
// displayed `name` (freely editable by the user). The file contains the
// `name` + the complete state (`WorkspaceState`).

use std::path::PathBuf;

/// Serialized content of a named workspace.
///
/// `saved_at` = nanoseconds since the UNIX epoch at the last save
/// (creation **or** update), used to sort the list by recency. When the
/// field is absent, `#[serde(default)]` produces 0 and the sort then falls
/// back to the file's modification date.
/// TOML note: scalar fields (`name`, `saved_at`) MUST precede the
/// `state` table, otherwise they would get swallowed into `[state]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct NamedWorkspaceFile {
    name: String,
    #[serde(default)]
    saved_at: u64,
    state: WorkspaceState,
}

/// Lightweight metadata of a named workspace (for list display).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedWorkspaceMeta {
    /// Stable identifier = file name without extension.
    pub id: String,
    /// Displayed name, editable by the user.
    pub name: String,
    /// Number of panels (for the metadata shown in the list).
    pub panels: usize,
    /// Total number of tabs (sum over all panels).
    pub tabs: usize,
    /// Date of the last save (epoch nanos) — sort key for recency.
    pub saved_at: u64,
}

/// Validates that an `id` is safe as a file name (anti path-traversal).
pub(super) fn id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn workspace_file_path(dir: &Path, id: &str) -> Option<PathBuf> {
    if id_is_safe(id) {
        Some(dir.join(format!("{id}.toml")))
    } else {
        None
    }
}

/// Lists the named workspaces present in `dir`, sorted by **recency**
/// (last saved first). Ties are broken by name (case-insensitive).
/// Unreadable/corrupted files are ignored.
/// The caller (GUI) can reverse the order to offer "oldest first".
pub fn list_named_workspaces(dir: &Path) -> Vec<NamedWorkspaceMeta> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(), // missing folder = no workspace
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !id_is_safe(id) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(file) = toml::from_str::<NamedWorkspaceFile>(&content)
        {
            let panels = file.state.panels.len();
            let tabs = file.state.panels.iter().map(|p| p.tabs.len()).sum();
            // Recency: `saved_at`, falling back to the file's
            // modification date when the field is 0.
            let saved_at = if file.saved_at != 0 {
                file.saved_at
            } else {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0)
            };
            out.push(NamedWorkspaceMeta {
                id: id.to_string(),
                name: file.name,
                panels,
                tabs,
                saved_at,
            });
        }
    }
    // Most recent first; ties → name (case-insensitive).
    out.sort_by(|a, b| {
        b.saved_at
            .cmp(&a.saved_at)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Finds a named workspace by its user-facing label. The comparison is
/// the same as in the UI: surrounding whitespace ignored and comparison
/// in Unicode lowercase. The session file `workspace.toml`, located
/// elsewhere, never enters this search.
pub fn find_named_workspace(dir: &Path, name: &str) -> Option<NamedWorkspaceMeta> {
    let wanted = name.trim().to_lowercase();
    if wanted.is_empty() {
        return None;
    }
    list_named_workspaces(dir)
        .into_iter()
        .find(|meta| meta.name.trim().to_lowercase() == wanted)
}

/// Nanoseconds (u64) since the UNIX epoch. Precision sufficient to
/// distinguish two close saves and sort by recency.
fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Generates a stable identifier based on the clock (nanos since the epoch).
fn generate_id() -> String {
    format!("ws-{}", now_nanos())
}

/// Creates a new named workspace from `state`. Returns its `id`.
pub fn save_named_workspace(dir: &Path, name: &str, state: &WorkspaceState) -> Result<String> {
    std::fs::create_dir_all(dir)?;
    let id = generate_id();
    let file = NamedWorkspaceFile {
        name: name.trim().to_string(),
        saved_at: now_nanos(),
        state: state.clone(),
    };
    write_named(dir, &id, &file)?;
    Ok(id)
}

/// Overwrites the state of an existing named workspace (keeps its `name`).
pub fn overwrite_named_workspace(dir: &Path, id: &str, state: &WorkspaceState) -> Result<()> {
    let mut file = read_named(dir, id)?;
    file.state = state.clone();
    file.saved_at = now_nanos(); // update → moves to the top of the list
    write_named(dir, id, &file)
}

/// Loads a named workspace. Returns `(name, sanitized state)`.
pub fn load_named_workspace(dir: &Path, id: &str) -> Result<(String, WorkspaceState)> {
    let file = read_named(dir, id)?;
    Ok((file.name, file.state.sanitized()))
}

/// Renames a workspace (only changes the `name` field, keeps the `id`/file).
pub fn rename_named_workspace(dir: &Path, id: &str, new_name: &str) -> Result<()> {
    let mut file = read_named(dir, id)?;
    file.name = new_name.trim().to_string();
    write_named(dir, id, &file)
}

/// Deletes a named workspace's file.
pub fn delete_named_workspace(dir: &Path, id: &str) -> Result<()> {
    let path = workspace_file_path(dir, id)
        .ok_or_else(|| crate::error::Error::Workspace(format!("invalid workspace id: {id:?}")))?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn read_named(dir: &Path, id: &str) -> Result<NamedWorkspaceFile> {
    let path = workspace_file_path(dir, id)
        .ok_or_else(|| crate::error::Error::Workspace(format!("invalid workspace id: {id:?}")))?;
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content)
        .map_err(|e| crate::error::Error::Workspace(format!("parse {}: {e}", path.display())))
}

fn write_named(dir: &Path, id: &str, file: &NamedWorkspaceFile) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = workspace_file_path(dir, id)
        .ok_or_else(|| crate::error::Error::Workspace(format!("invalid workspace id: {id:?}")))?;
    let text = toml::to_string_pretty(file)
        .map_err(|e| crate::error::Error::Workspace(format!("serialize: {e}")))?;
    crate::paths::write_atomic(&path, &text)?;
    Ok(())
}
