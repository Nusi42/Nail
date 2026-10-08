//! Lookups in lists of (name, value) pairs with case-insensitive names:
//! HTTP headers, MIME / iCalendar parameters, auth-challenge parameters
//! (one definition; before Q2 each list type had its own copy).

/// The first value named `name` (ASCII case-insensitive).
#[must_use]
pub fn first<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Every value named `name` (ASCII case-insensitive), in order.
pub fn all<'a>(pairs: &'a [(String, String)], name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    pairs
        .iter()
        .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The value of the first `name: value` line of a header text (name ASCII
/// case-insensitive, both sides trimmed; no unfolding).
#[must_use]
pub fn in_lines<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (n, v) = line.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs() -> Vec<(String, String)> {
        [("Location", "/a"), ("X-Tag", "one"), ("x-tag", "two")]
            .into_iter()
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn names_match_without_case() {
        let pairs = pairs();
        assert_eq!(first(&pairs, "location"), Some("/a"));
        assert_eq!(first(&pairs, "X-TAG"), Some("one"));
        assert_eq!(first(&pairs, "missing"), None);
        assert_eq!(all(&pairs, "x-tag").collect::<Vec<_>>(), ["one", "two"]);
    }

    #[test]
    fn header_lines() {
        let head = "Content-Type: application/http\r\ncontent-id : <a>\r\n";
        assert_eq!(in_lines(head, "CONTENT-ID"), Some("<a>"));
        assert_eq!(in_lines(head, "content-type"), Some("application/http"));
        assert_eq!(in_lines(head, "x"), None);
    }
}
