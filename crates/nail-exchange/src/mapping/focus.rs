//! Focused Inbox (ADR 0072): the server's `inferenceClassification` of
//! inbox messages becomes [`ExchangeMessageInfo::inference`], a filterable
//! property (`focus:focused` / `focus:other` in search and in the rules'
//! message view, WP95). Messages outside the Inbox carry no
//! classification, so `focus:` never matches them.

use nail_api::exchange::{ExchangeMessageInfo, ExchangeWellKnownFolder, InferenceClassification};

/// The query field (`focus:focused`).
pub const QUERY_FIELD: &str = "focus";

/// The classification a message keeps in the store: the server's, for
/// messages of the Inbox only (`folder` = the folder's well-known name).
pub fn stored_inference(
    folder: Option<ExchangeWellKnownFolder>,
    server: Option<InferenceClassification>,
) -> Option<InferenceClassification> {
    if folder == Some(ExchangeWellKnownFolder::Inbox) {
        server
    } else {
        None
    }
}

/// The classification a `focus:` query value asks for (`None` = not a
/// valid value: the query parser reports it).
pub fn parse_query_value(value: &str) -> Option<InferenceClassification> {
    InferenceClassification::parse(value.trim().trim_matches('"'))
}

/// Whether a message matches `focus:<wanted>`.
pub fn matches(info: Option<&ExchangeMessageInfo>, wanted: InferenceClassification) -> bool {
    info.and_then(|i| i.inference) == Some(wanted)
}

/// The messages (by index) a `ClassifyFocused` request actually changes:
/// inbox messages with another (or no) classification.
pub fn needs_move(
    messages: &[Option<&ExchangeMessageInfo>],
    target: InferenceClassification,
) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, info)| info.is_some_and(|i| i.inference != Some(target)))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(inference: Option<InferenceClassification>) -> ExchangeMessageInfo {
        ExchangeMessageInfo {
            item_id: "AAMk".into(),
            conversation_id: None,
            inference,
            mailbox: None,
        }
    }

    #[test]
    fn only_inbox_messages_are_classified() {
        let focused = Some(InferenceClassification::Focused);
        assert_eq!(
            stored_inference(Some(ExchangeWellKnownFolder::Inbox), focused),
            focused
        );
        assert_eq!(
            stored_inference(Some(ExchangeWellKnownFolder::Archive), focused),
            None
        );
        assert_eq!(stored_inference(None, focused), None);
    }

    #[test]
    fn queries() {
        assert_eq!(
            parse_query_value("\"Other\""),
            Some(InferenceClassification::Other)
        );
        assert_eq!(parse_query_value("maybe"), None);
        let focused = info(Some(InferenceClassification::Focused));
        assert!(matches(Some(&focused), InferenceClassification::Focused));
        assert!(!matches(Some(&focused), InferenceClassification::Other));
        assert!(!matches(Some(&info(None)), InferenceClassification::Other));
        assert!(!matches(None, InferenceClassification::Focused));
    }

    #[test]
    fn moves_skip_unchanged_messages() {
        let focused = info(Some(InferenceClassification::Focused));
        let other = info(Some(InferenceClassification::Other));
        let unclassified = info(None);
        assert_eq!(
            needs_move(
                &[Some(&focused), Some(&other), None, Some(&unclassified)],
                InferenceClassification::Focused
            ),
            [1, 3]
        );
    }
}
