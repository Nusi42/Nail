# Installing Nail on Windows

Nail runs on Windows 10 (22H2) and Windows 11, on x64 and ARM64 PCs. Pick
the download for your PC (`x64` or `arm64` in the name):

| Download | For |
| --- | --- |
| `Nail_<version>_<arch>-setup.exe` | Everyday use. Installs for your Windows account only, without an administrator prompt. Nail keeps itself up to date. |
| `Nail_<version>_<arch>-offline-setup.exe` | The same for PCs without internet access during setup: it brings the Microsoft Edge WebView2 runtime along. |
| `Nail_<version>_<arch>_en-US.msi` | IT departments: installs for all users of a PC. See [Deploying Nail](deploying-nail.md). |
| `Nail_<version>_<arch>-portable.zip` | USB sticks and trying Nail out. Nothing is installed; everything stays in the unzipped folder. |

Preview builds are not signed yet, so Windows SmartScreen may warn that the
publisher is unknown. Choose *More info* > *Run anyway* only for a build you
downloaded from the Nail project. Each download comes with a
`SHA256SUMS.txt` file to check it against.

## Install

Run the setup program and follow the pages:

1. **Install location.** By default
   `%LOCALAPPDATA%\Programs\Nail` (for example
   `C:\Users\you\AppData\Local\Programs\Nail`).
2. **Options.**
   - *Start Nail when I sign in to Windows*: Nail starts minimized, so mail
     keeps syncing, rules keep running and connected AI agents (MCP) keep
     working.
   - *Add the nail command-line tool to PATH*: lets you run `nail serve`,
     `nail mcp` and the other commands from any terminal. Open a new
     terminal afterwards.
   - *Use Nail for "Send to > Mail recipient"*: lets other programs hand
     files to Nail (see [Mail from other programs](desktop-app.md#mail-from-other-programs-send-to--mail-recipient)).
     Windows keeps this setting for the whole PC, so this one step asks for
     administrator rights; if you decline, Nail is installed without it.
     The setup program copies this component to
     `C:\Program Files\Nail MAPI`, where only administrators can change it,
     because every program on the PC may load it. The installation folder
     must not contain a percent sign (%) for this option.
3. **Finish.** You can start Nail and create a desktop shortcut.

If the Microsoft Edge WebView2 runtime is missing (it is part of
Windows 11), the setup program installs it; this needs an internet
connection (or use the `offline-setup.exe`).

The first time Nail starts, a welcome screen helps you choose Nail as your
mail program and start it with Windows; see
[The desktop app on Windows](desktop-app.md).

Running the setup program again repairs or changes an installation; it
remembers your options.

### What the installer registers

Nail registers itself as a program that can open:

- `mailto:` links (writing an email),
- `webcal:` and `webcals:` links (calendar subscriptions),
- `.eml` (email messages), `.ics` (calendar files) and `.vcf` (contact cards).

Windows does not let programs make themselves the default. To use Nail for
these, open **Settings > Apps > Default apps**, pick **Nail**, and choose
the link and file types you want it to handle — or use *Choose in Default
apps* in Nail's **Settings → Desktop app**, which opens that page and
shows what Nail handles now ([details](desktop-app.md#default-apps)). You
can also right-click a file and use **Open with > Nail**.

What happens when you open one:

- a **`mailto:` link** opens a new message with the recipients, subject
  and text of the link;
- an **`.eml` file** is shown in Nail under **Opened files** (a local
  account that only holds the messages you opened this way);
- a **`webcal:` link** opens the calendar subscription dialog with the
  address filled in;
- an **`.ics` or `.vcf` file** opens the import dialog for calendars or
  contacts.

If Nail is already running, the link or file opens in that window.

### Silent installation

For scripted setups, run the setup program with `/S`:

```bat
Nail_0.1.0_x64-setup.exe /S /AUTOSTART=1 /ADDTOPATH=1
```

| Option | Meaning |
| --- | --- |
| `/S` | Silent: no pages, default answers |
| `/AUTOSTART=1` or `0` | Start Nail when signing in (default: off) |
| `/ADDTOPATH=1` or `0` | Put the `nail` tool on `PATH` (default: off) |
| `/MAPI=1` or `0` | Use Nail for "Send to > Mail recipient" (default: off; shows an administrator prompt even when silent) |
| `/NS` | Create no shortcuts |
| `/D=<folder>` | Install location; must be the last option, without quotes |

Installing for all users with the MSI, its properties (`AUTOSTART`,
`ADDTOPATH`, `DISABLEUPDATES`, `MAPI`), update policies and winget are
described in [Deploying Nail](deploying-nail.md).

## Updates

The setup program's Nail updates itself: it checks when it starts and once
a day, downloads a new version in the background and installs it when you
restart Nail (or when you quit it). Every update is checked against Nail's
update signature first. You choose the channel (stable or beta) in
**Settings → Desktop app**; see [Updates and About Nail](desktop-app.md#updates-and-about-nail).
Installations from the MSI are updated by your IT department; the portable
version is updated by replacing the folder.

## Uninstall

Use **Settings > Apps > Installed apps > Nail > Uninstall**.

Uninstalling removes the program, its registrations, the *Start with
Windows* entry and the `PATH` entry. **It keeps your data**: settings,
`theme.kdl`, `layout.kdl` and rules in `%APPDATA%\Nail`, and your mail
store, search index and logs in `%LOCALAPPDATA%\Nail`, so a later
installation continues where you left off.

To remove everything, tick **Also delete my mail data, settings and stored
passwords** on the first uninstall page. This also deletes the passwords
and tokens Nail kept in the Windows Credential Manager. Mail on your
servers is not affected. For a silent uninstall that removes everything,
run `uninstall.exe /S /DELETEDATA` from the installation folder.

## Portable version

Unzip `Nail_<version>_x64-portable.zip` anywhere, for example onto a USB
stick, and start `Nail\Nail.exe`.

The folder contains a file named `portable`. As long as it is there, Nail
keeps its settings (`config\`), mail data (`data\`) and browser profile
(`webview\`) next to `Nail.exe` instead of in your Windows profile.
Passwords you save are still kept in the Windows Credential Manager of the
computer you use, not on the stick. Delete the `portable` file to use the
normal locations instead.

The portable version does not register itself for links or file types and
does not start with Windows. It needs the WebView2 runtime, which Windows 11
already has. The command-line tool is in `Nail\bin\nail.exe`; the `Nail\ui`
folder holds the pages that remote access and `nail serve` show in a
browser.
