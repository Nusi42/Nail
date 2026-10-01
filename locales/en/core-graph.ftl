# Nail — English texts of the Microsoft Graph engine built in Rust (M3c;
# contract ADR 0072): the engine's errors (`graph-*`, GraphError::localized).
#
# Owned by WP91 (nail-proto-graph).

## Microsoft Graph engine errors (GraphError::localized)

graph-network = Microsoft 365 could not be reached. Check the internet connection.
graph-timeout = Microsoft 365 did not answer in time.
graph-tls = The secure connection to Microsoft 365 failed.
graph-auth-failed = Microsoft refused the sign-in. Sign in to the account again.
graph-forbidden = Microsoft 365 denied access. The account may lack a permission, or an administrator has to consent to it.
graph-missing = The item no longer exists in Microsoft 365.
graph-mailbox-unavailable = This mailbox cannot be reached through Microsoft 365. It may be an on-premises mailbox; use Exchange Web Services instead.
graph-sync-expired = The sync state expired. Nail syncs the folder again from the start.
graph-throttled = Microsoft 365 asks Nail to slow down. Nail tries again shortly.
graph-unavailable = Microsoft 365 is unavailable right now. Nail tries again later.
graph-rejected = Microsoft 365 rejected the request.
graph-conflict = The item was changed elsewhere in the meantime.
graph-too-large = The message is too large for Microsoft 365.
graph-http-status = Microsoft 365 answered with an unexpected status ({ $status }).
graph-bad-response = Microsoft 365 sent an answer Nail could not read.
graph-insecure-url = The Microsoft Graph address is not secure. Only https addresses are allowed.
graph-config = This account is not set up for Microsoft Graph.
graph-token = Signing in to Microsoft failed.
graph-unsupported = Microsoft 365 does not support this.
