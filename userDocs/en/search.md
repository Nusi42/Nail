# Searching

Type in the search bar (or press `/` to jump there) and press Enter. The
message list shows the results, newest first; the line above them says how
many there are. `Esc` or the × button ends the search.

By default Nail searches the selected account — with "All accounts"
selected, every account. The `scope` option of the
[`search-bar` widget](layout-reference.md#widgets) can limit it to the
selected folder or widen it to everything.

## The search language

Words without a field search the subject, the sender, the recipients and the
text. Several terms must all match.

| Term | Finds messages… |
| --- | --- |
| `invoice` | containing "invoice" |
| `"quarterly report"` | containing the exact phrase |
| `from:ada` | from a sender whose name or address contains "ada" |
| `to:team@example.org` | to that recipient |
| `subject:budget` | with "budget" in the subject |
| `has:attachment` | with attachments |
| `is:unread`, `is:read` | unread / read |
| `is:flagged`, `is:unflagged` | flagged / not flagged |
| `is:answered` | you replied to |
| `tag:work` | with the tag `work` |
| `account:work` | in the account "work" |
| `folder:receipts` | in a folder called "receipts" |
| `before:2026-09-01` | sent before that day |
| `after:2026-09-01` | sent on or after that day |
| `size>2M`, `size<100k` | larger / smaller than a size (`k`, `M`, `G`) |

Values with spaces go in quotes: `subject:"team lunch"`.

### Combining terms

| Query | Meaning |
| --- | --- |
| `from:ada invoice` | both (the same as `from:ada AND invoice`) |
| `from:ada OR from:grace` | either |
| `NOT is:read`, `-is:read` | not |
| `(from:ada OR from:grace) has:attachment` | parentheses group terms |

`AND`, `OR` and `NOT` are written in capitals.

## Hints and mistakes

While the search bar has focus, it lists the fields you can use; the list
narrows as you type (`is` shows the `is:` choices). Click a hint to insert
it.

If a query has a mistake, the search bar says what is wrong, at which
character, and underlines the spot:

| Message | Example |
| --- | --- |
| Unknown search field. | `colour:red` |
| This value does not fit the search field. | `is:purple`, `before:yesterday` |
| A quotation mark is not closed. | `subject:"lunch` |
| The parentheses do not match. | `(from:ada` |
| The search ends too early. | `from:ada AND` |
| This word cannot appear here. | `OR from:ada` |
