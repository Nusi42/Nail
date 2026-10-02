# Texts of the "Attachment check" example in English: the manifest's
# `%id%` references and the texts the code uses (`nail.l10n.t`).

plugin-name = Attachment check
plugin-description = Stops a message that mentions an attachment but has none. You can always send it anyway.
setting-keywords-title = More words that mean an attachment
setting-keywords-description = Separated by commas. End a word with * to match every word that starts with it (report*).
setting-quoted-title = Also check quoted text
setting-quoted-description = Look at the quoted message in replies too (lines starting with “>”).

# The words that mean "there is an attachment", separated by commas, in
# this language. A word ending in * matches every word that starts with it.
attachment-keywords = attach*, enclos*, see the file

# $keyword = the word as it appears in the message.
veto-reason = You mention “{ $keyword }”, but nothing is attached. Attach the file, or send the message anyway.
