//! Every KDL example in `userDocs/en/rules.md` is a valid `rules.kdl`, and
//! every inline example in its condition and action tables parses.
#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;

use common::{kdl_blocks, user_doc};

fn doc() -> String {
    user_doc("rules.md")
}

fn check(text: &str) {
    if let Err(errors) = nail_config::rules::parse_rules(text, "rules.md") {
        panic!(
            "{text}\n{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn code_blocks_are_valid() {
    let blocks = kdl_blocks(&doc());
    assert!(blocks.len() >= 3, "found only {} examples", blocks.len());
    for block in blocks {
        check(&block);
    }
}

#[test]
fn table_examples_are_valid() {
    let text = doc();
    let section = |title: &str| {
        let start = text.find(title).unwrap();
        let rest = &text[start..];
        let end = rest[title.len()..]
            .find("\n#")
            .map_or(rest.len(), |e| e + title.len());
        rest[..end].to_owned()
    };
    let snippets = |section: &str| -> Vec<String> {
        section
            .lines()
            .filter(|l| l.starts_with('|') && !l.starts_with("| ---"))
            .flat_map(|l| {
                l.split('`')
                    .enumerate()
                    .filter(|(i, _)| i % 2 == 1)
                    .map(|(_, s)| s.to_owned())
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let conditions = snippets(&section("### Conditions"));
    assert!(conditions.len() > 20);
    let whole = |s: &&String| {
        (!s.contains('…') && s.contains(' ')) || *s == "attachment" || *s == "list-id"
    };
    for snippet in conditions.iter().filter(whole) {
        check(&format!(
            "rule \"x\" {{\n    when {{\n        {snippet}\n    }}\n}}\n"
        ));
    }
    let actions = snippets(&section("### Actions"));
    assert!(actions.len() > 15);
    for snippet in actions.iter().filter(|s| !s.contains('…')) {
        check(&format!(
            "rule \"x\" {{\n    then {{\n        {snippet}\n    }}\n}}\n"
        ));
    }
}
