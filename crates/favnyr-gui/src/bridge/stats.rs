use super::*;

// Recursive folder modification date ----------

pub(super) struct RMtimeJob {
    path: PathBuf,
    /// Recursive mtime depth for this job (`0` = don't compute the date).
    mtime_depth: u32,
    /// Recursive size depth for this job (`0` = don't compute the size).
    size_depth: u32,
    r#gen: u64,
    panel: usize,
    row: usize,
}

/// Applies a (recursive) mtime to a row's "modified" + "age" cells.
pub(super) fn apply_rmtime_to_row(row: &mut FileRow, m: i64, now: i64, lang: Lang) {
    row.modified = rfs::format_mtime(m, mtime_offset()).into();
    row.age = rfs::format_age(m, now, i18n::age_units(lang)).into();
    row.age_bucket = rfs::age_bucket(m, now);
}

/// Background worker: computes a folder's recursive mtime AND size off the UI
/// thread (one shared walk) and pushes the result via
/// `folder-stats-ready(path, mtime, size)`. Ignores stale jobs (outdated
/// generation = folder left / a depth changed).
pub(super) fn spawn_rmtime_worker(
    rx: mpsc::Receiver<RMtimeJob>,
    r#gen: Arc<AtomicU64>,
    weak: slint::Weak<MainWindow>,
) {
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            if job.r#gen != r#gen.load(Ordering::Relaxed) {
                continue;
            }
            // One walk yields both metrics; each is `None` when its depth is 0.
            let (mtime, size) =
                rfs::recursive_folder_stats(&job.path, job.mtime_depth, job.size_depth);
            if mtime.is_none() && size.is_none() {
                continue;
            }
            let path_str = job.path.to_string_lossy().to_string();
            // An empty string means "not computed" for that metric.
            let mtime_str = mtime.map(|m| m.to_string()).unwrap_or_default();
            let size_str = size.map(|s| s.to_string()).unwrap_or_default();
            let panel = job.panel as i32;
            let row = job.row as i32;
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<crate::PanelsApi>().invoke_folder_stats_ready(
                        panel,
                        row,
                        path_str.into(),
                        mtime_str.into(),
                        size_str.into(),
                    );
                }
            });
        }
    });
}

/// If either "folder date" or "folder size" is enabled (its depth > 0): applies
/// the cached recursive mtime and/or size to each panel's FOLDER rows and queues
/// the missing computations — a single shared walk covers both. To be called
/// after any (re)population of rows. Increments the generation.
pub(super) fn request_folder_stats(state: &AppState) {
    let (mtime_depth, size_depth) = {
        let c = state.config.borrow();
        (
            c.recursive_mtime_depth.max(0) as u32,
            c.recursive_size_depth.max(0) as u32,
        )
    };
    if mtime_depth == 0 && size_depth == 0 {
        return;
    }
    let lang = state.config.borrow().language;
    let now = now_unix();
    let r#gen = state.rmtime_gen.fetch_add(1, Ordering::Relaxed) + 1;
    let panels = state.panels.borrow();
    for (panel_idx, panel) in panels.iter().enumerate() {
        let dir = panel.tabs.tabs[panel.tabs.active].current_path.clone();
        // NETWORK folder: a recursive walk through the share would burst a stat
        // per subfolder — Explorer doesn't either. Keep folders' own mtime and
        // no recursive size.
        if favnyr_core::places::is_network_path(&dir) {
            continue;
        }
        let model = &panel.rows_model;
        for i in 0..model.row_count() {
            let Some(mut row) = model.row_data(i) else {
                continue;
            };
            if !row.is_dir {
                continue; // only folders have recursive stats
            }
            let Some(full) = row_path(&row) else {
                continue; // section header
            };
            let key = full.to_string_lossy().to_string();
            let m_cached = (mtime_depth > 0)
                .then(|| state.rmtime_cache.borrow().get(&key).copied())
                .flatten();
            let s_cached = (size_depth > 0)
                .then(|| state.size_cache.borrow().get(&key).copied())
                .flatten();
            if m_cached.is_some() || s_cached.is_some() {
                if let Some(m) = m_cached {
                    apply_rmtime_to_row(&mut row, m, now, lang);
                }
                if let Some(s) = s_cached {
                    row.size = rfs::format_size(s, i18n::size_units(lang)).into();
                }
                model.set_row_data(i, row);
            }
            // Queue only what is BOTH enabled and not yet cached, so a job never
            // recomputes a metric already known.
            let need_mtime = mtime_depth > 0 && m_cached.is_none();
            let need_size = size_depth > 0 && s_cached.is_none();
            if need_mtime || need_size {
                let _ = state.rmtime_tx.send(RMtimeJob {
                    path: full,
                    mtime_depth: if need_mtime { mtime_depth } else { 0 },
                    size_depth: if need_size { size_depth } else { 0 },
                    r#gen,
                    panel: panel_idx,
                    row: i,
                });
            }
        }
    }
}

// Image metadata: resolution / depth ----------

pub(super) struct ImgMetaJob {
    path: PathBuf,
    r#gen: u64,
    panel: usize,
    row: usize,
    /// Language of the REQUEST. The worker outlives any language change, so it
    /// cannot read the current one; carrying it per job keeps each result in
    /// the language that was active when the row asked for it.
    lang: Lang,
}

/// Formats metadata into a displayable (resolution, depth). Resolution
/// "LxH"; color depth "N-bit" (+ "+ alpha" if present). The
/// alpha channel's bits are not included in `bits`: RGBA8 is therefore displayed as
/// "24-bit + alpha", not the misleading "32-bit +A".
pub(super) fn format_img_meta(
    w: u32,
    h: u32,
    bits: u16,
    alpha: bool,
    lang: Lang,
) -> (String, String) {
    let resolution = format!("{w}x{h}");
    let key = if alpha {
        "img_depth_bits_alpha"
    } else {
        "img_depth_bits"
    };
    let depth = i18n::tr(lang, key).replace("{bits}", &bits.to_string());
    (resolution, depth)
}

/// Unix mtime (seconds) of a file, or `None` if unreadable. Used to remember
/// an image's VERSION alongside its metadata (resolution/depth) and to detect
/// a later modification (new mtime → stale cache → recompute).
pub(super) fn file_mtime_unix(path: &Path) -> Option<i64> {
    let mt = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(match mt.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        // File dated before 1970 (rare): negative mtime.
        Err(e) => -(e.duration().as_secs() as i64),
    })
}

/// Background worker: reads the image header (dimensions + color type) off the
/// UI thread and pushes `(path, resolution, depth, mtime)` via `imgmeta-ready`. The
/// mtime is read as close as possible to the header → it matches the measured version.
/// Ignores stale jobs (folder left).
pub(super) fn spawn_imgmeta_worker(
    rx: mpsc::Receiver<ImgMetaJob>,
    r#gen: Arc<AtomicU64>,
    weak: slint::Weak<MainWindow>,
) {
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            if job.r#gen != r#gen.load(Ordering::Relaxed) {
                continue;
            }
            let Some((w, h, bits, alpha)) = thumbnail::image_meta(&job.path) else {
                continue;
            };
            let (resolution, depth) = format_img_meta(w, h, bits, alpha, job.lang);
            let mtime = file_mtime_unix(&job.path).unwrap_or(0).to_string();
            let path_str = job.path.to_string_lossy().to_string();
            let panel = job.panel as i32;
            let row = job.row as i32;
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = weak.upgrade() {
                    win.global::<crate::PanelsApi>().invoke_imgmeta_ready(
                        panel,
                        row,
                        path_str.into(),
                        resolution.into(),
                        depth.into(),
                        mtime.into(),
                    );
                }
            });
        }
    });
}

/// `true` if a panel displays the `resolution` or `depth` column.
pub(super) fn imgmeta_columns_active(state: &AppState) -> bool {
    state.panels.borrow().iter().any(|p| {
        p.columns
            .iter()
            .any(|c| c.visible && (c.id == "resolution" || c.id == "depth"))
    })
}

/// If a resolution/depth column is visible: applies the cached metadata
/// to IMAGE rows and queues the missing ones. To be called after any
/// (re)population of rows or column change. Increments the generation.
pub(super) fn request_imgmeta(state: &AppState) {
    if !imgmeta_columns_active(state) {
        return; // no relevant column → no cost
    }
    let lang = state.snapshot_config().language;
    let r#gen = state.imgmeta_gen.fetch_add(1, Ordering::Relaxed) + 1;
    let panels = state.panels.borrow();
    for (panel_idx, panel) in panels.iter().enumerate() {
        let shows = panel
            .columns
            .iter()
            .any(|c| c.visible && (c.id == "resolution" || c.id == "depth"));
        if !shows {
            continue;
        }
        let model = &panel.rows_model;
        for i in 0..model.row_count() {
            let Some(mut row) = model.row_data(i) else {
                continue;
            };
            if row.kind != 6 || !row.resolution.is_empty() {
                continue; // images only, and not already filled in
            }
            let Some(full) = row_path(&row) else {
                continue; // section header
            };
            let key = full.to_string_lossy().to_string();
            let cached = state.imgmeta_cache.borrow().get(&key).cloned();
            // The cache is only valid if the mtime STILL matches the file
            // (the stat only happens on a hit). A retouched image has a
            // different mtime → stale → recompute → columns up to date without F5.
            let fresh =
                matches!(&cached, Some((mtime, _, _)) if file_mtime_unix(&full) == Some(*mtime));
            if let Some((_, res, depth)) = cached.filter(|_| fresh) {
                row.resolution = res.into();
                row.depth = depth.into();
                model.set_row_data(i, row);
            } else {
                let _ = state.imgmeta_tx.send(ImgMetaJob {
                    path: full,
                    r#gen,
                    panel: panel_idx,
                    row: i,
                    lang,
                });
            }
        }
    }
}
