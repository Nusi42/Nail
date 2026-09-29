# Nail — English UI strings: per-folder sync policies and bodies that are
# downloaded on demand (M2c WP54; contract ADR 0043).
#
# Owned by WP54; message ids start with the prefixes listed in
# ui/src/features/sync/messages.ts.

## Body policy (nail_api::accounts::BodyPolicy)

sync-bodies-full = Full messages
sync-bodies-headers-only = Headers only (bodies on demand)
sync-bodies-recent = Full messages of the last { $days ->
        [one] day
       *[other] { $days } days
    }
