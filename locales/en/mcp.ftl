# Nail — English UI strings: MCP clients, grants and audit log (M1b WP23).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/mcp/messages.ts.

## Settings → MCP clients

# Layout label of the settings tab (`label="@mcp.settings.tab"`).
mcp-settings-tab = MCP clients
mcp-clients-label = MCP clients
mcp-clients-intro = AI agents and tools connect to Nail over MCP. Each client gets its own token and sees nothing until you grant it access. Every call is logged below.
mcp-clients-empty = No MCP clients yet.
mcp-server-running = The MCP server listens on { $url }
mcp-server-stopped = The MCP server is not running.

mcp-client-name = Client name
mcp-client-create = Create client
mcp-client-name-taken = A client with this name already exists.
mcp-client-state-granted = Has access
mcp-client-state-no-access = No access yet
mcp-client-state-revoked = Revoked
mcp-client-token-hint = Token …{ $hint }
mcp-client-last-used = Last used { $time }
mcp-client-never-used = Never used
mcp-client-revoke = Revoke
mcp-client-revoke-confirm = Revoke “{ $name }”? Its token stops working at once; its log entries stay.
mcp-client-revoke-yes = Revoke client
mcp-client-cancel = Cancel

## The token, shown once

mcp-token-title = Token for “{ $name }”
mcp-token-once = Copy this token now. Nail keeps only a fingerprint of it and cannot show it again.
mcp-token-copy = Copy token
mcp-token-copied = Copied
mcp-token-copy-failed = Copying failed. Select the token and copy it by hand.
mcp-token-done = Done
mcp-token-usage = HTTP clients send it as “Authorization: Bearer <token>”. Clients that start a program use: nail mcp --client { $name } (with the token in the NAIL_MCP_TOKEN environment variable).
mcp-token-next = The client has no access yet: choose what it may read below.

## Grants

mcp-grant-label = Access of { $name }
mcp-grant-empty = This client has no access. Choose at least one permission and one account.
mcp-grant-scopes = Permissions
mcp-scope-read = Read mail
mcp-scope-read-hint = Accounts, folders, messages, threads, search and rules.
mcp-scope-raw-source = Original message source
mcp-scope-raw-source-hint = The complete message as received, with all headers.
mcp-scope-attachments = Attachments
mcp-scope-attachments-hint = The content of attached files.
mcp-scope-encrypted = Encrypted messages
mcp-scope-encrypted-hint = Content of encrypted messages.
mcp-scope-context = What is on screen
mcp-scope-context-hint = The open message, selected text and the draft you are writing. Nail shows an indicator whenever the client reads it.
mcp-grant-accounts = Accounts
mcp-grant-all-accounts = All accounts, including ones added later
mcp-grant-account = { $name } ({ $email })
mcp-grant-folders = Hidden folders
mcp-grant-folders-of = { $account }{ $hidden ->
    [0] {""}
    [one] {" "}(1 hidden)
   *[other] {" "}({ $hidden } hidden)
}
mcp-grant-folder-hidden = Hide { $folder }
mcp-grant-save = Save access
mcp-grant-saved = Access saved.

## Audit log

mcp-audit-label = MCP activity
mcp-audit-show-all = Show activity of all clients
mcp-audit-refresh = Refresh
mcp-audit-more = Show older entries
mcp-audit-empty = No activity yet.
mcp-audit-time = Time
mcp-audit-client = Client
mcp-audit-call = Call
mcp-audit-outcome = Result
mcp-audit-detail = Detail
mcp-audit-unknown-client = Unknown client
mcp-audit-kind-resource = Read
mcp-audit-kind-subscribe = Watch
mcp-audit-kind-tool = Tool
mcp-audit-kind-prompt = Prompt
mcp-audit-kind-connect = Connect
mcp-audit-outcome-allowed = Allowed
mcp-audit-outcome-denied = Denied
mcp-audit-outcome-declined = Declined
mcp-audit-outcome-error = Failed

## On-screen indicator

mcp-indicator-label = Agent activity
mcp-indicator-reading = { $client } is reading what is on your screen
mcp-indicator-editing = { $client } is editing your draft
mcp-indicator-dismiss = Hide agent indicator

## MCP settings (M2c, nail_api::mcp::McpSettingsIssue)

mcp-settings-issue-port = Choose a port between 1024 and 65535.
