# Nail — English UI strings: "Show source" (ADR 0194).
#
# Message ids start with the prefixes listed in
# ui/src/features/message-source/messages.ts.

window-message-source = Message source
source-action-message = Show source
source-action-keywords = source original raw headers mime eml view copy
source-views = Show
source-view-original = Original
source-view-decoded = Decoded
source-original-hint = The message as it was received: every header and the content as it was sent.
source-decoded-hint = Header fields and the content of each part decoded; parts that are not text are named, not shown.
source-encrypted = The message is end-to-end encrypted: "Original" shows it as received, "Decoded" decrypted.
source-wrap = Wrap lines
source-copy = Copy all
source-copied = Copied to the clipboard.
source-loading = Loading the source…
source-gone = The message of this window is no longer known. Open its source again.
source-truncated = The message has { $size }; only its beginning is shown.
source-text-label = Source of the message
source-omitted = [{ $mime }, { $size }, not shown]
source-omitted-named = [{ $name }, { $mime }, { $size }, not shown]
