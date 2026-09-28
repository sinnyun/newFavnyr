use super::*;

/// XDG application directories (priority order).
fn app_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(h) = std::env::var("XDG_DATA_HOME") {
        if !h.is_empty() {
            dirs.push(std::path::PathBuf::from(h).join("applications"));
        }
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(std::path::PathBuf::from(home).join(".local/share/applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for d in data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(std::path::PathBuf::from(d).join("applications"));
    }
    dirs
}

/// The `[Desktop Entry]` fields Favnyr acts on. Only those: the format has
/// dozens more, and reading one we never consult would just be noise.
#[derive(Default)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub mimes: String,
    /// A higher-priority entry marked as hidden disables this desktop ID.
    pub hidden: bool,
    /// Theme icon name OR absolute path to an image file — both are legal,
    /// and the two are told apart when resolving, not here.
    pub icon: String,
    /// Working directory to start the application in (`Path=`).
    pub work_dir: String,
    /// The application wants a terminal to run in.
    pub terminal: bool,
    /// `Application`, `Link` or `Directory`. Empty when absent.
    pub kind: String,
    /// Destination of a `Type=Link` entry.
    pub url: String,
}

/// Minimal parse of a `.desktop` file, limited to its `[Desktop Entry]`
/// group — a trailing `[Desktop Action …]` group carries its own `Exec`,
/// which must not be mistaken for the main one.
pub fn parse_desktop(content: &str) -> DesktopEntry {
    let mut entry = DesktopEntry::default();
    let mut in_entry = false;
    for line in content.lines() {
        let line = line.trim();
        // A comment may hold anything, including a line that looks like a
        // key (these files often start with a `#!` shebang).
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            if entry.name.is_empty() {
                entry.name = v.to_string();
            }
        } else if let Some(v) = line.strip_prefix("Exec=") {
            entry.exec = v.to_string();
        } else if let Some(v) = line.strip_prefix("MimeType=") {
            entry.mimes = v.to_string();
        } else if let Some(v) = line.strip_prefix("Hidden=") {
            entry.hidden = v.eq_ignore_ascii_case("true");
        } else if let Some(v) = line.strip_prefix("Icon=") {
            entry.icon = v.to_string();
        } else if let Some(v) = line.strip_prefix("Path=") {
            entry.work_dir = v.to_string();
        } else if let Some(v) = line.strip_prefix("Terminal=") {
            entry.terminal = v.eq_ignore_ascii_case("true");
        } else if let Some(v) = line.strip_prefix("Type=") {
            entry.kind = v.to_string();
        } else if let Some(v) = line.strip_prefix("URL=") {
            entry.url = v.to_string();
        }
    }
    entry
}

/// Splits an `Exec=` value into an `argv`, following the Desktop Entry
/// specification, and resolves its field codes.
///
/// Not [`crate::bridge::split_args`]: that one also treats `'` as a quote,
/// which is right for a command the user typed but wrong here — the
/// specification quotes with `"` only, so an apostrophe in a path (a folder
/// named "it's here") is an ordinary character that must survive intact.
/// Inside quotes, `\` escapes the next character.
///
/// `file` is the document to hand over, if any: `%f`/`%F`/`%u`/`%U` take it,
/// and are simply dropped when there is nothing to pass — launching an
/// application on its own must not leave a stray `%u` in its arguments.
/// The remaining codes carry nothing Favnyr can supply and are dropped too.
pub fn exec_argv(exec: &str, file: Option<&str>) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if quoted => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    argv.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '%' if !quoted => {
                // A code is always two characters; `%%` is a literal `%`.
                match chars.next() {
                    Some('%') => {
                        current.push('%');
                        started = true;
                    }
                    Some('f' | 'F' | 'u' | 'U') => {
                        if let Some(file) = file {
                            current.push_str(file);
                            started = true;
                        }
                    }
                    // %i %c %k and the deprecated ones: nothing to give.
                    Some(_) | None => {}
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        argv.push(current);
    }
    // A code alone in its token leaves an empty argument behind, which the
    // program would receive as a genuine empty parameter.
    argv.retain(|a| !a.is_empty());
    argv
}

/// Rough MIME type of an extension (common cases; otherwise `None` → we
/// exclude nothing). Good enough to filter a file browser.
fn mime_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "wav" => "audio/x-wav",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "zip" => "application/zip",
        _ => return None,
    })
}

pub(super) fn desktop_matches_mime(
    entry_mimes: &str,
    mime: Option<&str>,
    include_without_mime: bool,
) -> bool {
    let Some(mime) = mime else {
        return true;
    };
    entry_mimes.split(';').any(|item| item.trim() == mime)
        || (include_without_mime && entry_mimes.trim().is_empty())
}

pub(super) fn desktop_is_handler(
    entry: &DesktopEntry,
    mime: Option<&str>,
    include_without_mime: bool,
) -> bool {
    // `NoDisplay` only keeps an entry out of application menus. MIME-specific
    // launchers legitimately use it while remaining valid for "Open with".
    !entry.hidden
        && !entry.name.is_empty()
        && !entry.exec.is_empty()
        && desktop_matches_mime(&entry.mimes, mime, include_without_mime)
}

pub fn handlers_for_ext(ext: &str, include_without_mime: bool) -> Vec<AppHandler> {
    let mime = mime_for_ext(ext);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue; // priority to the first folder (user override)
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let entry = parse_desktop(&content);
            // Entries without a MIME declaration are optional; entries that
            // explicitly declare another MIME type remain excluded.
            if !desktop_is_handler(&entry, mime, include_without_mime) {
                continue;
            }
            out.push(AppHandler {
                name: entry.name,
                key: id.to_string(),
                exe: None,
                recommended: false,
            });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Launches the `.desktop` app `key` on `path` (substitutes XDG field codes).
pub fn launch(key: &str, _ext: &str, path: &Path) -> Result<()> {
    for dir in app_dirs() {
        let candidate = dir.join(key);
        let Ok(content) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        let entry = parse_desktop(&content);
        if entry.exec.is_empty() {
            continue;
        }
        let file = path.to_string_lossy().into_owned();
        let mut argv = exec_argv(&entry.exec, Some(&file));
        if argv.is_empty() {
            return Err(anyhow::anyhow!("empty Exec for {key}"));
        }
        // The specification has no variables in `Exec`, so a strict reader
        // is left with the literal text `$HOME/…`, which names no file.
        // Hand-written launchers use them all the same and the desktops
        // accept them, so the program is resolved the way Favnyr already
        // resolves a path typed in the address bar. A bare command name
        // comes back untouched and is still found on `PATH`.
        let prog = favnyr_core::fs::expand_typed_path(&argv.remove(0));
        // An entry declaring no field code still has to receive the file.
        if !argv.iter().any(|a| a == &file) {
            argv.push(file);
        }
        return crate::actions::spawn_program(&prog, &argv);
    }
    Err(anyhow::anyhow!("application not found: {key}"))
}

/// Acts on the launcher at `path`: starts the application it describes, or
/// opens the address for a `Type=Link` entry.
///
/// Unlike [`launch`], the entry is designated by its own path rather than
/// by a name looked up in the XDG folders, so a launcher sitting anywhere —
/// a download, a folder of games — works the same.
pub fn launch_desktop_file(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let entry = parse_desktop(&content);

    // A shortcut to an address: nothing to execute, the destination is
    // handed to the desktop like any other address.
    if entry.kind == "Link" {
        if entry.url.is_empty() {
            return Err(anyhow::anyhow!("no URL in {}", path.display()));
        }
        return open::that_detached(&entry.url)
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", entry.url));
    }

    // No file to hand over: the launcher is being started on its own.
    let mut argv = exec_argv(&entry.exec, None);
    if argv.is_empty() {
        return Err(anyhow::anyhow!("no Exec in {}", path.display()));
    }
    // Variables resolved as in `launch`, and for the same reason.
    let program = favnyr_core::fs::expand_typed_path(&argv.remove(0));

    // `Path=` may be present but empty, which means "no preference".
    let work_dir = Some(entry.work_dir.as_str())
        .filter(|d| !d.is_empty())
        .map(favnyr_core::fs::expand_typed_path);

    if entry.terminal {
        // Best effort: `-e <command>` is the option the usual terminals
        // share. An entry asking for a terminal is rare for a launcher, and
        // failing to start beats starting the program with no console at
        // all, silently.
        let term = crate::actions::pick_terminal()
            .ok_or_else(|| anyhow::anyhow!("no terminal detected for {}", path.display()))?;
        let mut term_argv = vec!["-e".to_string(), program.display().to_string()];
        term_argv.extend(argv);
        return crate::actions::spawn_program_in(Path::new(&term), &term_argv, work_dir.as_deref());
    }
    crate::actions::spawn_program_in(&program, &argv, work_dir.as_deref())
}

/// Folders holding icon themes, in priority order: the user's own first, so
/// a locally installed application overrides a system one of the same name.
fn icon_base_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var("HOME").ok();
    match std::env::var("XDG_DATA_HOME") {
        Ok(h) if !h.is_empty() => dirs.push(std::path::PathBuf::from(h).join("icons")),
        _ => {
            if let Some(home) = &home {
                dirs.push(std::path::PathBuf::from(home).join(".local/share/icons"));
            }
        }
    }
    // Long-standing location, still used by applications that install by
    // hand rather than through a package.
    if let Some(home) = &home {
        dirs.push(std::path::PathBuf::from(home).join(".icons"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for d in data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(std::path::PathBuf::from(d).join("icons"));
    }
    dirs
}

/// Icon theme the desktop is currently using, if it can be read cheaply.
/// Only the two mainstream settings files are consulted; anything else
/// falls back to `hicolor`, which every theme is required to inherit and
/// where applications install their own icon.
fn current_icon_theme() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let config = match std::env::var("XDG_CONFIG_HOME") {
        Ok(c) if !c.is_empty() => std::path::PathBuf::from(c),
        _ => std::path::PathBuf::from(&home).join(".config"),
    };
    // KDE: `[Icons] Theme=`. GTK: `gtk-icon-theme-name=`.
    for (file, key) in [
        ("kdeglobals", "Theme="),
        ("gtk-3.0/settings.ini", "gtk-icon-theme-name="),
        ("gtk-4.0/settings.ini", "gtk-icon-theme-name="),
    ] {
        let Ok(text) = std::fs::read_to_string(config.join(file)) else {
            continue;
        };
        if let Some(value) = text
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(key))
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// Extensions an icon may use, best first: a vector image stays sharp at
/// any row height, which a fixed-size bitmap does not.
const ICON_EXTENSIONS: &[&str] = &["svg", "png", "xpm"];

/// Icon sizes to try, largest first — the row scales it down, and scaling
/// down keeps far more detail than scaling a 16px icon up.
const ICON_SIZES: &[u32] = &[512, 256, 128, 96, 64, 48, 40, 36, 32, 24, 22, 16];

/// Finds the file an `Icon=` value refers to.
///
/// The value is allowed to be either an absolute path to an image or a
/// theme icon name, and both occur in the wild — an application installed
/// by hand points straight at its own file, while a packaged one names an
/// icon it installed into the theme. The two are told apart here.
///
/// The theme search follows the usual layout rather than reading every
/// `index.theme`: probing a bounded list of candidate paths costs a handful
/// of `stat` calls, where parsing the theme descriptions of a full icon set
/// would cost far more on every listing. The result is cached by the caller.
pub fn resolve_icon(icon: &str) -> Option<std::path::PathBuf> {
    if icon.is_empty() {
        return None;
    }
    // An absolute path is used as it stands.
    let as_path = std::path::Path::new(icon);
    if as_path.is_absolute() {
        return as_path.is_file().then(|| as_path.to_path_buf());
    }
    // A name carrying an extension is still a name: the specification says
    // to drop it before searching the theme.
    let stem = ICON_EXTENSIONS
        .iter()
        .find_map(|ext| icon.strip_suffix(&format!(".{ext}")))
        .unwrap_or(icon);

    let mut themes: Vec<String> = Vec::new();
    if let Some(theme) = current_icon_theme() {
        themes.push(theme);
    }
    // Every theme inherits from it, and an application's own icon lands
    // there whatever the desktop in use.
    themes.push("hicolor".to_string());

    find_themed_icon(&icon_base_dirs(), &themes, stem)
        .or_else(|| find_legacy_icon(&pixmap_dirs(), stem))
}

/// Folders of the flat, pre-theme layout, kept for what still installs
/// there.
fn pixmap_dirs() -> Vec<std::path::PathBuf> {
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    data_dirs
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|d| std::path::PathBuf::from(d).join("pixmaps"))
        .collect()
}

/// Searches the themed layout. Kept apart from the environment so the
/// ordering rules can be tested against a folder tree built for the test.
pub(super) fn find_themed_icon(
    bases: &[std::path::PathBuf],
    themes: &[String],
    stem: &str,
) -> Option<std::path::PathBuf> {
    const CONTEXTS: &[&str] = &["apps", "devices", "places", "mimetypes"];
    for base in bases {
        for theme in themes {
            let theme_dir = base.join(theme);
            // Vector first, whatever the size folders hold: one file stays
            // sharp at every row height.
            for context in CONTEXTS {
                let svg = theme_dir
                    .join("scalable")
                    .join(context)
                    .join(format!("{stem}.svg"));
                if svg.is_file() {
                    return Some(svg);
                }
            }
            for size in ICON_SIZES {
                for context in CONTEXTS {
                    for ext in ICON_EXTENSIONS {
                        // Both orderings exist across themes.
                        for candidate in [
                            theme_dir
                                .join(format!("{size}x{size}"))
                                .join(context)
                                .join(format!("{stem}.{ext}")),
                            theme_dir
                                .join(context)
                                .join(format!("{size}"))
                                .join(format!("{stem}.{ext}")),
                        ] {
                            if candidate.is_file() {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Searches the flat layout, where the file sits directly in the folder.
pub(super) fn find_legacy_icon(
    dirs: &[std::path::PathBuf],
    stem: &str,
) -> Option<std::path::PathBuf> {
    for dir in dirs {
        for ext in ICON_EXTENSIONS {
            let candidate = dir.join(format!("{stem}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Image a `.desktop` launcher wants to be shown with, as a path so the
/// caller can hand it straight to the renderer — which reads SVG as well as
/// bitmaps, where returning decoded pixels would lose the vector.
pub fn desktop_icon_path(path: &Path) -> Option<std::path::PathBuf> {
    let content = std::fs::read_to_string(path).ok()?;
    resolve_icon(&parse_desktop(&content).icon)
}

/// Icon extraction — not implemented on Linux (v1; XDG icon theme
/// resolution deferred).
pub fn icon_rgba(_path: &str) -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// Icon by extension — not implemented on Linux: it would take MIME
/// resolution followed by a lookup through the XDG icon theme, which is
/// a great deal of work for a per-extension icon. Falls back to the
/// generic type icon on the view side.
pub fn icon_rgba_for_ext(_ext: &str, _big: bool) -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// File-specific icon — not implemented on Linux.
pub fn icon_rgba_for_path(_path: &str, _big: bool) -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// Native file picker on Linux "in its own way": we delegate to the
/// desktop portal via `zenity` (GTK) then `kdialog` (KDE) — present on the
/// vast majority of environments, **without adding a dependency**. The
/// chosen path is written to stdout; cancellation ⇒ non-zero exit code.
pub fn browse_for_exe(lang: Lang) -> Option<String> {
    use std::process::Command;

    let title = i18n::tr(lang, "ow_dialog_choose_program");

    // zenity: GTK. `--file-selection` returns the absolute path on stdout.
    // If zenity responded (choice OR cancellation), we do NOT fall back to kdialog
    // (the user has already interacted) → explicit `return None` on cancellation.
    if let Ok(out) = Command::new("zenity")
        .args(["--file-selection", &format!("--title={title}")])
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(p);
            }
        }
        return None;
    }

    // zenity absent → kdialog (KDE). `--getopenfilename <dir>` → path on stdout.
    if let Ok(out) = Command::new("kdialog")
        .args(["--getopenfilename", ".", "--title", &title])
        .output()
        && out.status.success()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(p);
        }
    }
    None
}

/// No `.lnk` shortcuts outside Windows: symbolic links are followed
/// natively by the listing (a folder symlink is navigated directly).
/// (Not called outside Windows — the API stays symmetric.)
#[allow(dead_code)]
pub fn resolve_shortcut(_path: &Path) -> Option<std::path::PathBuf> {
    None
}

/// Creating a `.lnk` shortcut — not applicable outside Windows (the "New
/// shortcut" entry isn't shown there).
pub fn create_shortcut(_lnk_path: &Path, _target: &Path) -> Result<()> {
    Err(anyhow::anyhow!(".lnk shortcuts: Windows only"))
}

/// Shortcut target picker — not applicable outside Windows.
pub fn browse_for_target(_lang: Lang) -> Option<String> {
    None
}

/// Target folder picker — not applicable outside Windows.
pub fn browse_for_folder() -> Option<String> {
    None
}
