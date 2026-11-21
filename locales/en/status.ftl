# Nail — status bar texts of the queue of message writes (ADR 0230).
#
# The status bar's older texts stay in app.ftl (status-*); new ones for the
# queue summary go here.

# Several queued changes failed; $reason is the newest failure's reason,
# $count how many failed in all.
status-ops-failed-many = { $count } changes could not be saved: { $reason }
