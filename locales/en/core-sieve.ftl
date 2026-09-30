# Nail — English texts of messages built by nail-sieve (M2c WP51; contract
# ADR 0043): why a rule cannot run on the server, ManageSieve errors.
#
# Owned by WP51; ids start with `sieve-reason-` or `sieve-error-`.
# Arguments: $position = the condition's place in the rule ("1.2" = the
# second condition inside the first group), $action = the action's number.

## Not expressible in Sieve: the rule always runs in Nail

sieve-reason-query = Condition { $position } uses a search query, which the server cannot run.
sieve-reason-attachment = Condition { $position } checks attachments, which the server cannot do.
sieve-reason-age = Condition { $position } checks the message's age, which only Nail can do.
sieve-reason-flag = Condition { $position } checks a flag; messages have no flags before they are delivered.
sieve-reason-tag = Condition { $position } checks a tag; messages have no tags before they are delivered.
sieve-reason-folder = Condition { $position } checks a folder other than the Inbox; the server only sees incoming mail.
sieve-reason-regex = Condition { $position } uses a regular expression the server's rules cannot express.
sieve-reason-invalid = Condition { $position } is not valid.
sieve-reason-header-name = Condition { $position } names a header the server cannot test.
sieve-reason-keyword = Action { $action } adds or removes a tag that cannot be stored on the server (spaces or special characters).
sieve-reason-forward = Action { $action } forwards the message as a new message, which only Nail can do.
sieve-reason-reply = Action { $action } replies with a template, which only Nail can do.
sieve-reason-notify = Action { $action } controls notifications, which Nail shows.
sieve-reason-other-account = Action { $action } moves or copies the message into another account.
sieve-reason-address = Action { $action } redirects to something that is not an email address.

## Could run on the server, but runs in Nail

sieve-reason-disabled = The rule is switched off.
sieve-reason-trigger = The rule also runs when sending, on a schedule or by age.
sieve-reason-no-arrival = The rule does not run when new mail arrives.
sieve-reason-scope = The rule only applies to folders other than the Inbox.
sieve-reason-extension = The server does not support “{ $extension }”.
sieve-reason-folder-unknown = Action { $action }: Nail does not know the folder “{ $folder }” on the server yet.
sieve-reason-redirects = The rule redirects to { $count } addresses; the server allows { $max }.
sieve-reason-sieve-off = Server-side rules are switched off for this account.
sieve-reason-unavailable = The server has no ManageSieve service.
sieve-reason-pending = The rule moves to the server once the new script is uploaded.

## ManageSieve errors

sieve-error-connect = Could not connect to { $host } (port { $port }).
sieve-error-tls = The secure connection to { $host } failed.
sieve-error-starttls = The server does not offer an encrypted connection (STARTTLS).
sieve-error-encryption = The server requires an encrypted connection first.
sieve-error-auth = The server did not accept the login.
sieve-error-mechanism = The server offers no login method Nail supports ({ $mechanisms }).
sieve-error-unavailable = This server does not offer ManageSieve.
sieve-error-script = The server rejected the rules script: { $detail }
sieve-error-quota = The server has no room for the rules script: { $detail }
sieve-error-refused = The server refused { $command }: { $detail }
sieve-error-try-later = The server is busy. Nail will try again later. ({ $detail })
sieve-error-not-managed = A script named “{ $script }” on the server was not written by Nail. Choose another script name.
sieve-error-timeout = The server did not answer within { $seconds } seconds.
sieve-error-protocol = The connection to the server broke off.
