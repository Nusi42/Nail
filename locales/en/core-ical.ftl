# Nail — English texts built by nail-ical (M3a WP71; contract ADR 0058):
# problems found while reading iCalendar and vCard data (import previews,
# subscriptions, synced items).
#
# Owned by WP71; ids start with `ical-` or `vcard-`. Every message may get
# the variables $line (1-based line of the data) and $item (the item's
# title, name or UID); some get more, as noted.

ical-not-icalendar = This is not an iCalendar (.ics) file.
vcard-not-vcard = This is not a vCard (.vcf) file.
ical-malformed-line = A line of the file could not be read and was skipped.
# $component — the kind of block, e.g. VEVENT or VCARD.
ical-unclosed-component = A { $component } block is not closed; it was read up to the end of the file.
ical-missing-start = An event has no start time and was skipped.
# $property — the field, e.g. DTEND or EXDATE.
ical-invalid-time = A date or time ({ $property }) could not be read and was ignored.
# $tzid — the time zone name as written in the file.
ical-unknown-time-zone = The time zone “{ $tzid }” is unknown; its times are shown as local times.
# $tzid — the time zone name as written in the file.
ical-custom-time-zone = The time zone “{ $tzid }” matches no known time zone; its times were converted to UTC.
ical-invalid-recurrence = A repeat rule could not be read; only the first occurrence is shown.
ical-missing-uid = An item had no identifier; Nail gave it one.
ical-orphan-override = A changed occurrence belongs to a series that is not in the file; it is shown as a single event.
ical-duplicate-uid = An item appears twice; the later copy was used.
ical-no-items = The file contains no events or tasks.
ical-invalid-alarm = A reminder could not be read and was skipped.
vcard-missing-name = A contact has no name, email address or organization and was skipped.
vcard-invalid-photo = A contact photo could not be read or is too large; it was left out.
vcard-no-contacts = The file contains no contacts.
