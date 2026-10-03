# Nail — English texts of the settings page (M5b WP163): grouped
# navigation, settings search and section headers. Group and section labels
# of the default layout are in core-shell.ftl.

settings-section-action = Show a settings section
settings-search-action = Search settings
settings-keywords-search = find preferences options configure

## The page

settings-page-nav = Settings sections
settings-page-empty = This layout's settings page has no sections.
settings-page-section-missing = This section is not on the settings page.

## Search

settings-search-label = Search settings
settings-search-placeholder = Search settings
settings-search-clear = Clear the search
settings-search-results = Search results
settings-search-count =
    { $count ->
        [0] No settings match “{ $query }”.
        [one] One section matches “{ $query }”.
       *[other] { $count } sections match “{ $query }”.
    }

## Extra search words per settings widget (space separated; users search
## with their own words, not only with the labels on the page)

settings-keywords-account-settings = account email mail address server imap pop3 smtp exchange gmail password identity signature sender reply-to login username
settings-keywords-virtual-account-editor = virtual merge combine unified accounts folders identity
settings-keywords-toolbar = toolbar buttons customize
settings-keywords-sync-folder-policies = sync synchronize offline download headers bodies idle push folders storage
settings-keywords-junk-settings = junk spam filter classifier bayes threshold training
settings-keywords-notification-settings = notifications alerts sound popup toast quiet hours do not disturb new mail
settings-keywords-sieve-status = sieve managesieve server filters rules script
settings-keywords-vacation-editor = vacation out of office away holiday automatic reply autoresponder
settings-keywords-exchange-rules = exchange outlook microsoft 365 inbox rules server
settings-keywords-import-export = import export mbox eml maildir thunderbird ics vcard migrate move
settings-keywords-diagnostics = diagnostics support logs bundle troubleshooting problem bug
settings-keywords-update-settings = updates upgrade version release channel download install
settings-keywords-desktop-settings = tray notification area autostart startup windows background minimize close
settings-keywords-default-apps = default mail app mailto links eml files mapi send to
settings-keywords-appearance-settings = theme dark light mode colours colors density compact comfortable font text size corners radius accent high contrast follow windows
settings-keywords-key-binding-editor = keyboard shortcuts hotkeys keys bindings chords
settings-keywords-security-keys = pgp openpgp gpg gnupg s/mime smime certificates keys signature sign encrypt encryption
settings-keywords-security-settings = encryption at rest vault database search index decrypted privacy lookup wkd keyserver
settings-keywords-security-smartcards = smartcard yubikey token pkcs11 pin hardware
settings-keywords-remote-settings = remote web browser phone tablet server port network certificate tls https internet
settings-keywords-remote-users = users login password passkey two-factor 2fa totp authenticator
settings-keywords-remote-content-allows = remote content images tracking pixels privacy senders allow
settings-keywords-mcp-clients = ai agents mcp claude assistant clients tokens access permissions rate limits
settings-keywords-confirmation-center = confirmations approvals agents plug-ins pending history
settings-keywords-plugin-manager = plug-ins plugins extensions add-ons install permissions
