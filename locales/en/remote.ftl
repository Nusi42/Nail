# Nail — English UI strings: remote access (login, pairing, sessions and
# devices, remote-UI settings, security audit) and remote content in the
# reader (M2b WP43).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/remote/messages.ts.

## Shared

remote-cancel = Cancel
remote-save = Save
remote-rename = Rename
remote-never = never
window-remote-pair = Pair a device
remote-fingerprint-label = Certificate fingerprint (SHA-256)

## Login page (browser)

remote-login-label = Sign in to Nail
remote-login-title = Sign in to Nail
remote-login-loading = Connecting to Nail…
remote-login-unreachable = Nail cannot be reached. Check that the app on your computer is running and that remote access is turned on.
remote-login-retry = Try again
remote-login-unexpected = Nail answered something unexpected. Try again.
remote-login-continue = Continue
remote-login-passkey-intro = Use the passkey you set up for this device.
remote-login-passkey = Sign in with a passkey
remote-login-use-password = Use a password instead
remote-login-use-passkey = Use a passkey instead
remote-login-no-passkey-support = This browser cannot use passkeys here. Sign in with your password.
remote-login-password-title = Sign in with a password
remote-login-user = User name
remote-login-password = Password
remote-login-sign-in = Sign in
remote-login-totp-title = Two-step verification
remote-login-totp-intro = Enter the 6-digit code from your authenticator app.
remote-login-totp-code = Verification code
remote-login-verify = Verify
remote-login-start-over = Start over
remote-login-retry-after = { $seconds ->
    [one] Too many attempts. Try again in { $seconds } second.
   *[other] Too many attempts. Try again in { $seconds } seconds.
}

remote-login-failed-invalid-credentials = The user name or password is wrong.
remote-login-failed-invalid-code = That code is not valid. Check it and try again.
remote-login-failed-challenge-expired = The sign-in request timed out. Try again.
remote-login-failed-rate-limited = Too many attempts from this device.
remote-login-failed-locked-out = Signing in is locked for a while after too many failed attempts. The owner of the computer has been notified.
remote-login-failed-not-paired = This browser is not paired with Nail. Pair it with a code from your computer first.
remote-login-failed-not-logged-in = Your session has ended. Sign in again.
remote-login-failed-reauth-required = Confirm it’s you to continue.
remote-login-failed-disabled = This user is disabled.
remote-login-failed-bad-request = Nail did not understand the request. Reload the page and try again.

remote-passkey-cancelled = The passkey prompt was closed. Try again when you are ready.
remote-passkey-not-supported = Passkeys do not work for this address. Sign in with your password, or ask the owner to set a host name for remote access.
remote-passkey-failed = The passkey could not be used.

## Pairing, browser side

remote-pair-browser-title = Pair this browser
remote-pair-browser-intro = This browser is not paired with Nail yet. On your computer, open Settings → Remote access and choose “Pair a device”. Then enter the code it shows.
remote-pair-browser-compare = Check that this fingerprint matches the one shown on your computer:
remote-pair-browser-mismatch = If the fingerprints differ, stop: you may not be talking to your computer.
remote-pair-browser-no-tls = This connection is not encrypted, which Nail allows only on this computer itself.
remote-pair-code = Pairing code
remote-pair-code-hint = For example K7QM-2WXR. Upper or lower case, with or without the dash.
remote-pair-device-name = Name for this device
remote-pair-device-name-hint = For example “Phone” or “Laptop”. You see this name in the list of devices.
remote-pair-default-device = Browser
remote-pair-browser-submit = Pair this browser

remote-passkey-offer-title = Add a passkey
remote-passkey-offer-intro = This browser is now paired. Add a passkey so you can sign in next time without a password.
remote-passkey-name = Name for the passkey
remote-passkey-offer-add = Add a passkey
remote-passkey-offer-skip = Not now
remote-passkey-added-title = Passkey added
remote-passkey-added = The passkey “{ $name }” is ready. Use it the next time you sign in.

## Re-authentication

remote-reauth-title = Confirm it’s you
remote-reauth-intro = This change needs a fresh sign-in as { $user }.
remote-reauth-passkey-intro = Confirm with your passkey.
remote-reauth-confirm = Confirm

## Pairing, desktop side

remote-pair-intro = A new device needs a one-time code from this computer. Choose who it belongs to and what it may do.
remote-pair-user = User
remote-pair-device-name-desktop-hint = Optional. Leave it empty to use the name the browser suggests.
remote-pair-ttl = Code valid for
remote-pair-ttl-minutes = { $minutes } minutes
remote-pair-create = Create pairing code
remote-pair-no-users = There is no remote user yet. Create one under Users first.
remote-pair-open-dialog = Pair a device
remote-pair-code-title = Pairing code
remote-pair-open = On the new device, open { $url } and enter this code:
remote-pair-open-not-running = Remote access is not running. Turn it on, then open its address on the new device and enter this code:
remote-pair-expires = Valid until { $time }, and only once.
remote-pair-qr-label = QR code for { $url }
remote-pair-fingerprint-intro = The browser shows this certificate fingerprint. Pair only if it matches:
remote-pair-no-fingerprint = Remote access runs without TLS, so there is no fingerprint to compare.
remote-pair-once = This code is shown only now. Anyone who has it can pair a device until it is used or expires.
remote-pair-cancel-code = Cancel code
remote-pair-close = Close

remote-pairing-title = Codes waiting to be used
remote-pairing-pending = { $device } for { $user }, until { $time }
remote-pairing-pending-any = A device for { $user }, until { $time }
remote-pairing-state-pending = Waiting for the new device…
remote-pairing-state-used = Paired. The new device is signed in.
remote-pairing-state-expired = This code expired before it was used.
remote-pairing-state-cancelled = This code was cancelled.

## Access presets

remote-access = Access
remote-preset-read-only = Read only
remote-preset-read-only-hint = Read mail, attachments and message sources. Nothing can be changed or sent.
remote-preset-no-send = Everything but sending
remote-preset-no-send-hint = Read and organize mail, write drafts, manage rules, calendar and contacts. Sending is not allowed.
remote-preset-full = Full access
remote-preset-full-hint = Everything, including sending mail.
remote-preset-custom = Custom
remote-preset-custom-hint = Narrowed to some accounts or folders. The settings stay as they are.
remote-access-admin = Administration (settings, accounts, keys, remote users)
remote-access-admin-short = administration
remote-access-admin-not-allowed = This user may not administer Nail.
remote-access-narrowing = Narrowing access also narrows the user’s devices and sessions at once.

## Settings

remote-settings-title = Remote access
remote-settings-intro = Use Nail from a browser on another device. Off by default; each new device must be paired with a code from this computer.
remote-settings-enabled = Serve Nail to browsers on other devices
remote-settings-network = Network
remote-settings-bind = Listen on
remote-bind-loopback = This computer only (127.0.0.1)
remote-bind-lan = All networks of this computer (0.0.0.0)
remote-bind-custom = One address…
remote-settings-bind-address = Address
remote-settings-bind-address-hint = An IP address of this computer, for example 192.168.1.20.
remote-settings-lan-warning = Other devices on your network can reach Nail. Windows may ask to allow Nail through the firewall.
remote-settings-port = Port
remote-settings-host-name = Host name
remote-settings-host-name-hint = The name other devices use to reach this computer, for example nail-desktop.local. Passkeys need a name, not an IP address. Empty: this computer’s name.
remote-settings-tls = Encryption (TLS)
remote-tls-self-signed = Certificate made by Nail
remote-tls-self-signed-hint = Nail makes its own certificate. Browsers warn about it once; compare its fingerprint when you pair.
remote-tls-off = Off
remote-tls-off-hint = Only allowed when Nail listens on this computer only.
remote-tls-acme = Let’s Encrypt
remote-tls-acme-hint = Only available in internet mode.
remote-settings-networks = Allowed networks
remote-settings-networks-hint = One network per line, for example 192.168.1.0/24. Empty: any device that can reach the port.
remote-settings-sessions = Sessions
remote-settings-idle = Sign out after this many idle minutes
remote-settings-max-hours = Sign out after this many hours at the latest
remote-settings-audit-retention = Keep the security log for this many days
remote-settings-reauth = Ask again for sensitive changes after this many minutes
remote-settings-issues = Problems
remote-settings-save = Save
remote-settings-reset = Undo changes
remote-settings-saved = Remote access settings saved.

remote-issue-invalid-bind-address = The address must be an IP address of this computer.
remote-issue-invalid-port = The port must be a number from 1 to 65535.
remote-issue-tls-required = Encryption is required when other devices can connect.
remote-issue-internet-mode-locked = Internet mode is locked.
remote-issue-acme-needs-internet-mode = Let’s Encrypt certificates need internet mode, which is locked.
remote-issue-acme-incomplete = Let’s Encrypt needs a domain name and accepted terms.
remote-issue-invalid-network = Each allowed network must look like 192.168.1.0/24.
remote-issue-public-network = Allowed networks must be local networks (such as 192.168.1.0/24) while internet mode is locked.
remote-issue-invalid-host-name = The host name is not valid (letters, digits and dashes; not an IP address).
remote-issue-invalid-session-limits = Session limits must be at least 1, and the idle limit cannot be longer than the maximum.
remote-issue-invalid-audit-retention = Keep the security log for 1 to 3650 days.

remote-status-title = Status
remote-state-stopped = Not running.
remote-state-starting = Starting…
remote-state-running = Running.
remote-state-failed = Could not start.
remote-state-failed-because = Could not start: { $error }
remote-status-urls = Addresses
remote-status-sessions = { $count ->
    [0] Nobody is signed in.
    [one] One session is signed in.
   *[other] { $count } sessions are signed in.
}
remote-certificate-self-signed = Certificate made by Nail
remote-certificate-acme = Certificate from Let’s Encrypt
remote-certificate-valid = Valid from { $from } until { $until }
remote-certificate-names = For { $names }
remote-certificate-regenerate = Make a new certificate
remote-certificate-regenerate-confirm = Browsers that trusted the old certificate will warn again, and paired devices should compare the new fingerprint. Continue?
remote-certificate-regenerate-yes = Make a new certificate
remote-certificate-regenerated = A new certificate is in use.

remote-internet-title = Internet access
remote-internet-intro = Reaching Nail from outside your network, with a certificate from Let’s Encrypt.
remote-internet-mode = Allow access from the internet
remote-internet-lock-pentest-pending = Locked: internet access stays off until an external security test (penetration test) of Nail has passed.
remote-internet-advice = To reach Nail while away, use a VPN such as WireGuard or Tailscale into your home network instead.

## Users

remote-users-title = Users
remote-users-intro = People who may use Nail from other devices. There are no default passwords: a new user signs in first from a device paired with a code.
remote-users-empty = No remote users yet.
remote-user-methods = { $passkeys ->
    [0] No passkey
    [one] 1 passkey
   *[other] { $passkeys } passkeys
} · { $password ->
    [yes] password
   *[no] no password
} · { $totp ->
    [yes] two-step
   *[no] no two-step
}
remote-user-disabled-badge = disabled
remote-user-create-title = Add a user
remote-user-name = Login name
remote-user-display-name = Display name (optional)
remote-user-create = Add user
remote-user-name-taken = A user with this name already exists.
remote-user-detail = User { $user }
remote-user-meta = Login name { $name } · added { $created } · last sign-in { $login }
remote-user-locked = Signing in is locked until { $until } after too many failed attempts.
remote-user-unlock = Unlock now
remote-user-unlocked = The user can sign in again.
remote-user-access-save = Save access
remote-user-access-saved = Access saved.
remote-user-disable = Disable user
remote-user-enable = Enable user
remote-user-disabled = The user is disabled and was signed out everywhere.
remote-user-enabled = The user is enabled again.
remote-user-delete = Delete user
remote-user-delete-confirm = Delete { $user } with all devices and sessions? They are signed out at once.
remote-user-delete-yes = Delete user

remote-password-title = Password
remote-password-set = A password is set.
remote-password-none = No password yet: this user can sign in only with a passkey.
remote-password = Password
remote-password-new = New password
remote-password-repeat = Repeat the password
remote-password-hint = At least { $min } characters. A long passphrase is best.
remote-password-save = Set password
remote-password-saved = Password saved.
remote-password-too-short = The password needs at least { $min } characters.
remote-password-mismatch = The passwords do not match.
remote-password-remove = Remove password
remote-password-remove-confirm = Remove the password of { $user }? Signing in then needs a passkey.
remote-password-remove-yes = Remove password
remote-password-removed = Password removed.

remote-totp-title = Two-step verification (authenticator app)
remote-totp-on = On: signing in with the password also needs a code from the app.
remote-totp-off = Off.
remote-totp-start = Set up
remote-totp-replace = Set up again with a new app
remote-totp-enroll-title = Set up an authenticator app
remote-totp-scan = Scan this QR code with an authenticator app, then enter the code it shows.
remote-totp-qr-label = QR code for the authenticator app
remote-totp-manual = Or add it by hand: account { $account }, issuer { $issuer }, key:
remote-totp-secret = Key for manual entry
remote-totp-once = This key is shown only now.
remote-totp-code = Code from the app
remote-totp-confirm = Turn on
remote-totp-wrong-code = That code does not match. Check the time on your phone and try again.
remote-totp-enabled = Two-step verification is on.
remote-totp-disable = Turn off
remote-totp-disable-confirm = Turn off two-step verification for { $user }?
remote-totp-disable-yes = Turn off
remote-totp-disabled = Two-step verification is off.

remote-passkeys-title = Passkeys
remote-passkeys-none = No passkeys. They are added from the device itself after signing in.
remote-passkey-meta = Added { $created } · last used { $used }
remote-passkey-rename-label = Rename passkey { $name }
remote-passkey-new-name = New name
remote-passkey-delete = Delete
remote-passkey-delete-confirm = Delete the passkey “{ $name }”? It can no longer be used to sign in.
remote-passkey-delete-yes = Delete passkey

## Devices and sessions

remote-devices-title = Devices
remote-devices-empty = No device is paired.
remote-device-meta = { $user } · paired { $paired } · last seen { $seen } { $ip }
remote-device-rename-label = Rename { $name }
remote-device-new-name = New name
remote-device-revoke = Revoke
remote-device-revoke-confirm = Revoke { $name }? It is signed out at once and must be paired again with a new code.
remote-device-revoke-yes = Revoke device
remote-device-revoked = Revoked { $time }

remote-sessions-title = Signed-in sessions
remote-sessions-include-ended = Also show ended sessions
remote-sessions-empty = Nobody is signed in.
remote-session-label = Session of { $user } on { $device }
remote-session-who = { $user } on { $device }
remote-session-meta = Signed in { $started } · last active { $seen } { $ip }
remote-session-expires = Ends { $time } at the latest
remote-session-revoke = Sign out
remote-session-revoke-label = Sign out { $user } on { $device }
remote-session-ended = Ended { $time }: { $reason }
remote-sessions-revoke-all = Sign out everyone
remote-sessions-revoke-all-confirm = { $count ->
    [one] Sign out the one signed-in session?
   *[other] Sign out all { $count } signed-in sessions?
}
remote-sessions-revoke-all-yes = Sign out everyone

remote-method-passkey = Passkey
remote-method-password = Password
remote-method-password-totp = Password and code
remote-method-pairing = Pairing code

remote-end-logout = signed out
remote-end-revoked = signed out from this computer
remote-end-expired = expired
remote-end-device-revoked = device revoked
remote-end-user-changed = user disabled or removed

## Audit

remote-audit-title = Security log
remote-audit-intro = Every sign-in, failed sign-in, pairing and change to remote access.
remote-audit-filter-user = User
remote-audit-all-users = All users
remote-audit-refresh = Refresh
remote-audit-empty = Nothing recorded yet.
remote-audit-load-older = Load older entries
remote-audit-summary = { $count ->
    [one] 1 entry
   *[other] { $count } entries
}, { $failures ->
    [0] no failures
    [one] 1 failed or denied
   *[other] { $failures } failed or denied
}
remote-audit-unknown-user = unknown
remote-audit-col-time = Time
remote-audit-col-event = Event
remote-audit-col-user = User
remote-audit-col-origin = From
remote-audit-col-outcome = Outcome
remote-audit-kind-login = Sign-in
remote-audit-kind-reauth = Confirmation
remote-audit-kind-logout = Sign-out
remote-audit-kind-pairing-created = Pairing code created
remote-audit-kind-pairing = Device paired
remote-audit-kind-lockout = Locked out
remote-audit-kind-password-changed = Password changed
remote-audit-kind-totp-changed = Two-step verification changed
remote-audit-kind-passkey-changed = Passkey changed
remote-audit-kind-session-ended = Session ended
remote-audit-kind-device-changed = Device changed
remote-audit-kind-user-changed = User changed
remote-audit-kind-settings-changed = Settings changed
remote-audit-kind-request = Action
remote-audit-outcome-success = Succeeded
remote-audit-outcome-failure = Failed
remote-audit-outcome-denied = Denied

## Alerts

remote-alerts-label = Security alerts
remote-alert-title-new-device = New device signed in
remote-alert-title-lockout = Sign-in locked
remote-alert-new-device = { $device } was paired and signed in as { $user }.
remote-alert-new-device-ip = { $device } was paired and signed in as { $user } from { $ip }.
remote-alert-lockout-user = Too many failed sign-ins for { $user }. Signing in is locked until { $until }.
remote-alert-lockout-ip = Too many failed sign-ins from { $ip }. That address is locked out until { $until }.
remote-alert-lockout = Too many failed sign-ins. Signing in is locked until { $until }.
remote-alert-review = Review devices
remote-alert-dismiss = Dismiss

## Remote content in the reader

remote-content-label = Remote content
remote-content-actions = Load remote content
remote-content-blocked = { $count ->
    [one] Nail blocked 1 image or other file from the internet in this message.
   *[other] Nail blocked { $count } images or other files from the internet in this message.
}
remote-content-blocked-unknown = Nail blocked images and other files from the internet in this message.
remote-content-privacy = Loading them tells nobody your address: Nail fetches them for you.
remote-content-allow-message = Load for this message
remote-content-allow-sender = Always load from { $sender }
remote-content-allow-domain = Always load from { $domain }
remote-content-allowed-message = Remote content is loaded for this message.
remote-content-allowed-sender = Remote content is loaded because you allowed { $sender }.
remote-content-allowed-domain = Remote content is loaded because you allowed { $domain }.
remote-content-block-message = Block again
remote-content-block-sender = Stop loading from this sender
remote-content-block-domain = Stop loading from this domain
remote-content-allow-once = Load once
remote-content-allowed-once = Remote content is loaded this time only.
remote-content-block-once = Block again

## Privacy settings (allow-lists)

remote-allows-title = Remote content
remote-allows-intro = Messages, senders and domains whose images and other files from the internet Nail loads.
remote-allows-empty = Nothing is allowed: remote content is blocked in every message.
remote-allows-scope-message = Messages
remote-allows-scope-sender = Senders
remote-allows-scope-domain = Domains
remote-allows-since = allowed { $time }
remote-allows-remove = Remove
remote-allows-remove-label = Stop loading remote content for { $value }

## Settings tabs and actions (labels referenced from layout.kdl)

remote-settings-tab = Remote access
remote-settings-tab-users = Remote users
remote-settings-tab-devices = Devices
remote-settings-tab-audit = Security log
remote-settings-tab-privacy = Privacy
remote-action-pair = Pair a device
remote-action-load-content = Load remote content
remote-action-logout = Sign out
