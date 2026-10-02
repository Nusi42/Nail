# Nail — English texts about the Windows desktop app built in Rust or named
# by the contract (M4b; contract ADR 0087): installation facts, update
# channels / failures / policy reasons, default apps, Simple MAPI results,
# and the shell's own texts (tray menu, OS dialogs).
#
# The ids named by the contract (nail_api::desktop, nail_api::mapi) are
# pre-filled; nail-api/tests/catalog.rs checks they exist. WP113 (desktop
# shell) adds its tray / dialog texts below; the UI's own texts are in
# system.ftl (WP114).

## Installation (InstallKind, InstallScope)

install-kind-nsis = Installed with Nail-setup.exe
install-kind-msi = Installed by your organization (MSI)
install-kind-portable = Portable copy
install-kind-development = Development build
install-scope-per-user = For you only
install-scope-per-machine = For all users of this PC

## Updates (UpdateChannel, UpdateDisabledReason, UpdateFailure)

update-channel-stable = Stable
update-channel-beta = Beta (early versions)
update-channel-locked = Your organization chose the update channel.
update-disabled-msi-managed = Your organization installs updates for Nail.
update-disabled-policy = Updates are turned off by your organization’s policy.
update-disabled-portable = The portable copy is updated by hand: download the new version.
update-disabled-development = Development builds do not update themselves.
update-disabled-not-configured = This build of Nail has no update source.
update-failure-network = Could not reach the update server.
update-failure-manifest = The update information could not be read.
update-failure-signature = The update’s signature is not valid. Nothing was installed.
update-failure-download = The download did not finish.
update-failure-install = The update could not be installed.

## Default apps (DefaultAppTarget, DefaultAppState)

default-app-mailto = E-mail links (mailto:)
default-app-webcal = Calendar links (webcal:)
default-app-webcals = Secure calendar links (webcals:)
default-app-eml = E-mail files (.eml)
default-app-ics = Calendar files (.ics)
default-app-vcf = Contact files (.vcf)
default-app-state-default = Nail is the default
default-app-state-other = Another app is the default
default-app-state-unknown = Windows does not tell

## Desktop requests without the desktop app

desktop-unavailable = This is only available in the Nail desktop app.

## Simple MAPI (MapiResult; "Send to → Mail recipient" from other programs)

# $program = the program's file name, e.g. WINWORD.EXE
mapi-draft-from-program = { $program } prepared this message. Check it, then press Send.
mapi-result-success = The message is open in Nail.
mapi-result-user-abort = Cancelled.
mapi-result-failure = Nail could not take the message.
mapi-result-login-failure = Set up an e-mail account in Nail first.
mapi-result-insufficient-memory = Not enough memory.
mapi-result-access-denied = Access denied.
mapi-result-too-many-files = Too many attachments.
mapi-result-too-many-recipients = Too many recipients.
mapi-result-attachment-not-found = An attachment was not found.
mapi-result-attachment-open-failure = An attachment could not be read.
mapi-result-unknown-recipient = A recipient is not known.
mapi-result-bad-recip-type = A recipient type is not supported.
mapi-result-text-too-large = The text is too long.
mapi-result-invalid-recips = A recipient’s address is not valid.
mapi-result-not-supported = Nail does not support this request.
mapi-result-attachment-too-large = An attachment is too large.

## Desktop shell (WP113: tray menu, dialogs)

# The tray icon's tooltip (the product name).
tray-tooltip = Nail
tray-open = Open Nail
tray-new-message = New message
tray-check-mail = Check mail
tray-quit = Quit Nail
desktop-run-key-failed = Nail could not change “Start with Windows”.
desktop-open-settings-failed = Nail could not open Settings → Default apps.
