//! The hub's trailer and music from a Steam store movie, as `extras/antcloud/sync.ps1` makes them. Steam trailers are
//! HLS with fMP4 pieces of 3 s: the trailer is the 720p rendition's init segment plus pieces 3–12 (30 s, skipping the
//! rating card and publisher logos), the music the audio rendition's first 40 pieces (2 min). Pieces cut from the
//! middle keep their timestamps, so every `tfdt` and `sidx` is shifted to start at 0; then ffmpeg remuxes the joined
//! file into a plain MP4 (`+faststart`), which plays from frame 1 and loops cleanly.

use super::steam::{pause_check, Http, PAUSED};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PIECE_SEC: u32 = 3;
const TRAILER_SKIP: usize = 6 / PIECE_SEC as usize;
const TRAILER_PIECES: usize = 30 / PIECE_SEC as usize;
const MUSIC_PIECES: usize = 120 / PIECE_SEC as usize;

/// The video and audio playlists of an HLS master playlist: the 1280x720 rendition, else the second one listed.
pub fn renditions(master_url: &str, master: &str) -> (Option<String>, Option<String>) {
    let base = base_url(master_url);
    let lines: Vec<&str> = master.lines().map(str::trim).collect();
    let video = lines
        .iter()
        .position(|l| l.contains("RESOLUTION=1280x720"))
        .and_then(|i| lines.get(i + 1))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .or_else(|| lines.iter().filter(|l| !l.is_empty() && !l.starts_with('#')).nth(1))
        .map(|u| join_url(&base, u));
    let audio = lines.iter().find(|l| l.starts_with("#EXT-X-MEDIA:TYPE=AUDIO")).and_then(|l| uri(l)).map(|u| join_url(&base, &u));
    (video, audio)
}

/// The init segment and the media pieces of a media playlist, as absolute URLs.
fn pieces(playlist_url: &str, playlist: &str) -> (Option<String>, Vec<String>) {
    let base = base_url(playlist_url);
    let lines: Vec<&str> = playlist.lines().map(str::trim).collect();
    let init = lines.iter().find(|l| l.starts_with("#EXT-X-MAP:")).and_then(|l| uri(l)).map(|u| join_url(&base, &u));
    let parts = lines.iter().filter(|l| !l.is_empty() && !l.starts_with('#')).map(|u| join_url(&base, u)).collect();
    (init, parts)
}

fn uri(tag: &str) -> Option<String> {
    let start = tag.find("URI=\"")? + 5;
    let len = tag[start..].find('"')?;
    Some(tag[start..start + len].to_string())
}

fn base_url(url: &str) -> String {
    let url = url.split('?').next().unwrap_or(url);
    url.rsplit_once('/').map(|(b, _)| b.to_string()).unwrap_or_default()
}

fn join_url(base: &str, u: &str) -> String {
    if u.starts_with("http://") || u.starts_with("https://") {
        u.to_string()
    } else {
        format!("{base}/{u}")
    }
}

/// Downloads init + `max` pieces after `skip` into one fMP4 at `out` and rebases its timestamps. Stops with `PAUSED`
/// when `stop` turns true between pieces.
fn join_pieces(http: &Http, playlist_url: &str, out: &Path, skip: usize, max: usize, stop: &dyn Fn() -> bool) -> Result<(), String> {
    pause_check(stop)?;
    let playlist = http.text(playlist_url)?;
    let (init, parts) = pieces(playlist_url, &playlist);
    let init = init.ok_or("playlist has no init segment")?;
    let parts: Vec<_> = parts.into_iter().skip(skip).take(max).collect();
    if parts.is_empty() {
        return Err("playlist has no pieces".into());
    }
    pause_check(stop)?;
    let mut bytes = http.bytes(&init)?;
    for p in &parts {
        pause_check(stop)?;
        bytes.extend(http.bytes(p)?);
    }
    pause_check(stop)?;
    rebase(&mut bytes);
    super::write_file(out, &bytes)
}

/// Shifts every `tfdt` (fragment start) and `sidx` (segment index) so the first of each is 0.
pub fn rebase(b: &mut [u8]) {
    let mut first = [None, None];
    walk(b, 0, b.len(), &mut first);
}

fn get(b: &[u8], q: usize, n: usize) -> u64 {
    b[q..q + n].iter().fold(0, |v, &x| (v << 8) | x as u64)
}

fn put(b: &mut [u8], q: usize, n: usize, mut v: u64) {
    for i in (0..n).rev() {
        b[q + i] = v as u8;
        v >>= 8;
    }
}

fn walk(b: &mut [u8], start: usize, end: usize, first: &mut [Option<u64>; 2]) {
    let mut p = start;
    while p + 8 <= end {
        let mut size = get(b, p, 4) as usize;
        let mut h = 8;
        let kind = [b[p + 4], b[p + 5], b[p + 6], b[p + 7]];
        if size == 1 {
            if p + 16 > end {
                return;
            }
            size = get(b, p + 8, 8) as usize;
            h = 16;
        } else if size == 0 {
            size = end - p;
        }
        if size < h || p + size > end {
            return;
        }
        match &kind {
            b"moof" | b"traf" => walk(b, p + h, p + size, first),
            b"tfdt" | b"sidx" => {
                let k = usize::from(&kind == b"sidx");
                let n = if b[p + h] == 1 { 8 } else { 4 };
                let q = p + h + 4 + k * 8;
                if q + n <= p + size {
                    let v = get(b, q, n);
                    let f = *first[k].get_or_insert(v);
                    put(b, q, n, v.saturating_sub(f));
                }
            }
            _ => {}
        }
        p += size;
    }
}

/// ffmpeg: `settings.ffmpeg`, else `ffmpeg.exe` beside the app, else one on PATH. `None` when none runs.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let set = crate::data::setting::<String>("ffmpeg").filter(|s| !s.trim().is_empty()).map(PathBuf::from);
    let beside = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("ffmpeg.exe")));
    [set, beside.filter(|p| p.is_file()), Some(PathBuf::from("ffmpeg.exe"))].into_iter().flatten().find(|p| {
        Command::new(p)
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success())
    })
}

fn remux(ffmpeg: &Path, input: &Path, out: &Path, stop: &dyn Fn() -> bool) -> Result<(), String> {
    let tmp = out.with_extension("part.mp4");
    let mut child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-map", "0", "-c", "copy", "-movflags", "+faststart", "-f", "mp4"])
        .arg(&tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    // read stderr aside, so a chatty ffmpeg can't block on a full pipe while we wait
    let stderr = child.stderr.take().map(|mut e| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = e.read_to_string(&mut s);
            s
        })
    });
    let status = wait_or_stop(&mut child, stop);
    let stderr = stderr.and_then(|t| t.join().ok()).unwrap_or_default();
    match status {
        Ok(s) if s.success() => std::fs::rename(&tmp, out).map_err(|e| format!("{}: {e}", out.display())),
        Ok(_) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("ffmpeg: {}", stderr.trim()))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// How often a running ffmpeg checks whether a game started.
const STOP_POLL: Duration = Duration::from_millis(500);

/// Waits for `child`; if `stop` turns true first (a game started), ends it and fails with `PAUSED`.
fn wait_or_stop(child: &mut Child, stop: &dyn Fn() -> bool) -> Result<ExitStatus, String> {
    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("ffmpeg: {e}"))? {
            return Ok(status);
        }
        if stop() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PAUSED.into());
        }
        std::thread::sleep(STOP_POLL);
    }
}

/// The trailer (`trailer.mp4`, needs ffmpeg) and its music (`music.m4a`) from an HLS master playlist into `dir`.
/// Returns the files that landed, or `PAUSED` as soon as `stop` turns true (a game started).
#[allow(clippy::type_complexity)]
pub fn fetch(
    http: &Http,
    master_url: &str,
    dir: &Path,
    ffmpeg: Option<&Path>,
    stop: &dyn Fn() -> bool,
) -> Result<(Option<PathBuf>, Option<PathBuf>, Vec<String>), String> {
    let mut errors = Vec::new();
    let master = match http.text(master_url) {
        Ok(m) => m,
        Err(e) => return Ok((None, None, vec![format!("trailer playlist: {e}")])),
    };
    let (video, audio) = renditions(master_url, &master);
    let mut trailer = None;
    match (video, ffmpeg) {
        (Some(v), Some(ff)) => {
            let joined = dir.join("trailer.join.mp4");
            let out = dir.join("trailer.mp4");
            let r = join_pieces(http, &v, &joined, TRAILER_SKIP, TRAILER_PIECES, stop).and_then(|_| remux(ff, &joined, &out, stop));
            let _ = std::fs::remove_file(&joined);
            match r {
                Ok(()) => trailer = Some(out),
                Err(e) if e == PAUSED => return Err(e),
                Err(e) => errors.push(format!("trailer: {e}")),
            }
        }
        (Some(_), None) => errors.push("trailer skipped: no ffmpeg".into()),
        (None, _) => errors.push("trailer: no video rendition".into()),
    }
    let mut music = None;
    if let Some(a) = audio {
        let out = dir.join("music.m4a");
        let result = match ffmpeg {
            Some(ff) => {
                let joined = dir.join("music.join.mp4");
                let r = join_pieces(http, &a, &joined, 0, MUSIC_PIECES, stop).and_then(|_| remux(ff, &joined, &out, stop));
                let _ = std::fs::remove_file(&joined);
                r
            }
            // the rebased fMP4 plays as it is; ffmpeg only tidies it
            None => join_pieces(http, &a, &out, 0, MUSIC_PIECES, stop),
        };
        match result {
            Ok(()) => music = Some(out),
            Err(e) if e == PAUSED => return Err(e),
            Err(e) => errors.push(format!("music: {e}")),
        }
    } else {
        errors.push("music: no audio rendition".into());
    }
    Ok((trailer, music, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_starting_ends_a_running_child() {
        // a 30 s child; a game "starts" after the first poll
        let mut child =
            Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(Stdio::null()).creation_flags(CREATE_NO_WINDOW).spawn().unwrap();
        let polls = std::cell::Cell::new(0);
        let t0 = std::time::Instant::now();
        let r = wait_or_stop(&mut child, &|| {
            polls.set(polls.get() + 1);
            polls.get() > 1
        });
        assert_eq!(r, Err(PAUSED.to_string()));
        assert!(t0.elapsed() < Duration::from_secs(3), "ended within a poll or two: {:?}", t0.elapsed());
        assert!(child.try_wait().unwrap().is_some(), "the child is gone");
    }

    #[test]
    fn picks_the_720p_video_and_the_audio() {
        let master = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",URI=\"hls_264_4_audio.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=5800000,RESOLUTION=1920x1080\nhls_264_0_video.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2600000,RESOLUTION=1280x720\nhls_264_1_video.m3u8\n";
        let (v, a) = renditions("https://v.example/t/1/hls_264_master.m3u8?t=5", master);
        assert_eq!(v.as_deref(), Some("https://v.example/t/1/hls_264_1_video.m3u8"));
        assert_eq!(a.as_deref(), Some("https://v.example/t/1/hls_264_4_audio.m3u8"));
        let (init, parts) = pieces(
            "https://v.example/t/1/hls_264_1_video.m3u8",
            "#EXT-X-MAP:URI=\"dash/init-1.m4s\"\n#EXTINF:3\ndash/c-1.m4s\n#EXTINF:3\ndash/c-2.m4s\n",
        );
        assert_eq!(init.as_deref(), Some("https://v.example/t/1/dash/init-1.m4s"));
        assert_eq!(parts, ["https://v.example/t/1/dash/c-1.m4s", "https://v.example/t/1/dash/c-2.m4s"]);
    }

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(kind);
        b.extend_from_slice(body);
        b
    }

    /// a moof whose tfdt (version 1, 64-bit) starts at `t`
    fn fragment(t: u64) -> Vec<u8> {
        let mut tfdt = vec![1, 0, 0, 0];
        tfdt.extend(t.to_be_bytes());
        mp4_box(b"moof", &mp4_box(b"traf", &mp4_box(b"tfdt", &tfdt)))
    }

    #[test]
    fn rebase_starts_the_joined_pieces_at_zero() {
        let mut file = mp4_box(b"ftyp", b"iso6");
        // sidx version 0: reference_ID, timescale, earliest_presentation_time (32-bit)
        file.extend(mp4_box(b"sidx", &[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0x3a, 0x98, 0, 0x01, 0x5f, 0x90]));
        file.extend(fragment(540_000));
        file.extend(mp4_box(b"mdat", b"xx"));
        file.extend(fragment(630_000));
        rebase(&mut file);
        let t = |at: usize| u64::from_be_bytes(file[at..at + 8].try_into().unwrap());
        let moof1 = 12 + 24; // ftyp 12 bytes, sidx 24 bytes
        let tfdt_value = 8 + 8 + 8 + 4; // moof, traf and tfdt headers, version and flags
        assert_eq!(t(moof1 + tfdt_value), 0);
        let moof2 = moof1 + 36 + 10;
        assert_eq!(t(moof2 + tfdt_value), 90_000);
        assert_eq!(u32::from_be_bytes(file[12 + 20..12 + 24].try_into().unwrap()), 0);
    }
}
