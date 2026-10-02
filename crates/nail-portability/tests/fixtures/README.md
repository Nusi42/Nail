# nail-portability test fixtures

## `thunderbird/` — a synthetic Thunderbird data folder

Not demo mail and not a real profile: every account, address and
"password" is invented (`example.org`, `example.net`).

```
thunderbird/
  profiles.ini, installs.ini            one profile, the install default
  Profiles/x7k2m9qa.default-release/
    prefs.js                            IMAP "Work", POP3 "ada@example.net",
                                        a feed account (not importable),
                                        Local Folders, SMTP servers,
                                        address book list (incl. old.mab)
    logins.json                         encrypted logins: must never be read
    Mail/Local Folders/                 Inbox (3 messages: read + flagged +
                                        keyword, deleted-not-compacted,
                                        unread), Archives + Archives.sbd/2024,
                                        empty Trash / Unsent Messages, .msf
                                        summaries, filter rules, a filter log
    Mail/pop.example.net/Inbox          POP3 mail
    ImapMail/imap.example.org/          filter rules (complete, partly
                                        expressible, disabled), an .msf
    abook.sqlite, history.sqlite        address books (binary, generated)
    old.mab                             a Mork address book (before TB 78)
```

The text files are written by hand. The two SQLite files are generated
from the card lists in `tests/thunderbird_profile.rs`:

```
NAIL_REGENERATE_FIXTURES=1 cargo test -p nail-portability --test thunderbird_profile
```

Every normal test run checks that the committed files hold exactly those
cards (`the_fixture_address_books_are_current`).

WP125's Playwright scenario "import a Thunderbird profile fixture" uses
this profile.
