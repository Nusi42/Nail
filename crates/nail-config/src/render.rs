//! Canonical KDL text for single items of a theme or layout: what the merge
//! dialog shows ("mine" / "theirs") and what the write-back and the merge
//! insert into a user's file.
//!
//! Every function renders complete lines for a given nesting depth, each
//! ending with the style's line break, so the text can be parsed and
//! inserted with [`crate::kdl_edit::append_rendered`]. Rendering and
//! reading round-trip: reading the text back gives the same item.

use std::fmt::Write as _;

use nail_api::{ActionDef, Breakpoint, LayoutNode, MenuItem, Orientation, ToolbarItem};
use serde_json::Value;

use crate::accounts::write::quote;
use crate::kdl_edit::{Style, json_text};
use crate::theme::{IconDef, TokenDef};

/// A line builder: `name arg… key=value…`.
struct Line(String);

impl Line {
    fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    fn arg(mut self, text: &str) -> Self {
        self.0.push(' ');
        self.0.push_str(text);
        self
    }

    fn str_arg(self, value: &str) -> Self {
        self.arg(&quote(value))
    }

    fn prop(mut self, key: &str, text: &str) -> Self {
        let _ = write!(self.0, " {key}={text}");
        self
    }

    fn str_prop(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.prop(key, &quote(v)),
            None => self,
        }
    }

    /// Positional JSON arguments; `None` when one is an array or object.
    fn json_args(mut self, args: &[Value]) -> Option<Self> {
        for arg in args {
            self = self.arg(&json_text(arg)?);
        }
        Some(self)
    }

    /// The line without children.
    fn end(self, depth: usize, style: &Style) -> String {
        format!("{}{}{}", style.pad(depth), self.0, style.nl)
    }

    /// The line with a children block (rendered for `depth + 1`).
    fn block(self, depth: usize, style: &Style, children: &str) -> String {
        let pad = style.pad(depth);
        format!("{pad}{} {{{nl}{children}{pad}}}{nl}", self.0, nl = style.nl)
    }
}

fn joined(values: &[String]) -> String {
    values.join(" ")
}

// ---------------------------------------------------------------------------
// Layout

/// A layout tree node (container or widget) with its subtree. `None` when
/// a widget option or action argument is not a KDL scalar.
pub(crate) fn layout_node(node: &LayoutNode, depth: usize, style: &Style) -> Option<String> {
    let children = |nodes: &[LayoutNode]| -> Option<String> {
        let mut out = String::new();
        for child in nodes {
            out.push_str(&layout_node(child, depth + 1, style)?);
        }
        Some(out)
    };
    Some(match node {
        LayoutNode::Split {
            id,
            label,
            orientation,
            sizes,
            min_sizes,
            max_sizes,
            collapsible,
            resizable,
            children: kids,
        } => {
            let orientation = match orientation {
                Orientation::Horizontal => "horizontal",
                Orientation::Vertical => "vertical",
            };
            let optional = |list: &[Option<String>]| {
                list.iter()
                    .map(|s| s.clone().unwrap_or_else(|| "none".to_owned()))
                    .collect::<Vec<_>>()
            };
            let mut line = Line::new("split")
                .str_arg(orientation)
                .str_prop("id", id.as_deref())
                .str_prop("label", label.as_deref());
            if !sizes.is_empty() {
                line = line.prop("sizes", &quote(&joined(sizes)));
            }
            if !min_sizes.is_empty() {
                line = line.prop("min-sizes", &quote(&joined(&optional(min_sizes))));
            }
            if !max_sizes.is_empty() {
                line = line.prop("max-sizes", &quote(&joined(&optional(max_sizes))));
            }
            if !collapsible.is_empty() {
                let flags: Vec<String> = collapsible.iter().map(bool::to_string).collect();
                line = line.prop("collapsible", &quote(&joined(&flags)));
            }
            if !resizable {
                line = line.prop("resizable", "#false");
            }
            line.block(depth, style, &children(kids)?)
        }
        LayoutNode::Stack {
            id,
            label,
            sizes,
            children: kids,
        } => {
            let mut line = Line::new("stack")
                .str_prop("id", id.as_deref())
                .str_prop("label", label.as_deref());
            if !sizes.is_empty() {
                line = line.prop("sizes", &quote(&joined(sizes)));
            }
            line.block(depth, style, &children(kids)?)
        }
        LayoutNode::Tabs {
            id,
            label,
            active,
            children: kids,
        } => {
            let mut line = Line::new("tabs")
                .str_prop("id", id.as_deref())
                .str_prop("label", label.as_deref());
            if *active != 0 {
                line = line.prop("active", &active.to_string());
            }
            line.block(depth, style, &children(kids)?)
        }
        LayoutNode::Widget {
            id,
            label,
            kind,
            options,
            rows,
            ..
        } => {
            let mut line = Line::new(kind)
                .str_prop("id", id.as_deref())
                .str_prop("label", label.as_deref());
            for (key, value) in options {
                line = line.prop(key, &json_text(value)?);
            }
            if rows.is_empty() {
                line.end(depth, style)
            } else {
                let mut out = String::new();
                for row in rows {
                    out.push_str(&Line::new("row").str_arg(row).end(depth + 1, style));
                }
                line.block(depth, style, &out)
            }
        }
    })
}

/// All definitions of one window: a `window` node for a breakpoint without
/// limits, else `breakpoint min-width=… { window … }`.
pub(crate) fn window(
    name: &str,
    breakpoints: &[Breakpoint],
    depth: usize,
    style: &Style,
) -> Option<String> {
    let mut out = String::new();
    for bp in breakpoints {
        if bp.min_width.is_none() && bp.max_width.is_none() {
            let root = layout_node(&bp.root, depth + 1, style)?;
            out.push_str(&Line::new("window").str_arg(name).block(depth, style, &root));
        } else {
            let root = layout_node(&bp.root, depth + 2, style)?;
            let win = Line::new("window")
                .str_arg(name)
                .block(depth + 1, style, &root);
            let mut line = Line::new("breakpoint");
            if let Some(min) = bp.min_width {
                line = line.prop("min-width", &min.to_string());
            }
            if let Some(max) = bp.max_width {
                line = line.prop("max-width", &max.to_string());
            }
            out.push_str(&line.block(depth, style, &win));
        }
    }
    Some(out)
}

/// `action "<id>" label=… icon=… alias-of=… <args>…`.
pub(crate) fn action(id: &str, def: &ActionDef, depth: usize, style: &Style) -> Option<String> {
    let line = Line::new("action")
        .str_arg(id)
        .str_prop("label", def.label.as_deref())
        .str_prop("icon", def.icon.as_deref())
        .str_prop("alias-of", def.alias_of.as_deref())
        .json_args(&def.args)?;
    Some(line.end(depth, style))
}

/// `bind "<key>" "<action>" <args>…`.
pub(crate) fn bind(
    key: &str,
    action: &str,
    args: &[Value],
    depth: usize,
    style: &Style,
) -> Option<String> {
    let line = Line::new("bind")
        .str_arg(key)
        .str_arg(action)
        .json_args(args)?;
    Some(line.end(depth, style))
}

/// `unbind "<key>"`.
pub(crate) fn unbind(key: &str, depth: usize, style: &Style) -> String {
    Line::new("unbind").str_arg(key).end(depth, style)
}

/// `keys context="…" { <lines> }` (no `context` for `global`).
pub(crate) fn keys_block(context: &str, lines: &str, depth: usize, style: &Style) -> String {
    let mut line = Line::new("keys");
    if context != "global" {
        line = line.prop("context", &quote(context));
    }
    line.block(depth, style, lines)
}

fn item_line(
    action: &str,
    args: &[Value],
    label: Option<&str>,
    icon: Option<&str>,
) -> Option<Line> {
    Some(
        Line::new("item")
            .str_arg(action)
            .json_args(args)?
            .str_prop("label", label)
            .str_prop("icon", icon),
    )
}

/// One toolbar item.
pub(crate) fn toolbar_item(item: &ToolbarItem, depth: usize, style: &Style) -> Option<String> {
    Some(match item {
        ToolbarItem::Action {
            action,
            args,
            label,
            icon,
        } => item_line(action, args, label.as_deref(), icon.as_deref())?.end(depth, style),
        ToolbarItem::Spacer => Line::new("spacer").end(depth, style),
        ToolbarItem::Separator => Line::new("separator").end(depth, style),
    })
}

/// `toolbar "<name>" { … }`.
pub(crate) fn toolbar(
    name: &str,
    items: &[ToolbarItem],
    depth: usize,
    style: &Style,
) -> Option<String> {
    let mut out = String::new();
    for item in items {
        out.push_str(&toolbar_item(item, depth + 1, style)?);
    }
    let line = Line::new("toolbar").str_arg(name);
    Some(if items.is_empty() {
        format!("{}{} {{}}{}", style.pad(depth), line.0, style.nl)
    } else {
        line.block(depth, style, &out)
    })
}

fn menu_items(items: &[MenuItem], depth: usize, style: &Style) -> Option<String> {
    let mut out = String::new();
    for item in items {
        out.push_str(&match item {
            MenuItem::Action {
                action,
                args,
                label,
                icon,
            } => item_line(action, args, label.as_deref(), icon.as_deref())?.end(depth, style),
            MenuItem::Separator => Line::new("separator").end(depth, style),
            MenuItem::Submenu { label, items } => Line::new("submenu").str_arg(label).block(
                depth,
                style,
                &menu_items(items, depth + 1, style)?,
            ),
        });
    }
    Some(out)
}

/// `menu "<name>" { … }`.
pub(crate) fn menu(name: &str, items: &[MenuItem], depth: usize, style: &Style) -> Option<String> {
    let line = Line::new("menu").str_arg(name);
    Some(if items.is_empty() {
        format!("{}{} {{}}{}", style.pad(depth), line.0, style.nl)
    } else {
        line.block(depth, style, &menu_items(items, depth + 1, style)?)
    })
}

// ---------------------------------------------------------------------------
// Theme

/// `<category> "<name>" "<value>" dark=… high-contrast=…`.
pub(crate) fn token(
    category: &str,
    name: &str,
    def: &TokenDef,
    depth: usize,
    style: &Style,
) -> String {
    let mut line = Line::new(category).str_arg(name);
    if let Some(value) = &def.value {
        line = line.str_arg(&value.value);
    }
    for (mode, value) in &def.modes {
        line = line.prop(mode, &quote(&value.value));
    }
    line.end(depth, style)
}

/// `part "<selector>" { <property> "<value>" … }`.
pub(crate) fn part<'a>(
    selector: &str,
    declarations: impl IntoIterator<Item = (&'a String, &'a String)>,
    depth: usize,
    style: &Style,
) -> String {
    let mut out = String::new();
    for (property, value) in declarations {
        out.push_str(&Line::new(property).str_arg(value).end(depth + 1, style));
    }
    Line::new("part")
        .str_arg(selector)
        .block(depth, style, &out)
}

/// `icon "<name>" svg="…" view-box="…"`.
pub(crate) fn icon(name: &str, def: &IconDef, depth: usize, style: &Style) -> String {
    Line::new("icon")
        .str_arg(name)
        .prop("svg", &quote(&def.svg))
        .str_prop("view-box", def.view_box.as_deref())
        .end(depth, style)
}

/// A setting inside `message { … }`: `<key> "<value>"…`.
pub(crate) fn setting(key: &str, values: &[String], depth: usize, style: &Style) -> String {
    let mut line = Line::new(key);
    for value in values {
        line = line.str_arg(value);
    }
    line.end(depth, style)
}

/// `raw-css "<part>" "<declarations>"`.
pub(crate) fn raw_css(selector: &str, css: &str, depth: usize, style: &Style) -> String {
    Line::new("raw-css")
        .str_arg(selector)
        .str_arg(css)
        .end(depth, style)
}

/// A section block `<name> { <lines> }`.
pub(crate) fn section(name: &str, lines: &str, depth: usize, style: &Style) -> String {
    Line::new(name).block(depth, style, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layout_items_render_canonically() {
        let style = Style::default();
        let node: LayoutNode = serde_json::from_value(json!({
            "type": "split", "id": "panes", "sizes": ["240px", "1fr"],
            "min_sizes": ["160px", null], "collapsible": [true, false], "resizable": false,
            "children": [
                {"type": "widget", "kind": "folder-tree", "options": {"show-unread": true}},
                {"type": "widget", "kind": "message-list", "options": {"page-size": 50},
                 "rows": ["{subject}"]}
            ]
        }))
        .unwrap();
        assert_eq!(
            layout_node(&node, 1, &style).unwrap(),
            "    split \"horizontal\" id=\"panes\" sizes=\"240px 1fr\" min-sizes=\"160px none\" collapsible=\"true false\" resizable=#false {\n        folder-tree show-unread=#true\n        message-list page-size=50 {\n            row \"{subject}\"\n        }\n    }\n"
        );
        assert_eq!(
            bind("g i", "nav.goto-folder", &[json!("Inbox")], 1, &style).unwrap(),
            "    bind \"g i\" \"nav.goto-folder\" \"Inbox\"\n"
        );
        assert_eq!(bind("x", "a.b", &[json!({})], 0, &style), None);
        assert_eq!(unbind("e", 0, &style), "unbind \"e\"\n");
        assert_eq!(
            keys_block("global", &unbind("e", 1, &style), 0, &style),
            "keys {\n    unbind \"e\"\n}\n"
        );
        let items = vec![
            ToolbarItem::Action {
                action: "mail.reply".into(),
                args: vec![],
                label: Some("@x".into()),
                icon: None,
            },
            ToolbarItem::Spacer,
        ];
        assert_eq!(
            toolbar("reader", &items, 0, &style).unwrap(),
            "toolbar \"reader\" {\n    item \"mail.reply\" label=\"@x\"\n    spacer\n}\n"
        );
        assert_eq!(toolbar("x", &[], 0, &style).unwrap(), "toolbar \"x\" {}\n");
        let def = ActionDef {
            label: Some("@a".into()),
            icon: None,
            alias_of: Some("window.open".into()),
            args: vec![json!("outbox")],
        };
        assert_eq!(
            action("outbox.open", &def, 0, &style).unwrap(),
            "action \"outbox.open\" label=\"@a\" alias-of=\"window.open\" \"outbox\"\n"
        );
    }
}
