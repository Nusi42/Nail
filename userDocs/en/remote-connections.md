# Secure connections and remote content

This page explains how Nail protects the connection between your browser
and Nail when you use the browser UI from another device, how mail is kept
apart from the rest of the app, and how pictures and other remote content
in mail are loaded without exposing you.

## Local use stays local

`nail serve` and the desktop app listen only on your own computer
(`127.0.0.1`) unless you turn on remote access. Every request must name
that address (or `localhost`) and the right port; a web page that tries to
reach Nail under another name — a trick called *DNS rebinding* — is refused.

## HTTPS for remote access

As soon as Nail listens on your network, it only speaks HTTPS:

- **TLS 1.3**, and TLS 1.2 only with modern ciphers that keep past traffic
  secret even if a key leaks later (forward secrecy). Older protocols are
  not offered.
- **Your own certificate.** On a home or office network Nail makes a
  certificate for itself. Browsers do not know it, so when you pair a
  device, compare the **fingerprint** Nail shows on the desktop with the one
  your browser shows for the connection. If they match, you are talking to
  your Nail and nobody in between.
- The certificate is kept, so paired devices keep trusting it after a
  restart. Nail replaces it about a month before it expires or when you
  change the host name; you can also make a new one in the remote access
  settings. After that, check the new fingerprint on each device.
- **Internet access is locked.** Certificates from Let's Encrypt are only
  for use over the internet, which stays switched off in this version
  until an external security test has been passed. If you want to reach
  Nail from outside, use a VPN such as WireGuard or Tailscale to your
  network.
- You can limit remote access to certain networks (for example
  `192.168.1.0/24`); connections from anywhere else are dropped before
  anything is exchanged.

## What your browser is told

Nail sends strict security instructions with every page: only Nail's own
scripts may run, the app cannot be embedded in other sites, other sites
cannot read Nail's data, and the browser remembers to use HTTPS for Nail.
You do not need to set anything up for this.

## Mail is shown in a separate, locked box

Mail can contain hostile HTML. In the browser UI a message is shown from a
**separate address** (the same computer, another port) that has no access
to the app, your session or other messages. The message view runs no
scripts and cannot send forms. The address of a shown message works only
for your own signed-in session, and only for an hour: a copied link is
useless in another browser.

## Remote content (pictures, fonts, styles)

Remote content is **blocked by default**, because loading it tells the
sender that you opened the mail and from where. When you allow it for a
message, a sender or a whole domain, Nail loads it **for** you:

- Your browser never contacts the sender's server; Nail's proxy does, so
  the sender does not see your device's address, cookies or browser.
- Only addresses that appear in the message itself can be loaded, and only
  while you allow them. Nobody can use Nail to fetch arbitrary pages.
- Nail refuses anything that points into your own network (routers,
  printers, other computers, `localhost`), also when a web address or a
  redirect tries to disguise that.
- Only pictures, fonts and style sheets are loaded, up to 5 MB each and
  within 15 seconds. Anything else is left out.
