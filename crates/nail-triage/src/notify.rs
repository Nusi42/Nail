//! The new-mail notification policy.
//!
//! [`decide`] answers "does this arriving message notify?" with the
//! precedence of ADR 0043:
//!
//! 1. notifications off ([`NotificationSettings::enabled`]) → no;
//! 2. quiet hours ([`in_quiet_hours`], local time of the given zone,
//!    across midnight, per weekday) → no (dropped, not deferred);
//! 3. a rule's `notify` action (`notify #true` / `notify #false`) → its
//!    choice;
//! 4. mail filed as junk → no;
//! 5. a folder override → its choice; then an account override;
//! 6. [`NotificationSettings::folders`]: the Inbox only, or every folder
//!    except Sent, Drafts, Trash and Junk.
//!
//! [`NotificationBatcher`] then turns a burst of notifying messages into
//! one [`MailNotification`] per account and folder: messages that arrive
//! within [`NotificationBatcher::quiet_period`] of each other wait
//! together, and nothing waits longer than
//! [`NotificationBatcher::max_delay`]. The texts are
//! [`LocalizedMessage`]s from `core-triage.ftl`.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike};
use nail_api::triage::{
    MailNotification, NotificationKind, NotificationScope, NotificationSettings, NotifyFolders,
    QuietHours, Weekday, parse_hhmm,
};
use nail_api::{AccountId, FolderId, FolderRole, LocalizedMessage, MessageId};

use crate::localtime::weekday;

/// A message that arrived, as the policy sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    /// The message.
    pub message_id: MessageId,
    /// Its account.
    pub account_id: AccountId,
    /// The account's display name (argument of the title).
    pub account_name: String,
    /// Its folder.
    pub folder_id: FolderId,
    /// The folder's role.
    pub folder_role: FolderRole,
    /// Sender as shown (name, else address).
    pub sender: String,
    /// Subject.
    pub subject: String,
    /// The rules' `notify` decision for it (last `notify` action that
    /// ran), `None` = no rule said anything.
    pub rule_notify: Option<bool>,
    /// Filed as junk (by the server or the classifier).
    pub junk: bool,
}

/// Why a message does or does not notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Notifications are off.
    Disabled,
    /// Quiet hours.
    QuietHours,
    /// A rule's `notify` action.
    Rule,
    /// Junk mail.
    Junk,
    /// A folder override.
    FolderOverride,
    /// An account override.
    AccountOverride,
    /// The default folder selection.
    Folders,
}

/// The policy's answer for one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Notify or not.
    pub notify: bool,
    /// The step that decided.
    pub reason: Reason,
}

impl Decision {
    fn new(notify: bool, reason: Reason) -> Self {
        Self { notify, reason }
    }
}

/// Whether a local time lies in the quiet hours. A range whose end is
/// before its start crosses midnight and belongs to the day it starts on
/// (`22:00`–`07:00` on Fridays covers Saturday 03:00, not Friday 03:00).
/// Malformed or empty ranges are never quiet.
pub fn in_quiet_hours<Tz: TimeZone>(quiet: &QuietHours, local: &DateTime<Tz>) -> bool {
    let (Some(start), Some(end)) = (parse_hhmm(&quiet.start), parse_hhmm(&quiet.end)) else {
        return false;
    };
    let minute = local.hour() * 60 + local.minute();
    let today = weekday(local.weekday());
    let yesterday = weekday(local.weekday().pred());
    let on = |day: Weekday| quiet.days.is_empty() || quiet.days.contains(&day);
    if start < end {
        (start..end).contains(&minute) && on(today)
    } else if start > end {
        (minute >= start && on(today)) || (minute < end && on(yesterday))
    } else {
        false
    }
}

/// Decides whether an arriving message notifies at `local` (the current
/// time in the user's zone).
pub fn decide<Tz: TimeZone>(
    settings: &NotificationSettings,
    arrival: &Arrival,
    local: &DateTime<Tz>,
) -> Decision {
    if !settings.enabled {
        return Decision::new(false, Reason::Disabled);
    }
    if settings
        .quiet_hours
        .as_ref()
        .is_some_and(|q| in_quiet_hours(q, local))
    {
        return Decision::new(false, Reason::QuietHours);
    }
    if let Some(notify) = arrival.rule_notify {
        return Decision::new(notify, Reason::Rule);
    }
    if arrival.junk {
        return Decision::new(false, Reason::Junk);
    }
    let override_for = |wanted: &dyn Fn(&NotificationScope) -> bool| {
        settings
            .overrides
            .iter()
            .find(|o| wanted(&o.scope))
            .map(|o| o.notify)
    };
    if let Some(notify) = override_for(
        &|s| matches!(s, NotificationScope::Folder { folder_id } if *folder_id == arrival.folder_id),
    ) {
        return Decision::new(notify, Reason::FolderOverride);
    }
    if let Some(notify) = override_for(
        &|s| matches!(s, NotificationScope::Account { account_id } if *account_id == arrival.account_id),
    ) {
        return Decision::new(notify, Reason::AccountOverride);
    }
    let notify = match settings.folders {
        NotifyFolders::Inbox => arrival.folder_role == FolderRole::Inbox,
        NotifyFolders::All => !matches!(
            arrival.folder_role,
            FolderRole::Sent | FolderRole::Drafts | FolderRole::Trash | FolderRole::Junk
        ),
    };
    Decision::new(notify, Reason::Folders)
}

/// The arrivals that notify, in order.
pub fn notifying<'a, Tz: TimeZone>(
    settings: &NotificationSettings,
    arrivals: &'a [Arrival],
    local: &DateTime<Tz>,
) -> Vec<&'a Arrival> {
    arrivals
        .iter()
        .filter(|a| decide(settings, a, local).notify)
        .collect()
}

/// The tag of new-mail notifications of a folder (a later notification
/// with the same tag replaces the earlier one on screen).
pub fn new_mail_tag(account: &AccountId, folder: &FolderId) -> String {
    format!("new-mail:{account}:{folder}")
}

/// The title and body of a group of messages (newest last in `messages`).
fn texts(
    title_id: &str,
    account_name: Option<&str>,
    newest: Option<(&str, &str)>,
    count: usize,
    preview: bool,
) -> (LocalizedMessage, Option<LocalizedMessage>) {
    let mut title = LocalizedMessage::new(title_id).arg("count", count);
    if let Some(name) = account_name {
        title = title.arg("account", name);
    }
    let body = preview.then_some(()).and(newest).map(|(sender, subject)| {
        LocalizedMessage::new("notification-new-mail-body")
            .arg("sender", sender)
            .arg("subject", subject)
    });
    (title, body)
}

#[derive(Debug, Clone)]
struct Pending {
    account_id: AccountId,
    account_name: String,
    folder_id: FolderId,
    /// Oldest first.
    messages: Vec<(MessageId, String, String)>,
    first_at: i64,
    last_at: i64,
    preview: bool,
    sound: bool,
}

/// Collects notifying arrivals and releases one notification per account
/// and folder once a burst is over.
#[derive(Debug, Clone)]
pub struct NotificationBatcher {
    /// A group is released this long after its last message.
    pub quiet_period: Duration,
    /// …and at the latest this long after its first.
    pub max_delay: Duration,
    pending: BTreeMap<String, Pending>,
    sequence: u64,
}

impl Default for NotificationBatcher {
    fn default() -> Self {
        Self::new(Duration::seconds(3), Duration::seconds(15))
    }
}

impl NotificationBatcher {
    /// A batcher with the given timing.
    pub fn new(quiet_period: Duration, max_delay: Duration) -> Self {
        Self {
            quiet_period,
            max_delay,
            pending: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// Adds messages that notify (after [`decide`]) at `now` (Unix
    /// seconds); `settings` decide preview and sound.
    pub fn push(&mut self, settings: &NotificationSettings, arrivals: &[&Arrival], now: i64) {
        for a in arrivals {
            let tag = new_mail_tag(&a.account_id, &a.folder_id);
            let entry = self.pending.entry(tag).or_insert_with(|| Pending {
                account_id: a.account_id.clone(),
                account_name: a.account_name.clone(),
                folder_id: a.folder_id.clone(),
                messages: Vec::new(),
                first_at: now,
                last_at: now,
                preview: settings.show_preview,
                sound: settings.sound,
            });
            if !entry.messages.iter().any(|(id, ..)| *id == a.message_id) {
                entry
                    .messages
                    .push((a.message_id.clone(), a.sender.clone(), a.subject.clone()));
            }
            entry.last_at = entry.last_at.max(now);
            entry.preview = settings.show_preview;
            entry.sound = settings.sound;
        }
    }

    fn due_at(&self, p: &Pending) -> i64 {
        (p.last_at + self.quiet_period.num_seconds()).min(p.first_at + self.max_delay.num_seconds())
    }

    /// When the next group is due (for the core's timer).
    pub fn next_due(&self) -> Option<i64> {
        self.pending.values().map(|p| self.due_at(p)).min()
    }

    /// Whether nothing waits.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Releases the groups due at `now`.
    pub fn flush(&mut self, now: i64) -> Vec<MailNotification> {
        let due: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, p)| self.due_at(p) <= now)
            .map(|(tag, _)| tag.clone())
            .collect();
        due.into_iter()
            .filter_map(|tag| {
                let p = self.pending.remove(&tag)?;
                Some(self.build(tag, p))
            })
            .collect()
    }

    /// Releases everything now (shutdown, or settings turned off: then
    /// the caller drops the result).
    pub fn flush_all(&mut self) -> Vec<MailNotification> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(tag, p)| self.build(tag, p))
            .collect()
    }

    fn build(&mut self, tag: String, p: Pending) -> MailNotification {
        self.sequence += 1;
        let newest = p
            .messages
            .last()
            .map(|(_, s, subj)| (s.as_str(), subj.as_str()));
        let (title, body) = texts(
            "notification-new-mail-title",
            Some(&p.account_name),
            newest,
            p.messages.len(),
            p.preview,
        );
        MailNotification {
            id: format!("{tag}:{}", self.sequence),
            tag,
            kind: NotificationKind::NewMail,
            account_id: Some(p.account_id),
            folder_id: Some(p.folder_id),
            message_ids: p.messages.into_iter().rev().map(|(id, ..)| id).collect(),
            title,
            body,
            sound: p.sound,
        }
    }
}

/// A snoozed message that came back (for [`snooze_woke`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WokeMessage {
    /// The message.
    pub message_id: MessageId,
    /// Sender as shown.
    pub sender: String,
    /// Subject.
    pub subject: String,
}

/// The notification for snoozed messages of one account and folder that
/// came back (oldest first in `messages`), `None` when notifications are
/// off or it is quiet time. Snooze wake-ups ignore the folder selection:
/// the user asked for them. The caller makes one per account and folder,
/// so a notification never names mail of another account or folder (a
/// remote session sees only the ones its grant covers).
pub fn snooze_woke<Tz: TimeZone>(
    settings: &NotificationSettings,
    account: Option<&AccountId>,
    folder: Option<&FolderId>,
    messages: &[WokeMessage],
    local: &DateTime<Tz>,
    sequence: u64,
) -> Option<MailNotification> {
    if messages.is_empty()
        || !settings.enabled
        || settings
            .quiet_hours
            .as_ref()
            .is_some_and(|q| in_quiet_hours(q, local))
    {
        return None;
    }
    let newest = messages
        .last()
        .map(|m| (m.sender.as_str(), m.subject.as_str()));
    let (title, body) = texts(
        "notification-snooze-woke-title",
        None,
        newest,
        messages.len(),
        settings.show_preview,
    );
    Some(MailNotification {
        id: format!("snooze-woke:{sequence}"),
        tag: match folder {
            Some(folder) => format!("snooze-woke:{folder}"),
            None => "snooze-woke".to_owned(),
        },
        kind: NotificationKind::SnoozeWoke,
        account_id: account.cloned(),
        folder_id: folder.cloned(),
        message_ids: messages
            .iter()
            .rev()
            .map(|m| m.message_id.clone())
            .collect(),
        title,
        body,
        sound: settings.sound,
    })
}

/// The notification of `TestNotification` (ignores quiet hours and the
/// switch: the user asked for it).
pub fn test_notification(settings: &NotificationSettings, sequence: u64) -> MailNotification {
    MailNotification {
        id: format!("test:{sequence}"),
        tag: "test".to_owned(),
        kind: NotificationKind::Test,
        account_id: None,
        folder_id: None,
        message_ids: Vec::new(),
        title: LocalizedMessage::new("notification-test-title"),
        body: Some(LocalizedMessage::new("notification-test-body")),
        sound: settings.sound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localtime::resolve_local;
    use crate::localtime::testzone::{BERLIN, NEW_YORK};
    use chrono::{FixedOffset, NaiveDate, NaiveDateTime, Utc};
    use nail_api::triage::NotificationOverride;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    fn quiet(start: &str, end: &str, days: &[Weekday]) -> QuietHours {
        QuietHours {
            start: start.into(),
            end: end.into(),
            days: days.to_vec(),
        }
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.from_utc_datetime(&dt(y, mo, d, h, mi))
    }

    fn arrival(id: &str, folder: &str, role: FolderRole) -> Arrival {
        Arrival {
            message_id: id.into(),
            account_id: "work".into(),
            account_name: "Work".into(),
            folder_id: folder.into(),
            folder_role: role,
            sender: "Ada".into(),
            subject: format!("Hello {id}"),
            rule_notify: None,
            junk: false,
        }
    }

    // 2026-10-02 is a Friday.
    #[test]
    fn quiet_hours_within_a_day() {
        let q = quiet("12:00", "13:30", &[]);
        assert!(!in_quiet_hours(&q, &utc(2026, 10, 2, 11, 59)));
        assert!(in_quiet_hours(&q, &utc(2026, 10, 2, 12, 0)));
        assert!(in_quiet_hours(&q, &utc(2026, 10, 2, 13, 29)));
        assert!(
            !in_quiet_hours(&q, &utc(2026, 10, 2, 13, 30)),
            "end is exclusive"
        );
        let weekdays = quiet("12:00", "13:30", &[Weekday::Mon]);
        assert!(!in_quiet_hours(&weekdays, &utc(2026, 10, 2, 12, 30)));
        assert!(in_quiet_hours(&weekdays, &utc(2026, 10, 5, 12, 30)));
    }

    #[test]
    fn quiet_hours_across_midnight_belong_to_the_start_day() {
        let q = quiet("22:00", "07:00", &[Weekday::Fri]);
        assert!(!in_quiet_hours(&q, &utc(2026, 10, 2, 21, 59)));
        assert!(in_quiet_hours(&q, &utc(2026, 10, 2, 22, 0)));
        assert!(in_quiet_hours(&q, &utc(2026, 10, 2, 23, 59)));
        // Saturday early morning: Friday's range.
        assert!(in_quiet_hours(&q, &utc(2026, 10, 3, 0, 0)));
        assert!(in_quiet_hours(&q, &utc(2026, 10, 3, 6, 59)));
        assert!(!in_quiet_hours(&q, &utc(2026, 10, 3, 7, 0)));
        // Friday early morning belongs to Thursday: not quiet.
        assert!(!in_quiet_hours(&q, &utc(2026, 10, 2, 3, 0)));
        // Saturday night: not a Friday.
        assert!(!in_quiet_hours(&q, &utc(2026, 10, 3, 23, 0)));
        // Every day.
        let every = quiet("22:00", "07:00", &[]);
        assert!(in_quiet_hours(&every, &utc(2026, 10, 2, 3, 0)));
        // Malformed and empty ranges are never quiet.
        assert!(!in_quiet_hours(
            &quiet("25:00", "07:00", &[]),
            &utc(2026, 10, 2, 3, 0)
        ));
        assert!(!in_quiet_hours(
            &quiet("07:00", "07:00", &[]),
            &utc(2026, 10, 2, 7, 0)
        ));
    }

    #[test]
    fn quiet_hours_use_the_given_time_zone() {
        let q = quiet("22:00", "07:00", &[]);
        let instant = utc(2026, 10, 2, 23, 0);
        assert!(in_quiet_hours(&q, &instant));
        // 23:00 UTC is 08:00 the next day in Tokyo, 19:00 in New York.
        let tokyo = FixedOffset::east_opt(9 * 3600).unwrap();
        assert!(!in_quiet_hours(&q, &instant.with_timezone(&tokyo)));
        assert!(!in_quiet_hours(&q, &instant.with_timezone(&NEW_YORK)));
        // Weekday in the local zone: Friday 23:00 UTC is Saturday in Tokyo.
        let sat = quiet("08:00", "09:00", &[Weekday::Sat]);
        assert!(in_quiet_hours(&sat, &instant.with_timezone(&tokyo)));
    }

    #[test]
    fn quiet_hours_across_dst_changes() {
        let night = quiet("22:00", "07:00", &[Weekday::Sat]);
        // Spring forward in Berlin (Sun 2026-03-29, 02:00 → 03:00): the
        // night is an hour shorter but ends at 07:00 local.
        let sat_night = |h, m| utc(2026, 3, 28, h, m).with_timezone(&BERLIN);
        assert!(in_quiet_hours(&night, &sat_night(21, 0))); // 22:00 CET
        let sun = |h, m| utc(2026, 3, 29, h, m).with_timezone(&BERLIN);
        assert!(in_quiet_hours(&night, &sun(0, 59))); // 01:59 CET
        assert!(in_quiet_hours(&night, &sun(1, 0))); // 03:00 CEST
        assert!(in_quiet_hours(&night, &sun(4, 59))); // 06:59 CEST
        assert!(!in_quiet_hours(&night, &sun(5, 0))); // 07:00 CEST
        // Quiet hours inside the gap never happen that night.
        let gap = quiet("02:00", "03:00", &[]);
        for m in 0..120 {
            let t = utc(2026, 3, 29, 0, 0) + Duration::minutes(m);
            assert!(!in_quiet_hours(&gap, &t.with_timezone(&BERLIN)), "{t}");
        }
        // Fall back (Sun 2026-10-25, 03:00 CEST → 02:00 CET): 02:30 exists
        // twice and is quiet both times.
        let early = quiet("02:00", "03:00", &[]);
        let first = resolve_local(&BERLIN, dt(2026, 10, 25, 2, 30));
        let second = first + Duration::hours(1);
        assert_eq!(first.naive_local(), second.naive_local());
        assert!(in_quiet_hours(&early, &first));
        assert!(in_quiet_hours(&early, &second));
        // New York, fall back: 01:30 twice.
        let ny = quiet("01:00", "02:00", &[Weekday::Sun]);
        let first = resolve_local(&NEW_YORK, dt(2026, 11, 1, 1, 30));
        assert!(in_quiet_hours(&ny, &first));
        assert!(in_quiet_hours(&ny, &(first + Duration::hours(1))));
    }

    #[test]
    fn precedence_of_the_policy() {
        let now = utc(2026, 10, 2, 12, 0);
        let mut s = NotificationSettings::default();
        let inbox = arrival("m1", "inbox", FolderRole::Inbox);
        let other = arrival("m2", "lists", FolderRole::Other);
        assert_eq!(
            decide(&s, &inbox, &now),
            Decision::new(true, Reason::Folders)
        );
        assert_eq!(
            decide(&s, &other, &now),
            Decision::new(false, Reason::Folders)
        );
        // A rule wins over the folder selection and overrides.
        let ruled = Arrival {
            rule_notify: Some(true),
            ..other.clone()
        };
        s.overrides = vec![NotificationOverride {
            scope: NotificationScope::Folder {
                folder_id: "lists".into(),
            },
            notify: false,
        }];
        assert_eq!(decide(&s, &ruled, &now), Decision::new(true, Reason::Rule));
        let suppressed = Arrival {
            rule_notify: Some(false),
            ..inbox.clone()
        };
        assert_eq!(
            decide(&s, &suppressed, &now),
            Decision::new(false, Reason::Rule)
        );
        // Folder override beats account override.
        s.overrides.push(NotificationOverride {
            scope: NotificationScope::Account {
                account_id: "work".into(),
            },
            notify: true,
        });
        assert_eq!(
            decide(&s, &other, &now),
            Decision::new(false, Reason::FolderOverride)
        );
        let archive = arrival("m3", "archive", FolderRole::Archive);
        assert_eq!(
            decide(&s, &archive, &now),
            Decision::new(true, Reason::AccountOverride)
        );
        // Junk never notifies unless a rule says so.
        let junk = Arrival {
            junk: true,
            ..inbox.clone()
        };
        assert_eq!(decide(&s, &junk, &now), Decision::new(false, Reason::Junk));
        let junk_ruled = Arrival {
            rule_notify: Some(true),
            ..junk
        };
        assert!(decide(&s, &junk_ruled, &now).notify);
        // Quiet hours beat rules.
        s.quiet_hours = Some(quiet("11:00", "13:00", &[]));
        assert_eq!(
            decide(&s, &ruled, &now),
            Decision::new(false, Reason::QuietHours)
        );
        // Off beats everything.
        s.enabled = false;
        assert_eq!(
            decide(&s, &ruled, &now),
            Decision::new(false, Reason::Disabled)
        );
    }

    #[test]
    fn all_folders_skip_sent_drafts_trash_junk() {
        let now = utc(2026, 10, 2, 12, 0);
        let s = NotificationSettings {
            folders: NotifyFolders::All,
            ..NotificationSettings::default()
        };
        let got: Vec<bool> = [
            FolderRole::Inbox,
            FolderRole::Other,
            FolderRole::Archive,
            FolderRole::Sent,
            FolderRole::Drafts,
            FolderRole::Trash,
            FolderRole::Junk,
        ]
        .into_iter()
        .map(|role| decide(&s, &arrival("m", "f", role), &now).notify)
        .collect();
        assert_eq!(got, [true, true, true, false, false, false, false]);
        let arrivals = [
            arrival("a", "inbox", FolderRole::Inbox),
            arrival("b", "sent", FolderRole::Sent),
        ];
        let n = notifying(&s, &arrivals, &now);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].message_id.as_str(), "a");
    }

    #[test]
    fn bursts_become_one_notification_per_folder() {
        let s = NotificationSettings::default();
        let mut b = NotificationBatcher::default();
        let a1 = arrival("m1", "inbox", FolderRole::Inbox);
        let a2 = arrival("m2", "inbox", FolderRole::Inbox);
        let a3 = arrival("m3", "lists", FolderRole::Other);
        b.push(&s, &[&a1], 100);
        assert_eq!(b.next_due(), Some(103));
        assert!(b.flush(102).is_empty());
        b.push(&s, &[&a2, &a3, &a1], 102);
        assert_eq!(b.next_due(), Some(105));
        let out = b.flush(105);
        assert_eq!(out.len(), 2);
        let inbox = out
            .iter()
            .find(|n| n.folder_id == Some("inbox".into()))
            .unwrap();
        assert_eq!(
            inbox.message_ids,
            [MessageId::from("m2"), MessageId::from("m1")]
        );
        assert_eq!(inbox.tag, "new-mail:work:inbox");
        assert_eq!(inbox.kind, NotificationKind::NewMail);
        assert_eq!(
            inbox.title,
            LocalizedMessage::new("notification-new-mail-title")
                .arg("count", 2u32)
                .arg("account", "Work")
        );
        assert_eq!(
            inbox.body,
            Some(
                LocalizedMessage::new("notification-new-mail-body")
                    .arg("sender", "Ada")
                    .arg("subject", "Hello m2")
            )
        );
        assert!(inbox.sound);
        assert_ne!(out[0].id, out[1].id);
        assert!(b.is_empty());
        assert_eq!(b.next_due(), None);
    }

    #[test]
    fn a_steady_stream_is_released_after_max_delay() {
        let s = NotificationSettings {
            show_preview: false,
            sound: false,
            ..NotificationSettings::default()
        };
        let mut b = NotificationBatcher::new(Duration::seconds(3), Duration::seconds(10));
        let mut released = Vec::new();
        for t in 0..12 {
            let a = arrival(&format!("m{t}"), "inbox", FolderRole::Inbox);
            b.push(&s, &[&a], 100 + t);
            released.extend(b.flush(100 + t));
        }
        assert_eq!(released.len(), 1, "released at 110 despite the stream");
        assert_eq!(released[0].message_ids.len(), 11);
        assert_eq!(released[0].body, None, "no preview");
        assert!(!released[0].sound);
        let rest = b.flush_all();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].message_ids.len(), 1);
    }

    #[test]
    fn snooze_woke_and_test_notifications() {
        let now = utc(2026, 10, 2, 12, 0);
        let mut s = NotificationSettings::default();
        let woke = [
            WokeMessage {
                message_id: "m1".into(),
                sender: "Ada".into(),
                subject: "Old".into(),
            },
            WokeMessage {
                message_id: "m2".into(),
                sender: "Bob".into(),
                subject: "New".into(),
            },
        ];
        let n = snooze_woke(
            &s,
            Some(&"work".into()),
            Some(&"work-inbox".into()),
            &woke,
            &now,
            7,
        )
        .unwrap();
        assert_eq!(n.kind, NotificationKind::SnoozeWoke);
        assert_eq!(n.id, "snooze-woke:7");
        assert_eq!(n.tag, "snooze-woke:work-inbox");
        assert_eq!(n.folder_id, Some(FolderId::from("work-inbox")));
        assert_eq!(
            n.message_ids,
            [MessageId::from("m2"), MessageId::from("m1")]
        );
        assert_eq!(n.title.args["count"], 2u32.into());
        assert_eq!(n.body.unwrap().args["sender"], "Bob".into());
        assert!(snooze_woke(&s, None, None, &[], &now, 1).is_none());
        s.quiet_hours = Some(quiet("11:00", "13:00", &[]));
        assert!(snooze_woke(&s, None, None, &woke, &now, 1).is_none());
        // The test notification ignores quiet hours and the switch.
        s.enabled = false;
        let t = test_notification(&s, 3);
        assert_eq!(t.kind, NotificationKind::Test);
        assert_eq!(t.title.id, "notification-test-title");
    }

    fn arb_hhmm() -> impl Strategy<Value = String> {
        (0u32..24, 0u32..60).prop_map(|(h, m)| format!("{h:02}:{m:02}"))
    }

    proptest! {
        /// Quiet hours are exactly the minutes from start (inclusive) to end
        /// (exclusive), counted forward across midnight.
        #[test]
        fn quiet_minutes_match_a_forward_walk(
            start in arb_hhmm(), end in arb_hhmm(),
            minute in 0i64..(7 * 1440),
        ) {
            let q = quiet(&start, &end, &[]);
            let s = i64::from(parse_hhmm(&start).unwrap());
            let e = i64::from(parse_hhmm(&end).unwrap());
            let t = utc(2026, 10, 5, 0, 0) + Duration::minutes(minute);
            let m = minute % 1440;
            let len = (e - s).rem_euclid(1440);
            let want = len != 0 && (m - s).rem_euclid(1440) < len;
            prop_assert_eq!(in_quiet_hours(&q, &t), want);
        }

        /// With a weekday list, a quiet minute always lies in a range that
        /// started on a listed day.
        #[test]
        fn quiet_days_are_start_days(
            start in arb_hhmm(), end in arb_hhmm(),
            days in prop::collection::vec(prop::sample::select(Weekday::ALL.to_vec()), 0..4),
            minute in 0i64..(14 * 1440),
        ) {
            let q = quiet(&start, &end, &days);
            let s = i64::from(parse_hhmm(&start).unwrap());
            let t = utc(2026, 10, 5, 0, 0) + Duration::minutes(minute);
            if in_quiet_hours(&q, &t) {
                let m = minute % 1440;
                // The range started today when m ≥ s, else yesterday.
                let start_day = if m >= s { t } else { t - Duration::days(1) };
                prop_assert!(days.is_empty() || days.contains(&weekday(start_day.weekday())));
            }
        }
    }
}
