use super::*;

/// Tags describing ONE file. Their presence is what makes a command run once
/// per selected item. Order = order of the GUI insertion buttons.
pub const TAGS: &[&str] = &[
    "file", "dir", "dirname", "setname", "name", "stem", "ext", "uri",
];

/// Tag standing for the WHOLE selection. Unlike the tags above it does not
/// describe one file, so it does the opposite: the command runs a single time
/// and this tag expands, in place, into one argument per selected path.
///
/// It is what makes "compress everything into one archive" expressible:
/// `a {dir}/{dirname}.7z {files}`. It must be an argument on its own — anywhere
/// else it stays literal, like any unrecognized tag.
pub const LIST_TAG: &str = "{files}";

/// Like [`LIST_TAG`] but expanding to bare NAMES instead of full paths.
///
/// Exists for tools that take a base directory and then relative names —
/// `tar -C <dir> a.txt b.txt`. Handed absolute paths, `tar` stores the whole
/// tree that leads to each file, so the archive holds
/// `home/user/Docs/a.txt` rather than `a.txt`.
///
/// Pair it with `{dir}`, which in a batch run designates the folder the whole
/// selection shares.
pub const LIST_NAMES_TAG: &str = "{names}";

/// Context of a target file (all tag values pre-computed).
#[derive(Debug, Clone, Default)]
pub struct TagContext {
    pub file: String,
    pub dir: String,
    /// Name alone of the containing folder — `{dir}` gives its full path.
    pub dirname: String,
    /// Name to give something built FROM the selection, an archive above all.
    ///
    /// The only value here that describes the selection rather than one path:
    /// the item's own name when a single one is selected, and the name of the
    /// folder they share when there are several — which is what an archiver
    /// does, having nothing better to name the whole after.
    pub setname: String,
    pub name: String,
    pub stem: String,
    pub ext: String,
    pub uri: String,
}

impl TagContext {
    /// Builds the context from a path (absolute preferred).
    pub fn from_path(path: &Path) -> Self {
        let file = path.display().to_string();
        let parent = path.parent();
        let dir = parent.map(|p| p.display().to_string()).unwrap_or_default();
        let dirname = parent
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let uri = file_uri(&file);
        // A folder keeps its whole name, a file loses its extension: an
        // archive of `report.pdf` is `report.zip`, but a folder named
        // `archive.old` must not become `archive`. Telling the two apart needs
        // the filesystem, the extension alone cannot — and this is paid once
        // per launch, not per row.
        let setname = if path.is_dir() {
            name.clone()
        } else {
            stem.clone()
        };
        Self {
            file,
            dir,
            dirname,
            setname,
            name,
            stem,
            ext,
            uri,
        }
    }
}

/// Coarse `file://` URI (sufficient for editors/browsers). Encodes
/// the space; keeps other characters as-is (common case). Windows: `\`→`/`.
fn file_uri(path: &str) -> String {
    let norm = path.replace('\\', "/");
    let enc = norm.replace(' ', "%20");
    if enc.starts_with('/') {
        format!("file://{enc}")
    } else {
        // Windows path "C:/…" → file:///C:/…
        format!("file:///{enc}")
    }
}

/// Substitutes `{...}` tags in a template. `{{`/`}}` = literals; an
/// unknown tag is left as-is (never fails).
pub(super) fn substitute(template: &str, ctx: &TagContext) -> String {
    let mut out = String::with_capacity(template.len());
    let mut it = template.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '{' => {
                let mut tag = String::new();
                let mut closed = false;
                for nc in it.by_ref() {
                    if nc == '}' {
                        closed = true;
                        break;
                    }
                    tag.push(nc);
                }
                match (closed, tag.as_str()) {
                    (true, "file") => out.push_str(&ctx.file),
                    (true, "dir") => out.push_str(&ctx.dir),
                    (true, "dirname") => out.push_str(&ctx.dirname),
                    (true, "setname") => out.push_str(&ctx.setname),
                    (true, "name") => out.push_str(&ctx.name),
                    (true, "stem") => out.push_str(&ctx.stem),
                    (true, "ext") => out.push_str(&ctx.ext),
                    (true, "uri") => out.push_str(&ctx.uri),
                    // Unknown / unclosed → left literal.
                    (true, other) => {
                        out.push('{');
                        out.push_str(other);
                        out.push('}');
                    }
                    (false, rest) => {
                        out.push('{');
                        out.push_str(rest);
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}
