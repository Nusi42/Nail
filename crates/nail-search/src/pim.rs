//! PIM search (M3a WP73; contract ADR 0058): Tantivy fields for events,
//! tasks and contacts behind `nail_api::Request::SearchPim` and
//! `SearchContacts`.
//!
//! Slot created by WP70; WP73 fills it (its own index or document kind next
//! to the mail index, sealed like the mail index when encryption at rest
//! is on).

/// The item kinds PIM search covers.
pub const PIM_KINDS: [nail_api::pim::PimItemKind; 3] = [
    nail_api::pim::PimItemKind::Event,
    nail_api::pim::PimItemKind::Task,
    nail_api::pim::PimItemKind::Contact,
];

#[cfg(test)]
mod tests {
    #[test]
    fn covers_events_tasks_and_contacts() {
        assert_eq!(super::PIM_KINDS.len(), 3);
    }
}
