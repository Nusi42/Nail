# Nail — English texts of messages built in Rust by nail-pgp (OpenPGP key
# import and errors) and nail-mime (opened message files) (M2c carry-over
# WP65; contract ADR 0043). They reach the UI as LocalizedMessage (Fluent id
# + arguments).
#
# Arguments named `detail` are a library's reason (rPGP, the base64
# decoder, the operating system) and stay English.
#
# Owned by WP65; ids start with `pgp-` or `message-file-`.

## OpenPGP key import: why a key was skipped (KeyImportIssue)

pgp-import-unreadable = this is not readable OpenPGP key data ({ $detail })
pgp-import-no-key = no OpenPGP key found
pgp-import-unusable = the key cannot be used ({ $detail })
pgp-import-keychain = the key could not be protected in the system keychain
pgp-import-store = the key could not be saved
# The whole import was refused: the data was sent as base64 but is not.
pgp-import-not-base64 = The key data is not valid base64 ({ $detail }).

## OpenPGP errors (nail_pgp::Error)

# `key` is the id of the own key that needs its passphrase.
pgp-error-locked = The secret key { $key } is locked. Enter its passphrase and try again.
pgp-error-bad-passphrase = The passphrase is wrong.
pgp-error-crypto = Signing, encryption or verification with OpenPGP failed ({ $detail }).
pgp-error-invalid = The OpenPGP data is not valid ({ $detail }).
pgp-error-unsupported = This OpenPGP feature is not supported ({ $detail }).

## Opened message files (.eml; nail_mime::read_message_file)

# `path` is the file as the system names it, `max` the size limit in bytes.
message-file-unreadable = The file { $path } cannot be read ({ $detail }).
message-file-size = The file { $path } is empty or larger than { $max } bytes.
message-file-not-mail = The file { $path } is not an email message.
