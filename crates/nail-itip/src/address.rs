//! Calendar user addresses and the user's identities.
//!
//! iTIP names people by calendar user address (`mailto:` URIs in
//! `ORGANIZER` / `ATTENDEE`, RFC 5545 §3.3.3). Nail compares them as
//! email addresses: `mailto:` stripped, surrounding space and angle
//! brackets removed, ASCII case folded (local parts are case-insensitive
//! in practice; every real calendar server treats them so).

use nail_api::ids::IdentityId;

/// The comparable form of a calendar user address or email address
/// (`MAILTO:Ada@Example.org` → `ada@example.org`). Other URIs (`urn:…`,
/// `http:…`) are kept lowercase as they are.
pub fn normalize(address: &str) -> String {
    let mut text = address.trim();
    if let Some(prefix) = text.get(..7)
        && prefix.eq_ignore_ascii_case("mailto:")
    {
        text = &text[7..];
    }
    text.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_ascii_lowercase()
}

/// Whether two addresses name the same calendar user.
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    !a.is_empty() && a == b
}

/// `mailto:` URI of an address.
pub fn mailto(address: &str) -> String {
    format!("mailto:{}", address.trim())
}

/// One address of the user: an identity's sender address (or an alias
/// the core knows for it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAddress {
    /// The identity that answers / sends for this address (`None` = the
    /// address is the user's but nothing can send from it).
    pub identity_id: Option<IdentityId>,
    /// The address.
    pub email: String,
}

/// The user's addresses, in priority order (the identity of the account
/// a message arrived in first, then the others).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserAddresses {
    entries: Vec<UserAddress>,
}

impl UserAddresses {
    /// The addresses in priority order (duplicates keep the first).
    pub fn new(entries: impl IntoIterator<Item = UserAddress>) -> Self {
        let mut out: Vec<UserAddress> = Vec::new();
        for entry in entries {
            if normalize(&entry.email).is_empty() {
                continue;
            }
            if !out.iter().any(|e| same(&e.email, &entry.email)) {
                out.push(entry);
            }
        }
        Self { entries: out }
    }

    /// From `(identity, address)` pairs.
    pub fn from_identities<'a>(
        identities: impl IntoIterator<Item = (&'a IdentityId, &'a str)>,
    ) -> Self {
        Self::new(identities.into_iter().map(|(id, email)| UserAddress {
            identity_id: Some(id.clone()),
            email: email.to_owned(),
        }))
    }

    /// The entry for an address, if it is one of the user's.
    pub fn find(&self, address: &str) -> Option<&UserAddress> {
        let wanted = normalize(address);
        if wanted.is_empty() {
            return None;
        }
        self.entries.iter().find(|e| normalize(&e.email) == wanted)
    }

    /// Whether an address is one of the user's.
    pub fn contains(&self, address: &str) -> bool {
        self.find(address).is_some()
    }

    /// The position of an address in the priority order.
    pub fn rank(&self, address: &str) -> Option<usize> {
        let wanted = normalize(address);
        self.entries
            .iter()
            .position(|e| normalize(&e.email) == wanted)
    }

    /// Every entry.
    pub fn entries(&self) -> &[UserAddress] {
        &self.entries
    }

    /// Whether the user has no addresses.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_normalize() {
        assert_eq!(normalize("MAILTO:Ada@Example.org"), "ada@example.org");
        assert_eq!(normalize(" mailto: <bob@x.org> "), "bob@x.org");
        assert_eq!(normalize("urn:uuid:ABC"), "urn:uuid:abc");
        assert!(same("mailto:A@b.c", "a@B.c"));
        assert!(!same("", ""));
        assert_eq!(mailto(" a@b.c"), "mailto:a@b.c");
    }

    #[test]
    fn user_addresses_keep_priority_and_drop_duplicates() {
        let me = UserAddresses::new([
            UserAddress {
                identity_id: Some("work".into()),
                email: "Ada@Work.example".into(),
            },
            UserAddress {
                identity_id: Some("home".into()),
                email: "ada@home.example".into(),
            },
            UserAddress {
                identity_id: Some("dup".into()),
                email: "ada@work.example".into(),
            },
            UserAddress {
                identity_id: None,
                email: "  ".into(),
            },
        ]);
        assert_eq!(me.entries().len(), 2);
        assert_eq!(
            me.find("mailto:ADA@work.example")
                .and_then(|e| e.identity_id.clone()),
            Some("work".into())
        );
        assert_eq!(me.rank("ada@home.example"), Some(1));
        assert!(!me.contains("bob@work.example"));
        assert!(!me.contains(""));
        let ids: Vec<IdentityId> = vec!["a".into()];
        let from = UserAddresses::from_identities([(&ids[0], "a@x.org")]);
        assert!(from.contains("A@X.org") && !from.is_empty());
    }
}
