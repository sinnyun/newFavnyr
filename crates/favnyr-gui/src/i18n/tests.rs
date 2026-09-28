use super::*;

#[test]
fn every_language_loads_and_has_app_title() {
    for &lang in Lang::all() {
        let s = strings_for(lang);
        assert_eq!(s.app_title.as_str(), "Favnyr", "{lang:?}");
    }
}

#[test]
fn french_overrides_english() {
    assert_eq!(tr(Lang::Fr, "settings_language"), "Langue");
    assert_eq!(tr(Lang::En, "settings_language"), "Language");
}

#[test]
fn missing_key_falls_back_to_english_then_key() {
    // Key present everywhere: the language's value.
    assert_eq!(shortcut_action_name(Lang::Fr, "copy"), "Copier");
    // Nonexistent key → returned as-is (no panic/empty string).
    assert_eq!(tr(Lang::Fr, "zzz_missing_key"), "zzz_missing_key");
}

#[test]
fn conflict_template_interpolates_name() {
    let msg = shortcut_conflict_message(Lang::En, "Copy");
    assert!(msg.contains("Copy") && !msg.contains("{name}"));
}

#[test]
fn item_in_use_message_is_translated_and_interpolated() {
    for &lang in Lang::all() {
        let processes = vec!["editor.exe".to_owned()];
        let known = item_in_use(lang, "my_file.txt", &processes, true);
        assert!(known.contains("my_file.txt") && known.contains("editor.exe"));
        assert!(known.contains('…'));
        assert!(!known.contains("{item}") && !known.contains("{process}"));

        let unknown = item_in_use(lang, "my_file.txt", &[], false);
        assert!(unknown.contains("my_file.txt"));
        assert!(!unknown.contains("{item}") && !unknown.contains("{process}"));
    }
}

#[test]
fn move_source_kept_is_translated_and_interpolated() {
    for &lang in Lang::all() {
        // Attributed holder: the process list reaches the sentence.
        let named = move_source_kept(lang, "report.odt", &process_list(lang, &["editor"], false));
        assert!(named.contains("report.odt") && named.contains("editor"));
        assert!(!named.contains("{item}") && !named.contains("{reason}"));

        // No attributable holder: the system error takes its place.
        let raw = move_source_kept(lang, "report.odt", "permission denied");
        assert!(raw.contains("permission denied"));
        assert!(!raw.contains("{reason}"));
    }
    // Nothing diagnosed at all → empty fragment, which is what makes the
    // caller fall back to the system error instead of an empty reason.
    assert!(process_list::<&str>(Lang::En, &[], false).is_empty());
    assert_eq!(process_list(Lang::En, &["a", "b"], true), "a, b, …");
}

#[test]
fn rename_failure_is_translated_and_interpolated() {
    for &lang in Lang::all() {
        let message = rename_failed(lang, "photo.png", "access denied");
        assert!(message.contains("photo.png") && message.contains("access denied"));
        assert!(!message.contains("{item}") && !message.contains("{reason}"));
    }
}

#[test]
fn all_strings_fields_resolve_nonempty() {
    // No `Strings` field should fall back to the raw key (= a key
    // missing from en.toml). Covers all 162 fields via one language.
    let c = catalog(Lang::En);
    for (k, v) in c {
        assert!(!v.is_empty(), "empty key: {k}");
    }
}

/// Every key requested by `strings_for` via `g("…")` must exist at the
/// root level of the English catalog. A key placed under a `[…]` section
/// becomes `section.key` and can't be resolved by `g`. This test re-reads
/// the source of `strings_for`.
#[test]
fn every_g_key_present_in_en() {
    let src = include_str!("strings.rs");
    let c = catalog(Lang::En);
    let mut checked = 0usize;
    for line in src.lines() {
        // Ignore COMMENTS (including this docstring, which contains `g("…")`).
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut rest = line;
        while let Some(pos) = rest.find("g(\"") {
            let after = &rest[pos + 3..];
            let Some(end) = after.find('"') else { break };
            let key = &after[..end];
            assert!(
                c.contains_key(key),
                "key g(\"{key}\") missing from the top level of en.toml (misplaced under a [section]?)"
            );
            checked += 1;
            rest = &after[end + 1..];
        }
    }
    // Guard: the scan did find keys (otherwise the test proves nothing).
    assert!(
        checked > 100,
        "scan g(\"…\") found abnormally few keys: {checked}"
    );
}

/// `{name}`-style tokens of a message, sorted and deduplicated.
fn placeholders(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let token = &after[..close];
        if !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.push(token.to_string());
        }
        rest = &after[close + 1..];
    }
    out.sort();
    out.dedup();
    out
}

/// Every bundled catalog must define every English key, with the same
/// placeholders. A missing key silently falls back to English — exactly
/// the half-translated interface this test exists to prevent — and a
/// dropped `{token}` turns into a message that lies about the item.
#[test]
fn every_bundled_catalog_covers_english_and_keeps_its_placeholders() {
    let mut en = HashMap::new();
    overlay(EMBED_EN, &mut en);
    assert!(en.len() > 400, "en.toml looks truncated: {} keys", en.len());

    for &lang in Lang::all() {
        let mut map = HashMap::new();
        overlay(embedded(lang), &mut map);
        for (key, english) in &en {
            let Some(value) = map.get(key) else {
                panic!(
                    "{}: \"{key}\" is missing from the bundled catalog",
                    lang.code()
                );
            };
            assert_eq!(
                placeholders(value),
                placeholders(english),
                "{}: \"{key}\" no longer carries the same placeholders",
                lang.code()
            );
        }
        let mut extra: Vec<&String> = map.keys().filter(|k| !en.contains_key(*k)).collect();
        extra.sort();
        assert!(
            extra.is_empty(),
            "{}: keys that exist only in that catalog: {extra:?}",
            lang.code()
        );
    }
}

/// The Chinese catalog must actually be Chinese. Without this, a file
/// copied from `en.toml` would pass the coverage test above while leaving
/// the whole interface in English. These keys legitimately stay ASCII: the
/// brand, the size units, the decimal mark, separator glyphs, modifier-key
/// names and the two example placeholders.
#[test]
fn chinese_catalog_is_translated() {
    const NEUTRAL: &[&str] = &[
        "app_title",
        "unit_byte",
        "unit_kb",
        "unit_mb",
        "unit_gb",
        "unit_tb",
        "decimal_separator",
        "ext_filter_placeholder",
        "separator_dot",
        "list_separator",
        "list_ellipsis",
        "key_ctrl",
        "key_alt",
        "key_shift",
        "key_delete",
        "key_pageup",
        "key_pagedown",
        "ow_default_ext_placeholder",
        "ow_used_ext_placeholder",
    ];
    let is_han = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c);

    let mut zh = HashMap::new();
    overlay(EMBED_ZH, &mut zh);
    for (key, value) in &zh {
        if NEUTRAL.contains(&key.as_str()) {
            continue;
        }
        assert!(
            value.chars().any(is_han),
            "zh: \"{key}\" was left untranslated: {value}"
        );
    }
}

/// Chinese wording reaches the parts of the interface driven by code
/// rather than by a `Strings` field.
#[test]
fn chinese_reaches_the_computed_labels() {
    assert_eq!(tr(Lang::Zh, "settings_language"), "语言");
    assert_eq!(tr(Lang::Zh, "ctx_copy"), "复制");
    assert_eq!(shortcut_action_name(Lang::Zh, "copy"), "复制");
    assert_eq!(shortcut_group_name(Lang::Zh, "navigation"), "导航");
    assert_eq!(theme_labels(Lang::Zh)[2], "深色");
    assert_eq!(tabbar_labels(Lang::Zh)[0], "标签栏在顶部");
    assert_eq!(
        footer_text(Lang::Zh, 12, 2, 3),
        "12 个项目  ·  3 个隐藏项  ·  已选 2 项"
    );
    assert_eq!(footer_items_text(Lang::Zh, 0), "空文件夹");
    assert_eq!(op_more_text(Lang::Zh, 2), "+2 个其他操作");
    assert_eq!(
        shortcut_conflict_message(Lang::Zh, "复制"),
        "已被“复制”使用。"
    );
    assert_eq!(size_units(Lang::Zh).steps[2], "MB");
    assert_eq!(age_units(Lang::Zh).now, "刚刚");
    assert_eq!(
        favnyr_core::fs::format_age(0, 90, age_units(Lang::Zh)),
        "1 分钟"
    );
    assert_eq!(
        favnyr_core::fs::format_size(1536, size_units(Lang::Zh)),
        "1.5 KB"
    );
    assert_eq!(
        language_labels().last().map(|l| l.to_string()).as_deref(),
        Some("简体中文")
    );
}
