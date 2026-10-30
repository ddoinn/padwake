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

If Padwake crashes, or is ended in Task Manager, it starts again within a few seconds and goes straight to Home.

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

Press **View** (the button with two squares) while you play to save a screenshot of the game; a toast confirms it.
On Home, **View** opens the **Media Gallery** with every screenshot, newest first: **A** shows one full screen, **X**
deletes it. They are JPEG files in `data\captures`, one folder per game. Games that run in exclusive full screen or play
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

**Settings → Saved Data** keeps your time played, sessions, ratings, status, progress and notes in a private gist on
your own GitHub account. It is free. A new or reset PC gets them back.

1. Select **Sign In with GitHub**. A code shows on the TV.
2. On your phone or computer, go to `github.com/login/device`, enter the code and allow Padwake. It only asks for
   gists.
3. If that account already has a backup (from another PC), Padwake offers to restore it.

After that, it backs up by itself a couple of minutes after each game and after you change your Journal (**Back Up
Automatically**). **Back Up Now** does it at once. A restore never loses anything: each game keeps the larger time, and
the notes you wrote on this PC stay. Games in the backup that aren't on this PC join the Journal marked "From backup",
and get their time back when you put the game in your games folder under the same folder name.

Art, trailers and your games folder aren't backed up: the PC fetches those again. The sign-in is kept in `data\`,
readable only by your Windows account. **Sign Out** removes it; the backup stays on GitHub.
