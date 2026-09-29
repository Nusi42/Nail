//! Filter rules (M1b WP22; stub after WP20).
//!
//! Expected shape (ADR 0022): evaluation of [`nail_api::rules::Rule`]s —
//! triggers, nested all / any / not conditions (headers, [`regex`], the
//! search query language, attachments, size, date, flags / tags, account,
//! authentication results, list-id) and actions turned into queued
//! operations; dry runs ("test rule") and runs that can be undone as one
//! unit (the store's `rule_runs` / `rule_run_actions`).

#![warn(missing_docs)]

#[cfg(test)]
mod tests {
    use nail_api::rules::{Condition, TextMatch, TextOp};

    #[test]
    fn regex_and_the_rules_model_are_available() {
        let re = regex::RegexBuilder::new(r"^\[list\]")
            .case_insensitive(true)
            .build()
            .unwrap();
        assert!(re.is_match("[LIST] hello"));
        let condition = Condition::Header {
            name: "Subject".into(),
            matches: Some(TextMatch {
                op: TextOp::Regex,
                value: r"^\[list\]".into(),
                case_sensitive: false,
            }),
        };
        assert!(matches!(condition, Condition::Header { .. }));
    }
}
