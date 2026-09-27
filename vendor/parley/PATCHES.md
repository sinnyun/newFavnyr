# Vendored `parley` 0.8.0 — Favnyr patch

This directory is an unmodified copy of `parley` 0.8.0 as published on
crates.io, plus **one** change, in `src/analysis/mod.rs`. It is wired up by
`[patch.crates-io]` in the repository root `Cargo.toml`; Slint 1.16 builds its
text layout on parley, so this is the text engine of the whole interface.

## The change

`AnalysisDataSources::word_segmenter()` and `::line_segmenter()` now build
ICU4X segmenters **with their models loaded** (`new_dictionary`) instead of the
upstream `new_for_non_complex_scripts`, which loads none:

```rust
-        const { WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default()) }
+        WordSegmenter::new_dictionary(WordBreakInvariantOptions::default())
```

(The `const` blocks go away: `new_dictionary` is not a `const fn`. It is
`new_for_non_complex_scripts` plus `load_dictionary()`.)

## Why

Without the models, ICU4X announces the fallback once per text run it cannot
segment — on a Chinese interface that is essentially every label:

```text
WARN icu_provider::error: ICU4X data error: No segmentation model for language: ja
```

parley runs its word segmenter over **every** analysed run, so the notice is not
tied to any app action, and it is repeated: one line per Han run, per layout.
A user's `favnyr.log` had 425 of them.

The data is not missing from the build: `icu_segmenter`'s `compiled_data`
feature (already enabled by parley) pulls in `icu_segmenter_data`, whose baked
`SegmenterDictionaryAutoV1` (Chinese/Japanese) and
`SegmenterDictionaryExtendedV1` (Thai/Lao/Khmer/Myanmar) are what
`load_dictionary()` reads. Upstream simply never calls it.

Measured on this repository (release, `icu_segmenter` 2.2.0):

| | upstream (`new_for_non_complex_scripts`) | this patch (`new_dictionary`) |
|---|---|---|
| word+line construction | 13 ns | 469 ns |
| Chinese word breaks for `这是一个中文分词与换行行为的测试句子。` | `[0, 54, 57]` (one token) | `[0, 3, 6, 12, 18, 24, 27, 30, 33, 39, 42, 48, 54, 57]` (这是/一个/中文/分词/…) |
| binary size (word segmenter referenced) | 0.16 MB | 3.79 MB (**+3.62 MB**) |

The same font-context layout of a long Chinese sentence breaks into the same
lines either way: Chinese line breaking comes from UAX #14 (ideographs may
break anywhere) and never consults this model, so wrapping is unchanged.
`crates/favnyr-gui/src/main.rs` has a regression test asserting the notice is
not emitted during a real layout.

## Exit path

Delete this directory and the `[patch.crates-io]` stanza (plus the `parley`
and `log` dev-dependencies of `favnyr-gui`) once either upstream does this:

- parley loads the models itself, or gains a way to opt out of the notice;
- ICU4X stops logging a "data error" from the documented
  `*_for_non_complex_scripts` constructors, whose whole point is having no
  model (see the discussion in <https://github.com/unicode-org/icu4x/issues/2781>).

Slint pins parley through `i-slint-core` (`shared-parley`); a Slint release that
picks a fixed parley makes this patch unnecessary — check `cargo tree -i parley`
after such an upgrade and re-apply or drop it.
