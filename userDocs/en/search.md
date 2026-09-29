# Searching

Type in the search bar (or press `/` to jump there) and press Enter. The
message list shows the results, newest first; the line above them says how
many there are. `Esc` or the × button ends the search.

By default Nail searches the selected account — with "All accounts"
selected, every account. The `scope` option of the
[`search-bar` widget](layout-reference.md#widgets) can limit it to the
selected folder or widen it to everything.

## The search language

Words without a field search the subject, the sender, the recipients, the
text and the names of attachments. Several terms must all match. Upper and
lower case do not matter, and accents are ignored (`cafe` finds "café").

| Term | Finds messages… |
| --- | --- |
| `invoice` | containing "invoice" |
| `"quarterly report"` | containing the exact phrase |
| `from:ada` | from a sender whose name or address contains "ada" |
| `to:team@example.org` | to that recipient |
| `cc:grace` | with "grace" in Cc |
| `subject:budget` | with "budget" in the subject |
| `body:invoice` | with "invoice" in the text only |
| `filename:report` | with an attachment whose name contains "report" |
| `has:attachment` | with attachments |
| `is:unread`, `is:read` (or `is:seen`) | unread / read |
| `is:flagged` (or `is:starred`), `is:unflagged` | flagged / not flagged |
| `is:answered` (or `is:replied`), `is:unanswered` | you replied to / not yet |
| `is:draft` | drafts |
| `tag:work` | with the tag `work` |
| `account:work` | in the account "work" (by its name in `accounts.kdl`); a virtual account's name means all its accounts; `account:all` means every account |
| `folder:receipts` | in a folder called "receipts" |
| `before:2026-09-01` | sent before that day (days start at midnight, your time) |
| `after:2026-09-01` | sent on or after that day |
| `after:7d`, `before:2w` | sent within / longer ago than 7 days, 2 weeks (`h` hours, `d` days, `w` weeks, `m` months of 30 days, `y` years) |
| `size>2M`, `size<100k` | larger / smaller than a size (`k`, `M`, `G`; also `>=` and `<=`) |

Values with spaces go in quotes: `subject:"team lunch"`. To search for a
word that looks like a field (`http://…`), quote it: `"http://example.org"`.

Search covers every message Nail has downloaded. Messages still being
downloaded appear as soon as they are stored.

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
