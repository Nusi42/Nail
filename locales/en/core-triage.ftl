# Nail — English texts of notifications built in Rust (M2c WP52; contract
# ADR 0043). The core pushes them as LocalizedMessage in
# NotificationRequested; the desktop shell and the remote UI render them.
#
# Owned by WP52; ids start with `notification-`.

notification-test-title = Notifications work
notification-test-body = This is how Nail tells you about new mail.
notification-new-mail-title = { $count ->
        [one] New message in { $account }
       *[other] { $count } new messages in { $account }
    }
notification-new-mail-body = { $sender }: { $subject }
notification-snooze-woke-title = { $count ->
        [one] A snoozed message is back
       *[other] { $count } snoozed messages are back
    }

# Snooze requests the scheduler refuses (SnoozeError::message_id).
snooze-error-not-in-future = The snooze time has to be in the future.
snooze-error-too-far = The snooze time is too far away (at most ten years).
snooze-error-store = The snooze could not be saved.
