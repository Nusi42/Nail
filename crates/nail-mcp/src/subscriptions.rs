//! Resource subscriptions: who wants `notifications/resources/updated` for
//! which URI.
//!
//! Two protocol generations are served: legacy `resources/subscribe` on a
//! session (the notification goes through the session's peer) and
//! `subscriptions/listen` streams (protocol 2026-07-28, through the
//! stream's sink). Grants are checked again for every notification, so a
//! revoked client or a narrowed grant stops notifications at once.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use nail_api::McpClientId;
use rmcp::service::{Peer, RoleServer, SubscriptionSink};

/// Where a subscriber's notifications go.
#[derive(Clone)]
pub(crate) enum Sink {
    /// A legacy session.
    Session(Peer<RoleServer>),
    /// A `subscriptions/listen` stream.
    Listen(SubscriptionSink),
}

impl Sink {
    /// Sends `notifications/resources/updated`; `false` when the peer is gone.
    pub(crate) async fn updated(&self, uri: &str) -> bool {
        match self {
            Self::Session(peer) => {
                let param = rmcp::model::ResourceUpdatedNotificationParam::new(uri);
                peer.notify_resource_updated(param).await.is_ok()
            }
            Self::Listen(sink) => sink.notify_resource_updated(uri).await.is_ok(),
        }
    }
}

struct Subscriber {
    client: McpClientId,
    sink: Sink,
    uris: BTreeSet<String>,
}

/// A subscriber due for a notification.
pub(crate) struct Due {
    pub key: u64,
    pub client: McpClientId,
    pub sink: Sink,
}

/// Every active subscription.
#[derive(Default)]
pub(crate) struct Hub {
    next: AtomicU64,
    subscribers: Mutex<HashMap<u64, Subscriber>>,
}

impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Subscriber>> {
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new key for a session or a listen stream.
    pub(crate) fn key(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// Adds `uri` to a legacy session's subscriptions.
    pub(crate) fn subscribe(
        &self,
        key: u64,
        client: &McpClientId,
        peer: &Peer<RoleServer>,
        uri: &str,
    ) {
        let mut subs = self.lock();
        let entry = subs.entry(key).or_insert_with(|| Subscriber {
            client: client.clone(),
            sink: Sink::Session(peer.clone()),
            uris: BTreeSet::new(),
        });
        entry.uris.insert(uri.to_owned());
    }

    /// Removes `uri` from a legacy session's subscriptions.
    pub(crate) fn unsubscribe(&self, key: u64, uri: &str) {
        let mut subs = self.lock();
        if let Some(entry) = subs.get_mut(&key) {
            entry.uris.remove(uri);
            if entry.uris.is_empty() {
                subs.remove(&key);
            }
        }
    }

    /// Registers a listen stream for `uris`.
    pub(crate) fn listen(
        &self,
        key: u64,
        client: &McpClientId,
        sink: SubscriptionSink,
        uris: BTreeSet<String>,
    ) {
        self.lock().insert(
            key,
            Subscriber {
                client: client.clone(),
                sink: Sink::Listen(sink),
                uris,
            },
        );
    }

    /// Forgets a session or stream.
    pub(crate) fn remove(&self, key: u64) {
        self.lock().remove(&key);
    }

    /// Subscribers of `uri`.
    pub(crate) fn due(&self, uri: &str) -> Vec<Due> {
        self.lock()
            .iter()
            .filter(|(_, s)| s.uris.contains(uri))
            .map(|(key, s)| Due {
                key: *key,
                client: s.client.clone(),
                sink: s.sink.clone(),
            })
            .collect()
    }

    /// Number of subscribers (tests, diagnostics).
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }
}

/// Removes a legacy session's subscriptions when the session's handler is
/// dropped.
pub(crate) struct SessionGuard {
    pub key: u64,
    pub hub: std::sync::Weak<Hub>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.remove(self.key);
        }
    }
}
