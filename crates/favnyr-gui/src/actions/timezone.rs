/// Offset (seconds) between LOCAL time and UTC, current DST included — to
/// ADD to a Unix (UTC) timestamp to get local time. Used for the
/// "Modified" column. 0 on failure (falls back to UTC).
///
/// Windows: `GetTimeZoneInformation` (bias + active seasonal bias), raw FFI
/// (no extra `windows` feature). Other OSes: libc's `localtime_r`
/// and its `tm_gmtoff` field (glibc/musl/BSD extension).
#[cfg(windows)]
pub fn local_utc_offset_secs() -> i64 {
    #[repr(C)]
    struct SysTime {
        year: u16,
        month: u16,
        dow: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        ms: u16,
    }
    #[repr(C)]
    struct Tzi {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SysTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SysTime,
        daylight_bias: i32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTimeZoneInformation(info: *mut Tzi) -> u32;
    }
    const TIME_ZONE_ID_STANDARD: u32 = 1;
    const TIME_ZONE_ID_DAYLIGHT: u32 = 2;
    const TIME_ZONE_ID_INVALID: u32 = 0xFFFF_FFFF;
    unsafe {
        let mut tzi: Tzi = std::mem::zeroed();
        let r = GetTimeZoneInformation(&mut tzi);
        if r == TIME_ZONE_ID_INVALID {
            return 0;
        }
        let seasonal = match r {
            TIME_ZONE_ID_DAYLIGHT => tzi.daylight_bias,
            TIME_ZONE_ID_STANDARD => tzi.standard_bias,
            _ => 0,
        };
        // Win32 doc: UTC = local + (Bias + seasonal bias) minutes.
        // → offset to add to UTC to get local = −(Bias + seasonal).
        -((tzi.bias + seasonal) as i64) * 60
    }
}

#[cfg(not(windows))]
pub fn local_utc_offset_secs() -> i64 {
    // libc's `struct tm` (x86_64): 9 × int, then `tm_gmtoff` (long, offset 40
    // after padding) and `tm_zone` (ptr). `#[repr(C)]` reproduces this layout.
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const i8,
    }
    unsafe extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, result: *mut Tm) -> *mut Tm;
    }
    unsafe {
        let now = time(std::ptr::null_mut());
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        tm.gmtoff
    }
}
