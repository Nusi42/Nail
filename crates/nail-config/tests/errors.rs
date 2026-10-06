//! Every error of a file is reported at once (M1b WP24), in file order,
//! while the first-error functions keep their M1a behaviour.
#![cfg(test)]

use nail_config::{
    ConfigErrorKind, issues_event, parse_accounts, parse_accounts_all, parse_layout,
    parse_layout_all, parse_theme, parse_theme_all,
};

fn positions(errors: &[nail_config::ConfigError]) -> Vec<(u32, u32)> {
    errors.iter().map(|e| (e.line, e.column)).collect()
}

#[test]
fn every_bad_item_of_a_theme_is_reported() {
    let text = r##"theme "x" extends="default" {
    tokens {
        color "ok" "#fff"
        colour "bad" "#000"
        color "Bad Name" "#000"
    }
    part "message-list.row" {
        colour "red"
        color "blue"
        font-weight #true
    }
    icon "Archive" svg="M0"
    bogus
    sounds {
        mail.new "../x.wav"
    }
}
"##;
    let errors = parse_theme_all(text, "theme.kdl").unwrap_err();
    assert_eq!(
        positions(&errors),
        [
            (4, 9),
            (5, 15),
            (8, 9),
            (10, 21),
            (12, 10),
            (13, 5),
            (15, 18)
        ],
        "{errors:#?}"
    );
    assert!(errors.iter().all(|e| e.kind == ConfigErrorKind::Invalid));
    assert!(errors.iter().all(|e| e.file == "theme.kdl"));
    // The first-error function reports the first of them.
    assert_eq!(parse_theme(text, "theme.kdl").unwrap_err(), errors[0]);
}

#[test]
fn resolution_errors_are_all_reported_after_a_clean_read() {
    let text = r#"theme "x" extends="default" {
    tokens {
        color "a" "$color.nope"
        color "b" "$color.c"
        color "c" "$color.b"
    }
    part "message-list.row" {
        color "$color.missing"
        background "$color.also-missing"
    }
}
"#;
    let errors = parse_theme_all(text, "theme.kdl").unwrap_err();
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(errors.len(), 4, "{messages:#?}");
    assert!(messages[0].contains("$color.nope"));
    assert!(messages.iter().any(|m| m.contains("cycle")));
    assert!(messages.iter().any(|m| m.contains("$color.missing")));
    assert!(messages.iter().any(|m| m.contains("$color.also-missing")));
    // Sorted by position.
    let mut sorted = positions(&errors);
    sorted.sort_unstable();
    assert_eq!(positions(&errors), sorted);
}

#[test]
fn every_bad_item_of_a_layout_is_reported() {
    let text = r#"layout "x" extends="default" {
    window "main" {
        split {
            folder-tree nope=1
            message-list page-size="many"
            wat
        }
    }
    keys context="list" {
        bind "ctrl+" "mail.next"
        bind "j" "NotAnAction"
        bind "k" "mail.previous"
    }
    toolbar "reader" {
        item "mail.reply"
        gadget
    }
    menus {
        widget "x"
    }
}
"#;
    let errors = parse_layout_all(text, "layout.kdl").unwrap_err();
    assert_eq!(
        positions(&errors),
        [
            (4, 25),
            (5, 26),
            (6, 13),
            (10, 14),
            (11, 18),
            (16, 9),
            (19, 9)
        ],
        "{errors:#?}"
    );
    assert_eq!(parse_layout(text, "layout.kdl").unwrap_err(), errors[0]);
}

#[test]
fn layout_resolution_errors_are_all_reported() {
    let text = r#"layout "x" {
    window "side" {
        stack {
            toolbar toolbar="one"
            toolbar toolbar="two"
        }
    }
}
"#;
    let errors = parse_layout_all(text, "layout.kdl").unwrap_err();
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(errors.len(), 3, "{messages:#?}");
    assert!(messages[0].contains("no `main` window"));
    assert!(messages[1].contains("`one`") && messages[2].contains("`two`"));
}

#[test]
fn syntax_errors_come_from_the_parser() {
    let errors =
        parse_layout_all("layout \"x\" {\n  a 1.\n  b \"open\n", "layout.kdl").unwrap_err();
    assert!(!errors.is_empty(), "{errors:?}");
    assert!(errors.iter().all(|e| e.kind == ConfigErrorKind::Syntax));
}

#[test]
fn every_bad_node_of_accounts_is_reported() {
    let text = r#"account "a" email="a@example.com" {
    imap host="imap.example.com"
    smtp host="smtp.example.com"
    wat
}
account "b" email="not-an-address" {
    imap host="imap.example.com"
}
oauth-provider "x"
undo-send-delay 5
undo-send-delay 6
"#;
    let errors = parse_accounts_all(text, "accounts.kdl").unwrap_err();
    assert!(errors.len() >= 4, "{errors:#?}");
    let lines: Vec<u32> = errors.iter().map(|e| e.line).collect();
    assert!(lines.windows(2).all(|w| w[0] <= w[1]), "{lines:?}");
    assert_eq!(parse_accounts(text, "accounts.kdl").unwrap_err().line, 4);
}

#[test]
fn errors_become_one_event_per_file() {
    let errors =
        parse_theme_all("theme \"x\" extends=\"default\" { a; b; }", "theme.kdl").unwrap_err();
    let nail_api::Event::ConfigErrors {
        file,
        errors: issues,
    } = issues_event("theme.kdl", &errors)
    else {
        panic!("expected ConfigErrors");
    };
    assert_eq!(file, "theme.kdl");
    assert_eq!(issues.len(), 2);
    assert_eq!((issues[0].line, issues[0].column), (1, 31));
    // An error located in another file names that file.
    let mut foreign = errors[0].clone();
    foreign.file = "defaults/theme.kdl".into();
    let nail_api::Event::ConfigErrors { errors: issues, .. } =
        issues_event("theme.kdl", std::slice::from_ref(&foreign))
    else {
        panic!("expected ConfigErrors");
    };
    assert!(issues[0].message.starts_with("defaults/theme.kdl: "));
    assert_eq!(
        issues_event("layout.kdl", &[]),
        nail_api::Event::ConfigErrors {
            file: "layout.kdl".into(),
            errors: vec![]
        }
    );
}
