//! The folder side of the library, ported from `extras/antcloud/sync.ps1`: which folders are games, which exe
//! starts each one, when a new folder is quiet enough to join, and stable ids. No network here.

use super::stores::Installed;
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

/// True if `inner` is `outer` or inside it.
fn within(inner: &Path, outer: &Path) -> bool {
    let (i, o) = (key(inner), key(outer));
    i == o || i.strip_prefix(&o).is_some_and(|rest| rest.starts_with('\\'))
}

/// Matches the folders and the store games against the library. Known folders keep their entry (and id); quiet new
/// folders with an exe join, taking back the id, playtime and journal of a game that was removed from the same folder
/// (`kept`); entries whose folder is gone leave. An unplayed game's exe is picked again, as sync.ps1 does for games
/// added mid-install. Store games (`stores.rs`) join at once, since their launcher says they are installed, and leave
/// when it no longer lists them; a games-root folder that is or holds a store game's folder is left to the store.
pub fn pass(known: &[Game], kept: &[Game], folders: &[PathBuf], stores: &[Installed], now: SystemTime, quiet: Duration) -> Pass {
    let mut by_folder: HashMap<String, &Game> = known.iter().map(|g| (key(&g.folder), g)).collect();
    let mut taken: HashSet<String> = known.iter().chain(kept).map(|g| g.id.clone()).collect();
    // kept entries already given back in this pass: each comes back once
    let mut restored: HashSet<String> = HashSet::new();
    let mut p = Pass::default();
    let folders = folders.iter().filter(|f| !stores.iter().any(|s| within(&s.folder, f)));
    for folder in folders {
        if let Some(g) = by_folder.remove(&key(folder)) {
            let mut g = g.clone();
            if g.playtime_sec == 0 && g.last_played.is_none() {
                if let Some(exe) = pick_exe(folder).filter(|e| *e != g.exe) {
                    g.exe = exe;
                    p.exe_changed.push(g.id.clone());
                }
            }
            // a store game whose store was turned off, in the games folder: it starts from its exe now
            if g.store.take().is_some() && !p.exe_changed.contains(&g.id) {
                p.exe_changed.push(g.id.clone());
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
        // the same folder, or a folder of the same name (a games folder moved, or a game restored from a backup)
        let free: Vec<&Game> = kept.iter().filter(|k| !restored.contains(&k.id)).collect();
        let back = free
            .iter()
            .find(|k| key(&k.folder) == key(folder))
            .or_else(|| free.iter().find(|k| k.folder.file_name().is_some_and(|n| n.eq_ignore_ascii_case(&name))))
            .copied();
        let id = match back {
            Some(k) => {
                restored.insert(k.id.clone());
                k.id.clone()
            }
            None => unique(&slug(&name), &taken),
        };
        taken.insert(id.clone());
        let mut g = Game { id, name, folder: folder.clone(), exe, added_at: Some(ms(now)), ..Default::default() };
        if let Some(k) = back {
            // the journal's entry comes back whole, with the art it kept until enrichment fetches it again
            g.playtime_sec = k.playtime_sec;
            g.last_played = k.last_played;
            g.first_played = k.first_played;
            g.sessions = k.sessions;
            g.journal = k.journal.clone();
            g.art = k.art.clone();
        }
        p.joined.push(g.id.clone());
        p.games.push(g);
    }
    for s in stores {
        if let Some(g) = by_folder.remove(&key(&s.folder)) {
            let mut g = g.clone();
            // a folder game the store now knows, or a launch that changed (an update moved the exe)
            let exe = s.exe.clone().unwrap_or_else(|| g.exe.clone());
            if g.store.as_ref() != Some(&s.store) || g.exe != exe {
                (g.store, g.exe) = (Some(s.store.clone()), exe);
                // Steam says which app it is: a different match from the name search is fetched again
                if s.steam_app_id.is_some() && g.steam_app_id != s.steam_app_id {
                    (g.steam_app_id, g.synced_at) = (s.steam_app_id, None);
                }
                p.exe_changed.push(g.id.clone());
            }
            p.games.push(g);
            continue;
        }
        // the same store game, the same folder, the same Steam app, or (from a backup, which keeps no store) the same
        // name. Not a folder of the same name: every Xbox game's folder is called Content.
        let free: Vec<&Game> = kept.iter().filter(|k| !restored.contains(&k.id)).collect();
        let back = free
            .iter()
            .find(|k| k.store.as_ref().is_some_and(|ks| ks.name == s.store.name && ks.id == s.store.id))
            .or_else(|| free.iter().find(|k| key(&k.folder) == key(&s.folder)))
            .or_else(|| free.iter().find(|k| s.steam_app_id.is_some() && k.steam_app_id == s.steam_app_id))
            .or_else(|| free.iter().find(|k| k.store.is_none() && k.name.eq_ignore_ascii_case(&s.name)))
            .copied();
        let id = match back {
            Some(k) => {
                restored.insert(k.id.clone());
                k.id.clone()
            }
            None => unique(&slug(&s.name), &taken),
        };
        taken.insert(id.clone());
        let exe = s.exe.clone().or_else(|| pick_exe(&s.folder)).unwrap_or_default();
        let mut g = Game {
            id,
            name: s.name.clone(),
            folder: s.folder.clone(),
            exe,
            store: Some(s.store.clone()),
            steam_app_id: s.steam_app_id,
            added_at: Some(ms(now)),
            ..Default::default()
        };
        if let Some(k) = back {
            g.playtime_sec = k.playtime_sec;
            g.last_played = k.last_played;
            g.first_played = k.first_played;
            g.sessions = k.sessions;
            g.journal = k.journal.clone();
            g.art = k.art.clone();
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
    use super::super::stores::{Store, StoreName};
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
        let p = pass(&[], &[], &folders, &[], SystemTime::now(), q);
        assert!(p.games.is_empty() && !p.changed());
        assert_eq!(p.waiting.len(), 2); // Empty and New
        t.age("New", Duration::from_secs(200));
        let p = pass(&[], &[], &folders, &[], SystemTime::now(), q);
        assert_eq!(p.joined, ["new"]);
    }

    #[test]
    fn ids_stay_stable_across_restarts() {
        let t = Tmp::new("ids");
        for n in ["Spider-Man", "Hades", "Spider Man"] {
            t.file(&format!("{n}/x.exe"), 1);
        }
        let scan = |known: &[Game], kept: &[Game]| pass(known, kept, &folders(&t.0).unwrap(), &[], SystemTime::now(), Duration::ZERO);
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
        let mut kept = gone.left.clone();
        kept[0].sessions = 4;
        kept[0].journal.rating = Some(5);
        let back = scan(&gone.games, &kept);
        let hades = back.games.iter().find(|g| g.id == "hades").unwrap();
        assert_eq!((hades.playtime_sec, hades.sessions, hades.journal.rating), (3600, 4, Some(5)));
    }

    fn installed(store: StoreName, id: &str, name: &str, folder: PathBuf) -> Installed {
        Installed {
            store: Store { name: store, id: id.into(), uri: Some(format!("{id}://run")), args: None },
            name: name.into(),
            folder,
            exe: None,
            steam_app_id: (store == StoreName::Steam).then(|| id.parse().unwrap()),
        }
    }

    #[test]
    fn store_games_join_at_once_and_leave_when_uninstalled() {
        let t = Tmp::new("stores");
        t.file("Games/Celeste/Celeste.exe", 1);
        t.file("Steam/common/Hades/Hades.exe", 1);
        t.file("Epic/Hades/x64/Hades.exe", 5);
        let root = folders(&t.0.join("Games")).unwrap();
        let hades = installed(StoreName::Steam, "1145360", "Hades", t.0.join(r"Steam\common\Hades"));
        let epic = installed(StoreName::Epic, "Min", "Hades", t.0.join(r"Epic\Hades"));
        // a quiet time of an hour: the files were just written, yet store games don't wait
        let hour = Duration::from_secs(3600);
        let p = pass(&[], &[], &root, &[hades.clone(), epic.clone()], SystemTime::now(), hour);
        assert_eq!(p.joined, ["hades", "hades-2"], "two stores, two entries");
        assert_eq!(p.waiting, [t.0.join(r"Games\Celeste")], "a folder still waits for its quiet time");
        let steam = &p.games[0];
        assert_eq!((steam.steam_app_id, steam.store.as_ref().map(|s| s.name)), (Some(1145360), Some(StoreName::Steam)));
        assert_eq!(steam.exe, t.0.join(r"Steam\common\Hades\Hades.exe"), "no exe from the store: the folder's is picked");

        // played, then uninstalled: it leaves (the journal keeps it), and comes back with its id and time
        let mut known = p.games.clone();
        known[0].playtime_sec = 600;
        let gone = pass(&known, &[], &root, &[epic.clone()], SystemTime::now(), hour);
        assert_eq!(gone.left.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["hades"]);
        let moved = installed(StoreName::Steam, "1145360", "Hades", t.0.join(r"D\SteamLibrary\common\Hades"));
        let back = pass(&gone.games, &gone.left, &root, &[moved, epic.clone()], SystemTime::now(), hour);
        let h = back.games.iter().find(|g| g.store.as_ref().is_some_and(|s| s.name == StoreName::Steam)).unwrap();
        assert_eq!((h.id.as_str(), h.playtime_sec), ("hades", 600), "the same Steam app, reinstalled elsewhere");
    }

    #[test]
    fn a_kept_store_game_comes_back_to_itself_only() {
        let t = Tmp::new("kept-stores");
        t.file("XboxGames/Halo/Content/halo.exe", 1);
        t.file("Steam/common/Hades/Hades.exe", 1);
        t.file("Epic/Hades/Hades.exe", 1);
        let gone = |id: &str, store: StoreName, sid: &str, folder: &str| Game {
            id: id.into(),
            name: id.into(),
            folder: folder.into(),
            store: Some(Store { name: store, id: sid.into(), uri: None, args: None }),
            playtime_sec: 60,
            ..Default::default()
        };
        // two removed Xbox games, both in a folder called Content, and a removed Steam Hades
        let kept = [
            gone("forza", StoreName::Xbox, "Forza_pub", r"D:\XboxGames\Forza\Content"),
            gone("starfield", StoreName::Xbox, "Starfield_pub", r"D:\XboxGames\Starfield\Content"),
            Game { name: "Hades".into(), steam_app_id: Some(1145360), ..gone("hades", StoreName::Steam, "1145360", r"Z:\Steam\common\Hades") },
        ];
        let halo = installed(StoreName::Xbox, "Halo_pub", "Halo", t.0.join(r"XboxGames\Halo\Content"));
        let steam = installed(StoreName::Steam, "1145360", "Hades", t.0.join(r"Steam\common\Hades"));
        let epic = installed(StoreName::Epic, "Min", "Hades", t.0.join(r"Epic\Hades"));
        let p = pass(&[], &kept, &[], &[halo, steam, epic], SystemTime::now(), Duration::ZERO);
        let by = |name: StoreName| p.games.iter().find(|g| g.store.as_ref().unwrap().name == name).unwrap();
        assert_eq!((by(StoreName::Xbox).id.as_str(), by(StoreName::Xbox).playtime_sec), ("halo", 0), "no other Xbox game's journal");
        assert_eq!((by(StoreName::Steam).id.as_str(), by(StoreName::Steam).playtime_sec), ("hades", 60), "the same Steam app");
        assert_eq!(by(StoreName::Epic).id, "hades-2", "the kept Hades came back once");
    }

    #[test]
    fn steam_corrects_a_name_search_match() {
        let t = Tmp::new("steam-id");
        t.file("common/Hades/Hades.exe", 1);
        let root = folders(&t.0.join("common")).unwrap();
        let mut known = pass(&[], &[], &root, &[], SystemTime::now(), Duration::ZERO).games;
        // the name search matched Hades II
        (known[0].steam_app_id, known[0].synced_at) = (Some(1145350), Some(1));
        let hades = installed(StoreName::Steam, "1145360", "Hades", t.0.join(r"common\Hades"));
        let p = pass(&known, &[], &root, &[hades], SystemTime::now(), Duration::ZERO);
        assert_eq!((p.games[0].steam_app_id, p.games[0].synced_at), (Some(1145360), None), "Steam's app, fetched again");
    }

    #[test]
    fn the_store_takes_a_games_folder_entry_over() {
        let t = Tmp::new("takeover");
        t.file("common/Hades/Hades.exe", 1);
        t.file("common/Celeste/Celeste.exe", 1);
        // the games folder is Steam's common folder
        let root = folders(&t.0.join("common")).unwrap();
        let first = pass(&[], &[], &root, &[], SystemTime::now(), Duration::ZERO);
        assert_eq!(first.joined, ["celeste", "hades"]);
        let hades = installed(StoreName::Steam, "1145360", "Hades", t.0.join(r"common\Hades"));
        let p = pass(&first.games, &[], &root, &[hades], SystemTime::now(), Duration::ZERO);
        assert!(p.joined.is_empty() && p.left.is_empty(), "the same entries");
        assert_eq!(p.exe_changed, ["hades"]);
        let h = p.games.iter().find(|g| g.id == "hades").unwrap();
        assert_eq!(h.store.as_ref().map(|s| s.uri.as_deref()), Some(Some("1145360://run")), "it starts through Steam now");
        // Steam turned off: the folder is a plain game again
        let off = pass(&p.games, &[], &root, &[], SystemTime::now(), Duration::ZERO);
        assert!(off.games.iter().all(|g| g.store.is_none()) && off.exe_changed == ["hades"]);

        // a games-root folder that holds store games (D:\ as the games folder, D:\XboxGames inside) is the store's
        t.file("XboxGames/Halo/Content/halo.exe", 1);
        let root = folders(&t.0).unwrap();
        let halo = installed(StoreName::Xbox, "Halo_pub", "Halo", t.0.join(r"XboxGames\Halo\Content"));
        let p = pass(&[], &[], &root, &[halo], SystemTime::now(), Duration::ZERO);
        assert_eq!(p.joined, ["common", "halo"], "XboxGames isn't a game of its own");
    }

    #[test]
    fn a_restored_game_comes_back_by_folder_name() {
        let t = Tmp::new("restored");
        t.file("hollow knight/x.exe", 1);
        // from a backup made on another PC: only the folder's name is known
        let kept = [Game {
            id: "hollow-knight".into(),
            name: "Hollow Knight".into(),
            folder: PathBuf::from("Hollow Knight"),
            playtime_sec: 900,
            ..Default::default()
        }];
        let p = pass(&[], &kept, &folders(&t.0).unwrap(), &[], SystemTime::now(), Duration::ZERO);
        assert_eq!(p.joined, ["hollow-knight"]);
        assert_eq!((p.games[0].playtime_sec, p.games[0].restored_at), (900, None));
    }
}
