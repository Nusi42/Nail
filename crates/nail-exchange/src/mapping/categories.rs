//! Categories ↔ tags (ADR 0072): an Exchange category is the Nail tag of
//! the same name, compared case-insensitively like Outlook; the master
//! category list gives tags their colours. A tag set on an Exchange
//! message is written as a category (Exchange accepts names outside the
//! master list), so tags never create master entries; renaming a category
//! renames the tag on messages ([`retag`], through the mail queue).

use nail_api::exchange::engine::RemoteCategory;
use nail_api::exchange::{
    CategoryColor, ExchangeCategory, ExchangeCategoryIssue, ExchangeCategorySettings,
};

/// Something with a category name and colour.
pub trait CategoryLike {
    /// The name.
    fn name(&self) -> &str;
    /// The colour.
    fn color(&self) -> CategoryColor;
}

impl CategoryLike for ExchangeCategory {
    fn name(&self) -> &str {
        &self.name
    }
    fn color(&self) -> CategoryColor {
        self.color
    }
}

impl CategoryLike for RemoteCategory {
    fn name(&self) -> &str {
        &self.name
    }
    fn color(&self) -> CategoryColor {
        self.color
    }
}

impl CategoryLike for ExchangeCategorySettings {
    fn name(&self) -> &str {
        &self.name
    }
    fn color(&self) -> CategoryColor {
        self.color
    }
}

/// Whether two category / tag names are the same (case-insensitive,
/// surrounding spaces ignored).
pub fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

/// The master entry of a tag.
pub fn find<'a, C: CategoryLike>(master: &'a [C], tag: &str) -> Option<&'a C> {
    master.iter().find(|c| same_name(c.name(), tag))
}

/// The colour Nail draws for a tag of an Exchange account (`#rrggbb`), if
/// the master list colours it.
pub fn tag_color<C: CategoryLike>(master: &[C], tag: &str) -> Option<&'static str> {
    find(master, tag).and_then(|c| c.color().rgb())
}

/// Whether a tag can be written as a category: a valid category name and
/// not an IMAP-style system keyword (`$Junk`, `\Seen`).
pub fn is_category_name(tag: &str) -> bool {
    !tag.starts_with(['$', '\\']) && ExchangeCategorySettings::named(tag).issues().is_empty()
}

/// The categories to write for a message's tags: valid names only,
/// case-insensitive duplicates dropped (the first spelling wins).
pub fn categories_for_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if is_category_name(tag) && !out.iter().any(|c| same_name(c, tag)) {
            out.push(tag.to_owned());
        }
    }
    out
}

/// A message's tags from its categories: the master list's spelling where
/// it has the category (so `work` and `Work` are one tag), duplicates
/// dropped.
pub fn tags_for_categories<C: CategoryLike>(categories: &[String], master: &[C]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for category in categories {
        let category = category.trim();
        if category.is_empty() {
            continue;
        }
        let name = find(master, category).map_or(category, |c| c.name());
        if !out.iter().any(|t| same_name(t, name)) {
            out.push(name.to_owned());
        }
    }
    out
}

/// A message's tags after renaming category `old` to `new`; `None` when
/// the message does not have the tag (nothing to queue).
pub fn retag(tags: &[String], old: &str, new: &str) -> Option<Vec<String>> {
    if !tags.iter().any(|t| same_name(t, old)) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = if same_name(tag, old) {
            new
        } else {
            tag.as_str()
        };
        if !out.iter().any(|t| same_name(t, tag)) {
            out.push(tag.to_owned());
        }
    }
    Some(out)
}

/// Problems of new or changed settings against the account's other
/// categories (`except` = the id being changed): the contract's own checks
/// plus `duplicate-name`.
pub fn issues(
    settings: &ExchangeCategorySettings,
    others: &[ExchangeCategory],
    except: Option<&str>,
) -> Vec<ExchangeCategoryIssue> {
    let mut out = settings.issues();
    let clash = others
        .iter()
        .filter(|c| Some(c.id.as_str()) != except)
        .any(|c| same_name(&c.name, &settings.name));
    if clash {
        out.push(ExchangeCategoryIssue::DuplicateName);
    }
    out
}

fn rgb_of(hex: &str) -> Option<(i32, i32, i32)> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let part = |i: usize| i32::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((part(0)?, part(2)?, part(4)?))
}

/// The Outlook colour closest to a Nail colour (`#rrggbb`), for creating a
/// category from a coloured tag; `none` for anything unreadable.
pub fn nearest_color(hex: &str) -> CategoryColor {
    let Some((r, g, b)) = rgb_of(hex) else {
        return CategoryColor::None;
    };
    CategoryColor::ALL
        .into_iter()
        .filter_map(|c| c.rgb().and_then(rgb_of).map(|rgb| (c, rgb)))
        .min_by_key(|(_, (cr, cg, cb))| {
            // Weighted ("redmean"-like) distance; good enough to pick a swatch.
            2 * (cr - r).pow(2) + 4 * (cg - g).pow(2) + 3 * (cb - b).pow(2)
        })
        .map_or(CategoryColor::None, |(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master() -> Vec<RemoteCategory> {
        vec![
            RemoteCategory {
                id: "1".into(),
                name: "Work".into(),
                color: CategoryColor::Preset7,
            },
            RemoteCategory {
                id: "2".into(),
                name: "Größe".into(),
                color: CategoryColor::None,
            },
        ]
    }

    #[test]
    fn names_match_case_insensitively() {
        assert!(same_name("work", " WORK "));
        assert!(same_name("GRÖSSE", "grösse"));
        assert!(!same_name("work", "works"));
        assert_eq!(find(&master(), "WORK").unwrap().id, "1");
        assert_eq!(tag_color(&master(), "work"), Some("#9db7e8"));
        assert_eq!(tag_color(&master(), "größe"), None, "no colour");
        assert_eq!(tag_color(&master(), "other"), None);
    }

    #[test]
    fn tags_and_categories() {
        let tags: Vec<String> = ["work", "Work", "$Junk", "a,b", " Travel ", ""]
            .map(String::from)
            .to_vec();
        assert_eq!(categories_for_tags(&tags), ["work", "Travel"]);
        let categories: Vec<String> = ["work", "Personal", "WORK", " "].map(String::from).to_vec();
        assert_eq!(
            tags_for_categories(&categories, &master()),
            ["Work", "Personal"]
        );
        assert!(is_category_name("Project X"));
        assert!(!is_category_name("\\Seen"));
        assert!(!is_category_name(&"x".repeat(300)));
    }

    #[test]
    fn renames_retag() {
        let tags: Vec<String> = ["work", "urgent"].map(String::from).to_vec();
        assert_eq!(
            retag(&tags, "Work", "Office").unwrap(),
            ["Office", "urgent"]
        );
        assert_eq!(retag(&tags, "home", "x"), None);
        // Renaming onto an existing tag merges them.
        assert_eq!(retag(&tags, "work", "URGENT").unwrap(), ["URGENT"]);
    }

    #[test]
    fn duplicate_names() {
        let others = vec![ExchangeCategory {
            account_id: "work".into(),
            id: "c1".into(),
            name: "Work".into(),
            color: CategoryColor::None,
            total: 0,
            unread: 0,
            sync: Default::default(),
        }];
        let settings = ExchangeCategorySettings::named("work");
        assert_eq!(
            issues(&settings, &others, None),
            [ExchangeCategoryIssue::DuplicateName]
        );
        assert!(
            issues(&settings, &others, Some("c1")).is_empty(),
            "renaming itself"
        );
        assert_eq!(
            issues(&ExchangeCategorySettings::named("a;b"), &others, None),
            [ExchangeCategoryIssue::InvalidName]
        );
    }

    #[test]
    fn nearest_colours() {
        for color in CategoryColor::ALL.into_iter().skip(1) {
            let rgb = color.rgb().unwrap();
            let found = nearest_color(rgb);
            // Two presets may share a swatch; the found one must draw the same.
            assert_eq!(found.rgb(), Some(rgb), "{color:?}");
        }
        assert_eq!(nearest_color("#ff0000"), CategoryColor::Preset15);
        assert_eq!(nearest_color("blue"), CategoryColor::None);
        assert_eq!(nearest_color("#12345"), CategoryColor::None);
    }
}
