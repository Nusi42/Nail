//! Jamie Zawinski's threading algorithm (<https://www.jwz.org/doc/threading.html>).
//!
//! [`build_threads`] turns a set of messages into a forest of reply trees
//! using `Message-ID`, `References` and `In-Reply-To`, then groups remaining
//! roots by normalized subject. It is pure and in-memory; the store uses it
//! to lay out one thread ([`crate::Store::thread_tree`]), while thread
//! *membership* is maintained incrementally (see [`crate::thread`]).
//!
//! Edge cases handled as the algorithm describes them:
//!
//! * **Missing parents:** referenced messages that are not present become
//!   empty containers; they keep their children together and are pruned
//!   (a missing root with several children stays as a dummy node).
//! * **Loops:** a link that would make a container its own ancestor is not
//!   made (`A` references `B` and `B` references `A`, or a message that
//!   references itself).
//! * **Duplicate Message-IDs:** the second message with an id already in
//!   use gets a fresh container of its own, so neither copy is lost.

use std::collections::HashMap;

use super::subject::{SubjectKind, normalize_subject};

/// A message to thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMessage<K> {
    /// Caller's key for the message (returned in the tree).
    pub key: K,
    /// `Message-ID` without angle brackets.
    pub message_id: Option<String>,
    /// `References` in header order, then `In-Reply-To` (the parent is the
    /// last entry). Use [`parent_chain`] to build it.
    pub references: Vec<String>,
    /// Subject as sent.
    pub subject: String,
    /// Sort key for siblings (e.g. Unix seconds).
    pub date: i64,
}

/// A node of a thread tree. `message` is `None` for a dummy node that
/// stands for a missing message and keeps its children together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadNode<K> {
    /// The message at this node, `None` for a dummy.
    pub message: Option<K>,
    /// Replies, oldest first.
    pub children: Vec<ThreadNode<K>>,
}

impl<K: Clone> ThreadNode<K> {
    /// The messages of this subtree in depth-first order with their depth
    /// (dummies are skipped; their children keep the dummy's depth + 1).
    pub fn flatten(&self, depth: u32, out: &mut Vec<(K, u32)>) {
        let child_depth = match &self.message {
            Some(key) => {
                out.push((key.clone(), depth));
                depth + 1
            }
            None => depth,
        };
        for child in &self.children {
            child.flatten(child_depth, out);
        }
    }
}

/// `References` followed by the `In-Reply-To` entries that are not already
/// last, which is the parent chain JWZ step 1B/1C walks.
pub fn parent_chain(references: &[String], in_reply_to: &[String]) -> Vec<String> {
    let mut chain: Vec<String> = references.to_vec();
    for id in in_reply_to {
        if chain.last() != Some(id) {
            chain.retain(|x| x != id);
            chain.push(id.clone());
        }
    }
    chain
}

struct Container {
    message: Option<usize>,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct Arena {
    nodes: Vec<Container>,
}

impl Arena {
    fn add(&mut self) -> usize {
        self.nodes.push(Container {
            message: None,
            parent: None,
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }

    /// Whether `a` is `b` or one of its ancestors.
    fn is_ancestor_or_self(&self, a: usize, b: usize) -> bool {
        let mut cur = Some(b);
        let mut steps = 0;
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            cur = self.nodes[c].parent;
            steps += 1;
            if steps > self.nodes.len() {
                return true; // defensive: treat a corrupt chain as a loop
            }
        }
        false
    }

    fn unlink(&mut self, child: usize) {
        if let Some(p) = self.nodes[child].parent.take() {
            self.nodes[p].children.retain(|&c| c != child);
        }
    }

    fn link(&mut self, parent: usize, child: usize) {
        self.unlink(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }
}

/// Threads `messages`; see the [module docs](self). Roots are ordered by
/// their oldest message, children oldest first.
pub fn build_threads<K: Clone>(messages: &[ThreadMessage<K>]) -> Vec<ThreadNode<K>> {
    let mut arena = Arena { nodes: Vec::new() };
    let mut id_table: HashMap<&str, usize> = HashMap::new();

    // Step 1.
    for (index, message) in messages.iter().enumerate() {
        // 1A: this message's container.
        let this = match message.message_id.as_deref() {
            Some(id) => match id_table.get(id) {
                Some(&c) if arena.nodes[c].message.is_none() => c,
                // Duplicate Message-ID (or none): a container of its own.
                Some(_) => arena.add(),
                None => {
                    let c = arena.add();
                    id_table.insert(id, c);
                    c
                }
            },
            None => arena.add(),
        };
        arena.nodes[this].message = Some(index);

        // 1B: link the References chain, never overriding existing links
        // and never creating a loop.
        let mut prev: Option<usize> = None;
        for id in &message.references {
            let c = *id_table.entry(id.as_str()).or_insert_with(|| arena.add());
            if let Some(p) = prev
                && arena.nodes[c].parent.is_none()
                && !arena.is_ancestor_or_self(c, p)
            {
                arena.link(p, c);
            }
            prev = Some(c);
        }

        // 1C: the last reference is the definitive parent.
        match prev {
            Some(p) if !arena.is_ancestor_or_self(this, p) => arena.link(p, this),
            _ => arena.unlink(this),
        }
    }

    // Step 2: the root set.
    let roots: Vec<usize> = (0..arena.nodes.len())
        .filter(|&c| arena.nodes[c].parent.is_none())
        .collect();

    // Step 4: prune empty containers.
    let mut forest: Vec<Node> = roots
        .into_iter()
        .flat_map(|r| prune(&arena, r, true))
        .collect();

    // Step 5: group the root set by subject.
    forest = group_by_subject(forest, messages);

    // Sort siblings (and roots) by their oldest message.
    sort_nodes(&mut forest, messages);
    forest.iter().map(|n| to_public(n, messages)).collect()
}

/// Internal tree during pruning / grouping (indexes into `messages`).
#[derive(Debug)]
struct Node {
    message: Option<usize>,
    children: Vec<Node>,
}

fn prune(arena: &Arena, c: usize, at_root: bool) -> Vec<Node> {
    let children: Vec<Node> = arena.nodes[c]
        .children
        .iter()
        .flat_map(|&k| prune(arena, k, false))
        .collect();
    match arena.nodes[c].message {
        Some(m) => vec![Node {
            message: Some(m),
            children,
        }],
        // Empty without children: drop.
        None if children.is_empty() => Vec::new(),
        // Empty at root level with several children: keep as a dummy.
        None if at_root && children.len() > 1 => vec![Node {
            message: None,
            children,
        }],
        // Otherwise promote the children one level up.
        None => children,
    }
}

fn node_subject<'a, K>(node: &Node, messages: &'a [ThreadMessage<K>]) -> Option<&'a str> {
    match node.message {
        Some(m) => Some(&messages[m].subject),
        None => node
            .children
            .iter()
            .find_map(|c| c.message.map(|m| messages[m].subject.as_str())),
    }
}

fn group_by_subject<K>(roots: Vec<Node>, messages: &[ThreadMessage<K>]) -> Vec<Node> {
    // 5A-5B: pick the preferred root per subject key.
    let mut keys: Vec<Option<(String, SubjectKind)>> = Vec::with_capacity(roots.len());
    let mut table: HashMap<String, usize> = HashMap::new();
    for (i, root) in roots.iter().enumerate() {
        let entry = node_subject(root, messages).map(normalize_subject);
        let key = entry.filter(|n| !n.key.is_empty()).map(|n| (n.key, n.kind));
        if let Some((key, kind)) = &key {
            let better = match table.get(key) {
                None => true,
                Some(&j) => {
                    // Only keyed roots are in the table.
                    let other_kind = keys[j].as_ref().map(|(_, k)| *k);
                    // Prefer a dummy, then a non-reply.
                    (roots[i].message.is_none() && roots[j].message.is_some())
                        || (roots[i].message.is_some() == roots[j].message.is_some()
                            && *kind == SubjectKind::Original
                            && other_kind != Some(SubjectKind::Original))
                }
            };
            if better {
                table.insert(key.clone(), i);
            }
        }
        keys.push(key);
    }

    // 5C: merge the others into the preferred one.
    let mut slots: Vec<Option<Node>> = roots.into_iter().map(Some).collect();
    for i in 0..slots.len() {
        let Some((key, kind)) = keys[i].clone() else {
            continue;
        };
        let target = table[&key];
        if target == i {
            continue;
        }
        // A preferred root is never merged away, and each root is merged
        // once, so both slots are still full.
        let Some(this) = slots[i].take() else {
            continue;
        };
        let Some(that) = slots[target].take() else {
            slots[i] = Some(this);
            continue;
        };
        let that_kind = keys[target].as_ref().map(|(_, k)| *k);
        let merged = match (this.message.is_none(), that.message.is_none()) {
            // Both dummies: that takes this one's children.
            (true, true) => {
                let mut that = that;
                that.children.extend(this.children);
                that
            }
            // That is a dummy: this becomes its child.
            (false, true) => {
                let mut that = that;
                that.children.push(this);
                that
            }
            // This is a dummy (not preferred, so that one is a dummy too;
            // unreachable) or both are messages.
            _ if that_kind == Some(SubjectKind::Original) && kind != SubjectKind::Original => {
                let mut that = that;
                that.children.push(this);
                that
            }
            _ => Node {
                message: None,
                children: vec![that, this],
            },
        };
        slots[target] = Some(merged);
    }
    slots.into_iter().flatten().collect()
}

fn to_public<K: Clone>(node: &Node, messages: &[ThreadMessage<K>]) -> ThreadNode<K> {
    ThreadNode {
        message: node.message.map(|m| messages[m].key.clone()),
        children: node
            .children
            .iter()
            .map(|c| to_public(c, messages))
            .collect(),
    }
}

/// The oldest date in a subtree (dummies take their children's).
fn min_date<K>(node: &Node, messages: &[ThreadMessage<K>]) -> i64 {
    let own = node.message.map_or(i64::MAX, |m| messages[m].date);
    node.children
        .iter()
        .map(|c| min_date(c, messages))
        .fold(own, i64::min)
}

fn sort_nodes<K>(nodes: &mut [Node], messages: &[ThreadMessage<K>]) {
    for n in nodes.iter_mut() {
        sort_nodes(&mut n.children, messages);
    }
    // Stable: equal dates keep input order.
    nodes.sort_by_cached_key(|n| min_date(n, messages));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(
        key: &'static str,
        id: Option<&str>,
        refs: &[&str],
        subject: &str,
        date: i64,
    ) -> ThreadMessage<&'static str> {
        ThreadMessage {
            key,
            message_id: id.map(str::to_owned),
            references: refs.iter().map(|s| (*s).to_owned()).collect(),
            subject: subject.to_owned(),
            date,
        }
    }

    /// Renders a forest as `a(b c(d)) e` for compact assertions; `*` is a dummy.
    fn render(nodes: &[ThreadNode<&'static str>]) -> String {
        nodes
            .iter()
            .map(|n| {
                let name = n.message.unwrap_or("*");
                if n.children.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}({})", render(&n.children))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn simple_reply_chain() {
        let msgs = [
            msg("a", Some("a@x"), &[], "Plans", 1),
            msg("b", Some("b@x"), &["a@x"], "Re: Plans", 2),
            msg("c", Some("c@x"), &["a@x", "b@x"], "Re: Plans", 3),
            msg("d", Some("d@x"), &["a@x"], "Re: Plans", 4),
        ];
        assert_eq!(render(&build_threads(&msgs)), "a(b(c) d)");
    }

    #[test]
    fn children_arrive_before_parents() {
        let msgs = [
            msg("c", Some("c@x"), &["a@x", "b@x"], "Re: Plans", 3),
            msg("b", Some("b@x"), &["a@x"], "Re: Plans", 2),
            msg("a", Some("a@x"), &[], "Plans", 1),
        ];
        assert_eq!(render(&build_threads(&msgs)), "a(b(c))");
    }

    #[test]
    fn missing_parent_is_pruned_and_children_promoted() {
        // b's parent a is missing: b becomes a root; c stays below b.
        let msgs = [
            msg("b", Some("b@x"), &["a@x"], "Re: Plans", 2),
            msg("c", Some("c@x"), &["a@x", "b@x"], "Re: Plans", 3),
        ];
        assert_eq!(render(&build_threads(&msgs)), "b(c)");
    }

    #[test]
    fn missing_root_with_several_children_stays_a_dummy() {
        let msgs = [
            msg("b", Some("b@x"), &["a@x"], "Re: Plans", 2),
            msg("c", Some("c@x"), &["a@x"], "Re: Plans", 3),
        ];
        assert_eq!(render(&build_threads(&msgs)), "*(b c)");
    }

    #[test]
    fn missing_middle_link_is_bridged() {
        // c references a → m (missing) → c; m is pruned, c hangs under a.
        let msgs = [
            msg("a", Some("a@x"), &[], "Plans", 1),
            msg("c", Some("c@x"), &["a@x", "m@x"], "Re: Plans", 3),
        ];
        assert_eq!(render(&build_threads(&msgs)), "a(c)");
    }

    #[test]
    fn reference_loops_are_broken() {
        // a claims b as parent and b claims a: one of the links is refused.
        let msgs = [
            msg("a", Some("a@x"), &["b@x"], "Loop", 1),
            msg("b", Some("b@x"), &["a@x"], "Re: Loop", 2),
        ];
        let forest = build_threads(&msgs);
        let mut flat = Vec::new();
        for n in &forest {
            n.flatten(0, &mut flat);
        }
        assert_eq!(flat.len(), 2, "both messages kept: {}", render(&forest));
        assert_eq!(forest.len(), 1, "one thread: {}", render(&forest));
    }

    #[test]
    fn self_reference_and_long_loops() {
        let msgs = [
            msg("a", Some("a@x"), &["a@x"], "Self", 1),
            msg("b", Some("b@x"), &["c@x", "d@x"], "Ring", 2),
            msg("c", Some("c@x"), &["d@x", "b@x"], "Re: Ring", 3),
            msg("d", Some("d@x"), &["b@x", "c@x"], "Re: Ring", 4),
        ];
        let forest = build_threads(&msgs);
        let mut flat = Vec::new();
        for n in &forest {
            n.flatten(0, &mut flat);
        }
        let mut keys: Vec<_> = flat.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["a", "b", "c", "d"], "{}", render(&forest));
        assert_eq!(forest.len(), 2, "{}", render(&forest));
    }

    #[test]
    fn duplicate_message_ids_keep_both_messages() {
        let msgs = [
            msg("a1", Some("a@x"), &[], "Plans", 1),
            msg("a2", Some("a@x"), &[], "Plans", 1),
            msg("b", Some("b@x"), &["a@x"], "Re: Plans", 2),
        ];
        let forest = build_threads(&msgs);
        let rendered = render(&forest);
        // The reply attaches to the first copy; the duplicate joins by subject.
        assert_eq!(forest.len(), 1, "{rendered}");
        assert!(rendered.contains("a1(b"), "{rendered}");
        assert!(rendered.contains("a2"), "{rendered}");
    }

    #[test]
    fn later_message_fixes_a_guessed_parent() {
        // c's References guess b under a, but b itself says its parent is x.
        let msgs = [
            msg("c", Some("c@x"), &["a@x", "b@x"], "Re: T", 3),
            msg("x", Some("x@x"), &[], "T", 0),
            msg("b", Some("b@x"), &["x@x"], "Re: T", 2),
            msg("a", Some("a@x"), &[], "T", 1),
        ];
        let rendered = render(&build_threads(&msgs));
        assert!(rendered.contains("x(b(c))"), "{rendered}");
    }

    #[test]
    fn messages_without_ids_and_references_are_roots() {
        let msgs = [msg("a", None, &[], "One", 1), msg("b", None, &[], "Two", 2)];
        assert_eq!(render(&build_threads(&msgs)), "a b");
    }

    #[test]
    fn subject_grouping() {
        // An orphan reply joins the original with the same subject.
        let msgs = [
            msg("a", Some("a@x"), &[], "Lunch?", 1),
            msg("b", Some("b@x"), &[], "Re: Lunch?", 2),
            msg("z", Some("z@x"), &[], "Other", 3),
        ];
        assert_eq!(render(&build_threads(&msgs)), "a(b) z");

        // Two originals with the same subject become siblings under a dummy.
        let msgs = [
            msg("a", Some("a@x"), &[], "Hello", 1),
            msg("b", Some("b@x"), &[], "Hello", 2),
        ];
        assert_eq!(render(&build_threads(&msgs)), "*(a b)");

        // Empty subjects never group.
        let msgs = [
            msg("a", Some("a@x"), &[], "", 1),
            msg("b", Some("b@x"), &[], "Re:", 2),
        ];
        assert_eq!(render(&build_threads(&msgs)), "a b");
    }

    #[test]
    fn parent_chain_appends_in_reply_to() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert_eq!(parent_chain(&s(&["a", "b"]), &s(&["b"])), s(&["a", "b"]));
        assert_eq!(
            parent_chain(&s(&["a", "b"]), &s(&["c"])),
            s(&["a", "b", "c"])
        );
        assert_eq!(parent_chain(&s(&["a", "b"]), &s(&["a"])), s(&["b", "a"]));
        assert_eq!(parent_chain(&[], &s(&["p"])), s(&["p"]));
    }

    #[test]
    fn siblings_are_sorted_by_date() {
        let msgs = [
            msg("a", Some("a@x"), &[], "T", 1),
            msg("late", Some("l@x"), &["a@x"], "Re: T", 9),
            msg("early", Some("e@x"), &["a@x"], "Re: T", 5),
        ];
        assert_eq!(render(&build_threads(&msgs)), "a(early late)");
    }
}
