# Nail — English texts of messages built in Rust by the core, nail-config,
# the MCP server and the CLI (M2c WP53; contract ADR 0043): configuration
# errors, MCP hints, user-facing CLI output. They reach the UI as
# LocalizedMessage (Fluent id + arguments); the CLI renders them with
# nail-config's l10n module. The English rendering is also the diagnostic
# `message` of configuration errors (logs).
#
# Arguments named `what` are a property (`key`) or a node's argument
# (`node` #1) as written in the file; `found` is a value as written.
#
# Owned by WP53.

## Configuration files: syntax and reading

config-io = cannot read { $file }: { $detail }
config-syntax = syntax error: { $detail }
config-syntax-help = syntax error: { $detail } ({ $help })
config-syntax-invalid = invalid KDL document
config-syntax-nesting = syntax error: nested more than { $max } levels deep
config-file-empty = the file is empty; expected `{ $root } "<name>" {"{"} … {"}"}`
config-top-level-unexpected = unexpected `{ $node }` at the top level; the file contains a single `{ $root } "<name>" {"{"} … {"}"}` node
config-root-duplicate = only one `{ $root }` node is allowed
config-extends-unknown = unknown base `{ $base }`: only extends="{ $default }" is supported

## Configuration files: generic node and value checks

config-unknown-property-hint = unknown property `{ $key }` on `{ $node }` (allowed: { $allowed })
config-unknown-property-none = unknown property `{ $key }` on `{ $node }` (`{ $node }` takes no properties)
config-duplicate-property = property `{ $key }` is set twice
config-set-twice = `{ $node }` is set twice
config-too-many-arguments = too many arguments for `{ $node }` (expected at most { $max })
config-no-arguments = `{ $node }` takes no arguments
config-no-children = `{ $node }` does not take a children block
config-needs-child = `{ $node }` needs at least one child
config-missing-argument = `{ $node }` needs a value as argument { $index }
config-needs-value = `{ $node }` needs a value
config-needs-property = `{ $node }` needs { $key }="…"
config-unknown-node = unknown node `{ $node }` (expected { $expected })
config-unknown-child = unknown node `{ $node }` in `{ $parent }` (expected { $expected })
config-section-only = `{ $section }` may only contain `{ $expected }` nodes, found `{ $found }`
config-expected-string = { $what } must be a string, found { $found }
config-expected-string-or-number = { $what } must be a string or a number, found { $found }
config-expected-bool = { $what } must be #true or #false, found { $found }
config-expected-count = { $what } must be a non-negative whole number, found { $found }
config-expected-at-least = { $what } must be a whole number of at least { $min }, found { $found }
config-expected-range = { $what } must be a whole number from { $min } to { $max }, found { $found }
config-expected-one-of = { $what } must be one of { $choices }, found { $found }
config-expected-email = { $what } must be an email address like "name@example.org", found { $found }
config-expected-host = { $what } must be a host name or IP address, found { $found }
config-orientation-unknown = unknown orientation `{ $found }` (use horizontal or vertical)
config-role-unknown = unknown folder role "{ $found }" (expected { $expected })
config-expected-url = { $what } must be an https:// URL (http:// only for localhost), found { $found }
config-empty = { $what } must not be empty
config-no-spaces = { $what } must not contain spaces
config-number-too-large = { $what } is too large
config-number-out-of-range = number is out of range
config-number-not-finite = number must be finite

## accounts.kdl

config-account-duplicate = an account named "{ $name }" is already defined
config-account-name-taken = the name "{ $name }" is already used by another account or virtual account
config-account-name-reserved = "{ $name }" is reserved for the built-in "All accounts" virtual account
config-account-name-invalid = invalid name "{ $name }": use 1 to 64 lowercase letters, digits, `-`, `_` or `.`, starting with a letter or digit
config-account-needs-email = account "{ $name }" needs email="…"
config-account-needs-incoming = account "{ $name }" needs an incoming server: an `imap` or `pop3` node
config-account-one-incoming = an account has exactly one incoming server (`imap` or `pop3`)
config-account-one-smtp = an account has at most one `smtp` server
config-accounts-not-writable = the configuration cannot be written as accounts.kdl (it changes when read back)
config-identity-duplicate = identity id "{ $id }" is used twice; give one of them a different `id=`
config-identity-id-invalid = `id` must be non-empty and contain no spaces
config-identity-unknown = unknown identity "{ $id }"
config-identity-not-member = identity "{ $id }" belongs to account "{ $account }", which is not a member
config-identity-pattern-invalid = invalid pattern { $found } (use "*@example.com", "*@*.example.com" or a full address)
config-secret-not-allowed = `{ $key }` is not allowed: secrets are never stored in accounts.kdl. Keep them in the OS keychain and reference them with secret="keychain:nail/<account>/password"
config-secret-invalid = `{ $key }` must reference a secret in the OS keychain like "keychain:nail/<account>/password", found { $found }
config-oauth-provider-duplicate = OAuth provider "{ $id }" is defined twice
config-oauth-provider-unknown = unknown OAuth provider "{ $id }" (built in: google, microsoft; others need an `oauth-provider` node)
config-oauth-provider-id-invalid = invalid provider id "{ $id }": use lowercase letters, digits and `-`
config-oauth-provider-needs-client-id = oauth-provider "{ $id }" needs client-id="…"
config-oauth-provider-needs-urls = oauth-provider "{ $id }" is not built in and needs auth-url="…" and token-url="…"
config-oauth-needs-provider = `auth "oauth2"` needs provider="google", "microsoft" or the id of an `oauth-provider`
config-auth-method-unknown = unknown auth method { $found } (expected "password", "oauth2", "windows", "client-certificate" or "none")
config-authserv-id-empty = `authserv-id` needs at least one server id
config-authserv-id-invalid = { $found } is not a server id like "mx.example.net"
config-sieve-needs-imap = `sieve` (ManageSieve) needs an IMAP account
config-gmail-needs-oauth2 = a `gmail` account signs in with `auth "oauth2"`
config-gmail-no-smtp = a `gmail` account sends through the Gmail API; remove the `smtp` node
config-gmail-plaintext-needs-loopback = `tls="plaintext"` is only allowed for local test servers (localhost)
config-exchange-graph-needs-oauth2 = an `exchange` account with engine="graph" signs in with `auth "oauth2"` provider="microsoft"
config-exchange-unsupported-auth = an `exchange` account with engine="ews" signs in with `auth "windows"`, `auth "password"` (Basic over TLS) or `auth "oauth2"`
config-exchange-plaintext-needs-loopback = `tls="plaintext"` is only allowed for local test servers (localhost)
config-exchange-starttls = Exchange speaks HTTPS: use tls="implicit" (or "plaintext" for local test servers)
config-exchange-ews-only-option = `url` and `streaming` only apply with engine="ews"
config-exchange-invalid-url = `url` must be an https:// address of the EWS endpoint, found { $found }
config-exchange-needs-host = an `exchange` account with engine="ews" needs host="…" or url="…"
config-exchange-no-smtp = an `exchange` account sends through Exchange; remove the `smtp` node
config-auth-windows-needs-exchange = `auth "windows"` (Windows sign-in) only works for `exchange` accounts
config-sspi-package-invalid = `package` must be "negotiate", "kerberos" or "ntlm", found { $found }
config-sieve-script-invalid = `script` must be 1 to 128 characters without control characters or slashes, found { $found }
config-needs-leave-on-server = `{ $key }` only applies with leave-on-server=#true
config-bcc-empty = `bcc` needs at least one address
config-sender-needs-on-behalf = `sender` only applies with send-as="on-behalf"
config-fingerprint-invalid = `{ $key }` must be a fingerprint of { $lengths } hexadecimal digits, found { $found }
config-sync-recent-needs-days = bodies="recent" needs days=<number of days>
config-sync-days-needs-recent = `days` only applies with bodies="recent"
config-sync-rule-empty = this `folder` rule changes nothing; set bodies=, enabled= or idle=
config-path-or-role = use either `path` or `role`, not both
config-folder-needs-path-or-role = `folder` needs path="…" or role="…"
config-member-unknown = unknown account "{ $name }"
config-member-virtual = unknown account "{ $name }" (virtual accounts cannot be members)
config-member-duplicate = account "{ $name }" is listed twice
config-members-empty = `members` needs at least one account
config-virtual-needs-members = virtual account "{ $name }" needs `members "<account>" …`
config-pim-source-duplicate = a calendar or contact source named "{ $name }" is already defined
config-pim-source-reserved = "{ $name }" is reserved for the built-in local calendars and address books
config-pim-source-invalid = invalid source name "{ $name }": use 1 to 64 lowercase letters, digits, `-`, `_` or `.`, starting with a letter or digit
config-pim-account-unknown = unknown account "{ $name }"
config-pim-needs-url = calendar subscription "{ $name }" needs url="…"
config-pim-needs-url-or-account = "{ $name }" needs url="…" or account="…" (to use that account's login and find its server)
config-pim-needs-auth = "{ $name }" has no account="…", so it needs an `auth` node
config-pim-needs-username = the `auth` node of "{ $name }" needs username="…"
config-pim-color-invalid = `color` must be #RGB, #RRGGBB, #RRGGBBAA or a colour name, found { $found }
config-map-folder-duplicate = folder "{ $name }" is mapped twice
config-map-not-member = account "{ $name }" is not a member of this virtual account
config-map-entry-duplicate = this folder is listed twice
config-map-empty = `map` needs at least one `folder account="…" path="…"`
config-undo-send-needs-seconds = `undo-send-delay` needs a number of seconds

## theme.kdl

config-theme-unknown-section = unknown theme section `{ $node }` (expected appearance, tokens, modes, part, parts, icon, icons, message, sounds or raw-css)
config-mode-unknown = unknown mode `{ $found }` (use system, light, dark or high-contrast)
config-mode-override-mode-unknown = unknown mode `{ $found }` (expected light, dark or high-contrast)
config-html-dark-mode-unknown = unknown html-dark-mode `{ $found }` (use off, invert or smart)
config-message-setting-unknown = unknown message setting `{ $node }` (expected font-family, text-font-family, font-size, line-height, max-width, quote-colors or html-dark-mode)
config-part-name-invalid = invalid part name `{ $name }`: use dot-separated lowercase words like `message-list.row`
config-part-empty = part `{ $selector }` has no declarations
config-css-property-unsupported = unsupported CSS property `{ $property }` (see the theme reference for the supported subset; `raw-css` covers the rest)
config-css-character = the character `{ $character }` is not allowed in theme values
config-css-remote = `{ $construct }` is not allowed: theme files cannot load external resources
config-raw-css-character = the character `{ $character }` is not allowed in raw-css
config-token-needs-value = token `{ $token }` needs a value
config-token-needs-value-example = token `{ $category }.{ $name }` needs a value, e.g. `{ $category } "{ $name }" <value>`
config-token-name-invalid = invalid token name `{ $name }`: use lowercase letters, digits and hyphens
config-token-category-unknown = unknown token category `{ $category }` in `{ $token }` (known: { $known })
config-token-category-unknown-node = unknown token category `{ $category }` (expected one of { $known })
config-token-reference-invalid = `$` must start a token reference like `$color.accent` (write `$$` for a literal `$`)
config-unknown-token = unknown token `{ $token }`
config-token-cycle = token reference cycle: { $chain }
config-mode-override-unknown = mode override for unknown token `{ $token }`
config-icon-name-invalid = invalid icon name `{ $name }`
config-icon-needs-svg = icon `{ $name }` needs svg="<path data>"
config-svg-character = `{ $character }` is not allowed in SVG path data
config-view-box-invalid = view-box must be four numbers, e.g. "0 0 24 24"
config-message-missing = the theme's `message` section is missing `{ $field }`
config-sound-event-invalid = invalid event name `{ $event }`, e.g. mail.new
config-sound-invalid = sounds must be a bundled sound name or a path inside the config directory
config-selector-empty = a part selector must not be empty
config-selector-relative = `{ $part }` continues a previous part name, but there is none
config-selector-needs-name = `{ $selector }` needs a part name before its states or conditions
config-selector-state-invalid = invalid state `:{ $state }`: use lowercase words like `:selected`
config-selector-bracket = missing `]` in `{ $selector }`
config-selector-unexpected = unexpected `{ $text }` in part selector
config-selector-condition-value = invalid condition value in `[{ $condition }]`
config-selector-condition-key = invalid condition `[{ $condition }]`: the key must be a lowercase word like `tag`

## layout.kdl

config-layout-unknown-section = unknown layout section `{ $node }` (expected window, breakpoint, action(s), keys, toolbar(s) or menu(s))
config-layout-no-main = the layout defines no `main` window
config-breakpoint-range = min-width { $min } is larger than max-width { $max }
config-breakpoint-empty = `breakpoint` needs at least one `window`
config-window-name-invalid = invalid window name `{ $name }`
config-window-one-root = window `{ $name }` must contain exactly one root node (a container or widget)
config-id-duplicate = the id `{ $id }` is used twice in this window tree
config-id-invalid = invalid id `{ $id }`: use lowercase words like `reader-pane`
config-label-invalid = invalid message id in label `{ $label }`, e.g. "@mail.action.reply"
config-collapsible-invalid = collapsible must list `true` or `false` per child, e.g. "true false true"
config-tabs-active-range = active={ $active } but the tabs have only { $count } children
config-container-unsupported = the `{ $node }` container is not supported yet (use split, stack or tabs)
config-list-length = `{ $key }` lists { $count } values but `{ $node }` has { $children } children
config-widget-unknown = unknown widget or container `{ $node }` (containers: split, stack, tabs; widgets: { $widgets })
config-option-unknown = unknown option `{ $key }` for `{ $node }` (options: { $options })
config-option-unknown-none = unknown option `{ $key }` for `{ $node }` (it has no options)
config-option-required = `{ $node }` needs the option `{ $option }`
config-option-bool = option `{ $key }` must be #true or #false, found { $found }
config-option-count = option `{ $key }` must be a non-negative whole number, found { $found }
config-option-string = option `{ $key }` must be a string, found { $found }
config-option-one-of = option `{ $key }` must be one of { $choices }, found { $found }
config-widget-row-only = unexpected `{ $node }` inside `{ $widget }`; widgets only contain `row` templates
config-widget-no-rows = `{ $widget }` does not take row templates
config-template-nested = nested `{"{"}` in template `{ $template }`
config-template-unmatched = unmatched `{"}"}` in template `{ $template }`
config-template-unclosed = unclosed `{"{"}` in template `{ $template }`
config-action-args-need-alias = action arguments are only allowed together with alias-of
config-action-id-invalid = invalid action id `{ $id }`: use dotted lowercase names like `mail.reply`
config-action-quote = unterminated quote in `{ $spec }`
config-action-spec-start = `{ $spec }` must start with an action id
config-action-empty = the action must not be empty
config-key-context-invalid = invalid context `{ $context }`, e.g. list, reader, global
config-key-empty = the key must not be empty
config-key-invalid = invalid key `{ $key }`: join chord keys with `+`, e.g. "ctrl+shift+m"
config-keys-unexpected = unexpected `{ $node }` in `keys` (use bind or unbind)
config-toolbar-name-invalid = invalid toolbar name `{ $name }`
config-toolbar-unexpected = unexpected `{ $node }` in toolbar (use item, spacer or separator)
config-toolbar-unknown = unknown toolbar `{ $name }` (define it with `toolbar "{ $name }" {"{"} … {"}"}`)
config-menu-name-invalid = invalid menu name `{ $name }`: use "app" or "context:<part>"
config-menu-unexpected = unexpected `{ $node }` in menu (use item, separator or submenu)

## Layout edits from the UI (write-back)

config-edit-makes-invalid = the change would make { $file } invalid: { $error }
config-edit-no-root = the file has no `{ $root }` node
config-edit-size-invalid = invalid size { $size }
config-edit-not-split = the node at { $node } is a `{ $name }`, not a split or stack
config-edit-sizes-count = { $count } sizes for a `{ $name }` with { $children } children
config-edit-columns-invalid = invalid column ids { $columns }
config-edit-not-list = the node at { $node } is a `{ $name }` container, not a list widget
config-edit-no-window = the layout has no window `{ $name }`
config-edit-default-invalid = the default layout does not parse: { $detail }
config-edit-default-empty = the default layout is empty
config-edit-no-breakpoint = window `{ $name }` has no breakpoint { $breakpoint }
config-edit-window-empty = window `{ $name }` is empty
config-edit-no-node = no node at path { $path }
config-edit-plain-values = arguments of toolbar items and key bindings must be plain values

## Merging themes and layouts, backups

config-merge-incoming-invalid = the incoming { $file } has { $count ->
        [one] one error
       *[other] { $count } errors
    }, e.g. { $error }
config-merge-unsupported = { $file } cannot be merged (only theme.kdl and layout.kdl)
config-merge-item-unknown = unknown merge item `{ $item }`
config-merge-not-kdl = the incoming item has values KDL cannot hold
config-merge-default-theme-item = the default theme defines `{ $item }`; a file that extends the defaults cannot remove it, so it stays
config-merge-default-layout-item = the default layout defines `{ $item }`; a file that extends the defaults cannot remove it, so it stays
config-merge-needs-token = uses `{ $token }`, which only the incoming theme defines; take that token too
config-merge-widget-missing = the plug-in widget `{ $widget }` is not installed
config-merge-action-missing = the action `{ $action }` does not exist (a plug-in that is not installed?)
config-merge-drops-plugin-items = taking this drops plug-in items you placed: { $items }
config-merge-key-taken = your layout binds `{ $key }` to `{ $action }` in { $context }
config-backup-missing = no backup { $backup } of { $file }
config-backup-none = no backup of { $file }

## rules.kdl

config-rules-unknown-node = unknown node `{ $node }` (a rules file contains only `rule "<name>" {"{"} … {"}"}` nodes)
config-rules-not-writable = the rules cannot be written as rules.kdl (they change when read back)
config-rules-order-incomplete = the new order must list every rule exactly once
config-rule-text-count = { $count ->
        [0] expected one `rule "<name>" {"{"} … {"}"}` node, found none
       *[other] expected one `rule` node, found { $count }
    }
config-rule-id-duplicate = the rule id "{ $id }" is used twice
config-rule-id-invalid = invalid rule id "{ $id }" (1 to 64 lowercase letters, digits, `-`, `_`, `.`)
config-rule-needs-name-value = a rule needs a name
config-rule-needs-name = `{ $node }` needs a name: rule "<name>" {"{"} … {"}"}
config-rule-one-block = a rule has only one `{ $node }` block
config-rule-trigger-duplicate = this trigger is listed twice
config-rule-needs-trigger-kind = `{ $node }` needs a kind: arrival, on-send, manual, schedule or older-than
config-rule-trigger-unknown = unknown trigger "{ $found }" (expected { $expected })
config-rule-trigger-needs-params = unknown trigger "{ $found }" (expected { $expected }) (`{ $found }` needs parameters: use a child node, e.g. trigger "schedule" every=60)
config-rule-schedule-needs-every = `schedule` needs every=<minutes> (or "2h", "1d")
config-rule-older-than-needs-days = `older-than` needs days=<n>
config-rule-every-invalid = `every` must be minutes (a whole number) or a duration like "30m", "2h", "1d"; found { $found }
config-rule-needs-scope = `{ $node }` needs what it covers: "account:<id>", "virtual:<id>", "folder:<path>" or "role:<role>"
config-rule-scope-invalid = invalid scope { $found } (expected "account:<id>", "virtual:<id>", "folder:<path>" or "role:<role>")
config-rule-scope-account-option = `account=` applies only to folder and role scopes, not to { $kind }
config-rule-scope-kind-unknown = unknown scope kind { $found } (expected account, virtual, folder or role)
config-rule-when-mode = `when` takes `all` or `any`, found { $found }
config-rule-condition-unknown = unknown condition `{ $node }` (expected { $expected })
config-rule-action-unknown = unknown action `{ $node }` (expected { $expected })
config-rule-folder-path-or-role = `{ $node }` takes a folder path or `role=`, not both
config-rule-needs-folder = `{ $node }` needs a folder: a path like "Lists/Rust" or role="archive"
config-rule-one-comparison = `{ $node }` takes only one comparison
config-rule-needs-comparison = `{ $node }` needs a comparison ({ $ops })
config-rule-case-without-comparison = `case-sensitive` needs a comparison to apply to
config-rule-comparison-empty = the comparison text must not be empty
config-rule-needs-result = `{ $node }` needs a result, e.g. "pass" or "fail"
config-rule-needs-header = `{ $node }` needs a header name, e.g. header "List-Id"
config-flag-unknown = unknown flag "{ $found }" (expected { $expected })
config-auth-result-unknown = unknown result "{ $found }" (expected { $expected })
config-rule-header-name-invalid = { $found } is not a header name
config-rule-header-exists-or-comparison = `header` takes either `exists=` or a comparison, not both
config-rule-needs-query = `{ $node }` needs a search query, e.g. query "from:*@example.com"
config-rule-needs-size = `{ $node }` needs larger=<bytes> or smaller=<bytes> (e.g. larger="5MB")
config-rule-needs-date = `{ $node }` needs before="YYYY-MM-DD" or after="YYYY-MM-DD"
config-rule-date-invalid = { $found } is not a date (expected YYYY-MM-DD)
config-rule-needs-age = `{ $node }` needs older-than=<days> or newer-than=<days>
config-rule-needs-flag = `{ $node }` needs a flag: seen, flagged, answered or draft
config-rule-needs-tag = `{ $node }` needs a tag
config-rule-needs-account = `{ $node }` needs an account name
config-rule-needs-address = `{ $node }` needs at least one address
config-rule-needs-template = `{ $node }` needs a template text: reply "Thanks, …"
config-rule-only-one-of = `{ $node }` takes only one of { $options }
config-rule-size-invalid = a size is a number of bytes or a string like "500k" or "5MB"; found { $found }
config-rule-tag-invalid = { $found } cannot be a tag (no spaces or any of ( ) {"{"} % * " \ ])
config-rule-address-invalid = { $found } is not an email address

## Filter rules: problems found by the rules engine (nail-rules)
## `trigger` / `action`: 1-based position; `condition`: a path like 2.1,
## or `root` for the whole condition tree.

rule-issue-name-empty = the rule needs a name
rule-issue-schedule-zero = trigger { $trigger }: the schedule interval must be at least one minute
rule-issue-age-zero = trigger { $trigger }: the age must be at least one day
rule-issue-tag-empty = action { $action }: the tag is empty
rule-issue-no-recipients = action { $action }: there are no recipients
rule-issue-recipient-invalid = action { $action }: a recipient is not an email address
rule-issue-template-empty = action { $action }: the reply template is empty
rule-issue-folder-empty = action { $action }: the folder is empty
rule-issue-regex = { $condition ->
        [root] conditions
       *[other] condition { $condition }
    }: invalid regular expression: { $detail }
rule-issue-header-empty = { $condition ->
        [root] conditions
       *[other] condition { $condition }
    }: the header name is empty
rule-issue-query = { $condition ->
        [root] conditions
       *[other] condition { $condition }
    }: invalid search query: { $detail } (at { $start }–{ $end })
rule-issue-date = { $condition ->
        [root] conditions
       *[other] condition { $condition }
    }: "{ $found }" is not a date (expected YYYY-MM-DD)

## The `nail` command: errors and notices (stderr)

cli-error = Nail stopped with an error: { $detail }
cli-error-cause = Cause: { $detail }
cli-no-default-dir = There is no default directory here; pass { $flag }.
cli-demo-temp-dir = Cannot create a temporary directory for the demo.
cli-demo-real-accounts = { $path } holds real accounts; --seed demo-multi needs an empty --config-dir (or none, for a temporary one).
cli-cannot-write = Cannot write { $path }.
cli-cannot-open-data-dir = Cannot open the data directory { $dir }.
cli-data-dir-in-use = Another Nail process is using this data directory.
cli-data-dir-in-use-by = Another Nail process ({ $owner }, process { $pid }) is using this data directory.
cli-data-dir-lock-failed = Cannot lock the data directory: { $detail }
cli-mcp-no-data-dir = There is no default data directory here; pass --data-dir or --url.
cli-mcp-instance-without-mcp = Nail is running ({ $owner }, process { $pid }) but its MCP server is off. Turn it on in Nail, or stop that process to let `nail mcp` start Nail itself.
cli-mcp-instance-without-mcp-unknown = Nail is running but its MCP server is off. Turn it on in Nail, or stop it to let `nail mcp` start Nail itself.
cli-mcp-headless-started = Nail was not running: started it without a window for this MCP client (data: { $dir }, MCP server: { $url }). It stops when the client disconnects.

## MCP server (nail-mcp): what users read in the audit log, the settings
## and `nail mcp` (agents keep reading English on the MCP wire)

mcp-core-token-missing = connection without a client token
mcp-core-token-refused = connection with an unknown or revoked client token
mcp-core-refused-more = { $count ->
        [one] one more refused connection was not logged
       *[other] { $count } more refused connections were not logged
    }
mcp-core-denied-scope = refused: the permission “{ $scope }” is not granted
mcp-core-denied-account = refused: the account “{ $account }” is not granted
mcp-core-denied-folder = refused: the folder “{ $folder }” is excluded or not in a granted account
mcp-core-denied-all-accounts = refused: this needs access to all accounts
mcp-core-client-name-length = An MCP client name has 1 to { $max } characters.
mcp-core-port-in-use = The MCP server cannot use port { $port }: another program is using it. Choose another port or let Nail pick a free one.
mcp-core-port-denied = The MCP server may not use port { $port }. Choose another port or let Nail pick a free one.
mcp-core-start-failed = The MCP server could not start: { $detail }
mcp-core-bridge-no-token = No MCP client token: set { $variable } to the token Nail showed when the client was created.
mcp-core-bridge-not-running = Nail is not running or its MCP server is off ({ $detail }).
mcp-core-bridge-token-refused = The MCP client token was refused (unknown or revoked); create a new client in Nail.
mcp-core-bridge-wrong-client = The token belongs to the MCP client “{ $actual }”, not “{ $expected }”.
mcp-core-bridge-transport = The connection to Nail failed: { $detail }

## The core (nail-core): synchronization and other diagnostics shown in the UI

# A sync pass finished but some folders failed; `folders` lists their paths.
sync-error-folders =
    { $count ->
        [one] The folder { $folders } could not be synchronized.
       *[other] { $count } folders could not be synchronized: { $folders }
    }

## Attachments of received mail (M5a WP155)

attachment-too-large = This attachment is too large to download here. Save it in the desktop app instead.
attachment-path-not-absolute = Choose a full path for the attachment, including the drive or folder.
