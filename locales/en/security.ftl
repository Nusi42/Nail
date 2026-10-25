# Nail — English UI strings: keys and certificates, message security,
# sender authentication, reader warnings, encryption at rest (M2a WP34).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/security/messages.ts.

## Shared

security-cancel = { common-cancel }
security-technology-openpgp = OpenPGP
security-technology-smime = { security-technology-short-smime }
security-technology-short-openpgp = PGP
security-technology-short-smime = S/MIME
security-preference-auto = { security-compose-technology-auto }
security-preference-openpgp = Prefer OpenPGP
security-preference-smime = Prefer S/MIME

security-trust-rejected = { security-decision-rejected }
security-trust-unknown = Not verified
security-trust-autocrypt-gossip = Seen in other people’s mail (not verified)
security-trust-autocrypt = Learned from the sender’s mail (Autocrypt)
security-trust-verified = Verified by you
security-trust-own = Your own key

## Layout labels (settings tabs, actions)

security-settings-tab-keys = { security-keys-label }
security-settings-tab-identities = { security-compose-label }
security-settings-tab-protection = { security-settings-label }
security-settings-tab-autocrypt = { security-autocrypt-label }
security-settings-tab-recovery-key = { security-recovery-input }
security-settings-tab-smartcards = Smartcards
security-action-autocrypt-import = Import Autocrypt setup message
window-security-autocrypt-import = { security-action-autocrypt-import }

## Message security badge and details

security-message-label = Message security
security-badge-label = Security: { $summary }. Show details
security-badge-separator = { " · " }
security-badge-sig-verified = Signed, verified
security-badge-sig-valid = Signed, sender not verified
security-badge-sig-mismatch = Signed by someone else
security-badge-sig-unknown-key = Signed, unknown key
security-badge-sig-key-problem = Signed with an expired or revoked key
security-badge-sig-invalid = Invalid signature
security-badge-sig-error = Signature not checked
security-badge-enc-decrypted = Encrypted
security-badge-enc-locked = Encrypted, key locked
security-badge-enc-no-key = Encrypted, no key
security-badge-enc-failed = Could not decrypt
security-badge-partial = Only partly protected

security-signature-valid = The signature is valid: the message was not changed after it was signed.
security-signature-invalid = The signature is invalid. The message was changed after signing, or the signature is forged. Do not trust its content.
security-signature-unknown-key = Signed with a key that is not in your keyring, so the signature cannot be checked.
security-signature-key-expired = Signed with a key that has expired.
security-signature-key-revoked = Signed with a key its owner has revoked.
security-signature-unsupported = This kind of signature is not supported.
security-signature-error = The signature could not be checked.

security-encryption-decrypted = This message was encrypted. Nail decrypted it for display; the decrypted text is not stored.
security-encryption-no-key = This message is encrypted to a key you do not have.
security-encryption-locked = This message is encrypted to your key, which is locked. Enter its passphrase to read it.
security-encryption-failed = This message could not be decrypted.
security-encryption-unsupported = This kind of encryption is not supported.

security-headers-none = The subject and other headers are not protected.
security-headers-protected = The subject and headers are protected (encrypted or signed with the message).
security-headers-mismatch = The protected headers differ from the visible ones. The visible subject or sender may have been changed.

security-details-partial = Only a part of this message is signed or encrypted. Text outside that part (for example a footer added by a mailing list) is not protected.
security-details-inline-pgp = This message uses the older “inline PGP” format.
security-details-unsigned = The message is not signed, so its sender is not confirmed.
security-details-signer = Signed by
security-details-fingerprint = Fingerprint
security-details-trust = Trust
security-details-signed-at = Signed on
security-details-algorithm = Cipher
security-details-recipients = Encrypted for
security-details-recipient-count = { $count ->
    [0] hidden recipients
    [one] 1 key
   *[other] { $count } keys
}
security-details-signer-mismatch = The signature is by { $signer }, but the message says it is from { $from }.
security-details-unknown-key-hint = Import the sender’s key under Keys and certificates to check this signature.
security-details-mark-verified = Mark key as verified
security-details-marked-verified = Key marked as verified.

security-chain-label = Certificate chain
security-chain-valid = The certificate chain is valid.
security-chain-untrusted-root = The certificate chain ends at a root { common-system-name } does not trust.
security-chain-incomplete = The certificate chain is incomplete.
security-chain-expired = A certificate in the chain has expired.
security-chain-wrong-usage = The certificate may not be used for e-mail.
security-chain-invalid = The certificate chain is invalid.
security-chain-issued-by = Issued by { $issuer }
security-chain-validity = Valid from { $from } to { $to }
security-revocation-not-checked = Revocation was not checked (not supported yet).
security-revocation-good = Not revoked.
security-revocation-revoked = The certificate has been revoked.

security-unlock-title = Unlock key
security-unlock-passphrase = { common-passphrase }
security-unlock-remember = Keep unlocked
security-unlock-remember-0 = Only for this message
security-unlock-remember-15 = For 15 minutes
security-unlock-remember-60 = For 1 hour
security-unlock-remember-session = Until Nail closes
security-unlock-submit = Unlock
security-unlock-wrong = Wrong passphrase.

## Sender authentication (DKIM / SPF / DMARC)

security-auth-label = Sender check: { $summary }. Show details
security-auth-badge-pass = Sender confirmed
security-auth-badge-fail = Sender check failed
security-auth-badge-softfail = Sender questionable
security-auth-badge-neutral = Sender not confirmed
security-auth-badge-none = Sender not checked
security-auth-badge-temperror = Sender check unavailable
security-auth-badge-permerror = Sender check broken
security-auth-badge-policy = Sender check: policy
security-auth-badge-unknown = Sender check unknown
security-auth-summary-good = The sending domain confirmed that this message comes from it.
security-auth-summary-neutral = The sender’s domain could not be confirmed. That is common, but be careful with requests for money or passwords.
security-auth-summary-warning = The sender check did not pass cleanly. The message may not come from where it says.
security-auth-summary-danger = The sender check failed. This message probably does not come from the domain it claims.
security-auth-method-dmarc = { common-dmarc }
security-auth-method-dkim = DKIM
security-auth-method-spf = { common-spf }
security-auth-method-arc = ARC
security-auth-verdict-pass = pass
security-auth-verdict-fail = fail
security-auth-verdict-softfail = { common-soft-fail }
security-auth-verdict-neutral = { common-neutral }
security-auth-verdict-none = none
security-auth-verdict-temperror = { common-temporary-error }
security-auth-verdict-permerror = { common-permanent-error }
security-auth-verdict-policy = policy
security-auth-verdict-unknown = { common-unknown }
security-auth-domain-selector = { $domain } (selector { $selector })
security-auth-aligned = matches the sender
security-auth-not-aligned = does not match the sender
security-auth-nothing = No checks were reported.
security-auth-checked-by = Checked by { $server }

## Reader warnings

security-warnings-label = Warnings
security-warnings-title = { $count ->
    [one] Nail found 1 thing to be careful about in this message.
   *[other] Nail found { $count } things to be careful about in this message.
}
security-warning-sender-domain = The sender's address { $address } uses look-alike characters: { $domain }.
security-warning-sender-domain-resembles = The sender's address { $address } uses look-alike characters: { $domain } imitates { $resembles }.
security-warning-sender-name = The sender's name is written with look-alike or invisible characters; it reads "{ $name }".
security-warning-sender-hint = Do not trust who this message claims to be from.
security-warning-link-mismatch = A link shows { $shown } but goes to { $target }.
security-warning-lookalike = A link goes to { $host } ({ $ascii }), a name made of look-alike characters.
security-warning-lookalike-resembles = A link goes to { $host } ({ $ascii }), which imitates { $resembles }.
security-warning-suspicious-link = A link looks suspicious.
security-warning-real-target = Real target: { $href }
security-warnings-link-hint = Hover over a link to see where it really goes before you click.
security-risk-executable = “{ $name }” is a program. Opening it can run code on your computer.
security-risk-macro = “{ $name }” is an Office document that can contain macros.
security-risk-disguised-name = “{ $name }” hides its real file type.
security-risk-container = “{ $name }” is a disk image or archive that can hide programs from virus scanners.
security-risk-shortcut = “{ $name }” is a shortcut that can start programs.
security-risk-type-mismatch = “{ $name }” is not the type its name claims.
security-risk-short-executable = program
security-risk-short-macro = may contain macros
security-risk-short-disguised-name = disguised file type
security-risk-short-container = disk image or archive
security-risk-short-shortcut = shortcut
security-risk-short-type-mismatch = wrong file type
security-attachment-flag = Careful: { $risk }

## Composer

security-compose-label = Signing and encryption
security-compose-sign = Sign
security-compose-encrypt = Encrypt
security-compose-technology = Technology
security-compose-technology-auto = Automatic
security-compose-use-defaults = Use defaults
security-compose-recipients = Recipient keys
security-compose-key-usable = Key available ({ $trust })
security-compose-key-missing = No key
security-compose-key-expired = Key expired
security-compose-key-revoked = Key revoked
security-compose-key-rejected = Key rejected by you
security-compose-key-cannot-encrypt = Key cannot encrypt
security-compose-key-invalid-chain = Certificate not valid
security-compose-autocrypt-mutual = Prefers encryption
security-compose-autocrypt-discourage = Some keys are only known second-hand; encryption may not be readable everywhere.
security-compose-autocrypt-available = Encryption is possible for every recipient.
security-compose-autocrypt-encrypt = Every recipient prefers encrypted mail.
security-compose-lookup = { $count ->
    [one] Look up the missing key online
   *[other] Look up { $count } missing keys online
}
security-compose-lookup-found = Key found online
security-compose-lookup-not-found = No key found online
security-compose-lookup-failed = Online lookup failed
security-compose-lookup-gone = The key found online is no longer available.
security-compose-import-found = Use key found online
security-compose-block-checking = Checking recipient keys…
security-compose-block-no-recipients = Add a recipient to encrypt.
security-compose-block-missing = { $count ->
    [one] Cannot encrypt: no usable key for { $recipients }. Nail will not send it unencrypted — turn encryption off or get the key.
   *[other] Cannot encrypt: no usable key for { $recipients }. Nail will not send it unencrypted — turn encryption off or get the keys.
}
security-compose-block-no-own-key-openpgp = You have no OpenPGP key for this identity. Create or import one under Keys and certificates.
security-compose-block-no-own-key-smime = You have no S/MIME certificate for this identity. Import one under Keys and certificates.

## Keys and certificates

security-keys-label = Keys and certificates
security-keys-intro = Your own keys sign and decrypt your mail; other people’s keys and certificates let you encrypt to them and check their signatures.
security-keys-tools = Key tools
security-keys-list = Keys
security-keys-empty = No keys match.
security-keys-own = Yours
security-keys-search = { common-search-noun }
security-keys-technology = { common-type }
security-keys-technology-all = { common-all }
security-keys-own-only = Only my own keys

security-tool-import = { common-import }
security-tool-generate = Create key
security-tool-lookup = Find online
security-tool-windows = Windows certificates

security-key-detail-label = Key { $name }
security-key-state-revoked = This key has been revoked by its owner. Do not use it.
security-key-state-expired = This key has expired.
security-key-state-rejected = You rejected this key. Nail does not encrypt to it or trust its signatures.
security-key-state-short-revoked = { common-revoked }
security-key-state-short-expired = { confirmation-state-expired }
security-key-state-short-rejected = { security-decision-rejected }
security-key-technology = { common-type }
security-key-user-ids = Names
security-key-addresses = Addresses
security-key-fingerprint = { security-details-fingerprint }
security-key-algorithm = { security-generate-algorithm }
security-key-created = { security-recovery-created }
security-key-expires = { security-generate-expiry }
security-key-usage = Can
security-key-usage-value = { $sign ->
    [yes] { $encrypt ->
        [yes] sign and encrypt
       *[other] sign only
    }
   *[other] { $encrypt ->
        [yes] encrypt only
       *[other] neither sign nor encrypt
    }
}
security-key-source = Source
security-key-secret = Secret key
security-key-trust = { security-details-trust }
security-key-issuer = Issuer
security-key-serial = Serial number
security-source-generated = Created in Nail
security-source-imported = Imported
security-source-pkcs12 = Imported from a PKCS#12 file
security-source-windows-store = Windows certificate store
security-source-autocrypt = Autocrypt header of the sender
security-source-autocrypt-gossip = Autocrypt gossip (sent by someone else)
security-source-setup-message = Autocrypt setup message
security-source-wkd = Web Key Directory of the domain
security-source-keyserver = { security-lookup-source-keys-openpgp-org }
security-source-message = Attached to a message
security-secret-keychain = Protected by { common-keychain }
security-secret-passphrase = Protected by a passphrase
security-secret-windows-store = Kept in the Windows certificate store

security-key-trust-decision = Your decision
security-decision-unset = Not decided
security-decision-verified = Verified
security-decision-rejected = Rejected
security-decision-unset-hint = Trust follows where the key came from.
security-decision-verified-hint = Only choose this after comparing the fingerprint with its owner, in person or by phone.
security-decision-rejected-hint = Nail will neither encrypt to this key nor trust its signatures.
security-key-trust-save = Save trust
security-key-trust-saved = Trust saved.

security-key-export-openpgp = Export public key
security-key-export-smime = Export certificate
security-key-exported-label = Exported key
security-key-download = Save { $filename }
security-key-copy = { common-copy }
security-key-copied = { common-copied }
security-key-copy-failed = Copying failed. Select the text and copy it by hand.
security-key-unlocked = The key is unlocked.

security-key-delete-confirm = Delete “{ $name }” from the keyring? Your trust decision is kept if you import it again.
security-key-delete-own-confirm = “{ $name }” is your own key. Without it you cannot read mail encrypted to it or sign with it. Export a backup first.
security-key-delete-secret = Also delete my secret key
security-key-delete-yes = Delete key

## Protection of new secret keys

security-protection = Protect the secret key with
security-protection-keychain = { common-keychain-name } (no prompts)
security-protection-passphrase = A passphrase
security-protection-keychain-hint =
    Recommended. The key is protected by your { $os ->
        [windows] Windows sign-in
        [macos] macOS login
       *[other] login keyring
    }.
security-protection-passphrase-hint = You will be asked for the passphrase when the key is used. It cannot be recovered if you forget it.
security-protection-new-passphrase = { common-passphrase }
security-protection-confirm = Repeat passphrase
security-protection-empty = Enter a passphrase.
security-protection-mismatch = The passphrases do not match.

## Import

security-import-intro = Paste a public key or certificate, or choose a key file. PKCS#12 files (.p12, .pfx) bring your own S/MIME certificate with its secret key.
security-import-paste = Key or certificate text
security-import-file = Key file
security-import-summary = Imported: { $created } new, { $updated } updated.
security-import-issues = Skipped
security-import-issue = { $item }: { $message }
security-import-nothing = No key or certificate was found in this data.
security-import-p12-title = Import { $name }
security-import-p12-passphrase = Password of the file
security-import-p12-submit = Import certificate
security-import-p12-wrong = Wrong password for this file.

## Generate

security-generate-intro = Create an OpenPGP key for one of your addresses. Share its public key so others can encrypt to you.
security-generate-identity = For
security-generate-algorithm = Algorithm
security-algorithm-curve25519 = Curve 25519 (recommended)
security-algorithm-rsa4096 = RSA 4096 (for old software)
security-generate-expiry = Expires
security-expiry-365 = In 1 year
security-expiry-730 = In 2 years
security-expiry-1825 = In 5 years
security-expiry-never = { common-never }
security-generate-submit = { security-tool-generate }
security-generate-busy = Creating…
security-generate-done = Key created: { $fingerprint }

## Online lookup

security-lookup-intro = Find a person’s OpenPGP key by their address. Nothing is imported until you choose a key.
security-lookup-email = { common-email-address-short }
security-lookup-submit = { common-search }
security-lookup-busy = Searching…
security-lookup-found = Keys found
security-lookup-none = No key found for { $email }.
security-lookup-meta = { $source } · { $algorithm } · created { $created }
security-lookup-source-wkd = Web Key Directory
security-lookup-source-keys-openpgp-org = keys.openpgp.org
security-lookup-known = In your keyring
security-lookup-failed = { $source } could not be reached: { $error }
security-lookup-verify-hint = A key found online is not verified. Compare its fingerprint with its owner before you mark it as verified.
security-lookup-off = Online key lookup is turned off in the data protection settings.

## Windows certificate store

security-windows-intro = Use certificates Windows already has. Your own certificate’s private key stays in the Windows store.
security-windows-store = Store
security-windows-store-personal = My certificates
security-windows-store-other-people = Other people
security-windows-empty = This store has no certificates.
security-windows-private-key = Has a private key
security-windows-not-for-email = Cannot be used for e-mail
security-windows-imported = Already imported
security-windows-import = Use in Nail
security-windows-unsupported = The Windows certificate store is only available on Windows.

## Per-identity settings

security-identities-label = { security-compose-label }
security-identities-intro = Defaults for new messages from each of your addresses. You can change them for every message in the composer.
security-identities-empty = No accounts yet.
security-identities-save = Save { $account }
security-identities-saved = { common-saved }
security-identity-label = Signing and encryption for { $identity }
security-identity-technology = { security-compose-technology }
security-identity-sign = Sign new messages
security-identity-encrypt = Encrypt new messages when possible
security-identity-encrypt-hint = Only when every recipient has a usable key; the composer shows why not otherwise.
security-identity-openpgp-key = OpenPGP key
security-identity-smime-cert = S/MIME certificate
security-identity-key-auto = Newest usable
security-identity-no-openpgp-key = No OpenPGP key for this address yet. Create one under Keys and certificates.
security-identity-no-smime-cert = No S/MIME certificate for this address yet. Import one under Keys and certificates.
security-identity-autocrypt = Exchange keys automatically (Autocrypt)
security-identity-prefer-encrypt = Tell others I prefer encrypted mail
security-identity-protect-headers = Protect the subject of encrypted mail
security-identity-protect-headers-hint = The real subject travels inside the encrypted part; servers only see “...”.

## Data protection

security-settings-label = Data protection
security-vault-title = Encryption at rest
security-vault-intro =
    Encrypts the mail Nail stores on this computer (messages, drafts, message previews and the search index) with a key kept in { common-keychain }. It protects your mail if the disk is copied or the laptop is stolen, even without { $os ->
        [windows] BitLocker
        [macos] FileVault
       *[other] disk encryption
    }. Subjects, addresses and dates stay readable in the database so that lists and searches stay fast.
security-vault-recommended = Recommended: turn this on. It is off by default for now.
security-vault-state-plain = Off: your mail is stored unencrypted on this computer.
security-vault-state-encrypting = Encrypting your data…
security-vault-state-encrypted = On: your mail is stored encrypted.
security-vault-state-decrypting = Decrypting your data…
security-vault-key-keychain = The key is kept in { common-keychain }.
security-vault-phase = { $phase }: { $done } of { $total } ({ $percent } %)
security-vault-phase-database = Database
security-vault-phase-blobs = Stored messages
security-vault-phase-index = Search index
security-vault-keep-open = You can keep working. If Nail closes, it continues on the next start.
security-vault-failed = The migration stopped: { $error }
security-vault-turn-on = Turn on encryption at rest
security-vault-turn-off = { common-turn-off }
security-vault-confirm-on = Nail encrypts your existing data now. This can take a while for large mailboxes.
security-vault-confirm-on-yes = Encrypt now
security-vault-confirm-off = Your mail will be stored unencrypted again. Anyone with access to the disk can read it.
security-vault-confirm-off-yes = Turn off and decrypt

security-lookup-title = Finding keys online
security-lookup-policy = Look up recipients’ keys
security-lookup-policy-off = { common-never }
security-lookup-policy-on-demand = When I ask
security-lookup-policy-automatic = Automatically while writing
security-lookup-policy-off-hint = Nail never asks a server for keys.
security-lookup-policy-on-demand-hint = Nail asks only when you click “Look up” in the composer or the key manager.
security-lookup-policy-automatic-hint = Nail asks for every recipient without a key. The servers learn which addresses you write to.

security-index-title = Searching encrypted mail
security-index-intro = Decrypted mail is not searchable by default. Turn it on per account: its decrypted text then goes into the search index, which becomes sensitive.
security-index-account = { $account } ({ $email })
security-index-plain-warning = Decrypted text is indexed while encryption at rest is off. Turn on encryption at rest to protect the search index.
security-settings-error = The settings could not be saved.

## Autocrypt setup message

security-autocrypt-label = Autocrypt
security-autocrypt-send-title = Use your key on another device
security-autocrypt-send-intro = Nail sends your OpenPGP secret key to yourself, encrypted with a setup code. Open the message on your other device and enter the code there.
security-autocrypt-identity = Key of
security-autocrypt-send = Send setup message
security-autocrypt-no-key = This address has no OpenPGP key yet. Create one under Keys and certificates first.
security-autocrypt-code-title = Setup code
security-autocrypt-code-once = Write this code down now. Nail does not keep it and cannot show it again.
security-autocrypt-code-next = The setup message is in your outbox. Enter this code on the other device to import the key.
security-autocrypt-code-done = { common-done }
security-autocrypt-import-title = Import a key from another device
security-autocrypt-import-intro = Select the Autocrypt setup message sent from your other device and enter the code it showed.
security-autocrypt-select-message = Select the setup message in the message list first.
security-autocrypt-selected = Setup message: { $subject }
security-autocrypt-code = { security-autocrypt-code-title }
security-autocrypt-code-hint = 36 digits; the dashes are added for you.
security-autocrypt-import = Import key
security-autocrypt-wrong-code = The setup code is wrong.
security-autocrypt-not-setup = { common-not-autocrypt-setup }

## The Autocrypt setup message itself (written by the core; the subject
## stays the specification's fixed "Autocrypt Setup Message")

autocrypt-setup-message-body = This message contains your OpenPGP secret key for Autocrypt, encrypted with a setup code. To use the key on another device, open this message there in an Autocrypt-capable mail program and enter the setup code that was shown on the device that sent it.
autocrypt-setup-message-html-intro = This is your Autocrypt setup message. The key below is encrypted with the setup code shown on the sending device; enter that code in your mail program to import it.

## M4c (WP124): keys on smartcards

security-source-smartcard = On a smartcard
security-secret-smartcard = Stays on the smartcard

## M4c (WP124): the vault recovery key

window-security-recovery-create = { security-recovery-input }
window-security-recovery-verify = Check the recovery key
window-security-recovery-restore = Open your data with the recovery key
security-action-recovery-create = Create a recovery key…
security-action-recovery-check = Check the recovery key…
security-action-recovery-restore = Restore access with the recovery key…
security-action-recovery-keywords = recovery key vault encryption lost keychain backup
security-recovery-title = { security-recovery-input }
security-recovery-what = With encryption at rest, the key to your data is kept in { common-keychain }. If that entry is ever lost — a new user profile, a reset — the recovery key is the only way back to your data.
security-recovery-not-encrypted = Turn on encryption at rest first; then you can create a recovery key for it.
security-recovery-missing = There is no recovery key yet. If the key in { common-keychain-short } is lost, your data cannot be opened.
security-recovery-needed-banner = Your data is encrypted but its key is missing from this computer. Enter your recovery key to open it.
security-recovery-created = Created
security-recovery-checked = Last checked
security-recovery-unknown = Unknown
security-recovery-never-checked = Never — check that your copy works
security-recovery-create = Create recovery key
security-recovery-replace = Create a new recovery key
security-recovery-replace-warning = The current recovery key stops working as soon as the new one is created.
security-recovery-check = Check recovery key
security-recovery-remove = Remove recovery key
security-recovery-remove-warning = Without a recovery key, your data is lost if the key in { common-keychain-short } is lost.
security-recovery-remove-confirm = Remove it
security-recovery-keep = Keep it
security-recovery-intro = Nail creates a recovery key and shows it once. Write it down or print it, and keep it somewhere safe away from this computer. Nail does not keep a copy.
security-recovery-save-kit = Also save it in a text file (a recovery kit)
security-recovery-kit-file = Recovery kit file
security-recovery-kit-pick-title = Save the recovery kit as
security-recovery-kit-default-name = Nail recovery key.txt
security-recovery-kit-filter = { common-text-files }
security-recovery-show-once = This is your recovery key. It is shown only now.
security-recovery-key-label = { security-recovery-input }
security-recovery-kit-saved = Also saved to { $path }. Move this file somewhere safe, away from this computer.
security-recovery-copy = { common-copy }
security-recovery-copied = { common-copied }
security-recovery-copy-failed = The key could not be copied. Write it down instead.
security-recovery-stored = I have written down, printed or saved the key
security-recovery-next = Next
security-recovery-verify-intro = To be sure your copy is right, type the recovery key from it.
security-recovery-input = Recovery key
security-recovery-input-hint = 56 letters and digits; dashes and spaces do not matter.
security-recovery-verify = Check
security-recovery-verify-mismatch = This is not the key that was just shown. Check your copy.
security-recovery-show-again = Show the key again
security-recovery-done = Your recovery key works. Keep it safe.
security-recovery-check-intro = Type your recovery key to check that your copy still works. Nothing changes.
security-recovery-check-valid = This recovery key works.
security-recovery-check-invalid = This is not your current recovery key.
security-recovery-restore-needed = Your data is encrypted, but its key is missing from { common-keychain } on this computer. Type your recovery key to open your data again.
security-recovery-restore-intro = Type your recovery key to put the key to your data back into { common-keychain-short }.
security-recovery-restore = Open with recovery key
security-recovery-restore-lost = Without the recovery key the encrypted data cannot be opened by anyone, including Nail.
security-recovery-restored = Your data is open again. The key is back in { common-keychain-short }.
security-recovery-kit-title = { recovery-kit-title }
security-recovery-kit-text = Recovery key for Nail’s encrypted data, created { $created }.
security-recovery-kit-keep = Keep this page somewhere safe. Anyone with it and a copy of your data can read your data.

## M4c (WP124): smartcards

security-smartcard-title = { security-settings-tab-smartcards }
security-smartcard-intro = Use keys on a smartcard or USB token (PKCS#11) for S/MIME and OpenPGP. The private key never leaves the card; Nail asks for the PIN when it needs it and never stores it.
security-smartcard-desktop-only = Smartcards can only be used in the desktop app.
security-smartcard-modules = { security-smartcard-module-filter }
security-smartcard-modules-hint =
    The driver of your card or token, for example { $os ->
        [windows] C:\Windows\System32\opensc-pkcs11.dll
        [macos] /Library/OpenSC/lib/opensc-pkcs11.so
       *[other] /usr/lib/x86_64-linux-gnu/opensc-pkcs11.so
    }. Your card’s vendor names it.
security-smartcard-no-modules = No module yet.
security-smartcard-module-path =
    Module ({ $os ->
        [windows] DLL
       *[other] shared library
    })
security-smartcard-module-pick-title = Choose the PKCS#11 module
security-smartcard-module-filter = PKCS#11 modules
security-smartcard-add-module = Add module
security-smartcard-pin-cache = Keep a card unlocked for (minutes)
security-smartcard-pin-cache-hint = 0 asks for the PIN every time. At most 480 minutes (8 hours).
security-smartcard-cards = Cards and tokens
security-smartcard-enable-first = Add and switch on a module to see your cards.
security-smartcard-no-cards = No card found. Insert your card or token.
security-smartcard-module-problem = { $module }: { $problem }
security-smartcard-token = { $manufacturer } { $model }, serial number { $serial }
security-smartcard-pin-low = Few PIN attempts are left on this card. Type the PIN carefully.
security-smartcard-certificate = Certificate { $subject }, valid until { $until }
security-smartcard-openpgp = OpenPGP key { $fingerprint }
security-smartcard-in-keyring = In your keys
security-smartcard-use-smime = Use for S/MIME
security-smartcard-use-openpgp = Use for OpenPGP
security-smartcard-in-use = This key is already in your keys.

## M4c (WP124): PIN prompts

window-smartcard-pin = Smartcard PIN
security-pin-token = Card: { $label } ({ $serial })
security-pin-key = Key: { $key }
security-pin-attempts = { $count ->
    [one] 1 attempt left.
   *[other] { $count } attempts left.
}
security-pin-last-attempt = Last attempt: a wrong PIN locks the card.
security-pin-label = PIN
security-pin-hint = Nail waits until { $until }. The PIN is passed to the card and not stored.
security-pin-nothing = No card is asking for a PIN.
security-pin-prompt-gone = { common-pin-request-gone }
