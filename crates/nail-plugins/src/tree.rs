//! Component trees on their way from a plug-in to the UI: package images
//! are inlined, host-filled fields are cleared, the tree is validated
//! ([`validate_tree`]) and `message-list` rows are filled with what the
//! plug-in may read. Values coming back from the UI are checked against
//! the tree they belong to.

use std::collections::{BTreeMap, HashMap, HashSet};

use base64::Engine as _;
use nail_api::plugins::ui::{PluginValueKind, is_image_data_uri};
use nail_api::plugins::{PluginMessage, PluginUiIssue, PluginUiNode, validate_tree};
use nail_api::{Address, MessageId};
use serde_json::Value;

use crate::package::{PackageFiles, is_safe_path};
use crate::services::{AccountScope, PluginServices};

/// Image types by file extension (package assets named in `image` nodes).
fn image_type(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

fn inline(files: &PackageFiles, node: &mut PluginUiNode) {
    match node {
        PluginUiNode::Image { src, .. } if !src.starts_with("data:") => {
            if is_safe_path(src)
                && let Some(kind) = image_type(src)
                && let Ok(bytes) = files.read(src)
            {
                let uri = format!(
                    "data:{kind};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                );
                // Oversized images stay as they are and fail validation.
                if is_image_data_uri(&uri) {
                    *src = uri;
                }
            }
        }
        PluginUiNode::MessageList { messages, .. } => messages.clear(),
        PluginUiNode::Stack { children, .. } => {
            for child in children {
                inline(files, child);
            }
        }
        _ => {}
    }
}

/// Inlines package images, clears host-filled fields and validates the
/// tree.
pub fn prepare_tree(
    files: &PackageFiles,
    mut nodes: Vec<PluginUiNode>,
) -> Result<Vec<PluginUiNode>, Vec<PluginUiIssue>> {
    for node in &mut nodes {
        inline(files, node);
    }
    let issues = validate_tree(&nodes);
    if issues.is_empty() {
        Ok(nodes)
    } else {
        Err(issues)
    }
}

fn walk<'a>(nodes: &'a [PluginUiNode], f: &mut dyn FnMut(&'a PluginUiNode)) {
    for node in nodes {
        f(node);
        walk(node.children(), f);
    }
}

fn walk_mut(nodes: &mut [PluginUiNode], f: &mut dyn FnMut(&mut PluginUiNode)) {
    for node in nodes {
        f(node);
        if let PluginUiNode::Stack { children, .. } = node {
            walk_mut(children, f);
        }
    }
}

/// The message ids named by `message-list` nodes.
pub fn message_ids(nodes: &[PluginUiNode]) -> Vec<MessageId> {
    let mut ids = Vec::new();
    walk(nodes, &mut |n| {
        if let PluginUiNode::MessageList { message_ids, .. } = n {
            ids.extend(message_ids.iter().cloned());
        }
    });
    ids
}

/// Fills `message-list` rows from `found`; ids not in it are dropped.
pub fn fill_messages<S: std::hash::BuildHasher>(
    nodes: &mut [PluginUiNode],
    found: &HashMap<MessageId, PluginMessage, S>,
) {
    walk_mut(nodes, &mut |n| {
        if let PluginUiNode::MessageList {
            message_ids,
            messages,
            ..
        } = n
        {
            *messages = message_ids
                .iter()
                .filter_map(|id| found.get(id).cloned())
                .collect();
        }
    });
}

/// Looks up messages the plug-in may read (`scope` from `mail.read`;
/// `None` = no access): unknown ids and messages outside the scope are
/// left out.
pub async fn readable_messages(
    services: &dyn PluginServices,
    scope: Option<&AccountScope>,
    ids: Vec<MessageId>,
) -> HashMap<MessageId, PluginMessage> {
    let mut found = HashMap::new();
    let Some(scope) = scope else {
        return found;
    };
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Ok(Some(message)) = services.mail_get(id.clone()).await
            && scope.contains(&message.account_id)
        {
            found.insert(id, message);
        }
    }
    found
}

/// Fills a prepared tree's message lists.
pub async fn fill_tree(
    services: &dyn PluginServices,
    scope: Option<&AccountScope>,
    mut nodes: Vec<PluginUiNode>,
) -> Vec<PluginUiNode> {
    let ids = message_ids(&nodes);
    if !ids.is_empty() {
        let found = readable_messages(services, scope, ids).await;
        fill_messages(&mut nodes, &found);
    }
    nodes
}

/// The value ids and kinds of a tree's input nodes.
pub fn input_kinds(nodes: &[PluginUiNode]) -> HashMap<String, PluginValueKind> {
    let mut out = HashMap::new();
    walk(nodes, &mut |n| {
        if let (Some(id), Some(kind)) = (n.id(), n.value_kind()) {
            out.insert(id.to_owned(), kind);
        }
    });
    out
}

/// The ids of a tree's `button` nodes.
pub fn button_ids(nodes: &[PluginUiNode]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk(nodes, &mut |n| {
        if let PluginUiNode::Button { id, .. } = n {
            out.insert(id.clone());
        }
    });
    out
}

fn value_fits(kind: PluginValueKind, value: &Value) -> bool {
    let strings = |v: &Value| v.as_array().is_some_and(|a| a.iter().all(Value::is_string));
    match kind {
        PluginValueKind::Text => value.is_string(),
        PluginValueKind::TextList => strings(value),
        PluginValueKind::Bool => value.is_boolean(),
        PluginValueKind::OptionalText => value.is_string() || value.is_null(),
        PluginValueKind::Addresses => value.as_array().is_some_and(|a| {
            a.iter()
                .all(|x| serde_json::from_value::<Address>(x.clone()).is_ok())
        }),
        PluginValueKind::Indexes => value
            .as_array()
            .is_some_and(|a| a.iter().all(|x| x.as_u64().is_some())),
    }
}

/// The values a UI sent, limited to the tree's input ids and the value
/// shapes they report; anything else is dropped.
pub fn clean_values<S: std::hash::BuildHasher>(
    kinds: &HashMap<String, PluginValueKind, S>,
    values: BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    values
        .into_iter()
        .filter(|(id, value)| kinds.get(id).is_some_and(|k| value_fits(*k, value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::plugins::PluginUiIssueKind;
    use serde_json::json;

    fn nodes(value: Value) -> Vec<PluginUiNode> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn inlines_package_images_and_validates() {
        let files = PackageFiles::from_map(BTreeMap::from([
            ("assets/logo.png".to_owned(), vec![1, 2, 3]),
            ("assets/big.png".to_owned(), vec![0; 600 * 1024]),
            ("assets/x.svg".to_owned(), b"<svg/>".to_vec()),
        ]));
        let tree = nodes(json!([
            {"type": "stack", "children": [{"type": "image", "src": "assets/logo.png", "alt": "Logo"}]},
            {"type": "message-list", "messageIds": ["m1"], "messages": [{
                "id": "forged", "uri": "nail://messages/forged", "accountId": "a", "folderId": "f",
                "subject": "forged", "to": [], "date": "2026-01-01T00:00:00Z", "flags": {},
                "hasAttachments": false
            }]}
        ]));
        let ok = prepare_tree(&files, tree).unwrap();
        let PluginUiNode::Stack { children, .. } = &ok[0] else {
            panic!()
        };
        let PluginUiNode::Image { src, .. } = &children[0] else {
            panic!()
        };
        assert_eq!(src, "data:image/png;base64,AQID");
        let PluginUiNode::MessageList { messages, .. } = &ok[1] else {
            panic!()
        };
        assert!(messages.is_empty(), "plug-ins cannot forge rows");

        for src in [
            "assets/big.png",
            "assets/x.svg",
            "https://evil.example/x.png",
            "../secret.png",
            "assets/missing.png",
        ] {
            let issues = prepare_tree(
                &files,
                nodes(json!([{"type": "image", "src": src, "alt": ""}])),
            )
            .unwrap_err();
            assert_eq!(issues[0].kind, PluginUiIssueKind::InvalidImage, "{src}");
        }
        let issues = prepare_tree(
            &files,
            nodes(json!([{"type": "input", "id": "a", "label": "A"}, {"type": "input", "id": "a", "label": "B"}])),
        )
        .unwrap_err();
        assert_eq!(issues[0].kind, PluginUiIssueKind::DuplicateId);
    }

    #[test]
    fn values_are_limited_to_the_tree() {
        let tree = nodes(json!([
            {"type": "input", "id": "name", "label": "Name"},
            {"type": "stack", "children": [
                {"type": "checkbox", "id": "ok", "label": "OK"},
                {"type": "date", "id": "due", "label": "Due"},
                {"type": "select", "id": "tags", "label": "Tags", "multiple": true,
                 "options": [{"value": "a", "label": "A"}]},
                {"type": "contact-picker", "id": "who", "label": "Who"},
                {"type": "table", "id": "rows", "selectable": true, "columns": [{"label": "C"}], "rows": [["x"]]},
                {"type": "button", "id": "go", "label": "Go"}
            ]}
        ]));
        let kinds = input_kinds(&tree);
        assert_eq!(kinds.len(), 6);
        assert_eq!(button_ids(&tree), HashSet::from(["go".to_owned()]));
        let values: BTreeMap<String, Value> = serde_json::from_value(json!({
            "name": "Ada", "ok": true, "due": null, "tags": ["a"],
            "who": [{"name": null, "email": "a@example.org"}], "rows": [0],
            "go": "x", "unknown": 1
        }))
        .unwrap();
        let clean = clean_values(&kinds, values);
        assert_eq!(clean.len(), 6);
        assert!(!clean.contains_key("go") && !clean.contains_key("unknown"));
        let wrong: BTreeMap<String, Value> = serde_json::from_value(json!({
            "name": 1, "ok": "yes", "due": 3, "tags": "a", "who": [{"x": 1}], "rows": [-1]
        }))
        .unwrap();
        assert!(clean_values(&kinds, wrong).is_empty());
    }

    #[test]
    fn fills_message_lists() {
        let mut tree = nodes(json!([
            {"type": "stack", "children": [{"type": "message-list", "messageIds": ["m1", "m2"]}]}
        ]));
        assert_eq!(
            message_ids(&tree),
            [MessageId::from("m1"), MessageId::from("m2")]
        );
        let message: PluginMessage = serde_json::from_value(json!({
            "id": "m2", "uri": "nail://messages/m2", "accountId": "a", "folderId": "f",
            "subject": "Hi", "to": [], "date": "2026-01-01T00:00:00Z", "flags": {},
            "hasAttachments": false
        }))
        .unwrap();
        fill_messages(
            &mut tree,
            &HashMap::from([(MessageId::from("m2"), message)]),
        );
        let PluginUiNode::Stack { children, .. } = &tree[0] else {
            panic!()
        };
        let PluginUiNode::MessageList { messages, .. } = &children[0] else {
            panic!()
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].subject, "Hi");
    }
}
