use super::*;

pub(in crate::bridge) fn entry_to_row(
    e: &Entry,
    parent_display: &str,
    ctx: &RowContext,
) -> FileRow {
    let RowContext {
        lang,
        now_unix,
        style,
        annotations,
        ..
    } = *ctx;
    let big_icon = style.mode.thumbnails();
    let compact_icon_rows = style.compact_icon_rows;
    let size = if e.is_dir {
        "—".to_string()
    } else {
        e.size_bytes
            .map(|b| rfs::format_size(b, i18n::size_units(lang)))
            .unwrap_or_else(|| "—".to_string())
    };
    let modified = e
        .mtime_unix
        .map(|m| rfs::format_mtime(m, mtime_offset()))
        .unwrap_or_else(|| "—".to_string());
    // "age" column: compact text + warm-to-cold color bucket.
    let (age, age_bucket) = match e.mtime_unix {
        Some(m) => (
            rfs::format_age(m, now_unix, i18n::age_units(lang)),
            rfs::age_bucket(m, now_unix),
        ),
        None => ("—".to_string(), -1),
    };
    // Splits name/extension for coloring (files only: a
    // folder named "x.y" has no "extension" to highlight).
    // Name DISPLAY (stem + colored extension): `split_name` leaves
    // dotfiles whole. TYPING (the "ext" column, icon, sort, filter): `ext_of`
    // treats a dotfile's suffix as an extension (`.gitignore` → gitignore).
    let (name_base, name_ext) = if e.is_dir {
        (e.name.clone(), String::new())
    } else {
        let (stem, ext) = ops::split_name(&e.name);
        if ext.is_empty() {
            (e.name.clone(), String::new())
        } else {
            (stem, format!(".{ext}"))
        }
    };
    let ext_raw = if e.is_dir {
        String::new()
    } else {
        ops::ext_of(&e.name)
    };
    #[cfg(windows)]
    let drop_runnable = !e.is_dir && matches!(ext_raw.as_str(), "exe" | "com" | "cmd" | "bat");
    // The executable bit alone gives false positives (FAT/NTFS/exFAT mounts at
    // 0777, files copied from Windows): an image/video/document… is NOT
    // a program, so no "launch with these files" (see can_be_program).
    #[cfg(not(windows))]
    let drop_runnable = !e.is_dir && e.executable && e.kind.can_be_program();
    let preview_capable = thumbnail_kind_for_row(e.kind.as_i32(), &ext_raw).is_some();
    // Actual OS app icon for this extension — empty for a folder
    // or if unavailable (falls back to the view's type icon). Cached.
    // Resolution suited to the mode: 256 px in previews (enlarged), 32 px in list.
    // For `.lnk` shortcuts: we resolve the target → target app icon
    // (file) OR "folder + arrow" marker (folder) via `link_folder`.
    let use_large_icon = big_icon && (!compact_icon_rows || preview_capable);
    let (app_icon, link_folder) =
        row_app_icon(parent_display, &e.name, &ext_raw, e.is_dir, use_large_icon);
    FileRow {
        name: e.name.clone().into(),
        name_base: name_base.into(),
        name_ext: name_ext.into(),
        ext: ext_raw.into(),
        path: parent_display.to_string().into(),
        size: size.into(),
        modified: modified.into(),
        is_dir: e.is_dir,
        is_symlink: e.is_symlink,
        // Only consulted for folders, so a stray slot on a file costs nothing.
        folder_slot: i32::from(annotations.color_of(&e.path)),
        comment: annotations.note_of(&e.path).into(),
        drop_runnable,
        kind: e.kind.as_i32(),
        selected: false,
        cut: false,
        hidden: e.hidden,
        age: age.into(),
        age_bucket,
        // Image metadata: empty here; filled in place by the
        // imgmeta worker when the resolution/depth column is visible.
        resolution: SharedString::default(),
        depth: SharedString::default(),
        // Thumbnail: empty here; filled in place by the worker in
        // preview mode, or immediately from the cache in update_panels_ui.
        thumbnail: slint::Image::default(),
        app_icon,
        link_folder,
        preview_capable,
        visual_x: 0.0,
        visual_w: 0.0,
        visual_y: 0.0,
        visual_h: COMPACT_ICON_ROW_HEIGHT,
        model_index: 0,
        rendered: false,
        role: ROW_ROLE_ENTRY,
        section: SharedString::default(),
        section_label: SharedString::default(),
        section_count_text: SharedString::default(),
        section_pending: false,
    }
}
