# Texts of the manifest (`%id%` references) and of the code (`nail.l10n.t`) in English.
plugin-description = A starter attachment plug-in: previews text files and counts their words.
preview-title = Text
count-words-title = Count words
preview-summary = { $lines ->
    [one] One line
   *[other] { $lines } lines
}, { $words ->
    [one] one word
   *[other] { $words } words
}
column-line = Line
column-text = Text
preview-more = The first { $shown } lines are shown.
words-counted = “{ $name }” has { $words ->
    [one] one word
   *[other] { $words } words
}.
