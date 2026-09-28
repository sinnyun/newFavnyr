use super::*;

#[test]
fn user_places_includes_existing_home() {
    let places = user_places();
    // The home folder always exists in a test environment.
    if dirs::home_dir().map(|h| h.is_dir()).unwrap_or(false) {
        assert!(places.iter().any(|p| p.kind == PlaceKind::Home));
    }
    // All returned paths exist and are folders.
    for p in &places {
        assert!(
            p.path.is_dir(),
            "{} should be a directory",
            p.path.display()
        );
    }
}

#[test]
fn drives_are_deduplicated_and_grouped() {
    let d = drives();
    // No duplicate mount point.
    for i in 1..d.len() {
        assert_ne!(d[i - 1].path, d[i].path, "no duplicate mount point");
    }
    // Only local or network drives.
    assert!(
        d.iter()
            .all(|p| matches!(p.kind, PlaceKind::Drive | PlaceKind::Network))
    );
    // Local ones come before network (grouped sections), and each family
    // is sorted by path.
    let first_net = d.iter().position(|p| p.kind == PlaceKind::Network);
    if let Some(k) = first_net {
        assert!(
            d[k..].iter().all(|p| p.kind == PlaceKind::Network),
            "all network places must be grouped at the end"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn network_fs_and_removable_detection() {
    assert!(is_network_fs("nfs4"));
    assert!(is_network_fs("cifs"));
    assert!(is_network_fs("fuse.sshfs"));
    assert!(!is_network_fs("ext4"));
    assert!(!is_network_fs("vfat"));
    assert!(is_removable_mount(Path::new("/run/media/u/USB")));
    assert!(is_removable_mount(Path::new("/media/u/Stick")));
    assert!(!is_removable_mount(Path::new("/")));
    assert!(!is_removable_mount(Path::new("/home/u")));
}

#[cfg(target_os = "linux")]
#[test]
fn gvfs_names_are_prettified() {
    assert_eq!(
        gvfs_pretty_name("smb-share:server=nas,share=media"),
        "media (nas)"
    );
    assert_eq!(
        gvfs_pretty_name("sftp:host=host.tld,user=me"),
        "host.tld (sftp)"
    );
    // Unknown format → returned as-is.
    assert_eq!(gvfs_pretty_name("weird-thing"), "weird-thing");
}

#[cfg(target_os = "linux")]
#[test]
fn the_gvfs_root_follows_the_session_when_it_declares_one() {
    assert_eq!(
        gvfs_root_in(Some(Path::new("/run/user/4242")), 7),
        PathBuf::from("/run/user/4242/gvfs")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn the_gvfs_root_falls_back_to_the_conventional_path() {
    assert_eq!(gvfs_root_in(None, 7), PathBuf::from("/run/user/7/gvfs"));
}

/// Inventory row with only the fields a test cares about.
#[cfg(target_os = "linux")]
fn row(name: &str, kind: &str, fs: &str, mount: &str) -> BlockRow {
    BlockRow {
        name: name.to_string(),
        kind: kind.to_string(),
        fs_type: fs.to_string(),
        mountpoint: mount.to_string(),
        ..BlockRow::default()
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_quoted_pair_line_is_split_into_its_fields() {
    let pairs = parse_pairs(r#"NAME="sda1" TYPE="part" SIZE="1024" LABEL="Volume01""#);
    assert_eq!(
        pairs,
        vec![
            ("NAME".to_string(), "sda1".to_string()),
            ("TYPE".to_string(), "part".to_string()),
            ("SIZE".to_string(), "1024".to_string()),
            ("LABEL".to_string(), "Volume01".to_string()),
        ]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_label_may_carry_an_escaped_quote() {
    let pairs = parse_pairs(r#"NAME="sda1" LABEL="a\"b" TYPE="part""#);
    assert_eq!(pairs[1].1, r#"a"b"#);
    // The row after the escape is still read: the scan did not lose its place.
    assert_eq!(pairs[2], ("TYPE".to_string(), "part".to_string()));
}

#[cfg(target_os = "linux")]
#[test]
fn a_row_is_built_with_its_size_in_bytes() {
    let rows = parse_block_rows(
        r#"NAME="sda1" PKNAME="sda" TYPE="part" FSTYPE="ext4" MOUNTPOINT="" RM="1" SIZE="2048" LABEL="Volume01" PARTTYPENAME="Linux filesystem""#,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].size, 2048);
    assert!(rows[0].removable);
    assert_eq!(rows[0].parent, "sda");
}

#[cfg(target_os = "linux")]
#[test]
fn an_unmounted_partition_with_a_filesystem_is_offered() {
    let rows = vec![row("sda1", "part", "ext4", "")];
    assert_eq!(volume_kind(&rows[0], &rows), Some(PlaceKind::Volume));
}

#[cfg(target_os = "linux")]
#[test]
fn an_already_mounted_partition_is_left_to_the_mount_scan() {
    let rows = vec![row("sda1", "part", "ext4", "/mnt/somewhere")];
    assert_eq!(volume_kind(&rows[0], &rows), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_partition_without_a_filesystem_is_not_offered() {
    let rows = vec![row("sda1", "part", "", "")];
    assert_eq!(volume_kind(&rows[0], &rows), None);
}

#[cfg(target_os = "linux")]
#[test]
fn bookkeeping_filesystems_are_not_offered() {
    for fs in ["swap", "LVM2_member", "linux_raid_member", "squashfs"] {
        let rows = vec![row("sda1", "part", fs, "")];
        assert_eq!(volume_kind(&rows[0], &rows), None, "{fs} should be hidden");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_firmware_partition_is_not_offered() {
    let mut efi = row("sda1", "part", "vfat", "");
    efi.part_type = "EFI System".to_string();
    let rows = vec![efi];
    assert_eq!(volume_kind(&rows[0], &rows), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_whole_disk_carrying_a_filesystem_is_offered() {
    let rows = vec![row("sdb", "disk", "vfat", "")];
    assert_eq!(volume_kind(&rows[0], &rows), Some(PlaceKind::Volume));
}

#[cfg(target_os = "linux")]
#[test]
fn a_partitioned_disk_yields_its_partitions_not_itself() {
    let mut child = row("sdb1", "part", "ext4", "");
    child.parent = "sdb".to_string();
    let rows = vec![row("sdb", "disk", "vfat", ""), child];
    assert_eq!(volume_kind(&rows[0], &rows), None);
    assert_eq!(volume_kind(&rows[1], &rows), Some(PlaceKind::Volume));
}

#[cfg(target_os = "linux")]
#[test]
fn a_volume_is_named_by_its_label_and_falls_back_to_its_device() {
    let mut labelled = row("sda1", "part", "ext4", "");
    labelled.label = "Volume01".to_string();
    assert_eq!(volume_place(&labelled, PlaceKind::Volume).name, "Volume01");
    assert_eq!(
        volume_place(&row("sda1", "part", "ext4", ""), PlaceKind::Volume).name,
        "sda1"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_volume_carries_its_device_and_no_path() {
    let place = volume_place(&row("sda1", "part", "ext4", ""), PlaceKind::Volume);
    assert_eq!(place.kind, PlaceKind::Volume);
    assert_eq!(place.device, "/dev/sda1");
    assert_eq!(place.path, PathBuf::new());
    // No gauge is possible before the kernel has the filesystem.
    assert_eq!(place.free_bytes, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn a_locked_encrypted_container_is_shown_as_locked() {
    let rows = vec![row("sda1", "part", "crypto_LUKS", "")];
    assert_eq!(volume_kind(&rows[0], &rows), Some(PlaceKind::LockedVolume));
}

#[cfg(target_os = "linux")]
#[test]
fn an_unlocked_container_steps_aside_for_the_volume_it_exposes() {
    let mut opened = row("dm-0", "crypt", "ext4", "");
    opened.parent = "sda1".to_string();
    let rows = vec![row("sda1", "part", "crypto_LUKS", ""), opened];
    // The container has nothing left to offer once it has been opened…
    assert_eq!(volume_kind(&rows[0], &rows), None);
    // …and what it exposes is an ordinary volume, mountable like any other.
    assert_eq!(volume_kind(&rows[1], &rows), Some(PlaceKind::Volume));
}

#[cfg(target_os = "linux")]
#[test]
fn an_unlocked_container_whose_volume_is_mounted_shows_neither() {
    let mut opened = row("dm-0", "crypt", "ext4", "/mnt/somewhere");
    opened.parent = "sda1".to_string();
    let rows = vec![row("sda1", "part", "crypto_LUKS", ""), opened];
    assert_eq!(volume_kind(&rows[0], &rows), None);
    assert_eq!(volume_kind(&rows[1], &rows), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_hotplug_bus_is_recognised_in_a_resolved_device_path() {
    // Shapes taken from a real sysfs resolution.
    assert!(path_traverses_hotplug_bus(
        "/sys/devices/pci0000:00/0000:00:14.0/usb4/4-1/4-1:1.0/host8/target8:0:0/8:0:0:0/block/sdi"
    ));
    assert!(path_traverses_hotplug_bus(
        "/sys/devices/platform/soc/mmc_host/mmc0/mmc0:0001/block/mmcblk0"
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn an_internal_bus_is_not_a_hotplug_one() {
    // The same disk family, wired to the motherboard: releasing it must not
    // claim the cable can be pulled.
    assert!(!path_traverses_hotplug_bus(
        "/sys/devices/pci0000:00/0000:00:17.0/ata6/host5/target5:0:0/5:0:0:0/block/sdd"
    ));
    assert!(!path_traverses_hotplug_bus(
        "/sys/devices/pci0000:00/0000:00:1d.0/nvme/nvme0/nvme0n1"
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn a_locked_container_keeps_its_size_and_its_device() {
    let mut locked = row("sda1", "part", "crypto_LUKS", "");
    locked.size = 4096;
    let place = volume_place(&locked, PlaceKind::LockedVolume);
    assert_eq!(place.kind, PlaceKind::LockedVolume);
    assert_eq!(place.device, "/dev/sda1");
    assert_eq!(place.total_bytes, 4096);
}

#[test]
fn trash_place_kind() {
    assert_eq!(trash_place().kind, PlaceKind::Trash);
}

#[cfg(not(windows))]
#[test]
fn drives_always_include_root() {
    assert!(
        drives().iter().any(|p| p.path == Path::new("/")),
        "the / root must always be present"
    );
}

#[cfg(not(windows))]
#[test]
fn filters_pseudo_filesystems() {
    assert!(!is_user_facing_mount(Path::new("/proc"), "proc"));
    assert!(!is_user_facing_mount(
        Path::new("/snap/core/123"),
        "squashfs"
    ));
    assert!(is_user_facing_mount(Path::new("/"), "ext4"));
    assert!(is_user_facing_mount(Path::new("/run/media/u/USB"), "vfat"));
}
