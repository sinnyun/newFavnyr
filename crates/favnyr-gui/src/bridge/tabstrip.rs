use super::*;

/// Splits an absolute path into breadcrumb segments. Each `Crumb`
/// carries a `label` (displayed name) and the cumulative absolute `path` to
/// navigate to on click. The first segment represents the root.
///
/// - Linux: `/usr/lib` → [("/", "/"), ("usr", "/usr"), ("lib", "/usr/lib")].
/// - Windows: `C:\Users\user` → [("C:\\", "C:\\"), ("Users", "C:\\Users"),
///   ("user", "C:\\Users\\user")]. The **drive prefix** (`C:`) and the
///   **root** (`\`) are merged into ONE `C:\` segment — without this we'd get
///   a "C:" segment (drive-relative, ambiguous) followed by an incorrect "/".
pub(super) fn breadcrumbs(path: &Path) -> Vec<Crumb> {
    use std::path::{Component, Prefix};
    // UNC server root `\\HOST`: `std::path` recognizes NO prefix there
    // (it requires `server\share`) → components = RootDir + Normal("HOST"), and the
    // generic rendering used to give "/ › HOST" — a "/" that makes no sense on
    // Windows. A single "\\HOST" crumb.
    #[cfg(windows)]
    if let Some(server) = rfs::unc_server_root(path) {
        let nav = format!(r"\\{server}");
        return vec![Crumb {
            label: nav.clone().into(),
            path: nav.into(),
        }];
    }
    let mut out: Vec<Crumb> = Vec::new();
    let mut acc = PathBuf::new();
    // Windows: we defer emitting the prefix (`C:`) until the root, to
    // merge them into "C:\". `pending_prefix` = a prefix seen but not yet
    // emitted.
    let mut pending_prefix = false;
    // Emits the prefix alone (degenerate case: prefix without a root, e.g. "C:foo"
    // drive-relative — unlikely since we only navigate absolute paths,
    // but we stay robust).
    macro_rules! flush_prefix {
        () => {
            if pending_prefix {
                pending_prefix = false;
                let nav = acc.display().to_string();
                out.push(Crumb {
                    label: nav.clone().into(),
                    path: nav.into(),
                });
            }
        };
    }
    // UNC share name (remembered at the prefix): the share-root crumb
    // then displays as "share" alone — the server crumb already precedes it.
    let mut unc_share: Option<String> = None;
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => {
                // UNC path `\\HOST\share\…`: a clickable "\\HOST" crumb
                // leads to the server root, which enumerates the shares.
                if let Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) = p.kind() {
                    let nav = format!(r"\\{}", server.to_string_lossy());
                    out.push(Crumb {
                        label: nav.clone().into(),
                        path: nav.into(),
                    });
                    unc_share = Some(share.to_string_lossy().into_owned());
                }
                acc.push(p.as_os_str());
                pending_prefix = true;
            }
            Component::RootDir => {
                acc.push(Component::RootDir.as_os_str());
                let nav = acc.display().to_string();
                // UNC → "share" label (server already in the preceding crumb);
                // with a drive prefix (Windows) → "C:\"; otherwise (Unix) → "/".
                let label = match unc_share.take() {
                    Some(share) if !share.is_empty() => share,
                    _ if pending_prefix => nav.clone(),
                    _ => "/".to_string(),
                };
                pending_prefix = false;
                out.push(Crumb {
                    label: label.into(),
                    path: nav.into(),
                });
            }
            Component::Normal(seg) => {
                flush_prefix!();
                acc.push(seg);
                out.push(Crumb {
                    label: seg.to_string_lossy().to_string().into(),
                    path: acc.display().to_string().into(),
                });
            }
            // `.` / `..`: added as-is while staying robust (rare case
            // after canonicalization).
            other => {
                flush_prefix!();
                let s = other.as_os_str().to_string_lossy().to_string();
                if !s.is_empty() {
                    acc.push(&s);
                    out.push(Crumb {
                        label: s.clone().into(),
                        path: acc.display().to_string().into(),
                    });
                }
            }
        }
    }
    // Final flush (degenerate case "C:" without a root) — inline to avoid
    // reassigning `pending_prefix` one last time (dead assignment).
    if pending_prefix {
        let nav = acc.display().to_string();
        out.push(Crumb {
            label: nav.clone().into(),
            path: nav.into(),
        });
    }
    if out.is_empty() {
        out.push(Crumb {
            label: "/".into(),
            path: "/".into(),
        });
    }
    out
}

/// Imposed width of a tab, in logical pixels, estimated from its title.
/// The bridge is the bar's geometric source of truth: widths and
/// offsets are provided as plain values to the Slint model (`TabInfo`). The
/// drop-point computation therefore stays independent of the reactive layout and shares
/// the rendering's geometry. The estimate targets an 11 px sans-serif
/// interface font; a wider title is simply elided by the `TabItem`.
pub(super) fn estimate_tab_width(title: &str) -> f32 {
    // TabItem's fixed chrome: padding-left 10 + spacing 6 + "×" slot 18 +
    // padding-right 4 = 38 px.
    const CHROME: f32 = 38.0;
    let text: f32 = title
        .chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | 't' | 'f' | 'r' | '.' | ',' | '\'' | '!' | ':' | ';' | '|' | ' ' => {
                3.4
            }
            'm' | 'w' | 'M' | 'W' | '@' => 10.0,
            c if c.is_ascii_uppercase() || c.is_ascii_digit() => 7.2,
            // CJK / full-width.
            c if (c as u32) >= 0x2E80 => 11.5,
            _ => 6.0,
        })
        .sum();
    // Design bounds (e.g. TabItem's min/max-width).
    (CHROME + text).clamp(90.0, 180.0)
}

/// (width, offset) of each tab in `tabs-row` (2 px spacing), in logical
/// px — the PLAIN values pushed into `TabInfo`.
pub(super) fn tab_layout(titles: &[String]) -> Vec<(f32, f32)> {
    const SPACING: f32 = 2.0; // MUST == tabs-row.spacing (slint)
    let mut off = 0.0_f32;
    titles
        .iter()
        .map(|t| {
            let w = estimate_tab_width(t);
            let cur = (w, off);
            off += w + SPACING;
            cur
        })
        .collect()
}

/// Height of a VERTICAL bar tab + spacing.
/// MUST == `TabItem.height` / `vtabs-col.spacing` on the Slint side.
pub(super) const VTAB_EXTENT: f32 = 26.0;
pub(super) const VTAB_SPACING: f32 = 2.0;

/// (extent, offset) of the tabs on the panel bar's **main axis**:
/// horizontal → widths estimated from the title (`tab_layout`);
/// vertical → UNIFORM step (fixed height), the offset is in Y. Same `TabInfo`
/// contract in both cases — the gap claim (Slint) and the
/// hit-tests (bridge) share this single source of truth.
pub(super) fn tab_layout_for(mode: u8, titles: &[String]) -> Vec<(f32, f32)> {
    if mode == 0 {
        return tab_layout(titles);
    }
    (0..titles.len())
        .map(|i| (VTAB_EXTENT, i as f32 * (VTAB_EXTENT + VTAB_SPACING)))
        .collect()
}

/// Width, in logical pixels, of the vertical tab bar.
/// Uses the user-chosen width if positive, otherwise 40% of the
/// panel with a 180 px cap. The result stays between 110 px
/// and 60% of the panel. This computation must match `vbar-w` on the Slint side.
pub(super) fn vbar_width(panel_w: f32, user_w: f32) -> f32 {
    let base = if user_w > 0.0 {
        user_w
    } else {
        (panel_w * 0.40).min(180.0)
    };
    base.clamp(110.0, (panel_w * 0.60).max(110.0))
}

/// Scroll target of the tab bar (px, `viewport-x` ≤ 0) to PROPERLY reveal
/// a tab — rather than an arbitrary distance jump. The
/// geometry (same widths/offsets as the `TabInfo` model) is recomputed here
/// (the bridge has loops, unlike Slint). `idx >= 0` → reveal
/// tab `idx` (wheel); `idx < 0` → step "tab by tab" in direction
/// `forward` (chevrons). `viewport_x`/`view_w` come from the Flickable on the Slint side.
pub(super) fn tab_scroll_target(
    state: &AppState,
    panel: usize,
    idx: i32,
    forward: bool,
    viewport_x: f32,
    view_w: f32,
) -> f32 {
    let panels = state.panels.borrow();
    let Some(p) = panels.get(panel) else {
        return viewport_x;
    };
    let titles: Vec<String> = p
        .tabs
        .tabs
        .iter()
        .map(|t| tab_title(&t.current_path))
        .collect();
    // (extent, offset) on the bar's main axis — the rest of the computation is
    // agnostic to orientation (`view_w` = the same axis's visible extent).
    let geo = tab_layout_for(p.tab_bar_mode, &titles);
    let Some(&(lw, lo)) = geo.last() else {
        return viewport_x;
    };
    let content_w = lo + lw;
    if content_w <= view_w + 0.5 {
        return 0.0; // no overflow
    }
    let max_scroll = view_w - content_w; // < 0 (right end)
    let vleft = -viewport_x; // visible left edge (content coords)
    let target = if idx >= 0 {
        // Reveal tab `idx`: overflowing right edge → align right; hidden
        // left edge → align left; otherwise already visible (unchanged).
        match geo.get(idx as usize) {
            Some(&(w, off)) if off + w > vleft + view_w => view_w - (off + w),
            Some(&(_w, off)) if off < vleft => -off,
            _ => viewport_x,
        }
    } else if forward {
        // 1st tab not entirely visible on the right → align its right edge.
        geo.iter()
            .find(|(w, off)| off + w > vleft + view_w + 0.5)
            .map(|(w, off)| view_w - (off + w))
            .unwrap_or(max_scroll)
    } else {
        // Last tab hidden on the left (just before the visible zone) → left edge.
        let mut t = 0.0_f32;
        for &(_w, off) in &geo {
            if off < vleft - 0.5 {
                t = -off;
            } else {
                break;
            }
        }
        t
    };
    target.clamp(max_scroll, 0.0)
}

/// Tab title from a path. Prefers the last segment; special-cased
/// for `$HOME` and the root (`/`).
///
/// $HOME → `~` on Unix (universal convention). On Windows, `~` isn't
/// a convention known to users → we let the folder's real name
/// ("firstname.lastname") show through `file_name` below.
pub(super) fn tab_title(path: &Path) -> String {
    #[cfg(not(windows))]
    if let Some(home) = dirs::home_dir()
        && path == home
    {
        return "~".to_string();
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        return name.to_string();
    }
    let s = path.display().to_string();
    if s.is_empty() { "/".to_string() } else { s }
}

// Tabs are embedded in each `PanelView` via `tabs: [TabInfo]`, which
// `update_panels_ui` fills in for each panel.
