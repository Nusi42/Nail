# Nail — English UI strings: updates and "About", tray and startup settings,
# default apps, the first-run screen and the composer entry for mail from
# other programs (M4b WP114; contract ADR 0087).
#
# Owned by WP114; message ids start with the prefixes listed in
# ui/src/features/system/messages.ts. Texts the contract names
# (installation kinds, update channels / failures / policy reasons,
# default-app targets, MAPI results) are in core-desktop.ftl.

system-settings-title = Desktop

## Layout labels (settings tabs, actions, window titles)

system-settings-tab-updates = Updates
system-settings-tab-desktop = Tray and startup
system-settings-tab-default-apps = Default apps
system-action-about = About Nail
system-action-check-updates = Check for updates
system-action-first-run = Welcome screen
system-action-default-apps = Choose default apps
window-about-nail = About Nail
window-first-run = Welcome to Nail

## Updates (widget update-settings, "About Nail")

system-updates-title = Updates
# $version = e.g. 1.0.0; $kind = how Nail was installed (install-kind-*)
system-version-line = Nail { $version } · { $kind }
system-update-options = Update options
system-update-channel = Update channel
system-update-auto-check = Check for updates automatically (at start and once a day)
system-update-auto-download = Download updates in the background
system-update-idle-auto = Nail checks for updates automatically.
system-update-idle-manual = Automatic checks are off. Use “Check now”.
system-update-checking = Checking for updates…
# $version = the running version
system-update-up-to-date = Nail { $version } is up to date.
# $version = the new version
system-update-available = Version { $version } is available.
# $size = the download size, e.g. 48 MB
system-update-available-size = Version { $version } is available ({ $size }).
system-update-downloading = Downloading version { $version }…
system-update-progress-label = Download progress
# $percent = 0–100
system-update-progress-percent = { $percent } % downloaded
# $done = the amount downloaded so far, e.g. 12 MB
system-update-progress-bytes = { $done } downloaded
system-update-ready = Version { $version } is ready. Restart Nail to install it.
system-update-installing = Installing version { $version }. Nail restarts in a moment.
# $when = date and time
system-update-last-checked = Last checked: { $when }
system-update-check = Check now
system-update-retry = Try again
system-update-download = Download
system-update-restart = Restart to install
system-release-notes-label = What’s new in { $version }

## About Nail

system-about-product = Nail
system-about-tagline = E-mail, calendar and contacts.
system-about-facts = Installation
system-about-version = Version
system-about-installer = Installed by
system-about-scope = Installed for
system-about-architecture = Architecture
system-about-channel = Update channel
system-about-folder = Folder
system-about-webview = WebView2 runtime
system-about-mapi = “Send to → Mail recipient”
system-about-mapi-yes = Installed
system-about-mapi-no = Not installed

## Tray and startup (widget desktop-settings)

# Shown on the desktop-only cards (updates, tray and startup, default apps) in the browser.
system-desktop-only = This setting is available in the Nail desktop app.

system-desktop-title = Tray and startup
system-desktop-tray = When the window closes
system-desktop-close-to-tray = Keep Nail running in the notification area
system-desktop-close-to-tray-hint = Mail sync, rules, reminders and connected AI apps keep working. Choose Quit in the tray menu to end Nail.
system-desktop-startup = When Windows starts
system-desktop-start-with-windows = Start Nail with Windows
system-desktop-start-with-windows-hint = Nail starts when you sign in to Windows.
system-desktop-start-with-windows-managed = Your administrator set Nail to start with Windows for everyone on this computer. Only an administrator can turn this off.
system-desktop-start-hidden = Start in the notification area, without a window
system-desktop-start-hidden-hint = Only when Nail starts with Windows. Open it from the tray icon.
system-desktop-developer = Plug-in development
system-desktop-dev-endpoint = Allow “nail plugin dev” to load plug-ins into this app
system-desktop-dev-endpoint-hint = Only programs on this PC that can read your Nail data folder can connect. Leave this off unless you write plug-ins.
# $url = the local address, e.g. http://127.0.0.1:47811/
system-desktop-dev-endpoint-running = Listening on { $url }

## Default apps (widget default-apps)

system-default-apps-title = Default apps
system-default-apps-intro = Windows decides which app opens e-mail links and files. Only you can choose Nail, in Windows Settings.
system-default-apps-list = Links and files
system-default-apps-mapi = Mail from other programs (“Send to → Mail recipient”)
system-default-apps-mapi-missing = The component for “Send to → Mail recipient” is not installed. Run the installer again to add it.
system-default-apps-unregistered = Nail is not registered for this
system-default-apps-not-registered = Nail is not listed in Windows Settings. Run the installer again to repair it.
system-default-apps-choose = Choose in Default apps
system-default-apps-opened = Choose Nail in Windows Settings, then come back here.
system-default-apps-refresh = Check again

## First-run screen

system-first-run-title = Welcome to Nail
system-first-run-intro = A few things to set up. You can change all of them later in Settings.
system-first-run-steps = Setup steps
system-first-run-default-apps = Open e-mail links with Nail
system-first-run-default-apps-text = Windows lets only you choose the app for e-mail links, calendar links and mail files. The button opens the right page in Windows Settings.
# $state = default-app-state-* text
system-first-run-mailto-state = E-mail links now: { $state }
system-first-run-startup = Start with Windows
system-first-run-start-with-windows = Start Nail with Windows, in the notification area
system-first-run-startup-text = Nail then gets new mail, runs your rules and shows reminders from the moment you sign in.
system-first-run-import = Bring your mail along
system-first-run-import-text = Import mail, contacts and calendars from another mail program.
system-first-run-import-start = Import…
system-first-run-import-later = The import assistant comes with a later version of Nail.
system-first-run-later = You can change these later in Settings.
system-first-run-done = Get started
system-first-run-skip = Skip

## Composer: mail prepared by another program (Simple MAPI)

system-program-draft-label = Message from another program
system-program-draft-unknown = Another program prepared this message. Check it, then press Send.
system-program-draft-dismiss = Dismiss
