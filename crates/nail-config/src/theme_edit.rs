//! The theme editor's operations (M5d, ADR 0131): a theme's tokens and part
//! rules ([`ThemeDetails`]), token and part-override edits ([`ThemePatch`])
//! on the user's `theme.kdl` or a user theme, and duplicating / renaming /
//! deleting user themes.
//!
//! Every write is format and comment preserving ([`crate::kdl_edit`]),
//! happens under the write lock, checks the editor's `base_revision` and is
//! one entry of the `appearance` undo history with a [`FileChange`] per file
//! it touched (a renamed theme: the old file, the new file, `theme.kdl` when
//! the selection followed, and themes that extended it).
//!
//! Token edits ([`ThemePatch::SetToken`]):
//!
//! * a base value is written as the token line's value
//!   (`<category> "<name>" <value>`) in place, or as a new line in the first
//!   `tokens` block (created when there is none);
//! * a mode value (`dark`, `high-contrast`, `print`, `compact`) becomes the
//!   token line's property (`dark=…`), or — when the file has no line for
//!   the token — a line in `modes { <mode> { … } }`;
//! * removing a base value removes the token from the file (its line and its
//!   `modes` lines): the extended theme's value applies again; removing a
//!   mode value removes only that property / line. Emptied `modes` blocks go.
//!
//! Part edits set or remove one declaration of the file's (last) rule for a
//! selector; a rule left empty is removed, as is a `parts` block left empty.
//! Values are written as KDL numbers when they are plain numbers (so `px`
//! is added where the category or property wants it), else as strings.

use kdl::{KdlDocument, KdlNode, KdlValue};
use nail_api::appearance::{
    APPEARANCE_NODE, Appearance, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, HIGH_CONTRAST_THEME,
    MAX_THEME_ID_CHARS, ThemeKind, ThemeSource, is_builtin_theme, is_theme_id,
};
use nail_api::editing::{
    EditIssue, EditKind, HistoryScope, ThemeDetails, ThemeEditTarget, ThemePartRule, ThemePatch,
    ThemeTokenValue, ValueOrigin, config_revision, is_theme_name,
};
use nail_api::{ApiError, ConfigFile, ErrorCode, LocalizedMessage};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::appearance_edit::ThemeEditor;
use crate::backup;
use crate::edit_history::FileChange;
use crate::kdl_edit::{
    Style, append_rendered, block_of, ensure_multiline, first_arg, remove_node, remove_prop,
    replace_rendered, set_arg, set_first_arg, set_prop, str_prop,
};
use crate::kdl_util::children;
use crate::kdl_write::quote;
use crate::manager::{ConfigManager, ConfigUpdate};
use crate::theme::{DEFAULT_BASE, ThemeFile};
use crate::themes::{
    ThemeCatalog, UserThemeText, builtin_theme, read_user_themes, user_theme_file,
};

fn error(code: ErrorCode, localized: LocalizedMessage) -> ApiError {
    ApiError::new(code, crate::l10n::english(&localized)).with_localized(localized)
}

fn theme_error(code: ErrorCode, issue: EditIssue, theme_id: &str) -> ApiError {
    error(
        code,
        LocalizedMessage::new(issue.message_id()).arg("theme", theme_id),
    )
}

fn issue(issue: EditIssue) -> LocalizedMessage {
    LocalizedMessage::new(issue.message_id())
}

fn io_error(file: &str, e: &std::io::Error) -> ApiError {
    crate::error::ConfigError::io(file, e).into()
}

/// The modes a token edit may name (`light` is the base value).
const TOKEN_MODES: &[&str] = &["dark", "high-contrast", "print", "compact"];

/// Whether a value is a plain decimal number (`12`, `-1`, `1.5`).
fn is_plain_number(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let mut parts = digits.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
}

/// A value as a KDL value: a number when it is a plain number, else a string.
fn typed(value: &str) -> KdlValue {
    if is_plain_number(value) {
        if let Ok(i) = value.parse::<i128>() {
            return KdlValue::Integer(i);
        }
        if let Ok(f) = value.parse::<f64>() {
            return KdlValue::Float(f);
        }
    }
    KdlValue::String(value.to_owned())
}

/// A value as KDL source text ([`typed`]).
fn value_source(value: &str) -> String {
    if is_plain_number(value) {
        value.to_owned()
    } else {
        quote(value)
    }
}

/// Reads a snippet of theme sections on its own: the config's first error.
fn check_snippet(sections: &str) -> Result<(), String> {
    let text = format!("theme \"check\" extends=\"default\" {{\n{sections}}}\n");
    ThemeFile::parse_all(&text, "theme.kdl")
        .map(drop)
        .map_err(|errors| errors.first().map(ToString::to_string).unwrap_or_default())
}

/// Checks one patch on its own (category, name, mode, selector, property
/// and the value as KDL would read it).
fn check_patch(patch: &ThemePatch) -> Result<(), ApiError> {
    let refuse = |which: EditIssue, detail: String| {
        Err(error(
            ErrorCode::InvalidRequest,
            issue(which).arg("detail", detail),
        ))
    };
    match patch {
        ThemePatch::SetToken {
            category,
            name,
            value,
            mode,
        } => {
            if let Some(mode) = mode
                && !TOKEN_MODES.contains(&mode.as_str())
            {
                return refuse(EditIssue::InvalidToken, mode.clone());
            }
            let value = value_source(value.as_deref().unwrap_or("x"));
            let line = match mode {
                None => format!("{category} {} {value}", quote(name)),
                Some(mode) => format!("{category} {} \"x\" {mode}={value}", quote(name)),
            };
            match check_snippet(&format!("    tokens {{\n        {line}\n    }}\n")) {
                Ok(()) => Ok(()),
                Err(detail) => refuse(EditIssue::InvalidToken, detail),
            }
        }
        ThemePatch::SetPartProperty {
            selector,
            property,
            value,
        } => {
            let value = value_source(value.as_deref().unwrap_or("inherit"));
            let rule = format!(
                "    part {} {{\n        {property} {value}\n    }}\n",
                quote(selector)
            );
            match check_snippet(&rule) {
                Ok(()) => Ok(()),
                Err(detail) => refuse(EditIssue::InvalidPart, detail),
            }
        }
        ThemePatch::ResetPart { selector } => match crate::selector::parse_selector(selector) {
            Ok(_) => Ok(()),
            Err(message) => refuse(EditIssue::InvalidPart, crate::l10n::english(&message)),
        },
    }
}

fn is_named(node: &KdlNode, name: &str) -> bool {
    node.name().value() == name
}

/// Removes the nodes at `indexes` (ascending) from a block at `depth`.
fn remove_all(block: &mut KdlDocument, indexes: &[usize], depth: usize, style: &Style) {
    if indexes.is_empty() {
        return;
    }
    ensure_multiline(block, depth, style);
    for &i in indexes.iter().rev() {
        remove_node(block, i, style);
    }
}

/// Replaces line `index` of a block at `depth` with a rendered line.
fn replace_line(block: &mut KdlDocument, index: usize, line: &str, depth: usize, style: &Style) {
    ensure_multiline(block, depth, style);
    replace_rendered(
        block,
        index,
        &format!("{}{line}{}", style.pad(depth), style.nl),
    );
    // A line that ended an inline block keeps its line break in `trailing`;
    // the rendered line brings its own.
    if let Some(format) = block.nodes_mut()[index].format_mut()
        && format.terminator.ends_with('\n')
        && format.trailing.trim().is_empty()
    {
        format.trailing.clear();
    }
}

/// Appends a rendered line to the children of `node` (children at `depth`).
fn append_line(node: &mut KdlNode, line: &str, depth: usize, style: &Style) {
    let text = format!("{}{line}{}", style.pad(depth), style.nl);
    append_rendered(block_of(node), depth, &text, style);
}

/// A rendered block `name { line }` for `depth`.
fn block_text(name: &str, line: &str, depth: usize, style: &Style) -> String {
    format!(
        "{pad}{name} {{{nl}{inner}{line}{nl}{pad}}}{nl}",
        pad = style.pad(depth),
        inner = style.pad(depth + 1),
        nl = style.nl
    )
}

/// Where a token's lines are: `(block, line)` in `tokens` blocks and
/// `(modes block, mode block, line)` in `modes` (only `mode`'s when given).
struct TokenSites {
    lines: Vec<(usize, usize)>,
    mode_lines: Vec<(usize, usize, usize)>,
}

fn token_sites(body: &KdlDocument, category: &str, name: &str, mode: Option<&str>) -> TokenSites {
    let is_token = |n: &KdlNode| is_named(n, category) && first_arg(n) == Some(name);
    let mut sites = TokenSites {
        lines: Vec::new(),
        mode_lines: Vec::new(),
    };
    for (b, block) in body.nodes().iter().enumerate() {
        if is_named(block, "tokens") {
            for (t, token) in children(block).iter().enumerate() {
                if is_token(token) {
                    sites.lines.push((b, t));
                }
            }
        } else if is_named(block, "modes") {
            for (m, mode_block) in children(block).iter().enumerate() {
                if mode.is_some_and(|mode| !is_named(mode_block, mode)) {
                    continue;
                }
                for (t, token) in children(mode_block).iter().enumerate() {
                    if is_token(token) {
                        sites.mode_lines.push((b, m, t));
                    }
                }
            }
        }
    }
    sites
}

impl ThemeEditor {
    /// [`ThemePatch::SetToken`].
    fn set_token(&mut self, category: &str, name: &str, value: Option<&str>, mode: Option<&str>) {
        let style = self.style.clone();
        let body = self.body();
        let sites = token_sites(body, category, name, mode);
        let line = |v: &str| format!("{category} {} {}", quote(name), value_source(v));
        match (mode, value) {
            (None, Some(v)) => {
                if let Some(&(b, t)) = sites.lines.last() {
                    let token = &mut block_of(&mut body.nodes_mut()[b]).nodes_mut()[t];
                    set_arg(token, 1, typed(v));
                } else {
                    append_token_line(body, &line(v), &style);
                }
            }
            (Some(m), Some(v)) => {
                if let Some(&(b, t)) = sites.lines.last() {
                    let token = &mut block_of(&mut body.nodes_mut()[b]).nodes_mut()[t];
                    set_prop(token, m, typed(v));
                } else if let Some(&(b, mb, t)) = sites.mode_lines.last() {
                    let mode_block = &mut block_of(&mut body.nodes_mut()[b]).nodes_mut()[mb];
                    set_arg(&mut block_of(mode_block).nodes_mut()[t], 1, typed(v));
                } else {
                    append_mode_line(body, m, &line(v), &style);
                }
            }
            (None, None) => {
                remove_lines(body, &sites.lines, &style);
                remove_mode_lines(body, &sites.mode_lines, &style);
            }
            (Some(m), None) => {
                for &(b, t) in &sites.lines {
                    remove_prop(&mut block_of(&mut body.nodes_mut()[b]).nodes_mut()[t], m);
                }
                remove_mode_lines(body, &sites.mode_lines, &style);
            }
        }
    }

    /// [`ThemePatch::SetPartProperty`].
    fn set_part_property(&mut self, selector: &str, property: &str, value: Option<&str>) {
        let style = self.style.clone();
        let body = self.body();
        let rules = part_sites(body, selector);
        if let Some(v) = value {
            let line = format!("{property} {}", value_source(v));
            if let Some(&(parts, index)) = rules.last() {
                let depth = if parts.is_some() { 3 } else { 2 };
                let rule = rule_mut(body, parts, index);
                let existing = children(rule).iter().rposition(|d| is_named(d, property));
                match existing {
                    Some(d) => replace_line(block_of(rule), d, &line, depth, &style),
                    None => append_line(rule, &line, depth, &style),
                }
            } else {
                let text = block_text(&format!("part {}", quote(selector)), &line, 1, &style);
                append_rendered(body, 1, &text, &style);
            }
            return;
        }
        let mut emptied = Vec::new();
        for &(parts, index) in rules.iter().rev() {
            let depth = if parts.is_some() { 3 } else { 2 };
            let block = block_of(rule_mut(body, parts, index));
            let matching: Vec<usize> = block
                .nodes()
                .iter()
                .enumerate()
                .filter(|(_, d)| is_named(d, property))
                .map(|(i, _)| i)
                .collect();
            remove_all(block, &matching, depth, &style);
            if block.nodes().is_empty() {
                emptied.push((parts, index));
            }
        }
        remove_rules(body, &emptied, &style);
    }

    /// [`ThemePatch::ResetPart`].
    fn reset_part(&mut self, selector: &str) {
        let style = self.style.clone();
        let body = self.body();
        let mut rules = part_sites(body, selector);
        rules.reverse();
        remove_rules(body, &rules, &style);
    }

    /// Sets the theme's display name (`theme "<name>"`).
    fn set_name(&mut self, name: &str) {
        set_first_arg(self.root_mut(), KdlValue::String(name.to_owned()));
    }

    /// Points the selection away from a theme: `extends` and the
    /// `appearance` node's `light` / `dark` naming `old` get their
    /// replacement.
    fn retarget(&mut self, old: &str, extends: &str, light: &str, dark: &str) {
        let root = self.root_mut();
        if str_prop(root, "extends") == Some(old) {
            set_prop(root, "extends", KdlValue::String(extends.to_owned()));
        }
        if let Some(appearance) = root.children_mut().as_mut().and_then(|c| {
            c.nodes_mut()
                .iter_mut()
                .find(|n| is_named(n, APPEARANCE_NODE))
        }) {
            for (key, replacement) in [("light", light), ("dark", dark)] {
                if str_prop(appearance, key) == Some(old) {
                    set_prop(appearance, key, KdlValue::String(replacement.to_owned()));
                }
            }
        }
    }
}

/// Appends a token line to the first `tokens` block (created when needed).
fn append_token_line(body: &mut KdlDocument, line: &str, style: &Style) {
    if let Some(b) = body.nodes().iter().position(|n| is_named(n, "tokens")) {
        append_line(&mut body.nodes_mut()[b], line, 2, style);
    } else {
        append_rendered(body, 1, &block_text("tokens", line, 1, style), style);
    }
}

/// Appends a token line to `modes { <mode> { … } }` (blocks created when
/// needed; the last `modes` block and its last `<mode>` block are used).
fn append_mode_line(body: &mut KdlDocument, mode: &str, line: &str, style: &Style) {
    let Some(b) = body.nodes().iter().rposition(|n| is_named(n, "modes")) else {
        let inner = block_text(mode, line, 2, style);
        let text = format!(
            "{pad}modes {{{nl}{inner}{pad}}}{nl}",
            pad = style.pad(1),
            nl = style.nl
        );
        append_rendered(body, 1, &text, style);
        return;
    };
    let modes = &mut body.nodes_mut()[b];
    match children(modes).iter().rposition(|n| is_named(n, mode)) {
        Some(m) => append_line(&mut block_of(modes).nodes_mut()[m], line, 3, style),
        None => append_rendered(block_of(modes), 2, &block_text(mode, line, 2, style), style),
    }
}

/// Removes token lines `(block, line)` of `tokens` blocks.
fn remove_lines(body: &mut KdlDocument, lines: &[(usize, usize)], style: &Style) {
    let mut blocks: Vec<usize> = lines.iter().map(|&(b, _)| b).collect();
    blocks.dedup();
    for b in blocks {
        let indexes: Vec<usize> = lines
            .iter()
            .filter(|&&(lb, _)| lb == b)
            .map(|&(_, t)| t)
            .collect();
        remove_all(block_of(&mut body.nodes_mut()[b]), &indexes, 2, style);
    }
}

/// Removes lines `(modes block, mode block, line)` of `modes` blocks; mode
/// blocks and `modes` blocks left empty go too.
fn remove_mode_lines(body: &mut KdlDocument, lines: &[(usize, usize, usize)], style: &Style) {
    let mut groups: Vec<(usize, usize)> = lines.iter().map(|&(b, m, _)| (b, m)).collect();
    groups.dedup();
    let mut emptied: Vec<usize> = Vec::new();
    for &(b, m) in groups.iter().rev() {
        let indexes: Vec<usize> = lines
            .iter()
            .filter(|&&(lb, lm, _)| lb == b && lm == m)
            .map(|&(_, _, t)| t)
            .collect();
        let modes = block_of(&mut body.nodes_mut()[b]);
        let mode_block = block_of(&mut modes.nodes_mut()[m]);
        remove_all(mode_block, &indexes, 3, style);
        if mode_block.nodes().is_empty() {
            remove_all(modes, &[m], 2, style);
            if modes.nodes().is_empty() && !emptied.contains(&b) {
                emptied.push(b);
            }
        }
    }
    emptied.sort_unstable();
    remove_all(body, &emptied, 1, style);
}

/// `(parts block, index)` of every `part "<selector>"` rule: top-level
/// rules have no parts block.
fn part_sites(body: &KdlDocument, selector: &str) -> Vec<(Option<usize>, usize)> {
    let is_rule = |n: &KdlNode| is_named(n, "part") && first_arg(n) == Some(selector);
    let mut sites = Vec::new();
    for (b, node) in body.nodes().iter().enumerate() {
        if is_rule(node) {
            sites.push((None, b));
        } else if is_named(node, "parts") {
            for (i, rule) in children(node).iter().enumerate() {
                if is_rule(rule) {
                    sites.push((Some(b), i));
                }
            }
        }
    }
    sites
}

fn rule_mut(body: &mut KdlDocument, parts: Option<usize>, index: usize) -> &mut KdlNode {
    match parts {
        None => &mut body.nodes_mut()[index],
        Some(b) => &mut block_of(&mut body.nodes_mut()[b]).nodes_mut()[index],
    }
}

/// Removes rules (given in descending file order, as [`part_sites`]
/// reversed); `parts` blocks left empty go too.
fn remove_rules(body: &mut KdlDocument, rules: &[(Option<usize>, usize)], style: &Style) {
    // Descending file order: a removal never shifts a site still to come.
    for &(parts, index) in rules {
        match parts {
            None => remove_all(body, &[index], 1, style),
            Some(b) => {
                let block = block_of(&mut body.nodes_mut()[b]);
                remove_all(block, &[index], 2, style);
                if block.nodes().is_empty() {
                    remove_all(body, &[b], 1, style);
                }
            }
        }
    }
}

/// Applies theme patches to the text of a theme file (`None` = no file yet;
/// the user's `theme.kdl` then starts from the sparse skeleton).
pub(crate) fn apply_theme_patches(
    current: Option<&str>,
    patches: &[ThemePatch],
    file: &str,
) -> Result<String, crate::write_back::EditError> {
    let mut editor = ThemeEditor::open(current, file)?;
    for patch in patches {
        match patch {
            ThemePatch::SetToken {
                category,
                name,
                value,
                mode,
            } => editor.set_token(category, name, value.as_deref(), mode.as_deref()),
            ThemePatch::SetPartProperty {
                selector,
                property,
                value,
            } => editor.set_part_property(selector, property, value.as_deref()),
            ThemePatch::ResetPart { selector } => editor.reset_part(selector),
        }
    }
    editor.finish(file)
}

/// A theme id from a display name: accents dropped (NFKD without combining
/// marks, so `Café` is `cafe`), lowercase ASCII letters and digits,
/// everything else a dash; `theme-` in front when it does not start with a
/// letter. The UI derives the same id (`themeIdFromName`), pinned by
/// `crates/nail-config/src/ts_vectors.rs`.
pub(crate) fn id_from_name(name: &str) -> String {
    let mut id = String::new();
    for c in name.trim().nfkd().filter(|c| !is_combining_mark(*c)) {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let mut id = id.trim_end_matches('-').to_owned();
    if !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        id = if id.is_empty() {
            "theme".to_owned()
        } else {
            format!("theme-{id}")
        };
    }
    // Room for a `-<n>` suffix.
    id.truncate(MAX_THEME_ID_CHARS - 4);
    id.trim_end_matches('-').to_owned()
}

/// `text` with its leading comment block (the `//` lines and blank lines
/// before the first node) replaced by the header of a copied built-in
/// theme. Comment-only text in a configuration file, so English like the
/// other headers Nail writes (`// Changes made in Nail. …`).
fn with_copy_header(text: &str, name: &str, source_name: &str) -> String {
    let mut rest = text;
    while !rest.is_empty() {
        let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
        let trimmed = line.trim();
        if !(trimmed.is_empty() || trimmed.starts_with("//")) {
            break;
        }
        rest = after;
    }
    // A name is one line of user text; keep the comment on one line.
    let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "// {}: a copy of the built-in theme {}, made in Nail.\n\
         //\n\
         // Change it in Settings → Appearance → Edit theme, or here. Spacing,\n\
         // type, corners and density come from the base theme unless set below.\n\
         // See the theme reference in the user documentation for every token.\n\n{rest}",
        one_line(name),
        one_line(source_name),
    )
}

/// The default theme of a kind (where a deleted theme's selection goes).
fn default_of_kind(kind: ThemeKind) -> &'static str {
    match kind {
        ThemeKind::Light => DEFAULT_LIGHT_THEME,
        ThemeKind::Dark => DEFAULT_DARK_THEME,
        ThemeKind::HighContrast => HIGH_CONTRAST_THEME,
    }
}

/// `invalid_request` with `edit-issue-invalid-theme-name` for a blank, too
/// long or control-character name.
fn check_name(name: &str) -> Result<(), ApiError> {
    if is_theme_name(name) {
        Ok(())
    } else {
        Err(error(
            ErrorCode::InvalidRequest,
            issue(EditIssue::InvalidThemeName),
        ))
    }
}

impl ConfigManager {
    /// `not_found` unless the catalog lists `theme_id`; with `user_only`,
    /// `invalid_request` (`edit-issue-not-editable`) for a built-in theme.
    fn check_theme(&self, theme_id: &str, user_only: bool) -> Result<(), ApiError> {
        match self.theme_catalog().get(theme_id) {
            None => Err(theme_error(
                ErrorCode::NotFound,
                EditIssue::ThemeNotFound,
                theme_id,
            )),
            Some(info) if user_only && info.source != ThemeSource::User => Err(theme_error(
                ErrorCode::InvalidRequest,
                EditIssue::NotEditable,
                theme_id,
            )),
            Some(_) => Ok(()),
        }
    }

    /// Whether a theme id is taken (a theme, the base, or a file in
    /// `themes/`).
    fn theme_id_taken(&self, catalog: &ThemeCatalog, id: &str) -> bool {
        id == DEFAULT_BASE
            || is_builtin_theme(id)
            || catalog.get(id).is_some()
            || self.dir().join(user_theme_file(id)).exists()
    }

    /// Writes (`Some`) or removes (`None`) a file of the config directory;
    /// `theme.kdl` is backed up first.
    fn write_config_file(&self, file: &str, text: Option<&str>) -> Result<(), ApiError> {
        let path = self.dir().join(file);
        if let Some(config_file) = ConfigFile::from_file_name(file) {
            backup::take(self.dir(), config_file).map_err(|e| io_error(file, &e))?;
        }
        match text {
            Some(text) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io_error(file, &e))?;
                }
                backup::write_atomic(&path, text).map_err(|e| io_error(file, &e))
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_error(file, &e)),
                _ => Ok(()),
            },
        }
    }

    /// Writes the changes, records them as one undo entry and returns the
    /// updates (theme list, and `theme.kdl` when it changed).
    fn commit_theme_changes(
        &self,
        kind: EditKind,
        theme_id: &str,
        changes: Vec<FileChange>,
        mut updates: Vec<ConfigUpdate>,
    ) -> Result<Vec<ConfigUpdate>, ApiError> {
        let changes: Vec<FileChange> = changes
            .into_iter()
            .filter(|c| c.before != c.after)
            .collect();
        for change in &changes {
            self.write_config_file(&change.file, change.after.as_deref())?;
        }
        let theme_changed = changes
            .iter()
            .any(|c| c.file == ConfigFile::Theme.file_name());
        self.record_edit(
            HistoryScope::Appearance,
            kind,
            Self::plain_label(kind).arg("theme", theme_id),
            changes,
        );
        updates.extend(self.reload_themes());
        if theme_changed {
            updates.extend(self.updates_after_write(ConfigFile::Theme));
        }
        Ok(updates)
    }

    /// `GetThemeDetails`: every token (base tokens included, schema order)
    /// and every part rule that applies to `theme_id`, with their origins,
    /// whether it can be edited and its file's revision. `not_found` with
    /// `edit-issue-theme-not-found`.
    pub fn theme_details(&self, theme_id: &str) -> Result<ThemeDetails, ApiError> {
        self.check_theme(theme_id, false)?;
        let catalog = self.theme_catalog();
        let info = catalog
            .get(theme_id)
            .cloned()
            .ok_or_else(|| theme_error(ErrorCode::NotFound, EditIssue::ThemeNotFound, theme_id))?;
        let base = crate::default_theme_file();
        let chain = catalog.chain(theme_id);
        let own = chain.first().copied();
        // An unusable theme (a broken chain) shows what can be read.
        let merged = catalog.merged(theme_id).unwrap_or_else(|| {
            chain
                .iter()
                .rev()
                .fold(base.clone(), |file, next| file.merge((*next).clone()))
        });
        let defines = |file: &ThemeFile, category: &str, name: &str| {
            file.tokens
                .get(category)
                .is_some_and(|t| t.contains_key(name))
        };
        let mut tokens = Vec::new();
        for (category, defs) in &merged.tokens {
            for (name, def) in defs {
                let Some(value) = &def.value else {
                    continue;
                };
                let origin = if own.is_some_and(|f| defines(f, category, name)) {
                    ValueOrigin::Own
                } else if chain.iter().skip(1).any(|f| defines(f, category, name)) {
                    ValueOrigin::Extended
                } else {
                    ValueOrigin::Base
                };
                tokens.push(ThemeTokenValue {
                    category: category.clone(),
                    name: name.clone(),
                    value: value.value.clone(),
                    modes: def
                        .modes
                        .iter()
                        .map(|(m, v)| (m.clone(), v.value.clone()))
                        .collect(),
                    origin,
                });
            }
        }
        let rules = |file: &ThemeFile, origin: ValueOrigin| -> Vec<ThemePartRule> {
            file.parts
                .iter()
                .map(|part| ThemePartRule {
                    selector: part.selector.value.clone(),
                    properties: part
                        .declarations
                        .iter()
                        .map(|(p, v)| (p.clone(), v.value.clone()))
                        .collect(),
                    origin,
                })
                .collect()
        };
        let mut parts = rules(base, ValueOrigin::Base);
        for file in chain.iter().skip(1).rev() {
            parts.extend(rules(file, ValueOrigin::Extended));
        }
        if let Some(own) = own {
            parts.extend(rules(own, ValueOrigin::Own));
        }
        let editable = info.source == ThemeSource::User;
        let revision = if editable {
            self.file_revision(&info.file)?
        } else {
            config_revision(builtin_theme(theme_id).map(|t| t.kdl))
        };
        Ok(ThemeDetails {
            info,
            editable,
            revision,
            tokens,
            parts,
        })
    }

    /// `EditTheme`: applies `patches` to the target file as one write (one
    /// undo entry, [`EditKind::EditTheme`]). Returns the new appearance and
    /// the updates to broadcast. `conflict` for a stale `base_revision`,
    /// `invalid_request` with `edit-issue-not-editable` /
    /// `edit-issue-invalid-token` / `edit-issue-invalid-part`.
    pub fn edit_theme(
        &self,
        target: &ThemeEditTarget,
        patches: &[ThemePatch],
        base_revision: Option<&str>,
    ) -> Result<(Appearance, Vec<ConfigUpdate>), ApiError> {
        let (file, theme_id) = match target {
            ThemeEditTarget::User => (ConfigFile::Theme.file_name().to_owned(), None),
            ThemeEditTarget::Theme { theme_id } => {
                self.check_theme(theme_id, true)?;
                (user_theme_file(theme_id), Some(theme_id.as_str()))
            }
        };
        for patch in patches {
            check_patch(patch)?;
        }
        if patches.is_empty() {
            return Ok((self.appearance(), Vec::new()));
        }
        let _guard = self.lock_writes();
        let updates = self.reload_themes();
        let current = self.read_relative(&file)?;
        Self::check_base_revision(&file, base_revision, current.as_deref())?;
        let text = apply_theme_patches(current.as_deref(), patches, &file)?;
        if current.as_deref() == Some(text.as_str()) {
            return Ok((self.appearance(), updates));
        }
        if let Err(detail) = self.check_theme_text(theme_id, &text) {
            let tokens_only = patches
                .iter()
                .all(|p| matches!(p, ThemePatch::SetToken { .. }));
            let parts_only = patches
                .iter()
                .all(|p| !matches!(p, ThemePatch::SetToken { .. }));
            let message = if tokens_only {
                issue(EditIssue::InvalidToken)
            } else if parts_only {
                issue(EditIssue::InvalidPart)
            } else {
                crate::l10n::msg("config-edit-makes-invalid").arg("file", file.as_str())
            };
            return Err(error(
                ErrorCode::InvalidRequest,
                message.arg("detail", detail.clone()).arg("error", detail),
            ));
        }
        let label = theme_id.unwrap_or(ConfigFile::Theme.file_name()).to_owned();
        let changes = vec![FileChange::new(file, current, Some(text))];
        let updates = self.commit_theme_changes(EditKind::EditTheme, &label, changes, updates)?;
        Ok((self.appearance(), updates))
    }

    /// Whether edited theme text still works: the user's `theme.kdl`
    /// (`theme_id` `None`) resolves with the selection; a user theme is
    /// usable in the catalog with the new text. `Err` = the first problem.
    fn check_theme_text(&self, theme_id: Option<&str>, text: &str) -> Result<(), String> {
        let first = |errors: Vec<crate::error::ConfigError>| {
            errors.first().map(ToString::to_string).unwrap_or_default()
        };
        let Some(id) = theme_id else {
            let parsed =
                ThemeFile::parse_all(text, ConfigFile::Theme.file_name()).map_err(first)?;
            return crate::resolve_selected(
                &self.theme_catalog(),
                Some(&parsed),
                self.system_appearance(),
            )
            .map(drop)
            .map_err(first);
        };
        let mut texts = read_user_themes(self.dir());
        match texts.iter_mut().find(|t| t.stem == id) {
            Some(entry) => entry.text = Ok(text.to_owned()),
            None => texts.push(UserThemeText {
                stem: id.to_owned(),
                text: Ok(text.to_owned()),
            }),
        }
        let catalog = ThemeCatalog::with_user_themes(texts);
        match catalog.get(id).and_then(|info| info.problem.clone()) {
            None => Ok(()),
            Some(problem) => Err(crate::l10n::english(&problem)),
        }
    }

    /// `DuplicateTheme`: copies `theme_id` (built-in or user) into
    /// `themes/<new id>.kdl` named `name` (id from the name, kebab-case,
    /// unique). Returns the new id, the appearance and the updates.
    pub fn duplicate_theme(
        &self,
        theme_id: &str,
        name: &str,
    ) -> Result<(String, Appearance, Vec<ConfigUpdate>), ApiError> {
        self.check_theme(theme_id, false)?;
        check_name(name)?;
        let _guard = self.lock_writes();
        let updates = self.reload_themes();
        let catalog = self.theme_catalog();
        let source = match builtin_theme(theme_id) {
            Some(builtin) => builtin.kdl.to_owned(),
            None => self
                .read_relative(&user_theme_file(theme_id))?
                .ok_or_else(|| {
                    theme_error(ErrorCode::NotFound, EditIssue::ThemeNotFound, theme_id)
                })?,
        };
        let base = id_from_name(name);
        let new_id = std::iter::once(base.clone())
            // One of `themes + 1` suffixes is free (every taken id is a theme).
            .chain((2..=catalog.themes().len() + 2).map(|n| format!("{base}-{n}")))
            .find(|id| !self.theme_id_taken(&catalog, id))
            .unwrap_or(base);
        let file = user_theme_file(&new_id);
        let mut editor = ThemeEditor::open(Some(&source), &file)?;
        editor.set_name(name);
        let mut text = editor.finish(&file)?;
        if is_builtin_theme(theme_id) {
            // The built-in file's header describes the built-in theme
            // ("Nail built-in theme: Frost …"); the copy gets its own. A user
            // theme's comments are the user's and stay.
            let source_name = catalog
                .get(theme_id)
                .map_or_else(|| theme_id.to_owned(), |info| info.name.clone());
            text = with_copy_header(&text, name, &source_name);
        }
        let changes = vec![FileChange::new(file, None, Some(text))];
        let updates =
            self.commit_theme_changes(EditKind::DuplicateTheme, &new_id, changes, updates)?;
        Ok((new_id, self.appearance(), updates))
    }

    /// `RenameTheme`: a user theme's display name, and with `new_id` its
    /// file (the selection in `theme.kdl` and themes extending it follow,
    /// same undo entry). Returns the (new) id, the appearance and the
    /// updates.
    pub fn rename_theme(
        &self,
        theme_id: &str,
        name: &str,
        new_id: Option<&str>,
    ) -> Result<(String, Appearance, Vec<ConfigUpdate>), ApiError> {
        self.check_theme(theme_id, true)?;
        check_name(name)?;
        let _guard = self.lock_writes();
        let updates = self.reload_themes();
        let catalog = self.theme_catalog();
        let new_id = new_id.filter(|id| *id != theme_id);
        if let Some(id) = new_id {
            if !is_theme_id(id) || id == DEFAULT_BASE {
                return Err(theme_error(
                    ErrorCode::InvalidRequest,
                    EditIssue::InvalidThemeName,
                    id,
                ));
            }
            if self.theme_id_taken(&catalog, id) {
                return Err(theme_error(
                    ErrorCode::InvalidRequest,
                    EditIssue::ThemeExists,
                    id,
                ));
            }
        }
        let old_file = user_theme_file(theme_id);
        let current = self
            .read_relative(&old_file)?
            .ok_or_else(|| theme_error(ErrorCode::NotFound, EditIssue::ThemeNotFound, theme_id))?;
        let mut editor = ThemeEditor::open(Some(&current), &old_file)?;
        editor.set_name(name);
        let text = editor.finish(&old_file)?;
        let Some(new_id) = new_id else {
            let changes = vec![FileChange::new(old_file, Some(current), Some(text))];
            let updates =
                self.commit_theme_changes(EditKind::RenameTheme, theme_id, changes, updates)?;
            return Ok((theme_id.to_owned(), self.appearance(), updates));
        };
        let mut changes = vec![
            FileChange::new(old_file, Some(current), None),
            FileChange::new(user_theme_file(new_id), None, Some(text)),
        ];
        changes.extend(self.retarget_selection(theme_id, new_id, new_id, new_id)?);
        changes.extend(self.retarget_extends(&catalog, theme_id, new_id)?);
        let updates = self.commit_theme_changes(EditKind::RenameTheme, new_id, changes, updates)?;
        Ok((new_id.to_owned(), self.appearance(), updates))
    }

    /// `DeleteTheme`: removes a user theme's file; a selection naming it
    /// falls back to the default theme of its kind (same undo entry).
    pub fn delete_theme(
        &self,
        theme_id: &str,
    ) -> Result<(Appearance, Vec<ConfigUpdate>), ApiError> {
        self.check_theme(theme_id, true)?;
        let _guard = self.lock_writes();
        let updates = self.reload_themes();
        let kind = self
            .theme_catalog()
            .get(theme_id)
            .map_or(ThemeKind::Light, |info| info.kind);
        let file = user_theme_file(theme_id);
        let current = self.read_relative(&file)?;
        let mut changes = vec![FileChange::new(file, current, None)];
        changes.extend(self.retarget_selection(
            theme_id,
            default_of_kind(kind),
            DEFAULT_LIGHT_THEME,
            DEFAULT_DARK_THEME,
        )?);
        let updates =
            self.commit_theme_changes(EditKind::DeleteTheme, theme_id, changes, updates)?;
        Ok((self.appearance(), updates))
    }

    /// The change of `theme.kdl` that points its selection away from `old`
    /// (none when it does not name it, has no file or does not parse).
    fn retarget_selection(
        &self,
        old: &str,
        extends: &str,
        light: &str,
        dark: &str,
    ) -> Result<Option<FileChange>, ApiError> {
        let file = ConfigFile::Theme.file_name();
        let Some(current) = self.read_relative(file)? else {
            return Ok(None);
        };
        let Ok(mut editor) = ThemeEditor::open(Some(&current), file) else {
            return Ok(None);
        };
        editor.retarget(old, extends, light, dark);
        let text = editor.finish(file)?;
        Ok((text != current).then(|| FileChange::new(file, Some(current), Some(text))))
    }

    /// The changes of user themes that extend `old`: `extends="<new>"`.
    fn retarget_extends(
        &self,
        catalog: &ThemeCatalog,
        old: &str,
        new: &str,
    ) -> Result<Vec<FileChange>, ApiError> {
        let mut changes = Vec::new();
        for info in catalog.themes() {
            if info.source != ThemeSource::User
                || info.id == old
                || info.extends.as_deref() != Some(old)
            {
                continue;
            }
            let file = user_theme_file(&info.id);
            let Some(current) = self.read_relative(&file)? else {
                continue;
            };
            let Ok(mut editor) = ThemeEditor::open(Some(&current), &file) else {
                continue;
            };
            set_prop(
                editor.root_mut(),
                "extends",
                KdlValue::String(new.to_owned()),
            );
            let text = editor.finish(&file)?;
            changes.push(FileChange::new(file, Some(current), Some(text)));
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_builtin_themes_get_their_own_header() {
        let source = "// Nail built-in theme: Frost (light).\n//\n// Cool neutrals.\n\n\
                      theme \"Mine\" extends=\"default\" {\n    // tokens\n}\n";
        let text = with_copy_header(source, "My\nFrost", "Frost");
        assert!(
            text.starts_with("// My Frost: a copy of the built-in theme Frost, made in Nail.\n"),
            "{text}"
        );
        assert!(!text.contains("Nail built-in theme"), "{text}");
        assert!(!text.contains("Cool neutrals"), "{text}");
        // The theme node and the comments inside it stay as they were.
        assert!(
            text.ends_with("\n\ntheme \"Mine\" extends=\"default\" {\n    // tokens\n}\n"),
            "{text}"
        );
        // Without a header the copy still gets one; nothing else changes.
        let bare = "theme \"X\" {\n}\n";
        let header = with_copy_header(bare, "X", "Sage");
        assert!(header.ends_with("\n\ntheme \"X\" {\n}\n"), "{header}");
        // Every line of the header is a comment.
        let head = header.split("\n\ntheme").next().unwrap_or_default();
        assert!(head.lines().all(|l| l.starts_with("//")), "{head}");
    }

    #[test]
    fn duplicating_a_builtin_rewrites_its_header() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::load(dir.path());
        let (id, _, _) = manager.duplicate_theme("frost", "Ocean").unwrap();
        let copy =
            std::fs::read_to_string(dir.path().join("themes").join(format!("{id}.kdl"))).unwrap();
        assert!(
            copy.starts_with("// Ocean: a copy of the built-in theme Frost"),
            "{copy}"
        );
        assert!(!copy.contains("Nail built-in theme"), "{copy}");
        assert!(
            copy.contains("theme \"Ocean\" extends=\"default\""),
            "{copy}"
        );
    }

    #[test]
    fn plain_numbers() {
        for yes in ["0", "12", "-1", "1.5", "0.25"] {
            assert!(is_plain_number(yes), "{yes}");
        }
        for no in ["", "-", "1.", ".5", "1.2.3", "12px", "#123", "1e3"] {
            assert!(!is_plain_number(no), "{no}");
        }
        assert_eq!(typed("12"), KdlValue::Integer(12));
        assert_eq!(typed("1.5"), KdlValue::Float(1.5));
        assert_eq!(typed("#fff"), KdlValue::String("#fff".into()));
        assert_eq!(value_source("10"), "10");
        assert_eq!(
            value_source("1px solid $color.border"),
            "\"1px solid $color.border\""
        );
    }

    #[test]
    fn ids_from_names() {
        assert_eq!(id_from_name("My Theme"), "my-theme");
        assert_eq!(id_from_name("  Ocean -- Blue!  "), "ocean-blue");
        assert_eq!(id_from_name("2024"), "theme-2024");
        assert_eq!(id_from_name("Über Café"), "uber-cafe");
        assert_eq!(id_from_name("Ωmega"), "mega");
        assert_eq!(id_from_name("!!!"), "theme");
        let long = id_from_name(&"a".repeat(200));
        assert!(long.len() <= MAX_THEME_ID_CHARS - 4);
        for name in ["My Theme", "2024", "!!!", "Über", &"a-".repeat(80)] {
            assert!(is_theme_id(&id_from_name(name)), "{name}");
        }
    }

    #[test]
    fn patches_are_checked_on_their_own() {
        let token = |category: &str, name: &str, value: Option<&str>, mode: Option<&str>| {
            check_patch(&ThemePatch::SetToken {
                category: category.into(),
                name: name.into(),
                value: value.map(Into::into),
                mode: mode.map(Into::into),
            })
        };
        assert!(token("color", "accent", Some("#123456"), None).is_ok());
        assert!(token("radius", "md", Some("10"), Some("compact")).is_ok());
        assert!(token("color", "accent", None, Some("dark")).is_ok());
        for bad in [
            token("colour", "accent", Some("#123456"), None),
            token("color", "Accent", Some("#123456"), None),
            token("color", "accent", Some("url(x)"), None),
            token("color", "accent", Some("#000"), Some("light")),
            token("color", "accent", Some("#000"), Some("sepia")),
        ] {
            let err = bad.unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidRequest);
            assert_eq!(
                err.localized.as_deref().map(|l| l.id.as_str()),
                Some("edit-issue-invalid-token")
            );
        }
        let part = |selector: &str, property: &str, value: Option<&str>| {
            check_patch(&ThemePatch::SetPartProperty {
                selector: selector.into(),
                property: property.into(),
                value: value.map(Into::into),
            })
        };
        assert!(part("message-list.row", "background-color", Some("$color.bg")).is_ok());
        assert!(part("message-list.row", "padding", Some("4")).is_ok());
        for bad in [
            part("Message List", "color", Some("#000")),
            part("message-list.row", "behavior", Some("x")),
            part("message-list.row", "color", Some("url(evil)")),
            check_patch(&ThemePatch::ResetPart {
                selector: "[[".into(),
            }),
        ] {
            assert_eq!(
                bad.unwrap_err().localized.as_deref().map(|l| l.id.as_str()),
                Some("edit-issue-invalid-part")
            );
        }
    }
}
