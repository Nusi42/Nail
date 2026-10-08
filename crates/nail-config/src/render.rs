//! Canonical KDL text for single items of a theme or layout: what the merge
//! dialog shows ("mine" / "theirs") and what the write-back and the merge
//! insert into a user's file.
//!
//! Every function renders complete lines for a given nesting depth, each
//! ending with the style's line break, so the text can be parsed and
//! inserted with [`crate::kdl_edit::append_rendered`]. Rendering and
//! reading round-trip: reading the text back gives the same item.

use nail_api::windows::WindowFrame;
use nail_api::{
    ActionDef, ActionItem, Breakpoint, LayoutNode, MenuItem, ToolbarItem, ToolbarOverflow,
    ToolbarSettings,
};
use serde_json::Value;

use crate::kdl_edit::{Style, json_text};
use crate::kdl_write::Line;
use crate::theme::{IconDef, TokenDef};

impl Line {
    /// `hidden=#true` on a hidden layout node (M5d); nothing when shown.
    fn hidden(self, hidden: bool) -> Self {
        if hidden {
            self.raw_prop("hidden", "#true")
        } else {
            self
        }
    }

    /// Positional JSON arguments; `None` when one is an array or object.
    fn json_args(mut self, args: &[Value]) -> Option<Self> {
        for arg in args {
            self = self.raw_arg(json_text(arg)?);
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
            hidden,
            children: kids,
        } => {
            let orientation = orientation.as_str();
            let optional = |list: &[Option<String>]| {
                list.iter()
                    .map(|s| s.clone().unwrap_or_else(|| "none".to_owned()))
                    .collect::<Vec<_>>()
            };
            let mut line = Line::new("split")
                .arg(orientation)
                .prop_opt("id", id.as_deref())
                .prop_opt("label", label.as_deref());
            if !sizes.is_empty() {
                line = line.prop("sizes", &joined(sizes));
            }
            if !min_sizes.is_empty() {
                line = line.prop("min-sizes", &joined(&optional(min_sizes)));
            }
            if !max_sizes.is_empty() {
                line = line.prop("max-sizes", &joined(&optional(max_sizes)));
            }
            if !collapsible.is_empty() {
                let flags: Vec<String> = collapsible.iter().map(bool::to_string).collect();
                line = line.prop("collapsible", &joined(&flags));
            }
            if !resizable {
                line = line.raw_prop("resizable", "#false");
            }
            line.hidden(*hidden).block(depth, style, &children(kids)?)
        }
        LayoutNode::Stack {
            id,
            label,
            sizes,
            hidden,
            children: kids,
        } => {
            let mut line = Line::new("stack")
                .prop_opt("id", id.as_deref())
                .prop_opt("label", label.as_deref());
            if !sizes.is_empty() {
                line = line.prop("sizes", &joined(sizes));
            }
            line.hidden(*hidden).block(depth, style, &children(kids)?)
        }
        LayoutNode::Tabs {
            id,
            label,
            active,
            hidden,
            children: kids,
        } => {
            let mut line = Line::new("tabs")
                .prop_opt("id", id.as_deref())
                .prop_opt("label", label.as_deref());
            if *active != 0 {
                line = line.raw_prop("active", active);
            }
            line.hidden(*hidden).block(depth, style, &children(kids)?)
        }
        LayoutNode::Widget {
            id,
            label,
            kind,
            options,
            rows,
            hidden,
            ..
        } => {
            let mut line = Line::new(kind)
                .prop_opt("id", id.as_deref())
                .prop_opt("label", label.as_deref());
            for (key, value) in options {
                line = line.raw_prop(key, &json_text(value)?);
            }
            line = line.hidden(*hidden);
            if rows.is_empty() {
                line.end(depth, style)
            } else {
                let mut out = String::new();
                for row in rows {
                    out.push_str(&Line::new("row").arg(row).end(depth + 1, style));
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
    frame: Option<&WindowFrame>,
    depth: usize,
    style: &Style,
) -> Option<String> {
    let mut out = String::new();
    // The window-model properties (M5c) go on the first node only.
    let mut frame = frame.filter(|f| !f.is_empty());
    let mut head = || {
        let line = Line::new("window").arg(name);
        match frame.take() {
            Some(f) => window_frame(line, f),
            None => line,
        }
    };
    for bp in breakpoints {
        if bp.min_width.is_none() && bp.max_width.is_none() {
            let root = layout_node(&bp.root, depth + 1, style)?;
            out.push_str(&head().block(depth, style, &root));
        } else {
            let root = layout_node(&bp.root, depth + 2, style)?;
            let win = head().block(depth + 1, style, &root);
            let mut line = Line::new("breakpoint");
            if let Some(min) = bp.min_width {
                line = line.raw_prop("min-width", min);
            }
            if let Some(max) = bp.max_width {
                line = line.raw_prop("max-width", max);
            }
            out.push_str(&line.block(depth, style, &win));
        }
    }
    Some(out)
}

/// `size=… min-size=… modal=… resizable=… pop-out=… instances=…` (M5c).
fn window_frame(line: Line, frame: &WindowFrame) -> Line {
    let flag = |b: bool| if b { "#true" } else { "#false" };
    let mut line = line
        .prop_opt("size", frame.size.map(|s| s.to_string()).as_deref())
        .prop_opt("min-size", frame.min_size.map(|s| s.to_string()).as_deref());
    for (key, value) in [
        ("modal", frame.modal),
        ("resizable", frame.resizable),
        ("pop-out", frame.pop_out),
    ] {
        if let Some(value) = value {
            line = line.raw_prop(key, flag(value));
        }
    }
    line.prop_opt(
        "instances",
        frame
            .instances
            .map(nail_api::windows::WindowInstances::as_str),
    )
}

/// `action "<id>" label=… icon=… alias-of=… <args>…`.
pub(crate) fn action(id: &str, def: &ActionDef, depth: usize, style: &Style) -> Option<String> {
    let line = Line::new("action")
        .arg(id)
        .prop_opt("label", def.label.as_deref())
        .prop_opt("icon", def.icon.as_deref())
        .prop_opt("alias-of", def.alias_of.as_deref())
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
    let line = Line::new("bind").arg(key).arg(action).json_args(args)?;
    Some(line.end(depth, style))
}

/// `unbind "<key>"`.
pub(crate) fn unbind(key: &str, depth: usize, style: &Style) -> String {
    Line::new("unbind").arg(key).end(depth, style)
}

/// `keys context="…" { <lines> }` (no `context` for `global`).
pub(crate) fn keys_block(context: &str, lines: &str, depth: usize, style: &Style) -> String {
    let mut line = Line::new("keys");
    if context != "global" {
        line = line.prop("context", context);
    }
    line.block(depth, style, lines)
}

fn item_line(item: &ActionItem) -> Option<Line> {
    Some(
        Line::new("item")
            .arg(&item.action)
            .json_args(&item.args)?
            .prop_opt("label", item.label.as_deref())
            .prop_opt("icon", item.icon.as_deref()),
    )
}

/// One toolbar item.
pub(crate) fn toolbar_item(item: &ToolbarItem, depth: usize, style: &Style) -> Option<String> {
    Some(match item {
        ToolbarItem::Action {
            item,
            rank,
            display,
        } => {
            let mut line = item_line(item)?;
            if let Some(rank) = rank {
                line = line.raw_prop("rank", rank);
            }
            line.prop_opt("display", display.map(nail_api::ItemDisplay::as_str))
                .end(depth, style)
        }
        ToolbarItem::More { label, icon, items } => {
            let line = Line::new("more")
                .prop_opt("label", label.as_deref())
                .prop_opt("icon", icon.as_deref());
            if items.is_empty() {
                line.end(depth, style)
            } else {
                line.block(depth, style, &menu_items(items, depth + 1, style)?)
            }
        }
        ToolbarItem::Spacer => Line::new("spacer").end(depth, style),
        ToolbarItem::Separator => Line::new("separator").end(depth, style),
    })
}

/// `toolbar "<name>" hide-disabled=… overflow=… { … }` (properties only
/// when they differ from the defaults).
pub(crate) fn toolbar(
    name: &str,
    items: &[ToolbarItem],
    settings: ToolbarSettings,
    depth: usize,
    style: &Style,
) -> Option<String> {
    let mut out = String::new();
    for item in items {
        out.push_str(&toolbar_item(item, depth + 1, style)?);
    }
    let mut line = Line::new("toolbar").arg(name);
    if settings.hide_disabled {
        line = line.raw_prop("hide-disabled", "#true");
    }
    if settings.overflow != ToolbarOverflow::default() {
        line = line.prop_opt("overflow", Some(settings.overflow.as_str()));
    }
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
            MenuItem::Action(item) => item_line(item)?.end(depth, style),
            MenuItem::Separator => Line::new("separator").end(depth, style),
            MenuItem::Submenu { label, items } => Line::new("submenu").arg(label).block(
                depth,
                style,
                &menu_items(items, depth + 1, style)?,
            ),
            MenuItem::Header { label } => Line::new("header").arg(label).end(depth, style),
        });
    }
    Some(out)
}

/// `menu "<name>" { … }`.
pub(crate) fn menu(name: &str, items: &[MenuItem], depth: usize, style: &Style) -> Option<String> {
    let line = Line::new("menu").arg(name);
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
    let mut line = Line::new(category).arg(name);
    if let Some(value) = &def.value {
        line = line.arg(&value.value);
    }
    for (mode, value) in &def.modes {
        line = line.prop(mode, &value.value);
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
        out.push_str(&Line::new(property).arg(value).end(depth + 1, style));
    }
    Line::new("part").arg(selector).block(depth, style, &out)
}

/// `icon "<name>" svg="…" view-box="…"`.
pub(crate) fn icon(name: &str, def: &IconDef, depth: usize, style: &Style) -> String {
    Line::new("icon")
        .arg(name)
        .prop("svg", &def.svg)
        .prop_opt("view-box", def.view_box.as_deref())
        .end(depth, style)
}

/// A setting inside `message { … }`: `<key> "<value>"…`.
pub(crate) fn setting(key: &str, values: &[String], depth: usize, style: &Style) -> String {
    let mut line = Line::new(key);
    for value in values {
        line = line.arg(value);
    }
    line.end(depth, style)
}

/// `raw-css "<part>" "<declarations>"`.
pub(crate) fn raw_css(selector: &str, css: &str, depth: usize, style: &Style) -> String {
    Line::new("raw-css")
        .arg(selector)
        .arg(css)
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
    fn hidden_nodes_render_with_the_property_m5d() {
        let style = Style::default();
        let node: LayoutNode = serde_json::from_value(json!({
            "type": "stack", "hidden": true,
            "children": [
                {"type": "widget", "kind": "folder-tree", "hidden": true},
                {"type": "tabs", "children": [{"type": "widget", "kind": "message-list"}]}
            ]
        }))
        .unwrap();
        assert_eq!(
            layout_node(&node, 0, &style).unwrap(),
            "stack hidden=#true {\n    folder-tree hidden=#true\n    tabs {\n        message-list\n    }\n}\n"
        );
    }

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
                item: ActionItem {
                    label: Some("@x".into()),
                    ..ActionItem::new("mail.reply")
                },
                rank: None,
                display: None,
            },
            ToolbarItem::Spacer,
        ];
        let plain = ToolbarSettings::default();
        assert_eq!(
            toolbar("reader", &items, plain, 0, &style).unwrap(),
            "toolbar \"reader\" {\n    item \"mail.reply\" label=\"@x\"\n    spacer\n}\n"
        );
        assert_eq!(
            toolbar("x", &[], plain, 0, &style).unwrap(),
            "toolbar \"x\" {}\n"
        );
        // M5b: settings, rank / display and the More menu with a header.
        let items = vec![
            ToolbarItem::Action {
                item: ActionItem::new("mail.archive"),
                rank: Some(4),
                display: Some(nail_api::ItemDisplay::Icon),
            },
            ToolbarItem::More {
                label: None,
                icon: Some("ellipsis".into()),
                items: vec![
                    MenuItem::Header {
                        label: "@toolbar.group.message".into(),
                    },
                    MenuItem::Action(ActionItem::new("mail.toggle-read")),
                ],
            },
        ];
        let settings = ToolbarSettings {
            hide_disabled: true,
            overflow: ToolbarOverflow::Wrap,
        };
        assert_eq!(
            toolbar("reader", &items, settings, 0, &style).unwrap(),
            "toolbar \"reader\" hide-disabled=#true overflow=\"wrap\" {\n    item \"mail.archive\" rank=4 display=\"icon\"\n    more icon=\"ellipsis\" {\n        header \"@toolbar.group.message\"\n        item \"mail.toggle-read\"\n    }\n}\n"
        );
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
