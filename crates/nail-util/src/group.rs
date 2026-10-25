//! Grouping in first-seen order: groups keep the order their first value
//! came in, and keys need only equality (no hashing).

/// Adds `value` to the group whose key is `same` as `key`, or starts a
/// new group at the end.
pub fn push_by<K, V>(
    groups: &mut Vec<(K, Vec<V>)>,
    key: K,
    value: V,
    same: impl Fn(&K, &K) -> bool,
) {
    match groups.iter_mut().find(|(k, _)| same(k, &key)) {
        Some((_, values)) => values.push(value),
        None => groups.push((key, vec![value])),
    }
}

/// [`push_by`] with keys compared by equality.
pub fn push<K: PartialEq, V>(groups: &mut Vec<(K, Vec<V>)>, key: K, value: V) {
    push_by(groups, key, value, PartialEq::eq);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_join_their_group_in_first_seen_order() {
        let mut groups = Vec::new();
        for (key, value) in [("b", 1), ("a", 2), ("b", 3)] {
            push(&mut groups, key, value);
        }
        assert_eq!(groups, [("b", vec![1, 3]), ("a", vec![2])]);
    }

    #[test]
    fn keys_compare_as_told() {
        let mut groups = Vec::new();
        for (key, value) in [((1, "x"), 1), ((1, "y"), 2), ((2, "z"), 3)] {
            push_by(&mut groups, key, value, |a, b| a.0 == b.0);
        }
        // The group keeps the key it started with.
        assert_eq!(groups, [((1, "x"), vec![1, 2]), ((2, "z"), vec![3])]);
    }
}
