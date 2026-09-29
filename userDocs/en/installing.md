# Installing Nail on Windows

Nail comes in two forms for Windows 10 (22H2) and Windows 11 on x64:

| Download | For |
| --- | --- |
| `Nail_<version>_x64-setup.exe` | Everyday use. Installs for your Windows account only, without an administrator prompt. |
| `Nail_<version>_x64-portable.zip` | USB sticks and trying Nail out. Nothing is installed; everything stays in the unzipped folder. |

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
3. **Finish.** You can start Nail and create a desktop shortcut.

If the Microsoft Edge WebView2 runtime is missing (it is part of
Windows 11), the setup program installs it; this needs an internet
connection.

Running the setup program again repairs or changes an installation; it
remembers your options.

### What the installer registers

Nail registers itself as a program that can open:

- `mailto:` links (writing an email),
- `webcal:` and `webcals:` links (calendar subscriptions),
- `.eml` (email messages), `.ics` (calendar files) and `.vcf` (contact cards).

Windows does not let programs make themselves the default. To use Nail for
these, open **Settings > Apps > Default apps**, pick **Nail**, and choose
the link and file types you want it to handle. You can also right-click a
file and use **Open with > Nail**.

What happens when you open one:

- a **`mailto:` link** opens a new message with the recipients, subject
  and text of the link;
- an **`.eml` file** is shown in Nail under **Opened files** (a local
  account that only holds the messages you opened this way);
- **calendar and contact files and links** (`webcal:`, `.ics`, `.vcf`)
  arrive in a later preview; until then Nail says it cannot import them
  yet.

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
| `/NS` | Create no shortcuts |
| `/D=<folder>` | Install location; must be the last option, without quotes |

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
