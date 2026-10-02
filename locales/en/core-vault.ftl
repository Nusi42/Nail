# Nail — English texts of data protection and smartcards built in Rust or
# named by the contract (M4c; contract ADR 0094): the vault recovery key,
# PKCS#11 smartcards and PIN prompts, full-database encryption.
#
# The ids named by the contract (nail_api::recovery, nail_api::smartcard)
# are pre-filled; nail-api/tests/catalog.rs checks they exist. WP122 adds
# its own texts below its heading; the UI's texts are in security.ftl.

## Recovery key (RecoveryKeyIssue)

recovery-key-issue-empty = Type the recovery key.
recovery-key-issue-invalid-character = The recovery key contains a character it cannot have.
recovery-key-issue-wrong-length = The recovery key is too short or too long.
recovery-key-issue-checksum-mismatch = The recovery key has a typo.
recovery-key-wrong = This recovery key does not open your data.
vault-not-encrypted = Turn on encryption at rest first.

## Smartcards (SmartcardSettingsIssue, SmartcardObjectKind, SmartcardKeyUsage, SmartcardError)

smartcard-settings-issue-relative-path = Give the full path of the PKCS#11 module.
smartcard-settings-issue-duplicate-module = This module is listed twice.
smartcard-settings-issue-cache-too-long = Keep the card unlocked for at most 8 hours.
smartcard-object-private-key = Private key
smartcard-object-certificate = Certificate
smartcard-object-public-key = Public key
smartcard-usage-sign = Signing
smartcard-usage-decrypt = Decryption
smartcard-error-module-not-found = The PKCS#11 module was not found.
smartcard-error-module-load-failed = The PKCS#11 module could not be loaded.
smartcard-error-token-not-present = Insert the smartcard.
smartcard-error-pin-incorrect = The PIN is wrong.
smartcard-error-pin-locked = The PIN is locked. Unlock it with the card’s own tool.
smartcard-error-pin-cancelled = The PIN was not entered.
smartcard-error-key-not-found = The key is not on the inserted card.
smartcard-error-mechanism-unsupported = The card cannot do this.
smartcard-error-device-error = The card reported an error.

## PIN prompts (PinPurpose, PinPromptCloseReason)

pin-purpose-login = Unlock the card
pin-purpose-sign = Sign the message
pin-purpose-decrypt = Decrypt the message
pin-prompt-closed-answered = Answered in another window.
pin-prompt-closed-timed-out = Nail stopped waiting for the PIN.
pin-prompt-closed-token-removed = The card was removed.
pin-prompt-closed-cancelled = The operation was cancelled.

## Vault, database encryption and smartcards (WP122)
