# Nail — English UI strings: the confirmation center for writes by AI agents
# (MCP clients) and plug-ins (M4a WP104; contract ADR 0079).
#
# Owned by WP104; message ids start with the prefixes listed in
# ui/src/features/agents/messages.ts. Texts the core builds (action kinds,
# summaries, states, before / after fields) are in core-agents.ftl.

## Windows and layout labels (@agents.…)

window-confirmation-center = { common-confirmations }
agents-center-tab = { common-confirmations }
agents-action-open-center = Show confirmations

## Confirmation center

agents-center-title = { common-confirmations }
agents-center-intro = AI agents and plug-ins ask here before they delete, send or change something you have not allowed them to do on their own. Nothing happens until you approve.
agents-center-empty = Nothing is waiting for your confirmation.
agents-center-show-pending = Waiting ({ $count })
agents-center-show-history = History
agents-center-pending-label = { confirmation-state-pending }
agents-center-history-label = Decided
agents-history-empty = No decisions yet.
agents-history-more = Show older

## One confirmation

agents-actor-mcp-client = AI agent “{ $name }” (MCP client)
agents-actor-plugin = Plug-in “{ $name }”
agents-on-behalf-of = for the AI agent “{ $client }”
agents-risk-destructive = Deletes
agents-risk-outgoing = Sends mail
agents-operation = Requested through { $operation }
agents-targets = Affected
agents-more-targets =
    { $count ->
        [one] and 1 more
       *[other] and { $count } more
    }
agents-target-message = { common-message }
agents-target-draft = { common-draft }
agents-target-folder = { common-folder }
agents-target-event = { common-event }
agents-target-contact = { common-contact }
agents-target-task = { common-task }
agents-target-rule = { common-rule }
agents-target-recipient = Recipient
agents-changes = What changes
agents-change-before = Now
agents-change-after = After
agents-change-new = (new)
agents-change-removed = (removed)
agents-accounts =
    { $count ->
        [one] Account: { $accounts }
       *[other] Accounts: { $accounts }
    }
agents-requested-at = Asked { $time }
agents-expires-at = Expires { $time }
agents-decided-at = Decided { $time }
agents-via-nail = in Nail
agents-via-elicitation = in the AI agent’s own prompt
agents-via-system = by Nail (timed out or cancelled)
agents-always-allow = From now on, let { $actor } do this without asking: { $kind }
agents-always-asks = Deleting always asks first. This cannot be allowed without asking.
agents-approve = { mcp-tool-elicit-approve }
agents-approve-destructive = Approve and delete
agents-decide-conflict = This was already decided, has expired or was cancelled.

## Notices ("something waits")

agents-notices-label = Waiting for your confirmation
agents-notice-dismiss = Hide this notice

## Indicator

agents-indicator-label =
    { $count ->
        [0] No confirmations waiting
        [one] 1 confirmation waiting
       *[other] { $count } confirmations waiting
    }
agents-indicator-count = { $count }

## Confirmation policy (MCP client settings, plug-in manager)

agents-policy-label = What { $name } may do without asking
agents-policy-title = { common-confirmations }
agents-policy-intro = Choose what { $name } has to ask you about first. Deleting always asks.
agents-policy-ask = Ask first
agents-policy-allow = Without asking
agents-policy-default = (default)
agents-policy-always-asks = Always asks
