# Texts of the "CSV preview" example in English: the manifest's `%id%`
# references and the texts the code shows (`nail.l10n.t`).

plugin-name = CSV preview
plugin-description = Shows CSV and TSV attachments as a table.
preview-title = Table
setting-header-title = First row is a header
setting-header-description = Use the first row as the column names.
setting-rows-title = Rows to show
setting-rows-description = The preview shows at most this many rows.

summary = { $rows ->
    [one] One row
   *[other] { $rows } rows
}, { $columns ->
    [one] one column
   *[other] { $columns } columns
}
truncated = The first { $shown } rows are shown.
column = Column { $number }
empty = The file is empty.
