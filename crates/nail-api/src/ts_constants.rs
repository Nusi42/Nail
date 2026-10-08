//! Contract constants exported to TypeScript (wave Q2, SSOT / ODR / DRY,
//! ADR 0171): the UI and the plug-in SDK import them from the generated
//! `constants.ts` (`ui/src/api/generated/constants.ts`,
//! `packages/plugin-api/src/generated/constants.ts`) instead of repeating
//! them by hand. Plain JavaScript (the SDK's `tools/*.mjs`, `scripts/`)
//! reads the same values from `constants.json` next to it.
//!
//! To export a constant, add one [`Constants::add`] (or [`Constants::typed`]
//! when the value has a contract type) line to [`ui_constants`] (or to
//! [`plugin_constants`] for what plug-in code sees; the UI gets those too)
//! with the Rust constant as the value, then run `npm run gen:api`; the
//! `bindings` test fails while the checked-in files are stale. Any value that
//! serializes to JSON works (numbers, strings, lists, records).
//!
//! Every `labeled_enum!` exports `<NAME>_VALUES` (every value, in declaration
//! order) and `<NAME>_MESSAGE_IDS` (value → Fluent id), so the UI never lists
//! a contract enum's values or label ids by hand.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::labels::Labeled;

/// Every value of a contract enum that has no `ALL`, in the order listed:
/// `every!(TextOp: Contains, Equals)`. The match fails to compile while a
/// variant is missing, so the list cannot fall behind the enum.
macro_rules! every {
    ($ty:path : $($variant:ident),+ $(,)?) => {{
        type Enum = $ty;
        #[expect(dead_code, reason = "only compiled: the match is the exhaustiveness check")]
        const fn listed(value: Enum) {
            match value {
                $(Enum::$variant)|+ => {}
            }
        }
        [$(Enum::$variant),+]
    }};
}

/// The generated module's file name, next to the ts-rs types.
pub const TS_CONSTANTS_FILE: &str = "constants.ts";

/// The same values as JSON (`{ "NAME": value, … }`), for plain JavaScript.
pub const JSON_CONSTANTS_FILE: &str = "constants.json";

/// Constants of the whole API, for the UI: the plug-in constants and the
/// rest of the contract.
///
/// # Errors
///
/// When a value does not serialize to JSON.
pub fn ui_constants() -> Result<Constants, serde_json::Error> {
    let c = plugin_constants()?;
    let c = mail(c)?;
    let c = appearance(c)?;
    let c = shell(c)?;
    let c = windows(c)?;
    let c = remote(c)?;
    let c = portability(c)?;
    let c = security(c)?;
    let c = desktop(c)?;
    let c = providers(c)?;
    let c = enums(c)?;
    let c = agents(c)?;
    let c = triage(c)?;
    let c = sieve(c)?;
    labeled_enums(c)
}

/// Constants plug-in code sees (`@nail/plugin-api`): the manifest's,
/// hooks', extensions' and component trees' limits.
///
/// # Errors
///
/// When a value does not serialize to JSON.
pub fn plugin_constants() -> Result<Constants, serde_json::Error> {
    use crate::plugins::{admin, extensions, hooks, jobs, manifest, permission, ui};
    plugin_writes(Constants::default())?
        .add(
            "MAX_MANIFEST_BYTES",
            "Largest `manifest.json` (bytes).",
            &manifest::MAX_MANIFEST_BYTES,
        )?
        .add(
            "KEY_CONTEXTS",
            "Key-binding contexts of `layout.kdl` a plug-in key may use.",
            &manifest::KEY_CONTEXTS,
        )?
        .add(
            "MAX_PACKAGE_BYTES",
            "Largest `.nailplugin` archive (bytes), and largest unpacked size.",
            &admin::MAX_PACKAGE_BYTES,
        )?
        .add(
            "PACKAGE_EXTENSION",
            "File extension of plug-in packages (without the dot).",
            &admin::PACKAGE_EXTENSION,
        )?
        .add(
            "FUTURE_PERMISSIONS",
            "Permissions of the design that this version refuses (`permission-not-in-preview`).",
            &permission::FUTURE_PERMISSIONS,
        )?
        .add(
            "MAX_PREVIEW_INPUT",
            "Longest preview input a renderer gets unless it declares less (bytes).",
            &extensions::MAX_PREVIEW_INPUT,
        )?
        .add(
            "MAX_PREVIEW_IMAGE",
            "Largest image a preview may answer (bytes, decoded).",
            &extensions::MAX_PREVIEW_IMAGE,
        )?
        .add(
            "PREVIEW_IMAGE_TYPES",
            "Image types a preview may answer (no SVG: it can carry script).",
            &extensions::PREVIEW_IMAGE_TYPES,
        )?
        .add(
            "MAX_BLOCK_HTML",
            "Largest HTML a Markdown block may render to (bytes).",
            &extensions::MAX_BLOCK_HTML,
        )?
        .add(
            "MAX_MCP_TOOL_NAME",
            "Longest MCP tool name (what agents' APIs accept).",
            &extensions::MAX_MCP_TOOL_NAME,
        )?
        .add(
            "MAX_MCP_TOOL_SCHEMA",
            "Largest `inputSchema` of an MCP tool (bytes of JSON).",
            &extensions::MAX_MCP_TOOL_SCHEMA,
        )?
        .add(
            "HOOK_TIMEOUT_MS",
            "The budget of one hook call unless the manifest asks for less, per hook kind (ms).",
            &hooks::PluginHookKind::ALL
                .iter()
                .map(|&kind| (kind.as_str(), kind.default_timeout_ms()))
                .collect::<BTreeMap<_, _>>(),
        )?
        .add(
            "MIN_HOOK_TIMEOUT_MS",
            "Shortest hook budget a manifest may ask for (ms).",
            &hooks::MIN_HOOK_TIMEOUT_MS,
        )?
        .add(
            "MAX_HOOK_TIMEOUT_MS",
            "Longest hook budget a manifest may ask for (ms).",
            &hooks::MAX_HOOK_TIMEOUT_MS,
        )?
        .add(
            "MAX_ARRIVAL_ACTIONS",
            "Most actions one arrival hook call may answer.",
            &hooks::MAX_ARRIVAL_ACTIONS,
        )?
        .add(
            "MAX_ADDED_HEADERS",
            "Most headers one before-send call may add.",
            &hooks::MAX_ADDED_HEADERS,
        )?
        .add(
            "MAX_HEADER_VALUE",
            "Longest added header value (bytes; RFC 5322 line limit).",
            &hooks::MAX_HEADER_VALUE,
        )?
        .add(
            "MAX_HEADER_NAME",
            "Longest header name (bytes).",
            &hooks::MAX_HEADER_NAME,
        )?
        .add(
            "MAX_ADDED_CONTENT",
            "Largest content one before-send call may add (bytes of text + HTML).",
            &hooks::MAX_ADDED_CONTENT,
        )?
        .add(
            "MAX_HOOK_TEXT",
            "Longest veto reason, banner or badge tooltip text (characters).",
            &hooks::MAX_HOOK_TEXT,
        )?
        .add(
            "MAX_BADGE_TEXT",
            "Longest badge text (characters).",
            &hooks::MAX_BADGE_TEXT,
        )?
        .add(
            "MAX_OPEN_ITEMS",
            "Most banners and badges one open hook call may answer (each).",
            &hooks::MAX_OPEN_ITEMS,
        )?
        .add(
            "RESERVED_HEADER_PREFIX",
            "Header names plug-ins may never add (compared case-insensitively as a prefix).",
            &hooks::RESERVED_HEADER_PREFIX,
        )?
        .add(
            "JOB_WALL_CLOCK_MS",
            "Wall-clock limit of one job run (ms), pending promises included.",
            &jobs::JOB_WALL_CLOCK_MS,
        )?
        .add(
            "MIN_JOB_INTERVAL_MINUTES",
            "Shortest interval between two runs of a schedule (minutes).",
            &jobs::MIN_JOB_INTERVAL_MINUTES,
        )?
        .add(
            "MAX_UI_NODES",
            "Most nodes in one tree (a dialog body or a panel).",
            &ui::MAX_UI_NODES,
        )?
        .add(
            "MAX_UI_DEPTH",
            "Deepest nesting of `stack` nodes (a top-level node has depth 1).",
            &ui::MAX_UI_DEPTH,
        )?
        .add(
            "MAX_UI_ITEMS",
            "Most options, rows or messages of one node.",
            &ui::MAX_UI_ITEMS,
        )?
        .add(
            "IMAGE_TYPES",
            "Image types an `image` node may carry (`data:<type>;base64,…`; no SVG).",
            &ui::IMAGE_TYPES,
        )?
        .add(
            "MAX_ATTACHMENT_BYTES",
            "Largest attachment a draft may carry (bytes).",
            &crate::compose::MAX_ATTACHMENT_BYTES,
        )
}

fn mail(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::accounts::{MailProtocol, TlsMode, default_port};
    use crate::{mail_actions, plugins};
    let tls_modes = every!(TlsMode: Implicit, Starttls, Plaintext);
    let ports = every!(MailProtocol: Imap, Pop3, Smtp, Gmail, Graph, Ews)
        .iter()
        .map(|&protocol| {
            let by_tls = tls_modes
                .iter()
                .map(|&tls| Ok((wire_name(&tls)?, default_port(protocol, tls))))
                .collect::<Result<BTreeMap<_, _>, serde_json::Error>>()?;
            Ok((wire_name(&protocol)?, by_tls))
        })
        .collect::<Result<BTreeMap<_, _>, serde_json::Error>>()?;
    c.typed(
        "DEFAULT_PORTS",
        "The default port of each protocol for each TLS mode (`default_port`).",
        &ports,
        "Readonly<Record<MailProtocol, Readonly<Record<TlsMode, number>>>>",
        &["MailProtocol", "TlsMode"],
    )?
    .add(
        "MAX_MAIL_ACTION_TARGETS",
        "Most targets one `ApplyMailAction` may name.",
        &mail_actions::MAX_MAIL_ACTION_TARGETS,
    )?
    .add(
        "MAIL_UNDO_SECS",
        "How long a mail action can be undone, in seconds.",
        &mail_actions::MAIL_UNDO_SECS,
    )?
    .add(
        "PLUGIN_ACTION_PREFIX",
        "Prefix of plug-in action ids in `layout.kdl`.",
        &plugins::PLUGIN_ACTION_PREFIX,
    )?
    .add(
        "MAX_PLUGIN_NAME_LEN",
        "Longest plug-in id and contribution name (`MAX_NAME_LEN`).",
        &plugins::MAX_NAME_LEN,
    )
}

fn appearance(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::appearance as a;
    use crate::config::theme;
    c.add(
        "DEFAULT_LIGHT_THEME",
        "The light theme of a fresh install (and of Follow Windows by default).",
        &a::DEFAULT_LIGHT_THEME,
    )?
    .add(
        "DEFAULT_DARK_THEME",
        "The dark theme of a fresh install (and of Follow Windows by default).",
        &a::DEFAULT_DARK_THEME,
    )?
    .add(
        "HIGH_CONTRAST_THEME",
        "The theme Follow Windows uses while Windows is in a high-contrast mode.",
        &a::HIGH_CONTRAST_THEME,
    )?
    .typed(
        "BUILTIN_THEMES",
        "The built-in themes, in gallery order: `[id, kind]`.",
        &a::BUILTIN_THEMES,
        "readonly (readonly [string, ThemeKind])[]",
        &["ThemeKind"],
    )?
    .add(
        "MAX_THEME_ID_CHARS",
        "Longest theme id (file stem).",
        &a::MAX_THEME_ID_CHARS,
    )?
    .add(
        "MAX_THEME_NAME_CHARS",
        "Longest theme name the core accepts (`edit-issue-invalid-theme-name`).",
        &crate::editing::MAX_THEME_NAME_CHARS,
    )?
    .add(
        "FONT_SIZE_RANGE",
        "Smallest / largest body text size the overrides accept (px, inclusive).",
        &a::FONT_SIZE_RANGE,
    )?
    .add(
        "RADIUS_RANGE",
        "Smallest / largest corner radius (`radius.md`) the overrides accept (px, inclusive).",
        &a::RADIUS_RANGE,
    )?
    .add(
        "COMPACT_VARIANT",
        "The density variant of a token (`compact=`), a key of `ResolvedTheme::modes`.",
        &a::COMPACT_VARIANT,
    )?
    .add(
        "PREVIEW_TOKENS",
        "The colour tokens a theme card previews (`ThemeInfo::preview`).",
        &a::PREVIEW_TOKENS,
    )?
    .add(
        "PSEUDO_CLASS_STATES",
        "States that map to CSS pseudo-classes instead of `data-state` tokens.",
        &theme::PSEUDO_CLASS_STATES,
    )?
    .add(
        "LEGACY_ICON_NAMES",
        "Pre-M5 Nail icon names → Lucide names (only names that differ).",
        &crate::icons::LEGACY_ICON_NAMES
            .iter()
            .copied()
            .collect::<BTreeMap<_, _>>(),
    )
}

fn shell(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::{menus, views as v};
    c.add(
        "BUILTIN_VIEWS",
        "The views of the default layout, in rail order.",
        &v::BUILTIN_VIEWS,
    )?
    .add(
        "VIEW_SHOW_ACTION",
        "`view.show \"<view>\"`: show a view.",
        &v::VIEW_SHOW_ACTION,
    )?
    .add(
        "VIEW_NEXT_ACTION",
        "`view.next`: the next view on the rail (wraps around).",
        &v::VIEW_NEXT_ACTION,
    )?
    .add(
        "VIEW_PREVIOUS_ACTION",
        "`view.previous`: the previous view on the rail (wraps around).",
        &v::VIEW_PREVIOUS_ACTION,
    )?
    .add(
        "NAV_RAIL_WIDGET",
        "The navigation rail widget.",
        &v::NAV_RAIL_WIDGET,
    )?
    .add(
        "VIEW_HOST_WIDGET",
        "The view host widget (renders the active view's window tree).",
        &v::VIEW_HOST_WIDGET,
    )?
    .add(
        "SETTINGS_PAGE_WIDGET",
        "The settings page widget.",
        &v::SETTINGS_PAGE_WIDGET,
    )?
    .add(
        "LIST_HEADER_WIDGET",
        "The message list header widget (search, title with counts, filter chips).",
        &v::LIST_HEADER_WIDGET,
    )?
    .add(
        "SETTINGS_GROUPS",
        "The settings groups of the default layout, in navigation order.",
        &v::SETTINGS_GROUPS,
    )?
    .add(
        "SETTINGS_SECTION_ACTION",
        "`settings.section \"<id>\"`: show the settings page at a section.",
        &v::SETTINGS_SECTION_ACTION,
    )?
    .add(
        "SETTINGS_SEARCH_ACTION",
        "`settings.search`: focus the settings search.",
        &v::SETTINGS_SEARCH_ACTION,
    )?
    .add(
        "LIST_FILTER_ACTION",
        "`list.filter \"<filter>\"`: set the message list's filter chip.",
        &v::LIST_FILTER_ACTION,
    )?
    .add(
        "FOLDER_SECTION_TOGGLE_ACTION",
        "Collapse or expand a folder-pane section.",
        &v::FOLDER_SECTION_TOGGLE_ACTION,
    )?
    .add(
        "UI_STATE_COLLAPSED_SECTIONS",
        "UI-state key of the collapsed folder-pane sections.",
        &v::UI_STATE_COLLAPSED_SECTIONS,
    )?
    .add(
        "UI_STATE_VIEW",
        "UI-state key of the last view.",
        &v::UI_STATE_VIEW,
    )?
    .add(
        "UI_STATE_SETTINGS_SECTION",
        "UI-state key of the last settings section.",
        &v::UI_STATE_SETTINGS_SECTION,
    )?
    .typed(
        "DEFAULT_FAVORITES",
        "The Favorites of the default layout.",
        &v::DEFAULT_FAVORITES,
        "readonly FavoriteFolder[]",
        &["FavoriteFolder"],
    )?
    .typed(
        "LIST_FILTER_TERMS",
        "The search term of each list filter (`ListFilter::search_term`; `null` = none).",
        &v::ListFilter::ALL
            .iter()
            .map(|f| (f.as_str(), f.search_term()))
            .collect::<BTreeMap<_, _>>(),
        "Readonly<Record<ListFilter, string | null>>",
        &["ListFilter"],
    )?
    .add(
        "ACCOUNT_COLOR_SLOTS",
        "How many account colours a theme defines (`color.avatar-1` … `color.avatar-8`).",
        &v::ACCOUNT_COLOR_SLOTS,
    )?
    .add(
        "FAVORITES_SECTION_ID",
        "The section id of the Favorites section.",
        &v::FAVORITES_SECTION_ID,
    )?
    .add(
        "SMART_FOLDERS_SECTION_ID",
        "The section id of the smart-folders section.",
        &v::SMART_FOLDERS_SECTION_ID,
    )?
    .add(
        "CONTEXT_MENU_PREFIX",
        "Prefix of context-menu names in the layout.",
        &menus::CONTEXT_MENU_PREFIX,
    )?
    .add(
        "CONTEXT_MENU_PARTS",
        "Parts with context menus → the key context whose shortcuts they show.",
        &menus::CONTEXT_MENU_PARTS
            .iter()
            .map(|p| (p.part, p.key_context))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn windows(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::windows as w;
    c.add(
        "MAX_WINDOW_STATES",
        "Most window kinds remembered; a new kind beyond it is refused.",
        &w::MAX_WINDOW_STATES,
    )?
    .add(
        "MAX_WINDOW_KIND_CHARS",
        "Longest window kind.",
        &w::MAX_WINDOW_KIND_CHARS,
    )?
    .add(
        "MAX_WINDOW_INSTANCE_CHARS",
        "Longest instance key (the part after `:`).",
        &w::MAX_WINDOW_INSTANCE_CHARS,
    )?
    .add(
        "WINDOW_INSTANCE_SEPARATOR",
        "Separates kind and instance in a window id.",
        &w::WINDOW_INSTANCE_SEPARATOR,
    )?
    .add(
        "MIN_WINDOW_EDGE",
        "Smallest width or height of a window (and of a minimum size), in CSS pixels.",
        &w::MIN_WINDOW_EDGE,
    )?
    .add(
        "MAX_WINDOW_EDGE",
        "Largest width or height of a window, in CSS pixels.",
        &w::MAX_WINDOW_EDGE,
    )?
    .add(
        "MAX_WINDOW_COORDINATE",
        "Largest absolute x / y of a remembered window position.",
        &w::MAX_WINDOW_COORDINATE,
    )?
    .add(
        "WINDOW_CASCADE_OFFSET",
        "How far a new instance is moved from the previous one (right and down), in CSS pixels.",
        &w::WINDOW_CASCADE_OFFSET,
    )?
    .add(
        "WINDOW_KEYBOARD_STEP",
        "Keyboard move / resize step (arrow keys on a focused title bar).",
        &w::WINDOW_KEYBOARD_STEP,
    )?
    .add(
        "WINDOW_MIN_VISIBLE",
        "How much of a window stays visible when the page shrinks or it is moved off an edge.",
        &w::WINDOW_MIN_VISIBLE,
    )?
    .add(
        "MAX_POP_OUTS",
        "Most pop-out windows open at once.",
        &w::MAX_POP_OUTS,
    )?
    .add(
        "POP_OUT_ROUTE_PREFIX",
        "The UI route of a pop-out window: `#/window/<window id>`.",
        &w::POP_OUT_ROUTE_PREFIX,
    )?
    .add(
        "POP_OUT_LABEL_PREFIX",
        "The prefix of a pop-out's OS window label (Tauri).",
        &w::POP_OUT_LABEL_PREFIX,
    )?
    .add(
        "POP_OUT_CLOSE_EVENT",
        "The DOM event the desktop shell dispatches in a pop-out's page when the user closes its OS window.",
        &w::POP_OUT_CLOSE_EVENT,
    )?
    .typed(
        "DEFAULT_WINDOW_SPEC",
        "Every window that does not say otherwise (pop-outs are opt-in).",
        &w::DEFAULT_WINDOW_SPEC,
        "Readonly<WindowSpec>",
        &["WindowSpec"],
    )?
    .typed(
        "CONFIRMATION_WINDOW_SPEC",
        "Confirmations (the only modal windows).",
        &w::CONFIRMATION_WINDOW_SPEC,
        "Readonly<WindowSpec>",
        &["WindowSpec"],
    )
}

fn remote(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::{remote as r, remote_content as rc};
    c.add(
        "AUTH_PATH",
        "Path of the login protocol (`POST`).",
        &r::AUTH_PATH,
    )?
    .add(
        "NO_LOGIN_STATUS",
        "The answer to `POST AUTH_PATH` from a server without a login.",
        &r::NO_LOGIN_STATUS,
    )?
    .add(
        "CSRF_HEADER",
        "The header carrying the CSRF token.",
        &r::CSRF_HEADER,
    )?
    .add(
        "MAX_AUDIT_RETENTION_DAYS",
        "Largest `RemoteUiSettings::audit_retention_days` (ten years).",
        &r::MAX_AUDIT_RETENTION_DAYS,
    )?
    .add(
        "LOCAL_NETWORKS",
        "Local address ranges (CIDR): outside internet mode only peers from these may connect.",
        &r::local_networks(),
    )?
    .add(
        "PROXY_URL_SCHEME",
        "Scheme of proxied resource URLs in `sanitized_html`.",
        &rc::PROXY_URL_SCHEME,
    )?
    .add(
        "PROXY_PATH",
        "Route prefix of the remote-content proxy in web mode.",
        &rc::PROXY_PATH,
    )?
    .add(
        "RENDER_PARENT_PARAM",
        "Query parameter naming the parent origin of a sandbox-origin mail document.",
        &rc::RENDER_PARENT_PARAM,
    )?
    .add(
        "FRAME_HEIGHT_MESSAGE",
        "`type` of the only message a sandbox-origin mail document posts to its parent.",
        &rc::FRAME_HEIGHT_MESSAGE,
    )?
    .add(
        "MAX_FRAME_HEIGHT",
        "Tallest mail frame the reader sizes to its content (CSS px).",
        &rc::MAX_FRAME_HEIGHT,
    )
}

fn portability(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::{backup, diagnostics, portability as p, print};
    c.add(
        "LOCAL_FOLDERS_ACCOUNT_ID",
        "The account id of \"Local Folders\".",
        &p::LOCAL_FOLDERS_ACCOUNT_ID,
    )?
    .add(
        "BACKUP_FILE_EXTENSION",
        "File extension of backup archives (without the dot).",
        &backup::BACKUP_FILE_EXTENSION,
    )?
    .add(
        "MIN_BACKUP_PASSPHRASE_CHARS",
        "Shortest passphrase accepted for a new backup (characters).",
        &backup::MIN_BACKUP_PASSPHRASE_CHARS,
    )?
    .add(
        "MAX_BACKUP_PASSPHRASE_CHARS",
        "Longest backup passphrase accepted (characters).",
        &backup::MAX_BACKUP_PASSPHRASE_CHARS,
    )?
    .add(
        "MAX_DIAGNOSTICS_MESSAGES",
        "Most messages a diagnostics bundle may include.",
        &diagnostics::MAX_DIAGNOSTICS_MESSAGES,
    )?
    .add(
        "MAX_AGENDA_DAYS",
        "Longest agenda printed (days).",
        &print::MAX_AGENDA_DAYS,
    )?
    .typed(
        "DEFAULT_BACKUP_CONTENTS",
        "What a new backup holds until the user chooses (`BackupContents::default()`).",
        &backup::BackupContents::default(),
        "Readonly<BackupContents>",
        &["BackupContents"],
    )?
    .typed(
        "DEFAULT_DIAGNOSTICS_OPTIONS",
        "What a diagnostics bundle holds until the user chooses (`DiagnosticsOptions::default()`).",
        &diagnostics::DiagnosticsOptions::default(),
        "Readonly<DiagnosticsOptions>",
        &["DiagnosticsOptions"],
    )?
    .typed(
        "DEFAULT_PRINT_OPTIONS",
        "The print options of a new print job (`PrintOptions::default()`).",
        &print::PrintOptions::default(),
        "Readonly<PrintOptions>",
        &["PrintOptions"],
    )
}

fn security(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::{recovery, smartcard as s};
    c.add(
        "RECOVERY_KEY_BYTES",
        "Bytes of a recovery key.",
        &recovery::RECOVERY_KEY_BYTES,
    )?
    .add(
        "RECOVERY_KEY_CHARS",
        "Characters of the formatted recovery key without dashes.",
        &recovery::RECOVERY_KEY_CHARS,
    )?
    .add(
        "RECOVERY_KEY_DATA_CHARS",
        "Characters of a recovery key that carry the bytes (the rest is the check).",
        &recovery::DATA_CHARS,
    )?
    .add(
        "RECOVERY_KEY_GROUP",
        "Characters per dash-separated group of a formatted recovery key.",
        &recovery::GROUP,
    )?
    .add(
        "RECOVERY_KEY_ALPHABET",
        "The recovery key's alphabet (Crockford base32).",
        &recovery::alphabet(),
    )?
    .add(
        "PIN_PROMPT_TIMEOUT_SECS",
        "How long the core waits for a PIN (seconds).",
        &s::PIN_PROMPT_TIMEOUT_SECS,
    )?
    .add(
        "MIN_PIN_CHARS",
        "Shortest PIN passed to a card.",
        &s::MIN_PIN_CHARS,
    )?
    .add(
        "MAX_PIN_CHARS",
        "Longest PIN passed to a card.",
        &s::MAX_PIN_CHARS,
    )?
    .add(
        "MAX_PIN_CACHE_MINUTES",
        "Longest the core may keep a card logged in after a PIN (minutes).",
        &s::MAX_PIN_CACHE_MINUTES,
    )
}

fn desktop(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::{desktop as d, mapi};
    let registered = |schemes: bool| {
        d::DefaultAppTarget::ALL
            .iter()
            .filter(|t| t.is_scheme() == schemes)
            .map(wire_name)
            .collect::<Result<Vec<_>, _>>()
    };
    c.add(
        "REGISTERED_URL_SCHEMES",
        "URL schemes the installers register (`DefaultAppTarget::is_scheme`).",
        &registered(true)?,
    )?
    .add(
        "REGISTERED_FILE_EXTENSIONS",
        "File extensions (without the dot) the installers register.",
        &registered(false)?,
    )?
    .enum_table(
        "InstallKind",
        &d::InstallKind::ALL,
        d::InstallKind::message_id,
    )?
    .enum_table(
        "InstallScope",
        &d::InstallScope::ALL,
        d::InstallScope::message_id,
    )?
    .enum_table(
        "UpdateChannel",
        &d::UpdateChannel::ALL,
        d::UpdateChannel::message_id,
    )?
    .enum_table("UpdateDisabledReason", &d::UpdateDisabledReason::ALL, |r| {
        r.message_id().to_owned()
    })?
    .enum_table("UpdateFailure", &d::UpdateFailure::ALL, |f| {
        f.message_id().to_owned()
    })?
    .enum_table("DefaultAppTarget", &d::DefaultAppTarget::ALL, |t| {
        t.message_id().to_owned()
    })?
    .enum_table("DefaultAppState", &d::DefaultAppState::ALL, |s| {
        s.message_id().to_owned()
    })?
    .enum_table(
        "MapiResult",
        &mapi::MapiResult::ALL,
        mapi::MapiResult::message_id,
    )
}

fn providers(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::exchange::{self, oof};
    c.add(
        "MAX_CATEGORY_NAME",
        "Longest category name Exchange accepts (characters).",
        &exchange::MAX_CATEGORY_NAME,
    )?
    .add(
        "CATEGORY_PRESET_RGB",
        "Outlook's preset category colours (`CategoryColor::rgb`), `preset0` first.",
        &exchange::CategoryColor::ALL
            .iter()
            .filter_map(|c| c.rgb())
            .collect::<Vec<_>>(),
    )?
    .add(
        "MAX_REPLY_CHARS",
        "Longest automatic reply (characters).",
        &oof::MAX_REPLY_CHARS,
    )?
    .add(
        "MAX_LABEL_NAME",
        "Longest label name Gmail accepts (characters).",
        &crate::gmail::MAX_LABEL_NAME,
    )?
    .add(
        "GMAIL_LABEL_PALETTE",
        "The colours Gmail accepts for labels (background and text alike, lower case).",
        crate::gmail::LABEL_PALETTE.as_slice(),
    )
}

/// The Fluent ids of agent and plug-in writes (`nail_api::agents`; the
/// rules are plug-in constants, [`plugin_writes`]).
fn agents(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::agents::{ActionKind, ConfirmationState};
    c.enum_message_ids("ActionKind", &ActionKind::ALL, ActionKind::message_id)?
        .enum_table(
            "ConfirmationState",
            &ConfirmationState::ALL,
            ConfirmationState::message_id,
        )
}

/// What plug-in code may ask for and how its writes are treated: the
/// permissions and their relations, hook and job names, MCP scopes, and the
/// rules of each kind of write (`nail_api::agents`).
fn plugin_writes(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::agents::ActionKind;
    use crate::plugins::PluginPermission;
    use crate::plugins::hooks::PluginHookKind;
    use crate::plugins::jobs::PluginEventName;
    let names = |filter: fn(&PluginPermission) -> bool| {
        PluginPermission::FIXED
            .iter()
            .filter(|p| filter(p))
            .map(PluginPermission::name)
            .collect::<Vec<_>>()
    };
    let required = PluginPermission::FIXED
        .iter()
        .filter(|p| !p.requires().is_empty())
        .map(|p| {
            (
                p.name(),
                p.requires()
                    .iter()
                    .map(PluginPermission::name)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let hooks = PluginHookKind::ALL
        .iter()
        .map(|&kind| (kind.as_str(), kind.permission().name()))
        .collect::<BTreeMap<_, _>>();
    let rules = ActionKind::ALL
        .iter()
        .map(|&kind| {
            let rule = serde_json::json!({
                "destructive": kind.is_destructive(),
                "outgoing": kind.is_outgoing(),
                "can_always_allow": kind.can_always_allow(),
                "default_mode": kind.default_mode(),
            });
            (kind.as_str(), rule)
        })
        .collect::<BTreeMap<_, _>>();
    c.typed(
        "PLUGIN_PERMISSIONS",
        "Every permission Nail offers (plus `network:<host>`), in display order.",
        &names(|_| true),
        "readonly PluginPermission[]",
        &["PluginPermission"],
    )?
    .add(
        "REQUIRED_PERMISSIONS",
        "Permissions another one needs in the same manifest (`missing-required-permission`).",
        &required,
    )?
    .typed(
        "ACCOUNT_PERMISSIONS",
        "Permissions a grant can limit to accounts (`PluginPermission::takes_accounts`).",
        &names(PluginPermission::takes_accounts),
        "readonly PluginPermission[]",
        &["PluginPermission"],
    )?
    .typed(
        "WRITE_PERMISSIONS",
        "Permissions that let a plug-in change the user's data (`PluginPermission::is_write`).",
        &names(PluginPermission::is_write),
        "readonly PluginPermission[]",
        &["PluginPermission"],
    )?
    .typed(
        "HOOK_PERMISSIONS",
        "The permission each kind of hook needs.",
        &hooks,
        "Readonly<Record<PluginHookKind, PluginPermission>>",
        &["PluginHookKind", "PluginPermission"],
    )?
    .enum_values("PluginEventName", &PluginEventName::ALL)?
    .enum_values("McpScope", &crate::mcp::McpScope::ALL)?
    .enum_values("ActionKind", &ActionKind::ALL)?
    .typed(
        "ACTION_KIND_RULES",
        "Per kind of write: destructive (always asks), outgoing (sends mail), whether \"always allow\" may be set, the policy until the user changes it.",
        &rules,
        "Readonly<Record<ActionKind, { destructive: boolean; outgoing: boolean; can_always_allow: boolean; default_mode: ActionPolicyMode }>>",
        &["ActionKind", "ActionPolicyMode"],
    )
}

/// Junk, snooze and notification settings (`nail_api::triage`).
fn triage(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::triage::{
        JunkIssue, JunkSettings, JunkSource, NotificationIssue, NotificationSettings, Weekday,
    };
    c.typed(
        "DEFAULT_JUNK_SETTINGS",
        "The junk settings until the user changes them (`JunkSettings::default()`).",
        &JunkSettings::default(),
        "Readonly<JunkSettings>",
        &["JunkSettings"],
    )?
    .typed(
        "DEFAULT_NOTIFICATION_SETTINGS",
        "The notification settings until the user changes them (`NotificationSettings::default()`).",
        &NotificationSettings::default(),
        "Readonly<NotificationSettings>",
        &["NotificationSettings"],
    )?
    .enum_table("JunkIssue", &JunkIssue::ALL, |i| i.message_id().to_owned())?
    .enum_table("NotificationIssue", &NotificationIssue::ALL, |i| {
        i.message_id().to_owned()
    })?
    .enum_values("JunkSource", &every!(JunkSource: Server, Classifier, User))?
    .enum_values("Weekday", &Weekday::ALL)
}

/// Server-side rules and vacation replies (`nail_api::sieve`).
fn sieve(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::sieve::{self as s, RulePlacement, SieveState, VacationIssue, VacationSettings};
    c.typed(
        "DEFAULT_VACATION",
        "The vacation reply until the user changes it (`VacationSettings::default()`).",
        &VacationSettings::default(),
        "Readonly<VacationSettings>",
        &["VacationSettings"],
    )?
    .add(
        "MAX_SCRIPT_NAME_LEN",
        "Longest Sieve script name (characters).",
        &s::MAX_SCRIPT_NAME_LEN,
    )?
    .enum_table("VacationIssue", &VacationIssue::ALL, |i| {
        i.message_id().to_owned()
    })?
    .enum_values(
        "SieveState",
        &every!(SieveState: Disabled, Unavailable, Pending, Uploading, InSync, Failed),
    )?
    .enum_values(
        "RulePlacement",
        &every!(RulePlacement: Server, Client, NotExpressible),
    )
}

/// Value lists of contract enums the UI offers in full.
fn enums(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::config::theme::ThemeMode;
    use crate::domain::FolderRole;
    use crate::remote::{RemoteAccessPreset, RemoteAuthMethod, SessionEndReason};
    use crate::rules::{AddressField, AuthMethod, AuthResult, MessageFlag, TextOp};
    c.enum_values(
        "TextOp",
        &every!(TextOp: Contains, Equals, StartsWith, EndsWith, Regex),
    )?
    .enum_values(
        "AddressField",
        &every!(AddressField: From, To, Cc, Recipients, ReplyTo, Sender),
    )?
    .enum_values(
        "MessageFlag",
        &every!(MessageFlag: Seen, Flagged, Answered, Draft),
    )?
    .enum_values(
        "FolderRole",
        &every!(FolderRole: Inbox, Sent, Drafts, Archive, Junk, Trash, Other),
    )?
    .enum_values("AuthMethod", &every!(AuthMethod: Dkim, Spf, Dmarc))?
    .enum_values(
        "AuthResult",
        &every!(AuthResult: Pass, Fail, Softfail, Neutral, None, Temperror, Permerror),
    )?
    .enum_values(
        "RemoteAuthMethod",
        &every!(RemoteAuthMethod: Passkey, Password, PasswordTotp, Pairing),
    )?
    .enum_values(
        "SessionEndReason",
        &every!(SessionEndReason: Logout, Revoked, Expired, DeviceRevoked, UserChanged),
    )?
    .enum_values("RemoteAccessPreset", &RemoteAccessPreset::ALL)?
    .enum_values(
        "ComposerMode",
        &every!(crate::accounts::ComposerMode: Plain, Html, Markdown),
    )?
    .enum_values(
        "ThemeMode",
        &every!(ThemeMode: System, Light, Dark, HighContrast),
    )
}

fn labeled_enums(c: Constants) -> Result<Constants, serde_json::Error> {
    use crate::{
        appearance, backup, diagnostics, editing, mail_actions, palette, portability, print,
        recovery, smartcard, ui_state, views, windows,
    };
    c.labeled::<appearance::ThemeKind>()?
        .labeled::<appearance::ThemeSource>()?
        .labeled::<appearance::Density>()?
        .labeled::<appearance::ColorScheme>()?
        .labeled::<appearance::ThemeOverridesIssue>()?
        .labeled::<appearance::ThemeProblem>()?
        .labeled::<backup::BackupItem>()?
        .labeled::<backup::BackupIssue>()?
        .labeled::<backup::RestoreEffect>()?
        .labeled::<diagnostics::DiagnosticsItem>()?
        .labeled::<diagnostics::Redaction>()?
        .labeled::<diagnostics::DiagnosticsIssue>()?
        .labeled::<editing::HistoryScope>()?
        .labeled::<editing::EditKind>()?
        .labeled::<editing::EditIssue>()?
        .labeled::<mail_actions::MailActionKind>()?
        .labeled::<mail_actions::MailActionIssue>()?
        .labeled::<palette::ActionAvailability>()?
        .labeled::<portability::ImportFormat>()?
        .labeled::<portability::ExportFormat>()?
        .labeled::<portability::MboxVariant>()?
        .labeled::<portability::ImportCollectionKind>()?
        .labeled::<portability::JobKind>()?
        .labeled::<portability::JobPhase>()?
        .labeled::<portability::JobState>()?
        .labeled::<portability::IssueSeverity>()?
        .labeled::<portability::PortabilityIssueCode>()?
        .labeled::<print::AgendaLayout>()?
        .labeled::<print::PrintTargetIssue>()?
        .labeled::<print::PageSize>()?
        .labeled::<print::PageOrientation>()?
        .labeled::<print::HeaderDetail>()?
        .labeled::<recovery::RecoveryKeyIssue>()?
        .labeled::<smartcard::SmartcardSettingsIssue>()?
        .labeled::<smartcard::SmartcardObjectKind>()?
        .labeled::<smartcard::SmartcardKeyUsage>()?
        .labeled::<smartcard::PinPurpose>()?
        .labeled::<smartcard::PinPromptCloseReason>()?
        .labeled::<smartcard::SmartcardError>()?
        .labeled::<ui_state::UiStateIssue>()?
        .labeled::<views::ListFilter>()?
        .labeled::<views::FavoriteFolder>()?
        .labeled::<views::FolderPaneSectionKind>()?
        .labeled::<windows::WindowSnap>()?
        .labeled::<windows::WindowInstances>()?
        .labeled::<windows::WindowFrameIssue>()?
        .labeled::<windows::WindowStateIssue>()?
        .labeled::<windows::PopOutChange>()?
        .labeled::<windows::PopOutIssue>()
}

/// One generated constant.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    doc: String,
    json: String,
    /// The contract type the value satisfies (`satisfies <type>`), if any.
    ts_type: Option<String>,
}

/// A list of named constants, rendered as one TypeScript module (and as
/// JSON).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Constants {
    entries: Vec<Entry>,
    /// Generated types the module imports (for `satisfies`).
    imports: std::collections::BTreeSet<String>,
}

impl Constants {
    /// Adds `name` with its documentation and value.
    ///
    /// # Errors
    ///
    /// When the value does not serialize to JSON.
    pub fn add<T: Serialize + ?Sized>(
        self,
        name: &str,
        doc: &str,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        self.push(name, doc, value, None, &[])
    }

    /// Adds `name` like [`Self::add`], checked against a TypeScript type
    /// (`as const satisfies <ts_type>`) built from the generated `imports`.
    ///
    /// # Errors
    ///
    /// When the value does not serialize to JSON.
    pub fn typed<T: Serialize + ?Sized>(
        self,
        name: &str,
        doc: &str,
        value: &T,
        ts_type: &str,
        imports: &[&str],
    ) -> Result<Self, serde_json::Error> {
        self.push(name, doc, value, Some(ts_type), imports)
    }

    fn push<T: Serialize + ?Sized>(
        mut self,
        name: &str,
        doc: &str,
        value: &T,
        ts_type: Option<&str>,
        imports: &[&str],
    ) -> Result<Self, serde_json::Error> {
        self.entries.push(Entry {
            name: name.to_owned(),
            doc: doc.to_owned(),
            json: serde_json::to_string(value)?,
            ts_type: ts_type.map(str::to_owned),
        });
        self.imports.extend(imports.iter().map(|&i| i.to_owned()));
        Ok(self)
    }

    /// Adds the value list `<NAME>_VALUES` and the Fluent id table
    /// `<NAME>_MESSAGE_IDS` of the contract enum `ty` (its TypeScript name).
    ///
    /// # Errors
    ///
    /// When a value does not serialize to JSON.
    pub fn enum_table<T: Serialize + Copy>(
        self,
        ty: &str,
        all: &[T],
        message_id: impl Fn(T) -> String,
    ) -> Result<Self, serde_json::Error> {
        self.enum_values(ty, all)?
            .enum_message_ids(ty, all, message_id)
    }

    /// Adds the Fluent id table `<NAME>_MESSAGE_IDS` of the contract enum
    /// `ty` (its TypeScript name).
    ///
    /// # Errors
    ///
    /// When a value does not serialize to JSON.
    pub fn enum_message_ids<T: Serialize + Copy>(
        self,
        ty: &str,
        all: &[T],
        message_id: impl Fn(T) -> String,
    ) -> Result<Self, serde_json::Error> {
        let ids = all
            .iter()
            .map(|&v| Ok((wire_name(&v)?, message_id(v))))
            .collect::<Result<BTreeMap<_, _>, serde_json::Error>>()?;
        self.typed(
            &format!("{}_MESSAGE_IDS", screaming_snake(ty)),
            &format!("The Fluent id of each `{ty}`'s label."),
            &ids,
            &format!("Readonly<Record<{ty}, string>>"),
            &[ty],
        )
    }

    /// Adds the value list `<NAME>_VALUES` of the contract enum `ty` (its
    /// TypeScript name).
    ///
    /// # Errors
    ///
    /// When a value does not serialize to JSON.
    pub fn enum_values<T: Serialize>(self, ty: &str, all: &[T]) -> Result<Self, serde_json::Error> {
        self.typed(
            &format!("{}_VALUES", screaming_snake(ty)),
            &format!("Every `{ty}`, in the contract's order."),
            all,
            &format!("readonly {ty}[]"),
            &[ty],
        )
    }

    /// [`Self::enum_table`] of a `labeled_enum!`.
    ///
    /// # Errors
    ///
    /// When a value does not serialize to JSON.
    pub(crate) fn labeled<T: Labeled>(self) -> Result<Self, serde_json::Error> {
        self.enum_table(T::NAME, T::VALUES, T::label_id)
    }

    /// Whether no constant was added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The names, in the order added.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// The TypeScript module: one documented `export const … as const` per
    /// constant (`satisfies` its contract type where it has one).
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::from(
            "// This file was generated by nail-api (ts_constants.rs). Do not edit this file manually.\n",
        );
        if !self.imports.is_empty() {
            out.push('\n');
            for ty in &self.imports {
                out.push_str("import type { ");
                out.push_str(ty);
                out.push_str(" } from \"./");
                out.push_str(ty);
                out.push_str("\";\n");
            }
        }
        for entry in &self.entries {
            out.push_str("\n/** ");
            out.push_str(&entry.doc);
            out.push_str(" */\nexport const ");
            out.push_str(&entry.name);
            out.push_str(" = ");
            out.push_str(&entry.json);
            out.push_str(" as const");
            if let Some(ty) = &entry.ts_type {
                out.push_str(" satisfies ");
                out.push_str(ty);
            }
            out.push_str(";\n");
        }
        out
    }

    /// The same values as one JSON object (`{ "NAME": value }`, in the
    /// order added), for plain JavaScript.
    #[must_use]
    pub fn render_json(&self) -> String {
        let mut out = String::from("{\n");
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push_str(",\n");
            }
            out.push_str("  ");
            out.push_str(&serde_json::Value::String(entry.name.clone()).to_string());
            out.push_str(": ");
            out.push_str(&entry.json);
        }
        out.push_str("\n}\n");
        out
    }

    /// Writes [`TS_CONSTANTS_FILE`] and [`JSON_CONSTANTS_FILE`] into
    /// `out_dir` (nothing when empty).
    ///
    /// # Errors
    ///
    /// When a file cannot be written.
    pub fn write(&self, out_dir: &Path) -> std::io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        std::fs::write(out_dir.join(TS_CONSTANTS_FILE), self.render())?;
        std::fs::write(out_dir.join(JSON_CONSTANTS_FILE), self.render_json())
    }
}

/// The wire name of a unit enum value (what serde writes).
fn wire_name<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(serde::ser::Error::custom(format!(
            "not a unit enum value: {other}"
        ))),
    }
}

/// `ImportFormat` → `IMPORT_FORMAT`.
fn screaming_snake(camel: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_documented_constants() {
        let constants = Constants::default()
            .add("LIMIT", "A limit.", &500_usize)
            .unwrap()
            .add("NAMES", "Some names.", &["a", "b"])
            .unwrap();
        assert_eq!(
            constants.render(),
            "// This file was generated by nail-api (ts_constants.rs). Do not edit this file manually.\n\
             \n/** A limit. */\nexport const LIMIT = 500 as const;\n\
             \n/** Some names. */\nexport const NAMES = [\"a\",\"b\"] as const;\n"
        );
        assert_eq!(
            constants.render_json(),
            "{\n  \"LIMIT\": 500,\n  \"NAMES\": [\"a\",\"b\"]\n}\n"
        );
    }

    #[test]
    fn typed_constants_import_and_satisfy_their_type() {
        let constants = Constants::default()
            .typed("SPEC", "A spec.", &[1, 2], "readonly Size[]", &["Size"])
            .unwrap();
        assert_eq!(
            constants.render(),
            "// This file was generated by nail-api (ts_constants.rs). Do not edit this file manually.\n\
             \nimport type { Size } from \"./Size\";\n\
             \n/** A spec. */\nexport const SPEC = [1,2] as const satisfies readonly Size[];\n"
        );
    }

    #[test]
    fn enum_tables_list_values_and_message_ids() {
        let c = Constants::default()
            .labeled::<crate::print::PageOrientation>()
            .unwrap();
        let text = c.render();
        assert!(
            text.contains(
                "export const PAGE_ORIENTATION_VALUES = [\"portrait\",\"landscape\"] as const satisfies readonly PageOrientation[];"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "export const PAGE_ORIENTATION_MESSAGE_IDS = {\"landscape\":\"print-orientation-landscape\",\"portrait\":\"print-orientation-portrait\"} as const satisfies Readonly<Record<PageOrientation, string>>;"
            ),
            "{text}"
        );
        assert_eq!(
            screaming_snake("PinPromptCloseReason"),
            "PIN_PROMPT_CLOSE_REASON"
        );
    }

    #[test]
    fn an_empty_list_writes_no_file() {
        let dir = tempfile::tempdir().unwrap();
        Constants::default().write(dir.path()).unwrap();
        assert!(!dir.path().join(TS_CONSTANTS_FILE).exists());
        assert!(!dir.path().join(JSON_CONSTANTS_FILE).exists());
    }

    #[test]
    fn exports_the_contract_values() {
        let ui = ui_constants().unwrap();
        let text = ui.render();
        assert!(text.contains(&format!(
            "export const MAX_MAIL_ACTION_TARGETS = {} as const;",
            crate::mail_actions::MAX_MAIL_ACTION_TARGETS
        )));
        // The UI gets every plug-in constant too, each name once.
        let plugin = plugin_constants().unwrap();
        let names: Vec<_> = ui.names().collect();
        assert!(plugin.names().all(|n| names.contains(&n)));
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        assert!(text.contains("export const LOCAL_NETWORKS = [\"127.0.0.0/8\""));
        assert!(
            text.contains(
                "export const RECOVERY_KEY_ALPHABET = \"0123456789ABCDEFGHJKMNPQRSTVWXYZ\""
            )
        );
    }
}
