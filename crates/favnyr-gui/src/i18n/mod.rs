//! UI translations — loaded from **TOML** files.
//!
//! The strings live in `crates/favnyr-gui/i18n/{lang}.toml` and are
//! **embedded at compile time** (so
//! 100% offline, no missing file possible). The user can
//! override or extend any key by dropping a
//! `<config>/favnyr/i18n/{lang}.toml` file (dotted-format keys, e.g.
//! `shortcut_action.copy = "…"`).
//!
//! Cascading fallback: key missing from a language → English value (catalogs
//! are built on an English base) → the key itself (never a panic,
//! never a surprise empty string). Adding a language = adding a `.toml` file +
//! an entry in `embedded()`/`catalog()`; adding a string = a key in
//! the TOML files + (if exposed to Slint) a field in `Strings`.

use std::collections::HashMap;
use std::sync::OnceLock;

use favnyr_core::{Lang, paths};

use crate::Strings;

// Embedded catalogs (sources of truth, versioned in the repo).
const EMBED_EN: &str = include_str!("../../i18n/en.toml");
const EMBED_FR: &str = include_str!("../../i18n/fr.toml");
const EMBED_ES: &str = include_str!("../../i18n/es.toml");
const EMBED_DE: &str = include_str!("../../i18n/de.toml");
const EMBED_IT: &str = include_str!("../../i18n/it.toml");
const EMBED_ZH: &str = include_str!("../../i18n/zh.toml");

fn embedded(lang: Lang) -> &'static str {
    match lang {
        Lang::En => EMBED_EN,
        Lang::Fr => EMBED_FR,
        Lang::Es => EMBED_ES,
        Lang::De => EMBED_DE,
        Lang::It => EMBED_IT,
        Lang::Zh => EMBED_ZH,
    }
}

/// Flattens a TOML table into dotted keys (`shortcut_action.copy`).
fn flatten_into(prefix: &str, table: &toml::Table, out: &mut HashMap<String, String>) {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            toml::Value::Table(t) => flatten_into(&key, t, out),
            _ => {}
        }
    }
}

/// Merges the keys from a TOML source on top of `out` (new ones overwrite).
fn overlay(src: &str, out: &mut HashMap<String, String>) {
    match toml::from_str::<toml::Table>(src) {
        Ok(table) => flatten_into("", &table, out),
        Err(err) => tracing::warn!(error = %err, "i18n: ignoring invalid TOML"),
    }
}

/// Builds a language's catalog: English base (fallback) → embedded
/// language → optional on-disk override `<config>/favnyr/i18n/{lang}.toml`.
fn build_catalog(lang: Lang) -> HashMap<String, String> {
    let mut map = HashMap::new();
    overlay(EMBED_EN, &mut map);
    if lang != Lang::En {
        overlay(embedded(lang), &mut map);
    }
    let disk = paths::config_dir()
        .join("i18n")
        .join(format!("{}.toml", lang.code()));
    if let Ok(src) = std::fs::read_to_string(&disk) {
        tracing::info!(path = %disk.display(), "i18n: on-disk override applied");
        overlay(&src, &mut map);
    }
    map
}

/// A language's (memoized) catalog. Built only once per language.
fn catalog(lang: Lang) -> &'static HashMap<String, String> {
    fn get(
        lang: Lang,
        cell: &'static OnceLock<HashMap<String, String>>,
    ) -> &'static HashMap<String, String> {
        cell.get_or_init(|| build_catalog(lang))
    }
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    static FR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static ES: OnceLock<HashMap<String, String>> = OnceLock::new();
    static DE: OnceLock<HashMap<String, String>> = OnceLock::new();
    static IT: OnceLock<HashMap<String, String>> = OnceLock::new();
    static ZH: OnceLock<HashMap<String, String>> = OnceLock::new();
    match lang {
        Lang::En => get(lang, &EN),
        Lang::Fr => get(lang, &FR),
        Lang::Es => get(lang, &ES),
        Lang::De => get(lang, &DE),
        Lang::It => get(lang, &IT),
        Lang::Zh => get(lang, &ZH),
    }
}

/// Translates a dotted key. Fallback: the language's value → (English base
/// already merged in) → the key itself.
pub fn tr(lang: Lang, key: &str) -> String {
    catalog(lang)
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

mod labels;
mod messages;
mod strings;
mod units;

#[cfg(test)]
mod tests;

pub use labels::*;
pub use messages::*;
pub use strings::*;
pub use units::*;
