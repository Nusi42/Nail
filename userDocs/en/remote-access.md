# Remote access and remote content

Nail can serve its window to a browser on another device — your phone,
a tablet, a second computer — so you can read and answer mail there.
It is **off** until you turn it on, and every new device has to be
**paired** with a one-time code shown on your computer.

This page also explains **remote content**: pictures and other files that
a message would load from the internet, which Nail blocks until you
allow them.

## Turning remote access on

Open **Settings → Remote access**.

- **Serve Nail to browsers on other devices** switches it on or off.
- **Listen on** decides who can connect:
  - *This computer only (127.0.0.1)* — the default; only browsers on this
    computer.
  - *All networks of this computer (0.0.0.0)* — every device in your home
    or office network. Windows may ask once whether Nail may accept
    connections.
  - *One address…* — one network address of this computer.
- **Port** — 4780 unless you change it. If you also use Nail in the
  browser on this computer (`nail serve`, which uses 4780 as well), choose
  another port for remote access.
- **Host name** — the name other devices use to reach this computer,
  for example `nail-desktop.local`. **Passkeys need a name**; they do not
  work with a bare IP address.
- **Encryption (TLS)** — Nail makes its own certificate. Encryption can
  only be switched off when Nail listens on this computer only.
- **Allowed networks** — optionally limit access to networks such as
  `192.168.1.0/24`, one per line. Only local networks can be listed while
  internet access is locked (see below).
- **Sessions** — how long a device stays signed in when idle, at most,
  and how long a fresh confirmation lasts for sensitive changes.

Saving starts remote access at once (or restarts it with the new
settings); switching it off stops it and signs nobody out — devices sign in
again the next time. Nail on this computer itself never needs a sign-in.

The **Status** part shows whether remote access runs, the addresses to
open, how many devices are signed in, and the **certificate fingerprint**
(see *Pairing* below). *Make a new certificate* replaces it; browsers
warn once more afterwards.

If something is wrong — a TLS setting that is not allowed, an invalid
network, a host name that is really an IP address — Nail lists the
problem and does not save until it is fixed.

### Internet access is locked

The option **Allow access from the internet** is shown but locked: it
stays off until an external security test (penetration test) of Nail has
passed. To use Nail while away, connect to your home network with a VPN
such as WireGuard or Tailscale and use the network address.

Until then Nail only accepts connections from local network addresses:
this computer, private home and office networks (`10.x.x.x`,
`172.16.x.x`–`172.31.x.x`, `192.168.x.x`), link-local addresses, the
`100.64.x.x` range that Tailscale and some providers use, and private IPv6
networks (`fd…`, `fe80::…`). Connections from any other address are
dropped, even if a router forwards the port or the computer has a public
IPv6 address.

## Users

Under **Users**, add the people who may use Nail remotely. Each user
has:

- **Access** — *Read only* (read mail, attachments and sources),
  *Everything but sending*, or *Full access*. **Administration** (settings,
  accounts, keys, remote users) is a separate choice. Narrowing a user's
  access narrows their devices and sessions at once.
- **Password** — at least 12 characters. There are **no default
  passwords**: a new user has none and first signs in from a device paired
  with a code.
- **Two-step verification** — scan the QR code with an authenticator app
  and enter the code it shows. From then on, signing in with the password
  also asks for a code.
- **Passkeys** — added from the device itself after signing in; here you
  can rename or delete them.

After too many failed sign-ins a user (or a network address) is locked
out for a while and you get a notification; **Unlock now** lifts it.
*Disable user* signs the user out everywhere; *Delete user* removes the
user with all devices.

## Pairing a device

1. On your computer choose **Pair a device** (under *Users* or *Devices*).
   Pick the user, optionally a name for the device, what it may do, and
   how long the code is valid.
2. Nail shows a code like `K7QM-2WXR`, the address to open (also as a QR
   code) and the **certificate fingerprint**.
3. On the new device open the address. The browser shows *Pair this
   browser* with a fingerprint: **it must be the same as on your
   computer**. If it differs, stop — you may not be talking to your
   computer.
4. Enter the code (upper or lower case, with or without the dash) and a
   name for the device, then *Pair this browser*.
5. Nail offers to add a **passkey** so the next sign-in needs no password.

A code works once and only for a few minutes. *Cancel code* withdraws it.
Your computer shows a **New device signed in** alert whenever a device is
paired.

## Signing in

A paired browser offers **Sign in with a passkey** first. *Use a password
instead* asks for the user name and password, then — with two-step
verification — for the code from the app. After several wrong attempts
Nail makes you wait and shows for how long.

Some changes (for example access rights) ask you to **confirm it's you**
again, with your passkey or password.

## Devices and sessions

Under **Devices** you see every paired browser with its user, when it was
paired and last seen, and from which address. **Revoke** signs it out at
once; it must be paired again with a new code.

**Signed-in sessions** lists who is signed in where and how (passkey,
password, password and code, pairing code). *Sign out* ends one session;
*Sign out everyone* ends them all. *Also show ended sessions* shows past
sessions and why they ended.

## Security log

**Security log** records every sign-in, failed sign-in, pairing, lockout
and change to remote access, newest first, with the address and browser.
Filter by user; *Load older entries* goes further back.

## Remote content in messages

Pictures, fonts and style sheets that a message loads from the internet
can tell the sender when and where you read it. Nail **blocks them** and
shows a bar above the message:

- **Load for this message** — this message only (it stays allowed when
  you open it again).
- **Always load from** *sender* — every message from this address.
- **Always load from** *domain* — every message from this domain (for
  example `shop.example`; `news.shop.example` is a separate domain).

Allowed content is fetched **by Nail**, not by your browser, so the sender
never sees your address. The bar then says why the content is shown and
offers to block it again.

**Settings → Privacy** lists everything you allowed — messages, senders
and domains — and removes entries.

## More about security

- [Remote sign-in security](remote-sign-in-security.md) — how passwords,
  codes, passkeys and pairing codes are protected, session limits,
  lockouts and what the security log records.
- [Secure connections and remote content](remote-connections.md) — HTTPS,
  the certificate fingerprint, allowed networks, the browser protections
  and how remote content is fetched.
