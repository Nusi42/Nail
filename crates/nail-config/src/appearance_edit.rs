//! Writing the theme selection and the user's overrides into `theme.kdl`
//! (M5a, ADR 0102) without losing comments or formatting.
//!
//! * [`apply_theme_selection`] (`SetTheme`): `extends="<id>"` on the
//!   `theme` node; an existing `appearance` node gets `follow-system=#false`
//!   (without one, a file extending a theme id is a fixed theme anyway).
//! * [`apply_theme_preferences`] (`SetThemePreferences`): the `appearance`
//!   node (updated in place, or added as the first child of `theme`) with
//!   all four properties, and the override tokens in `tokens`:
//!   `font-size "body"` (with a `compact=` value scaled like the base
//!   theme's, so Compact still shrinks the text), `radius "md"` and
//!   `color "accent"`; an override that is `None` removes its token line.
//!
//! Everything else stays byte for byte ([`crate::kdl_edit`]); a missing file
//! starts from the same sparse skeleton the merge dialog uses.

use kdl::{KdlDocument, KdlNode, KdlValue};
use nail_api::appearance::{APPEARANCE_NODE, Density, ThemeOverrides, ThemePreferences};

use crate::accounts::write::quote;
use crate::kdl_edit::{
    Style, append_rendered, ensure_multiline, first_arg, parse_rendered, remove_node,
    replace_rendered, set_prop, to_text,
};
use crate::l10n::msg;
use crate::theme::ThemeFile;
use crate::themes::{ACCENT_TOKEN, FONT_SIZE_TOKEN, RADIUS_TOKEN};
use crate::write_back::EditError;

/// Writes the fixed theme `theme_id` into the text of `theme.kdl` (`None`
/// = no file yet) and switches Follow Windows off. Whether the theme exists
/// is the caller's check ([`crate::ConfigManager::set_theme`]).
pub fn apply_theme_selection(
    current: Option<&str>,
    theme_id: &str,
    file: &str,
) -> Result<String, EditError> {
    let mut editor = ThemeEditor::open(current, file)?;
    let root = editor.root_mut();
    set_prop(root, "extends", KdlValue::String(theme_id.to_owned()));
    if let Some(appearance) = root.children_mut().as_mut().and_then(|c| {
        c.nodes_mut()
            .iter_mut()
            .find(|n| is_named(n, APPEARANCE_NODE))
    }) {
        set_prop(appearance, "follow-system", KdlValue::Bool(false));
    }
    editor.finish(file)
}

/// Writes Follow Windows, the pair, the density and the overrides into the
/// text of `theme.kdl` (`None` = no file yet). The values are the caller's
/// to check ([`ThemeOverrides::issues`], theme ids).
pub fn apply_theme_preferences(
    current: Option<&str>,
    preferences: &ThemePreferences,
    file: &str,
) -> Result<String, EditError> {
    let mut editor = ThemeEditor::open(current, file)?;
    editor.set_appearance(preferences);
    editor.set_overrides(&preferences.overrides);
    editor.finish(file)
}

/// The rendered override token lines (without indentation).
fn override_lines(
    overrides: &ThemeOverrides,
) -> [((&'static str, &'static str), Option<String>); 3] {
    let font_size = overrides.font_size.map(|size| {
        let mut line = token_line(FONT_SIZE_TOKEN, &size.to_string());
        if let Some(compact) = compact_font_size(size) {
            line.push_str(&format!(" compact={compact}"));
        }
        line
    });
    [
        (FONT_SIZE_TOKEN, font_size),
        (
            RADIUS_TOKEN,
            overrides
                .radius
                .map(|r| token_line(RADIUS_TOKEN, &r.to_string())),
        ),
        (
            ACCENT_TOKEN,
            overrides
                .accent
                .as_deref()
                .map(|a| token_line(ACCENT_TOKEN, &quote(a))),
        ),
    ]
}

fn token_line((category, name): (&str, &str), value: &str) -> String {
    format!("{category} {} {value}", quote(name))
}

/// The Compact text size for a body size: scaled like the base theme's
/// `font-size "body"` and its `compact=` value (14 → 13), rounded; `None`
/// when the base has no Compact value or it would not be smaller.
pub(crate) fn compact_font_size(size: u32) -> Option<u32> {
    let def = crate::default_theme_file()
        .tokens
        .get(FONT_SIZE_TOKEN.0)?
        .get(FONT_SIZE_TOKEN.1)?;
    let px = |v: &str| v.strip_suffix("px").unwrap_or(v).trim().parse::<f64>().ok();
    let base = px(&def.value.as_ref()?.value)?;
    let compact = px(&def.modes.get(nail_api::appearance::COMPACT_VARIANT)?.value)?;
    if base <= 0.0 || compact >= base {
        return None;
    }
    let scaled = (f64::from(size) * compact / base).round();
    // `scaled` is between 0 and `size`, so the conversion is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = scaled as u32;
    (scaled < size).then_some(scaled)
}

fn is_named(node: &KdlNode, name: &str) -> bool {
    node.name().value() == name
}

/// A `theme.kdl` document being edited.
struct ThemeEditor {
    doc: KdlDocument,
    root: usize,
    style: Style,
}

impl ThemeEditor {
    /// Starts editing; the current file must be a valid theme file (it may
    /// still fail to resolve onto a theme: the selection is how that gets
    /// fixed, so only reading errors refuse the edit).
    fn open(current: Option<&str>, file: &str) -> Result<Self, EditError> {
        let text = current.unwrap_or(crate::merge::NEW_THEME);
        ThemeFile::parse_all(text, file).map_err(EditError::Config)?;
        let doc = KdlDocument::parse_v2(text)
            .map_err(|e| EditError::Invalid(msg("config-syntax").arg("detail", e.to_string())))?;
        let root = doc
            .nodes()
            .iter()
            .position(|n| is_named(n, "theme"))
            .ok_or_else(|| EditError::Invalid(msg("config-edit-no-root").arg("root", "theme")))?;
        Ok(Self {
            doc,
            root,
            style: Style::of(text),
        })
    }

    fn root_mut(&mut self) -> &mut KdlNode {
        &mut self.doc.nodes_mut()[self.root]
    }

    /// The children of `theme` (created when the node has none), one per
    /// line so added or replaced lines read well.
    fn body(&mut self) -> &mut KdlDocument {
        let style = self.style.clone();
        let root = self.root_mut();
        if root.children().is_none() {
            if let Some(format) = root.format_mut() {
                format.before_children = " ".to_owned();
            }
            root.ensure_children();
        }
        let body = root
            .children_mut()
            .as_mut()
            .expect("children were just ensured");
        ensure_multiline(body, 1, &style);
        body
    }

    /// Updates the `appearance` node in place, or adds it as the first
    /// child of `theme`.
    fn set_appearance(&mut self, p: &ThemePreferences) {
        let density = match p.density {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        };
        let style = self.style.clone();
        let body = self.body();
        if let Some(node) = body
            .nodes_mut()
            .iter_mut()
            .find(|n| is_named(n, APPEARANCE_NODE))
        {
            set_prop(node, "follow-system", KdlValue::Bool(p.follow_system));
            set_prop(node, "light", KdlValue::String(p.light.clone()));
            set_prop(node, "dark", KdlValue::String(p.dark.clone()));
            set_prop(node, "density", KdlValue::String(density.to_owned()));
            return;
        }
        let line = format!(
            "{}{APPEARANCE_NODE} follow-system={} light={} dark={} density={}{}",
            style.pad(1),
            if p.follow_system { "#true" } else { "#false" },
            quote(&p.light),
            quote(&p.dark),
            quote(density),
            style.nl
        );
        insert_first(body, &line, &style);
    }

    /// Sets or removes the three override tokens in `tokens`.
    fn set_overrides(&mut self, overrides: &ThemeOverrides) {
        let style = self.style.clone();
        for ((category, name), line) in override_lines(overrides) {
            let body = self.body();
            // Every top-level `<category> "<name>" …` line of every
            // `tokens` block, as (block index, node index).
            let mut sites: Vec<(usize, usize)> = Vec::new();
            for (b, block) in body.nodes().iter().enumerate() {
                if !is_named(block, "tokens") {
                    continue;
                }
                for (t, token) in block
                    .children()
                    .map_or(&[][..], KdlDocument::nodes)
                    .iter()
                    .enumerate()
                {
                    if is_named(token, category) && first_arg(token) == Some(name) {
                        sites.push((b, t));
                    }
                }
            }
            let keep = match (&line, sites.first()) {
                (Some(line), Some(&(b, t))) => {
                    let block = body.nodes_mut()[b]
                        .children_mut()
                        .as_mut()
                        .expect("tokens block has children");
                    ensure_multiline(block, 2, &style);
                    replace_rendered(block, t, &format!("{}{line}{}", style.pad(2), style.nl));
                    // A line that ended an inline block keeps its line break
                    // in `trailing`; the rendered line brings its own.
                    if let Some(format) = block.nodes_mut()[t].format_mut()
                        && format.terminator.ends_with('\n')
                        && format.trailing.trim().is_empty()
                    {
                        format.trailing.clear();
                    }
                    Some((b, t))
                }
                (Some(line), None) => {
                    let rendered = format!("{}{line}{}", style.pad(2), style.nl);
                    match body.nodes().iter().position(|n| is_named(n, "tokens")) {
                        Some(b) => {
                            let tokens = &mut body.nodes_mut()[b];
                            if tokens.children().is_none() {
                                if let Some(format) = tokens.format_mut() {
                                    format.before_children = " ".to_owned();
                                }
                                tokens.ensure_children();
                            }
                            let block = tokens.children_mut().as_mut().expect("ensured");
                            append_rendered(block, 2, &rendered, &style);
                        }
                        None => {
                            let block = format!(
                                "{pad}tokens {{{nl}{rendered}{pad}}}{nl}",
                                pad = style.pad(1),
                                nl = style.nl
                            );
                            append_rendered(body, 1, &block, &style);
                        }
                    }
                    None
                }
                (None, _) => None,
            };
            // Remove the other (or, for `None`, all) lines, last first so
            // the indices stay valid.
            for &(b, t) in sites.iter().rev() {
                if Some((b, t)) == keep {
                    continue;
                }
                let block = body.nodes_mut()[b]
                    .children_mut()
                    .as_mut()
                    .expect("tokens block has children");
                ensure_multiline(block, 2, &style);
                remove_node(block, t, &style);
            }
        }
    }

    /// The new text; it must still be a valid theme file.
    fn finish(self, file: &str) -> Result<String, EditError> {
        let out = to_text(&self.doc, &self.style);
        if let Err(errors) = ThemeFile::parse_all(&out, file) {
            return Err(EditError::Invalid(
                msg("config-edit-makes-invalid")
                    .arg("file", file)
                    .arg("error", errors[0].to_string()),
            ));
        }
        Ok(out)
    }
}

/// Inserts nodes rendered for depth 1 (indented, ending with a line break)
/// as the first children of `block`.
fn insert_first(block: &mut KdlDocument, text: &str, style: &Style) {
    if block.nodes().is_empty() {
        append_rendered(block, 1, text, style);
        return;
    }
    ensure_multiline(block, 1, style);
    let doc_leading_breaks = block.format().is_some_and(|f| f.leading.contains('\n'));
    let mut nodes = parse_rendered(text);
    if !doc_leading_breaks {
        // The line break after `{` lives in the first child's leading text:
        // it moves to the new first node.
        let first = &mut block.nodes_mut()[0];
        if let Some(format) = first.format_mut() {
            let rest = format
                .leading
                .strip_prefix("\r\n")
                .or_else(|| format.leading.strip_prefix('\n'));
            if let Some(rest) = rest {
                format.leading = rest.to_owned();
            }
        }
        if let Some(new_first) = nodes.first_mut()
            && let Some(format) = new_first.format_mut()
        {
            format.leading.insert_str(0, style.nl);
        }
    }
    let count = nodes.len();
    block.nodes_mut().splice(0..0, nodes);
    debug_assert!(count > 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::appearance_settings;

    fn prefs() -> ThemePreferences {
        ThemePreferences {
            follow_system: true,
            light: "sage".into(),
            dark: "nocturne".into(),
            density: Density::Compact,
            overrides: ThemeOverrides::default(),
        }
    }

    fn read(text: &str) -> nail_api::appearance::AppearanceSettings {
        let file = ThemeFile::parse(text, "theme.kdl").unwrap();
        appearance_settings(Some(&file))
    }

    #[test]
    fn compact_text_scales_with_the_base() {
        // The base: body 14, compact 13.
        assert_eq!(compact_font_size(14), Some(13));
        assert_eq!(compact_font_size(16), Some(15));
        assert_eq!(compact_font_size(20), Some(19));
        assert_eq!(compact_font_size(11), Some(10));
    }

    #[test]
    fn insert_first_into_inline_and_multiline_blocks() {
        let style = Style::default();
        let mut d =
            KdlDocument::parse_v2("theme \"x\" {\n    // mine\n    sounds { a.b \"c\"; }\n}\n")
                .unwrap();
        let body = d.nodes_mut()[0].children_mut().as_mut().unwrap();
        insert_first(body, "    appearance density=\"compact\"\n", &style);
        assert_eq!(
            d.to_string(),
            "theme \"x\" {\n    appearance density=\"compact\"\n    // mine\n    sounds { a.b \"c\"; }\n}\n"
        );
        let mut d = KdlDocument::parse_v2("theme \"x\" { sounds { a.b \"c\"; }; }\n").unwrap();
        let body = d.nodes_mut()[0].children_mut().as_mut().unwrap();
        insert_first(body, "    appearance density=\"compact\"\n", &style);
        let text = d.to_string();
        assert!(
            text.starts_with("theme \"x\" {\n    appearance density=\"compact\"\n"),
            "{text}"
        );
        assert!(KdlDocument::parse_v2(&text).is_ok());
    }

    #[test]
    fn a_missing_file_starts_from_the_skeleton() {
        let out = apply_theme_selection(None, "graphite", "theme.kdl").unwrap();
        assert!(out.starts_with("// Changes made in Nail."), "{out}");
        assert!(out.contains("extends=\"graphite\""), "{out}");
        let s = read(&out);
        assert_eq!(s.theme, "graphite");
        assert!(!s.follow_system);

        let out = apply_theme_preferences(None, &prefs(), "theme.kdl").unwrap();
        assert_eq!(
            out,
            "// Changes made in Nail. See the theme reference in the user documentation.\n\
             theme \"custom\" extends=\"default\" {\n    \
             appearance follow-system=#true light=\"sage\" dark=\"nocturne\" density=\"compact\"\n}\n"
        );
    }

    #[test]
    fn a_theme_node_without_children_gets_a_block() {
        let out = apply_theme_preferences(
            Some("theme \"x\" extends=\"default\"\n"),
            &prefs(),
            "theme.kdl",
        )
        .unwrap();
        assert_eq!(
            out,
            "theme \"x\" extends=\"default\" {\n    appearance follow-system=#true light=\"sage\" dark=\"nocturne\" density=\"compact\"\n}\n"
        );
    }

    #[test]
    fn inline_blocks_become_multiline_when_edited() {
        let text =
            "theme \"Mine\" extends=\"default\" { tokens { color \"accent\" \"#ff0000\"; }; }
";
        let p = ThemePreferences {
            overrides: ThemeOverrides {
                accent: Some("#00ff00".into()),
                ..ThemeOverrides::default()
            },
            ..prefs()
        };
        let out = apply_theme_preferences(Some(text), &p, "theme.kdl").unwrap();
        assert_eq!(
            out,
            "theme \"Mine\" extends=\"default\" {
    appearance follow-system=#true light=\"sage\" dark=\"nocturne\" density=\"compact\"
    tokens {
        color \"accent\" \"#00ff00\"
    }
}
"
        );
        let cleared = apply_theme_preferences(Some(text), &prefs(), "theme.kdl").unwrap();
        assert!(
            cleared.contains(
                "    tokens {
    }
"
            ),
            "{cleared}"
        );
        assert_eq!(read(&cleared).overrides, ThemeOverrides::default());
    }

    #[test]
    fn invalid_files_are_refused() {
        let err =
            apply_theme_selection(Some("theme \"x\" { nope; }"), "frost", "theme.kdl").unwrap_err();
        assert!(matches!(err, EditError::Config(_)), "{err:?}");
        let err =
            apply_theme_preferences(Some("theme \"x\" {"), &prefs(), "theme.kdl").unwrap_err();
        assert!(matches!(err, EditError::Config(_)), "{err:?}");
    }
}
