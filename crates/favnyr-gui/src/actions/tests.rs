#[cfg(test)]
mod ffmpeg_tests {
    use crate::actions::ffmpeg::distro_index_from;

    #[test]
    fn distro_resolution_covers_id_and_id_like() {
        // Direct ID.
        assert_eq!(distro_index_from("ID=ubuntu\n"), 0);
        assert_eq!(distro_index_from("ID=fedora\n"), 1);
        assert_eq!(distro_index_from("ID=arch\n"), 2);
        assert_eq!(distro_index_from("ID=alpine\n"), 3);
        assert_eq!(distro_index_from("ID=manjaro\n"), 2);
        // Derivatives via ID_LIKE (Mint -> apt, Nobara -> dnf).
        assert_eq!(
            distro_index_from("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"),
            0
        );
        assert_eq!(distro_index_from("ID=nobara\nID_LIKE=fedora\n"), 1);
        assert_eq!(distro_index_from("ID=\"endeavouros\"\nID_LIKE=arch\n"), 2);
        // Quotes, case, ID priority over ID_LIKE.
        assert_eq!(distro_index_from("ID=Debian\nID_LIKE=whatever\n"), 0);
        // Unknown / empty.
        assert_eq!(distro_index_from("ID=void\n"), -1);
        assert_eq!(distro_index_from(""), -1);
    }
}

#[cfg(all(test, windows))]
mod image_activation_tests {
    use crate::actions::opening::{is_microsoft_photos_app_id, photos_gallery_target};

    #[test]
    fn photos_handler_detection_is_specific_and_case_insensitive() {
        assert!(is_microsoft_photos_app_id(
            "Microsoft.Windows.Photos_8wekyb3d8bbwe!App"
        ));
        assert!(is_microsoft_photos_app_id(
            "microsoft.windows.photos_8WEKYB3D8BBWE!app"
        ));
        assert!(!is_microsoft_photos_app_id(
            "Contoso.Windows.Photos_8wekyb3d8bbwe!App"
        ));
        assert!(!is_microsoft_photos_app_id(
            "Microsoft.Paint_8wekyb3d8bbwe!App"
        ));
        assert!(!is_microsoft_photos_app_id(
            "Microsoft.Windows.Photos_fakepublisher!App"
        ));
    }

    #[test]
    fn photos_gallery_protocol_encodes_query_delimiters_and_unicode() {
        let target =
            photos_gallery_target(std::path::Path::new(r"C:\Photos café\image + #1 %20.png"))
                .unwrap();
        assert_eq!(
            target.to_string_lossy(),
            r"ms-photos:viewer?fileName=C:\Photos%20caf%C3%A9\image%20%2B%20%231%20%2520.png"
        );
    }

    #[test]
    fn photos_gallery_protocol_rejects_relative_and_non_unicode_paths() {
        use std::os::windows::ffi::OsStringExt;

        assert!(photos_gallery_target(std::path::Path::new("relative.png")).is_err());

        let invalid = std::ffi::OsString::from_wide(&[
            b'C' as u16,
            b':' as u16,
            b'\\' as u16,
            0xd800,
            b'.' as u16,
            b'p' as u16,
            b'n' as u16,
            b'g' as u16,
        ]);
        assert!(photos_gallery_target(std::path::Path::new(&invalid)).is_err());
    }
}

#[cfg(all(test, windows))]
mod terminal_tests {
    use crate::actions::terminal::{command_prompt_command, windows_terminal_command};
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn terminal_commands_do_not_use_an_intermediate_cmd_start() {
        let cwd = Path::new(r"C:\Work folder");

        let wt = windows_terminal_command(cwd);
        assert_eq!(wt.get_program(), OsStr::new("wt.exe"));
        assert_eq!(
            wt.get_args().collect::<Vec<_>>(),
            [OsStr::new("-d"), cwd.as_os_str()]
        );

        let cmd = command_prompt_command(cwd);
        assert_eq!(cmd.get_program(), OsStr::new("cmd.exe"));
        assert_eq!(cmd.get_args().count(), 0);
        assert_eq!(cmd.get_current_dir(), Some(cwd));
    }
}

#[cfg(all(test, unix))]
mod program_tests {
    use crate::actions::opening::{is_desktop_launcher, is_launchable_program};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "favnyr-launchable-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"#!/bin/sh\ntrue\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn only_real_programs_bypass_the_desktop_opener() {
        let dir = scratch();

        // A binary without an extension with its bit set: the everyday case
        // this bypass exists for.
        assert!(is_launchable_program(&write(&dir, "sample-app", 0o755)));
        assert!(is_launchable_program(&write(&dir, "run.sh", 0o755)));

        // Same file without the bit: it is not a program, the desktop opener
        // stays in charge of finding it an editor.
        assert!(!is_launchable_program(&write(&dir, "notes", 0o644)));

        // A data file on a FAT/NTFS mount reports 0777. Executing it would be
        // absurd, so the type is what decides, not the bit.
        for data in ["photo.jpg", "clip.mp4", "report.pdf", "archive.zip"] {
            assert!(
                !is_launchable_program(&write(&dir, data, 0o777)),
                "{data} must never be run"
            );
        }

        // A launcher description, not a program: only the desktop knows how to
        // act on it, so it keeps going through the normal opener.
        assert!(!is_launchable_program(&write(&dir, "app.desktop", 0o755)));

        // A folder is never a program, whatever it is called.
        let folder = dir.join("tools.sh");
        std::fs::create_dir(&folder).unwrap();
        assert!(!is_launchable_program(&folder));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_launcher_is_recognised_whether_or_not_it_carries_the_executable_bit() {
        let dir = scratch();

        // The regression this guards. Requiring the bit sent a launcher that
        // did not carry it to the generic opener, which showed the entry's
        // source text in an editor instead of starting the application. The
        // desktop's own rule never consults the bit for a launcher.
        assert!(is_desktop_launcher(&write(
            &dir,
            "Sample App 5.desktop",
            0o644
        )));
        assert!(is_desktop_launcher(&write(&dir, "game.desktop", 0o755)));
        // Matched on the extension alone, whatever its case.
        assert!(is_desktop_launcher(&write(&dir, "App.DESKTOP", 0o644)));

        // Anything else is not a launcher, bit or no bit.
        assert!(!is_desktop_launcher(&write(&dir, "notes.txt", 0o644)));
        assert!(!is_desktop_launcher(&write(&dir, "runner", 0o755)));

        // And a launcher is never mistaken for a native program: running the
        // entry as a script would do the wrong thing, whoever calls.
        assert!(!is_launchable_program(&write(&dir, "run.desktop", 0o755)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_link_is_judged_on_the_binary_it_points_at() {
        let dir = scratch();
        let real = write(&dir, "engine", 0o755);
        let link = dir.join("engine-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(is_launchable_program(&link));

        // A link to something that is not executable stays a plain document.
        let plain = write(&dir, "readme", 0o644);
        let plain_link = dir.join("readme-link");
        std::os::unix::fs::symlink(&plain, &plain_link).unwrap();
        assert!(!is_launchable_program(&plain_link));

        std::fs::remove_dir_all(&dir).ok();
    }
}
