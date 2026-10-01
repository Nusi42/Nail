# Nail — English texts built by nail-dav (M3a WP72; contract ADR 0058):
# CalDAV / CardDAV discovery, sign-in and sync problems.
#
# Owned by WP72; ids start with `dav-`.

dav-not-found = No calendar or contacts server was found at this address.
dav-network = The calendar or contacts server cannot be reached. Nail will try again later.
dav-timeout = The calendar or contacts server did not answer in time. Nail will try again later.
dav-tls = The secure connection to the calendar or contacts server failed. Check the address and the server's certificate.
dav-auth-failed = The calendar or contacts server did not accept the user name or password.
dav-forbidden = The calendar or contacts server does not allow this change.
dav-missing = This calendar, address book or item no longer exists on the server.
dav-conflict = This item was changed on the server at the same time. The server's version is shown; your version is kept as a conflict copy.
dav-sync-token-invalid = The server forgot the previous sync state. Nail is syncing this collection again from the start.
dav-rate-limited = The calendar or contacts server is busy. Nail will try again later.
dav-too-large = This item is too large for the calendar or contacts server.
dav-http-status = The calendar or contacts server answered with an unexpected status ({ $status }).
dav-bad-response = The calendar or contacts server sent an answer Nail does not understand.
dav-insecure-url = The address must start with https:// (plain http:// only works for this computer).
dav-token = Nail could not get a sign-in token for the calendar or contacts server. Sign in again.
dav-redirect-login = The server sent Nail on to { $host }, which asks for your password. Nail does not send your password to another server on its own: enter an address on { $host } as the server address if you trust it.
