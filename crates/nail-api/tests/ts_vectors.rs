//! The test vectors of `nail_api`'s rules the UI ports (see
//! `nail_api::ts_vectors`). Regenerate with `NAIL_UPDATE_SNAPSHOTS=1 cargo
//! test -p nail-api --test ts_vectors`.
#![cfg(test)]

use std::path::Path;

use nail_api::appearance::{self, AppearanceSettings, SystemAppearance, ThemeInfo};
use nail_api::config::layout::ToolbarItem;
use nail_api::recovery;
use nail_api::smartcard::{self, SmartcardSettings};
use nail_api::ts_vectors::Vectors;
use nail_api::windows::{self, WindowFrame, WindowGeometry, WindowSpec, WindowState, WindowStates};
use nail_api::{AccountId, ResolvedLayout, menus, remote, views};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// A contract value from JSON (the same JSON the TypeScript side gets).
fn from<T: DeserializeOwned>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap()
}

/// The default layout (the fixture the UI's tests use too).
fn default_layout() -> ResolvedLayout {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/default-layout.json"),
    )
    .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn windows_vectors() {
    let mut v = Vectors::default();
    let long = "a".repeat(windows::MAX_WINDOW_KIND_CHARS + 1);
    for kind in [
        "composer",
        "pim-event-editor",
        "a1",
        "x",
        "",
        "Composer",
        "1x",
        "a-",
        "a--b",
        "a b",
        "a.b",
        "a:b",
        long.as_str(),
    ] {
        v.case("isWindowKind", json!([kind]), windows::is_window_kind(kind));
    }
    let long_key = "k".repeat(windows::MAX_WINDOW_INSTANCE_CHARS + 1);
    for key in ["d42", "Draft_1-x", "", "a b", "a:b", long_key.as_str()] {
        v.case(
            "isWindowInstance",
            json!([key]),
            windows::is_window_instance(key),
        );
    }
    for id in [
        "composer:d42",
        "settings",
        "composer:Draft_1-x",
        "",
        "composer:",
        ":x",
        "composer:a:b",
        "composer:a b",
        "Composer:x",
    ] {
        v.case(
            "parseWindowId",
            json!([id]),
            windows::WindowId::parse(id).map(|w| json!({ "kind": w.kind, "instance": w.instance })),
        );
        v.case("windowKind", json!([id]), windows::window_kind(id));
    }
    for (kind, instance) in [("composer", Some("d1")), ("settings", None)] {
        v.case(
            "windowId",
            json!([kind, instance]),
            windows::window_id(kind, instance),
        );
    }
    for window in ["composer:d1", "settings"] {
        v.case(
            "popOutRoute",
            json!([window]),
            windows::pop_out_route(window),
        );
        v.case(
            "popOutLabel",
            json!([window]),
            windows::pop_out_label(window),
        );
    }
    for hash in [
        "#/window/composer:d1",
        "#/window/Bad",
        "#/print/x",
        "",
        "#/window/",
    ] {
        v.case(
            "parsePopOutRoute",
            json!([hash]),
            windows::parse_pop_out_route(hash),
        );
    }
    let specs = [
        serde_json::to_value(windows::DEFAULT_WINDOW_SPEC).unwrap(),
        serde_json::to_value(windows::CONFIRMATION_WINDOW_SPEC).unwrap(),
        json!({ "modal": false, "resizable": true, "min_size": { "width": 10, "height": 50_000 },
                "size": { "width": 5, "height": 100 }, "pop_out": true, "instances": "multiple" }),
        json!({ "modal": false, "resizable": true, "min_size": { "width": 500, "height": 400 },
                "size": { "width": 300, "height": 900 }, "pop_out": false, "instances": "single" }),
    ];
    for spec in &specs {
        v.case(
            "normalizeSpec",
            json!([spec]),
            from::<WindowSpec>(spec).normalized(),
        );
    }
    let frames = [
        json!({}),
        json!({ "modal": true, "size": { "width": 900, "height": 700 } }),
        json!({ "min_size": { "width": 1000, "height": 50 }, "instances": "multiple", "pop_out": true }),
        json!({ "resizable": false, "size": { "width": 100_000, "height": 1 } }),
    ];
    for spec in &specs[..2] {
        for frame in &frames {
            let full = frame_json(frame);
            v.case(
                "withFrame",
                json!([spec, full]),
                from::<WindowSpec>(spec).with_frame(&from::<WindowFrame>(&full)),
            );
        }
    }
    let geometries = [
        geometry(0, 0, 720, 520, false, None),
        geometry(-65_536, 65_536, 1, 16_384, false, None),
        geometry(0, 0, 0, 100, false, None),
        geometry(0, 0, 100, 16_385, false, None),
        geometry(65_537, 0, 100, 100, false, None),
        geometry(0, -65_537, 100, 100, false, None),
        geometry(0, 0, 100, 100, true, None),
        geometry(0, 0, 100, 100, false, Some("left")),
        geometry(0, 0, 100, 100, true, Some("right")),
    ];
    for g in &geometries {
        v.case(
            "isValidGeometry",
            json!([g]),
            from::<WindowGeometry>(g).is_valid(),
        );
    }
    let full: WindowStates = (0..windows::MAX_WINDOW_STATES)
        .map(|i| (format!("kind{i}"), WindowState::default()))
        .collect();
    let some = json!({ "page": geometries[0], "pop_out": null, "popped_out": false });
    let bad = json!({ "page": geometries[2], "pop_out": null, "popped_out": false });
    let empty = json!({ "page": null, "pop_out": null, "popped_out": false });
    let popped = json!({ "page": null, "pop_out": null, "popped_out": true });
    for (current, window, state) in [
        (&WindowStates::new(), "composer", Some(&some)),
        (&WindowStates::new(), "Bad Kind", Some(&some)),
        (&WindowStates::new(), "Bad Kind", None),
        (&WindowStates::new(), "composer", None),
        (&WindowStates::new(), "composer", Some(&bad)),
        (&full, "composer", Some(&some)),
        (&full, "composer", Some(&empty)),
        (&full, "composer", Some(&popped)),
        (&full, "kind3", Some(&some)),
    ] {
        let state = state.map(from::<WindowState>);
        v.case(
            "checkWindowStateWrite",
            json!([current.keys().collect::<Vec<_>>(), window, state]),
            windows::check_window_state_write(current, window, state.as_ref()),
        );
    }
    v.check("windows");
}

fn frame_json(frame: &Value) -> Value {
    let mut full = json!({ "modal": null, "resizable": null, "min_size": null, "size": null,
                          "pop_out": null, "instances": null });
    for (key, value) in frame.as_object().unwrap() {
        full[key] = value.clone();
    }
    full
}

fn geometry(x: i32, y: i32, width: u32, height: u32, maximized: bool, snap: Option<&str>) -> Value {
    json!({ "rect": { "x": x, "y": y, "width": width, "height": height },
            "maximized": maximized, "snap": snap })
}

#[test]
fn views_vectors() {
    let mut v = Vectors::default();
    let layout = default_layout();
    let mut views: Vec<&str> = layout.views.keys().map(String::as_str).collect();
    views.push("nope");
    for current in &views {
        for step in [1, -1] {
            v.case(
                "stepView",
                json!(["default-layout", current, step]),
                views::step_view(&layout, current, step),
            );
        }
    }
    for window in ["mail", "calendar", "settings", "main", "nope"] {
        v.case(
            "viewOfWindow",
            json!(["default-layout", window]),
            views::view_of_window(&layout, window),
        );
    }
    v.case("startView", json!(["default-layout"]), layout.start_view());
    for index in [0, 1, 7, 8, 9, 17] {
        v.case(
            "accountColorSlot",
            json!([index]),
            views::account_color_slot(index),
        );
    }
    let all_kinds = json!(["favorites", "accounts", "smart-folders"]);
    for (kinds, favorites, accounts, smart, collapsed) in [
        (
            all_kinds.clone(),
            json!(["all-inboxes", "flagged"]),
            json!(["a1", "a2"]),
            0,
            json!([]),
        ),
        (
            all_kinds.clone(),
            json!([]),
            json!(["a1"]),
            2,
            json!(["account:a1", "smart-folders"]),
        ),
        (
            json!(["smart-folders", "favorites"]),
            json!(["unread"]),
            json!([]),
            1,
            json!(["favorites"]),
        ),
        (
            all_kinds,
            json!(["drafts"]),
            json!((1..=10).map(|i| format!("acc{i}")).collect::<Vec<_>>()),
            0,
            json!([]),
        ),
    ] {
        let sections = views::folder_pane_sections(
            &from::<Vec<views::FolderPaneSectionKind>>(&kinds),
            &from::<Vec<views::FavoriteFolder>>(&favorites),
            &from::<Vec<AccountId>>(&accounts),
            smart,
            &from::<Vec<String>>(&collapsed),
        );
        let out: Vec<Value> = sections
            .iter()
            .map(|s| {
                json!({ "id": s.id, "kind": s.kind, "accountId": s.account_id,
                        "colorSlot": s.color_slot, "collapsed": s.collapsed })
            })
            .collect();
        v.case(
            "folderPaneSections",
            json!([kinds, favorites, accounts, smart, collapsed]),
            out,
        );
    }
    let action = |id: &str, rank: Option<u32>| {
        json!({ "type": "action", "action": id, "args": [], "label": null, "icon": null,
                "rank": rank, "display": null })
    };
    let toolbars = [
        json!([
            action("a", Some(1)),
            action("b", None),
            { "type": "separator" },
            action("c", Some(3)),
            action("d", None),
            action("e", Some(3)),
            { "type": "spacer" },
            action("f", Some(2)),
        ]),
        json!([action("only", None)]),
        json!([{ "type": "spacer" }]),
        json!([]),
    ];
    for items in &toolbars {
        v.case(
            "overflowOrder",
            json!([items]),
            views::overflow_order(&from::<Vec<ToolbarItem>>(items)),
        );
    }
    // Every toolbar of the default layout.
    for items in layout.toolbars.values() {
        v.case(
            "overflowOrder",
            json!([items]),
            views::overflow_order(items),
        );
    }
    v.check("views");
}

#[test]
fn menus_vectors() {
    let mut v = Vectors::default();
    let layout = default_layout();
    let mut parts: Vec<&str> = menus::CONTEXT_MENU_PARTS.iter().map(|p| p.part).collect();
    parts.push("unknown.part");
    for part in &parts {
        v.case("keyContextOf", json!([part]), menus::key_context_of(part));
        v.case(
            "contextMenuName",
            json!([part]),
            menus::context_menu_name(part),
        );
    }
    for name in layout.menus.keys() {
        for context in ["global", "list", "reader", "composer", "calendar"] {
            v.case(
                "resolveMenu",
                json!(["default-layout", name, context]),
                menus::resolve_menu(&layout, name, context),
            );
        }
    }
    v.case(
        "resolveMenu",
        json!(["default-layout", "no-such-menu", "global"]),
        menus::resolve_menu(&layout, "no-such-menu", "global"),
    );
    for binding in &layout.keys {
        for context in ["global", "list", "reader"] {
            v.case(
                "shortcut",
                json!(["default-layout", binding.action, binding.args, context]),
                menus::shortcut(&layout, &binding.action, &binding.args, context),
            );
        }
    }
    v.check("menus");
}

#[test]
fn appearance_vectors() {
    let mut v = Vectors::default();
    for color in [
        "#abc", "#AbCdEf", "#12345", "abc", "#ggg", "#1234567", "#", "",
    ] {
        v.case(
            "isHexColor",
            json!([color]),
            appearance::is_hex_color(color),
        );
    }
    let long = format!("t{}", "a".repeat(appearance::MAX_THEME_ID_CHARS));
    for id in [
        "frost",
        "my-theme-2",
        "Frost",
        "2024",
        "a--b",
        "a-",
        "",
        "a_b",
        long.as_str(),
    ] {
        v.case("isThemeId", json!([id]), appearance::is_theme_id(id));
    }
    let long_name = "n".repeat(nail_api::editing::MAX_THEME_NAME_CHARS + 1);
    for name in [
        "My Ocean",
        "  ",
        "",
        "tab\there",
        " padded ",
        "äöü",
        long_name.as_str(),
    ] {
        v.case(
            "isThemeName",
            json!([name]),
            nail_api::editing::is_theme_name(name),
        );
    }
    for overrides in [
        json!({}),
        json!({ "font_size": 11, "radius": 0, "accent": "#fff" }),
        json!({ "font_size": 10, "radius": 21, "accent": "red" }),
        json!({ "font_size": 21 }),
        json!({ "accent": "#12345g" }),
    ] {
        v.case(
            "overrideIssues",
            json!([overrides]),
            from::<appearance::ThemeOverrides>(&overrides).issues(),
        );
    }
    let theme = |id: &str, kind: &str, problem: bool| {
        let mut t = json!({ "id": id, "name": id, "kind": kind, "source": "builtin",
                            "file": format!("{id}.kdl") });
        if problem {
            t["problem"] = json!({ "id": "theme-problem-unreadable", "args": {} });
        }
        t
    };
    let themes = json!([
        theme("frost", "light", false),
        theme("graphite", "dark", false),
        theme("high-contrast", "high-contrast", false),
        theme("ocean", "dark", false),
        theme("broken", "dark", true),
        theme("mine-hc", "high-contrast", false),
    ]);
    let no_hc = json!([
        theme("frost", "light", false),
        theme("graphite", "dark", false)
    ]);
    let settings = |theme: &str, follow: bool, light: &str, dark: &str| {
        json!({ "theme": theme, "follow_system": follow, "light": light, "dark": dark,
                "density": "comfortable", "overrides": {} })
    };
    let systems = [
        Value::Null,
        json!({ "scheme": "light", "high_contrast": false }),
        json!({ "scheme": "dark", "high_contrast": false }),
        json!({ "scheme": "dark", "high_contrast": true }),
    ];
    for s in [
        settings("frost", true, "frost", "ocean"),
        settings("frost", true, "broken", "broken"),
        settings("frost", true, "missing", "graphite"),
        settings("ocean", false, "frost", "graphite"),
        settings("broken", false, "frost", "graphite"),
        settings("missing", false, "frost", "graphite"),
    ] {
        for system in &systems {
            for list in [&themes, &no_hc] {
                let parsed: Option<SystemAppearance> = from(system);
                v.case(
                    "activeTheme",
                    json!([s, system, list]),
                    appearance::active_theme(
                        &from::<AppearanceSettings>(&s),
                        parsed,
                        &from::<Vec<ThemeInfo>>(list),
                    ),
                );
            }
        }
    }
    for active in ["frost", "high-contrast", "mine-hc", "missing"] {
        v.case(
            "accentIgnored",
            json!([active, themes]),
            appearance::accent_ignored(active, &from::<Vec<ThemeInfo>>(&themes)),
        );
    }
    // WCAG colour math (`nail_util::color`, the UI's `theme/contrast.ts`).
    for color in [
        "#000",
        "#ffffff",
        "#767676",
        "#1A2b3C",
        " #fb4c2f ",
        "#0a0a0a",
        "#0b0b0b",
        "red",
        "#12345",
        "var(--x)",
    ] {
        v.case(
            "parseHex",
            json!([color]),
            nail_util::color::parse_hex(color),
        );
    }
    for (a, b) in [
        ("#000", "#fff"),
        ("#767676", "#ffffff"),
        ("#fb4c2f", "#202124"),
        ("#0a0a0a", "#0b0b0b"),
        ("#ffffff", "#ffffff"),
        ("red", "#ffffff"),
    ] {
        let ratio = nail_util::color::parse_hex(a)
            .zip(nail_util::color::parse_hex(b))
            .map(|(a, b)| (nail_util::color::contrast_ratio(a, b) * 1e6).round() / 1e6);
        v.case("hexContrastRatio", json!([a, b]), ratio);
    }
    v.check("appearance");
}

#[test]
fn security_vectors() {
    let mut v = Vectors::default();
    let keys: [[u8; recovery::RECOVERY_KEY_BYTES]; 3] = [
        [0; 32],
        [0xff; 32],
        std::array::from_fn(|i| u8::try_from(i * 7 + 3).unwrap()),
    ];
    for key in &keys {
        let text = recovery::format_recovery_key(key);
        v.case("formatRecoveryKey", json!([key]), &text);
        let lower = text.to_lowercase().replace('-', " ");
        let mut typo = text.clone();
        typo.replace_range(0..1, if text.starts_with('0') { "1" } else { "0" });
        for typed in [text.clone(), lower, typo, text[..text.len() - 2].to_owned()] {
            v.case(
                "parseRecoveryKey",
                json!([typed]),
                recovery_result(recovery::parse_recovery_key(&typed)),
            );
        }
    }
    for typed in [
        "",
        "  - ",
        "U000",
        "oil0-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000",
    ] {
        v.case(
            "parseRecoveryKey",
            json!([typed]),
            recovery_result(recovery::parse_recovery_key(typed)),
        );
    }
    let module = |path: &str| json!({ "path": path, "name": null, "enabled": true });
    for settings in [
        json!({ "modules": [], "pin_cache_minutes": 0 }),
        json!({ "modules": [module("C:\\pkcs11\\a.dll"), module("/usr/lib/b.so"), module("\\\\srv\\c.dll")],
                "pin_cache_minutes": smartcard::MAX_PIN_CACHE_MINUTES }),
        json!({ "modules": [module("a.dll")], "pin_cache_minutes": 0 }),
        json!({ "modules": [module("C:/x.dll"), module("c:/X.DLL")], "pin_cache_minutes": 0 }),
        json!({ "modules": [], "pin_cache_minutes": smartcard::MAX_PIN_CACHE_MINUTES + 1 }),
        json!({ "modules": [module("rel"), module("rel")], "pin_cache_minutes": 9999 }),
    ] {
        v.case(
            "smartcardSettingsIssue",
            json!([settings]),
            from::<SmartcardSettings>(&settings)
                .issues()
                .first()
                .map(|i| i.message_id()),
        );
    }
    let long = "9".repeat(smartcard::MAX_PIN_CHARS + 1);
    for pin in [
        "1234",
        "123",
        "äöüß",
        "12\n34",
        "12\u{7f}3",
        long.as_str(),
        "",
    ] {
        v.case(
            "pinIsPlausible",
            json!([pin]),
            smartcard::pin_is_plausible(pin),
        );
    }
    let method = |verdict: &str| json!({ "verdict": verdict });
    for results in [
        json!({ "authserv_id": "mx", "dkim": [] }),
        json!({ "authserv_id": "mx", "dkim": [], "spf": method("softfail") }),
        json!({ "authserv_id": "mx", "dkim": [method("fail"), method("pass")], "spf": method("fail") }),
        json!({ "authserv_id": "mx", "dkim": [method("temperror"), method("neutral")] }),
        json!({ "authserv_id": "mx", "dkim": [method("pass")], "dmarc": method("policy") }),
        json!({ "authserv_id": "mx", "dkim": [], "spf": method("pass"), "dmarc": method("fail") }),
    ] {
        v.case(
            "authSummary",
            json!([results]),
            from::<nail_api::security::AuthResults>(&results).summary(),
        );
    }
    v.check("security");
}

#[test]
fn remote_vectors() {
    let mut v = Vectors::default();
    for net in [
        "127.0.0.1/32",
        "10.1.0.0/16",
        "10.0.0.0/7",
        "172.16.0.0/12",
        "172.32.0.0/16",
        "192.168.1.0/24",
        "169.254.0.0/16",
        "100.64.0.0/10",
        "100.128.0.0/10",
        "8.8.8.8/32",
        "::1/128",
        "::1/127",
        "fd00::/8",
        "fc00::/6",
        "fe80::1/64",
        "2001:db8::/32",
        "::ffff:192.168.0.1/128",
        "::ffff:8.8.8.8/128",
        "not-a-net",
        "10.0.0.1",
        "10.0.0.0/33",
    ] {
        v.case(
            "isLocalNetwork",
            json!([net]),
            remote::is_local_network(net),
        );
    }
    for preset in remote::RemoteAccessPreset::ALL {
        v.case("presetGrant", json!([preset]), preset.grant());
    }
    let base = serde_json::to_value(remote::RemoteUiSettings::default()).unwrap();
    let with = |changes: Value| {
        let mut s = base.clone();
        for (key, value) in changes.as_object().unwrap() {
            s[key] = value.clone();
        }
        s
    };
    for settings in [
        base.clone(),
        with(json!({ "bind_address": "nope" })),
        with(json!({ "bind_address": "192.168.1.5", "tls": "off" })),
        with(json!({ "bind_address": "192.168.1.5", "tls": "self-signed" })),
        with(json!({ "port": 0 })),
        with(json!({ "internet_mode": true })),
        with(json!({ "tls": "acme" })),
        with(json!({ "tls": "acme", "internet_mode": true,
                     "acme": { "domains": ["mail.example.org"], "contact_email": null, "directory_url": "https://acme.example/dir", "accept_terms": true } })),
        with(json!({ "allowed_networks": ["10.0.0.0/8", "nope"] })),
        with(json!({ "allowed_networks": ["8.8.8.0/24"] })),
        with(json!({ "allowed_networks": ["192.168.0.0/16", "::1/128"] })),
        with(json!({ "host_name": "bad name" })),
        with(json!({ "host_name": "nail.example.org" })),
        with(json!({ "session_idle_minutes": 0 })),
        with(json!({ "session_idle_minutes": 600, "session_max_hours": 2 })),
        with(json!({ "reauth_minutes": 0 })),
        with(json!({ "audit_retention_days": 0 })),
        with(json!({ "audit_retention_days": remote::MAX_AUDIT_RETENTION_DAYS + 1 })),
    ] {
        v.case(
            "settingsIssues",
            json!([settings]),
            from::<remote::RemoteUiSettings>(&settings).issues(),
        );
    }
    v.check("remote");
}

fn recovery_result(
    result: Result<[u8; recovery::RECOVERY_KEY_BYTES], recovery::RecoveryKeyIssue>,
) -> Value {
    match result {
        Ok(bytes) => json!({ "ok": true, "bytes": bytes }),
        Err(issue) => json!({ "ok": false, "issue": issue }),
    }
}

#[test]
fn mail_actions_vectors() {
    use nail_api::mail_actions::MailAction;
    let mut v = Vectors::default();
    for action in [
        json!({ "type": "archive" }),
        json!({ "type": "trash" }),
        json!({ "type": "delete-permanently" }),
        json!({ "type": "move", "target": { "role": "inbox" } }),
        json!({ "type": "move", "target": { "folder_id": "f1" } }),
        json!({ "type": "mark-read", "read": true }),
        json!({ "type": "mark-read", "read": false }),
        json!({ "type": "flag", "flagged": true }),
        json!({ "type": "flag", "flagged": false }),
        json!({ "type": "tags", "add": ["a"], "remove": [] }),
        json!({ "type": "snooze", "until": "2026-10-09T08:00:00Z", "wake_folder_id": null }),
        json!({ "type": "junk", "junk": true }),
        json!({ "type": "junk", "junk": false }),
    ] {
        let parsed: MailAction = from(&action);
        v.case("reachOf", json!([action]), parsed.reach());
        v.case("kindOf", json!([action]), parsed.kind());
    }
    v.check("mail-actions");
}

#[test]
fn agents_vectors() {
    use nail_api::agents::{ActionKind, ActionPolicy, ActionPolicyMode};
    let mut v = Vectors::default();
    let all_allow: Vec<(ActionKind, ActionPolicyMode)> = ActionKind::ALL
        .iter()
        .map(|&k| (k, ActionPolicyMode::Allow))
        .collect();
    let all_ask: Vec<(ActionKind, ActionPolicyMode)> = ActionKind::ALL
        .iter()
        .map(|&k| (k, ActionPolicyMode::Ask))
        .collect();
    for stored in [
        vec![],
        all_allow,
        all_ask,
        vec![
            (ActionKind::Send, ActionPolicyMode::Allow),
            (ActionKind::Draft, ActionPolicyMode::Ask),
            (ActionKind::Trash, ActionPolicyMode::Allow),
        ],
    ] {
        let record: serde_json::Map<String, Value> = stored
            .iter()
            .map(|(k, m)| (k.as_str().to_owned(), serde_json::to_value(m).unwrap()))
            .collect();
        v.case("policyTable", json!([record]), ActionPolicy::table(&stored));
    }
    v.check("agents");
}

#[test]
fn exchange_vectors() {
    use nail_api::exchange::oof::AutomaticRepliesSettings;
    use nail_api::exchange::{ExchangeCategorySettings, ExchangeMailboxSettings};
    let mut v = Vectors::default();
    let long = "c".repeat(nail_api::exchange::MAX_CATEGORY_NAME + 1);
    for name in [
        "Project",
        "  ",
        "a,b",
        "a;b",
        "tab\there",
        "x\u{1f}y",
        long.as_str(),
        " Ünïcode ",
    ] {
        let settings = json!({ "name": name, "color": "preset3" });
        v.case(
            "categoryIssues",
            json!([settings]),
            from::<ExchangeCategorySettings>(&settings).issues(),
        );
    }
    let mailbox = |address: &str, name: Option<&str>, shown: bool| {
        json!({ "address": address, "name": name, "kind": "shared", "mail": shown,
                "calendar": false, "contacts": false, "tasks": false })
    };
    for settings in [
        mailbox("support@contoso.example", None, true),
        mailbox("", None, true),
        mailbox("not-an-address", None, true),
        mailbox("Me@Contoso.Example", None, true),
        mailbox("team@contoso.example", Some("  "), true),
        mailbox("team@contoso.example", Some("Team"), false),
        mailbox("@contoso.example", None, false),
    ] {
        v.case(
            "mailboxIssues",
            json!([settings, "me@contoso.example"]),
            from::<ExchangeMailboxSettings>(&settings).issues("me@contoso.example"),
        );
    }
    for email in [
        "a@b.example",
        "a@b",
        "@b.example",
        "a@",
        "a@.b",
        "a@b.",
        "a b@c.example",
        "a@b@c.example",
        "",
    ] {
        v.case(
            "isPlausibleEmail",
            json!([email]),
            nail_api::pim::is_plausible_email(email),
        );
    }
    let replies = |status: &str, start: Option<&str>, end: Option<&str>, internal: &str| {
        json!({ "status": status, "start": start, "end": end, "internal_reply": internal,
                "external_reply": "", "external_audience": "all" })
    };
    let long_reply = "r".repeat(nail_api::exchange::oof::MAX_REPLY_CHARS + 1);
    for settings in [
        replies("disabled", None, None, ""),
        replies("always-enabled", None, None, "Away"),
        replies("always-enabled", None, None, " "),
        replies("scheduled", None, None, "Away"),
        replies(
            "scheduled",
            Some("2026-10-09T08:00:00Z"),
            Some("2026-10-08T08:00:00Z"),
            "Away",
        ),
        replies(
            "scheduled",
            Some("2026-10-08T08:00:00Z"),
            Some("2026-10-09T08:00:00Z"),
            "Away",
        ),
        replies("disabled", Some("tomorrow"), None, ""),
        replies("always-enabled", None, None, long_reply.as_str()),
    ] {
        v.case(
            "automaticRepliesIssues",
            json!([settings]),
            from::<AutomaticRepliesSettings>(&settings).issues(),
        );
    }
    for host in [
        "outlook.office365.com",
        "Outlook.Office365.com.",
        "outlook.office.com",
        "eur.outlook.com",
        "notoffice365.com",
        "office365.com.evil.example",
        "mail.contoso.example",
        "hotmail.com",
        "",
    ] {
        v.case(
            "isExchangeOnlineHost",
            json!([host]),
            nail_api::hosts::is_exchange_online_host(host),
        );
    }
    v.check("exchange");
}

#[test]
fn gmail_vectors() {
    use nail_api::gmail::GmailLabelSettings;
    let mut v = Vectors::default();
    let long = "l".repeat(nail_api::gmail::MAX_LABEL_NAME + 1);
    let fits = "l".repeat(nail_api::gmail::MAX_LABEL_NAME);
    let label = |name: &str, color: Option<(&str, &str)>| {
        json!({
            "name": name,
            "color": color.map(|(background, text)| json!({ "background": background, "text": text })),
            "list_visibility": "show",
            "message_visibility": "show",
        })
    };
    for settings in [
        label("Work", None),
        label("Work/Projects", Some(("#fb4c2f", "#ffffff"))),
        label("", None),
        label("   ", None),
        label("INBOX", None),
        label("Category_Social", None),
        label("/Work", None),
        label("Work/", None),
        label("a//b", None),
        label("tab\there", None),
        label("x\u{7f}y", None),
        label("x\u{9f}y", None),
        label("Ünïcödé", None),
        label(long.as_str(), None),
        label(fits.as_str(), None),
        label("Work", Some(("red", "#ffffff"))),
        label("", Some(("#fb4c2f", "#fff"))),
    ] {
        v.case(
            "labelIssues",
            json!([settings]),
            from::<GmailLabelSettings>(&settings).issues(),
        );
    }
    v.check("gmail");
}

#[test]
fn print_vectors() {
    use nail_api::print::{PrintTarget, print_view_route};
    let mut v = Vectors::default();
    let agenda = |start: &str, end: &str| json!({ "type": "agenda", "start": start, "end": end, "calendar_ids": [], "layout": "list" });
    for target in [
        agenda("2026-10-01", "2026-10-07"),
        agenda("2026-10-07", "2026-10-01"),
        agenda("2025-01-01", "2026-12-31"),
        agenda("2026-01-01", "2026-12-31"),
        agenda("2024-01-01", "2024-12-31"),
        agenda("soon", "2026-12-31"),
        agenda("2026-02-30", "2026-03-01"),
        json!({ "type": "message", "message_id": "m1" }),
    ] {
        v.case(
            "targetIssue",
            json!([target]),
            from::<PrintTarget>(&target)
                .issues()
                .first()
                .map(|i| i.message_id()),
        );
    }
    for job in ["j1", "job-42"] {
        v.case(
            "printViewRoute",
            json!([job]),
            print_view_route(&job.to_owned().into()),
        );
    }
    v.check("print");
}

#[test]
fn system_vectors() {
    use nail_api::desktop::UpdateState;
    let mut v = Vectors::default();
    let update = json!({ "version": "1.2.0", "channel": "stable", "published_at": null,
                         "notes": null, "size_bytes": 1000 });
    for state in [
        json!({ "state": "idle" }),
        json!({ "state": "checking" }),
        json!({ "state": "up-to-date" }),
        json!({ "state": "available", "update": update }),
        json!({ "state": "downloading", "update": update, "downloaded_bytes": 10, "total_bytes": null }),
        json!({ "state": "ready-to-install", "update": update }),
        json!({ "state": "installing", "update": update }),
        json!({ "state": "failed", "failure": "network", "detail": null, "update": null }),
        json!({ "state": "failed", "failure": "download", "detail": "x", "update": update }),
        json!({ "state": "disabled", "reason": "portable" }),
    ] {
        let parsed: UpdateState = from(&state);
        v.case("canCheck", json!([state]), parsed.can_check());
        v.case("canDownload", json!([state]), parsed.can_download());
        v.case("canInstall", json!([state]), parsed.can_install());
        v.case("updateOf", json!([state]), parsed.update());
    }
    v.check("system");
}

#[test]
fn triage_vectors() {
    use nail_api::triage::{JunkSettings, NotificationSettings, parse_hhmm};
    let mut v = Vectors::default();
    let junk = |threshold: u32, min_training: u32| {
        json!({ "enabled": true, "threshold": threshold, "min_training": min_training,
                "honor_server_flags": true, "move_to_junk": true })
    };
    for settings in [
        junk(90, 10),
        junk(49, 10),
        junk(50, 1),
        junk(99, 10_000),
        junk(100, 0),
        junk(70, 10_001),
    ] {
        v.case(
            "junkIssues",
            json!([settings]),
            from::<JunkSettings>(&settings).issues(),
        );
    }
    for text in [
        "00:00", "23:59", "24:00", "12:60", "7:00", "07:5", "ab:cd", "", "07:00 ",
    ] {
        v.case("parseHhmm", json!([text]), parse_hhmm(text));
    }
    let notify = |quiet: Value, overrides: Value| {
        json!({ "enabled": true, "show_preview": true, "sound": true, "folders": "inbox",
                "overrides": overrides, "quiet_hours": quiet })
    };
    let quiet = |start: &str, end: &str| json!({ "start": start, "end": end, "days": [] });
    let account =
        |id: &str| json!({ "scope": { "type": "account", "account_id": id }, "notify": false });
    let folder =
        |id: &str| json!({ "scope": { "type": "folder", "folder_id": id }, "notify": true });
    for settings in [
        notify(Value::Null, json!([])),
        notify(quiet("22:00", "07:00"), json!([])),
        notify(quiet("22:00", "22:00"), json!([])),
        notify(quiet("25:00", "07:00"), json!([])),
        notify(
            Value::Null,
            json!([account("a1"), folder("a1"), account("a2")]),
        ),
        notify(Value::Null, json!([account("a1"), account("a1")])),
        notify(quiet("x", "07:00"), json!([folder("f"), folder("f")])),
    ] {
        v.case(
            "notificationIssues",
            json!([settings]),
            from::<NotificationSettings>(&settings).issues(),
        );
    }
    v.check("triage");
}

#[test]
fn sieve_vectors() {
    use nail_api::sieve::{ManageSieveConfig, VacationSettings, is_iso_date};
    let mut v = Vectors::default();
    for text in [
        "2026-10-08",
        "2026-13-01",
        "2026-00-10",
        "2026-02-31",
        "2026-1-01",
        "26-10-08",
        "",
    ] {
        v.case("isIsoDate", json!([text]), is_iso_date(text));
    }
    let vacation = |changes: Value| {
        let mut s = serde_json::to_value(VacationSettings::default()).unwrap();
        for (key, value) in changes.as_object().unwrap() {
            s[key] = value.clone();
        }
        s
    };
    for settings in [
        vacation(json!({})),
        vacation(json!({ "enabled": true })),
        vacation(json!({ "enabled": true, "body": "Away" })),
        vacation(json!({ "days": 0 })),
        vacation(json!({ "days": 366 })),
        vacation(json!({ "start_date": "2026-10-09", "end_date": "2026-10-08" })),
        vacation(json!({ "start_date": "tomorrow" })),
        vacation(json!({ "addresses": ["a@b.example", "nope"] })),
        vacation(json!({ "addresses": ["a b@c.example"] })),
        vacation(json!({ "from": " " })),
        vacation(json!({ "subject": "two\nlines" })),
    ] {
        v.case(
            "vacationIssues",
            json!([settings]),
            from::<VacationSettings>(&settings).issues(),
        );
    }
    let long = "s".repeat(nail_api::sieve::MAX_SCRIPT_NAME_LEN + 1);
    for (host, port, script) in [
        (None, None, None),
        (Some("sieve.example"), Some(4190), Some("nail")),
        (Some(" "), Some(0), Some("")),
        (Some("a b"), None, Some("a/b")),
        (None, None, Some("tab\there")),
        (None, None, Some(long.as_str())),
    ] {
        let config = json!({ "enabled": null, "host": host, "port": port, "tls": null,
                             "script_name": script });
        v.case(
            "manageSieveIssues",
            json!([config]),
            from::<ManageSieveConfig>(&config).issues(),
        );
    }
    v.check("sieve");
}

#[test]
fn portability_vectors() {
    use nail_api::backup::{self, BackupContents, BackupItem};
    use nail_api::diagnostics::{DiagnosticsItem, DiagnosticsOptions};
    use nail_api::portability::{ExportFormat, ExportSource, ImportFormat, JobProgress, JobState};
    let mut v = Vectors::default();
    for f in ImportFormat::ALL {
        v.case("importIsDirectory", json!([f]), f.is_directory());
        v.case("importHasMail", json!([f]), f.has_mail());
        v.case("importHasPim", json!([f]), f.has_pim());
    }
    let sources = [
        json!({ "type": "folders", "folder_ids": [], "include_subfolders": false }),
        json!({ "type": "messages", "message_ids": [] }),
        json!({ "type": "calendars", "calendar_ids": [] }),
        json!({ "type": "address-books", "address_book_ids": [] }),
    ];
    for f in ExportFormat::ALL {
        v.case("exportIsDirectory", json!([f]), f.is_directory());
        v.case("exportExtension", json!([f]), f.extension());
        for source in &sources {
            v.case(
                "exportAccepts",
                json!([f, source["type"]]),
                f.accepts(&from::<ExportSource>(source)),
            );
        }
    }
    for item in BackupItem::ALL {
        v.case("backupDefaultOn", json!([item]), item.default_on());
    }
    for item in DiagnosticsItem::ALL {
        v.case("diagnosticsIsMail", json!([item]), item.is_mail());
    }
    for state in JobState::ALL {
        v.case("isFinished", json!([state]), state.is_finished());
    }
    for items in [
        json!([]),
        json!(["settings", "keys"]),
        json!(["theme", "theme"]),
    ] {
        let contents = json!({ "items": items });
        v.case(
            "backupContentsIssues",
            json!([contents]),
            from::<BackupContents>(&contents).issues(),
        );
    }
    let short = "p".repeat(backup::MIN_BACKUP_PASSPHRASE_CHARS - 1);
    let enough = "p".repeat(backup::MIN_BACKUP_PASSPHRASE_CHARS);
    let long = "p".repeat(backup::MAX_BACKUP_PASSPHRASE_CHARS + 1);
    for passphrase in [
        "",
        short.as_str(),
        enough.as_str(),
        long.as_str(),
        "äöüß€😀aaaaaa",
    ] {
        v.case(
            "passphraseIssues",
            json!([passphrase]),
            backup::passphrase_issues(passphrase),
        );
    }
    let many: Vec<String> = (0..=nail_api::diagnostics::MAX_DIAGNOSTICS_MESSAGES)
        .map(|i| format!("m{i}"))
        .collect();
    for (items, ids) in [
        (json!([]), json!([])),
        (json!(["logs"]), json!([])),
        (json!(["mail-headers"]), json!([])),
        (json!(["logs"]), json!(["m1"])),
        (json!(["mail-content"]), json!(many)),
    ] {
        let options = json!({ "items": items, "message_ids": ids, "redact_addresses": true });
        v.case(
            "diagnosticsIssues",
            json!([options]),
            from::<DiagnosticsOptions>(&options).issues(),
        );
    }
    for (done, total, bytes_done, bytes_total) in [
        (0, None, 0, None),
        (5, Some(10), 0, None),
        (5, Some(10), 300, Some(1000)),
        (12, Some(10), 0, None),
        (0, Some(0), 0, None),
        (0, None, 0, Some(0)),
        (3, Some(0), 10, Some(40)),
    ] {
        let progress = json!({ "phase": "reading", "done": done, "total": total,
                               "bytes_done": bytes_done, "bytes_total": bytes_total, "current": null });
        v.case(
            "progressFraction",
            json!([progress]),
            from::<JobProgress>(&progress).fraction(),
        );
    }
    v.check("portability");
}

#[test]
fn plugin_extensions_vectors() {
    use nail_api::plugins::extensions::{
        PluginAttachmentPreviewDef, PluginLinkActionDef, parse_plugin_extension_id,
        plugin_extension_id,
    };
    let mut v = Vectors::default();
    for (plugin, name) in [("acme-crm", "ticket"), ("x", "y")] {
        v.case(
            "extensionId",
            json!([plugin, name]),
            plugin_extension_id(&plugin.to_owned().into(), name),
        );
    }
    for id in ["acme-crm.ticket", "a.b.c", ".x", "x.", "nodot", ""] {
        v.case(
            "parseExtensionId",
            json!([id]),
            parse_plugin_extension_id(id)
                .map(|(plugin, name)| json!({ "plugin": plugin, "name": name })),
        );
    }
    for (mimes, exts, mime, filename) in [
        (json!(["text/csv"]), json!([]), "text/csv", Some("a.csv")),
        (json!(["text/*"]), json!([]), "TEXT/Plain", None),
        (json!(["image/*"]), json!([]), "text/plain", Some("a.png")),
        (
            json!([]),
            json!(["csv"]),
            "application/octet-stream",
            Some("Data.CSV"),
        ),
        (
            json!([]),
            json!(["csv"]),
            "application/octet-stream",
            Some("csv"),
        ),
        (
            json!([]),
            json!(["gz"]),
            "application/gzip",
            Some("a.tar.gz"),
        ),
        (json!(["text/csv"]), json!(["tsv"]), "text/plain", None),
    ] {
        let def = json!({ "id": "p", "title": "P", "mimeTypes": mimes, "extensions": exts });
        v.case(
            "matchesFile",
            json!([mimes, exts, mime, filename]),
            from::<PluginAttachmentPreviewDef>(&def).matches(mime, filename),
        );
    }
    for (pattern, host) in [
        ("jira.example.com", "jira.example.com"),
        ("jira.example.com", "JIRA.example.com."),
        ("*.example.com", "a.example.com"),
        ("*.example.com", "example.com"),
        ("*.example.com", "xexample.com"),
        ("*.example.com", "a.b.example.com"),
        ("jira.example.com", ""),
    ] {
        let def = json!({ "id": "l", "title": "L", "hosts": [pattern] });
        v.case(
            "hostMatches",
            json!([pattern, host]),
            from::<PluginLinkActionDef>(&def).matches_host(host),
        );
    }
    let long = format!("a{}", "b".repeat(nail_api::plugins::MAX_NAME_LEN));
    for name in [
        "acme-crm",
        "a1",
        "",
        "Acme",
        "1a",
        "a-",
        "a--b",
        "a_b",
        long.as_str(),
    ] {
        v.case(
            "isPluginName",
            json!([name]),
            nail_api::plugins::is_plugin_id(name),
        );
    }
    for (plugin, command) in [("acme", "open-ticket"), ("x", "y")] {
        v.case(
            "pluginCommandId",
            json!([plugin, command]),
            nail_api::plugins::plugin_command_id(&plugin.to_owned().into(), command),
        );
    }
    for action in [
        "plugin.acme.open",
        "plugin.acme",
        "plugin.Acme.open",
        "mail.reply",
        "plugin.acme.open.x",
        "plugin..open",
    ] {
        v.case(
            "parsePluginCommandId",
            json!([action]),
            nail_api::plugins::parse_plugin_command_id(action)
                .map(|(plugin, command)| json!({ "plugin": plugin, "command": command })),
        );
    }
    for id in ["acme.panel", "acme", "acme.Panel", "a.b.c"] {
        v.case(
            "parsePluginPanelId",
            json!([id]),
            nail_api::plugins::parse_plugin_panel_id(id)
                .map(|(plugin, panel)| json!({ "plugin": plugin, "panel": panel })),
        );
    }
    v.check("plugin-extensions");
}
