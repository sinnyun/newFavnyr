use super::*;

thread_local! {
    /// Cache of OS app icons by `(lowercase extension, jumbo?)` → a single shell
    /// extraction per extension/size and per session, regardless of the
    /// folder's size. `None` is also memoized (no retry). Two sizes:
    /// 32 px (list) and 256 px (previews). UI thread only.
    static EXT_ICON_CACHE: RefCell<HashMap<(String, bool), Option<Image>>> =
        RefCell::new(HashMap::new());
}

/// Icon of the OS's default application for extension `ext` (without the dot),
/// cached by extension + size. `big` = preview mode → 256 px icon (sharp
/// even enlarged); otherwise 32 px (list mode). `None` (→ empty `Image`) if
/// unavailable: the view falls back to the SVG type icon. Windows (Linux stub).
pub(in crate::bridge) fn ext_app_icon(ext: &str, big: bool) -> Image {
    if ext.is_empty() {
        return Image::default();
    }
    let key = (ext.to_ascii_lowercase(), big);
    EXT_ICON_CACHE.with(|c| {
        if let Some(v) = c.borrow().get(&key) {
            return v.clone().unwrap_or_default();
        }
        let img = openwith::icon_rgba_for_ext(&key.0, big).map(|(rgba, w, h)| {
            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &rgba, w, h,
            ))
        });
        c.borrow_mut().insert(key, img.clone());
        img.unwrap_or_default()
    })
}

thread_local! {
    /// Cache of icons SPECIFIC to a path by `(path, jumbo?)` — a single
    /// shell extraction per file/size and per session. Essential:
    /// unlike the per-extension cache, this path does disk I/O (reading
    /// the binary's resources or Shell-resolving a `.lnk`) and `entry_to_row`
    /// is replayed on every sort/filter.
    static SELF_ICON_CACHE: RefCell<HashMap<(String, bool), Option<Image>>> =
        RefCell::new(HashMap::new());
}

/// Does this file type carry its OWN icon (≠ generic icon for its
/// extension)? Executables and related types generally embed their own
/// resource, which must be resolved by path rather than by extension.
pub(in crate::bridge) fn has_own_icon(ext: &str) -> bool {
    matches!(ext, "exe" | "scr" | "cpl" | "ico")
}

/// Exact icon of path `path`, cached by path + size. Unlike
/// [`self_icon`], it doesn't replace a failure with the extension's generic icon:
/// a `.lnk` shortcut needs this to keep its own `IconLocation`.
pub(in crate::bridge) fn cached_path_icon(path: &Path, big: bool) -> Option<Image> {
    let key = (path.display().to_string(), big);
    let cached = SELF_ICON_CACHE.with(|c| c.borrow().get(&key).cloned());
    if let Some(v) = cached {
        return v;
    }
    let img = openwith::icon_rgba_for_path(&key.0, big).map(|(rgba, w, h)| {
        Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &rgba, w, h,
        ))
    });
    SELF_ICON_CACHE.with(|c| c.borrow_mut().insert(key, img.clone()));
    img
}

/// Icon specific to file `path`, cached by path + size. Falls back to
/// the extension icon if the shell returns nothing.
pub(in crate::bridge) fn self_icon(path: &Path, ext: &str, big: bool) -> Image {
    cached_path_icon(path, big).unwrap_or_else(|| ext_app_icon(ext, big))
}

#[cfg(not(windows))]
thread_local! {
    /// Resolved launcher icons, by `.desktop` path. Finding one walks the icon
    /// theme and decoding it parses an image; a folder of launchers would pay
    /// both on every re-listing — a sort or a filter — without this.
    static DESKTOP_ICON_CACHE: RefCell<HashMap<String, Option<Image>>> =
        RefCell::new(HashMap::new());
}

/// Icon of a `.desktop` launcher, or `None` when it declares none or names one
/// the theme does not provide.
///
/// The file is handed to the renderer by PATH rather than decoded here: it
/// reads SVG as well as bitmaps, and launcher icons are very often vector —
/// decoding to fixed-size pixels would throw that away. Not split by size for
/// the same reason: one file serves every row height.
#[cfg(not(windows))]
pub(in crate::bridge) fn cached_desktop_icon(path: &Path) -> Option<Image> {
    let key = path.display().to_string();
    if let Some(cached) = DESKTOP_ICON_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return cached;
    }
    let image = openwith::desktop_icon_path(path)
        .and_then(|icon| Image::load_from_path(&icon).ok())
        .filter(|image| image.size().width > 0);
    DESKTOP_ICON_CACHE.with(|c| c.borrow_mut().insert(key, image.clone()));
    image
}

thread_local! {
    /// Cache of `.lnk` shortcut targets by PATH → `(target_is_folder,
    /// target_extension)`, or `None` if unresolved. Avoids a COM call + a `stat`
    /// per `.lnk` on EVERY (re)listing (sort, filter, timezone change…).
    static LNK_TARGET_CACHE: RefCell<HashMap<String, Option<(bool, String)>>> =
        RefCell::new(HashMap::new());
}

/// Icon of a `.lnk` shortcut. For a file target (or an unresolved one), the LINK's
/// PATH is submitted to the Shell first: it's the one that knows the `IconLocation`
/// possibly stored in the shortcut and the target's own icon. Manual
/// resolution now only serves the historical "folder + arrow" rendering
/// and the extension fallback if the Shell renders nothing.
/// `None` = link and target unresolved (→ generic `.lnk` icon) or non-Windows.
pub(in crate::bridge) fn resolve_lnk_icon(
    parent_display: &str,
    name: &str,
    big: bool,
) -> Option<(Image, bool)> {
    #[cfg(windows)]
    {
        let path = std::path::PathBuf::from(parent_display).join(name);
        let key = path.display().to_string();
        let cached = LNK_TARGET_CACHE.with(|c| c.borrow().get(&key).cloned());
        let resolved = match cached {
            Some(v) => v,
            None => {
                let v = openwith::resolve_shortcut(&path).map(|t| {
                    let ext = t
                        .extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    (t.is_dir(), ext)
                });
                LNK_TARGET_CACHE.with(|c| c.borrow_mut().insert(key, v.clone()));
                v
            }
        };
        match resolved {
            // Keeps the explicit historical "folder + arrow" rendering: a
            // non-empty Shell image would take priority over this SVG in Slint.
            Some((true, _)) => Some((Image::default(), true)),
            Some((false, ext)) => Some((
                cached_path_icon(&path, big).unwrap_or_else(|| ext_app_icon(&ext, big)),
                false,
            )),
            None => cached_path_icon(&path, big).map(|icon| (icon, false)),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (parent_display, name, big);
        None
    }
}

pub(in crate::bridge) fn row_app_icon(
    parent_display: &str,
    name: &str,
    ext: &str,
    is_dir: bool,
    big: bool,
) -> (Image, bool) {
    if is_dir {
        return (Image::default(), false);
    }
    // A launcher shows the application it starts, like everywhere else on the
    // desktop — a row of identical generic icons says nothing about which game
    // or program each entry actually is.
    #[cfg(not(windows))]
    if ext == "desktop"
        && let Some(icon) = cached_desktop_icon(&Path::new(parent_display).join(name))
    {
        return (icon, false);
    }
    if ext == "lnk" {
        return resolve_lnk_icon(parent_display, name, big)
            .unwrap_or_else(|| (ext_app_icon(ext, big), false));
    }
    if has_own_icon(ext) {
        return (
            self_icon(&Path::new(parent_display).join(name), ext, big),
            false,
        );
    }
    (ext_app_icon(ext, big), false)
}

/// Parses the extension filter's free text into lowercase extensions
/// without a dot. FLEXIBLE syntax: comma AND/OR space separators
/// ("jpg, png", "jpg png", ".JPG,.PNG" → `["jpg","png"]`). Empty if nothing.
pub(in crate::bridge) fn parse_ext_filter(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .map(|t| t.trim_start_matches('.').to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Applies the extension filter to `entries` (in place): keeps only the
/// files whose extension is in the filter. FOLDERS remain visible
/// (navigation preserved). No-op if the filter is empty/disabled.
pub(in crate::bridge) fn apply_ext_filter(entries: &mut Vec<Entry>, on: bool, filter: &str) {
    if !on {
        return;
    }
    let exts = parse_ext_filter(filter);
    if exts.is_empty() {
        return;
    }
    entries.retain(|e| {
        e.is_dir || {
            let ext = ops::ext_of(&e.name);
            !ext.is_empty() && exts.contains(&ext)
        }
    });
}
