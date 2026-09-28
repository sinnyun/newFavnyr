use super::*;

/// Flat store of openers (the Vec's order = base display order).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenerStore {
    #[serde(default)]
    pub openers: Vec<Opener>,
}

impl OpenerStore {
    /// Loads from `path`; empty store if missing/unreadable (never fatal).
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(store) => store,
                Err(err) => {
                    // The file is there but cannot be read back. Starting from
                    // an empty store keeps the application usable; letting the
                    // next save write that emptiness over the user's only copy
                    // does not, so the file is kept aside first.
                    tracing::warn!(
                        error = %err,
                        path = %path.display(),
                        "unreadable store"
                    );
                    crate::paths::preserve_unreadable(path);
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Writes the store as TOML (creates the parent folder).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let s = toml::to_string_pretty(self)
            .map_err(|e| Error::Openers(format!("serialization: {e}")))?;
        crate::paths::write_atomic(path, &s)?;
        Ok(())
    }

    /// Adds an opener (normalized label/program/args). Returns its `id`.
    pub fn add(&mut self, label: &str, program: &str, args: Vec<String>) -> String {
        let id = generate_id();
        self.openers.push(Opener {
            id: id.clone(),
            label: label.trim().to_string(),
            program: program.trim().to_string(),
            assoc: None,
            icon: OpenerIcon::None,
            args,
            default_exts: Vec::new(),
            used_exts: Vec::new(),
            use_count: 0,
            last_used: 0,
            elevated: false,
            ctx_menu: 0,
            ctx_exts: Vec::new(),
        });
        id
    }

    /// Adds an opener based on an **OS association handler** (app without
    /// a classic exe: Windows UWP/Store, Linux `.desktop`). Launched via the OS.
    /// Returns its `id`.
    pub fn add_assoc(&mut self, label: &str, assoc: &str) -> String {
        let id = generate_id();
        self.openers.push(Opener {
            id: id.clone(),
            label: label.trim().to_string(),
            program: String::new(),
            assoc: Some(assoc.trim().to_string()),
            icon: OpenerIcon::None,
            args: Vec::new(),
            default_exts: Vec::new(),
            used_exts: Vec::new(),
            use_count: 0,
            last_used: 0,
            elevated: false, // an OS association app is never launched elevated
            ctx_menu: 0,
            ctx_exts: Vec::new(),
        });
        id
    }

    pub fn get(&self, id: &str) -> Option<&Opener> {
        self.openers.iter().find(|o| o.id == id)
    }

    /// Updates an opener's label/program/args.
    pub fn update(&mut self, id: &str, label: &str, program: &str, args: Vec<String>) -> bool {
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            o.label = label.trim().to_string();
            o.program = program.trim().to_string();
            o.args = args;
            true
        } else {
            false
        }
    }

    /// (Un)marks opener `id` as "launch as administrator" (Windows).
    /// A separate setting, like `set_used_exts` / `set_default_ext` (the UI
    /// applies it after `add`/`update`). On other OSes the flag is simply
    /// ignored at launch.
    pub fn set_elevated(&mut self, id: &str, elevated: bool) -> bool {
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            o.elevated = elevated;
            true
        } else {
            false
        }
    }

    /// Attaches the visual family selected by a ready-made recipe.
    pub fn set_icon(&mut self, id: &str, icon: OpenerIcon) -> bool {
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            o.icon = icon;
            true
        } else {
            false
        }
    }

    /// Sets the context-menu pinning mask (bits [`CTX_FILE`] /
    /// [`CTX_DIR`] / [`CTX_BACKGROUND`]). Same style as `set_elevated`.
    pub fn set_ctx_menu(&mut self, id: &str, mask: u8) -> bool {
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            o.ctx_menu = mask;
            true
        } else {
            false
        }
    }

    /// Restricts the FILE context-menu entry to `exts` (no dot, lowercase).
    /// Empty, or a list holding [`CTX_EXT_ALL`], means every file.
    pub fn set_ctx_exts(&mut self, id: &str, exts: &[String]) -> bool {
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            o.ctx_exts = exts.to_vec();
            true
        } else {
            false
        }
    }

    /// Openers pinned for a given context (`ctx_menu` bit), in the
    /// store's display order.
    pub fn for_context(&self, bit: u8) -> Vec<&Opener> {
        self.openers
            .iter()
            .filter(|o| o.ctx_menu & bit != 0)
            .collect()
    }

    /// Duplicates opener `id`: the clone is inserted RIGHT AFTER the original,
    /// with a new `id`, a usage counter reset to zero, and EMPTY `default_exts`
    /// (only one "Favnyr default" opener per extension — the clone doesn't
    /// steal the status). `used_exts` and `elevated` are kept. Returns the new
    /// `id`. Used by the "Duplicate" button (e.g. same command as admin /
    /// non-admin).
    pub fn duplicate(&mut self, id: &str) -> Option<String> {
        let pos = self.openers.iter().position(|o| o.id == id)?;
        let src = &self.openers[pos];
        let clone = Opener {
            id: generate_id(),
            label: src.label.clone(),
            program: src.program.clone(),
            assoc: src.assoc.clone(),
            icon: src.icon,
            args: src.args.clone(),
            default_exts: Vec::new(),
            used_exts: src.used_exts.clone(),
            use_count: 0,
            last_used: 0,
            elevated: src.elevated,
            ctx_menu: src.ctx_menu, // context-menu pinning follows the copy
            ctx_exts: src.ctx_exts.clone(), // …and so does its extension filter
        };
        let new_id = clone.id.clone();
        self.openers.insert(pos + 1, clone);
        Some(new_id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let n = self.openers.len();
        self.openers.retain(|o| o.id != id);
        self.openers.len() != n
    }

    /// Moves opener `id` right BEFORE `before` (or to the end if `None`).
    pub fn move_before(&mut self, id: &str, before: Option<&str>) -> bool {
        let Some(pos) = self.openers.iter().position(|o| o.id == id) else {
            return false;
        };
        let o = self.openers.remove(pos);
        let at = match before {
            Some(b) => self
                .openers
                .iter()
                .position(|x| x.id == b)
                .unwrap_or(self.openers.len()),
            None => self.openers.len(),
        };
        self.openers.insert(at, o);
        true
    }

    /// Increments the usage counter (called after a successful launch) and
    /// timestamps it. `last_used` is forced STRICTLY greater than all others
    /// → the most recently used exe ALWAYS moves to the top of suggestions,
    /// even if several launches fall within the same second (`now_secs`
    /// resolution). `ext` (if provided) is **learned**: the opener will from
    /// then on be offered for this extension in the "Open with" flyout.
    pub fn record_use(&mut self, id: &str, ext: Option<&str>) {
        let top = self.openers.iter().map(|o| o.last_used).max().unwrap_or(0);
        let stamp = now_secs().max(top.saturating_add(1));
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            o.use_count = o.use_count.saturating_add(1);
            o.last_used = stamp;
            if let Some(ext) = ext {
                let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
                if !ext.is_empty() && !o.used_exts.contains(&ext) {
                    o.used_exts.push(ext);
                }
            }
        }
    }

    /// Sets (or unsets) THIS opener as the Favnyr default for `ext`. Only one
    /// opener per extension: `ext` is removed from all the others.
    pub fn set_default_ext(&mut self, id: &str, ext: &str, enabled: bool) {
        let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        if ext.is_empty() {
            return;
        }
        for o in self.openers.iter_mut() {
            o.default_exts
                .retain(|e| e != &ext || (o.id == id && enabled));
            if o.id == id && enabled && !o.default_exts.contains(&ext) {
                o.default_exts.push(ext.clone());
            }
        }
    }

    /// Replaces opener `id`'s list of **learned** extensions (`used_exts`)
    /// with `exts` (normalized: no dot, lowercase, deduplicated). MANUAL
    /// edit from Settings: the opener will from then on be offered in the
    /// "Open with" flyout for these extensions (see `suggested_for_ext`).
    /// Unlike `default_exts`, there is NO cross-opener uniqueness
    /// constraint — several programs can be suggested for the same extension.
    pub fn set_used_exts(&mut self, id: &str, exts: &[String]) {
        if let Some(o) = self.openers.iter_mut().find(|o| o.id == id) {
            let mut norm: Vec<String> = Vec::new();
            for e in exts {
                let e = e.trim().trim_start_matches('.').to_ascii_lowercase();
                if !e.is_empty() && !norm.contains(&e) {
                    norm.push(e);
                }
            }
            o.used_exts = norm;
        }
    }

    /// Opener set as the Favnyr default for `ext` (the first one found), if any.
    pub fn default_for(&self, ext: &str) -> Option<&Opener> {
        let ext = ext.to_ascii_lowercase();
        self.openers
            .iter()
            .find(|o| o.default_exts.iter().any(|e| e == &ext))
    }

    /// "Suggested applications" list (the "Open with" flyout) sorted by
    /// **recency of use** (MRU): the most recently used exe moves to the TOP.
    /// Openers never used (`last_used == 0`) stay at the bottom, in their
    /// manual order (STABLE sort). The Settings list, meanwhile, keeps the
    /// manual order.
    pub fn suggested(&self, limit: usize) -> Vec<&Opener> {
        let mut v: Vec<&Opener> = self.openers.iter().collect();
        v.sort_by_key(|o| std::cmp::Reverse(o.last_used));
        v.truncate(limit);
        v
    }

    /// Suggestions ADAPTED to the `ext` extension: only openers
    /// **set** (`default_exts`) OR **already used** (`used_exts`) for this
    /// extension, sorted by recency (MRU). Avoids mixing in unrelated
    /// programs (e.g. a text editor offered for a `.mp4`). If `ext` is empty (file
    /// with no extension), falls back to the global MRU.
    ///
    /// An empty list is possible (extension never opened) → the GUI keeps the
    /// "Choose an application…" entry, which itself lists the programs
    /// recommended by the OS; choosing one there FEEDS the learning (via
    /// `record_use`).
    pub fn suggested_for_ext(&self, ext: &str, limit: usize) -> Vec<&Opener> {
        let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        if ext.is_empty() {
            return self.suggested(limit);
        }
        let mut v: Vec<&Opener> = self
            .openers
            .iter()
            .filter(|o| {
                o.default_exts.iter().any(|e| e == &ext) || o.used_exts.iter().any(|e| e == &ext)
            })
            .collect();
        v.sort_by_key(|o| std::cmp::Reverse(o.last_used));
        v.truncate(limit);
        v
    }
}
