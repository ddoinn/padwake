//! The folder side of the library, ported from `extras/antcloud/sync.ps1`: which folders are games, which exe
//! starts each one, when a new folder is quiet enough to join, and stable ids. No network here.

use super::Game;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Exe names that are never the game (sync.ps1's `$skipExe`, matched from the start of the lower-case name).
const SKIP_EXE_PREFIX: [&str; 22] = [
    "unins", "language", "config", "setup", "install", "uninstall", "vcredist", "vc_redist", "dxwebsetup", "dxsetup",
    "quicksfv", "crs-handler", "crashpad", "ue4prereq", "update", "patch", "7z", "python", "easyanticheat", "eac",
    "beservice", "be_setup",
];
/// The `.*word` parts of `$skipExe`.
const SKIP_EXE_CONTAINS: [&str; 4] = ["selector", "crash", "report", "prereq"];
/// Folders whose exes are never the game (sync.ps1's `$skipDir`). Checked below the game folder only, so a games root
/// like `D:\tools\games` doesn't hide everything.
const SKIP_DIRS: [&str; 10] =
    ["_redist", "redist", "_commonredist", "directx", "nodvd", "bonus content", "support", "tool", "tools", "crack"];

pub fn skip_exe(file_name: &str) -> bool {
    let n = file_name.to_lowercase();
    SKIP_EXE_PREFIX.iter().any(|p| n.starts_with(p)) || SKIP_EXE_CONTAINS.iter().any(|c| n.contains(c))
}

fn skip_dir(dir_name: &str) -> bool {
    SKIP_DIRS.contains(&dir_name.to_lowercase().as_str())
}

/// The game's exe: the biggest .exe in the folder or 2 levels below it that isn't an installer, redist or tool.
pub fn pick_exe(folder: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    let mut dirs = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if depth < 2 && !skip_dir(&name) {
                    dirs.push((e.path(), depth + 1));
                }
            } else if kind.is_file() && name.to_lowercase().ends_with(".exe") && !skip_exe(&name) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                // ties go to the shorter path, so the pick doesn't depend on read_dir order
                let better = match &best {
                    None => true,
                    Some((s, p)) => size > *s || (size == *s && e.path() < *p),
                };
                if better {
                    best = Some((size, e.path()));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// True when the folder has files and none of them was created or changed within `quiet`. Copies keep their old
/// modified time but get a new creation time, so both count.
pub fn is_quiet(folder: &Path, now: SystemTime, quiet: Duration) -> bool {
    let cut = now.checked_sub(quiet).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut any = false;
    let mut dirs = vec![folder.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { return false };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                dirs.push(e.path());
            } else if kind.is_file() {
                let Ok(m) = e.metadata() else { return false };
                let recent = |t: std::io::Result<SystemTime>| t.map(|t| t > cut).unwrap_or(false);
                if recent(m.created()) || recent(m.modified()) {
                    return false;
                }
                any = true;
            }
        }
    }
    any
}

/// The game folders in the root, A–Z. `None` when the root can't be read (a missing drive must not empty the library).
pub fn folders(root: &Path) -> Option<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    v.sort_by_key(|p| key(p));
    Some(v)
}

/// A folder's identity: Windows paths ignore case.
pub fn key(folder: &Path) -> String {
    folder.to_string_lossy().trim_end_matches('\\').to_lowercase()
}

pub fn slug(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "game".into()
    } else {
        s
    }
}

/// What one pass over the games root changes. `games` is the new list; the rest is for the log.
#[derive(Default)]
pub struct Pass {
    pub games: Vec<Game>,
    pub joined: Vec<String>,
    pub left: Vec<Game>,
    pub exe_changed: Vec<String>,
    /// new folders still being written (checked again next tick)
    pub waiting: Vec<PathBuf>,
    pub no_exe: Vec<PathBuf>,
}

impl Pass {
    pub fn changed(&self) -> bool {
        !self.joined.is_empty() || !self.left.is_empty() || !self.exe_changed.is_empty()
    }
}

/// Matches the folders against the library. Known folders keep their entry (and id); quiet new folders with an exe
/// join, taking back the id and playtime of a played game that was removed from the same folder (`kept`); entries
/// whose folder is gone leave. An unplayed game's exe is picked again, as sync.ps1 does for games added mid-install.
pub fn pass(known: &[Game], kept: &[Game], folders: &[PathBuf], now: SystemTime, quiet: Duration) -> Pass {
    let mut by_folder: HashMap<String, &Game> = known.iter().map(|g| (key(&g.folder), g)).collect();
    let mut taken: HashSet<String> = known.iter().chain(kept).map(|g| g.id.clone()).collect();
    let mut p = Pass::default();
    for folder in folders {
        if let Some(g) = by_folder.remove(&key(folder)) {
            let mut g = g.clone();
            if g.playtime_sec == 0 && g.last_played.is_none() {
                if let Some(exe) = pick_exe(folder).filter(|e| *e != g.exe) {
                    g.exe = exe;
                    p.exe_changed.push(g.id.clone());
                }
            }
            p.games.push(g);
            continue;
        }
        if !is_quiet(folder, now, quiet) {
            p.waiting.push(folder.clone());
            continue;
        }
        let Some(exe) = pick_exe(folder) else {
            p.no_exe.push(folder.clone());
            continue;
        };
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let back = kept.iter().find(|k| key(&k.folder) == key(folder));
        let id = match back {
            Some(k) => k.id.clone(),
            None => unique(&slug(&name), &taken),
        };
        taken.insert(id.clone());
        let mut g = Game { id, name, folder: folder.clone(), exe, added_at: Some(ms(now)), ..Default::default() };
        if let Some(k) = back {
            g.playtime_sec = k.playtime_sec;
            g.last_played = k.last_played;
        }
        p.joined.push(g.id.clone());
        p.games.push(g);
    }
    p.left = known.iter().filter(|g| by_folder.contains_key(&key(&g.folder))).cloned().collect();
    p
}

fn unique(base: &str, taken: &HashSet<String>) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

pub fn ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, FileTimes};
    use std::os::windows::fs::FileTimesExt;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Tmp {
            let p = std::env::temp_dir().join(format!("ch-lib-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn file(&self, rel: &str, size: usize) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, vec![0u8; size]).unwrap();
            p
        }
        /// every file in the folder made and changed `ago` before now
        fn age(&self, rel: &str, ago: Duration) {
            let t = SystemTime::now() - ago;
            let mut dirs = vec![self.0.join(rel)];
            while let Some(d) = dirs.pop() {
                for e in std::fs::read_dir(d).unwrap().flatten() {
                    if e.file_type().unwrap().is_dir() {
                        dirs.push(e.path());
                    } else {
                        let f = File::options().write(true).open(e.path()).unwrap();
                        f.set_times(FileTimes::new().set_modified(t).set_created(t)).unwrap();
                    }
                }
            }
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exe_skip_list() {
        for n in ["unins000.exe", "UnityCrashHandler64.exe", "Language Selector.exe", "vc_redist.x64.exe", "VCREDIST.exe",
            "dxwebsetup.exe", "CrashReportClient.exe", "UE4PrereqSetup_x64.exe", "EasyAntiCheat_Setup.exe", "BEService.exe",
            "be_setup.exe", "Updater.exe", "python.exe", "7za.exe", "config.exe", "Installer.exe"]
        {
            assert!(skip_exe(n), "{n} should be skipped");
        }
        for n in ["Hades.exe", "eldenring.exe", "Cyberpunk2077.exe", "Celeste.exe", "sekiro.exe", "RDR2.exe"] {
            assert!(!skip_exe(n), "{n} should be kept");
        }
    }

    #[test]
    fn exe_is_the_biggest_outside_the_skip_lists() {
        let t = Tmp::new("exe");
        t.file("G/launcher.exe", 10);
        t.file("G/unins000.exe", 900); // skipped by name
        t.file("G/_CommonRedist/DirectX/big.exe", 5000); // skipped by folder
        t.file("G/bin/x64/game.exe", 400); // 2 levels down: found
        t.file("G/a/b/c/deep.exe", 9000); // 3 levels down: too deep
        assert_eq!(pick_exe(&t.0.join("G")).unwrap(), t.0.join("G/bin/x64/game.exe"));
        t.file("G/Tools/tool.exe", 8000);
        assert_eq!(pick_exe(&t.0.join("G")).unwrap(), t.0.join("G/bin/x64/game.exe"));
        t.file("H/readme.txt", 1);
        assert_eq!(pick_exe(&t.0.join("H")), None);
    }

    #[test]
    fn a_new_folder_joins_after_its_quiet_time() {
        let t = Tmp::new("quiet");
        let q = Duration::from_secs(180);
        std::fs::create_dir_all(t.0.join("Empty")).unwrap();
        assert!(!is_quiet(&t.0.join("Empty"), SystemTime::now(), q), "no files yet");
        t.file("New/data/pak0.bin", 10);
        t.file("New/game.exe", 10);
        assert!(!is_quiet(&t.0.join("New"), SystemTime::now(), q), "just written");
        assert!(is_quiet(&t.0.join("New"), SystemTime::now(), Duration::ZERO));
        t.age("New", Duration::from_secs(200));
        assert!(is_quiet(&t.0.join("New"), SystemTime::now(), q));
        // a copy keeps the old modified time but gets a new creation time
        let f = t.file("New/data/pak1.bin", 10);
        let old = SystemTime::now() - Duration::from_secs(86_400);
        File::options().write(true).open(&f).unwrap().set_times(FileTimes::new().set_modified(old)).unwrap();
        assert!(!is_quiet(&t.0.join("New"), SystemTime::now(), q));

        let folders = folders(&t.0).unwrap();
        let p = pass(&[], &[], &folders, SystemTime::now(), q);
        assert!(p.games.is_empty() && !p.changed());
        assert_eq!(p.waiting.len(), 2); // Empty and New
        t.age("New", Duration::from_secs(200));
        let p = pass(&[], &[], &folders, SystemTime::now(), q);
        assert_eq!(p.joined, ["new"]);
    }

    #[test]
    fn ids_stay_stable_across_restarts() {
        let t = Tmp::new("ids");
        for n in ["Spider-Man", "Hades", "Spider Man"] {
            t.file(&format!("{n}/x.exe"), 1);
        }
        let scan = |known: &[Game], kept: &[Game]| pass(known, kept, &folders(&t.0).unwrap(), SystemTime::now(), Duration::ZERO);
        let first = scan(&[], &[]);
        let ids: Vec<_> = first.games.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["hades", "spider-man", "spider-man-2"]); // A–Z: "Spider Man" sorts before "Spider-Man"
        assert_eq!(first.games[1].folder, t.0.join("Spider Man"));

        // restart: saved entries keep their ids, though the new folder sorts before "Spider-Man" and would take its id
        t.file("Spider!Man/x.exe", 1);
        let again = scan(&first.games, &[]);
        let by_folder = |p: &Pass, f: &str| p.games.iter().find(|g| g.folder == t.0.join(f)).unwrap().id.clone();
        assert_eq!(by_folder(&again, "Spider Man"), "spider-man");
        assert_eq!(by_folder(&again, "Spider-Man"), "spider-man-2");
        assert_eq!(by_folder(&again, "Spider!Man"), "spider-man-3");
        assert_eq!(again.joined, ["spider-man-3"]);

        // a played game removed and copied back gets its id and playtime back
        let mut played = again.games.clone();
        played.iter_mut().find(|g| g.id == "hades").unwrap().playtime_sec = 3600;
        std::fs::remove_dir_all(t.0.join("Hades")).unwrap();
        let gone = scan(&played, &[]);
        assert_eq!(gone.left.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["hades"]);
        t.file("Hades/x.exe", 1);
        let back = scan(&gone.games, &gone.left);
        let hades = back.games.iter().find(|g| g.id == "hades").unwrap();
        assert_eq!(hades.playtime_sec, 3600);
    }
}
