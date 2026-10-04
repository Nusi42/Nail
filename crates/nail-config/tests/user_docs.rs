//! Every KDL example in the user docs (`userDocs/en/*-reference.md`, the
//! getting-started guide, the accounts, customization and Markdown mail
//! guides) must be valid. Complete files are parsed as they are; fragments
//! are wrapped in a file that extends the default.
//! Examples containing `…` are outlines and are skipped.
#![cfg(test)]

use std::path::PathBuf;

fn kdl_blocks(file: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../userDocs/en")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        match (&mut current, line.trim_start()) {
            (None, "```kdl") => current = Some(String::new()),
            (Some(block), "```") => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            (Some(block), _) => {
                block.push_str(line);
                block.push('\n');
            }
            (None, _) => {}
        }
    }
    assert!(current.is_none(), "unclosed code block in {file}");
    blocks.into_iter().filter(|b| !b.contains('…')).collect()
}

#[test]
fn theme_reference_examples_are_valid() {
    let blocks = kdl_blocks("theme-reference.md");
    assert!(blocks.len() >= 8, "found only {} examples", blocks.len());
    for block in blocks {
        let text = if block.trim_start().starts_with("theme ") {
            block.clone()
        } else {
            format!("theme \"example\" extends=\"default\" {{\n{block}}}\n")
        };
        if let Err(e) = nail_config::parse_theme(&text, "theme.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn layout_reference_examples_are_valid() {
    const SECTIONS: &[&str] = &[
        "breakpoint",
        "window",
        "actions",
        "action",
        "keys",
        "toolbar",
        "menu",
        "views",
        "rail",
        "settings-page",
    ];
    let blocks = kdl_blocks("layout-reference.md");
    assert!(blocks.len() >= 8, "found only {} examples", blocks.len());
    for block in blocks {
        let first = block.split_whitespace().next().unwrap_or_default();
        let text = if first == "layout" {
            block.clone()
        } else if SECTIONS.contains(&first) {
            format!("layout \"example\" extends=\"default\" {{\n{block}}}\n")
        } else {
            format!(
                "layout \"example\" extends=\"default\" {{\nwindow \"main\" {{\n{block}}}\n}}\n"
            )
        };
        if let Err(e) = nail_config::parse_layout(&text, "layout.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn getting_started_examples_are_valid() {
    let blocks = kdl_blocks("getting-started.md");
    assert!(blocks.len() >= 2, "found only {} examples", blocks.len());
    for block in blocks {
        let result = if block.trim_start().starts_with("theme ") {
            nail_config::parse_theme(&block, "theme.kdl").map(|_| ())
        } else if block.trim_start().starts_with("layout ") {
            nail_config::parse_layout(&block, "layout.kdl").map(|_| ())
        } else {
            panic!("getting-started examples are complete files:\n{block}");
        };
        if let Err(e) = result {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn accounts_reference_examples_are_valid() {
    let blocks = kdl_blocks("accounts-reference.md");
    assert!(blocks.len() >= 8, "found only {} examples", blocks.len());
    for block in blocks {
        if let Err(e) = nail_config::parse_accounts(&block, "accounts.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn accounts_guide_examples_are_valid() {
    let blocks = kdl_blocks("accounts.md");
    assert!(!blocks.is_empty());
    for block in blocks {
        if let Err(e) = nail_config::parse_accounts(&block, "accounts.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn customization_guide_examples_are_valid() {
    let blocks = kdl_blocks("customization.md");
    assert!(blocks.len() >= 3, "found only {} examples", blocks.len());
    for block in blocks {
        let text = format!("layout \"example\" extends=\"default\" {{\n{block}}}\n");
        if let Err(e) = nail_config::parse_layout(&text, "layout.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn markdown_mail_examples_are_valid() {
    let blocks = kdl_blocks("markdown-mail.md");
    assert!(!blocks.is_empty());
    for block in blocks {
        let config = nail_config::parse_accounts(&block, "accounts.kdl")
            .unwrap_or_else(|e| panic!("{e}\nin example:\n{block}"));
        // The style is read as written.
        let identity = &config.accounts[0].identities[0];
        assert!(identity.mail_style.is_some(), "{block}");
    }
}

#[test]
fn accessibility_guide_examples_are_valid() {
    let blocks = kdl_blocks("accessibility.md");
    assert!(!blocks.is_empty(), "the guide has a theme example");
    for block in blocks {
        if let Err(e) = nail_config::parse_theme(&block, "theme.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}

#[test]
fn appearance_guide_examples_are_valid() {
    let blocks = kdl_blocks("appearance.md");
    assert!(
        blocks.len() >= 2,
        "the guide has a settings and a theme example"
    );
    for block in blocks {
        if let Err(e) = nail_config::parse_theme(&block, "theme.kdl") {
            panic!("{e}\nin example:\n{block}");
        }
    }
}
