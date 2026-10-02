//! Background jobs (M4a, ADR 0079; design.md, "Plug-ins" → Background
//! jobs): scheduled tasks and event subscriptions of plug-ins with the
//! `background` permission.
//!
//! A job is declared in the manifest ([`PluginJobDef`]) with a cron-like
//! [`CronSchedule`] (five fields, local time) and / or the events it
//! subscribes to ([`PluginEventName`]); the plug-in registers its handler
//! in code (`ctx.jobs.register(id, handler)`). The handler gets a
//! [`PluginJobTrigger`].
//!
//! **Limits.** Each run has a wall-clock limit ([`JOB_WALL_CLOCK_MS`],
//! pending promises included) and every plug-in a cumulative CPU quota
//! per hour across all its calls ([`CPU_QUOTA_MS_PER_HOUR`]); a plug-in
//! over its quota skips runs until the hour has passed (`cpu-quota`). A
//! job never runs twice at the same time; missed schedule runs (Nail was
//! not running) run once at start. Schedules fire at most every
//! [`MIN_JOB_INTERVAL_MINUTES`] minutes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::hooks::PluginHookStats;
use crate::ids::PluginId;

/// Wall-clock limit of one job run (ms), pending promises included.
pub const JOB_WALL_CLOCK_MS: u64 = 60_000;
/// CPU time a plug-in may use per hour across all its calls (ms).
pub const CPU_QUOTA_MS_PER_HOUR: u64 = 120_000;
/// Shortest interval between two runs of a schedule (minutes).
pub const MIN_JOB_INTERVAL_MINUTES: u32 = 5;
/// Most jobs one manifest may declare.
pub const MAX_JOBS: usize = 10;
/// How long event deliveries are coalesced before a job runs (ms): many
/// `message.received` in a burst become one run with all message ids.
pub const EVENT_COALESCE_MS: u64 = 2_000;
/// Most message ids one event trigger carries.
pub const MAX_TRIGGER_MESSAGES: usize = 500;

/// Events a job can subscribe to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub enum PluginEventName {
    /// New mail arrived (after rules and arrival hooks).
    #[serde(rename = "message.received")]
    MessageReceived,
    /// A message was sent.
    #[serde(rename = "message.sent")]
    MessageSent,
    /// The selection in a UI changed (debounced).
    #[serde(rename = "selection.changed")]
    SelectionChanged,
    /// A reminder of an event or task fired.
    #[serde(rename = "event.reminder")]
    EventReminder,
    /// Nail (or the plug-in) started.
    #[serde(rename = "app.started")]
    AppStarted,
}

impl PluginEventName {
    /// All events.
    pub const ALL: [PluginEventName; 5] = [
        Self::MessageReceived,
        Self::MessageSent,
        Self::SelectionChanged,
        Self::EventReminder,
        Self::AppStarted,
    ];

    /// The name (`message.received`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MessageReceived => "message.received",
            Self::MessageSent => "message.sent",
            Self::SelectionChanged => "selection.changed",
            Self::EventReminder => "event.reminder",
            Self::AppStarted => "app.started",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == text)
    }

    /// The permission the job additionally needs to receive the event's
    /// data (message events carry message ids, readable with `mail.read`).
    pub fn data_permission(self) -> Option<super::PluginPermission> {
        match self {
            Self::MessageReceived | Self::MessageSent | Self::SelectionChanged => {
                Some(super::PluginPermission::MailRead)
            }
            Self::EventReminder => Some(super::PluginPermission::CalendarRead),
            Self::AppStarted => None,
        }
    }
}

/// A job in the manifest (`contributes.jobs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PluginJobDef {
    /// The job's name ([`super::is_contribution_name`]).
    pub id: String,
    /// What the plug-in manager calls it (or `%fluent-id%`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    /// When it runs: five cron fields in local time (`0,30 * * * *`,
    /// `0 8 * * 1-5`; steps like `/15` allowed) or `@hourly` / `@daily` /
    /// `@weekly` ([`CronSchedule::parse`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schedule: Option<String>,
    /// Events that run it.
    #[serde(default)]
    #[ts(optional, as = "Option<Vec<PluginEventName>>")]
    pub events: Vec<PluginEventName>,
}

impl PluginJobDef {
    /// Whether it has something to run on and a valid schedule.
    pub fn is_valid(&self) -> bool {
        (self.schedule.is_some() || !self.events.is_empty())
            && self
                .schedule
                .as_deref()
                .is_none_or(|s| CronSchedule::parse(s).is_ok())
    }
}

/// What started a job run (the handler's argument, camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginJobTrigger {
    /// Its schedule.
    Schedule {
        /// The scheduled time (RFC 3339).
        scheduled_for: String,
    },
    /// Events it subscribes to (coalesced).
    Event {
        /// The event.
        event: PluginEventName,
        /// Messages concerned (message events; ids the grant reaches).
        #[serde(default)]
        message_ids: Vec<crate::ids::MessageId>,
    },
    /// The user pressed "Run now" in the plug-in manager.
    Manual,
}

/// How a job run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginJobOutcome {
    /// It finished.
    Ok,
    /// It threw.
    Error,
    /// It ran out of wall-clock time.
    Timeout,
    /// Skipped: the plug-in was over its CPU quota.
    CpuQuota,
    /// Skipped: the previous run had not finished.
    Overlap,
}

impl PluginJobOutcome {
    /// The store's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Timeout => "timeout",
            Self::CpuQuota => "cpu-quota",
            Self::Overlap => "overlap",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Ok,
            Self::Error,
            Self::Timeout,
            Self::CpuQuota,
            Self::Overlap,
        ]
        .into_iter()
        .find(|o| o.as_str() == text)
    }

    /// Whether it counts as a failure.
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Error | Self::Timeout)
    }

    /// Fluent id (`plugin-job-outcome-<outcome>`).
    pub fn message_id(self) -> String {
        format!("plugin-job-outcome-{}", self.as_str())
    }
}

/// A job's state (the plug-in manager).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginJobStatus {
    /// The job's name.
    pub job: String,
    /// Its title (resolved), if any.
    pub title: Option<String>,
    /// Its schedule as written.
    pub schedule: Option<String>,
    /// Its events.
    pub events: Vec<PluginEventName>,
    /// Whether it runs now.
    pub running: bool,
    /// The last run (RFC 3339).
    pub last_run_at: Option<String>,
    /// How it ended.
    pub last_outcome: Option<PluginJobOutcome>,
    /// The last error (English, for logs).
    pub last_error: Option<String>,
    /// The next scheduled run (RFC 3339).
    pub next_run_at: Option<String>,
    /// Runs so far.
    pub run_count: u64,
    /// Failed runs so far.
    pub failure_count: u64,
}

/// What a plug-in does in the background ([`crate::Response::PluginActivity`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginActivity {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Its jobs.
    pub jobs: Vec<PluginJobStatus>,
    /// Its hooks.
    pub hooks: Vec<PluginHookStats>,
    /// CPU time used in the last hour (ms).
    pub cpu_ms_last_hour: u64,
    /// The quota ([`CPU_QUOTA_MS_PER_HOUR`]).
    pub cpu_quota_ms_per_hour: u64,
}

/// Why a cron schedule is not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CronIssue {
    /// Not five fields (or an unknown `@` name).
    FieldCount,
    /// A field is malformed or out of range.
    InvalidField,
    /// It would fire more often than every [`MIN_JOB_INTERVAL_MINUTES`].
    TooFrequent,
}

/// A parsed cron schedule: minute, hour, day of month, month, day of
/// week (0–7, 0 and 7 = Sunday), each `*`, a number, a range `a-b`, a step
/// `*/n` / `a-b/n`, or a comma list of those. As in cron, when both day
/// fields are restricted a day matches either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    day_restricted: bool,
    weekday_restricted: bool,
}

impl CronSchedule {
    /// Parses a schedule.
    pub fn parse(text: &str) -> Result<Self, CronIssue> {
        let expanded = match text.trim() {
            "@hourly" => "0 * * * *",
            "@daily" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            other if other.starts_with('@') => return Err(CronIssue::FieldCount),
            other => other,
        };
        let fields: Vec<&str> = expanded.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = fields[..] else {
            return Err(CronIssue::FieldCount);
        };
        let minutes = field(minute, 0, 59)?;
        let hours = field(hour, 0, 23)?;
        let days = field(day, 1, 31)?;
        let months = field(month, 1, 12)?;
        let mut weekdays = field(weekday, 0, 7)?;
        if weekdays & (1 << 7) != 0 {
            weekdays |= 1;
        }
        let schedule = Self {
            minutes,
            hours: u32::try_from(hours).map_err(|_| CronIssue::InvalidField)?,
            days: u32::try_from(days).map_err(|_| CronIssue::InvalidField)?,
            months: u16::try_from(months).map_err(|_| CronIssue::InvalidField)?,
            weekdays: u8::try_from(weekdays & 0x7f).map_err(|_| CronIssue::InvalidField)?,
            day_restricted: day != "*",
            weekday_restricted: weekday != "*",
        };
        if schedule.min_gap_minutes() < MIN_JOB_INTERVAL_MINUTES {
            return Err(CronIssue::TooFrequent);
        }
        Ok(schedule)
    }

    /// Whether the schedule fires at this local time (`weekday` 0 =
    /// Sunday).
    pub fn matches(&self, minute: u32, hour: u32, day: u32, month: u32, weekday: u32) -> bool {
        let bit = |set: u64, n: u32| n < 64 && set & (1 << n) != 0;
        let day_ok = bit(u64::from(self.days), day);
        let weekday_ok = bit(u64::from(self.weekdays), weekday % 7);
        let date_ok = match (self.day_restricted, self.weekday_restricted) {
            (true, true) => day_ok || weekday_ok,
            (true, false) => day_ok,
            (false, true) => weekday_ok,
            (false, false) => true,
        };
        bit(self.minutes, minute)
            && bit(u64::from(self.hours), hour)
            && bit(u64::from(self.months), month)
            && date_ok
    }

    /// The smallest gap between two firings within a day (minutes),
    /// counting the wrap from the last to the first minute of the next
    /// firing hour.
    fn min_gap_minutes(&self) -> u32 {
        let minutes: Vec<u32> = (0..60).filter(|m| self.minutes & (1 << m) != 0).collect();
        let hours: Vec<u32> = (0..24).filter(|h| self.hours & (1 << h) != 0).collect();
        let mut times: Vec<u32> = hours
            .iter()
            .flat_map(|h| minutes.iter().map(move |m| h * 60 + m))
            .collect();
        times.sort_unstable();
        if times.len() < 2 {
            return u32::MAX;
        }
        let mut gap = times
            .windows(2)
            .map(|w| w[1] - w[0])
            .min()
            .unwrap_or(u32::MAX);
        // Into the next day.
        gap = gap.min(times[0] + 24 * 60 - times[times.len() - 1]);
        gap
    }
}

fn field(text: &str, min: u32, max: u32) -> Result<u64, CronIssue> {
    let mut set = 0u64;
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => (
                range,
                step.parse::<u32>()
                    .ok()
                    .filter(|s| *s > 0)
                    .ok_or(CronIssue::InvalidField)?,
            ),
            None => (part, 1),
        };
        let (from, to) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (number(a, min, max)?, number(b, min, max)?)
        } else {
            let n = number(range, min, max)?;
            // `5/10` means 5, 15, 25, … like `5-max/10`.
            (n, if part.contains('/') { max } else { n })
        };
        if from > to {
            return Err(CronIssue::InvalidField);
        }
        let mut n = from;
        while n <= to {
            set |= 1 << n;
            n += step;
        }
    }
    Ok(set)
}

fn number(text: &str, min: u32, max: u32) -> Result<u32, CronIssue> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CronIssue::InvalidField);
    }
    text.parse::<u32>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or(CronIssue::InvalidField)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cron_schedules() {
        let quarter = CronSchedule::parse("*/15 * * * *").unwrap();
        assert!(quarter.matches(0, 3, 1, 1, 4));
        assert!(quarter.matches(45, 23, 31, 12, 0));
        assert!(!quarter.matches(7, 3, 1, 1, 4));

        let workdays = CronSchedule::parse("0 8 * * 1-5").unwrap();
        assert!(workdays.matches(0, 8, 14, 10, 1));
        assert!(!workdays.matches(0, 8, 18, 10, 0));
        assert!(!workdays.matches(0, 9, 14, 10, 1));

        let sunday = CronSchedule::parse("30 6 * * 7").unwrap();
        assert!(sunday.matches(30, 6, 5, 10, 0));
        assert!(sunday.matches(30, 6, 5, 10, 7));

        // Both day fields restricted: either matches.
        let either = CronSchedule::parse("0 12 1 * 1").unwrap();
        assert!(either.matches(0, 12, 1, 3, 3));
        assert!(either.matches(0, 12, 9, 3, 1));
        assert!(!either.matches(0, 12, 9, 3, 2));

        let list = CronSchedule::parse("5,35 9-17/4 * 1,6 *").unwrap();
        assert!(list.matches(35, 13, 2, 6, 3));
        assert!(!list.matches(35, 14, 2, 6, 3));
        assert!(!list.matches(5, 9, 2, 2, 3));
        let offset = CronSchedule::parse("10/20 * * * *").unwrap();
        assert!(offset.matches(50, 0, 1, 1, 1) && !offset.matches(0, 0, 1, 1, 1));

        assert_eq!(
            CronSchedule::parse("@daily").unwrap(),
            CronSchedule::parse("0 0 * * *").unwrap()
        );
        assert!(CronSchedule::parse("@hourly").is_ok());
        assert!(CronSchedule::parse("@weekly").is_ok());
    }

    #[test]
    fn bad_schedules() {
        for (text, issue) in [
            ("", CronIssue::FieldCount),
            ("* * * *", CronIssue::FieldCount),
            ("* * * * * *", CronIssue::FieldCount),
            ("@reboot", CronIssue::FieldCount),
            ("60 * * * *", CronIssue::InvalidField),
            ("0 24 * * *", CronIssue::InvalidField),
            ("0 0 0 * *", CronIssue::InvalidField),
            ("0 0 * 13 *", CronIssue::InvalidField),
            ("0 0 * * 8", CronIssue::InvalidField),
            ("5-1 * * * *", CronIssue::InvalidField),
            ("*/0 * * * *", CronIssue::InvalidField),
            ("a * * * *", CronIssue::InvalidField),
            ("-1 * * * *", CronIssue::InvalidField),
            ("* * * * *", CronIssue::TooFrequent),
            ("*/2 * * * *", CronIssue::TooFrequent),
            ("0,3 * * * *", CronIssue::TooFrequent),
        ] {
            assert_eq!(CronSchedule::parse(text), Err(issue), "{text:?}");
        }
        // 23:58, then 00:02 the next day.
        assert_eq!(
            CronSchedule::parse("58,2 0,23 * * *"),
            Err(CronIssue::TooFrequent)
        );
        assert!(CronSchedule::parse("*/5 * * * *").is_ok());
    }

    #[test]
    fn jobs_and_events() {
        let job: PluginJobDef = serde_json::from_value(json!({
            "id": "digest", "schedule": "0 8 * * 1-5", "events": ["app.started"]
        }))
        .unwrap();
        assert!(job.is_valid());
        let events_only: PluginJobDef =
            serde_json::from_value(json!({"id": "tagger", "events": ["message.received"]}))
                .unwrap();
        assert!(events_only.is_valid());
        let nothing: PluginJobDef = serde_json::from_value(json!({"id": "idle"})).unwrap();
        assert!(!nothing.is_valid());
        let bad: PluginJobDef =
            serde_json::from_value(json!({"id": "x", "schedule": "* * * * *"})).unwrap();
        assert!(!bad.is_valid());
        for event in PluginEventName::ALL {
            assert_eq!(PluginEventName::parse(event.as_str()), Some(event));
            assert_eq!(serde_json::to_value(event).unwrap(), json!(event.as_str()));
        }
        assert_eq!(
            PluginEventName::MessageReceived.data_permission(),
            Some(super::super::PluginPermission::MailRead)
        );
        let trigger = PluginJobTrigger::Event {
            event: PluginEventName::MessageReceived,
            message_ids: vec!["m1".into()],
        };
        assert_eq!(
            serde_json::to_value(&trigger).unwrap(),
            json!({"kind": "event", "event": "message.received", "messageIds": ["m1"]})
        );
        assert_eq!(
            serde_json::to_value(PluginJobTrigger::Schedule {
                scheduled_for: "2026-10-02T08:00:00Z".into()
            })
            .unwrap(),
            json!({"kind": "schedule", "scheduledFor": "2026-10-02T08:00:00Z"})
        );
        for outcome in [
            PluginJobOutcome::Ok,
            PluginJobOutcome::Error,
            PluginJobOutcome::Timeout,
            PluginJobOutcome::CpuQuota,
            PluginJobOutcome::Overlap,
        ] {
            assert_eq!(PluginJobOutcome::parse(outcome.as_str()), Some(outcome));
            assert!(crate::localized::is_message_id(&outcome.message_id()));
        }
        assert!(PluginJobOutcome::Timeout.is_failure());
        assert!(!PluginJobOutcome::CpuQuota.is_failure());
    }
}
