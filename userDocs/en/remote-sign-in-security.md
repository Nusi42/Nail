# How remote sign-in is protected

This page explains what Nail does to keep other people out of your mail
when you use it from a browser on another device. How to turn remote
access on, pair devices and manage users is described in
[Remote access and remote content](remote-access.md).

## No device gets in without your computer's approval

- A browser can only sign in after it was **paired** with a code that you
  created on your computer. Creating the code is your approval of the
  device that enters it.
- A pairing code works **once** and for **10 minutes** unless you chose
  another time (at most 60 minutes). It is never stored in readable form.
- A pairing code also decides the device's name and what it may do.
  Nothing a device does can give it more than the user it belongs to.
- Every newly paired device raises a **New device signed in** alert on
  your computer.

## Passwords, codes and passkeys

- **Passkeys** are the preferred way to sign in: the secret never leaves
  your phone or security key, and a passkey only works on the address it
  was made for, so a look-alike site cannot use it. Passkeys need a **host
  name** (such as `nail-desktop.local`); they do not work with a bare IP
  address.
- **Passwords** must have at least 12 characters. Nail stores only a
  one-way hash (Argon2id), never the password itself.
- With **two-step verification**, signing in with the password also needs
  the 6-digit code from your authenticator app. Each code works **once**.
  Codes are accepted up to 30 seconds early or late, so a phone clock that
  is slightly off does not lock you out. The secret behind the codes is
  kept in the Windows Credential Manager, not in Nail's database.
- Changing a user's password or turning two-step verification off signs
  that user out on every **other** device.

## Guessing is slowed down and stopped

- After **3** wrong attempts, Nail makes the next attempt wait — 1 second,
  then 2, 4, 8 … up to 5 minutes. The sign-in page shows how long.
- After **10** wrong attempts for one user, or **30** from one network
  address, that user or address is **locked out for 15 minutes**. Each
  further wrong attempt doubles the lockout, up to a day.
- Your computer shows a notification for every lockout, and the security
  log records it. **Unlock now** in *Settings → Remote access → Users*
  lifts a user's lockout at once.
- Wrong pairing codes count the same way. If many addresses guess codes
  at once, pairing pauses for a minute.

## Sessions

- A signed-in browser holds a session in a cookie that pages and scripts
  cannot read, that is only sent to Nail over an encrypted connection, and
  never along with requests from other sites.
- The session only works **together with the device's own pairing
  cookie**: a copied session cookie is useless on another browser.
- The session cookie is **replaced every few minutes** while you use Nail.
- A session ends after **30 minutes without activity** and at the latest
  **12 hours** after signing in (you can change both in the settings). The
  device stays paired; signing in again needs a passkey or the password.
- **Revoking a device** or **signing out a session** takes effect at once:
  the browser is shown the sign-in page.

## Confirming it's you

Changing settings, users, accounts or keys from a browser — and unlocking
an encryption key — needs a **recent sign-in**: within the last 5 minutes
(adjustable). Otherwise Nail asks you to confirm with your passkey or
password first. A few things are only possible on the computer itself,
such as signing in to a mail provider or opening a configuration file in
an editor.

## The security log

Every sign-in, failed sign-in, pairing, lockout, sign-out, change to
passwords, two-step verification or passkeys, and every refused request is
recorded with the time, the user, the device, the network address and the
browser. Open it in *Settings → Remote access → Security log*.
