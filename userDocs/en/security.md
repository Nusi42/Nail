# Signing, encryption and data protection

Nail can sign and encrypt mail with **OpenPGP** and **S/MIME**, shows how
trustworthy a received message is, warns you about deceptive links and
dangerous attachments, and can encrypt everything it stores on your
computer.

> Preview note: sending signed or encrypted mail is switched on during
> milestone M2. Until then Nail refuses to send a message you asked to
> sign or encrypt — it never sends it unprotected instead.

## Keys and certificates

Open **Settings → Keys and certificates**. The list holds both kinds:

- **OpenPGP keys** (marked *PGP*) — used with most privacy-minded mail
  programs (Thunderbird, GnuPG, Proton, …).
- **S/MIME certificates** — issued by a certificate authority, common in
  companies and Outlook.

Your **own** keys (marked *Yours*) come first: they sign your mail and
decrypt mail sent to you. Everyone else's keys let you encrypt to them and
check their signatures. Search by name, address or fingerprint, filter by
type, or show only your own keys.

Select a key to see its addresses, **fingerprint**, algorithm, validity,
where it came from, how its secret part is protected, and — for S/MIME —
the certificate chain. Revocation of certificates is not checked yet;
Nail says so.

### Trust

Every key has a trust level:

| Level | Meaning |
| --- | --- |
| Your own key | A key you hold the secret part of. |
| Verified by you | You compared the fingerprint with its owner. |
| Learned from the sender's mail (Autocrypt) | Came in the sender's own mail headers. Good against casual snooping, not against a targeted attacker. |
| Seen in other people's mail | Someone else passed it on (Autocrypt "gossip"). Weakest. |
| Not verified | Imported or found online; nobody confirmed it. |
| Rejected | You rejected it: Nail never encrypts to it or trusts its signatures. |

To change it, choose **Your decision** → *Verified*, *Rejected* or *Not
decided* and **Save trust**. Only mark a key as verified after comparing
the fingerprint with its owner in person or by phone. Your decision is
kept even if you delete the key and import it again.

### Adding keys

Use the buttons above the list:

- **Import** — paste a key or certificate, or choose a file (`.asc`,
  `.gpg`, `.pem`, `.cer`, …). A **PKCS#12** file (`.p12`, `.pfx`) brings
  your own S/MIME certificate with its secret key: enter the file's
  password and choose how Nail protects the key (see below).
- **Create key** — creates an OpenPGP key for one of your addresses
  (Curve 25519 by default; RSA 4096 for old software) with an expiry date.
- **Find online** — looks a person's OpenPGP key up by address in their
  domain's *Web Key Directory* and on *keys.openpgp.org*. Nothing is
  imported until you choose **Import**, and a key found online is *not
  verified*.
- **Windows certificates** — uses certificates Windows already has
  (*My certificates* and *Other people*). The private key of your own
  certificate stays in the Windows store.

### Protecting your secret keys

When you create or import an own key, choose:

- **Windows Credential Manager** (recommended) — the key is protected by
  your Windows sign-in; no prompts.
- **A passphrase** — Nail asks for it when the key is needed (**Unlock**,
  optionally for 15 minutes, an hour or until Nail closes). A forgotten
  passphrase cannot be recovered.

### Exporting and deleting

**Export public key** (or **Export certificate**) shows the public part to
copy and a link to save it as a file — send it to people who want to write
to you encrypted. **Delete** removes a key; deleting your own key needs a
second confirmation, because mail encrypted to it can no longer be read.
Export a backup first.

## Defaults per address

**Settings → Signing and encryption** sets defaults for new messages from
each of your addresses:

- **Technology** — automatic, prefer OpenPGP, or prefer S/MIME.
- **Sign new messages** and **Encrypt new messages when possible**
  (encryption only happens when every recipient has a usable key).
- Which **OpenPGP key** and **S/MIME certificate** to use (default: the
  newest usable one for the address).
- **Exchange keys automatically (Autocrypt)** and **Tell others I prefer
  encrypted mail**.
- **Protect the subject of encrypted mail** — the real subject travels
  inside the encrypted part; mail servers only see `...`.

The settings are stored in `accounts.kdl` with the identity (see the
[accounts reference](accounts-reference.md)).

## Writing signed or encrypted mail

The composer shows **Sign** and **Encrypt** and, for every recipient,
whether Nail has a usable key: *Key available* (with its trust), *No
key*, *Key expired*, *Key revoked*, … It starts from your defaults and
from Autocrypt: when every recipient prefers encrypted mail, encryption
is switched on for you. **Use defaults** undoes your own changes.

If a recipient has no key, the composer offers **Look up missing keys
online** (or does it by itself, depending on the setting below) and
**Use key found online** for each key it found.

When encryption is on but a recipient has no usable key, Nail tells you
who, and **does not send the message unencrypted**. Turn encryption off or
get the key first.

## Reading: the badges

Above a message you may see two badges. Click one for details.

**Security badge** — for signed or encrypted mail:

- *Signed, verified* — the signature is valid and the key is trusted.
- *Signed, sender not verified* — valid, but the key is not verified yet.
  The details offer **Mark key as verified**.
- *Signed by someone else* — the signature is valid but belongs to a
  different address than the sender's. Be careful.
- *Invalid signature* — the message was changed or the signature is
  forged. Do not trust it.
- *Encrypted* — Nail decrypted the message for display. The decrypted
  text is not stored on disk unless you turn on searching encrypted mail.
- *Encrypted, key locked* — enter your key's passphrase in the details.
- *Only partly protected* — a part of the message (for example a footer a
  mailing list added) is outside the signature or encryption.

The details also show the signer, the key's fingerprint and trust, the
S/MIME certificate chain, and whether the subject was protected.

**Sender badge** — whether the sender's domain confirmed the message
(DKIM, SPF and DMARC, as checked by your own mail server). *Sender
confirmed* is good; *Sender check failed* means the message probably does
not come from the domain it claims. Only the check of your own receiving
server counts, because anyone can write such a header.

## Warnings in the reader

Nail lists what to be careful about above the message:

- a link whose text shows one address but goes to another,
- a link to a **look-alike domain** (for example with a Cyrillic "о"
  imitating a Latin "o"), shown with its real, encoded name,
- other suspicious links,
- **dangerous attachments**: programs, Office files with macros, files
  that hide their real type (`invoice.pdf.exe`), disk images and
  shortcuts.

Each link warning shows the link's **real target**.

## Data protection

**Settings → Data protection**:

- **Encryption at rest** encrypts Nail's database, stored messages and
  search index on your computer with a key kept in the Windows Credential
  Manager. It protects your mail if the disk is copied or a laptop is
  stolen, even without BitLocker. It is **off by default for now; we
  recommend turning it on.** Turning it on (or off) converts your existing
  data; the progress is shown, you can keep working, and if Nail closes it
  continues on the next start. If it stops with an error, choose **Try
  again**.
- **Finding keys online** — *Never*, *When I ask* (default), or
  *Automatically while writing*, and which directories to ask. Automatic
  lookups tell those servers which addresses you write to.
- **Searching encrypted mail** — off by default. Turn it on per account to
  add decrypted text to the search index; the index then contains
  sensitive text, so turn on encryption at rest as well.

## Using your key on another device (Autocrypt)

**Settings → Autocrypt**:

1. Under **Use your key on another device**, choose the address and
   **Send setup message**. Nail sends your OpenPGP secret key to yourself,
   encrypted with a **setup code** of 36 digits.
2. Write the code down — Nail shows it **once**.
3. On the other device, open the setup message and enter the code there.

To import a setup message sent from another device into Nail, select it
in the message list, then enter its code under **Import a key from another
device** (or use **Import Autocrypt setup message** from the message
menu).
