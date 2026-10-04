//! Property tests for the query language parser.
#![cfg(test)]

use nail_api::search::utf16_offset;
use nail_search::parse_query;
use nail_search::query::{
    AgeUnit, DateValue, FlagTerm, Query, SizeOp, Span, Term, TermKind, TextField, Value,
};
use proptest::prelude::*;

fn word() -> impl Strategy<Value = String> {
    // Bare words: letters, digits and some punctuation, never an operator.
    "[a-zA-Z0-9äöüß@._+][a-zA-Z0-9äöüß@._+-]{0,8}".prop_filter("operators are not words", |w| {
        !matches!(w.as_str(), "AND" | "OR" | "NOT")
    })
}

fn any_text() -> impl Strategy<Value = String> {
    // Anything, including quotes, backslashes, parentheses and unicode.
    proptest::string::string_regex("[ -~äöü😀\"\\\\()]{0,12}").unwrap()
}

fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        word().prop_map(|text| Value {
            text,
            quoted: false
        }),
        any_text().prop_map(|text| Value { text, quoted: true }),
    ]
}

fn text_field() -> impl Strategy<Value = TextField> {
    prop_oneof![
        Just(TextField::Any),
        Just(TextField::From),
        Just(TextField::To),
        Just(TextField::Cc),
        Just(TextField::Subject),
        Just(TextField::Body),
        Just(TextField::Filename),
    ]
}

fn date() -> impl Strategy<Value = DateValue> {
    prop_oneof![
        (1970i32..2100, 1u8..=12, 1u8..=28).prop_map(|(year, month, day)| DateValue::Day {
            year,
            month,
            day
        }),
        (
            0u32..1000,
            prop_oneof![
                Just(AgeUnit::Hours),
                Just(AgeUnit::Days),
                Just(AgeUnit::Weeks),
                Just(AgeUnit::Months),
                Just(AgeUnit::Years)
            ]
        )
            .prop_map(|(amount, unit)| DateValue::Ago { amount, unit }),
    ]
}

fn term() -> impl Strategy<Value = Query> {
    let kind = prop_oneof![
        (text_field(), value()).prop_map(|(field, v)| TermKind::Text {
            field,
            value: v.text,
            quoted: v.quoted
        }),
        prop_oneof![
            Just(FlagTerm::Unread),
            Just(FlagTerm::Read),
            Just(FlagTerm::Flagged),
            Just(FlagTerm::Unflagged),
            Just(FlagTerm::Answered),
            Just(FlagTerm::Unanswered),
            Just(FlagTerm::Draft)
        ]
        .prop_map(TermKind::Is),
        Just(TermKind::HasAttachment),
        value().prop_map(TermKind::Tag),
        value().prop_map(TermKind::Account),
        value().prop_map(TermKind::Folder),
        date().prop_map(TermKind::Before),
        date().prop_map(TermKind::After),
        (
            prop_oneof![
                Just(SizeOp::Greater),
                Just(SizeOp::GreaterOrEqual),
                Just(SizeOp::Less),
                Just(SizeOp::LessOrEqual)
            ],
            0u64..u64::from(u32::MAX)
        )
            .prop_map(|(op, n)| TermKind::Size(op, n)),
    ];
    kind.prop_map(|kind| {
        // Field values must not be empty unless quoted.
        let kind = match kind {
            TermKind::Text {
                field,
                value,
                quoted: false,
            } if value.is_empty() => TermKind::Text {
                field,
                value: "x".into(),
                quoted: false,
            },
            other => other,
        };
        Query::Term(Term {
            kind,
            span: Span::default(),
        })
    })
}

fn query() -> impl Strategy<Value = Query> {
    term().prop_recursive(4, 24, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(|q| Query::Not(Box::new(q))),
            prop::collection::vec(inner.clone(), 2..4).prop_map(Query::And),
            prop::collection::vec(inner, 2..4).prop_map(Query::Or),
        ]
    })
}

/// What the parser produces for a query built by hand: nested ANDs / ORs
/// flatten.
fn normalize(q: Query) -> Query {
    match q {
        Query::And(items) => Query::And(flatten(items, false)),
        Query::Or(items) => Query::Or(flatten(items, true)),
        Query::Not(inner) => Query::Not(Box::new(normalize(*inner))),
        other => other,
    }
}

fn flatten(items: Vec<Query>, or: bool) -> Vec<Query> {
    let mut out = Vec::new();
    for item in items.into_iter().map(normalize) {
        match item {
            Query::Or(inner) if or => out.extend(inner),
            Query::And(inner) if !or => out.extend(inner),
            other => out.push(other),
        }
    }
    out
}

proptest! {
    // No regression files in the repo; failures print their minimal input.
    #![proptest_config(ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// The parser never panics, and error positions are inside the text
    /// (in UTF-16 units) and ordered.
    #[test]
    fn never_panics_and_errors_are_in_bounds(text in any_text()) {
        let len = utf16_offset(&text, text.len());
        if let Err(e) = parse_query(&text) {
            prop_assert!(e.start <= e.end, "{e:?}");
            prop_assert!(e.end <= len, "{e:?} for {text:?}");
            prop_assert!(!e.message.is_empty());
        }
    }

    /// Arbitrary token soup (operators, fields, parentheses, quotes) never
    /// panics either.
    #[test]
    fn token_soup(parts in prop::collection::vec(prop_oneof![
        Just("AND".to_owned()), Just("OR".to_owned()), Just("NOT".to_owned()),
        Just("(".to_owned()), Just(")".to_owned()), Just("-".to_owned()),
        Just("\"".to_owned()), Just("from:".to_owned()), Just("is:".to_owned()),
        Just("size>".to_owned()), Just("before:".to_owned()), word(),
    ], 0..10), spaces in prop::collection::vec(any::<bool>(), 0..10)) {
        let mut text = String::new();
        for (i, p) in parts.iter().enumerate() {
            text.push_str(p);
            if spaces.get(i).copied().unwrap_or(true) {
                text.push(' ');
            }
        }
        let len = utf16_offset(&text, text.len());
        match parse_query(&text) {
            Ok(q) => {
                // Whatever parses prints back to an equal query.
                let printed = q.to_string();
                let again = parse_query(&printed).map(|q| q.without_spans());
                prop_assert_eq!(again, Ok(q.without_spans()), "{} -> {}", text, printed);
            }
            Err(e) => prop_assert!(e.start <= e.end && e.end <= len, "{e:?}"),
        }
    }

    /// Printing an AST and parsing it back gives the same AST.
    #[test]
    fn canonical_form_round_trips(q in query()) {
        let expected = normalize(q);
        let printed = expected.to_string();
        let parsed = parse_query(&printed)
            .map_err(|e| TestCaseError::fail(format!("{printed:?}: {e:?}")))?;
        prop_assert_eq!(parsed.without_spans(), expected, "printed as {:?}", printed);
    }

    /// Term spans point at the term's own text.
    #[test]
    fn spans_point_at_terms(q in query()) {
        let printed = normalize(q).to_string();
        let parsed = parse_query(&printed).unwrap();
        for term in parsed.terms() {
            let slice = &printed[term.span.start..term.span.end];
            let reparsed = parse_query(slice).unwrap().without_spans();
            prop_assert_eq!(
                reparsed,
                Query::Term(Term { kind: term.kind.clone(), span: Span::default() }),
                "{:?} in {:?}", slice, printed
            );
        }
    }
}
