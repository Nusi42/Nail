//! Contacts beyond plain storage: paged lists that show a merged person
//! once, duplicate detection, merging with links to the sources' cards,
//! the "Collected" book and address autocomplete ranking.

use std::collections::{BTreeSet, HashMap, HashSet};

use nail_api::pim::contact::{ContactKind, DuplicateReason, VCardVersion};
use nail_api::pim::{
    Contact, ContactFields, ContactSummary, DuplicateGroup, LabeledValue, PimItemSync,
    is_plausible_email,
};
use nail_api::{Address, AddressBookId, ContactGroupId, ContactId};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::PimItem;
use super::collections::COLLECTED_BOOK_ID;
use super::items::{
    PimWrite, delete_item_tx, in_list, linked_ids_of, load_contacts, member_key, save_item_tx,
    sort_name,
};
use super::ops::PimOpId;
use crate::error::{Error, Result};
use crate::store::Store;

/// Result of [`Store::merge_contacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    /// The primary contact as saved.
    pub contact: Contact,
    /// Contacts of the primary's book folded into it (deleted).
    pub folded: Vec<ContactId>,
    /// Contacts of other books now linked to it.
    pub linked: Vec<ContactId>,
    /// Groups that listed a folded contact: the core rewrites their
    /// `MEMBER`s to the primary and saves them.
    pub groups_to_update: Vec<ContactGroupId>,
    /// The queued operations.
    pub op_ids: Vec<PimOpId>,
}

/// Where an autocomplete suggestion comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionSource {
    /// A contact in an address book with autocomplete on.
    Contact(ContactId),
    /// An address the user wrote to or received from (the Collected book
    /// or the collected statistics).
    Collected,
}

/// One autocomplete suggestion with its score.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedAddress {
    /// Name and address.
    pub address: Address,
    /// Where it came from.
    pub source: SuggestionSource,
    /// Higher is better.
    pub score: f64,
}

/// Digits of a phone number for comparisons; `None` when it has fewer
/// than seven. The last nine digits are kept, so `+49 30 1234567` and
/// `030 1234567` compare equal.
pub fn normalize_phone(phone: &str) -> Option<String> {
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 7 {
        return None;
    }
    Some(digits[digits.len().saturating_sub(9)..].to_owned())
}

/// A name key: lower-case words, punctuation dropped, word order ignored
/// (`Lovelace, Ada` = `Ada Lovelace`); `None` without a letter.
fn name_key(fields: &ContactFields) -> Option<String> {
    let text = fields.display_name.to_lowercase();
    let mut words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ';' | '"' | '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    if !words.iter().any(|w| w.chars().any(char::is_alphabetic)) {
        return None;
    }
    words.sort();
    Some(words.join(" "))
}

/// Union of the primary's fields with the others' (the merge dialog's
/// starting point): lists are joined without duplicates, single values
/// are the primary's unless it has none.
pub fn merge_contact_fields(primary: &ContactFields, others: &[&ContactFields]) -> ContactFields {
    fn join_values(
        into: &mut Vec<LabeledValue>,
        from: &[LabeledValue],
        key: impl Fn(&str) -> String,
    ) {
        let mut seen: HashSet<String> = into.iter().map(|v| key(&v.value)).collect();
        for v in from {
            if seen.insert(key(&v.value)) {
                let mut v = v.clone();
                v.preferred = false;
                into.push(v);
            }
        }
    }
    fn join<T: Clone + PartialEq>(into: &mut Vec<T>, from: &[T]) {
        for v in from {
            if !into.contains(v) {
                into.push(v.clone());
            }
        }
    }
    fn fill<T: Clone>(into: &mut Option<T>, from: &Option<T>) {
        if into.is_none() {
            into.clone_from(from);
        }
    }
    let mut out = primary.clone();
    for other in others {
        join_values(&mut out.emails, &other.emails, |v| v.trim().to_lowercase());
        join_values(&mut out.phones, &other.phones, |v| {
            normalize_phone(v).unwrap_or_else(|| v.trim().to_owned())
        });
        join_values(&mut out.urls, &other.urls, |v| v.trim().to_owned());
        join_values(&mut out.impp, &other.impp, |v| v.trim().to_lowercase());
        join_values(&mut out.related, &other.related, |v| v.trim().to_owned());
        join(&mut out.addresses, &other.addresses);
        join(&mut out.nicknames, &other.nicknames);
        join(&mut out.categories, &other.categories);
        join(&mut out.custom, &other.custom);
        fill(&mut out.name, &other.name);
        fill(&mut out.organization, &other.organization);
        fill(&mut out.title, &other.title);
        fill(&mut out.role, &other.role);
        fill(&mut out.birthday, &other.birthday);
        fill(&mut out.anniversary, &other.anniversary);
        fill(&mut out.notes, &other.notes);
        fill(&mut out.photo, &other.photo);
        if out.display_name.trim().is_empty() {
            out.display_name.clone_from(&other.display_name);
        }
    }
    out
}

/// The list entry of a contact.
pub fn contact_summary(contact: &Contact) -> ContactSummary {
    let fields = &contact.fields;
    ContactSummary {
        id: contact.id.clone(),
        address_book_id: contact.address_book_id.clone(),
        display_name: fields.display_name.clone(),
        sort_name: sort_name(contact),
        kind: fields.kind,
        email: fields.primary_email().map(str::to_owned),
        emails: fields.emails.iter().map(|e| e.value.clone()).collect(),
        phone: fields
            .phones
            .iter()
            .find(|p| p.preferred)
            .or_else(|| fields.phones.first())
            .map(|p| p.value.clone()),
        organization: fields.organization.as_ref().map(|o| o.name.clone()),
        has_photo: fields.photo.is_some(),
        group_ids: contact.group_ids.clone(),
        linked_ids: contact.linked_ids.clone(),
        read_only: contact.read_only,
    }
}

/// Escapes a vCard text value.
fn vcard_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            c => out.push(c),
        }
    }
    out
}

/// The minimal vCard 4.0 of a collected address.
fn collected_vcard(uid: &str, name: &str, email: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{}\r\nFN:{}\r\nEMAIL:{}\r\nEND:VCARD\r\n",
        vcard_text(uid),
        vcard_text(name),
        vcard_text(email)
    )
}

fn hash_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// A union-find over contact indexes.
struct Sets {
    parent: Vec<usize>,
}

impl Sets {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut i = i;
        while self.parent[i] != root {
            let next = self.parent[i];
            self.parent[i] = root;
            i = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[a.max(b)] = a.min(b);
        }
    }
}

impl Store {
    /// A page of contacts of address books (every visible book when
    /// `None`; only members of `group` when given), by sort name. A
    /// contact linked to a primary that is listed too is left out (the
    /// person shows once; the primary carries `linked_ids`). Returns the
    /// total and the page.
    pub fn list_contacts(
        &self,
        books: Option<&[AddressBookId]>,
        group: Option<&ContactGroupId>,
        offset: u32,
        limit: u32,
    ) -> Result<(u32, Vec<ContactSummary>)> {
        let books: Vec<String> = match books {
            Some(ids) => ids.iter().map(|b| b.0.clone()).collect(),
            None => self
                .address_books(None)?
                .into_iter()
                .filter(|b| b.settings.visible)
                .map(|b| b.id.0)
                .collect(),
        };
        if books.is_empty() {
            return Ok((0, Vec::new()));
        }
        let refs: Vec<&str> = books.iter().map(String::as_str).collect();
        let (candidates, members): (Vec<(String, String)>, Option<HashSet<String>>) = self
            .with_conn(|conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT p.id, p.uid FROM pim_contacts p
                     WHERE p.kind <> 'group' AND {}
                       AND NOT EXISTS (
                           SELECT 1 FROM pim_contact_links l
                           JOIN pim_contacts h ON h.id = l.primary_id
                           WHERE l.contact_id = p.id AND {})
                     ORDER BY p.sort_name, p.id",
                    in_list("p.address_book_id", refs.len(), 1),
                    in_list("h.address_book_id", refs.len(), 1)
                ))?;
                let rows: Vec<(String, String)> = stmt
                    .query_map(rusqlite::params_from_iter(&refs), |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                let members = match group {
                    None => None,
                    Some(g) => {
                        let mut stmt = conn
                            .prepare("SELECT member FROM pim_group_members WHERE group_id = ?1")?;
                        let keys: HashSet<String> = stmt
                            .query_map([g.as_str()], |r| r.get(0))?
                            .collect::<rusqlite::Result<_>>()?;
                        Some(keys)
                    }
                };
                Ok((rows, members))
            })?;
        let candidates: Vec<String> = match members {
            None => candidates.into_iter().map(|(id, _)| id).collect(),
            Some(keys) => {
                let emails = self.emails_of(candidates.iter().map(|(id, _)| id.as_str()))?;
                candidates
                    .into_iter()
                    .filter(|(id, uid)| {
                        keys.contains(&member_key(uid))
                            || emails.get(id).is_some_and(|list| {
                                list.iter().any(|e| keys.contains(&format!("mailto:{e}")))
                            })
                    })
                    .map(|(id, _)| id)
                    .collect()
            }
        };
        let total = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
        let page: Vec<ContactId> = candidates
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(ContactId)
            .collect();
        let contacts = self.contacts_by_ids(&page)?;
        Ok((total, contacts.iter().map(contact_summary).collect()))
    }

    /// The addresses of contacts (lower-case), by contact id.
    fn emails_of<'a>(
        &self,
        ids: impl Iterator<Item = &'a str>,
    ) -> Result<HashMap<String, Vec<String>>> {
        let ids: Vec<&str> = ids.collect();
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for chunk in ids.chunks(500) {
            self.with_conn(|conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT contact_id, email FROM pim_contact_emails WHERE {}",
                    in_list("contact_id", chunk.len(), 1)
                ))?;
                let rows = stmt.query_map(rusqlite::params_from_iter(chunk), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                for row in rows {
                    let (id, email) = row?;
                    out.entry(id).or_default().push(email);
                }
                Ok(())
            })?;
        }
        Ok(out)
    }

    /// Contacts with this address (any book), by sort name.
    pub fn contacts_by_email(&self, email: &str) -> Result<Vec<Contact>> {
        let email = email.trim().to_lowercase();
        self.with_conn(|conn| {
            load_contacts(
                conn,
                "p.id IN (SELECT contact_id FROM pim_contact_emails WHERE email = ?1)",
                &[email.as_str()],
            )
        })
    }

    /// Groups of contacts that look like the same person — a shared
    /// address or phone number, or the same name — across the given books
    /// (all when `None`; the Collected book only when asked for by id).
    /// Contacts already linked count as one person and are reported by
    /// their primary.
    pub fn find_duplicate_contacts(
        &self,
        books: Option<&[AddressBookId]>,
    ) -> Result<Vec<DuplicateGroup>> {
        let mut contacts = self.contacts(books)?;
        if books.is_none() {
            contacts.retain(|c| c.address_book_id.as_str() != COLLECTED_BOOK_ID);
        }
        // One node per person: a linked contact counts as its primary.
        let index: HashMap<&str, usize> = contacts
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.as_str(), i))
            .collect();
        let primary_of: Vec<Option<String>> = self.with_conn(|conn| {
            let mut stmt = conn
                .prepare_cached("SELECT primary_id FROM pim_contact_links WHERE contact_id = ?1")?;
            contacts
                .iter()
                .map(|c| Ok(stmt.query_row([c.id.as_str()], |r| r.get(0)).optional()?))
                .collect()
        })?;
        let persons: Vec<usize> = primary_of
            .iter()
            .enumerate()
            .map(|(i, p)| {
                p.as_deref()
                    .and_then(|p| index.get(p).copied())
                    .unwrap_or(i)
            })
            .collect();
        // Keys → persons.
        let mut by_key: HashMap<(DuplicateReason, String), BTreeSet<usize>> = HashMap::new();
        for (i, contact) in contacts.iter().enumerate() {
            let person = persons[i];
            for email in &contact.fields.emails {
                let e = email.value.trim().to_lowercase();
                if !e.is_empty() {
                    by_key
                        .entry((DuplicateReason::Email, e))
                        .or_default()
                        .insert(person);
                }
            }
            for phone in &contact.fields.phones {
                if let Some(p) = normalize_phone(&phone.value) {
                    by_key
                        .entry((DuplicateReason::Phone, p))
                        .or_default()
                        .insert(person);
                }
            }
            if contact.fields.kind == ContactKind::Individual
                && let Some(name) = name_key(&contact.fields)
            {
                by_key
                    .entry((DuplicateReason::Name, name))
                    .or_default()
                    .insert(person);
            }
        }
        let mut person_sets = Sets::new(contacts.len());
        for members in by_key.values() {
            let members: Vec<usize> = members.iter().copied().collect();
            for pair in members.windows(2) {
                person_sets.union(pair[0], pair[1]);
            }
        }
        // Collect groups of persons.
        let mut groups: HashMap<usize, (BTreeSet<usize>, BTreeSet<u8>)> = HashMap::new();
        for ((reason, _), members) in &by_key {
            if members.len() < 2 {
                continue;
            }
            let root = person_sets.find(*members.iter().next().expect("non-empty"));
            let entry = groups.entry(root).or_default();
            entry.0.extend(members.iter().copied());
            entry.1.insert(match reason {
                DuplicateReason::Email => 0,
                DuplicateReason::Phone => 1,
                DuplicateReason::Name => 2,
            });
        }
        let mut out: Vec<(String, DuplicateGroup)> = groups
            .into_values()
            .map(|(members, reasons)| {
                let ids: Vec<ContactId> = members
                    .iter()
                    .map(|&person| contacts[person].id.clone())
                    .collect();
                let first = members
                    .iter()
                    .map(|&p| sort_name(&contacts[p]))
                    .min()
                    .unwrap_or_default();
                (
                    first,
                    DuplicateGroup {
                        contact_ids: ids,
                        reasons: reasons
                            .into_iter()
                            .map(|r| match r {
                                0 => DuplicateReason::Email,
                                1 => DuplicateReason::Phone,
                                _ => DuplicateReason::Name,
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        out.sort_by(|a, b| (&a.0, &a.1.contact_ids).cmp(&(&b.0, &b.1.contact_ids)));
        Ok(out.into_iter().map(|(_, g)| g).collect())
    }

    /// Merges contacts into `primary_id` (one transaction): `merged` (the
    /// primary with the chosen fields, `raw` its vCard) is saved; contacts
    /// of the primary's book are folded into it (deleted, their links move
    /// to the primary); contacts of other books keep their cards and are
    /// linked to it (as are contacts linked to them). Server books queue
    /// their writes.
    pub fn merge_contacts(
        &self,
        merged: &Contact,
        raw: &str,
        others: &[ContactId],
        now: i64,
    ) -> Result<MergeOutcome> {
        let primary_id = merged.id.clone();
        if others.is_empty() || others.contains(&primary_id) {
            return Err(Error::InvalidData(
                "merge needs other contacts than the primary".into(),
            ));
        }
        let primary = self
            .contact(&primary_id)?
            .ok_or_else(|| Error::not_found("contact", &primary_id))?;
        if merged.address_book_id != primary.address_book_id || merged.uid != primary.uid {
            return Err(Error::InvalidData(
                "the merged contact must stay in the primary's place".into(),
            ));
        }
        let mut other_contacts = Vec::new();
        for id in others {
            other_contacts.push(
                self.contact(id)?
                    .ok_or_else(|| Error::not_found("contact", id))?,
            );
        }
        self.with_tx(|tx| {
            let mut op_ids: Vec<PimOpId> = Vec::new();
            let mut folded = Vec::new();
            let mut linked = Vec::new();
            let mut groups_to_update: BTreeSet<String> = BTreeSet::new();
            // The primary heads the person: if it was linked elsewhere, that
            // head joins it instead.
            let old_head: Option<String> = tx
                .query_row(
                    "SELECT primary_id FROM pim_contact_links WHERE contact_id = ?1",
                    [primary_id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(head) = old_head {
                tx.execute(
                    "DELETE FROM pim_contact_links WHERE contact_id = ?1",
                    [primary_id.as_str()],
                )?;
                tx.execute(
                    "UPDATE pim_contact_links SET primary_id = ?2 WHERE primary_id = ?1",
                    params![head, primary_id.as_str()],
                )?;
                tx.execute(
                    "INSERT OR REPLACE INTO pim_contact_links (contact_id, primary_id)
                     VALUES (?1, ?2)",
                    params![head, primary_id.as_str()],
                )?;
            }
            for other in &other_contacts {
                let id = other.id.as_str();
                // Whatever pointed at it now points at the primary.
                tx.execute(
                    "UPDATE pim_contact_links SET primary_id = ?2
                     WHERE primary_id = ?1 AND contact_id <> ?2",
                    params![id, primary_id.as_str()],
                )?;
                tx.execute("DELETE FROM pim_contact_links WHERE primary_id = ?1", [id])?;
                if other.address_book_id == primary.address_book_id {
                    let mut stmt = tx.prepare(
                        "SELECT DISTINCT m.group_id FROM pim_group_members m
                         JOIN pim_contacts g ON g.id = m.group_id
                         WHERE g.address_book_id = ?1 AND m.member = ?2",
                    )?;
                    let groups: Vec<String> = stmt
                        .query_map(
                            params![other.address_book_id.as_str(), member_key(&other.uid)],
                            |r| r.get(0),
                        )?
                        .collect::<rusqlite::Result<_>>()?;
                    groups_to_update.extend(groups);
                    let write: PimWrite = delete_item_tx(
                        tx,
                        &nail_api::pim::PimItemRef::Contact {
                            contact_id: other.id.clone(),
                        },
                        now,
                    )?;
                    op_ids.extend(write.op_ids);
                    folded.push(other.id.clone());
                } else {
                    tx.execute(
                        "INSERT OR REPLACE INTO pim_contact_links (contact_id, primary_id)
                         VALUES (?1, ?2)",
                        params![id, primary_id.as_str()],
                    )?;
                    linked.push(other.id.clone());
                }
            }
            let write = save_item_tx(tx, &PimItem::Contact(merged.clone()), raw, now)?;
            op_ids.extend(write.op_ids);
            Ok(MergeOutcome {
                contact: merged.clone(),
                folded,
                linked,
                groups_to_update: groups_to_update.into_iter().map(ContactGroupId).collect(),
                op_ids,
            })
        })
        .and_then(|mut outcome| {
            outcome.contact = self
                .contact(&primary_id)?
                .ok_or_else(|| Error::not_found("contact", &primary_id))?;
            Ok(outcome)
        })
    }

    /// Undoes merge links of a contact: a linked contact stands alone
    /// again; for a primary, all its links go. Returns the contacts that
    /// were unlinked.
    pub fn unlink_contact(&self, id: &ContactId) -> Result<Vec<ContactId>> {
        self.with_tx(|tx| {
            let linked = linked_ids_of(tx, id.as_str())?;
            let as_member = tx.execute(
                "DELETE FROM pim_contact_links WHERE contact_id = ?1",
                [id.as_str()],
            )?;
            if as_member > 0 {
                return Ok(vec![id.clone()]);
            }
            tx.execute(
                "DELETE FROM pim_contact_links WHERE primary_id = ?1",
                [id.as_str()],
            )?;
            Ok(linked)
        })
    }

    /// Records addresses the user wrote to or received from: counts them
    /// for ranking ([`Store::record_addresses`]) and keeps an entry in the
    /// Collected book for each one that no other address book knows (the
    /// book is made with `book_name` when missing). A known address with a
    /// new name renames its Collected entry. Returns the Collected
    /// contacts written.
    pub fn collect_addresses(
        &self,
        addresses: &[Address],
        at: i64,
        book_name: &str,
    ) -> Result<Vec<ContactId>> {
        self.record_addresses(addresses, at)?;
        self.ensure_collected_book(book_name)?;
        let mut written = Vec::new();
        let mut seen = HashSet::new();
        for address in addresses {
            let email = address.email.trim().to_lowercase();
            if !is_plausible_email(&email) || !seen.insert(email.clone()) {
                continue;
            }
            let name = address
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty());
            let known = self.contacts_by_email(&email)?;
            if known
                .iter()
                .any(|c| c.address_book_id.as_str() != COLLECTED_BOOK_ID)
            {
                continue;
            }
            let existing = known.into_iter().next();
            let contact = match existing {
                Some(c) => {
                    let Some(name) = name else { continue };
                    if c.fields.display_name == name {
                        continue;
                    }
                    let mut c = c;
                    c.fields.display_name = name.to_owned();
                    c
                }
                None => {
                    let hash = hash_hex(&email);
                    let mut fields = ContactFields::named(name.unwrap_or(&email));
                    fields.emails = vec![LabeledValue::new(email.clone())];
                    Contact {
                        id: ContactId::new(format!("collected-{}", &hash[..20])),
                        address_book_id: AddressBookId::new(COLLECTED_BOOK_ID),
                        uid: format!(
                            "urn:uuid:{}-{}-{}-{}-{}",
                            &hash[..8],
                            &hash[8..12],
                            &hash[12..16],
                            &hash[16..20],
                            &hash[20..32]
                        ),
                        fields,
                        version: VCardVersion::V4,
                        group_ids: Vec::new(),
                        linked_ids: Vec::new(),
                        updated_at: None,
                        read_only: false,
                        sync: PimItemSync::Synced,
                    }
                }
            };
            let raw = collected_vcard(&contact.uid, &contact.fields.display_name, &email);
            self.save_pim_item(&PimItem::Contact(contact.clone()), &raw, at)?;
            written.push(contact.id);
        }
        Ok(written)
    }

    /// Address autocomplete: contacts of address books with autocomplete on
    /// plus collected addresses whose address, name or a word of the name
    /// starts with `prefix`, ranked by how well they match, whether they are
    /// a contact, how often (`use_count`) and how recently (`last_used`,
    /// against `now`) they were used. One entry per address, best first.
    pub fn suggest_recipients(
        &self,
        prefix: &str,
        limit: u32,
        now: i64,
    ) -> Result<Vec<RankedAddress>> {
        let prefix: String = prefix
            .trim()
            .chars()
            .take(256)
            .collect::<String>()
            .to_lowercase();
        if prefix.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let books: Vec<String> = self
            .address_books(None)?
            .into_iter()
            .filter(|b| b.settings.autocomplete && b.role.is_none())
            .map(|b| b.id.0)
            .collect();
        let pattern = format!("{}%", escape_like(&prefix));
        let word = format!("% {}%", escape_like(&prefix));
        // Contacts: (contact id, name, email, preferred).
        let contact_rows: Vec<(String, String, String, bool)> = if books.is_empty() {
            Vec::new()
        } else {
            self.with_conn(|conn| {
                let n = books.len();
                let mut stmt = conn.prepare(&format!(
                    "SELECT p.id, p.display_name, e.email, e.preferred
                     FROM pim_contact_emails e
                     JOIN pim_contacts p ON p.id = e.contact_id
                     WHERE {} AND (e.email LIKE ?{a} ESCAPE '\\'
                           OR p.display_name LIKE ?{a} ESCAPE '\\'
                           OR p.display_name LIKE ?{b} ESCAPE '\\')
                     LIMIT 500",
                    in_list("p.address_book_id", n, 1),
                    a = n + 1,
                    b = n + 2
                ))?;
                let mut args: Vec<String> = books.clone();
                args.push(pattern.clone());
                args.push(word.clone());
                let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })?
        };
        // Collected: (email, name, use_count, last_used).
        let collected_rows: Vec<(String, Option<String>, i64, i64)> = self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT email, name, use_count, last_used FROM collected_addresses
                 WHERE email LIKE ?1 ESCAPE '\\' OR name LIKE ?1 ESCAPE '\\'
                    OR name LIKE ?2 ESCAPE '\\'
                 ORDER BY use_count DESC, last_used DESC LIMIT 500",
            )?;
            let rows = stmt.query_map(params![pattern, word], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        // Usage of the contacts' addresses.
        let mut usage: HashMap<String, (i64, i64)> = collected_rows
            .iter()
            .map(|(e, _, n, t)| (e.to_lowercase(), (*n, *t)))
            .collect();
        let missing: Vec<String> = contact_rows
            .iter()
            .map(|(_, _, e, _)| e.clone())
            .filter(|e| !usage.contains_key(e))
            .collect();
        if !missing.is_empty() {
            self.with_conn(|conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT use_count, last_used FROM collected_addresses WHERE email = ?1",
                )?;
                for email in &missing {
                    if let Some(stats) = stmt
                        .query_row([email], |r| Ok((r.get(0)?, r.get(1)?)))
                        .optional()?
                    {
                        usage.insert(email.clone(), stats);
                    }
                }
                Ok(())
            })?;
        }
        let usage_score = |email: &str| -> f64 {
            let Some(&(count, last)) = usage.get(email) else {
                return 0.0;
            };
            let age_days = ((now - last).max(0) as f64) / 86_400.0;
            // Frequency fades to half over months; recency over weeks.
            2.0 * (1.0 + count.max(0) as f64).ln() * (0.5 + 0.5 * (-age_days / 90.0).exp())
                + 3.0 * (-age_days / 30.0).exp()
        };
        let match_score = |name: &str, email: &str| -> f64 {
            let name = name.to_lowercase();
            if email == prefix {
                5.0
            } else if name.starts_with(&prefix) {
                4.0
            } else if email.starts_with(&prefix) {
                3.0
            } else if name.split_whitespace().any(|w| w.starts_with(&prefix)) {
                2.5
            } else {
                0.0
            }
        };
        let mut best: HashMap<String, RankedAddress> = HashMap::new();
        let mut offer = |candidate: RankedAddress| {
            let key = candidate.address.email.to_lowercase();
            match best.get(&key) {
                Some(existing) if existing.score >= candidate.score => {}
                _ => {
                    best.insert(key, candidate);
                }
            }
        };
        for (id, name, email, preferred) in &contact_rows {
            let score = 4.0
                + match_score(name, email)
                + usage_score(email)
                + if *preferred { 0.5 } else { 0.0 };
            offer(RankedAddress {
                address: Address {
                    name: Some(name.clone()).filter(|n| !n.trim().is_empty()),
                    email: email.clone(),
                },
                source: SuggestionSource::Contact(ContactId::new(id.clone())),
                score,
            });
        }
        for (email, name, _, _) in &collected_rows {
            let lower = email.to_lowercase();
            let score = match_score(name.as_deref().unwrap_or(""), &lower) + usage_score(&lower);
            offer(RankedAddress {
                address: Address {
                    name: name.clone(),
                    email: email.clone(),
                },
                source: SuggestionSource::Collected,
                score,
            });
        }
        let mut out: Vec<RankedAddress> = best.into_values().collect();
        out.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.address.email.cmp(&b.address.email))
        });
        out.truncate(limit as usize);
        Ok(out)
    }
}

/// Escapes `%`, `_` and `\` for a `LIKE … ESCAPE '\'` pattern.
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use nail_api::pim::contact::ContactOrg;
    use nail_api::pim::{PimItemKind, PimItemRef};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::pim::item_ref;
    use crate::pim::testing::{self, BOOK, DAV_BOOK, raw};

    fn save(store: &Store, contact: &Contact) {
        store
            .save_pim_item(
                &PimItem::Contact(contact.clone()),
                &raw(contact.id.as_str()),
                1,
            )
            .unwrap();
    }

    fn in_book(mut c: Contact, book: &str) -> Contact {
        c.address_book_id = book.into();
        c
    }

    fn ids(list: &[ContactSummary]) -> Vec<&str> {
        list.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn phones_and_names_normalize() {
        assert_eq!(
            normalize_phone("+49 30 1234567"),
            normalize_phone("030 / 123 45 67")
        );
        assert_eq!(normalize_phone("112"), None);
        let a = ContactFields::named("Lovelace, Ada");
        let b = ContactFields::named("ada  lovelace");
        assert_eq!(name_key(&a), name_key(&b));
        assert_eq!(name_key(&ContactFields::named("1234")), None);
    }

    #[test]
    fn duplicates_across_books() {
        let store = testing::store();
        save(
            &store,
            &testing::contact("a1", "ua1", "Ada Lovelace", &["ada@example.org"]),
        );
        save(
            &store,
            &in_book(
                testing::contact("a2", "ua2", "A. Lovelace", &["ADA@example.org"]),
                DAV_BOOK,
            ),
        );
        let mut by_phone = testing::contact("a3", "ua3", "Countess", &[]);
        by_phone.fields.phones = vec![LabeledValue::new("+44 20 7946 0958")];
        let mut phone_twin = testing::contact("a4", "ua4", "Lovelace, Ada", &[]);
        phone_twin.fields.phones = vec![LabeledValue::new("020 7946 0958")];
        save(&store, &by_phone);
        save(&store, &phone_twin);
        save(
            &store,
            &testing::contact("b1", "ub1", "Bob", &["bob@example.org"]),
        );
        let groups = store.find_duplicate_contacts(None).unwrap();
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        // a1 ~ a2 by email, a1 ~ a4 by name, a4 ~ a3 by phone.
        assert_eq!(group.contact_ids.len(), 4);
        assert_eq!(
            group.reasons,
            [
                DuplicateReason::Email,
                DuplicateReason::Phone,
                DuplicateReason::Name
            ]
        );
        // Restricted to the DAV book: nothing.
        assert!(
            store
                .find_duplicate_contacts(Some(&[DAV_BOOK.into()]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn collected_entries_are_not_duplicates() {
        let store = testing::store();
        save(
            &store,
            &testing::contact("a1", "ua1", "Ada", &["ada@example.org"]),
        );
        store
            .collect_addresses(
                &[Address {
                    name: Some("Someone".into()),
                    email: "some@example.org".into(),
                }],
                1,
                "Collected",
            )
            .unwrap();
        save(
            &store,
            &testing::contact("s2", "us2", "Other", &["some@example.org"]),
        );
        assert!(store.find_duplicate_contacts(None).unwrap().is_empty());
    }

    #[test]
    fn merging_folds_same_book_and_links_others() {
        let store = testing::store();
        let primary = testing::contact("p", "up", "Ada Lovelace", &["ada@example.org"]);
        let same_book = testing::contact("s", "us", "Ada", &["ada@work.example"]);
        let other_book = in_book(
            testing::contact("o", "uo", "Ada L.", &["ada@example.org"]),
            DAV_BOOK,
        );
        for c in [&primary, &same_book, &other_book] {
            save(&store, c);
        }
        // A group of the primary's book lists the folded contact.
        store
            .save_pim_item(
                &PimItem::group(testing::group("g", "ug", "Friends", &["s"])),
                &raw("g"),
                1,
            )
            .unwrap();
        let mut merged = primary.clone();
        merged.fields =
            merge_contact_fields(&primary.fields, &[&same_book.fields, &other_book.fields]);
        let outcome = store
            .merge_contacts(&merged, &raw("merged"), &["s".into(), "o".into()], 2)
            .unwrap();
        assert_eq!(outcome.folded, [ContactId::new("s")]);
        assert_eq!(outcome.linked, [ContactId::new("o")]);
        assert_eq!(outcome.groups_to_update, [ContactGroupId::new("g")]);
        assert_eq!(
            outcome
                .contact
                .fields
                .emails
                .iter()
                .map(|e| e.value.as_str())
                .collect::<Vec<_>>(),
            ["ada@example.org", "ada@work.example"]
        );
        assert_eq!(outcome.contact.linked_ids, [ContactId::new("o")]);
        assert!(store.contact(&"s".into()).unwrap().is_none());
        // The linked one keeps its card and knows its primary.
        let linked = store.contact(&"o".into()).unwrap().unwrap();
        assert_eq!(linked.linked_ids, [ContactId::new("p")]);
        // Lists show the person once (when the primary is listed).
        let (total, all) = store.list_contacts(None, None, 0, 10).unwrap();
        assert_eq!((total, ids(&all)), (1, vec!["p"]));
        let (_, dav_only) = store
            .list_contacts(Some(&[DAV_BOOK.into()]), None, 0, 10)
            .unwrap();
        assert_eq!(ids(&dav_only), ["o"]);
        // No longer duplicates.
        assert!(store.find_duplicate_contacts(None).unwrap().is_empty());
        // Unlinking shows it on its own again.
        assert_eq!(
            store.unlink_contact(&"o".into()).unwrap(),
            [ContactId::new("o")]
        );
        assert_eq!(store.list_contacts(None, None, 0, 10).unwrap().0, 2);
        // The server book's link queued nothing; the merged primary is local.
        assert!(outcome.op_ids.is_empty());
    }

    #[test]
    fn merging_moves_existing_links_to_the_new_primary() {
        let store = testing::store();
        let a = testing::contact("a", "ua", "A", &["a@example.org"]);
        let b = in_book(
            testing::contact("b", "ub", "B", &["b@example.org"]),
            DAV_BOOK,
        );
        let c = in_book(
            testing::contact("c", "uc", "C", &["c@example.org"]),
            DAV_BOOK,
        );
        for x in [&a, &b, &c] {
            save(&store, x);
        }
        // b heads c.
        let outcome = store
            .merge_contacts(&b, &raw("b"), &["c".into()], 1)
            .unwrap();
        assert_eq!(outcome.folded, [ContactId::new("c")]);
        let d = in_book(
            testing::contact("d", "ud", "D", &["d@example.org"]),
            DAV_BOOK,
        );
        save(&store, &d);
        store
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO pim_contact_links (contact_id, primary_id) VALUES ('d', 'b')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        // a (local book) merges b: b and its linked d now point at a.
        store
            .merge_contacts(&a, &raw("a"), &["b".into()], 2)
            .unwrap();
        let a = store.contact(&"a".into()).unwrap().unwrap();
        assert_eq!(a.linked_ids, [ContactId::new("b"), ContactId::new("d")]);
        // Merging the other way round: d becomes the head.
        let d = store.contact(&"d".into()).unwrap().unwrap();
        store
            .merge_contacts(&d, &raw("d"), &["a".into()], 3)
            .unwrap();
        let d = store.contact(&"d".into()).unwrap().unwrap();
        assert_eq!(d.linked_ids, [ContactId::new("a"), ContactId::new("b")]);
        // Unlinking a primary drops all its links.
        assert_eq!(
            store.unlink_contact(&"d".into()).unwrap(),
            [ContactId::new("a"), ContactId::new("b")]
        );
        assert!(
            store
                .contact(&"a".into())
                .unwrap()
                .unwrap()
                .linked_ids
                .is_empty()
        );
    }

    #[test]
    fn merge_is_checked() {
        let store = testing::store();
        let a = testing::contact("a", "ua", "A", &[]);
        save(&store, &a);
        assert!(store.merge_contacts(&a, &raw("a"), &[], 1).is_err());
        assert!(
            store
                .merge_contacts(&a, &raw("a"), &["a".into()], 1)
                .is_err()
        );
        assert!(
            store
                .merge_contacts(&a, &raw("a"), &["nobody".into()], 1)
                .is_err()
        );
        let mut moved = a.clone();
        moved.address_book_id = DAV_BOOK.into();
        save(&store, &testing::contact("b", "ub", "B", &[]));
        assert!(
            store
                .merge_contacts(&moved, &raw("a"), &["b".into()], 1)
                .is_err()
        );
    }

    #[test]
    fn merge_queues_server_writes() {
        let store = testing::store();
        let p = in_book(
            testing::contact("p", "up", "P", &["p@example.org"]),
            DAV_BOOK,
        );
        let s = in_book(
            testing::contact("s", "us", "S", &["p@example.org"]),
            DAV_BOOK,
        );
        for c in [&p, &s] {
            store
                .apply_server_item(
                    &PimItem::Contact(c.clone()),
                    &raw(c.id.as_str()),
                    Some(&format!("/card/{}.vcf", c.id)),
                    Some("1"),
                    1,
                )
                .unwrap();
        }
        let outcome = store
            .merge_contacts(&p, &raw("p2"), &["s".into()], 2)
            .unwrap();
        // A delete of s and an upload of p.
        assert_eq!(outcome.op_ids.len(), 2);
        let kinds: Vec<&str> = outcome
            .op_ids
            .iter()
            .map(|id| store.pim_op(*id).unwrap().operation.kind())
            .collect();
        assert_eq!(kinds, ["delete", "put"]);
    }

    #[test]
    fn merged_fields_union() {
        let mut a = ContactFields::named("Ada");
        a.emails = vec![LabeledValue::new("ada@example.org")];
        a.phones = vec![LabeledValue::new("+49 30 1234567")];
        let mut b = ContactFields::named("Ada L.");
        b.emails = vec![
            LabeledValue::new("ADA@example.org"),
            LabeledValue {
                preferred: true,
                ..LabeledValue::new("ada@work.example")
            },
        ];
        b.phones = vec![
            LabeledValue::new("030 1234567"),
            LabeledValue::new("0170 9999999"),
        ];
        b.organization = Some(ContactOrg {
            name: "Analytical Engines".into(),
            units: Vec::new(),
        });
        b.categories = vec!["math".into()];
        let merged = merge_contact_fields(&a, &[&b]);
        assert_eq!(merged.display_name, "Ada");
        assert_eq!(merged.emails.len(), 2);
        assert!(!merged.emails[1].preferred);
        assert_eq!(merged.phones.len(), 2);
        assert_eq!(merged.organization.unwrap().name, "Analytical Engines");
        assert_eq!(merged.categories, ["math"]);
    }

    #[test]
    fn contact_lists_page_and_filter_by_group() {
        let store = testing::store();
        for (id, name) in [("c1", "Carol"), ("c2", "alice"), ("c3", "Bob")] {
            save(
                &store,
                &testing::contact(id, &format!("u{id}"), name, &[&format!("{id}@example.org")]),
            );
        }
        let (total, page) = store
            .list_contacts(Some(&[BOOK.into()]), None, 1, 1)
            .unwrap();
        assert_eq!(total, 3);
        assert_eq!(ids(&page), ["c3"]);
        let mut group = testing::group("g", "ug", "Team", &["c1"]);
        group.member_emails = vec!["c2@example.org".into()];
        store
            .save_pim_item(&PimItem::group(group), &raw("g"), 1)
            .unwrap();
        let (total, members) = store.list_contacts(None, Some(&"g".into()), 0, 10).unwrap();
        assert_eq!(total, 2);
        assert_eq!(ids(&members), ["c2", "c1"]);
        assert_eq!(members[1].group_ids, [ContactGroupId::new("g")]);
        assert_eq!(members[0].email.as_deref(), Some("c2@example.org"));
        // Hidden books are left out unless named.
        let mut book = store.address_book(&BOOK.into()).unwrap().unwrap();
        book.settings.visible = false;
        store
            .update_address_book_settings(&BOOK.into(), &book.settings)
            .unwrap();
        assert_eq!(store.list_contacts(None, None, 0, 10).unwrap().0, 0);
        assert_eq!(store.contacts_by_email("C1@example.org").unwrap().len(), 1);
    }

    #[test]
    fn collected_book() {
        let store = testing::store();
        save(
            &store,
            &testing::contact("a", "ua", "Ada", &["ada@example.org"]),
        );
        let addr = |name: Option<&str>, email: &str| Address {
            name: name.map(str::to_owned),
            email: email.into(),
        };
        let written = store
            .collect_addresses(
                &[
                    addr(Some("Ada"), "ada@example.org"),
                    addr(None, "Eve@Example.org"),
                    addr(Some("Eve"), "eve@example.org"),
                    addr(None, "not an address"),
                ],
                10,
                "Collected",
            )
            .unwrap();
        // Ada is a real contact; Eve is collected once.
        assert_eq!(written.len(), 1);
        let eve = store.contact(&written[0]).unwrap().unwrap();
        assert_eq!(eve.address_book_id.as_str(), COLLECTED_BOOK_ID);
        assert_eq!(eve.fields.display_name, "eve@example.org");
        let object = store
            .pim_object(&item_ref(PimItemKind::Contact, written[0].as_str()))
            .unwrap()
            .unwrap();
        assert!(object.raw.contains("EMAIL:eve@example.org"));
        // A name arrives later: the entry is renamed (same id).
        let again = store
            .collect_addresses(
                &[addr(Some("Eve; Admin"), "eve@example.org")],
                11,
                "Collected",
            )
            .unwrap();
        assert_eq!(again, written);
        let object = store
            .pim_object(&PimItemRef::Contact {
                contact_id: written[0].clone(),
            })
            .unwrap()
            .unwrap();
        assert!(object.raw.contains("FN:Eve\\; Admin"));
        assert!(
            store
                .collect_addresses(&[addr(None, "eve@example.org")], 12, "C")
                .unwrap()
                .is_empty()
        );
        // The usage was counted.
        assert_eq!(store.suggest_addresses("eve", 5).unwrap().len(), 1);
    }

    #[test]
    fn autocomplete_ranks_contacts_frequency_and_recency() {
        let store = testing::store();
        let day = 86_400;
        let now = 1_000 * day;
        let mut ada = testing::contact("ada", "u1", "Ada Lovelace", &[]);
        ada.fields.emails = vec![
            LabeledValue::new("ada@home.example"),
            LabeledValue {
                preferred: true,
                ..LabeledValue::new("ada@work.example")
            },
        ];
        save(&store, &ada);
        save(
            &store,
            &testing::contact("al", "u2", "Alan Turing", &["alan@example.org"]),
        );
        let addr = |email: &str| Address {
            name: None,
            email: email.into(),
        };
        // Collected only: often long ago, once today, often lately.
        for _ in 0..20 {
            store
                .record_addresses(&[addr("alfred@old.example")], now - 400 * day)
                .unwrap();
            store
                .record_addresses(&[addr("alba@busy.example")], now - day)
                .unwrap();
        }
        store
            .record_addresses(&[addr("alma@new.example")], now)
            .unwrap();
        // Ada's home address is used a lot recently.
        for _ in 0..5 {
            store
                .record_addresses(&[addr("ada@home.example")], now - day)
                .unwrap();
        }
        let ranked = store.suggest_recipients("al", 10, now).unwrap();
        let emails: Vec<&str> = ranked.iter().map(|r| r.address.email.as_str()).collect();
        // Frequent and recent use beats being a contact; a contact beats
        // occasional use; old use fades.
        assert_eq!(
            emails,
            [
                "alba@busy.example",
                "alan@example.org",
                "alma@new.example",
                "alfred@old.example"
            ]
        );
        assert_eq!(ranked[0].source, SuggestionSource::Collected);
        assert_eq!(ranked[1].source, SuggestionSource::Contact("al".into()));
        assert_eq!(ranked[1].address.name.as_deref(), Some("Alan Turing"));
        let ada_hits = store.suggest_recipients("ada", 10, now).unwrap();
        let emails: Vec<&str> = ada_hits.iter().map(|r| r.address.email.as_str()).collect();
        // Recent use beats the preferred flag; one entry per address.
        assert_eq!(emails, ["ada@home.example", "ada@work.example"]);
        assert_eq!(ada_hits[0].address.name.as_deref(), Some("Ada Lovelace"));
        // A word of the name matches.
        assert_eq!(
            store.suggest_recipients("turing", 10, now).unwrap().len(),
            1
        );
        assert!(store.suggest_recipients(" ", 10, now).unwrap().is_empty());
        assert_eq!(store.suggest_recipients("a", 1, now).unwrap().len(), 1);
        // Books with autocomplete off are skipped.
        let mut book = store.address_book(&BOOK.into()).unwrap().unwrap();
        book.settings.autocomplete = false;
        store
            .update_address_book_settings(&BOOK.into(), &book.settings)
            .unwrap();
        let emails: Vec<String> = store
            .suggest_recipients("al", 10, now)
            .unwrap()
            .into_iter()
            .map(|r| r.address.email)
            .collect();
        assert!(!emails.contains(&"alan@example.org".to_owned()));
        // LIKE wildcards are literal.
        assert!(store.suggest_recipients("%", 10, now).unwrap().is_empty());
    }
}
