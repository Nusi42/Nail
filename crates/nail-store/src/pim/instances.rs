//! The instance cache: expanded occurrences of events for fast range
//! queries (`pim_event_instances` + `pim_instance_windows`).
//!
//! * A single event has one instance, written when it is saved.
//! * A series is expanded **on demand** by an [`InstanceExpander`]
//!   (`nail-ical` in the core) in chunks of [`INSTANCE_CHUNK`]: a range
//!   query expands only what the event's window does not cover yet and
//!   records the grown window, so scrolling a month view expands each
//!   series once.
//! * Times are cache seconds ([`super::time`]): instants for zoned times,
//!   wall clock for dates and floating times. A query widens by
//!   [`FLOATING_SLACK`] and places floating rows in the viewer's zone, so a
//!   change of the **viewer's** zone needs no re-expansion.
//! * **Invalidation**: saving an event drops its instances and window (the
//!   next query re-expands); [`Store::invalidate_instances`] drops a
//!   calendar's (its zone changed) or everything; [`Store::note_zone_data`]
//!   drops everything when the zone database changed (new DST rules move
//!   zoned occurrences).
//!
//! Occurrences are expanded by their start; a chunk before the requested
//! range is expanded too, so occurrences up to [`INSTANCE_CHUNK`] long that
//! started earlier still show.

use std::collections::HashMap;

use chrono_tz::Tz;
use nail_api::pim::calendar::{EventStatus, Transparency};
use nail_api::pim::{CalendarEvent, EventFields, EventInstance, PimTime};
use nail_api::{CalendarId, EventId};
use rusqlite::{OptionalExtension, Transaction, params};

use super::items::{in_list, load_events};
use super::time::CacheTime;
use super::time::{FLOATING_SLACK, cache_time, parse_zone, place, time_like, utc_string};
use crate::error::{Error, Result};
use crate::store::Store;

/// Expansion granularity (90 days).
pub const INSTANCE_CHUNK: i64 = 90 * 86_400;

/// Window of a single (non-recurring) event: everything.
const WHOLE_FROM: i64 = i64::MIN / 4;
const WHOLE_UNTIL: i64 = i64::MAX / 4;

/// The app setting holding the zone database version of the cache.
const ZONE_DATA_KEY: &str = "pim.instance-cache.zone-data";

/// One occurrence produced by an [`InstanceExpander`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedInstance {
    /// The occurrence's `RECURRENCE-ID`; `None` for a single event.
    pub recurrence_id: Option<PimTime>,
    /// Its start (an override's start when overridden).
    pub start: PimTime,
    /// Its end.
    pub end: PimTime,
    /// An override changed it.
    pub overridden: bool,
    /// The occurrence is cancelled (override with `STATUS:CANCELLED`).
    pub cancelled: bool,
}

/// Expands a series (implemented with `nail-ical` by the core).
pub trait InstanceExpander {
    /// Every occurrence whose start, in cache seconds
    /// ([`super::cache_time`]), lies in `[from, until)` — RRULEs, RDATEs,
    /// minus EXDATEs, with overrides applied (an override moved into the
    /// range counts, one moved out does not). The store drops anything
    /// outside the range.
    fn expand(&self, event: &CalendarEvent, from: i64, until: i64) -> Vec<ExpandedInstance>;
}

/// What [`Store::invalidate_instances`] drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceScope {
    /// One series.
    Event(EventId),
    /// Every event of a calendar.
    Calendar(CalendarId),
    /// The whole cache.
    All,
}

pub(crate) fn invalidate_event_in_tx(tx: &Transaction<'_>, event_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM pim_event_instances WHERE event_id = ?1",
        [event_id],
    )?;
    tx.execute(
        "DELETE FROM pim_instance_windows WHERE event_id = ?1",
        [event_id],
    )?;
    Ok(())
}

fn align_down(t: i64) -> i64 {
    t.div_euclid(INSTANCE_CHUNK) * INSTANCE_CHUNK
}

fn align_up(t: i64) -> i64 {
    let down = align_down(t);
    if down == t { t } else { down + INSTANCE_CHUNK }
}

/// A row to cache.
struct Row {
    recurrence_id: String,
    start: CacheTime,
    end: CacheTime,
    all_day: bool,
    overridden: bool,
    cancelled: bool,
    busy: bool,
}

fn busy(fields: &EventFields, cancelled: bool) -> bool {
    fields.transparency == Transparency::Busy
        && fields.status != EventStatus::Cancelled
        && !cancelled
}

fn fields_of<'a>(event: &'a CalendarEvent, recurrence_id: Option<&PimTime>) -> &'a EventFields {
    recurrence_id
        .and_then(|rid| event.overrides.iter().find(|o| &o.recurrence_id == rid))
        .map(|o| &o.fields)
        .unwrap_or(&event.fields)
}

fn row_of(event: &CalendarEvent, instance: &ExpandedInstance) -> Option<Row> {
    let start = cache_time(&instance.start)?;
    let mut end = cache_time(&instance.end)?;
    if end.secs < start.secs {
        end.secs = start.secs;
    }
    let fields = fields_of(event, instance.recurrence_id.as_ref());
    Some(Row {
        recurrence_id: match &instance.recurrence_id {
            Some(rid) => serde_json::to_string(rid).ok()?,
            None => String::new(),
        },
        start,
        end,
        all_day: instance.start.is_date(),
        overridden: instance.overridden,
        cancelled: instance.cancelled,
        busy: busy(fields, instance.cancelled),
    })
}

fn insert_rows(tx: &Transaction<'_>, event: &CalendarEvent, rows: &[Row]) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "INSERT OR REPLACE INTO pim_event_instances
             (event_id, calendar_id, recurrence_id, start_at, end_at, floating, all_day,
              overridden, cancelled, busy)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for row in rows {
        stmt.execute(params![
            event.id.as_str(),
            event.calendar_id.as_str(),
            row.recurrence_id,
            row.start.secs,
            row.end.secs,
            row.start.floating,
            row.all_day,
            row.overridden,
            row.cancelled,
            row.busy
        ])?;
    }
    Ok(())
}

fn set_window(tx: &Transaction<'_>, event_id: &str, from: i64, until: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO pim_instance_windows (event_id, expanded_from, expanded_until)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (event_id) DO UPDATE SET
             expanded_from = excluded.expanded_from, expanded_until = excluded.expanded_until",
        params![event_id, from, until],
    )?;
    Ok(())
}

/// Expands what is missing of `[need_from, need_until)` for one event.
fn fill(
    tx: &Transaction<'_>,
    event: &CalendarEvent,
    window: Option<(i64, i64)>,
    need_from: i64,
    need_until: i64,
    expander: &dyn InstanceExpander,
) -> Result<()> {
    if !event.is_recurring() {
        let single = ExpandedInstance {
            recurrence_id: None,
            start: event.fields.start.clone(),
            end: event.fields.end.clone(),
            overridden: false,
            cancelled: false,
        };
        tx.execute(
            "DELETE FROM pim_event_instances WHERE event_id = ?1",
            [event.id.as_str()],
        )?;
        let rows: Vec<Row> = row_of(event, &single).into_iter().collect();
        insert_rows(tx, event, &rows)?;
        return set_window(tx, event.id.as_str(), WHOLE_FROM, WHOLE_UNTIL);
    }
    let pieces: Vec<(i64, i64)> = match window {
        None => vec![(need_from, need_until)],
        Some((from, until)) => {
            let mut p = Vec::new();
            if need_from < from {
                p.push((need_from, from));
            }
            if need_until > until {
                p.push((until, need_until));
            }
            p
        }
    };
    for (from, until) in pieces {
        let rows: Vec<Row> = expander
            .expand(event, from, until)
            .iter()
            .filter_map(|i| row_of(event, i))
            .filter(|r| r.start.secs >= from && r.start.secs < until)
            .collect();
        insert_rows(tx, event, &rows)?;
    }
    let (from, until) = match window {
        None => (need_from, need_until),
        Some((f, u)) => (f.min(need_from), u.max(need_until)),
    };
    set_window(tx, event.id.as_str(), from, until)
}

impl Store {
    /// Drops cached instances (they are expanded again on the next query).
    /// Returns how many series were dropped.
    pub fn invalidate_instances(&self, scope: &InstanceScope) -> Result<usize> {
        self.with_tx(|tx| {
            let (filter, arg): (&str, Option<&str>) = match scope {
                InstanceScope::Event(id) => ("event_id = ?1", Some(id.as_str())),
                InstanceScope::Calendar(id) => (
                    "event_id IN (SELECT id FROM pim_events WHERE calendar_id = ?1)",
                    Some(id.as_str()),
                ),
                InstanceScope::All => ("?1 IS NULL", None),
            };
            tx.execute(
                &format!("DELETE FROM pim_event_instances WHERE {filter}"),
                [arg],
            )?;
            Ok(tx.execute(
                &format!("DELETE FROM pim_instance_windows WHERE {filter}"),
                [arg],
            )?)
        })
    }

    /// Records the version of the time-zone database the cache was built
    /// with (e.g. chrono-tz's IANA release); a different version drops the
    /// whole cache. Returns whether it was dropped.
    pub fn note_zone_data(&self, version: &str) -> Result<bool> {
        let known: Option<String> = self.setting(ZONE_DATA_KEY)?;
        if known.as_deref() == Some(version) {
            return Ok(false);
        }
        self.invalidate_instances(&InstanceScope::All)?;
        self.set_setting(ZONE_DATA_KEY, version)?;
        Ok(known.is_some())
    }

    /// The expanded window of a series, `None` when not expanded.
    pub fn instance_window(&self, event: &EventId) -> Result<Option<(i64, i64)>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT expanded_from, expanded_until FROM pim_instance_windows
                     WHERE event_id = ?1",
                    [event.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?)
        })
    }

    /// The calendars a query covers: the given ones, or every visible one
    /// holding events.
    fn event_calendars(&self, calendars: Option<&[CalendarId]>) -> Result<Vec<String>> {
        Ok(match calendars {
            Some(ids) => ids.iter().map(|c| c.0.clone()).collect(),
            None => self
                .calendars(None)?
                .into_iter()
                .filter(|c| c.settings.visible && c.has_events())
                .map(|c| c.id.0)
                .collect(),
        })
    }

    /// Makes sure the cache covers `[from, to)` (Unix seconds, widened for
    /// floating times) for the events of `calendars`.
    fn ensure_instances(
        &self,
        calendars: &[String],
        from: i64,
        to: i64,
        expander: &dyn InstanceExpander,
    ) -> Result<()> {
        let need_from = align_down(from - FLOATING_SLACK) - INSTANCE_CHUNK;
        let need_until = align_up(to + FLOATING_SLACK);
        let refs: Vec<&str> = calendars.iter().map(String::as_str).collect();
        // Events that can have occurrences there and lack expansion.
        let missing: Vec<(String, Option<(i64, i64)>)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT e.id, w.expanded_from, w.expanded_until
                 FROM pim_events e
                 LEFT JOIN pim_instance_windows w ON w.event_id = e.id
                 WHERE {} AND e.first_start < ?{n1} AND (e.last_end IS NULL OR e.last_end > ?{n2})
                   AND (w.event_id IS NULL OR w.expanded_from > ?{n2}
                        OR w.expanded_until < ?{n1})",
                in_list("e.calendar_id", refs.len(), 1),
                n1 = refs.len() + 1,
                n2 = refs.len() + 2,
            ))?;
            let mut args: Vec<rusqlite::types::Value> = refs
                .iter()
                .map(|r| rusqlite::types::Value::Text((*r).to_owned()))
                .collect();
            args.push(need_until.into());
            args.push(need_from.into());
            let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
                let id: String = r.get(0)?;
                let from: Option<i64> = r.get(1)?;
                let until: Option<i64> = r.get(2)?;
                Ok((id, from.zip(until)))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        if missing.is_empty() {
            return Ok(());
        }
        let ids: Vec<&str> = missing.iter().map(|(id, _)| id.as_str()).collect();
        let events: HashMap<String, CalendarEvent> = self
            .with_conn(|conn| load_events(conn, &in_list("e.id", ids.len(), 1), &ids))?
            .into_iter()
            .map(|e| (e.id.0.clone(), e))
            .collect();
        self.with_tx(|tx| {
            for (id, window) in &missing {
                if let Some(event) = events.get(id) {
                    fill(tx, event, *window, need_from, need_until, expander)?;
                }
            }
            Ok(())
        })
    }

    /// The occurrences of events overlapping `[from, to)` (Unix seconds),
    /// with dates and floating times placed in `viewer_tz` (an IANA name),
    /// by start (all-day first), from the instance cache (expanding what it
    /// lacks with `expander`). `calendars` `None` = every visible calendar.
    pub fn list_event_instances(
        &self,
        from: i64,
        to: i64,
        calendars: Option<&[CalendarId]>,
        viewer_tz: &str,
        expander: &dyn InstanceExpander,
    ) -> Result<Vec<EventInstance>> {
        let tz = parse_zone(viewer_tz)
            .ok_or_else(|| Error::InvalidData(format!("unknown time zone {viewer_tz:?}")))?;
        let calendars = self.event_calendars(calendars)?;
        if calendars.is_empty() || to <= from {
            return Ok(Vec::new());
        }
        self.ensure_instances(&calendars, from, to, expander)?;
        let rows = self.instance_rows(&calendars, from, to, &tz)?;
        let ids: Vec<&str> = {
            let mut ids: Vec<&str> = rows.iter().map(|r| r.event_id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let events: HashMap<String, CalendarEvent> = self
            .with_conn(|conn| load_events(conn, &in_list("e.id", ids.len(), 1), &ids))?
            .into_iter()
            .map(|e| (e.id.0.clone(), e))
            .collect();
        let mut out: Vec<(i64, EventInstance)> = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(event) = events.get(&row.event_id) else {
                continue;
            };
            let recurrence_id: Option<PimTime> = if row.recurrence_id.is_empty() {
                None
            } else {
                serde_json::from_str(&row.recurrence_id).ok()
            };
            let fields = fields_of(event, recurrence_id.as_ref());
            let (start, end) = if row.overridden {
                (fields.start.clone(), fields.end.clone())
            } else {
                (
                    time_like(&event.fields.start, row.start_at),
                    time_like(&event.fields.end, row.end_at),
                )
            };
            out.push((
                row.placed_start,
                EventInstance {
                    event_id: event.id.clone(),
                    calendar_id: event.calendar_id.clone(),
                    recurrence_id,
                    start,
                    end,
                    start_utc: utc_string(row.placed_start),
                    end_utc: utc_string(row.placed_end),
                    all_day: row.all_day,
                    title: fields.title.clone(),
                    location: fields.location.clone(),
                    status: if row.cancelled {
                        EventStatus::Cancelled
                    } else {
                        fields.status
                    },
                    transparency: fields.transparency,
                    color: fields.color.clone(),
                    recurring: event.is_recurring(),
                    overridden: row.overridden,
                    has_attendees: !fields.attendees.is_empty(),
                    has_alarms: !fields.alarms.is_empty(),
                    read_only: event.read_only,
                },
            ));
        }
        out.sort_by(|(a, x), (b, y)| {
            (a, !x.all_day, &x.title, &x.event_id).cmp(&(b, !y.all_day, &y.title, &y.event_id))
        });
        Ok(out.into_iter().map(|(_, i)| i).collect())
    }

    /// Busy time of `[from, to)` (Unix seconds) in the calendars: merged,
    /// sorted intervals of busy, not cancelled occurrences, clipped to the
    /// range. Dates and floating times are placed in `viewer_tz`.
    pub fn busy_intervals(
        &self,
        from: i64,
        to: i64,
        calendars: Option<&[CalendarId]>,
        viewer_tz: &str,
        expander: &dyn InstanceExpander,
    ) -> Result<Vec<(i64, i64)>> {
        let tz = parse_zone(viewer_tz)
            .ok_or_else(|| Error::InvalidData(format!("unknown time zone {viewer_tz:?}")))?;
        let calendars = self.event_calendars(calendars)?;
        if calendars.is_empty() || to <= from {
            return Ok(Vec::new());
        }
        self.ensure_instances(&calendars, from, to, expander)?;
        let mut intervals: Vec<(i64, i64)> = self
            .instance_rows(&calendars, from, to, &tz)?
            .into_iter()
            .filter(|r| r.busy)
            .map(|r| (r.placed_start.max(from), r.placed_end.min(to)))
            .filter(|(s, e)| e > s)
            .collect();
        intervals.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (s, e) in intervals {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        Ok(merged)
    }

    fn instance_rows(
        &self,
        calendars: &[String],
        from: i64,
        to: i64,
        tz: &Tz,
    ) -> Result<Vec<CachedRow>> {
        let refs: Vec<&str> = calendars.iter().map(String::as_str).collect();
        let n = refs.len();
        let rows: Vec<CachedRow> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT event_id, recurrence_id, start_at, end_at, floating, all_day, overridden,
                        cancelled, busy
                 FROM pim_event_instances
                 WHERE {} AND start_at < ?{} AND end_at >= ?{}",
                in_list("calendar_id", n, 1),
                n + 1,
                n + 2
            ))?;
            let mut args: Vec<rusqlite::types::Value> = refs
                .iter()
                .map(|r| rusqlite::types::Value::Text((*r).to_owned()))
                .collect();
            args.push((to + FLOATING_SLACK).into());
            args.push((from - FLOATING_SLACK).into());
            let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
                let floating: bool = r.get(4)?;
                let start_at: i64 = r.get(2)?;
                let end_at: i64 = r.get(3)?;
                Ok(CachedRow {
                    event_id: r.get(0)?,
                    recurrence_id: r.get(1)?,
                    start_at,
                    end_at,
                    placed_start: place(
                        CacheTime {
                            secs: start_at,
                            floating,
                        },
                        tz,
                    ),
                    placed_end: place(
                        CacheTime {
                            secs: end_at,
                            floating,
                        },
                        tz,
                    ),
                    all_day: r.get(5)?,
                    overridden: r.get(6)?,
                    cancelled: r.get(7)?,
                    busy: r.get(8)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        Ok(rows
            .into_iter()
            .filter(|r| {
                r.placed_start < to
                    && (r.placed_end > from
                        || (r.placed_end == r.placed_start && r.placed_start >= from))
            })
            .collect())
    }
}

/// A cached occurrence placed in the viewer's zone.
struct CachedRow {
    event_id: String,
    recurrence_id: String,
    start_at: i64,
    end_at: i64,
    placed_start: i64,
    placed_end: i64,
    all_day: bool,
    overridden: bool,
    cancelled: bool,
    busy: bool,
}

/// Free gaps of at least `min_len` seconds in `[from, to)` between sorted,
/// merged `busy` intervals.
pub fn free_slots(busy: &[(i64, i64)], from: i64, to: i64, min_len: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut cursor = from;
    for &(s, e) in busy {
        if s > cursor && s.min(to) - cursor >= min_len.max(1) {
            out.push((cursor, s.min(to)));
        }
        cursor = cursor.max(e);
        if cursor >= to {
            return out;
        }
    }
    if to - cursor >= min_len.max(1) {
        out.push((cursor, to));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use nail_api::pim::{EventOverride, PimTime, RecurrenceEnd};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::pim::PimItem;
    use crate::pim::testing::{self, CAL, TestExpander, at, event_at, raw};

    fn save(store: &Store, event: &CalendarEvent) {
        store
            .save_pim_item(&PimItem::Event(event.clone()), &raw(event.id.as_str()), 1)
            .unwrap();
    }

    fn starts(instances: &[EventInstance]) -> Vec<&str> {
        instances.iter().map(|i| i.start_utc.as_str()).collect()
    }

    fn daily_berlin(count: u32) -> CalendarEvent {
        let mut e = event_at(
            "s",
            "us",
            "Standup",
            PimTime::zoned("2026-03-27T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-03-27T09:15:00", "Europe/Berlin"),
        );
        e.recurrence = Some(testing::daily(RecurrenceEnd::Count { count }));
        e
    }

    #[test]
    fn a_daily_series_keeps_its_wall_time_across_dst() {
        let store = testing::store();
        save(&store, &daily_berlin(4));
        let expander = TestExpander::default();
        let instances = store
            .list_event_instances(
                at("2026-03-27T00:00:00Z"),
                at("2026-04-01T00:00:00Z"),
                None,
                "Europe/Berlin",
                &expander,
            )
            .unwrap();
        // 09:00 Berlin is 08:00 UTC before 29 March and 07:00 after.
        assert_eq!(
            starts(&instances),
            [
                "2026-03-27T08:00:00Z",
                "2026-03-28T08:00:00Z",
                "2026-03-29T07:00:00Z",
                "2026-03-30T07:00:00Z"
            ]
        );
        let third = &instances[2];
        assert_eq!(
            third.start,
            PimTime::zoned("2026-03-29T09:00:00", "Europe/Berlin")
        );
        assert_eq!(
            third.end,
            PimTime::zoned("2026-03-29T09:15:00", "Europe/Berlin")
        );
        assert_eq!(third.end_utc, "2026-03-29T07:15:00Z");
        assert_eq!(
            third.recurrence_id,
            Some(PimTime::zoned("2026-03-29T09:00:00", "Europe/Berlin"))
        );
        assert!(third.recurring && !third.overridden);
        // A New York viewer sees the same instants.
        let ny = store
            .list_event_instances(
                at("2026-03-27T00:00:00Z"),
                at("2026-04-01T00:00:00Z"),
                None,
                "America/New_York",
                &expander,
            )
            .unwrap();
        assert_eq!(starts(&ny), starts(&instances));
    }

    #[test]
    fn expansion_is_cached_and_grows_by_chunks() {
        let store = testing::store();
        let mut series = daily_berlin(1);
        series.recurrence = Some(testing::daily(RecurrenceEnd::Never));
        save(&store, &series);
        let expander = TestExpander::default();
        let march = (at("2026-03-01T00:00:00Z"), at("2026-04-01T00:00:00Z"));
        let first = store
            .list_event_instances(march.0, march.1, None, "UTC", &expander)
            .unwrap();
        assert_eq!(first.len(), 5); // 27–31 March
        let calls = expander.calls.load(Ordering::Relaxed);
        assert!(calls >= 1);
        // The same range again: no expansion.
        store
            .list_event_instances(march.0, march.1, None, "UTC", &expander)
            .unwrap();
        assert_eq!(expander.calls.load(Ordering::Relaxed), calls);
        let (from, until) = store.instance_window(&"s".into()).unwrap().unwrap();
        assert!(from <= march.0 - INSTANCE_CHUNK && until >= march.1);
        // A later range grows the window.
        let later = store
            .list_event_instances(
                at("2027-01-01T00:00:00Z"),
                at("2027-01-03T00:00:00Z"),
                None,
                "UTC",
                &expander,
            )
            .unwrap();
        assert_eq!(later.len(), 2);
        let (from2, until2) = store.instance_window(&"s".into()).unwrap().unwrap();
        assert_eq!(from2, from);
        assert!(until2 > until);
    }

    #[test]
    fn edits_invalidate_the_cache() {
        let store = testing::store();
        let series = daily_berlin(3);
        save(&store, &series);
        let expander = TestExpander::default();
        let range = (at("2026-03-27T00:00:00Z"), at("2026-04-02T00:00:00Z"));
        assert_eq!(
            store
                .list_event_instances(range.0, range.1, None, "UTC", &expander)
                .unwrap()
                .len(),
            3
        );
        // Move one occurrence (an override) and cancel another.
        let mut edited = series.clone();
        let mut moved = edited.fields.clone();
        moved.start = PimTime::zoned("2026-03-28T14:00:00", "Europe/Berlin");
        moved.end = PimTime::zoned("2026-03-28T14:30:00", "Europe/Berlin");
        moved.title = "Moved".into();
        let mut cancelled = edited.fields.clone();
        cancelled.status = EventStatus::Cancelled;
        cancelled.start = PimTime::zoned("2026-03-29T09:00:00", "Europe/Berlin");
        cancelled.end = PimTime::zoned("2026-03-29T09:15:00", "Europe/Berlin");
        edited.overrides = vec![
            EventOverride {
                recurrence_id: PimTime::zoned("2026-03-28T09:00:00", "Europe/Berlin"),
                fields: moved,
                this_and_future: false,
            },
            EventOverride {
                recurrence_id: PimTime::zoned("2026-03-29T09:00:00", "Europe/Berlin"),
                fields: cancelled,
                this_and_future: false,
            },
        ];
        save(&store, &edited);
        assert_eq!(store.instance_window(&"s".into()).unwrap(), None);
        let instances = store
            .list_event_instances(range.0, range.1, None, "UTC", &expander)
            .unwrap();
        assert_eq!(instances.len(), 3);
        assert_eq!(instances[1].title, "Moved");
        assert_eq!(instances[1].start_utc, "2026-03-28T13:00:00Z");
        assert!(instances[1].overridden);
        assert_eq!(
            instances[1].start,
            PimTime::zoned("2026-03-28T14:00:00", "Europe/Berlin")
        );
        assert_eq!(instances[2].status, EventStatus::Cancelled);
        // Cancelled occurrences are not busy.
        let busy = store
            .busy_intervals(range.0, range.1, None, "UTC", &expander)
            .unwrap();
        assert_eq!(busy.len(), 2);
        // Explicit invalidation.
        assert_eq!(
            store
                .invalidate_instances(&InstanceScope::Calendar(CAL.into()))
                .unwrap(),
            1
        );
        assert_eq!(store.instance_window(&"s".into()).unwrap(), None);
        store
            .list_event_instances(range.0, range.1, None, "UTC", &expander)
            .unwrap();
        assert_eq!(
            store
                .invalidate_instances(&InstanceScope::Event("s".into()))
                .unwrap(),
            1
        );
        store
            .list_event_instances(range.0, range.1, None, "UTC", &expander)
            .unwrap();
        assert_eq!(store.invalidate_instances(&InstanceScope::All).unwrap(), 1);
    }

    #[test]
    fn floating_and_all_day_events_follow_the_viewer() {
        let store = testing::store();
        save(
            &store,
            &event_at(
                "f",
                "uf",
                "Gym",
                PimTime::floating("2026-10-01T18:00:00"),
                PimTime::floating("2026-10-01T19:00:00"),
            ),
        );
        save(
            &store,
            &event_at(
                "d",
                "ud",
                "Holiday",
                PimTime::date("2026-10-01"),
                PimTime::date("2026-10-02"),
            ),
        );
        let expander = TestExpander::default();
        let day = |tz: &str, from: &str, to: &str| {
            store
                .list_event_instances(at(from), at(to), None, tz, &expander)
                .unwrap()
        };
        let berlin = day(
            "Europe/Berlin",
            "2026-09-30T22:00:00Z",
            "2026-10-01T22:00:00Z",
        );
        assert_eq!(
            berlin
                .iter()
                .map(|i| i.event_id.as_str())
                .collect::<Vec<_>>(),
            ["d", "f"]
        );
        assert!(berlin[0].all_day);
        assert_eq!(berlin[0].start_utc, "2026-09-30T22:00:00Z");
        assert_eq!(berlin[1].start_utc, "2026-10-01T16:00:00Z");
        // Viewed from Tokyo the same wall times are other instants — with
        // no re-expansion.
        let calls = expander.calls.load(Ordering::Relaxed);
        let tokyo = day("Asia/Tokyo", "2026-09-30T15:00:00Z", "2026-10-01T15:00:00Z");
        assert_eq!(tokyo.len(), 2);
        assert_eq!(tokyo[1].start_utc, "2026-10-01T09:00:00Z");
        assert_eq!(expander.calls.load(Ordering::Relaxed), calls);
        // The day after in Tokyo has nothing.
        assert!(day("Asia/Tokyo", "2026-10-01T15:00:00Z", "2026-10-02T15:00:00Z").is_empty());
        assert!(
            store
                .list_event_instances(0, 10, None, "Not/AZone", &expander)
                .is_err()
        );
    }

    #[test]
    fn hidden_calendars_and_ranges() {
        let store = testing::store();
        save(
            &store,
            &testing::Fixtures::new().event("e", "ue", "Visible"),
        );
        let expander = TestExpander::default();
        let range = (at("2026-10-01T00:00:00Z"), at("2026-10-02T00:00:00Z"));
        assert_eq!(
            store
                .list_event_instances(range.0, range.1, None, "UTC", &expander)
                .unwrap()
                .len(),
            1
        );
        let mut settings = store.calendar(&CAL.into()).unwrap().unwrap().settings;
        settings.visible = false;
        store
            .update_calendar_settings(&CAL.into(), &settings)
            .unwrap();
        assert!(
            store
                .list_event_instances(range.0, range.1, None, "UTC", &expander)
                .unwrap()
                .is_empty()
        );
        // Asked for by id, hidden calendars still answer.
        assert_eq!(
            store
                .list_event_instances(range.0, range.1, Some(&[CAL.into()]), "UTC", &expander)
                .unwrap()
                .len(),
            1
        );
        // A range touching only the end is empty.
        assert!(
            store
                .list_event_instances(
                    at("2026-10-01T08:00:00Z"),
                    at("2026-10-01T09:00:00Z"),
                    Some(&[CAL.into()]),
                    "UTC",
                    &expander
                )
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .list_event_instances(range.1, range.0, None, "UTC", &expander)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_zone_database_update_drops_the_cache() {
        let store = testing::store();
        save(&store, &daily_berlin(2));
        let expander = TestExpander::default();
        assert!(!store.note_zone_data("2025b").unwrap());
        store
            .list_event_instances(
                at("2026-03-27T00:00:00Z"),
                at("2026-03-30T00:00:00Z"),
                None,
                "UTC",
                &expander,
            )
            .unwrap();
        assert!(store.instance_window(&"s".into()).unwrap().is_some());
        assert!(!store.note_zone_data("2025b").unwrap());
        assert!(store.instance_window(&"s".into()).unwrap().is_some());
        assert!(store.note_zone_data("2026a").unwrap());
        assert!(store.instance_window(&"s".into()).unwrap().is_none());
    }

    #[test]
    fn a_calendar_zone_change_drops_its_cache() {
        let store = testing::store();
        save(&store, &daily_berlin(2));
        let expander = TestExpander::default();
        store
            .list_event_instances(
                at("2026-03-27T00:00:00Z"),
                at("2026-03-30T00:00:00Z"),
                None,
                "UTC",
                &expander,
            )
            .unwrap();
        store
            .set_calendar_timezone(&CAL.into(), Some("Europe/Berlin"))
            .unwrap();
        assert!(store.instance_window(&"s".into()).unwrap().is_none());
        // Saving the calendar with the same zone keeps the cache.
        store
            .list_event_instances(
                at("2026-03-27T00:00:00Z"),
                at("2026-03-30T00:00:00Z"),
                None,
                "UTC",
                &expander,
            )
            .unwrap();
        let calendar = store.calendar(&CAL.into()).unwrap().unwrap();
        store.save_calendar(&calendar, None).unwrap();
        assert!(store.instance_window(&"s".into()).unwrap().is_some());
        let mut moved = calendar.clone();
        moved.timezone = Some("America/New_York".into());
        store.save_calendar(&moved, None).unwrap();
        assert!(store.instance_window(&"s".into()).unwrap().is_none());
    }

    #[test]
    fn deleting_drops_instances() {
        let store = testing::store();
        let e = testing::Fixtures::new().event("e", "ue", "Gone");
        save(&store, &e);
        let expander = TestExpander::default();
        let range = (at("2026-10-01T00:00:00Z"), at("2026-10-02T00:00:00Z"));
        assert_eq!(
            store
                .list_event_instances(range.0, range.1, None, "UTC", &expander)
                .unwrap()
                .len(),
            1
        );
        store
            .delete_pim_item(&PimItem::Event(e).item_ref(), 2)
            .unwrap();
        assert!(
            store
                .list_event_instances(range.0, range.1, None, "UTC", &expander)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn free_time() {
        let busy = [(10, 20), (25, 30)];
        assert_eq!(free_slots(&busy, 0, 40, 1), [(0, 10), (20, 25), (30, 40)]);
        assert_eq!(free_slots(&busy, 0, 40, 6), [(0, 10), (30, 40)]);
        assert_eq!(free_slots(&busy, 12, 28, 1), [(20, 25)]);
        assert_eq!(free_slots(&[], 0, 5, 10), []);
        assert_eq!(free_slots(&[(0, 50)], 0, 40, 1), []);
    }
}
