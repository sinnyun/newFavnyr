use super::*;

// ----- Sorting ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortColumn {
    Name,
    Path,
    Size,
    Modified,
    /// Modification age — sorts on the same field as `Modified`
    /// (mtime), it's just the "age" column (rendered with a hot→cold color gradient).
    Age,
    /// File extension — sorts by type, then by name when extensions are equal.
    Ext,
}

impl SortColumn {
    pub fn code(self) -> &'static str {
        match self {
            SortColumn::Name => "name",
            SortColumn::Path => "path",
            SortColumn::Size => "size",
            SortColumn::Modified => "modified",
            SortColumn::Age => "age",
            SortColumn::Ext => "ext",
        }
    }

    pub fn from_code(s: &str) -> Option<SortColumn> {
        Some(match s {
            "name" => SortColumn::Name,
            "path" => SortColumn::Path,
            "size" => SortColumn::Size,
            "modified" => SortColumn::Modified,
            "age" => SortColumn::Age,
            "ext" => SortColumn::Ext,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn flip(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }
}

/// Coarse category of an entry, used by the `GroupMode::Category` grouping.
/// Deliberately broader than `FileKind`: archives, programs, configs and
/// unknown files all land in `Other`, so a listing splits into a handful of
/// sections a reader can scan at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Folder,
    Image,
    Video,
    Audio,
    Document,
    Other,
}

impl Category {
    /// Section order, and the sort rank inside `GroupMode::Category` (folders
    /// first, then the media families, `Other` last).
    pub fn rank(self) -> u8 {
        match self {
            Category::Folder => 0,
            Category::Image => 1,
            Category::Video => 2,
            Category::Audio => 3,
            Category::Document => 4,
            Category::Other => 5,
        }
    }

    pub fn of(kind: FileKind) -> Self {
        match kind {
            FileKind::Folder => Category::Folder,
            FileKind::Image => Category::Image,
            FileKind::Video => Category::Video,
            FileKind::Audio => Category::Audio,
            FileKind::Document => Category::Document,
            FileKind::Archive | FileKind::Application | FileKind::Config | FileKind::File => {
                Category::Other
            }
        }
    }

    /// Category of a row's wire kind code (`FileRow.kind`): an unknown code is
    /// `Other` rather than an error, so a stale value can only misplace a row,
    /// never drop it.
    pub fn of_code(kind_code: i32) -> Self {
        FileKind::from_code(kind_code).map_or(Category::Other, Category::of)
    }

    pub fn code(self) -> &'static str {
        match self {
            Category::Folder => "folder",
            Category::Image => "image",
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Document => "document",
            Category::Other => "other",
        }
    }

    pub fn from_code(s: &str) -> Option<Category> {
        Some(match s {
            "folder" => Category::Folder,
            "image" => Category::Image,
            "video" => Category::Video,
            "audio" => Category::Audio,
            "document" => Category::Document,
            "other" => Category::Other,
            _ => return None,
        })
    }
}

/// Grouping by type, orthogonal to the sort criterion (column + direction).
/// - `FoldersFirst` : folders on top, then files (default mode).
/// - `FilesFirst`   : files on top, then folders.
/// - `Mixed`        : no grouping, everything is sorted together by the criterion.
/// - `Category`     : folders, then the media families, then everything else
///   (cf. `Category`); the view draws one section header per block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupMode {
    FoldersFirst,
    FilesFirst,
    Mixed,
    Category,
}

impl GroupMode {
    pub fn code(self) -> &'static str {
        match self {
            GroupMode::FoldersFirst => "folders",
            GroupMode::FilesFirst => "files",
            GroupMode::Mixed => "mixed",
            GroupMode::Category => "category",
        }
    }

    pub fn from_code(s: &str) -> Option<GroupMode> {
        Some(match s {
            "folders" => GroupMode::FoldersFirst,
            "files" => GroupMode::FilesFirst,
            "mixed" => GroupMode::Mixed,
            "category" => GroupMode::Category,
            _ => return None,
        })
    }
}

/// Sorts in place according to the criterion (`column` + `order`) and the
/// grouping by type (`group`). Grouping takes **priority** over the criterion:
/// in `FoldersFirst`/`FilesFirst` mode, one group always precedes the other
/// regardless of sort direction; the criterion only breaks ties within a
/// group. In `Mixed` mode, folders and files are sorted together by the
/// criterion alone.
///
/// Sort by name: case-insensitive (consistent with the visual sort).
pub fn sort(entries: &mut [Entry], column: SortColumn, order: SortOrder, group: GroupMode) {
    use std::cmp::Reverse;
    let asc = matches!(order, SortOrder::Asc);
    // Grouping rank (folders vs files): it splits the list into two blocks the
    // criterion never crosses, and — unlike the criterion — it is NEVER reversed
    // by the direction (folders stay on top in FoldersFirst even when sorting
    // descending). `Mixed` puts everything in one block.
    let rank = |e: &Entry| -> u8 {
        match group {
            GroupMode::FoldersFirst => u8::from(!e.is_dir),
            GroupMode::FilesFirst => u8::from(e.is_dir),
            GroupMode::Mixed => 0,
            GroupMode::Category => Category::of(e.kind).rank(),
        }
    };
    // Name/Ext sort case-insensitively. Lowercasing a name INSIDE a comparison
    // sort re-allocates it on every comparison (~N·log N times); caching the key
    // lowercases each name once per entry instead. Only the criterion is
    // reversed for a descending sort (via `Reverse`), never the grouping rank.
    match column {
        SortColumn::Name if asc => entries.sort_by_cached_key(|e| (rank(e), e.name.to_lowercase())),
        SortColumn::Name => {
            entries.sort_by_cached_key(|e| (rank(e), Reverse(e.name.to_lowercase())))
        }
        // Type sort: extension (lowercase, dotfiles included), then name so the
        // order stays stable within one type.
        SortColumn::Ext if asc => entries.sort_by_cached_key(|e| {
            (
                rank(e),
                ops::ext_of(&e.name).to_string(),
                e.name.to_lowercase(),
            )
        }),
        SortColumn::Ext => entries.sort_by_cached_key(|e| {
            (
                rank(e),
                Reverse((ops::ext_of(&e.name).to_string(), e.name.to_lowercase())),
            )
        }),
        // Size / date / path: these comparators allocate nothing, so a direct
        // comparison sort is already optimal — only the grouping is kept apart
        // from the (possibly reversed) criterion.
        SortColumn::Size | SortColumn::Modified | SortColumn::Age | SortColumn::Path => {
            entries.sort_by(|a, b| {
                rank(a).cmp(&rank(b)).then_with(|| {
                    let ord = match column {
                        SortColumn::Size => {
                            a.size_bytes.unwrap_or(0).cmp(&b.size_bytes.unwrap_or(0))
                        }
                        SortColumn::Modified | SortColumn::Age => {
                            a.mtime_unix.unwrap_or(0).cmp(&b.mtime_unix.unwrap_or(0))
                        }
                        _ => a.path.cmp(&b.path),
                    };
                    if asc { ord } else { ord.reverse() }
                })
            });
        }
    }
}
