# nail-pkcs11

PKCS#11 smartcards and tokens for Nail (plan M4c WP122; contract ADR 0094):
modules from the smartcard settings, token / key / certificate listing,
signing and decryption on the card with PIN prompts through the core
(`nail_api::smartcard::PinSource`). Used by `nail-pgp` and `nail-smime`.
