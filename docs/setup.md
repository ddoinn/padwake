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
