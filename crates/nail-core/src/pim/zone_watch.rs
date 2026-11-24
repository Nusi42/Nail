//! Noticing a change of the computer's time zone (R8): reminders are
//! planned in the local zone, so a change replans them at once instead of
//! at the next hourly pass.
//!
//! * Everywhere the reminder task compares the zone's name on each wake
//!   ([`ZoneTracker`]; it wakes at least every `snooze_poll`): the
//!   portable way to notice a change.
//! * On Windows a watcher thread also waits for a change notification of
//!   the registry key Windows writes the time zone to, and wakes the
//!   reminder task right away ([`ZoneWatch`]).

/// The local zone the reminders were last planned in.
#[derive(Debug)]
pub(crate) struct ZoneTracker {
    zone: String,
}

impl ZoneTracker {
    /// Starts with the zone of now.
    pub(crate) fn new(zone: String) -> Self {
        Self { zone }
    }

    /// Whether `now` differs from the zone seen last (and remembers it).
    pub(crate) fn changed(&mut self, now: String) -> bool {
        if now == self.zone {
            return false;
        }
        self.zone = now;
        true
    }
}

/// A running watch of the OS time zone; stops when dropped.
pub(crate) struct ZoneWatch {
    #[cfg(windows)]
    // Held for its `Drop` (stops the watcher thread).
    _windows: Option<windows::Watch>,
}

impl ZoneWatch {
    /// Watches the OS time zone and calls `changed` (on another thread)
    /// after each change notification. Without an OS notification (not
    /// Windows, or the watch cannot start) nothing is called: the
    /// reminder task's [`ZoneTracker`] still notices the change.
    pub(crate) fn start(changed: impl Fn() + Send + 'static) -> Self {
        #[cfg(windows)]
        {
            Self {
                _windows: windows::Watch::start(changed),
            }
        }
        #[cfg(not(windows))]
        {
            drop(changed);
            Self {}
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::Arc;
    use std::thread::JoinHandle;

    use crate::registry_watch::{Event, arm, wait_any};

    /// The key Windows updates when the time zone changes.
    const TIME_ZONE_KEY: &str = r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation";
    /// Re-arm this often even without a notification (a missed one).
    const POLL_MS: u32 = 60_000;

    /// The watcher thread and its stop event.
    pub(super) struct Watch {
        stop: Arc<Event>,
        thread: Option<JoinHandle<()>>,
    }

    impl Watch {
        pub(super) fn start(changed: impl Fn() + Send + 'static) -> Option<Self> {
            let stop = Arc::new(Event::new()?);
            let notified = Event::new()?;
            let stopper = Arc::clone(&stop);
            let thread = std::thread::Builder::new()
                .name("nail-time-zone".into())
                .spawn(move || {
                    // A registry key stays on the thread that opened it.
                    let key = match windows_registry::LOCAL_MACHINE.open(TIME_ZONE_KEY) {
                        Ok(key) => key,
                        Err(err) => {
                            tracing::debug!(%err, "cannot watch the time zone key");
                            return;
                        }
                    };
                    loop {
                        arm(&key, &notified, true);
                        match wait_any(&[&stopper, &notified], POLL_MS) {
                            Some(0) => return,
                            Some(_) => changed(),
                            None => {}
                        }
                    }
                })
                .inspect_err(|err| tracing::warn!(%err, "cannot watch the time zone"))
                .ok()?;
            Some(Self {
                stop,
                thread: Some(thread),
            })
        }
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            self.stop.set();
            if let Some(thread) = self.thread.take() {
                drop(thread.join());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_zone_is_noticed_once() {
        let mut tracker = ZoneTracker::new("Europe/Berlin".into());
        assert!(!tracker.changed("Europe/Berlin".into()));
        assert!(tracker.changed("America/New_York".into()));
        assert!(!tracker.changed("America/New_York".into()));
        assert!(tracker.changed("Europe/Berlin".into()));
    }

    #[test]
    fn the_watch_starts_and_stops() {
        let watch = ZoneWatch::start(|| {});
        drop(watch);
    }
}
