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

## Problems Nail detects itself (nail_pgp::Problem); only `detail` is a
## library text.

pgp-problem-not-an-address = “{ $address }” is not an email address.
pgp-problem-unsupported-domain = Keys cannot be looked up for the domain “{ $domain }”.
pgp-problem-candidate-without-key = The key found online holds no usable key.
# `attribute` is the attribute as it appears in the Autocrypt header.
pgp-problem-autocrypt-attribute = The Autocrypt header has an invalid attribute “{ $attribute }”.
pgp-problem-autocrypt-critical = The Autocrypt header has an unknown required attribute “{ $attribute }”.
pgp-problem-autocrypt-keydata = The key in the Autocrypt header is not valid base64 ({ $detail }).
# `attribute` is `addr` or `keydata`.
pgp-problem-autocrypt-missing = The Autocrypt header has no “{ $attribute }” attribute.
pgp-problem-autocrypt-secret-key = The Autocrypt header carries a secret key, which is not allowed.
pgp-problem-autocrypt-no-key = The Autocrypt header holds no key.
pgp-problem-setup-code-format = The setup code must be 36 digits.
pgp-problem-not-setup-message = This message is not an Autocrypt setup message.
pgp-problem-setup-without-payload = The Autocrypt setup message holds no encrypted key.
pgp-problem-setup-without-secret-key = The Autocrypt setup message holds no secret key.
pgp-problem-nothing-to-protect = The message is to be neither signed nor encrypted.
pgp-problem-no-self-signature = The key has no valid self-signature.
pgp-problem-no-user-id-for = The key has no valid user ID for { $address }.
pgp-problem-expiry-too-far = An expiry { $days } days from now is too far in the future.
pgp-problem-not-openpgp-key = The key { $key } is not an OpenPGP key.
pgp-problem-empty-passphrase = The passphrase must not be empty.
pgp-problem-no-secret-key = The key { $key } has no secret key.
pgp-problem-windows-store = OpenPGP keys cannot be kept in the Windows certificate store.

## Opened message files (.eml; nail_mime::read_message_file)

# `path` is the file as the system names it, `max` the size limit in bytes.
message-file-unreadable = The file { $path } cannot be read ({ $detail }).
message-file-size = The file { $path } is empty or larger than { $max } bytes.
message-file-not-mail = The file { $path } is not an email message.
