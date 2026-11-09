# Padwake: plan

What's next for Padwake, and the decisions behind it. The PS5 measurements the UI follows are in the sibling repo
`ch-night` (`design/ps5/`); code comments that say "row A…" mean rows in `ch-night/PLAN.md`. Rows here are "B".

## Decisions (2026-11-07, Madhav: "lock it")

1. **Next up: four features.** Display settings per game (B2), collections and smart lists (B3), a monthly and yearly
   recap for each user (B1 + B4), and a PS5-feel polish pass (B5).
2. **No game recommendations.** Neither "pick for tonight" from your own library, nor games you don't own, nor an
   AI chat ("kind of ass right now"). Don't propose them again unless Madhav asks.
3. **Later, not now:** the real trophy case (game achievements as trophies) and PlayStation / Switch controllers
   (input is XInput only today). Context for the trophy case: every game, from any store, gets Padwake's six trophies.
   Real achievements show only where the game keeps an unlock file Padwake reads (`library/progress.rs`); Epic,
   GOG, Xbox and EA app achievements aren't read.
4. **Recaps need a session log, which starts when B1 ships.** Today only totals are kept (time, sessions, first and
   last played), and trophies have no dates. Months before B1 can't be shown. A "so far" view of the current month
   is always there, so nobody waits for the 1st.
5. **Who owns what.** Display profiles and your own collections are shared, like games and settings. Smart lists,
   the session log and recaps belong to each user, like the Journal.
6. **Standing rules still apply.** Nothing the player can set (journal status, rating) earns a trophy. Sony's
   assets, names, font and sounds never ship. No 3D controller or connect animation. Home's measured speed and idle
   cost (`docs/performance.md`) must not get worse.

## Working on the Avita

- Rust: set `CARGO_HOME` and `RUSTUP_HOME` under `W:\system\tools\rust` and put `cargo\bin` on PATH.
- Line endings differ per file and there's no `.gitattributes`: edit with tools that keep them, and check
  `git ls-files --eol` before committing.
- Gates: `cargo test --lib --locked` in `app/src-tauri/`, and the UI probe checks listed in `DEVELOPMENT.md`.
- Test library: `W:\games-test`. Pushing needs a `ddoinn` token.

## Rows

Rows run in order: B2, B3 and B5 all touch `app/ui/home/home.js`, and B1 and B2 both touch
`app/src-tauri/src/games/mod.rs`. Each row's "done when" is red if any check misses; write what was measured into
the row when it's done. B1 is worth a release of its own, so the log starts collecting sooner.

### Row B1: Session log (each user's)
- **Outcome:**
  - Each finished session adds one line to `data\users\<id>\sessions.jsonl`: game id, start, end, seconds. It's
    written where `library::record_session` already records time, and goes to whoever started the game.
  - The log goes in the GitHub backup (`library/portable.rs`). A restore merges by game id and start time, so the
    same session is never counted twice, and no session from either PC is lost.
  - No UI in this row.
- **Done when:** a stand-in game from `W:\games-test`, played twice, leaves two lines in that user's log, and its
  seconds match the time `play.json` added. Unit tests: an append survives a crash mid-write (no broken line is
  read back), backup merge doesn't double or drop sessions, and a second user's session lands in their own file.
- **Owns:** `app/src-tauri/src/library/mod.rs` (record_session), a new `app/src-tauri/src/library/sessions.rs`,
  `app/src-tauri/src/library/portable.rs`.
- **Done (2026-11-07), measured:**
  - On the Avita, a debug build with its own data dir and `Hades` from `W:\games-test` (the 5 s stand-in), played
    twice from Home with the test pad, left two lines in `users\<id>\sessions.jsonl`:
    `{"game":"hades","start":1794076117762,"end":1794076122762,"secs":5}` and
    `{"game":"hades","start":1794076151552,"end":1794076156552,"secs":5}`. `play.json` went from 0 to 5 to 10 s and
    2 sessions; its `firstPlayed` is the first line's start and its `lastPlayed` the second line's end.
  - Unit tests: `appends_survive_a_crash_mid_write`, `a_backup_merge_never_doubles_or_drops_sessions`,
    `sessions_add_playtime_and_go_to_the_players_log` (second user, own file), `a_backup_from_before_the_session_log_reads_with_none`.
    `cargo test --lib --locked`: 95 passed, 12 ignored.
  - Also touched, outside the owned files: `app/src-tauri/src/cloud/backup.rs` builds the gist, so it now hashes,
    uploads and restores the log too (merged with what the gist holds, like the games).
  - For B4: sessions keep the game id they were played under. A game whose id differs between two PCs shows as two
    ids in a merged log; match by `portable.rs`'s rules if that ever matters. A PC still on 0.3.4 that uploads drops
    the log from the gist, but the next upload from a PC with B1 puts it back (its own log is merged in).

### Row B1b: Log everything cheap about play (each user's)

Decisions (2026-11-08, Madhav: "log everything you can so that we can make detailed stats"):
1. Padwake writes down what it can learn cheaply about each session, so later rows can build detailed stats. No UI.
2. All of it stays on the PC, except the user's own private backup gist.
3. Never logged: FPS, GPU or temperature sampling, single keys or buttons, window titles, search text, note text.
4. It mustn't cost anything you'd notice: running-game CPU and Home idle cost stay within `docs/performance.md`.
   Only OS events and job counters, with idle checked at most once a minute.

- **Outcome:**
  - Each line in `sessions.jsonl` gets new optional fields. Lines from B1 still read.
    - Where the time went: seconds with the game in front, Home (any Padwake screen), Control Center, or another app
      in front. This comes from `games/front.rs` foreground events, Guide, and the page reporting its top screen.
      Windows' alt-tab switcher, taskbar and desktop don't count as another app. After alt-tab, the window picked
      sometimes gets no event, so each event arms a single re-read of the foreground 500 ms later.
    - Idle seconds: stretches of 5+ min with no pad, keyboard or mouse input, checked once a minute.
    - Launch to first window (ms). Where it was launched from: Home, Library, Search or Switch Game.
    - How it ended: quit, Close Game, crash (a non-zero exit, with the code) or Padwake restart.
    - The job's CPU time and peak memory, read once at the end. Best effort: processes outside the job aren't counted.
    - At start and at end: pad name, connection and battery; display mode and HDR; AC or battery, and battery %.
    - Guide opens, screenshots taken, Padwake trophies earned, achievements before→after (when an unlock file
      exists), and the Padwake version.
  - Media apps (YouTube, Netflix, …) log lines too, with `"kind":"media"` and the app. They add no playtime.
  - A session cut short by a Padwake restart is still logged. A small file next to the log is updated once a minute
    while a game runs. At the next start it becomes a line ending "restart", and its time counts up to the last
    minute seen, so the log and `play.json` still agree.
  - A new `users\<id>\events.jsonl` with dated events: journal status set or cleared, rating changed, note edited
    (no text), user switched to, Padwake started and stopped. It goes in the backup and merges without doubles.
  - `addedAt` on library games (shared) was already there: every game gets it when its folder joins (0.1.0).
- **Done when:** unit tests for every field and for the backup merge of events. A live debug run (own data folder,
  test pad) of a stand-in from `W:\games-test` plays two sessions, one ended with Close Game and one alt-tabbed, and
  their lines show the right values. `cargo test --lib --locked` passes, and so do the UI probe checks in
  `DEVELOPMENT.md`. Running-game CPU and Home idle cost are measured against `docs/performance.md`.
- **Owns:** `app/src-tauri/src/library/{sessions,events,jsonl}.rs`, `games/{mod,stats,session,front}.rs`, the backup
  (`library/portable.rs`, `cloud/backup.rs`). `from` and the top-screen report in `app/ui/{app,host}.js`,
  `home/home.js`, `library/library.js`, `search/search.js`.
- **Done (2026-11-08), measured:**
  - On the Avita, a debug build with its own data dir, the test pad and `Stand-in Long` from `W:\games-test`
    (before the review fixes) logged, in `users\<id>\sessions.jsonl`:
    - Close Game: `"from":"home","ended":"close","launchMs":164,"front":{"game":10,"home":0,"controlCenter":8,
      "other":0},"cpuMs":187,"peakMb":25,"guide":1`, display `1920×1080@60`, HDR off, `"power":{"ac":true}`.
    - Alt-tabbed: `"secs":43,"ended":"quit","front":{"game":11,"controlCenter":3,"other":28}`. This matches the
      Padwake log's timestamps (6.6 s game, 28.2 s other app, 3 s Control Center, 4.6 s game). The Stand-in Long
      line `"trophies":["first-steps"]` matches `play.json` going from 39 to 82 s.
    - A screenshot during a session: `"screenshots":1`.
    - Padwake killed mid-game, then started again: one line `"ended":"restart","secs":60`. `play.json` went from
      82 to 142 s and 4 sessions, and the open-session file was removed.
    - `events.jsonl`: `started` (with `restarted`) and `stopped` on a clean close.
    - No `pad` fields: the test pad isn't XInput. No battery fields: the Avita reports no system battery.
  - Unit tests cover every field, the backup merge of events, restart recovery counted exactly once, and Switch Game
    keeping the new session's open file. `cargo test --lib --locked`: 111 passed, 13 ignored, no warnings.
  - UI probes: every check in `DEVELOPMENT.md` passes, including the new `play-log.json` (8 checks).
    `gallery.json` fails with its documented `captures=9` query on the base too; it passes with `scene=home`.
- **Done (2026-11-09), final run after the rebase onto #1 and #2:**
  - The re-run found a hole: about 10 ms after the "Ready to play" toast shows, Windows hands the foreground to the
    app's hidden window (and sometimes first to the window in front before the game). After that, Windows sends no
    foreground event when focus goes back to the game, so an alt-tab could go unseen. Seen in the session log
    as a 55 s alt-tabbed session with `"other":0`.
  - Fix (`games/mod.rs`, `came_to_front`): our own window coming to the front while the game is up and the app
    asleep is an accident, so the app hands the foreground straight back to the game (logged as "our window took
    the front over the game: given back"). That also gives the game its keyboard back.
  - Release build of the rebased head, own data dir, test pad, `Stand-in Long`, run in the desktop session through a
    one-shot scheduled task (the agent shell is session 0):
    - Close Game: `"secs":19,"ended":"close","launchMs":331,"front":{"game":10,"home":0,"controlCenter":9,
      "other":0},"guide":1,"cpuMs":281,"peakMb":26`. The log: 9.7 s game, then 9.0 s Control Center.
    - Alt-tabbed and back, then the game quit: `"secs":48,"ended":"quit","front":{"game":26,"controlCenter":0,
      "other":22}`, `"trophies":["first-steps"]`. A separate foreground-event listener saw the other window
      from 22:49:04.292 to 22:49:26.521 (22.2 s); Padwake logged 22:49:04.293 and 22:49:26.525.
    - `events.jsonl`: `started` (`"restarted":false`) and `stopped` on a clean close. The open-session file is gone.
  - Cost against `docs/performance.md` (release, 8 logical cores, 60 Hz, test pad, no XInput pad):
    - Home idle: 0.0575% of the PC, GPU 0.006% average (baseline 0.0830%, 0.006%). One earlier sample read 0.0945%;
      the other was 0.0575%. B1b adds no work on Home but a hook that waits for focus changes.
    - While a game runs (10 s after launch, before the fix): 0.0508% of the PC, GPU 0.000% (baseline 0.0576%,
      0.000%). The fix runs once per accidental focus change, never on a timer.
  - `cargo test --lib --locked` on the rebased head: 125 passed, 14 ignored, no warnings. UI probes: all 13 lines in
    `DEVELOPMENT.md` pass, `gallery.json` only with `scene=home` (its documented query fails on `main` too).
  - On the Avita, idle stays 0 because a phantom keyboard key keeps resetting the idle timer.

### Row B2: Display settings per game (shared)
- **Outcome:**
  - The Options menu (Menu on a game, `openMenu` in `home.js`) gets "Display for This Game". It offers the modes
    Settings → Display already lists (resolution and refresh rate, `system/display.rs`), HDR on/off, and "Use PC
    Setting", the default.
  - At launch, the profile's mode and HDR are applied as a temporary change (not saved to the registry). When the
    session ends, the display goes back to what it was before. If Padwake restarts during a game, it puts the saved
    display back at its next start.
  - The game hub shows the profile in one line when the game has one ("1280 × 720 · 60 Hz · HDR on").
  - Profiles are stored with the shared settings (`settings/mod.rs`), keyed by game id.
- **Done when:** on the Avita, a stand-in with a 1280 × 720 profile runs at that mode, and quitting brings back the
  previous mode within 2 s. Killing Padwake mid-game and starting it again restores the previous mode too. Unit
  tests: a profile for a mode the monitor no longer offers is skipped, not applied.
- **Owns:** `app/src-tauri/src/system/display.rs`, `app/src-tauri/src/games/mod.rs` (launch and end),
  `app/src-tauri/src/settings/mod.rs`, the Options menu entry and its picker in `app/ui/home/home.js`.

### Row B3: Collections and smart lists
- **Outcome:**
  - The Game Library gets a "Lists" tab beside Your Collection and Journal. It shows a grid of list tiles: smart
    lists first, then yours, then "New List". A opens a list as the usual cover grid; Menu there renames or deletes
    your list.
  - Smart lists, for whoever is playing, worked out every time and hidden when empty:
    - Playing (journal status Playing)
    - Never Played (no sessions)
    - Finished (status Finished or Completed)
    - Paused (status Paused)
    - Short Games (HowLongToBeat main story ≤ 10 h, `hltb_main_sec`)
    - Not Played in 3 Months (last played over 90 days ago, not Finished, Completed or Dropped)
  - Your own lists are shared by everyone on the PC, kept in `data\collections.json`. Options on a game gets "Add to
    List…", which ticks lists on and off and can make a new one with the on-screen keyboard.
- **Done when:** a new UI probe check (`tools/app/checks/lists.json`, mock data) passes. It covers each smart list
  with seeded games, making, renaming and deleting a list, adding a game from Options, and two users seeing
  different smart lists but the same own lists. Plus a screenshot of the Lists tab on the Avita.
- **Owns:** `app/ui/library/library.js` (tab), a new `app/ui/library/lists.js` and its CSS, "Add to List…" in
  `app/ui/home/home.js`, a new `app/src-tauri/src/library/collections.rs`, `tools/app/checks/lists.json`.

### Row B4: Monthly and yearly recap (each user's)
- **Outcome:**
  - From B1's log: hours and sessions, top 3 games by time (with art), longest session, busiest weekday and time of
    day, games started for the first time, Padwake trophies earned in the period, and one screenshot from it
    (captures carry their date in the file name).
  - Trophies earned in a period come from replaying the log: a game's totals at a date are today's totals minus
    the sessions logged after it. No trophy dates are stored.
  - Full-screen pages: Left / Right or LB / RB to step through, B to close. Reached from the Journal tab ("Recaps":
    "This month so far", then every past month with a session, then years).
  - On the first Home of a new month, a notice: "Your November recap is ready." The yearly one comes on 1 January.
    Months with no sessions get no notice.
- **Done when:** a UI probe check (`tools/app/checks/recap.json`, a mock log covering two months and two users)
  shows the right numbers on every page, gives the notice once, and shows nothing from before the log began. Unit
  tests for the totals-at-a-date replay. Plus screenshots of each page on the Avita.
- **Owns:** a new `app/ui/journal/recap.js` and its CSS, the Recaps entry in `app/ui/library/journal.js`, the
  notice in `app/ui/control-center/notices.js`, reading the log in `app/src-tauri/src/library/sessions.rs`,
  `tools/app/checks/recap.json`.

### Row B5: PS5-feel polish pass
- **Outcome:**
  - First, screenshot every screen at 1080p (Home, a game hub, Library, Journal, Search, Settings, Control Center,
    gallery, launch screen and recap). Compare them with `ch-night/design/ps5/` and pick up to 5 fixes that make it
    feel more like a console. Ideas to weigh, if they aren't already there: Home's tint following the focused game's
    art, more depth when entering a game hub, and a hold-to-confirm ring on Close Game.
  - Motion only on transform and opacity. Our own sounds and icons only.
- **Done when:** before and after screenshots of each fix are in the row, every UI probe check in `DEVELOPMENT.md`
  still passes, and the keypress-to-first-frame median and Home idle CPU/GPU stay within 10 % of
  `docs/performance.md`.
- **Owns:** whatever `app/ui/**` files the chosen fixes touch. Runs last, alone.
