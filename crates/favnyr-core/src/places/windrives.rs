use super::{Place, PlaceKind};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(lp_root_path_name: *const u16) -> u32;
    /// Capacity of a volume. The FIRST output is what the *caller* may
    /// still write, which a quota can make smaller than the volume's own
    /// free count — that is the figure a user cares about.
    fn GetDiskFreeSpaceExW(
        lp_directory_name: *const u16,
        lp_free_bytes_available_to_caller: *mut u64,
        lp_total_number_of_bytes: *mut u64,
        lp_total_number_of_free_bytes: *mut u64,
    ) -> i32;
    /// Suppresses the modal the system would otherwise raise on a drive
    /// with no media — the "There is no disk in the drive" box, which a
    /// background probe must never be able to trigger.
    fn SetThreadErrorMode(new_mode: u32, old_mode: *mut u32) -> i32;
    fn GetVolumeInformationW(
        lp_root_path_name: *const u16,
        lp_volume_name_buffer: *mut u16,
        n_volume_name_size: u32,
        lp_volume_serial_number: *mut u32,
        lp_maximum_component_length: *mut u32,
        lp_file_system_flags: *mut u32,
        lp_file_system_name_buffer: *mut u16,
        n_file_system_name_size: u32,
    ) -> i32;
    // Querying a volume's bus (USB? → ejectable), see `bus_ejectable`.
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sa: *mut core::ffi::c_void,
        disp: u32,
        flags: u32,
        template: isize,
    ) -> isize;
    fn DeviceIoControl(
        h: isize,
        ctl: u32,
        inbuf: *mut core::ffi::c_void,
        insz: u32,
        outbuf: *mut core::ffi::c_void,
        outsz: u32,
        ret: *mut u32,
        ov: *mut core::ffi::c_void,
    ) -> i32;
    fn CloseHandle(h: isize) -> i32;
}
const INVALID_HANDLE: isize = -1;
const FILE_SHARE_RW: u32 = 0x0000_0003;
const OPEN_EXISTING: u32 = 3;
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002d_1400;

// Target of a mapped network drive (`Z:` → `\\server\share`) + enumeration
// of a `\\HOST` server's SMB shares. `mpr.dll` is a system DLL, which
// avoids an extra crate.
#[link(name = "mpr")]
unsafe extern "system" {
    fn WNetGetConnectionW(local: *const u16, remote: *mut u16, len: *mut u32) -> u32;
    fn WNetOpenEnumW(
        scope: u32,
        ty: u32,
        usage: u32,
        netres: *mut NetResourceW,
        handle: *mut isize,
    ) -> u32;
    fn WNetEnumResourceW(
        handle: isize,
        count: *mut u32,
        buf: *mut core::ffi::c_void,
        bufsize: *mut u32,
    ) -> u32;
    fn WNetCloseEnum(handle: isize) -> u32;
    // Authenticated connection with system PROMPT (network login).
    fn WNetAddConnection2W(
        netres: *mut NetResourceW,
        password: *const u16,
        user: *const u16,
        flags: u32,
    ) -> u32;
}
// Win32 error codes "authentication required" vs "unreachable".
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_ALREADY_ASSIGNED: u32 = 85;
const ERROR_INVALID_PASSWORD: u32 = 86;
const ERROR_SESSION_CREDENTIAL_CONFLICT: u32 = 1219;
const ERROR_LOGON_FAILURE: u32 = 1326;
const CONNECT_INTERACTIVE: u32 = 0x0000_0008;
const CONNECT_PROMPT: u32 = 0x0000_0010;

fn is_auth_error(rc: u32) -> bool {
    matches!(
        rc,
        ERROR_ACCESS_DENIED
            | ERROR_INVALID_PASSWORD
            | ERROR_SESSION_CREDENTIAL_CONFLICT
            | ERROR_LOGON_FAILURE
    )
}

/// Establishes an authenticated connection to `resource` (`\\HOST\share` or
/// `\\HOST\IPC$`), showing the **Windows credentials dialog** if
/// necessary (`CONNECT_PROMPT`). `true` if connected (or already connected),
/// `false` if cancelled / failed. Blocking (modal dialog).
pub fn connect_prompt(resource: &str) -> bool {
    let mut remote_w = wide(resource);
    let mut nr = NetResourceW {
        dw_scope: 0,
        dw_type: RESOURCETYPE_DISK,
        dw_display_type: 0,
        dw_usage: 0,
        lp_local_name: std::ptr::null_mut(),
        lp_remote_name: remote_w.as_mut_ptr(),
        lp_comment: std::ptr::null_mut(),
        lp_provider: std::ptr::null_mut(),
    };
    let rc = unsafe {
        WNetAddConnection2W(
            &mut nr,
            std::ptr::null(),
            std::ptr::null(),
            CONNECT_INTERACTIVE | CONNECT_PROMPT,
        )
    };
    matches!(rc, NO_ERROR | ERROR_ALREADY_ASSIGNED)
}
// `NETRESOURCEW` (Win32): 4 DWORDs then 4 pointers → `#[repr(C)]`.
#[repr(C)]
struct NetResourceW {
    dw_scope: u32,
    dw_type: u32,
    dw_display_type: u32,
    dw_usage: u32,
    lp_local_name: *mut u16,
    lp_remote_name: *mut u16,
    lp_comment: *mut u16,
    lp_provider: *mut u16,
}
const RESOURCE_GLOBALNET: u32 = 0x0000_0002;
const RESOURCETYPE_DISK: u32 = 0x0000_0001;
const NO_ERROR: u32 = 0;
const ERROR_MORE_DATA: u32 = 234;

/// Reads a NUL-terminated UTF-16 string from a pointer (remote share).
unsafe fn pwstr_to_string(p: *const u16) -> String {
    unsafe {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0isize;
        while *p.offset(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len as usize))
    }
}

/// Enumerates a server's VISIBLE disk shares (`server` = host name,
/// without `\\`). Ignores hidden administrative shares (`C$`, `IPC$`…). A
/// BLOCKING call (network timeout) — like `read_dir` on a remote path.
/// Distinguishes "access denied" (→ offer a login) from "unreachable".
pub fn shares_of(server: &str) -> super::NetShares {
    use super::NetShares;
    let mut remote_w = wide(&format!(r"\\{server}"));
    let mut nr = NetResourceW {
        dw_scope: RESOURCE_GLOBALNET,
        dw_type: RESOURCETYPE_DISK,
        dw_display_type: 0,
        dw_usage: 0,
        lp_local_name: std::ptr::null_mut(),
        lp_remote_name: remote_w.as_mut_ptr(),
        lp_comment: std::ptr::null_mut(),
        lp_provider: std::ptr::null_mut(),
    };
    let mut handle: isize = 0;
    let rc = unsafe {
        WNetOpenEnumW(
            RESOURCE_GLOBALNET,
            RESOURCETYPE_DISK,
            0,
            &mut nr,
            &mut handle,
        )
    };
    if rc != NO_ERROR {
        return if is_auth_error(rc) {
            NetShares::AuthNeeded
        } else {
            NetShares::Unreachable
        };
    }
    let mut shares = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut count: u32 = 0xFFFF_FFFF; // as many as the buffer can hold
        let mut bufsize: u32 = buf.len() as u32;
        let rc = unsafe {
            WNetEnumResourceW(
                handle,
                &mut count,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                &mut bufsize,
            )
        };
        if rc == ERROR_MORE_DATA {
            buf.resize(bufsize as usize, 0);
            continue;
        }
        if rc != NO_ERROR {
            break; // ERROR_NO_MORE_ITEMS (259) or end
        }
        let items = unsafe {
            std::slice::from_raw_parts(buf.as_ptr() as *const NetResourceW, count as usize)
        };
        for it in items {
            let remote = unsafe { pwstr_to_string(it.lp_remote_name) };
            // `\\HOST\share` → last segment = share name.
            if let Some(name) = remote.rsplit('\\').next()
                && !name.is_empty()
                && !name.ends_with('$')
            {
                shares.push(name.to_string());
            }
        }
    }
    unsafe { WNetCloseEnum(handle) };
    super::NetShares::Ok(shares)
}

// Enumeration of WSL distributions via the registry (advapi32, system DLL).
type Hkey = isize;
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(hkey: Hkey, sub: *const u16, opts: u32, sam: u32, out: *mut Hkey) -> i32;
    fn RegEnumKeyExW(
        hkey: Hkey,
        index: u32,
        name: *mut u16,
        name_len: *mut u32,
        reserved: *mut u32,
        class: *mut u16,
        class_len: *mut u32,
        last_write: *mut u64,
    ) -> i32;
    fn RegQueryValueExW(
        hkey: Hkey,
        value: *const u16,
        reserved: *mut u32,
        ty: *mut u32,
        data: *mut u8,
        data_len: *mut u32,
    ) -> i32;
    fn RegCloseKey(hkey: Hkey) -> i32;
}
// `((HKEY)(LONG)0x80000001)` sign-extended to pointer size.
const HKEY_CURRENT_USER: Hkey = 0x8000_0001u32 as i32 as Hkey;
const KEY_READ: u32 = 0x2_0019;

// Drive types relevant to the user (UNKNOWN=0 and NO_ROOT_DIR=1 are
// excluded).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4; // mapped network
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Bitmask of mounted letters (bit 0 = A: … 25 = Z:) — instantaneous.
/// Used as a "signature" to detect a drive appearing/disappearing.
pub fn logical_drives_mask() -> u32 {
    unsafe { GetLogicalDrives() }
}

/// Capacity of a volume as `(total, free_for_this_user)`, `(0, 0)` when it
/// cannot be read.
///
/// A drive with no media — an empty card reader slot, an optical drive
/// standing open — is the hazard here: left to itself the call raises a
/// modal asking for a disc, and it can stall while the hardware is polled.
/// `SEM_FAILCRITICALERRORS` turns that into a plain failure, and the mode
/// is restored right after so nothing else inherits it.
fn volume_capacity(root_w: &[u16]) -> (u64, u64) {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    let mut previous_mode: u32 = 0;
    let changed = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut previous_mode) } != 0;

    let (mut free_for_caller, mut total) = (0u64, 0u64);
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            root_w.as_ptr(),
            &mut free_for_caller,
            &mut total,
            std::ptr::null_mut(),
        )
    } != 0;

    if changed {
        unsafe { SetThreadErrorMode(previous_mode, std::ptr::null_mut()) };
    }
    if ok { (total, free_for_caller) } else { (0, 0) }
}

/// Is the letter a mapped NETWORK drive? `GetDriveTypeW` reads the local
/// mount table (no network access, instantaneous).
pub fn drive_is_remote(letter: char) -> bool {
    let root_w = wide(&format!("{letter}:\\"));
    unsafe { GetDriveTypeW(root_w.as_ptr()) == DRIVE_REMOTE }
}

pub fn logical_drives() -> Vec<Place> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let root_w = wide(&root);
        let dtype = unsafe { GetDriveTypeW(root_w.as_ptr()) };
        if !matches!(
            dtype,
            DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_REMOTE | DRIVE_CDROM | DRIVE_RAMDISK
        ) {
            continue;
        }
        let letter_str = format!("{letter}:");
        let is_net = dtype == DRIVE_REMOTE;
        // Mapped network: label "\\server\share (P:)" (resolved target),
        // otherwise fallback "P:". Local: "Label (P:)" or "P:".
        let name = if is_net {
            match mapped_remote(&letter_str) {
                Some(r) => format!("{r} ({letter_str})"),
                None => letter_str.clone(),
            }
        } else {
            match volume_label(&root_w) {
                Some(l) if !l.is_empty() => format!("{l} ({letter_str})"),
                _ => letter_str.clone(),
            }
        };
        // Local volumes only. A mapped network letter is skipped for the
        // same reason a share is on the other platform: the call leaves the
        // machine and can stall, and the answer describes the server.
        let (total_bytes, free_bytes) = if is_net {
            (0, 0)
        } else {
            volume_capacity(&root_w)
        };
        // Ejectable if removable/CD, OR if it's a "fixed" disk but on
        // an external bus (USB/1394/SD/MMC): Windows classifies many
        // USB flash drives/SSDs as DRIVE_FIXED, hence the bus check.
        // Computed once — the bus check is an IOCTL, and two fields read it.
        let ejectable = match dtype {
            DRIVE_REMOVABLE | DRIVE_CDROM => true,
            DRIVE_FIXED => bus_ejectable(letter),
            _ => false, // network, ramdisk
        };
        out.push(Place {
            kind: if is_net {
                PlaceKind::Network
            } else {
                PlaceKind::Drive
            },
            path: PathBuf::from(&root),
            name,
            total_bytes,
            free_bytes,
            removable: ejectable,
            // An optical drive is the one case that parts company with the
            // line above: its disc pops out, but the drive itself stays
            // bolted in — so releasing it is not an invitation to unplug.
            hotplug: ejectable && dtype != DRIVE_CDROM,
            // Eject/disconnect target: the letter (`P:`).
            device: letter_str,
        });
    }
    out
}

/// Is a "fixed" disk actually **ejectable**? The volume's bus is queried
/// via `IOCTL_STORAGE_QUERY_PROPERTY`: USB / 1394 / SD / MMC (or
/// removable media) ⇒ yes. Covers USB flash drives & SSDs that Windows
/// files under `DRIVE_FIXED`. Best-effort: `false` if opening/IOCTL fails.
fn bus_ejectable(letter: char) -> bool {
    let path = wide(&format!(r"\\.\{letter}:"));
    // 0 access rights: the property IOCTL is FILE_ANY_ACCESS.
    let h = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_RW,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        )
    };
    if h == INVALID_HANDLE {
        return false;
    }
    // STORAGE_PROPERTY_QUERY { PropertyId = 0 (StorageDeviceProperty),
    // QueryType = 0 (PropertyStandardQuery), AdditionalParameters }.
    let query: [u32; 3] = [0, 0, 0];
    let mut buf = [0u8; 512];
    let mut ret = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            query.as_ptr() as *mut core::ffi::c_void,
            std::mem::size_of_val(&query) as u32,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            buf.len() as u32,
            &mut ret,
            std::ptr::null_mut(),
        )
    };
    unsafe { CloseHandle(h) };
    if ok == 0 || ret < 32 {
        return false;
    }
    // STORAGE_DEVICE_DESCRIPTOR : RemovableMedia @10 (BOOLEAN), BusType @28 (DWORD).
    let removable_media = buf[10] != 0;
    let bus_type = u32::from_ne_bytes([buf[28], buf[29], buf[30], buf[31]]);
    // External buses: 1394=0x4, USB=0x7, SD=0xC, MMC=0xD.
    removable_media || matches!(bus_type, 0x4 | 0x7 | 0xC | 0xD)
}

/// Target of a mapped network drive (`Z:` → `\\server\share`) via
/// `WNetGetConnectionW`. `None` if not mapped / unavailable.
fn mapped_remote(letter: &str) -> Option<String> {
    let local = wide(letter); // "Z:" (without backslash)
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    let rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) };
    if rc != 0 {
        return None; // NO_ERROR = 0
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
}

/// WSL distributions (including `docker-desktop`) exposed as `\\wsl$\<distro>`.
/// Read from the registry (`HKCU\…\Lxss\<guid>\DistributionName`) — fast,
/// without launching `wsl.exe` (no console flash, no service latency).
pub fn wsl_distros() -> Vec<Place> {
    let mut out = Vec::new();
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Lxss");
    let mut lxss: Hkey = 0;
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut lxss) } != 0 {
        return out; // WSL not installed
    }
    let mut idx = 0u32;
    loop {
        let mut name = [0u16; 128];
        let mut nlen = name.len() as u32;
        let rc = unsafe {
            RegEnumKeyExW(
                lxss,
                idx,
                name.as_mut_ptr(),
                &mut nlen,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            break; // ERROR_NO_MORE_ITEMS
        }
        idx += 1;
        let subname: Vec<u16> = name[..nlen as usize]
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .collect();
        let mut hsub: Hkey = 0;
        if unsafe { RegOpenKeyExW(lxss, subname.as_ptr(), 0, KEY_READ, &mut hsub) } != 0 {
            continue;
        }
        if let Some(distro) = reg_read_sz(hsub, "DistributionName") {
            out.push(Place {
                kind: PlaceKind::Network,
                path: PathBuf::from(format!(r"\\wsl$\{distro}")),
                name: distro,
                removable: false,
                hotplug: false,
                device: String::new(),
                // Reached through a network path: same reasoning as a share.
                total_bytes: 0,
                free_bytes: 0,
            });
        }
        unsafe { RegCloseKey(hsub) };
    }
    unsafe { RegCloseKey(lxss) };
    out
}

/// Reads a `REG_SZ` (string) value. `None` if absent / different type.
fn reg_read_sz(hkey: Hkey, value: &str) -> Option<String> {
    let vw = wide(value);
    let mut buf = [0u16; 260];
    let mut len = (buf.len() * 2) as u32; // bytes
    let rc = unsafe {
        RegQueryValueExW(
            hkey,
            vw.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        )
    };
    if rc != 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
}

fn volume_label(root_w: &[u16]) -> Option<String> {
    let mut buf = [0u16; 261]; // MAX_PATH + 1
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return None; // volume inaccessible (e.g. disconnected network)
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}
