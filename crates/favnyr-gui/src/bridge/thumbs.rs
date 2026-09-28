use super::*;

mod scheduler;

pub(super) use scheduler::*;

// Previews / thumbnails ----------

/// Invalidates only the content paths touched by an operation or watcher
/// event. Untouched thumbnails stay hot in the bounded LRU. The scheduler is
/// invalidated at the same time so an older asynchronous decode cannot restore
/// a stale texture after a path has been renamed or replaced.
pub(super) fn invalidate_thumbnail_paths(state: &AppState, paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let unique: HashSet<PathBuf> = paths.iter().cloned().collect();
    {
        let mut cache = state.thumb_cache.borrow_mut();
        for path in &unique {
            cache.remove_path(path);
        }
    }
    let paths: Vec<PathBuf> = unique.into_iter().collect();
    state.thumb_scheduler.invalidate_paths(&paths);
}

pub(super) fn drain_thumbnail_invalidations(state: &AppState) -> bool {
    let paths: Vec<PathBuf> = state
        .thumb_invalidations
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .drain()
        .collect();
    let changed = !paths.is_empty();
    invalidate_thumbnail_paths(state, &paths);
    changed
}

pub(super) fn active_thumbnail_paths(state: &AppState) -> Vec<PathBuf> {
    let active = *state.active_panel.borrow();
    let panels = state.panels.borrow();
    let Some(panel) = panels.get(active) else {
        return Vec::new();
    };
    (0..panel.rows_model.row_count())
        .filter_map(|index| panel.rows_model.row_data(index))
        // `.lnk` rows can preview an image/audio/video/PDF target even though the
        // shortcut itself is not classified as preview-capable.
        .filter(|row| row.preview_capable || row.ext.eq_ignore_ascii_case("lnk"))
        .filter_map(|row| row_path(&row))
        .collect()
}

/// Builds a `slint::Image` from a decoded thumbnail (RGBA8).
pub(super) fn image_from_thumb(t: &Thumbnail) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(t.width, t.height);
    buf.make_mut_bytes().copy_from_slice(&t.rgba);
    Image::from_rgba8(buf)
}

/// Selects the thumbnail source.
///
/// On **Windows** the **system shell thumbnail API** is the single source of
/// truth (`winthumb::shell_thumbnail`, `IShellItemImageFactory`): Explorer's
/// own providers — and therefore the system thumbnail cache — cover every type
/// the OS knows how to preview (photos, video, PDF, Office documents, e-books,
/// fonts…), including `.lnk` shortcuts, which the shell resolves itself. Favnyr
/// therefore reimplements none of them. Its own decoders
/// (`thumbnail::generate`) are consulted only when the shell produced no
/// thumbnail — a type the shell doesn't cover; PDF keeps the built-in WinRT
/// engine as a last resort, since the core path drives poppler, which is
/// Linux-only. `SIIGBF_THUMBNAILONLY` guarantees a failed lookup yields the
/// plain type icon instead of a fake thumbnail.
///
/// On **Linux**, everything goes through `thumbnail::generate` (images and
/// MP3/FLAC covers in-process; video/PDF via their best-effort CLIs).
pub(super) fn generate_thumb(path: &Path, kind: FileKind, max_px: u32) -> Option<Thumbnail> {
    #[cfg(windows)]
    {
        if let Some(thumb) = crate::winthumb::shell_thumbnail(path, max_px) {
            return Some(thumb);
        }
        if kind == FileKind::Document {
            return crate::winthumb::pdf_thumbnail(path, max_px);
        }
        thumbnail::generate(path, kind, max_px)
    }
    #[cfg(not(windows))]
    {
        thumbnail::generate(path, kind, max_px)
    }
}

/// Size of the thumbnail worker pool. The dominant cost (image decoding, or
/// launching ffmpeg/shell) is CPU-bound and parallelizable per file. Capped at
/// **4**: beyond that, applying the textures (single UI thread), memory
/// bandwidth, and I/O cap the gain, a visible folder shows only
/// ~15 rows, and N simultaneous full-resolution decodes bound the peak
/// memory (~4x the peak of a single image). We also leave one core for the UI thread on
/// small machines (`- 1`). Safe fallback to 1 if introspection fails.
pub(super) fn thumb_worker_count() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .saturating_sub(1)
        .clamp(1, 4)
}

/// Background worker: picks the best job at the moment it becomes available,
/// then waits for it to be applied by the event loop before picking the next one.
/// This way, a scroll that happens during decoding immediately influences that choice.
/// Several instances run in parallel (see `thumb_worker_count`): each
/// takes a DISTINCT job (the scheduler removes the path from `pending` under
/// the lock) and waits only for ITS OWN completion — the others keep decoding.
pub(super) fn spawn_thumb_worker(scheduler: Arc<ThumbScheduler>, weak: slint::Weak<MainWindow>) {
    // 256 px: covers the tallest row height at max zoom (≈220 px) without blur.
    const MAX_PX: u32 = 256;
    std::thread::spawn(move || {
        loop {
            let work = scheduler.take_next();
            let serial = work.serial;
            let job = work.job;
            let wait_path = job.path.clone();
            let path_str = job.path.to_string_lossy().to_string();
            let weak_for_ui = weak.clone();
            let scheduler_for_ui = scheduler.clone();

            if job.svg {
                let ui_path = job.path;
                let posted = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak_for_ui.upgrade() {
                        match Image::load_from_path(&ui_path) {
                            Ok(img) => w.invoke_thumb_ready(path_str.into(), serial, img),
                            Err(_) => scheduler_for_ui.complete(&ui_path, serial),
                        }
                    } else {
                        scheduler_for_ui.complete(&ui_path, serial);
                    }
                });
                if posted.is_err() {
                    scheduler.complete(&wait_path, serial);
                } else {
                    scheduler.wait_until_complete(&wait_path, serial);
                }
                continue;
            }

            let Some(thumb) = generate_thumb(&job.path, job.kind, MAX_PX) else {
                scheduler.complete(&wait_path, serial);
                continue;
            };
            let completion_path = wait_path.clone();
            let posted = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak_for_ui.upgrade() {
                    w.invoke_thumb_ready(path_str.into(), serial, image_from_thumb(&thumb));
                } else {
                    scheduler_for_ui.complete(&completion_path, serial);
                }
            });
            if posted.is_err() {
                scheduler.complete(&wait_path, serial);
            } else {
                scheduler.wait_until_complete(&wait_path, serial);
            }
        }
    });
}

/// Maps a listed row (its `kind` wire code and extension) to the thumbnail
/// request it raises, plus whether it is an SVG (rendered by Slint, not decoded).
///
/// - **Windows**: EVERY non-folder entry is offered a preview. The system shell
///   thumbnail API (`winthumb::shell_thumbnail`) covers far more types than
///   Favnyr decodes itself — Office documents, e-books, fonts, archives, PDFs,
///   media — so no whitelist is kept here: the OS decides. When it has no
///   thumbnail for the file, `SIIGBF_THUMBNAILONLY` returns nothing and the row
///   keeps its type icon.
/// - **Other platforms**: no universal thumbnail service exists, so only the
///   types Favnyr can actually render keep a preview (image, video, MP3/FLAC
///   cover, PDF). Folders never request one.
pub(super) fn thumbnail_kind_for_row(kind: i32, ext: &str) -> Option<(FileKind, bool)> {
    let file_kind = FileKind::from_code(kind)?;
    let svg = file_kind == FileKind::Image && ext.eq_ignore_ascii_case("svg");
    #[cfg(windows)]
    {
        (file_kind != FileKind::Folder).then_some((file_kind, svg))
    }
    #[cfg(not(windows))]
    {
        let previewable = match file_kind {
            FileKind::Image | FileKind::Video => true,
            FileKind::Audio => ext.eq_ignore_ascii_case("mp3") || ext.eq_ignore_ascii_case("flac"),
            FileKind::Document => ext.eq_ignore_ascii_case("pdf"),
            _ => false,
        };
        previewable.then_some((file_kind, svg))
    }
}

/// Merges a local request before touching the shared scheduler. The same
/// image can appear in several panels; all its locations are
/// kept, but the decoding stays unique.
pub(super) fn push_thumbnail_request(
    requests: &mut HashMap<PathBuf, ThumbRequest>,
    path: PathBuf,
    kind: FileKind,
    svg: bool,
    location: ThumbLocation,
) {
    if let Some(existing) = requests.get_mut(&path) {
        if !existing.locations.contains(&location) {
            existing.locations.push(location);
        }
    } else {
        requests.insert(
            path.clone(),
            ThumbRequest {
                job: ThumbJob { path, kind, svg },
                locations: vec![location],
            },
        );
    }
}

pub(super) fn request_thumbnails(state: &AppState) {
    let mut requests: HashMap<PathBuf, ThumbRequest> = HashMap::new();
    let panels = state.panels.borrow();
    for (panel_idx, panel) in panels.iter().enumerate() {
        let tab = &panel.tabs.tabs[panel.tabs.active];
        if !tab.mode.thumbnails() {
            continue;
        }
        let model = &panel.rows_model;
        for i in 0..model.row_count() {
            let Some(mut row) = model.row_data(i) else {
                continue;
            };
            if row.thumbnail.size().width > 0 {
                if row.rendered {
                    continue; // texture already attached to a visible row
                }
                // A rebuilt geometry may have briefly kept a
                // an off-screen texture. We detach it, then actually check
                // the LRU instead of assuming it still lives there.
                row.thumbnail = Image::default();
                model.set_row_data(i, row.clone());
            }
            let Some(full) = row_path(&row) else {
                continue; // section header: never a preview target
            };
            let key = full.to_string_lossy().to_string();

            // Shared source of truth with the row geometry: only the entries
            // this returns can receive a content texture. On Windows it accepts
            // every non-folder file (the shell decides), so `.lnk` shortcuts go
            // the same way as everything else — the shell resolves the link.
            let Some((kind, svg)) = thumbnail_kind_for_row(row.kind, row.ext.as_str()) else {
                continue;
            };
            if row.rendered {
                if let Some(img) = state.thumb_cache.borrow_mut().get(&key) {
                    row.thumbnail = img;
                    model.set_row_data(i, row);
                    continue;
                }
            } else if state.thumb_cache.borrow().contains(&key) {
                // Don't clone/promote every off-screen image: they
                // remain available, but LRU recency reflects only the actually
                // visible usages. Above all, no unnecessary re-decoding.
                continue;
            }
            push_thumbnail_request(
                &mut requests,
                full,
                kind,
                svg,
                ThumbLocation {
                    panel: panel_idx,
                    row: i,
                    row_count: model.row_count(),
                },
            );
        }
    }
    drop(panels);
    state
        .thumb_scheduler
        .replace_pending(requests.into_values().collect());
}

/// Adjusts a panel's delegate window. The filtered model then automatically
/// relays selection, cut, metadata, and thumbnails of the admitted rows.
/// Returns the STRICTLY visible range for the worker's priority.
pub(super) fn update_panel_render_window(
    state: &AppState,
    panel_idx: usize,
    top: f32,
    height: f32,
) -> (i32, i32) {
    let top = top.max(0.0);
    let height = height.max(0.0);
    let Some((model, preview, old_first, old_end, new_first, new_end, visible)) =
        state.panels.try_borrow().ok().and_then(|panels| {
            let panel = panels.get(panel_idx)?;
            panel.viewport_top.set(top);
            if height > 0.0 {
                panel.viewport_height.set(height);
            }
            let effective_height = panel.viewport_height.get().max(1.0);
            let low = (top - effective_height).max(0.0);
            let high = top + effective_height * 2.0;
            let (new_first, new_end) = row_range_for_content_span(&*panel.rows_model, low, high);
            let visible = if height > 0.0 {
                row_range_for_content_span(&*panel.rows_model, top, top + height)
            } else {
                (0, 0)
            };
            let old_first = panel.rendered_first.replace(new_first);
            let old_end = panel.rendered_end.replace(new_end);
            Some((
                panel.rows_model.clone(),
                panel.tabs.tabs[panel.tabs.active].mode.thumbnails(),
                old_first,
                old_end,
                new_first,
                new_end,
                visible,
            ))
        })
    else {
        return (-1, -1);
    };

    // The symmetric difference is enough to materialize/dematerialize the
    // delegates. We ALWAYS add to it the strictly visible range: after a
    // zoom relayout, `replace_rows` has already pre-marked the new window and
    // old == new on the next callback. Without this bounded reconciliation, a
    // missing texture (LRU evicted, request cancelled while switching to list mode)
    // could stay visible without being either pending or in-flight.
    let mut inspected = if (old_first, old_end) != (new_first, new_end) {
        render_window_changed_indices(old_first, old_end, new_first, new_end)
    } else {
        Vec::new()
    };
    inspected.extend(visible.0..visible.1);
    inspected.sort_unstable();
    inspected.dedup();

    let row_count = model.row_count();
    let mut requests: HashMap<PathBuf, ThumbRequest> = HashMap::new();
    for index in inspected {
        if index >= row_count {
            continue;
        }
        let should_render = index >= new_first && index < new_end;
        let Some(mut row) = model.row_data(index) else {
            continue;
        };
        let mut row_changed = false;
        if row.rendered != should_render {
            row.rendered = should_render;
            row_changed = true;
        }

        if !should_render {
            // The texture stays in the LRU, not in the thousands of off-screen
            // rows. This makes the cache's memory bound actually effective.
            if row.thumbnail.size().width > 0 {
                row.thumbnail = Image::default();
                row_changed = true;
            }
        } else if preview && row.preview_capable && row.thumbnail.size().width == 0 {
            let Some(full) = row_path(&row) else {
                continue; // section header
            };
            let key = full.to_string_lossy().to_string();
            if let Some(img) = state.thumb_cache.borrow_mut().get(&key) {
                row.thumbnail = img;
                row_changed = true;
            } else if let Some((kind, svg)) = thumbnail_kind_for_row(row.kind, row.ext.as_str()) {
                push_thumbnail_request(
                    &mut requests,
                    full,
                    kind,
                    svg,
                    ThumbLocation {
                        panel: panel_idx,
                        row: index,
                        row_count,
                    },
                );
            }
        }
        if row_changed {
            model.set_row_data(index, row);
        }
    }
    if !requests.is_empty() {
        state
            .thumb_scheduler
            .merge_pending(requests.into_values().collect());
    }

    if visible.0 < visible.1 {
        (visible.0 as i32, visible.1 as i32 - 1)
    } else {
        (-1, -1)
    }
}
