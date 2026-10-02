# nail-pkcs11

PKCS#11 smartcards and tokens for Nail (plan M4c WP122; contract ADR 0094,
decisions ADR 0095).

- `Module`: what Nail needs from a PKCS#11 module; `CryptokiModule` loads
  the vendor DLL with `cryptoki` (Apache-2.0, no OpenSSL, no bindgen).
- `Smartcards`: the modules of `SmartcardSettings`, the token listing as
  `SmartcardToken`s, and `CardKey`s that sign / decrypt / do ECDH on the
  card. Missing logins ask a `PinPrompter` (the core's `PinSource` through
  `BlockingPinSource`); PINs are never stored, the session stays logged in
  for the PIN cache time. Call card operations from blocking threads.
- Helpers: `digest_info`, `ecdsa_raw_to_der`, `public_key_of_spki`,
  `certificate_facts`.
- Feature `testing`: `testing::SoftToken`, an in-memory token with real
  RSA / P-256 keys (PIN tries, locking, removal) for tests without
  hardware. `nail-pgp` and `nail-smime` use it in their tests.
