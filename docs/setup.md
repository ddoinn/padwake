# Setting up a living-room PC

Padwake turns a Windows PC into a console: you sign in, it fills the TV, and an Xbox controller does the rest.

## 1. Install

Padwake is a portable folder; there is no installer.

1. Copy the `Padwake` folder (with `padwake.exe` in it) anywhere, for example `C:\Padwake`.
2. Start `padwake.exe` once. It keeps everything (settings, library, art, log) in a `data` folder beside it.

It needs the Microsoft Edge WebView2 Runtime, which Windows 11 already has. If the screen stays black on Windows 10,
install the "Evergreen" runtime from Microsoft's WebView2 page.

## 2. The games folder

- One folder per game, in `C:\Games` unless you pick another in **Settings → Library → Games Folder**.
- A folder you copy in joins the row once nothing in it has changed for 3 minutes. A folder you delete leaves.
- No scanning happens while a game runs.

## 3. Art and trailers

- **Art** comes from SteamGridDB. Make a free account at steamgriddb.com, copy your API key from Preferences → API,
  and save it as a text file named `sgdb.key` in the `data` folder. Without it, games get store art only.
- **Trailers** need `ffmpeg.exe`. Put it beside `padwake.exe` (or anywhere on PATH). Without it, games have no
  trailer.
- **Settings → Library → Refresh All Art** fetches the art again.

## 4. Start with Windows

Turn on **Settings → System → Start with Windows**. It adds a task named "Padwake" to Task Scheduler that starts
the app when you sign in, full screen and in front, with the controller working. Turning it off removes the task.
Windows keeps its own prompts (an administrator or firewall question) in front of every app, so answer those first.

At each start you see the light show, then "Press any button on your controller"; any button skips the light show.

If Padwake crashes, or is ended in Task Manager, it starts again within a few seconds and goes straight to Home. In the
first 10 minutes after the PC comes on it shows the start screen again instead.

## 5. Sign in without a password

For the PC to go straight to Padwake after a restart, Windows must sign in by itself. Padwake never changes
this; it is a Windows setting:

1. **Settings → Accounts → Sign-in options**: turn off "For improved security, only allow Windows Hello sign-in for
   Microsoft accounts on this device". Set "If you've been away, when should Windows require you to sign in again?"
   to **Never**.
2. Press **Win + R**, type `netplwiz`, press Enter.
3. Untick **"Users must enter a user name and password to use this computer"**, press OK, and type the account's
   password twice. (No tick box? Step 1's Windows Hello setting is still on.)

**The tradeoff:** anyone who can switch the PC on is signed in as you. Your browser sessions, saved passwords, store
accounts, email and files are open to them, and Windows keeps your password on the PC to do the sign-in. Do this only on
a PC that stays at home and holds nothing you'd mind a guest seeing; a separate local account just for the living room
is safer than your main Microsoft account.

## 6. The Guide button and Game Bar

- On Home, a **tap** of the Guide (Xbox) button opens Control Center and a **hold** goes Home, also over a running game.
- Windows normally gives that button to Xbox Game Bar. At each start Padwake takes it back: it turns off
  "Open Xbox Game Bar using this button on a controller" (Settings → Gaming → Xbox Game Bar), only if Game Bar is
  installed. The `data\padwake.log` file says what it did.
- To give the button back to Game Bar, uninstall or stop starting Padwake, then turn that setting on again.

## 7. Exit to the desktop

**Power** (top right on Home, or **Settings → Power**) **→ Exit to Desktop** closes Padwake completely and shows
the Windows desktop. To come back, start `padwake.exe` again, or sign out and in.

## 8. Screenshots

Press **Guide + View** (View is the button with two squares) while you play to save a screenshot of the game; a toast
confirms it. **View** on its own stays the game's button, for maps and scoreboards.
On Home, **View** opens the **Media Gallery** with every screenshot, newest first: **A** shows one full screen, **X**
deletes it. They are JPEG files in `data\users\<user>\captures`, one folder per game, and each user sees their own. Games that run in exclusive full screen or play
protected video copy as black; Padwake says so instead of saving a black picture.

## 9. Updates

Padwake updates itself from its GitHub releases. It is free and needs no account.

- About a minute after it starts, and every 6 hours, it looks for a new version, never while a game runs. A new
  version downloads in the background; a toast says when it is ready.
- It installs the next time Padwake starts. To install it now, go to **Settings → System → System Software →
  System Software Update** and pick **Update and Restart**. It takes a few seconds.
- **Install Updates Automatically** (same page) turns the background checks off; System Software Update still checks
  when you select it.
- The previous version stays beside the new one as `padwake.old.exe`. To go back, exit to the desktop, delete
  `padwake.exe` and rename `padwake.old.exe` to `padwake.exe`.

## 10. Back up your time and journal to GitHub

**Settings → Saved Data** keeps your time played, sessions, ratings, status and notes in a private gist on
your own GitHub account. It is free. A new or reset PC gets them back. Each user on the PC signs in to their own account.

1. Select **Sign In with GitHub**. A code shows on the TV.
2. On your phone or computer, go to `github.com/login/device`, enter the code and allow Padwake. It only asks for
   gists.
3. If that account already has a backup (from another PC), Padwake offers to restore it.

After that, it backs up by itself a couple of minutes after each game and after you change your Journal (**Back Up
Automatically**). **Back Up Now** does it at once. A restore never loses anything: each game keeps the larger time, and
the notes you wrote on this PC stay. Games in the backup that aren't on this PC join the Journal marked "From backup",
and get their time back when you put the game in your games folder under the same folder name.

Art, trailers and your games folder aren't backed up: the PC fetches those again. The sign-in is kept in your user's
folder in `data\users\`, readable only by your Windows account. **Sign Out** removes it; the backup stays on GitHub.

## 11. Progress in the Journal

Padwake only shows a percentage it can read from the game: the achievements you unlocked ("12 of 51 · 24 %"), updated
after each session. It can't be typed in, and it isn't a guess. A game that keeps no achievements on the PC shows no
percentage, only your time played and, when HowLongToBeat knows the game, how long its main story usually takes.

When you come back to Home after a session of a minute or more, a toast says how long you played, and another says how
many achievements the game recorded, if any.

## 12. Trophies

Every game has the same six Padwake trophies, on its Journal page. They come from what Padwake counts itself: bronze for
your first play and 10 sessions, silver for 10 hours and 50 sessions, gold for 50 and 100 hours. A toast tells you when
you come back to Home. They need no account or network, nothing you set can earn one, and a restored backup brings them
back. They are separate from a game's own achievements.

## 13. Users

Everyone who plays on the PC can have their own user, with a name and an avatar. Each user has their own time played,
Journal, trophies, screenshots and GitHub backup. The games, their art and every setting are shared.

- **Add a user:** **Settings → Users → Add User**, or **Add User** on "Who's using this controller?".
- **Choose who plays:** with two or more users, "Who's using this controller?" comes after "Press any button". To
  switch later, go to Home, move up to your avatar at the right of the top bar and pick **Switch User**.
- **Change a name or avatar:** **Settings → Users**, then the user, then **Avatar**. Pick a popular character (Berserk
  included), **Search Characters** for any other (from AniList, online), one of your own pictures, one of Padwake's,
  or a game's art. A new user starts with a random popular character.
- **Use your own picture:** save a JPEG or PNG in **Pictures**, **Downloads** or **Desktop** on the PC (a browser
  download lands in Downloads), or copy it into `data\avatars\mine`. It shows under **Your Pictures**, newest first.
- **Delete a user:** switch to someone else first, then **Settings → Users → the user → Delete User**. Their time,
  Journal and screenshots on this PC go; a GitHub backup stays on their account.

You can't switch while a game runs, so a session's time always goes to whoever started it. After an update from a
version without users, everything recorded so far belongs to the first user, "Player 1"; rename them in Settings → Users.

## 14. Cloud PCs (AntCloud)

An AntCloud PC doesn't turn off from Windows. Turn on **Settings → Power → Cloud PC Mode**: Shut Down becomes
**End Session**, which shows what to press on the device you stream from (Ctrl + F9 in the AntCloud app, F9 in a
browser, then **Shut Down PC**), and Sleep is turned off. Padwake can't press those keys for you; they only work on
your device.

With Cloud PC Mode on, a mouse click doesn't dismiss "Press any button", so the click that focuses the stream leaves it
waiting for you. `data\padwake.log` records each start and what dismissed the screen.
