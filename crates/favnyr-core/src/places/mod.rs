//! Locations for the "Places" sidebar — **cross-platform**.
//!
//! Three families:
//!   - **Shortcuts**: standard user folders (`dirs`) — Home, Desktop,
//!     Documents, Downloads, Pictures, Music, Videos.
//!   - **Drives**: mounted volumes (`sysinfo`) — `C:`/`D:`/USB on Windows;
//!     `/`, `/home`, `/run/media/$USER/*`… on Linux (pseudo-mounts filtered out).
//!   - **Trash**: a single location. On Linux it's *navigable*
//!     (`~/.local/share/Trash/files`); on Windows it's virtual → the GUI
//!     opens it in Explorer (the GUI decides based on `kind`).
//!
//! Display (i18n labels for fixed entries, icons) is decided on the GUI side
//! from the `kind`; the core only returns a raw `name` (mostly useful for
//! drives).

use std::path::{Path, PathBuf};

/// Category of a location — drives the icon and label on the GUI side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceKind {
    /// Home folder (`$HOME` / `%USERPROFILE%`).
    Home,
    /// Standard user folder (Documents, Downloads…).
    Folder,
    /// Local mounted volume (drive, partition, removable device).
    Drive,
    /// Local volume the machine can see but has not mounted. It carries no
    /// path yet — only the device that would be mounted.
    Volume,
    /// Encrypted volume, still locked. It holds no mountable filesystem until
    /// it is unlocked, and unlocking is not something Favnyr offers: that
    /// would mean holding a passphrase.
    LockedVolume,
    /// Network location: mapped drive (`Z:` → `\\srv\part`), NFS/CIFS/SSHFS/GVFS
    /// mount (Linux), or WSL distribution (`\\wsl$\…`, Windows).
    Network,
    /// Trash.
    Trash,
}

/// A location listable in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub kind: PlaceKind,
    /// Target path (empty for the Windows trash, which has no real path).
    pub path: PathBuf,
    /// Raw displayable name (volume name / folder name). The GUI may
    /// replace it with an i18n label for `Home`/`Trash`.
    pub name: String,
    /// Removable device (USB, CD…) → offers to release it.
    pub removable: bool,
    /// Can the device be physically unplugged while the machine runs? Decides
    /// whether releasing it is announced as "you may unplug it" or merely as
    /// "released" — see [`is_hotplug_device`].
    pub hotplug: bool,
    /// Eject/disconnect target: `/dev/sdX1` (Linux), drive letter `X:`
    /// (local or mapped-network Windows drive), otherwise empty (WSL, GVFS…).
    pub device: String,
    /// Capacity of the volume in bytes, `0` when it could not be read.
    ///
    /// Zero is the "unknown" marker rather than an `Option`: an empty card
    /// reader, an optical drive with no disc and a share that never answered
    /// all mean the same thing to the interface — show no capacity — and a
    /// volume of genuinely zero bytes does not exist.
    pub total_bytes: u64,
    /// Bytes still writable **by this user**, `0` when unknown.
    ///
    /// Deliberately the user-facing figure, not the raw free count: a
    /// filesystem keeps a reserve (ext4 holds back 5% for root) and a share may
    /// impose a quota. The badge answers "can I still write here", so it must
    /// report what the caller could actually use.
    pub free_bytes: u64,
}

impl Place {
    /// Constructor for a simple location (not removable, no device) —
    /// shortcuts, trash, root.
    fn simple(kind: PlaceKind, path: PathBuf, name: String) -> Self {
        Place {
            kind,
            path,
            name,
            removable: false,
            hotplug: false,
            device: String::new(),
            // A shortcut and the trash sit on a volume that is listed on its
            // own line: repeating its capacity here would state the same fact
            // twice under two names, which reads as a contradiction rather
            // than as agreement.
            total_bytes: 0,
            free_bytes: 0,
        }
    }
}

/// Shortcuts to standard user folders (the ones that exist).
pub fn user_places() -> Vec<Place> {
    let mut out = Vec::new();
    let mut push = |dir: Option<PathBuf>, kind: PlaceKind| {
        if let Some(p) = dir
            && p.is_dir()
        {
            let name = file_name_of(&p);
            out.push(Place::simple(kind, p, name));
        }
    };
    push(dirs::home_dir(), PlaceKind::Home);
    push(dirs::desktop_dir(), PlaceKind::Folder);
    push(dirs::document_dir(), PlaceKind::Folder);
    push(dirs::download_dir(), PlaceKind::Folder);
    push(dirs::picture_dir(), PlaceKind::Folder);
    push(dirs::audio_dir(), PlaceKind::Folder);
    push(dirs::video_dir(), PlaceKind::Folder);
    out
}

/// LIGHTWEIGHT "signature" of the set of drives/mounts, to cheaply detect an
/// external change (subst, `net use`, USB, mount…) and refresh the sidebar
/// without constantly re-scanning.
///   - **Windows**: bitmask of mounted letters (`GetLogicalDrives`,
///     instantaneous) → catches a drive appearing/disappearing.
///   - **Linux**: hash of `/proc/self/mountinfo` (mounts) + of the GVFS
///     folder (user network shares) → catches (un)mounts.
///
/// Two calls returning the SAME value ⇒ nothing has changed.
pub fn drives_signature() -> u64 {
    #[cfg(windows)]
    {
        windrives::logical_drives_mask() as u64
    }
    #[cfg(target_os = "linux")]
    {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Ok(s) = std::fs::read_to_string("/proc/self/mountinfo") {
            s.hash(&mut h);
        }
        // GVFS (user mounts, outside mountinfo): entry names.
        if let Ok(rd) = std::fs::read_dir(gvfs_root()) {
            let mut entries: Vec<String> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            entries.sort();
            entries.hash(&mut h);
        }
        h.finish()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0
    }
}

/// Mounted volumes "visible to the user": LOCAL drives
/// (`PlaceKind::Drive`) **and** network locations (`PlaceKind::Network`) —
/// mapped drives, NFS/CIFS/SSHFS/GVFS mounts, WSL distributions. The
/// GUI splits the two families into "Drives" / "Network" sections.
pub fn drives() -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();

    // ----- Linux (and other non-Windows): mounts via `sysinfo` -----
    // Windows does NOT use `sysinfo` here: `GetLogicalDrives` (below) already
    // sees ALL letters — including mapped network, SUBST, VHD — and properly
    // computes type/removable/device. Avoiding the double pass removes any
    // ambiguity on `device`/`removable`.
    #[cfg(not(windows))]
    {
        use sysinfo::Disks;
        let disks = Disks::new_with_refreshed_list();
        for d in disks.list() {
            let mount = d.mount_point().to_path_buf();
            let fs = d.file_system().to_string_lossy().to_string();
            let is_net = is_network_fs(&fs);
            // A mount is kept if it's network (regardless of its path) OR
            // if it passes the "user-facing" filter for local mounts.
            if !is_net && !is_user_facing_mount(&mount, &fs) {
                continue;
            }
            // Deduplicate by mount point (sysinfo can list duplicates).
            if out.iter().any(|p| p.path == mount) {
                continue;
            }
            let name = drive_label(&mount, &d.name().to_string_lossy());
            out.push(Place {
                kind: if is_net {
                    PlaceKind::Network
                } else {
                    PlaceKind::Drive
                },
                path: mount.clone(),
                name,
                // Removable → ejectable. udisks heuristic: mount under
                // /run/media|/media. (Network is never "removable".)
                removable: !is_net && is_removable_mount(&mount),
                // Whether the cable can be pulled — a different question from
                // the one above, and the one the release message answers.
                hotplug: !is_net && is_hotplug_device(&d.name().to_string_lossy()),
                // Eject device: `/dev/sdX1` (= `Disk::name()` on Linux).
                device: if is_net {
                    String::new()
                } else {
                    d.name().to_string_lossy().to_string()
                },
                // Free of charge: the listing above already refreshed these,
                // so the `statvfs` behind them is a cost this scan pays
                // whether or not anything reads the result. Shares are left
                // unmeasured all the same — the figure would describe the
                // server, not what this user may write.
                total_bytes: if is_net { 0 } else { d.total_space() },
                free_bytes: if is_net { 0 } else { d.available_space() },
            });
        }
    }

    // ----- Windows: all letters via the Win32 API -----
    #[cfg(windows)]
    for p in windrives::logical_drives() {
        if !out.iter().any(|e| e.path == p.path) {
            out.push(p);
        }
    }

    // Additional network locations WITHOUT a classic mount point:
    // GVFS (Linux) and WSL distributions (Windows).
    out.extend(network_extra());
    // Stable sort: first by family (local before network), then by path —
    // keeps sections grouped regardless of discovery order.
    out.sort_by(|a, b| {
        (a.kind == PlaceKind::Network)
            .cmp(&(b.kind == PlaceKind::Network))
            .then_with(|| a.path.cmp(&b.path))
    });
    // Guarantees the root "/" comes first: some systems (atomic / overlay /
    // composefs) don't report it via `sysinfo` → without this the Drives
    // section could be empty. (Windows always lists its letters.)
    //
    // It carries NO capacity, deliberately. On such a system the root is a
    // small read-only overlay — measured at 45 MB, entirely full — so a gauge
    // there would report the size of the overlay rather than of any storage
    // the user can write to. A figure that is technically exact and
    // practically meaningless is worse than none: the volumes that do hold the
    // user's data are listed on their own lines, with their own gauges.
    #[cfg(not(windows))]
    if !out.iter().any(|p| p.path == Path::new("/")) {
        out.insert(
            0,
            Place::simple(PlaceKind::Drive, PathBuf::from("/"), String::new()),
        );
    }
    out
}

/// Does the path live on a NETWORK filesystem? Used to spare the network
/// expensive convenience work (e.g. recursive folder mtime, which walks the
/// whole tree — Explorer does nothing like that on a share).
/// Detection WITHOUT touching the network:
///   - **Windows**: UNC path, or `DRIVE_REMOTE` mapped letter
///     (`GetDriveTypeW` reads the LOCAL mount table — instantaneous);
///   - **Linux**: network fstype (`is_network_fs`) of the path's most
///     specific mount point (`/proc/self/mountinfo`, local read).
pub fn is_network_path(path: &Path) -> bool {
    if crate::fs::is_unc_path(path) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(p)) = path.components().next()
            && let Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) = p.kind()
        {
            return windrives::drive_is_remote(letter as char);
        }
        false
    }
    #[cfg(target_os = "linux")]
    {
        // Mount point with the longest prefix → its fstype.
        let Ok(info) = std::fs::read_to_string("/proc/self/mountinfo") else {
            return false;
        };
        let mut best: Option<(usize, bool)> = None; // (mount point length, network?)
        for line in info.lines() {
            // Format: … field[4] = mount point … "-" fstype source options
            let mut parts = line.split(' ');
            let Some(mount) = parts.nth(4) else { continue };
            let Some(fs) = line.split(" - ").nth(1).and_then(|r| r.split(' ').next()) else {
                continue;
            };
            if path.starts_with(mount) && best.is_none_or(|(l, _)| mount.len() > l) {
                best = Some((mount.len(), is_network_fs(fs)));
            }
        }
        best.map(|(_, net)| net).unwrap_or(false)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// Result of enumerating a network server's shares.
pub enum NetShares {
    /// Visible shares (possibly empty).
    Ok(Vec<String>),
    /// Access denied / failed logon → offer a system login.
    AuthNeeded,
    /// Server unreachable (offline, invalid host, firewall…).
    Unreachable,
}

/// VISIBLE SMB shares of a server (`server` = host name without `\\`). Allows
/// "browsing" a `\\HOST` machine (like Explorer), where `std::fs` can only
/// list a single share. Windows only (`Unreachable` elsewhere).
pub fn net_shares(server: &str) -> NetShares {
    #[cfg(windows)]
    {
        windrives::shares_of(server)
    }
    #[cfg(not(windows))]
    {
        let _ = server;
        NetShares::Unreachable
    }
}

/// Establishes an authenticated network connection to `resource` (`\\HOST\share`
/// or `\\HOST\IPC$`), showing the **Windows credentials dialog** if needed.
/// `true` if connected (or already). Windows only. Blocking (modal).
pub fn net_connect_prompt(resource: &str) -> bool {
    #[cfg(windows)]
    {
        windrives::connect_prompt(resource)
    }
    #[cfg(not(windows))]
    {
        let _ = resource;
        false
    }
}

/// Buses whose devices can be unplugged while the machine is running.
#[cfg(target_os = "linux")]
const HOTPLUG_BUSES: &[&str] = &["/usb", "/mmc", "/firewire", "/pcmcia"];

/// Can this device be physically unplugged?
///
/// NOT "is it a stick rather than a disk". Measured on real hardware, the
/// rotational flag classifies the opposite way — an internal SSD reports 0
/// while a USB drive can report 1 — and the removable-media flag also marks an
/// optical drive. What a message needs to answer is whether the user may pull
/// the cable, and that is a property of the BUS, not of the storage medium.
///
/// This is the criterion the system itself applies: udisks separates
/// `power-off-drive` from `power-off-drive-system` on the same basis, so
/// following it keeps Favnyr and the system from contradicting each other.
///
/// Read from sysfs, where a device's real path traverses its bus: a plain
/// symlink resolution, no subprocess and no dependency. `false` on any other
/// platform, and whenever the device cannot be resolved.
pub fn is_hotplug_device(device: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Some(name) = Path::new(device).file_name() else {
            return false;
        };
        let Ok(resolved) = std::fs::canonicalize(Path::new("/sys/class/block").join(name)) else {
            return false;
        };
        path_traverses_hotplug_bus(&resolved.to_string_lossy())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = device;
        false
    }
}

/// Rule behind [`is_hotplug_device`], isolated from the filesystem.
#[cfg(target_os = "linux")]
fn path_traverses_hotplug_bus(resolved: &str) -> bool {
    HOTPLUG_BUSES.iter().any(|bus| resolved.contains(bus))
}

/// Cheap fingerprint of the block-device topology: the names the kernel
/// publishes under `/sys/class/block`.
///
/// Complementary to [`drives_signature`], which follows MOUNTS. Plugging a disk
/// in changes this one and not that one, and a volume that never gets mounted
/// would otherwise announce itself nowhere. It is a directory listing and
/// nothing else, so it costs microseconds and can be polled freely.
pub fn block_signature() -> u64 {
    #[cfg(target_os = "linux")]
    {
        use std::hash::{Hash, Hasher};
        let Ok(entries) = std::fs::read_dir("/sys/class/block") else {
            return 0;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        names.hash(&mut hasher);
        hasher.finish()
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Volumes the machine can see but has NOT mounted.
///
/// [`drives()`] reports mounted filesystems, which is why a disk stays
/// invisible until something else mounts it. This answers the other question —
/// what *could* be mounted — and is deliberately kept out of `drives()`: it
/// costs a subprocess, so the caller decides when to pay, and
/// [`block_signature`] tells it when the answer could have changed.
///
/// Empty on any failure: a machine without the tool keeps exactly the
/// behaviour it had before.
pub fn unmounted_volumes() -> Vec<Place> {
    #[cfg(target_os = "linux")]
    {
        let Some(text) = block_inventory() else {
            return Vec::new();
        };
        let rows = parse_block_rows(&text);
        let mut out: Vec<Place> = rows
            .iter()
            .filter_map(|row| volume_kind(row, &rows).map(|kind| volume_place(row, kind)))
            .collect();
        // Kernel order is not guaranteed; the device name keeps the sidebar
        // stable between two scans.
        out.sort_by(|a, b| a.device.cmp(&b.device));
        out
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Network locations that don't show up as a "disk" mount point:
/// GVFS shares (Linux) and WSL distributions (Windows).
fn network_extra() -> Vec<Place> {
    #[cfg(target_os = "linux")]
    {
        gvfs_mounts()
    }
    #[cfg(windows)]
    {
        windrives::wsl_distros()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

/// Enumeration of Windows drives via the Win32 API (complement to `sysinfo`).
///
/// `GetLogicalDrives` returns a bitmask of mounted letters (bit 0 = A:
/// … bit 25 = Z:) — it also sees **mapped network** drives, **SUBST** and
/// **mounted VHD** that `sysinfo` ignores. Each letter is enriched with its
/// type (`GetDriveTypeW`) and its volume label (`GetVolumeInformationW`).
///
/// A direct FFI to `kernel32`, linked by default under `windows-msvc`, avoids
/// an extra dependency.
#[cfg(windows)]
mod windrives;

mod linux;

#[cfg(test)]
mod tests;

pub use linux::*;
