//! The shortcuts on the side of Shale's own picker (PICK-006): the home folder, the folders of the
//! person's own account (Desktop, Documents, Downloads, Pictures, Videos, Music) that are there,
//! the vaults, the library folders and the drives or mounted volumes. Looking at them touches the
//! disk (a network folder can take seconds to answer), so [`scan`] runs on a worker thread.

use std::path::{Path, PathBuf};

/// Which heading a shortcut is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// the home folder and the folders of the account
    Places,
    Vaults,
    Libraries,
    /// the drives (Windows) or the mounted volumes
    Drives,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Places => "Places",
            Group::Vaults => "Vaults",
            Group::Libraries => "Libraries",
            Group::Drives => "Drives",
        }
    }
}

/// One shortcut: a name and the folder it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
    pub group: Group,
}

/// What the shortcuts are made from; [`scan`] keeps what is there.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub home: Option<PathBuf>,
    /// `(name, folder)` of the account's folders: Desktop, Documents, …
    pub user: Vec<(&'static str, Option<PathBuf>)>,
    /// the known vaults (VAULT-029), the open one first
    pub vaults: Vec<(String, PathBuf)>,
    /// the library folders (MEDIA-014), by their name after the `@`
    pub libraries: Vec<(String, PathBuf)>,
    /// folders whose subfolders are mounted volumes (`/media/<user>`, `/Volumes`)
    pub volume_roots: Vec<PathBuf>,
    /// the root of the file system, on systems that have one
    pub root: Option<PathBuf>,
    /// the letters of the drives to look for (Windows)
    pub drive_letters: Vec<char>,
}

impl Inputs {
    /// The sources of this computer: the account's folders from `userdirs` (DEV-047), the volumes
    /// where this system mounts them, the drive letters on Windows.
    pub fn of_this_computer(vaults: Vec<(String, PathBuf)>, libraries: Vec<(String, PathBuf)>) -> Inputs {
        use crate::userdirs as u;
        let user = vec![("Desktop", u::desktop_dir()), ("Documents", u::document_dir()), ("Downloads", u::download_dir()), ("Pictures", u::picture_dir()), ("Videos", u::video_dir()), ("Music", u::audio_dir())];
        let name = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
        let volume_roots = if cfg!(target_os = "macos") {
            vec![PathBuf::from("/Volumes")]
        } else if cfg!(windows) {
            vec![]
        } else {
            vec![PathBuf::from("/run/media").join(&name), PathBuf::from("/media").join(&name), PathBuf::from("/mnt")]
        };
        Inputs { home: u::home_dir(), user, vaults, libraries, volume_roots, root: (!cfg!(windows)).then(|| PathBuf::from("/")), drive_letters: if cfg!(windows) { ('A'..='Z').collect() } else { vec![] } }
    }
}

/// The shortcuts that lead somewhere: a folder that is not there is left out, and so is a second
/// shortcut to a folder that has one already.
pub fn scan(inputs: &Inputs) -> Vec<Place> {
    scan_with(inputs, |p| p.is_dir())
}

/// [`scan`] with the look at the disk given (a test pretends drives).
pub fn scan_with(inputs: &Inputs, is_dir: impl Fn(&Path) -> bool) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut add = |label: String, path: PathBuf, group: Group| {
        if is_dir(&path) && !out.iter().any(|p| p.path == path && p.group == group) {
            out.push(Place { label, path, group });
        }
    };
    if let Some(h) = &inputs.home {
        add("Home".into(), h.clone(), Group::Places);
    }
    for (name, dir) in &inputs.user {
        if let Some(d) = dir.as_ref().filter(|d| Some(*d) != inputs.home.as_ref()) {
            add((*name).into(), d.clone(), Group::Places);
        }
    }
    for (name, path) in &inputs.vaults {
        add(name.clone(), path.clone(), Group::Vaults);
    }
    for (name, path) in &inputs.libraries {
        add(format!("@{name}"), path.clone(), Group::Libraries);
    }
    if let Some(r) = &inputs.root {
        add("File system".into(), r.clone(), Group::Drives);
    }
    for letter in &inputs.drive_letters {
        add(format!("{letter}:"), PathBuf::from(format!("{letter}:\\")), Group::Drives);
    }
    for root in &inputs.volume_roots {
        let mut names: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
        names.sort_by_key(|n| n.to_lowercase());
        for n in names {
            add(n.clone(), root.join(n), Group::Drives);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-pick-places-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: PICK-006
    #[test]
    fn pick_006_the_account_folders_that_are_there_are_listed_under_the_home_folder() {
        let d = tmp("user");
        for n in ["home", "home/Documents", "home/Pictures", "elsewhere/Videos"] {
            std::fs::create_dir_all(d.join(n)).unwrap();
        }
        let inputs = Inputs {
            home: Some(d.join("home")),
            user: vec![
                ("Desktop", Some(d.join("home/Desktop"))),
                ("Documents", Some(d.join("home/Documents"))),
                ("Downloads", None),
                ("Pictures", Some(d.join("home/Pictures"))),
                ("Videos", Some(d.join("elsewhere/Videos"))),
                ("Music", Some(d.join("home"))),
            ],
            ..Inputs::default()
        };
        let got: Vec<(String, PathBuf)> = scan(&inputs).into_iter().map(|p| (p.label, p.path)).collect();
        let want = [("Home", "home"), ("Documents", "home/Documents"), ("Pictures", "home/Pictures"), ("Videos", "elsewhere/Videos")];
        assert_eq!(got, want.iter().map(|(l, p)| (l.to_string(), d.join(p))).collect::<Vec<_>>(), "Desktop is not there, Downloads is not set, Music is the home folder again");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PICK-006
    #[test]
    fn pick_006_vaults_libraries_and_volumes_are_listed_and_a_missing_folder_is_left_out() {
        let d = tmp("more");
        for n in ["vault-a", "lib", "mnt/Stick", "mnt/Backup"] {
            std::fs::create_dir_all(d.join(n)).unwrap();
        }
        std::fs::create_dir_all(d.join("mnt/.hidden")).unwrap();
        std::fs::write(d.join("mnt/file"), "x").unwrap();
        let inputs = Inputs {
            vaults: vec![("Garden".into(), d.join("vault-a")), ("Gone".into(), d.join("vault-gone"))],
            libraries: vec![("Videos".into(), d.join("lib"))],
            volume_roots: vec![d.join("mnt"), d.join("no-such-root")],
            ..Inputs::default()
        };
        let got: Vec<(Group, String)> = scan(&inputs).into_iter().map(|p| (p.group, p.label)).collect();
        assert_eq!(got, [(Group::Vaults, "Garden".to_string()), (Group::Libraries, "@Videos".to_string()), (Group::Drives, "Backup".to_string()), (Group::Drives, "Stick".to_string())]);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PICK-006
    #[test]
    fn pick_006_the_drives_of_windows_are_the_letters_that_answer() {
        let inputs = Inputs { drive_letters: ('A'..='Z').collect(), ..Inputs::default() };
        let got: Vec<String> = scan_with(&inputs, |p| p.to_string_lossy() == "C:\\" || p.to_string_lossy() == "D:\\").into_iter().map(|p| p.label).collect();
        assert_eq!(got, ["C:", "D:"]);
    }
}
