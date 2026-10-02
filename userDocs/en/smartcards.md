# Smartcards and security tokens

Nail can sign and decrypt mail with keys that live on a smartcard or USB
security token (an OpenPGP card, a PIV card, a YubiKey and the like).
The private key never leaves the card: Nail sends the card what to sign
or decrypt, and the card asks for its PIN.

Smartcards work in the Nail desktop app on Windows. They are not
available in the browser when you use Nail remotely.

## What you need

- The card's **PKCS#11 module**: a DLL from the card vendor or from
  [OpenSC](https://github.com/OpenSC/OpenSC), for example
  `C:\Program Files\OpenSC Project\OpenSC\pkcs11\opensc-pkcs11.dll`.
- For **S/MIME**: the certificate of the key — on the card, or imported
  into Nail's keyring before.
- For **OpenPGP**: your OpenPGP public key, imported into Nail's keyring
  before (the card holds the private keys, not the public key with your
  name and address).

## Setting it up

1. In **Settings → Smartcards**, add the PKCS#11 module with its full path (**Add module**; in the desktop app **Browse…** opens the file dialog) and **Save**.
   Nail shows the modules it could not load and why.
2. Insert the card. Nail lists the card with its keys and certificates:
   the algorithm, what each key may do (sign, decrypt) and, for
   certificates, the owner, the issuer, the e-mail addresses and the
   validity.
3. Choose **Use for S/MIME** or **Use for OpenPGP** on a key. The key
   then appears in your keyring as your own key, marked as being on a
   smartcard. For an OpenPGP card, use both its signing key and its
   encryption key; they belong to the same OpenPGP key.

## Using it

Signing and decrypting work as with any other own key. The first time a
card is needed, Nail asks for its PIN and tells you which card and which
key it is for, and what for (signing or decrypting). If you type a wrong
PIN, Nail asks again and warns you when only one try is left; after too
many wrong PINs the card locks its PIN, and you need the card's own tool
(and its PUK) to unlock it.

**PINs are never stored.** Instead, Nail keeps the card unlocked for a
while after you entered the PIN, so you are not asked for every message.
Set how long in **Settings → Smartcards** (**Keep a card unlocked for**, from 0 = ask every time to 480 minutes).
Removing the card ends it at once.

If you cancel the PIN prompt or the card is not inserted, the message is
shown as locked; insert the card and open the message again.

## Limits

- Supported keys: RSA, and ECDSA / ECDH on the curves P-256 and P-384.
  OpenPGP keys of the Ed25519 / Curve25519 kind cannot be used on a card
  in this version.
- Encrypted OpenPGP mail can be decrypted on the card with RSA keys.
- Keys that need the PIN for every single use ("always authenticate", for
  example the signature key of some PIV cards) are not supported yet.
- PIN pads on card readers work as far as the PKCS#11 module supports
  them.
