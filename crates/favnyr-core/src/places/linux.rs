use super::*;

/// The trash (a single location). On Linux: navigable. On Windows:
/// empty path → the GUI opens it in Explorer.
pub fn trash_place() -> Place {
    #[cfg(windows)]
    {
        Place::simple(PlaceKind::Trash, PathBuf::new(), String::new())
    }
    #[cfg(not(windows))]
    {
        let path = dirs::data_dir()
            .map(|d| d.join("Trash").join("files"))
            .unwrap_or_default();
        Place::simple(PlaceKind::Trash, path, String::new())
    }
}

// ----- Helpers -----

pub(super) fn file_name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// A drive's label (its **name**, not its path):
///   - **Windows**: `"Windows (C:)"` (volume label + letter);
///   - **Linux**: volume label (`/dev/disk/by-label`) → otherwise the mount
///     point's name (udisks removable mounts carry the label) → otherwise
///     empty for the root `/` (the GUI then sets an i18n "System" label).
///
/// `disk_name` = `Disk::name()` = **device path** (`/dev/sda1`…) on
/// Linux — hence the need to resolve it. (Windows builds its label directly
/// in `windrives`, without going through here.)
#[cfg(not(windows))]
pub(super) fn drive_label(mount: &Path, disk_name: &str) -> String {
    #[cfg(target_os = "linux")]
    if let Some(label) = linux_volume_label(Path::new(disk_name))
        && !label.is_empty()
    {
        return label;
    }
    let _ = disk_name;
    if mount != Path::new("/") {
        let base = file_name_of(mount);
        if !base.is_empty() && base != "/" {
            return base;
        }
    }
    // Root without a label → the GUI will show an i18n "System" label.
    String::new()
}

/// Resolves a device's volume label via `/dev/disk/by-label/*`
/// (symlinks `LABEL → ../../<device>`). `None` if not found.
#[cfg(target_os = "linux")]
fn linux_volume_label(device: &Path) -> Option<String> {
    let target = std::fs::canonicalize(device).ok()?;
    let dir = std::fs::read_dir("/dev/disk/by-label").ok()?;
    for entry in dir.flatten() {
        if let Ok(resolved) = std::fs::canonicalize(entry.path())
            && resolved == target
        {
            return Some(udev_unescape(&entry.file_name().to_string_lossy()));
        }
    }
    None
}

/// Decodes udev escaping of `by-label` names (`\xNN` hex, e.g. `\x20`
/// for space). Rebuilds as bytes then UTF-8 (accented labels handled correctly).
#[cfg(target_os = "linux")]
fn udev_unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1] == b'x'
            && let Ok(code) = u8::from_str_radix(&s[i + 2..i + 4], 16)
        {
            out.push(code);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A **network** filesystem? (remote mount → "Network" section).
/// Covers common protocols and FUSE backends (`fuse.sshfs`, GVFS…).
#[cfg(not(windows))]
pub(super) fn is_network_fs(fs: &str) -> bool {
    const NET_FS: &[&str] = &[
        "nfs",
        "nfs4",
        "cifs",
        "smbfs",
        "smb3",
        "9p",
        "afs",
        "ncpfs",
        "coda",
        "davfs",
        "ceph",
        "glusterfs",
        "fuse.sshfs",
        "fuse.gvfsd-fuse",
        "fuse.rclone",
        "fuse.s3fs",
        "fuse.davfs2",
    ];
    NET_FS.iter().any(|n| fs.eq_ignore_ascii_case(n))
}

/// Removable mount (USB, card, external disk) → ejectable.
/// udisks heuristic: removable devices are auto-mounted under
/// `/run/media/<user>/…` (Fedora/Arch) or `/media/…` (Debian/Ubuntu).
#[cfg(not(windows))]
pub(super) fn is_removable_mount(mount: &Path) -> bool {
    let s = mount.to_string_lossy();
    s.starts_with("/run/media/") || s.starts_with("/media/")
}

/// Shares mounted by **GVFS** (GNOME/Nautilus) under
/// `/run/user/<uid>/gvfs/<backend>:<params>` — SMB, SFTP, MTP, DAV… They do
/// NOT show up as "disk" mounts (a single FUSE mount point), so the folder
/// is listed instead.
#[cfg(target_os = "linux")]
pub(super) fn gvfs_mounts() -> Vec<Place> {
    let root = gvfs_root();
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let raw = e.file_name().to_string_lossy().to_string();
            out.push(Place {
                kind: PlaceKind::Network,
                path,
                name: gvfs_pretty_name(&raw),
                removable: false,
                hotplug: false,
                device: String::new(),
                // Never measured: asking a share for its size is a round trip
                // that can stall for seconds, and the answer would be the
                // server's free space, not the user's quota on it.
                total_bytes: 0,
                free_bytes: 0,
            });
        }
    }
    out
}

/// Turns a GVFS mount name into something readable: `smb-share:server=nas,share=media`
/// → `media (nas)`; `sftp:host=host.tld,user=me` → `host.tld (sftp)`. Kept
/// language-neutral on purpose (no embedded word like "on"/"at"): per this
/// module's own convention, the core only returns raw, un-translated names.
#[cfg(target_os = "linux")]
pub(super) fn gvfs_pretty_name(raw: &str) -> String {
    let (backend, rest) = raw.split_once(':').unwrap_or((raw, ""));
    let mut params = std::collections::HashMap::new();
    for kv in rest.split(',') {
        if let Some((k, v)) = kv.split_once('=') {
            params.insert(k, v);
        }
    }
    let server = params.get("server").or_else(|| params.get("host")).copied();
    let share = params.get("share").copied();
    match (backend, share, server) {
        ("smb-share", Some(sh), Some(sv)) => format!("{sh} ({sv})"),
        (_, _, Some(sv)) => {
            let proto = backend.trim_end_matches("-share");
            format!("{sv} ({proto})")
        }
        _ => raw.to_string(),
    }
}

/// Filesystems that name a volume nobody browses: system bookkeeping, or a
/// container whose real volumes appear on their own rows once assembled.
#[cfg(target_os = "linux")]
const HIDDEN_VOLUME_FS: &[&str] = &[
    "swap",
    "LVM2_member",
    "linux_raid_member",
    "isw_raid_member",
    "ddf_raid_member",
    "squashfs",
];

/// Filesystem a locked encrypted container reports. It holds nothing mountable
/// until unlocked, so it is classified apart rather than hidden.
#[cfg(target_os = "linux")]
const ENCRYPTED_FS: &str = "crypto_LUKS";

/// One row of the block inventory, reduced to the fields the sidebar needs.
#[cfg(target_os = "linux")]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct BlockRow {
    pub(super) name: String,
    /// Kernel name of the device this one sits on, empty for a whole disk.
    pub(super) parent: String,
    /// `disk`, `part`, `loop`, `dm`…
    pub(super) kind: String,
    pub(super) fs_type: String,
    pub(super) mountpoint: String,
    pub(super) removable: bool,
    pub(super) size: u64,
    pub(super) label: String,
    /// Partition type as named by the table, e.g. the firmware partition.
    pub(super) part_type: String,
}

/// Reads the block inventory.
///
/// Byte sizes and a fixed locale on purpose: the human-readable size is
/// localised down to its decimal mark, and would not parse the same on two
/// machines.
#[cfg(target_os = "linux")]
pub(super) fn block_inventory() -> Option<String> {
    let output = std::process::Command::new("lsblk")
        .env("LC_ALL", "C")
        .args([
            "-b",
            "-P",
            "-o",
            "NAME,PKNAME,TYPE,FSTYPE,MOUNTPOINT,RM,SIZE,LABEL,PARTTYPENAME",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Splits one `KEY="value"` line. Values are quoted, and an embedded quote is
/// backslash-escaped — a label is free text, so that case is real.
#[cfg(target_os = "linux")]
pub(super) fn parse_pairs(line: &str) -> Vec<(String, String)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key = line[key_start..i].to_string();
        i += 1;
        if i >= bytes.len() || bytes[i] != b'"' {
            break;
        }
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != b'"' {
            // Skip whatever the backslash protects, quote included.
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        let value = line[value_start..i.min(bytes.len())].replace("\\\"", "\"");
        out.push((key, value));
        i += 1;
    }
    out
}

/// Turns the inventory text into rows, ignoring anything unparsable.
#[cfg(target_os = "linux")]
pub(super) fn parse_block_rows(text: &str) -> Vec<BlockRow> {
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut row = BlockRow::default();
        for (key, value) in parse_pairs(line) {
            match key.as_str() {
                "NAME" => row.name = value,
                "PKNAME" => row.parent = value,
                "TYPE" => row.kind = value,
                "FSTYPE" => row.fs_type = value,
                "MOUNTPOINT" => row.mountpoint = value,
                "RM" => row.removable = value == "1",
                "SIZE" => row.size = value.parse().unwrap_or(0),
                "LABEL" => row.label = value,
                "PARTTYPENAME" => row.part_type = value,
                _ => {}
            }
        }
        if !row.name.is_empty() {
            rows.push(row);
        }
    }
    rows
}

/// How this row should appear in the sidebar, if at all. The whole policy
/// lives here so the two families cannot drift apart.
#[cfg(target_os = "linux")]
pub(super) fn volume_kind(row: &BlockRow, all: &[BlockRow]) -> Option<PlaceKind> {
    // Already mounted: it is `drives()` that reports it, with its capacity.
    if !row.mountpoint.is_empty() {
        return None;
    }
    let has_child = || all.iter().any(|other| other.parent == row.name);
    // An encrypted container. Once unlocked it grows a child holding the real
    // filesystem, and THAT child is the volume — the container itself then has
    // nothing left to offer. Still locked, it is worth showing: knowing the
    // disk is there is most of what the user was missing.
    if row.fs_type == ENCRYPTED_FS {
        return (!has_child()).then_some(PlaceKind::LockedVolume);
    }
    // Nothing to mount without a recognised filesystem.
    if row.fs_type.is_empty() || HIDDEN_VOLUME_FS.contains(&row.fs_type.as_str()) {
        return None;
    }
    // The firmware partition is not a place anyone browses.
    if row.part_type.contains("EFI") {
        return None;
    }
    match row.kind.as_str() {
        // A partition, or the volume exposed by an unlocked container.
        "part" | "crypt" => Some(PlaceKind::Volume),
        // A whole device carrying a filesystem — a stick written without a
        // partition table. Once it IS partitioned, its partitions are the
        // volumes and the disk itself is not one.
        "disk" => (!has_child()).then_some(PlaceKind::Volume),
        _ => None,
    }
}

/// Builds the sidebar entry for a volume that is not mounted.
#[cfg(target_os = "linux")]
pub(super) fn volume_place(row: &BlockRow, kind: PlaceKind) -> Place {
    Place {
        kind,
        // No path: nothing is mounted yet. Like the Windows trash, the entry is
        // addressed by what it is rather than by where it lives.
        path: PathBuf::new(),
        name: if row.label.is_empty() {
            row.name.clone()
        } else {
            row.label.clone()
        },
        removable: row.removable,
        hotplug: is_hotplug_device(&row.name),
        device: format!("/dev/{}", row.name),
        total_bytes: row.size,
        // Only a mounted filesystem can say how much of it is left.
        free_bytes: 0,
    }
}

/// Directory where GVFS exposes its mounts.
///
/// The session variable is authoritative when it is set; the conventional path
/// is the fallback. Both the listing and the change signature go through here
/// so they always look at the same place: they used to resolve it differently,
/// and a session without that variable left the signature blind to a mount
/// appearing — the sidebar then never refreshed on its own.
#[cfg(target_os = "linux")]
pub(super) fn gvfs_root() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty());
    gvfs_root_in(runtime.as_deref().map(Path::new), unsafe { libc_getuid() })
}

/// Resolution rule of [`gvfs_root`], isolated from the environment.
#[cfg(target_os = "linux")]
pub(super) fn gvfs_root_in(runtime: Option<&Path>, uid: u32) -> PathBuf {
    match runtime {
        Some(dir) => dir.join("gvfs"),
        None => PathBuf::from(format!("/run/user/{uid}/gvfs")),
    }
}

// `getuid(2)` — direct FFI to libc (already linked) → no dependency.
#[cfg(target_os = "linux")]
unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// Should this mount point be shown to the user? Pseudo-filesystems and
/// irrelevant system mounts are excluded.
#[cfg(not(windows))]
pub(super) fn is_user_facing_mount(mount: &Path, fs: &str) -> bool {
    // Pseudo / ephemeral filesystems → hidden. (NETWORK filesystems do NOT
    // pass through here: `drives()` handles them upstream via `is_network_fs`.)
    const HIDDEN_FS: &[&str] = &[
        "squashfs",
        "tmpfs",
        "devtmpfs",
        "overlay",
        "proc",
        "sysfs",
        "cgroup",
        "cgroup2",
        "ramfs",
        "autofs",
        "fuse.portal",
        "fuse.gvfsd-fuse",
    ];
    if HIDDEN_FS.iter().any(|h| fs.eq_ignore_ascii_case(h)) {
        return false;
    }
    if mount == Path::new("/") {
        return true;
    }
    let s = mount.to_string_lossy();
    // System mounts → hidden; common user mounts → kept.
    if s.starts_with("/boot")
        || s.starts_with("/snap")
        || s.starts_with("/var/lib/docker")
        || s.starts_with("/var/snap")
        || s == "/dev"
    {
        return false;
    }
    s == "/home"
        || s.starts_with("/home/")
        || s.starts_with("/run/media/")
        || s.starts_with("/media/")
        || s.starts_with("/mnt/")
}
