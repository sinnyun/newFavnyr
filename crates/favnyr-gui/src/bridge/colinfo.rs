use super::*;

/// Width (px) of a column by its id, or the default width.
pub(super) fn col_width(cols: &[ColumnSpec], id: &str) -> f32 {
    cols.iter()
        .find(|c| c.id == id)
        .map(|c| c.width)
        .unwrap_or_else(|| columns::default_width(id))
}

/// Applies a width to column `id` (if present).
pub(super) fn set_col_width(cols: &mut [ColumnSpec], id: &str, w: f32) {
    if let Some(c) = cols.iter_mut().find(|c| c.id == id) {
        c.width = w;
    }
}

/// Reorders column `id` based on a **horizontal move** `delta_px`
/// (signed). Computes the new slot from the widths of the
/// visible columns (the `name` anchor stays first). Hidden columns are
/// kept (pushed to the end of the list).
pub(super) fn reorder_column_by_delta(cols: &mut Vec<ColumnSpec>, id: &str, delta_px: f32) {
    if id == "name" {
        return;
    }
    // Visible columns (id, effective width) in order.
    let vis: Vec<(String, f32)> = cols
        .iter()
        .filter(|c| c.visible)
        .map(|c| {
            let w = if c.width > 0.0 {
                c.width
            } else {
                columns::default_width(&c.id)
            };
            (c.id.clone(), w)
        })
        .collect();
    let Some(cur) = vis.iter().position(|(i, _)| i == id) else {
        return;
    };
    // Includes the inter-column gap (= `Tokens.col-gap`) to match the
    // offsets/preview on the GUI side.
    const COL_GAP: f32 = 8.0;
    // Current center of the dragged column + movement = target center.
    let left_before: f32 = vis[..cur].iter().map(|(_, w)| w + COL_GAP).sum();
    let new_center = left_before + vis[cur].1 / 2.0 + delta_px;

    // Target "gap" in the STATIC layout (columns in place, like the
    // preview): the 1st gap whose column midpoint is past the target center.
    let mut gap = vis.len();
    let mut x = 0.0_f32;
    for (k, (_, w)) in vis.iter().enumerate() {
        if new_center < x + w / 2.0 {
            gap = k;
            break;
        }
        x += w + COL_GAP;
    }
    let gap = gap.max(1); // ≥ 1 → always after the `name` anchor

    // Removes the dragged column then adjusts the gap, whose index shifts when
    // the source was before the target.
    let mut order: Vec<String> = vis.iter().map(|(i, _)| i.clone()).collect();
    order.remove(cur);
    let t = if gap > cur { gap - 1 } else { gap };
    let t = t.clamp(1, order.len());
    order.insert(t, id.to_string());

    // Rebuilds: visible columns in the new order, then the hidden ones.
    let mut new: Vec<ColumnSpec> = Vec::with_capacity(cols.len());
    for oid in &order {
        if let Some(spec) = cols.iter().find(|c| &c.id == oid) {
            new.push(spec.clone());
        }
    }
    for c in cols.iter().filter(|c| !c.visible) {
        new.push(c.clone());
    }
    *cols = columns::sanitize(new);
}

/// Pushes the DEFAULT columns (config) to the Settings panel.
pub(super) fn push_settings_columns(window: &MainWindow, lang: Lang, cols: &[ColumnSpec]) {
    let strings = i18n::strings_for(lang);
    let infos: Vec<ColumnInfo> = columns::sanitize(cols.to_vec())
        .iter()
        .map(|c| column_info_explicit(&strings, c, 0.0))
        .collect();
    window
        .global::<crate::SettingsApi>()
        .set_settings_columns(ModelRc::new(VecModel::from(infos)));
}

/// [`column_info`] with the wording used wherever a column is CHOSEN from a
/// list — the Settings checkboxes and the header's context menu. Both name the
/// columns reserved for images outright ("Resolution (image)"), because a list
/// of column names read out of context gives no clue what "Depth" measures.
///
/// The header itself keeps the short form from [`column_info`]: it sits in a
/// resizable width the user may have narrowed, and there the surrounding
/// values say what the column holds.
pub(super) fn column_info_explicit(
    strings: &crate::Strings,
    c: &ColumnSpec,
    offset: f32,
) -> ColumnInfo {
    let mut info = column_info(strings, c, offset);
    // The i18n keys keep their `settings_` prefix: the wording is the same in
    // both lists, and renaming them across five catalogues would buy nothing.
    match c.id.as_str() {
        "resolution" => info.label = strings.settings_col_resolution.clone(),
        "depth" => info.label = strings.settings_col_depth.clone(),
        _ => {}
    }
    info
}

/// Builds a `ColumnInfo` (id + i18n label + visible + offset) for the GUI.
pub(super) fn column_info(strings: &crate::Strings, c: &ColumnSpec, offset: f32) -> ColumnInfo {
    let label = match c.id.as_str() {
        "name" => strings.col_name.clone(),
        "path" => strings.col_path.clone(),
        "size" => strings.col_size.clone(),
        "modified" => strings.col_modified.clone(),
        "age" => strings.col_age.clone(),
        "ext" => strings.col_ext.clone(),
        "resolution" => strings.col_resolution.clone(),
        "depth" => strings.col_depth.clone(),
        other => other.into(),
    };
    ColumnInfo {
        id: c.id.clone().into(),
        label,
        visible: c.visible,
        offset,
    }
}
