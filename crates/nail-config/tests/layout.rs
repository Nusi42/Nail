//! Layout parsing, merging and error reporting.
#![cfg(test)]

use nail_api::{ActionDef, LayoutNode, MenuItem, Orientation, ResolvedLayout, ToolbarItem};
use nail_config::{ConfigError, parse_layout};
use pretty_assertions::assert_eq;
use serde_json::json;

fn layout(text: &str) -> ResolvedLayout {
    parse_layout(text, "layout.kdl").unwrap_or_else(|e| panic!("{e}"))
}

fn error(text: &str) -> ConfigError {
    parse_layout(text, "layout.kdl").expect_err("expected an error")
}

#[track_caller]
fn assert_error(text: &str, line: u32, column: u32, needle: &str) {
    let err = error(text);
    assert!(err.message.contains(needle), "{err}");
    assert_eq!((err.line, err.column), (line, column), "{err}");
}

fn widgets(node: &LayoutNode) -> Vec<(String, serde_json::Map<String, serde_json::Value>)> {
    let mut out = Vec::new();
    node.walk(&mut |n| {
        if let LayoutNode::Widget { kind, options, .. } = n {
            out.push((kind.clone(), options.clone()));
        }
    });
    out
}

#[test]
fn design_doc_example_three_pane() {
    // The layout example from docs/design.md ("UI customization").
    let l = layout(
        r#"
layout "Three pane" extends="default" {
    breakpoint min-width=900 {
        window "main" {
            split "horizontal" sizes="240px 1fr 1.4fr" {
                stack {
                    account-switcher
                    folder-tree show-unread=#true show-account-badges=#true
                    mini-month
                }
                message-list group-by="date" threads="collapsed" {
                    row "{from.name | bold-if-unread}  {date | relative}"
                    row "{subject}  {tags | chips}  {account | badge}"
                }
                message-view headers="from to cc date auth crypto"
            }
        }
    }
    breakpoint max-width=899 {
        window "main" { tabs { folder-tree; message-list; message-view } }
    }
    keys context="list" {
        bind "e"   "mail.archive"
        bind "g i" "nav.goto-folder \"Inbox\""
        bind "ctrl+shift+m" "view.toggle \"mini-month\""
    }
    toolbar "reader" { item "mail.reply"; item "mail.reply-all"; item "mail.forward"; spacer; item "mail.archive" }
}
"#,
    );
    assert_eq!(l.name, "Three pane");
    let main = &l.windows["main"];
    assert_eq!(
        main.len(),
        2,
        "the file's breakpoints replace the default ones"
    );
    assert_eq!((main[0].min_width, main[0].max_width), (Some(900), None));
    assert_eq!((main[1].min_width, main[1].max_width), (None, Some(899)));

    let LayoutNode::Split {
        orientation,
        sizes,
        children,
        resizable,
        ..
    } = &main[0].root
    else {
        panic!("expected a split");
    };
    assert_eq!(*orientation, Orientation::Horizontal);
    assert_eq!(sizes, &["240px", "1fr", "1.4fr"]);
    assert!(*resizable);
    assert_eq!(children.len(), 3);
    let kinds: Vec<_> = widgets(&main[0].root).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        kinds,
        [
            "account-switcher",
            "folder-tree",
            "mini-month",
            "message-list",
            "message-view"
        ]
    );
    let (_, folder_opts) = &widgets(&main[0].root)[1];
    assert_eq!(folder_opts["show-unread"], json!(true));
    assert_eq!(folder_opts["show-account-badges"], json!(true));
    let LayoutNode::Widget { rows, options, .. } = &children[1] else {
        panic!("expected message-list");
    };
    assert_eq!(rows[0], "{from.name | bold-if-unread}  {date | relative}");
    assert_eq!(options["group-by"], json!("date"));

    let LayoutNode::Tabs {
        children, active, ..
    } = &main[1].root
    else {
        panic!("expected tabs");
    };
    assert_eq!((children.len(), *active), (3, 0));

    // Keys are added to the defaults.
    let find = |ctx: &str, key: &str| {
        l.keys
            .iter()
            .find(|k| k.context == ctx && k.key == key)
            .unwrap_or_else(|| panic!("{ctx} {key}"))
    };
    assert_eq!(
        find("list", "j").action,
        "mail.next",
        "default bindings stay"
    );
    assert_eq!(find("list", "e").action, "mail.archive");
    let goto = find("list", "g i");
    assert_eq!(
        (goto.action.as_str(), &goto.args),
        ("nav.goto-folder", &vec![json!("Inbox")])
    );
    assert_eq!(find("list", "ctrl+shift+m").args, [json!("mini-month")]);

    // The toolbar replaces the default "reader" toolbar.
    assert_eq!(l.toolbars["reader"].len(), 5);
    assert!(matches!(l.toolbars["reader"][3], ToolbarItem::Spacer));

    // Actions used without metadata get an empty entry; defaults stay.
    assert_eq!(l.actions["nav.goto-folder"], ActionDef::default());
    assert_eq!(l.actions["mail.reply"].icon.as_deref(), Some("reply"));
    assert!(l.menus.contains_key("context:message-list.row"));
}

#[test]
fn every_section_parses() {
    let l = layout(
        r#"
layout "All" {
    window "main" {
        split "vertical" id="outer" sizes="1fr 200px" min-sizes="none 100px" max-sizes="none none" collapsible="false true" resizable=#false {
            text id="hello" label="Hello" text="@app.name"
            stack sizes="auto 1fr" {
                toolbar toolbar="main"
                plugin-panel "acme.weather"
            }
        }
    }
    window "composer" { spacer }
    actions {
        action "mail.reply" label="Reply!" icon="reply"
        action "goto.inbox" alias-of="nav.goto-folder" "Inbox" label="@nav.inbox"
    }
    action "sync.now"
    keys { bind "ctrl+r" "mail.reply"; bind "x" "view.toggle" "sidebar" 2; }
    toolbars {
        toolbar "main" {
            item "mail.reply" label="Answer" icon="reply"
            separator
            item "goto.inbox"
        }
    }
    menus {
        menu "app" {
            submenu "@menu.mail" {
                item "mail.reply"
                separator
            }
        }
    }
    menu "context:folder-tree.item" { item "sync.now"; }
}
"#,
    );
    assert_eq!(l.windows.keys().collect::<Vec<_>>(), ["main", "composer"]);
    let bp = &l.windows["main"][0];
    assert_eq!((bp.min_width, bp.max_width), (None, None));
    let LayoutNode::Split {
        id,
        orientation,
        min_sizes,
        max_sizes,
        collapsible,
        resizable,
        ..
    } = &bp.root
    else {
        panic!("split");
    };
    assert_eq!(id.as_deref(), Some("outer"));
    assert_eq!(*orientation, Orientation::Vertical);
    assert_eq!(min_sizes, &[None, Some("100px".to_owned())]);
    assert_eq!(max_sizes, &[None, None]);
    assert_eq!(collapsible, &[false, true]);
    assert!(!*resizable);
    let w = widgets(&bp.root);
    assert_eq!(w[0].1["text"], json!("@app.name"));
    assert_eq!(w[2].0, "plugin-panel");
    assert_eq!(w[2].1["panel"], json!("acme.weather"));

    assert_eq!(l.actions["mail.reply"].label.as_deref(), Some("Reply!"));
    let alias = &l.actions["goto.inbox"];
    assert_eq!(alias.alias_of.as_deref(), Some("nav.goto-folder"));
    assert_eq!(alias.args, [json!("Inbox")]);
    assert_eq!(l.actions["sync.now"], ActionDef::default());
    assert!(
        l.actions.contains_key("view.toggle"),
        "used actions are listed"
    );

    assert_eq!(l.keys[0].context, "global");
    assert_eq!(l.keys[1].args, [json!("sidebar"), json!(2)]);
    assert_eq!(
        l.toolbars["main"][0],
        ToolbarItem::Action {
            item: nail_api::ActionItem {
                label: Some("Answer".into()),
                icon: Some("reply".into()),
                ..nail_api::ActionItem::new("mail.reply")
            },
            rank: None,
            display: None,
        }
    );
    let MenuItem::Submenu { label, items } = &l.menus["app"][0] else {
        panic!("submenu");
    };
    assert_eq!((label.as_str(), items.len()), ("@menu.mail", 2));
    assert!(l.menus.contains_key("context:folder-tree.item"));
}

#[test]
fn merge_semantics() {
    let l = layout(
        r#"
layout "Mine" extends="default" {
    window "composer" { spacer }
    action "mail.reply" icon="forward"
    keys context="list" {
        bind "j" "mail.previous"
        unbind "k"
    }
    keys { unbind "never-bound"; }
    toolbar "extra" { item "mail.reply"; }
}
"#,
    );
    // Windows not mentioned keep their default breakpoints.
    assert_eq!(l.windows["mail"].len(), 2);
    assert_eq!(l.windows["main"].len(), 1);
    assert!(l.windows.contains_key("composer"));
    // Actions merge field by field.
    assert_eq!(l.actions["mail.reply"].icon.as_deref(), Some("forward"));
    assert_eq!(
        l.actions["mail.reply"].label.as_deref(),
        Some("@mail.action.reply")
    );
    // A rebinding replaces the default in place; unbind removes it.
    let list: Vec<_> = l.keys.iter().filter(|k| k.context == "list").collect();
    assert_eq!(list[0].key, "j");
    assert_eq!(list[0].action, "mail.previous");
    assert!(!list.iter().any(|k| k.key == "k"));
    assert_eq!(l.keys.len(), nail_config::default_layout().keys.len() - 1);
    // Toolbars and menus are added or replaced by name.
    assert!(l.toolbars.contains_key("reader") && l.toolbars.contains_key("extra"));
}

#[test]
fn structural_errors() {
    assert_error("", 1, 1, "file is empty");
    assert_error("theme \"x\"", 1, 1, "unexpected `theme`");
    assert_error(
        "layout \"x\" extends=\"mine\"",
        1,
        12,
        "unknown base `mine`",
    );
    assert_error(
        "layout \"x\" {\n  panes {}\n}",
        2,
        3,
        "unknown layout section `panes`",
    );
    assert_error(
        "layout \"x\" {\n  window \"side\" { spacer }\n}",
        1,
        8,
        "no `main` window",
    );
    assert_error(
        "layout \"x\" {\n  window \"main\" { spacer; spacer }\n}",
        2,
        3,
        "exactly one root node",
    );
    assert_error(
        "layout \"x\" {\n  window \"main\" {}\n}",
        2,
        3,
        "exactly one root node",
    );
    assert_error(
        "layout \"x\" {\n  breakpoint min-width=9 max-width=1 { window \"main\" { spacer; }; }\n}",
        2,
        3,
        "larger than max-width",
    );
    assert_error(
        "layout \"x\" {\n  breakpoint min-width=-5 { window \"main\" { spacer; }; }\n}",
        2,
        14,
        "non-negative whole number",
    );
    assert_error(
        "layout \"x\" {\n  breakpoint { spacer; }\n}",
        2,
        16,
        "may only contain `window`",
    );
    assert_error(
        "layout \"x\" {\n  breakpoint\n}",
        2,
        3,
        "at least one `window`",
    );
}

#[test]
fn node_errors() {
    let wrap = |node: &str| format!("layout \"x\" {{\n  window \"main\" {{\n    {node}\n  }}\n}}");
    let check =
        |node: &str, column: u32, needle: &str| assert_error(&wrap(node), 3, column, needle);
    check("sidebar", 5, "unknown widget or container `sidebar`");
    // The pre-M2d placeholder for plug-in widgets points to `plugin-panel`.
    check(
        "plugin.acme.weather",
        5,
        "place plug-in panels as `plugin-panel \"acme.weather\"`",
    );
    check(
        "grid { spacer; }",
        5,
        "`grid` container is not supported yet",
    );
    check(
        "split \"diagonal\" { spacer; }",
        11,
        "unknown orientation `diagonal`",
    );
    check(
        "split sizes=\"1fr\" { spacer; spacer; }",
        11,
        "`sizes` lists 1 values but `split` has 2 children",
    );
    check(
        "split collapsible=\"yes no\" { spacer; spacer; }",
        5,
        "collapsible must list",
    );
    check(
        "split resizable=\"no\" { spacer; }",
        11,
        "resizable must be #true or #false",
    );
    check("stack", 5, "needs at least one child");
    check("tabs active=2 { spacer; spacer; }", 5, "only 2 children");
    check(
        "folder-tree show-unread=\"yes\"",
        17,
        "option `show-unread` must be #true or #false",
    );
    check(
        "folder-tree colour=\"red\"",
        17,
        "unknown option `colour` for `folder-tree`",
    );
    check("spacer size=3", 12, "it has no options");
    check(
        "message-list threads=\"sometimes\"",
        18,
        "must be one of off, collapsed, expandable, expanded",
    );
    check("message-list page-size=-1", 18, "non-negative whole number");
    check("toolbar", 5, "needs the option `toolbar`");
    check("toolbar toolbar=\"nope\"", 5, "unknown toolbar `nope`");
    check(
        "folder-tree { row \"{name}\"; }",
        19,
        "does not take row templates",
    );
    check(
        "message-list { column \"x\"; }",
        20,
        "only contain `row` templates",
    );
    check("message-list { row \"{subject\"; }", 24, "unclosed `{`");
    check("spacer id=\"Main Pane\"", 12, "invalid id");
    check("spacer label=\"@\"", 12, "invalid message id");
    check(
        "stack id=\"a\" { spacer id=\"a\"; }",
        27,
        "`a` is used twice",
    );
    check("folder-tree \"x\"", 17, "too many arguments");
    check(
        "plugin.weather",
        5,
        "unknown widget or container `plugin.weather`",
    );
}

#[test]
fn action_key_toolbar_and_menu_errors() {
    let base = "layout \"x\" extends=\"default\" {\n  ";
    let check = |body: &str, column: u32, needle: &str| {
        assert_error(&format!("{base}{body}\n}}"), 2, column, needle);
    };
    check("action \"reply\"", 10, "invalid action id `reply`");
    check(
        "action \"a.b\" \"x\"",
        16,
        "only allowed together with alias-of",
    );
    check("action \"a.b\" icon=\"Reply\"", 16, "invalid icon name");
    check("action \"a.b\" alias-of=\"x\"", 16, "invalid action id `x`");
    check(
        "actions { bind \"j\" \"a.b\"; }",
        13,
        "may only contain `action`",
    );
    check("keys context=\"List\" { }", 8, "invalid context");
    check(
        "keys { bind \"j\" \"mail.next\" frobnicate=#true; }",
        31,
        "unknown property",
    );
    check(
        "keys { press \"j\" \"mail.next\"; }",
        10,
        "use bind or unbind",
    );
    check(
        "keys { bind \"ctrl+\" \"mail.next\"; }",
        15,
        "join chord keys",
    );
    check(
        "keys { bind \"j\"; }",
        10,
        "`bind` needs a value as argument 2",
    );
    check(
        "keys { bind \"j\" \"mail.next \\\"x\"; }",
        19,
        "unterminated quote",
    );
    check(
        "toolbar \"reader\" { button \"mail.reply\"; }",
        22,
        "use item, more, spacer or separator",
    );
    check("toolbar \"Reader\" { }", 11, "invalid toolbar name");
    check("menu \"context\" { }", 8, "invalid menu name");
    check(
        "menu \"app\" { spacer; }",
        16,
        "use item, header, separator or submenu",
    );
    check("menu \"app\" { item \"Reply\"; }", 21, "invalid action id");
}

// ---------------------------------------------------------------------------
// M5b (ADR 0109): views, the rail, toolbar overflow, the settings page.

const SHELL: &str = r#"
layout "Shell" extends="default" {
    window "main" {
        split "horizontal" id="shell" sizes="auto 1fr" resizable=#false {
            nav-rail id="rail" show-labels=#true
            view-host id="views"
        }
    }
    window "mail" {
        stack id="mail" {
            list-header id="list-header" chips="all unread"
            message-list id="list"
        }
    }
    views default="calendar" {
        view "mail" label="@view.mail" icon="mail" window="mail" badge="unread"
        view "calendar" icon="calendar" window="calendar"
    }
    rail {
        view "mail"
        view "calendar"
        spacer
        item "agents.open-center" icon="shield-check" badge="confirmations"
        separator
    }
    toolbar "reader" hide-disabled=#true overflow="more" {
        item "mail.reply" rank=1 display="icon-label"
        item "mail.archive" rank=4 display="icon"
        more icon="ellipsis" {
            header "@toolbar.group.message"
            item "mail.toggle-read"
            separator
            header "@toolbar.group.turn-into"
            item "tasks.from-message"
        }
    }
    settings-page {
        group "general" label="@settings.group.general" {
            section "accounts" label="@settings.section.accounts" icon="user-round" keywords="imap smtp" {
                account-settings id="a"
                virtual-account-editor id="b"
            }
        }
        group "look" {
            section "appearance" {
                appearance-settings id="c"
            }
        }
    }
}
"#;

#[test]
fn views_rail_overflow_and_settings_page_resolve() {
    use nail_api::{BadgeSource, ItemDisplay, RailItem, ToolbarOverflow};
    let l = layout(SHELL);
    assert_eq!(l.views.keys().collect::<Vec<_>>(), ["mail", "calendar"]);
    assert_eq!(l.default_view.as_deref(), Some("calendar"));
    assert_eq!(l.start_view(), Some("calendar"));
    assert_eq!(l.views["mail"].badge, Some(BadgeSource::Unread));
    assert_eq!(l.views["calendar"].label, None);
    assert_eq!(l.rail.len(), 5);
    assert!(matches!(
        &l.rail[3],
        RailItem::Action { action, badge: Some(BadgeSource::Confirmations), icon: Some(icon), .. }
            if action == "agents.open-center" && icon == "shield-check"
    ));
    // The rail's action gets an `actions` entry like toolbar items do.
    assert!(l.actions.contains_key("agents.open-center"));

    let reader = &l.toolbars["reader"];
    assert!(matches!(
        &reader[0],
        ToolbarItem::Action {
            rank: Some(1),
            display: Some(ItemDisplay::IconLabel),
            ..
        }
    ));
    let ToolbarItem::More { icon, items, .. } = &reader[2] else {
        panic!("expected more");
    };
    assert_eq!(icon.as_deref(), Some("ellipsis"));
    assert!(matches!(&items[0], MenuItem::Header { .. }));
    assert!(l.actions.contains_key("tasks.from-message"));
    let settings = l.toolbar_settings["reader"];
    assert!(settings.hide_disabled);
    assert_eq!(settings.overflow, ToolbarOverflow::More);
    // Toolbars from the base get default settings.
    assert!(!l.toolbar_settings["main"].hide_disabled);

    assert_eq!(l.settings.len(), 2);
    let accounts = l.settings_section("accounts").unwrap();
    assert_eq!(accounts.icon.as_deref(), Some("user-round"));
    assert_eq!(accounts.keywords.as_deref(), Some("imap smtp"));
    // Several nodes become a stack of auto rows.
    let LayoutNode::Stack {
        sizes, children, ..
    } = &accounts.root
    else {
        panic!("expected a stack");
    };
    assert_eq!(sizes, &["auto", "auto"]);
    assert_eq!(children.len(), 2);
    assert!(matches!(
        &l.settings_section("appearance").unwrap().root,
        LayoutNode::Widget { kind, .. } if kind == "appearance-settings"
    ));
}

#[test]
fn shell_sections_replace_the_base_and_old_files_inherit_them() {
    // A pre-M5b user file inherits the default views, rail and page.
    let old = layout(
        "layout \"mine\" extends=\"default\" {\n  toolbar \"main\" {\n    item \"mail.compose\"\n  }\n}\n",
    );
    let default = nail_config::default_layout();
    assert_eq!(old.views, default.views);
    assert_eq!(old.rail, default.rail);
    assert_eq!(old.settings, default.settings);
    // A replaced toolbar takes its settings from the file.
    assert_eq!(
        old.toolbar_settings["main"],
        nail_api::ToolbarSettings::default()
    );
    // Sections in the file replace the base's as a whole.
    let l = layout(
        "layout \"mine\" extends=\"default\" {\n  views {\n    view \"tasks\" window=\"tasks\"\n  }\n  rail {\n    view \"tasks\"\n  }\n}\n",
    );
    assert_eq!(l.views.keys().collect::<Vec<_>>(), ["tasks"]);
    assert_eq!(l.rail.len(), 1);
    assert_eq!(l.settings, default.settings);
}

#[test]
fn shell_errors() {
    let base = "layout \"x\" extends=\"default\" {\n  ";
    for (body, needle) in [
        (
            "views {\n    view \"Mail\" window=\"main\"\n  }",
            "invalid id",
        ),
        ("views {\n    view \"a\"\n  }", "`window`"),
        (
            "views {\n    view \"a\" window=\"main\"\n    view \"a\" window=\"main\"\n  }",
            "defined twice",
        ),
        (
            "views {\n    view \"a\" window=\"tasks\" badge=\"mail\"\n  }",
            "unread, confirmations",
        ),
        (
            "views {\n    view \"a\" window=\"nowhere\"\n  }",
            "which the layout does not define",
        ),
        (
            "views default=\"b\" {\n    view \"a\" window=\"tasks\"\n  }",
            "default view \"b\"",
        ),
        ("rail {\n    view \"ghost\"\n  }", "names view \"ghost\""),
        (
            "rail {\n    button \"x\"\n  }",
            "unexpected `button` in rail",
        ),
        (
            "toolbar \"t\" {\n    item \"a.b\" rank=0\n  }",
            "`rank` starts at 1",
        ),
        (
            "toolbar \"t\" {\n    item \"a.b\" display=\"big\"\n  }",
            "icon, label, icon-label",
        ),
        ("toolbar \"t\" overflow=\"scroll\" {\n  }", "more, wrap"),
        (
            "toolbar \"t\" hide-disabled=\"yes\" {\n  }",
            "hide-disabled",
        ),
        (
            "toolbar \"t\" {\n    more\n    more\n  }",
            "more than one `more`",
        ),
        (
            "views {\n    view \"a\" window=\"main\"\n  }\n  views {\n    view \"b\" window=\"main\"\n  }",
            "more than one `views` section",
        ),
        ("settings-page {\n  }", "settings-page"),
        (
            "settings-page {\n    group \"g\" {\n      section \"s\" {\n        text id=\"t\"\n      }\n      section \"s\" {\n        text id=\"u\"\n      }\n    }\n  }",
            "`s`",
        ),
        (
            "settings-page {\n    group \"g\" {\n      section \"s\" {\n        text id=\"t\"\n      }\n      section \"r\" {\n        text id=\"t\"\n      }\n    }\n  }",
            "`t`",
        ),
        ("settings-page {\n    page \"g\"\n  }", "group"),
        (
            "window \"main\" {\n    view-host\n  }\n  views {\n    view \"m\" window=\"main\"\n  }",
            "contains a view-host itself",
        ),
    ] {
        let err = error(&format!("{base}{body}\n}}"));
        assert!(err.message.contains(needle), "{body}: {err}");
    }
}

#[test]
fn shell_widgets_need_their_sections() {
    let text = "layout \"x\" {\n  window \"main\" {\n    stack {\n      nav-rail\n      view-host\n      settings-page\n    }\n  }\n}\n";
    let errors = nail_config::parse_layout_all(text, "layout.kdl").unwrap_err();
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    for widget in ["view-host", "nav-rail", "settings-page"] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!("the {widget} widget needs"))),
            "{messages:?}"
        );
    }
}

#[test]
fn the_default_settings_page_keeps_every_settings_tab() {
    // Nothing of the pre-M5b settings window (its tabs) may be lost on the
    // settings page; since WP165 the settings window is the page itself.
    const FORMER_SETTINGS_TABS: &[&str] = &[
        "account-settings",
        "appearance-settings",
        "virtual-account-editor",
        "mcp-clients",
        "confirmation-center",
        "security-keys",
        "security-identities",
        "security-settings",
        "security-autocrypt",
        "security-recovery-key",
        "security-smartcards",
        "remote-settings",
        "remote-users",
        "remote-devices",
        "remote-audit",
        "remote-content-allows",
        "sieve-status",
        "exchange-rules",
        "vacation-editor",
        "junk-settings",
        "notification-settings",
        "sync-folder-policies",
        "plugin-manager",
        "update-settings",
        "desktop-settings",
        "default-apps",
        "import-export",
        "backup-restore",
        "diagnostics",
        "toolbar",
        "config-errors",
        "toolbar-editor",
        "key-binding-editor",
        "column-editor",
    ];
    let l = nail_config::default_layout();
    let mut page = Vec::new();
    for section in l.settings.iter().flat_map(|g| g.sections.iter()) {
        page.extend(widgets(&section.root).into_iter().map(|(kind, _)| kind));
    }
    for kind in FORMER_SETTINGS_TABS {
        assert!(
            page.iter().any(|k| k == kind),
            "{kind} is not on the settings page"
        );
    }
    let window: Vec<String> = l.windows["settings"]
        .iter()
        .flat_map(|bp| widgets(&bp.root).into_iter().map(|(kind, _)| kind))
        .collect();
    assert_eq!(window, ["settings-page"]);
    let groups: Vec<&str> = l.settings.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(groups, nail_api::views::SETTINGS_GROUPS);
}
