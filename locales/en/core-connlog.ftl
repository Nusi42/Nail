# Nail — English texts of the connection log and error details named by the
# contract (ADR 0190): protocols, directions, levels, error steps, and the
# texts the core builds for skipped messages.
#
# The ids named by nail_api::connlog are pre-filled; nail-api/tests/catalog.rs
# checks them. The UI's own texts (viewer, details window, settings) are in
# connlog.ftl.

## Protocols (nail_api::connlog::WireProtocol)

connlog-protocol-imap = IMAP
connlog-protocol-smtp = SMTP
connlog-protocol-pop3 = POP3
connlog-protocol-managesieve = ManageSieve
connlog-protocol-caldav = CalDAV
connlog-protocol-carddav = CardDAV
connlog-protocol-feed = Calendar subscription
connlog-protocol-gmail = Gmail API
connlog-protocol-graph = Microsoft Graph
connlog-protocol-ews = Exchange Web Services
connlog-protocol-oauth = Sign-in tokens
connlog-protocol-discovery = Server discovery

## Directions (nail_api::connlog::WireDirection)

connlog-direction-sent = Sent
connlog-direction-received = Received
connlog-direction-info = Note

## Levels (nail_api::connlog::WireLevel)

connlog-level-info = Normal
connlog-level-warning = Warning
connlog-level-error = Error

## What Nail was doing (nail_api::connlog::ErrorStep)

error-step-connect = Connecting to the server
error-step-tls = Securing the connection
error-step-login = Signing in
error-step-token = Getting a sign-in token
error-step-list = Listing folders
error-step-select = Opening the folder
error-step-search = Looking for new messages
error-step-fetch = Downloading messages
error-step-flags = Checking flags and removed messages
error-step-store = Saving messages on this computer
error-step-write = Writing a change to the server
error-step-send = Sending the message
error-step-idle = Waiting for new mail
error-step-discover = Finding the account's servers
error-step-other = Synchronizing

## Built by the core (sync.rs, ADR 0191)

# A folder synced, but some of its messages could not be downloaded or read;
# they are retried by the next synchronizations.
sync-skipped-messages =
    { $count ->
        [one] One message in { $folder } could not be downloaded; Nail will try again.
       *[other] { $count } messages in { $folder } could not be downloaded; Nail will try again.
    }
