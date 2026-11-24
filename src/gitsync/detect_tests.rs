//! Tests of the detection of network and cloud-synced folders (SYNC-059…SYNC-062).

use super::detect::*;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-detect-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const MOUNTS: &str = "\
22 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw\n\
40 22 0:35 / /mnt/nas rw,relatime shared:9 - nfs4 nas:/export rw,vers=4.2\n\
41 22 0:36 / /mnt/win\\040share rw,relatime - cifs //srv/share rw\n\
42 22 0:37 / /home/u/enc rw,nosuid - fuse.cryptomator cryptomator rw\n\
43 22 0:38 / /home/u/arch rw,nosuid - fuse arch rw\n\
44 40 0:39 / /mnt/nas/local rw - ext4 /dev/sdb rw\n";

// spec: SYNC-059
#[test]
fn sync_059_the_mount_table_names_network_file_systems_and_the_longest_mount_wins() {
    let inp = Inputs { mountinfo: Some(MOUNTS.into()), ..Inputs::none(Os::Linux) };
    let f = detect(Path::new("/mnt/nas/notes"), &inp);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].kind, Kind::Network { fs: "nfs4".into(), sure: true });
    assert_eq!(f[0].at, Path::new("/mnt/nas"));
    assert_eq!(f[0].what(), "a network folder (nfs4)");
    // a local disk mounted below the share is local
    assert!(detect(Path::new("/mnt/nas/local/notes"), &inp).is_empty());
    assert!(detect(Path::new("/home/u/notes"), &inp).is_empty());
    // octal escapes of the mount point
    let f = detect(Path::new("/mnt/win share/notes"), &inp);
    assert_eq!(f[0].kind, Kind::Network { fs: "cifs".into(), sure: true });
    // fuse: a known network one is sure, a plain fuse only maybe
    assert_eq!(detect(Path::new("/home/u/enc/n"), &inp)[0].kind, Kind::Network { fs: "fuse.cryptomator".into(), sure: true });
    let f = detect(Path::new("/home/u/arch/n"), &inp);
    assert_eq!(f[0].kind, Kind::Network { fs: "fuse".into(), sure: false });
    assert_eq!(f[0].what(), "maybe a network folder (fuse)");
}

// spec: SYNC-059
#[test]
fn sync_059_the_statfs_magic_number_is_used_when_the_mount_table_does_not_say() {
    for (magic, fs) in [(0x6969, "nfs"), (0x517B, "smb"), (0xFF53_4D42, "cifs"), (0xFE53_4D42, "smb2")] {
        let inp = Inputs { magic: Some(magic), ..Inputs::none(Os::Linux) };
        let f = detect(Path::new("/x/notes"), &inp);
        assert_eq!(f[0].kind, Kind::Network { fs: fs.into(), sure: true }, "{magic:#x}");
    }
    // ext4 and tmpfs and btrfs are local
    for magic in [0xEF53, 0x0102_1994, 0x9123_683E] {
        assert!(detect(Path::new("/x/notes"), &Inputs { magic: Some(magic), ..Inputs::none(Os::Linux) }).is_empty(), "{magic:#x}");
    }
    // the mount table wins over the magic number
    let inp = Inputs { mountinfo: Some(MOUNTS.into()), magic: Some(0x6969), ..Inputs::none(Os::Linux) };
    assert!(detect(Path::new("/home/u/notes"), &inp).is_empty());
}

// spec: SYNC-059
#[test]
fn sync_059_macos_names_its_network_file_systems() {
    for fs in ["nfs", "smbfs", "afpfs", "webdav", "webdavfs"] {
        let inp = Inputs { mac_fs: Some(fs.into()), ..Inputs::none(Os::Mac) };
        assert_eq!(detect(Path::new("/Volumes/x/notes"), &inp)[0].kind, Kind::Network { fs: fs.into(), sure: true });
    }
    let inp = Inputs { mac_fs: Some("macfuse".into()), ..Inputs::none(Os::Mac) };
    assert_eq!(detect(Path::new("/Volumes/x"), &inp)[0].kind, Kind::Network { fs: "macfuse".into(), sure: false });
    for fs in ["apfs", "hfs"] {
        assert!(detect(Path::new("/Users/u/notes"), &Inputs { mac_fs: Some(fs.into()), ..Inputs::none(Os::Mac) }).is_empty());
    }
}

// spec: SYNC-059
#[test]
fn sync_059_windows_knows_unc_paths_and_remote_drives() {
    for p in [r"\\server\share\notes", r"\\?\UNC\server\share\notes", "//server/share/notes"] {
        assert!(is_unc(p), "{p}");
        let f = detect(Path::new(p), &Inputs::none(Os::Windows));
        assert_eq!(f[0].kind, Kind::Network { fs: "UNC path".into(), sure: true }, "{p}");
    }
    for p in [r"C:\Users\u\notes", r"\\?\C:\Users\u\notes", r"\\.\C:\x"] {
        assert!(!is_unc(p), "{p}");
        assert!(detect(Path::new(p), &Inputs::none(Os::Windows)).is_empty(), "{p}");
    }
    let remote = Inputs { drive_remote: Some(true), ..Inputs::none(Os::Windows) };
    assert_eq!(detect(Path::new(r"Z:\notes"), &remote)[0].kind, Kind::Network { fs: "network drive".into(), sure: true });
    let local = Inputs { drive_remote: Some(false), ..Inputs::none(Os::Windows) };
    assert!(detect(Path::new(r"C:\notes"), &local).is_empty());
}

// spec: SYNC-060
#[test]
fn sync_060_places_name_cloud_folders_on_macos_and_windows_and_in_dropbox_s_info_file() {
    let mac = Inputs { home: Some("/Users/u".into()), ..Inputs::none(Os::Mac) };
    let f = detect(Path::new("/Users/u/Library/Mobile Documents/com~apple~CloudDocs/notes"), &mac);
    assert_eq!(f[0].kind, Kind::Cloud(Cloud::Apple));
    assert_eq!(f[0].what(), "a iCloud Drive folder");
    for (dir, cloud) in [("OneDrive-Personal", Cloud::OneDrive), ("GoogleDrive-me@x.org", Cloud::GoogleDrive), ("Dropbox", Cloud::Dropbox), ("Box-Box", Cloud::Other("Box".into()))] {
        let p = format!("/Users/u/Library/CloudStorage/{dir}/notes");
        assert_eq!(detect(Path::new(&p), &mac)[0].kind, Kind::Cloud(cloud), "{p}");
    }
    assert!(detect(Path::new("/Users/u/Documents/notes"), &mac).is_empty());
    // the same path on Linux is just a path
    assert!(detect(Path::new("/Users/u/Library/Mobile Documents/x"), &Inputs { home: Some("/Users/u".into()), ..Inputs::none(Os::Linux) }).is_empty());

    let win = Inputs { env: vec![("OneDrive".into(), r"C:\Users\U\OneDrive".into()), ("OneDriveCommercial".into(), r"C:\Users\U\OneDrive - Corp".into())], ..Inputs::none(Os::Windows) };
    assert_eq!(detect(Path::new(r"c:\users\u\onedrive\Notes"), &win)[0].kind, Kind::Cloud(Cloud::OneDrive));
    assert_eq!(detect(Path::new(r"\\?\C:\Users\U\OneDrive - Corp\Notes"), &win)[0].kind, Kind::Cloud(Cloud::OneDrive));
    assert!(detect(Path::new(r"C:\Users\U\OneDriveNot\Notes"), &win).is_empty());
    assert!(detect(Path::new(r"C:\Users\U\Documents"), &win).is_empty());

    let mut roots = dropbox_roots(r#"{"personal": {"path": "/home/u/Dropbox", "host": 1}, "business": {"path": "/home/u/Dropbox (Work)"}}"#);
    roots.sort();
    assert_eq!(roots, vec![PathBuf::from("/home/u/Dropbox"), PathBuf::from("/home/u/Dropbox (Work)")]);
    assert!(dropbox_roots("not json").is_empty() && dropbox_roots("[1]").is_empty());
    let inp = Inputs { dropbox: roots, ..Inputs::none(Os::Linux) };
    assert_eq!(detect(Path::new("/home/u/Dropbox (Work)/a/notes"), &inp)[0].kind, Kind::Cloud(Cloud::Dropbox));
    assert!(detect(Path::new("/home/u/Dropbox2/notes"), &inp).is_empty());
}

// spec: SYNC-060
#[test]
fn sync_060_markers_in_the_vault_or_a_parent_name_the_syncing_app() {
    let root = tmp("markers");
    let cases: [(&str, Option<Cloud>); 9] = [
        (".dropbox", Some(Cloud::Dropbox)),
        (".dropbox.cache", Some(Cloud::Dropbox)),
        (".stfolder", Some(Cloud::Syncthing)),
        (".sync_5a8b2c.db", Some(Cloud::Nextcloud)),
        (".nextcloudsync.log", Some(Cloud::Nextcloud)),
        (".owncloudsync.log", Some(Cloud::Nextcloud)),
        (".csync_journal.db", Some(Cloud::Nextcloud)),
        (".tmp.driveupload", Some(Cloud::GoogleDrive)),
        (".sync_notes.txt", None),
    ];
    for (i, (name, want)) in cases.iter().enumerate() {
        // in a parent of the vault
        let parent = root.join(format!("p{i}"));
        let vault = parent.join("deep").join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(parent.join(name), "").unwrap();
        let f = detect(&vault, &Inputs::none(Os::Linux));
        assert_eq!(f.first().map(|f| f.kind.clone()), want.clone().map(Kind::Cloud), "{name}");
        if want.is_some() {
            assert_eq!(f[0].at, parent, "{name}");
        }
        // in the vault itself
        let own = root.join(format!("v{i}"));
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join(name), "").unwrap();
        assert_eq!(detect(&own, &Inputs::none(Os::Linux)).first().map(|f| f.kind.clone()), want.clone().map(Kind::Cloud), "{name} in the vault");
    }
    // a Syncthing marker in a folder, a marker folder like `.stfolder` is a directory
    let st = root.join("st");
    std::fs::create_dir_all(st.join(".stfolder")).unwrap();
    assert_eq!(detect(&st, &Inputs::none(Os::Linux))[0].kind, Kind::Cloud(Cloud::Syncthing));
    // an iCloud stub counts in the vault, not in a parent
    let ic = root.join("ic");
    std::fs::create_dir_all(ic.join("vault")).unwrap();
    std::fs::write(ic.join(".Plan.md.icloud"), "").unwrap();
    assert!(detect(&ic.join("vault"), &Inputs::none(Os::Linux)).is_empty());
    std::fs::write(ic.join("vault").join(".Plan.md.icloud"), "").unwrap();
    assert_eq!(detect(&ic.join("vault"), &Inputs::none(Os::Linux))[0].kind, Kind::Cloud(Cloud::Apple));
    // two apps, found once each
    let two = root.join("two");
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(two.join(".dropbox"), "").unwrap();
    std::fs::write(two.join(".dropbox.cache"), "").unwrap();
    std::fs::write(two.join(".stfolder"), "").unwrap();
    assert_eq!(detect(&two, &Inputs::none(Os::Linux)).len(), 2);
    // a plain folder, and a missing one
    let plain = root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(detect(&plain, &Inputs::none(Os::Linux)).iter().all(|f| matches!(f.kind, Kind::Cloud(_))));
    assert!(detect(&root.join("missing").join("vault"), &Inputs::none(Os::Linux)).iter().all(|f| matches!(f.kind, Kind::Cloud(_))));
    let _ = std::fs::remove_dir_all(&root);
}

// spec: SYNC-059, SYNC-060
#[test]
fn sync_060_the_real_inputs_see_a_marker_in_a_real_folder() {
    let root = tmp("real");
    std::fs::create_dir_all(root.join(".stfolder")).unwrap();
    let f = detect_real(&root);
    assert!(f.iter().any(|f| f.kind == Kind::Cloud(Cloud::Syncthing)), "{f:?}");
    let _ = std::fs::remove_dir_all(&root);
}

// spec: SYNC-061, SYNC-062, NET-017
#[test]
fn sync_062_the_warnings_name_the_folder_and_what_to_do() {
    assert_eq!(warning(&[]), None);
    assert_eq!(repo_warning(&[], false), None);
    let cloud = vec![Finding { kind: Kind::Cloud(Cloud::Dropbox), at: "/h/Dropbox".into() }];
    let w = warning(&cloud).unwrap();
    assert!(w.contains("a Dropbox folder") && w.contains("history") && w.contains("only warns"), "{w}");
    let net = vec![Finding { kind: Kind::Network { fs: "nfs4".into(), sure: true }, at: "/mnt".into() }];
    assert!(warning(&net).unwrap().contains("network folder (nfs4)") && warning(&net).unwrap().contains("slow"));
    let no_server = repo_warning(&cloud, false).unwrap();
    assert!(no_server.contains(".git") && no_server.contains("move the repository out"), "{no_server}");
    let server = repo_warning(&cloud, true).unwrap();
    assert!(server.contains("sync with the server") && !server.contains("move the repository out"), "{server}");
}
