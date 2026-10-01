# Nail — English texts of the Exchange Web Services engine built in Rust
# (M3c; contract ADR 0072): the engine's errors (`ews-*`, EwsError::localized).
#
# Owned by WP92 (nail-proto-ews).

## EWS engine errors (EwsError::localized)

ews-network = The Exchange server could not be reached. Check the network connection.
ews-timeout = The Exchange server did not answer in time.
ews-tls = The secure connection to the Exchange server failed.
ews-auth-failed = The Exchange server refused the sign-in. Check the user name and password, or sign in again.
ews-forbidden = The Exchange server does not allow this. You may lack the permission for this mailbox or folder.
ews-missing = The item, folder or mailbox no longer exists on the Exchange server.
ews-rate-limited = The Exchange server asks Nail to slow down. Nail tries again later.
ews-unavailable = The Exchange server is not available at the moment. Nail tries again later.
ews-rejected = The Exchange server refused the request.
ews-conflict = The item was changed on the Exchange server in the meantime.
ews-too-large = The message is too large for the Exchange server, or the mailbox is full.
ews-http-status = The Exchange server answered with an unexpected error (HTTP { $status }).
ews-bad-response = The Exchange server sent an answer Nail does not understand.
ews-insecure-url = The Exchange Web Services address is not secure. Use https (plain http only for local test servers).
ews-config = This account is not set up for Exchange Web Services with a supported sign-in.
ews-credentials = Nail could not get the credentials for the Exchange server.
ews-unsupported = Exchange Web Services cannot do this.
ews-outlook-rule-blob = Outlook keeps rules on this mailbox that only Outlook understands. Changing the server rules from Nail would delete them.
