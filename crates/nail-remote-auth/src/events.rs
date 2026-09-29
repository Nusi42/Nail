//! Where this crate's events go. The core's broadcast implements
//! [`EventSink`] (WP45); tests use [`RecordingSink`].

use std::fmt;
use std::sync::{Mutex, PoisonError};

use nail_api::Event;

/// Receives the events this crate raises: `RemoteSecurityAlert` (new
/// device, lockout), `RemoteSessionEnded` and `RemoteAccessChanged`.
pub trait EventSink: Send + Sync + fmt::Debug {
    /// Delivers one event (must not block).
    fn emit(&self, event: Event);
}

/// Drops every event.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _event: Event) {}
}

/// Keeps every event (tests).
#[derive(Debug, Default)]
pub struct RecordingSink(Mutex<Vec<Event>>);

impl RecordingSink {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the recorded events.
    pub fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: Event) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_sink_keeps_events_in_order() {
        let sink = RecordingSink::new();
        sink.emit(Event::RemoteAccessChanged);
        sink.emit(Event::RemoteContentPolicyChanged);
        assert_eq!(
            sink.take(),
            [
                Event::RemoteAccessChanged,
                Event::RemoteContentPolicyChanged
            ]
        );
        assert!(sink.take().is_empty());
        NoEvents.emit(Event::RemoteAccessChanged);
    }
}
