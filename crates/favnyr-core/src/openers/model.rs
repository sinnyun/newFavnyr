use super::*;

pub(super) static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn generate_id() -> String {
    format!(
        "op-{}-{}",
        now_secs(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

impl Opener {
    /// Does at least one argument contain a tag (i.e. depend on the file)?
    ///
    /// `{files}` is deliberately NOT one of them: it stands for the whole
    /// selection, so it means the opposite — a single run. (It also contains no
    /// `{file}` substring, the closing brace differing, so it cannot be counted
    /// here by accident.)
    pub fn has_tag(&self) -> bool {
        self.args
            .iter()
            .any(|a| TAGS.iter().any(|t| a.contains(&format!("{{{t}}}"))))
    }

    /// Does this command apply to a file whose extension is `ext` (no dot,
    /// lowercase)? An empty list and the `*` wildcard both mean "every file";
    /// an extensionless file therefore only matches those two.
    ///
    /// Only consulted for the FILE entry — see [`Opener::ctx_exts`].
    pub fn matches_ctx_ext(&self, ext: &str) -> bool {
        self.ctx_exts.is_empty()
            || self
                .ctx_exts
                .iter()
                .any(|e| e == CTX_EXT_ALL || e.eq_ignore_ascii_case(ext))
    }

    /// Does the template expand the whole selection in place, i.e. run ONCE
    /// however many items are selected? True when `{files}` stands alone as an
    /// argument — see [`LIST_TAG`].
    pub fn expands_list(&self) -> bool {
        self.args
            .iter()
            .any(|a| a == LIST_TAG || a == LIST_NAMES_TAG)
    }

    /// Substituted `argv` for ONE file. If there is no tag, `{file}` is
    /// implicit (the path is appended at the end).
    pub fn render_for_file(&self, ctx: &TagContext) -> Vec<String> {
        if self.args.is_empty() || !self.has_tag() {
            let mut v: Vec<String> = self.args.iter().map(|a| substitute(a, ctx)).collect();
            v.push(ctx.file.clone());
            v
        } else {
            self.args.iter().map(|a| substitute(a, ctx)).collect()
        }
    }

    /// Substituted `argv` for a SINGLE run covering the whole selection.
    ///
    /// Each standalone `{files}` becomes one argument per path, keeping its
    /// position in the command — so the list can sit before a trailing option,
    /// unlike the historical fallback which always appended paths at the end.
    /// The remaining tags describe `ctx`, which the caller builds from the
    /// first selected item; `{dir}` and `{dirname}` therefore designate the
    /// folder the whole selection shares, and `{setname}` names the selection
    /// itself.
    /// `paths` are the selected items; each expands to its full path for
    /// [`LIST_TAG`] and to its bare name for [`LIST_NAMES_TAG`].
    pub fn render_for_batch(&self, ctx: &TagContext, paths: &[&Path]) -> Vec<String> {
        // `{setname}` is the one tag that depends on HOW MANY items there are.
        // `ctx` describes the first of them, which names the whole only when it
        // is the whole; past that, the folder they share does.
        let ctx = if paths.len() > 1 {
            let mut many = ctx.clone();
            many.setname = many.dirname.clone();
            std::borrow::Cow::Owned(many)
        } else {
            std::borrow::Cow::Borrowed(ctx)
        };
        let ctx = ctx.as_ref();
        let mut out = Vec::with_capacity(self.args.len() + paths.len());
        for arg in &self.args {
            if arg == LIST_TAG {
                out.extend(paths.iter().map(|p| p.display().to_string()));
            } else if arg == LIST_NAMES_TAG {
                out.extend(paths.iter().map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string())
                }));
            } else {
                out.push(substitute(arg, ctx));
            }
        }
        out
    }
}
