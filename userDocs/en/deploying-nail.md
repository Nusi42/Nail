# Deploying Nail: installers, silent setup, updates

This page is for people who install Nail on many computers, or who want to
script the installation. For a normal installation see
[Installing Nail on Windows](installing.md).

## Which package

| Download | For | Installs | Updates |
| --- | --- | --- | --- |
| `Nail_<version>_<arch>-setup.exe` | Everyday use, one Windows account | Per user, in `%LOCALAPPDATA%\Programs\Nail`, no administrator prompt | Nail updates itself |
| `Nail_<version>_<arch>-offline-setup.exe` | Computers without internet access | Like `setup.exe`, but brings the Microsoft Edge WebView2 runtime along | Nail updates itself |
| `Nail_<version>_<arch>_en-US.msi` | IT departments (Intune, Group Policy, Configuration Manager) | Per machine, in `Program Files\Nail` | Off by default: you deploy new versions |
| winget (`Nail.Nail`) | Scripted setups | Wraps `setup.exe` | `winget upgrade` or Nail itself |
| `Nail_<version>_<arch>-portable.zip` | USB sticks, trying Nail out | Nothing | Manual |

`<arch>` is `x64` or `arm64`. Every download comes with a
`SHA256SUMS.txt`. Signed builds carry a digital signature you can check in
the file's *Properties > Digital Signatures*; preview builds may not be
signed yet.

## Silent installation (per user)

```bat
Nail_0.2.0_x64-setup.exe /S /AUTOSTART=1 /ADDTOPATH=1 /MAPI=0
```

| Option | Meaning |
| --- | --- |
| `/S` | Silent: no pages |
| `/P` | Passive: only a progress window |
| `/AUTOSTART=1` or `0` | Start Nail (in the tray) when signing in (default: off) |
| `/ADDTOPATH=1` or `0` | Put the `nail` command-line tool on `PATH` (default: off) |
| `/MAPI=1` or `0` | Register Nail for "Send to > Mail recipient" (default: off; asks for administrator rights, see below) |
| `/NS` | Create no shortcuts |
| `/D=<folder>` | Install location; must be the last option, without quotes |

Without an option, a reinstall keeps the previous choice.

## Silent installation (per machine, MSI)

Run from an elevated prompt or your deployment tool:

```bat
msiexec /i Nail_0.2.0_x64_en-US.msi /qn ADDTOPATH=1 MAPI=1
```

| Property | Default | Meaning |
| --- | --- | --- |
| `AUTOSTART` | `0` | `1`: Nail starts in the tray when any user signs in |
| `ADDTOPATH` | `0` | `1`: the `nail` command-line tool on the machine `PATH` |
| `DISABLEUPDATES` | `1` | `0`: let Nail update itself. With `1` (the default) Nail shows that updates are managed by your organization |
| `MAPI` | `0` | `1`: register Nail for "Send to > Mail recipient" (Simple MAPI) |
| `INSTALLDIR` | `C:\Program Files\Nail` | Install location |

When you deploy a newer MSI without a property, the earlier value stays.
Uninstall with `msiexec /x Nail_<version>_x64_en-US.msi /qn`; the users'
mail data and settings stay on the computer.

## Updates

Nail checks for updates when it starts and once a day, downloads them in
the background and installs them when you restart it. There are two
channels: **stable** and **beta** (earlier access; beta users also get
every stable release). Every update is checked against Nail's own update
signature before it is installed; an update that fails the check is
refused.

To control updates centrally, set these registry values (Group Policy or
Intune); they win over the user's settings:

| Value under `HKLM\Software\Policies\Nail` (or `HKCU`) | Type | Meaning |
| --- | --- | --- |
| `DisableUpdates` | DWORD `1` | Nail never updates itself |
| `UpdateChannel` | String `stable` or `beta` | The channel; users cannot change it |

## "Send to > Mail recipient" (Simple MAPI)

With the MAPI option, programs that offer "email this" — Explorer's
*Send to > Mail recipient*, Office's *Send as attachment*, and many others —
can open a new Nail message with the file attached, once Nail is your
mail program (Settings > Apps > Default apps).

Windows keeps this registration for the whole computer, so the per-user
setup program asks for administrator rights for this one step. If you
decline, Nail is installed without it; run the setup program again to add
it later. Because every program on the computer (including programs run
as administrator) may load the component, the per-user setup copies it to
`C:\Program Files\Nail MAPI`, where only administrators can change it, and
registers that copy; it then starts each user's own Nail. Uninstalling
removes the registration and that folder again. On shared computers use
the MSI with `MAPI=1`.

A program running as administrator can hand a message to a Nail that is
already running, but it never starts Nail itself: Nail would run with
administrator rights too. Start Nail normally first.

Nail never sends mail on behalf of another program: even when a program
asks to send "without showing anything", Nail opens the message so you
can check it and press **Send** yourself.

## winget

```bat
winget install Nail.Nail
winget upgrade Nail.Nail
```

The winget package installs the per-user `setup.exe`.
