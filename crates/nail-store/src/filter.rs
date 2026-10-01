//! Store-side message filters: the parts of a search that depend on
//! mutable state (flags, tags, folder, account) or on columns the store
//! owns (date, size). `nail-search` compiles the query language into a
//! [`MessageFilter`] whose full-text leaves are [`MessageFilter::Ids`]
//! (the hits of the Tantivy index) and runs it with
//! [`Store::filter_message_ids`].

use nail_api::{AccountId, FolderId, MessageId};
use rusqlite::ToSql;

use crate::error::Result;
use crate::store::Store;

/// A comparison for [`MessageFilter::Size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeCmp {
    /// `<`
    Less,
    /// `<=`
    LessOrEqual,
    /// `>`
    Greater,
    /// `>=`
    GreaterOrEqual,
}

impl SizeCmp {
    fn sql(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        }
    }
}

/// A boolean filter over messages.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageFilter {
    /// Every message.
    All,
    /// No message.
    None,
    /// All of the filters (empty = all messages).
    And(Vec<MessageFilter>),
    /// Any of the filters (empty = no message).
    Or(Vec<MessageFilter>),
    /// Not the filter.
    Not(Box<MessageFilter>),
    /// `\Seen` is set (`true`) or not (`false`).
    Seen(bool),
    /// `\Flagged` is set / not set.
    Flagged(bool),
    /// `\Answered` is set / not set.
    Answered(bool),
    /// `\Draft` is set / not set.
    Draft(bool),
    /// Has / has no attachments.
    HasAttachments(bool),
    /// Carries this tag (case-insensitive).
    Tag(String),
    /// A Gmail message in this inbox category (`category:social`,
    /// case-insensitive; M3b).
    GmailCategory(String),
    /// An Exchange inbox message classified like this (`focus:focused` /
    /// `focus:other`, lowercase; M3c).
    ExchangeFocus(String),
    /// In one of these accounts.
    AccountIn(Vec<AccountId>),
    /// In an account whose id, name or address equals this text
    /// (case-insensitive).
    AccountNamed(String),
    /// In one of these folders.
    FolderIn(Vec<FolderId>),
    /// In a folder whose name, path or role equals this text
    /// (case-insensitive).
    FolderNamed(String),
    /// Dated strictly before this instant (Unix seconds).
    DateBefore(i64),
    /// Dated at or after this instant (Unix seconds).
    DateOnOrAfter(i64),
    /// Raw size compared to this many bytes (messages of unknown size never
    /// match).
    Size(SizeCmp, u64),
    /// One of these messages (e.g. full-text hits).
    Ids(Vec<MessageId>),
}

impl MessageFilter {
    /// `a AND b`, flattening nested ANDs.
    pub fn and(self, other: MessageFilter) -> MessageFilter {
        match (self, other) {
            (MessageFilter::All, x) | (x, MessageFilter::All) => x,
            (MessageFilter::And(mut a), MessageFilter::And(b)) => {
                a.extend(b);
                MessageFilter::And(a)
            }
            (MessageFilter::And(mut a), x) => {
                a.push(x);
                MessageFilter::And(a)
            }
            (x, y) => MessageFilter::And(vec![x, y]),
        }
    }
}

fn json_list<T: std::fmt::Display>(items: &[T]) -> Result<String> {
    Ok(serde_json::to_string(
        &items.iter().map(ToString::to_string).collect::<Vec<_>>(),
    )?)
}

/// Compiles a filter to an SQL condition over `messages m`.
fn to_sql(filter: &MessageFilter, params: &mut Vec<Box<dyn ToSql>>) -> Result<String> {
    fn bind(params: &mut Vec<Box<dyn ToSql>>, p: Box<dyn ToSql>) -> String {
        params.push(p);
        format!("?{}", params.len())
    }
    Ok(match filter {
        MessageFilter::All => "1".into(),
        MessageFilter::None => "0".into(),
        MessageFilter::And(items) if items.is_empty() => "1".into(),
        MessageFilter::Or(items) if items.is_empty() => "0".into(),
        MessageFilter::And(items) | MessageFilter::Or(items) => {
            let op = if matches!(filter, MessageFilter::And(_)) {
                " AND "
            } else {
                " OR "
            };
            let parts = items
                .iter()
                .map(|f| to_sql(f, params))
                .collect::<Result<Vec<_>>>()?;
            format!("({})", parts.join(op))
        }
        MessageFilter::Not(inner) => format!("NOT {}", to_sql(inner, params)?),
        MessageFilter::Seen(v) => format!("(m.seen = {})", i32::from(*v)),
        MessageFilter::Flagged(v) => format!("(m.flagged = {})", i32::from(*v)),
        MessageFilter::Answered(v) => format!("(m.answered = {})", i32::from(*v)),
        MessageFilter::Draft(v) => format!("(m.draft = {})", i32::from(*v)),
        MessageFilter::HasAttachments(v) => format!("(m.has_attachments = {})", i32::from(*v)),
        MessageFilter::Tag(tag) => {
            let p = bind(params, Box::new(tag.clone()));
            format!(
                "EXISTS (SELECT 1 FROM message_tags t WHERE t.message_id = m.id AND t.tag = {p} COLLATE NOCASE)"
            )
        }
        MessageFilter::GmailCategory(category) => {
            let p = bind(params, Box::new(category.to_ascii_lowercase()));
            format!(
                "EXISTS (SELECT 1 FROM gmail_messages g WHERE g.message_id = m.id AND g.category = {p})"
            )
        }
        MessageFilter::ExchangeFocus(focus) => {
            let p = bind(params, Box::new(focus.to_ascii_lowercase()));
            format!(
                "EXISTS (SELECT 1 FROM exchange_items x WHERE x.message_id = m.id AND x.inference = {p})"
            )
        }
        MessageFilter::AccountIn(ids) => {
            let p = bind(params, Box::new(json_list(ids)?));
            format!("(m.account_id IN (SELECT value FROM json_each({p})))")
        }
        MessageFilter::AccountNamed(name) => {
            let p = bind(params, Box::new(name.clone()));
            format!(
                "(m.account_id IN (SELECT id FROM accounts WHERE id = {p} COLLATE NOCASE
                   OR name = {p} COLLATE NOCASE OR email = {p} COLLATE NOCASE))"
            )
        }
        MessageFilter::FolderIn(ids) => {
            let p = bind(params, Box::new(json_list(ids)?));
            format!("(m.folder_id IN (SELECT value FROM json_each({p})))")
        }
        MessageFilter::FolderNamed(name) => {
            let p = bind(params, Box::new(name.clone()));
            format!(
                "(m.folder_id IN (SELECT id FROM folders WHERE name = {p} COLLATE NOCASE
                   OR path = {p} COLLATE NOCASE OR role = {p} COLLATE NOCASE))"
            )
        }
        MessageFilter::DateBefore(t) => {
            let p = bind(params, Box::new(*t));
            format!("(m.date_sort < {p})")
        }
        MessageFilter::DateOnOrAfter(t) => {
            let p = bind(params, Box::new(*t));
            format!("(m.date_sort >= {p})")
        }
        MessageFilter::Size(cmp, n) => {
            let n = i64::try_from(*n).unwrap_or(i64::MAX);
            let p = bind(params, Box::new(n));
            format!("(m.size IS NOT NULL AND m.size {} {p})", cmp.sql())
        }
        MessageFilter::Ids(ids) => {
            let p = bind(params, Box::new(json_list(ids)?));
            format!("(m.id IN (SELECT value FROM json_each({p})))")
        }
    })
}

impl Store {
    /// The ids of all messages matching `filter`, newest first (by date,
    /// then id).
    pub fn filter_message_ids(&self, filter: &MessageFilter) -> Result<Vec<MessageId>> {
        let mut params: Vec<Box<dyn ToSql>> = Vec::new();
        let condition = to_sql(filter, &mut params)?;
        let sql = format!(
            "SELECT m.id FROM messages m WHERE {condition} ORDER BY m.date_sort DESC, m.id DESC"
        );
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(
                rusqlite::params_from_iter(params.iter().map(AsRef::as_ref)),
                |r| Ok(MessageId(r.get(0)?)),
            )?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Number of messages matching `filter`.
    pub fn count_messages(&self, filter: &MessageFilter) -> Result<u32> {
        let mut params: Vec<Box<dyn ToSql>> = Vec::new();
        let condition = to_sql(filter, &mut params)?;
        let sql = format!("SELECT COUNT(*) FROM messages m WHERE {condition}");
        self.with_conn(|conn| {
            Ok(conn.query_row(
                &sql,
                rusqlite::params_from_iter(params.iter().map(AsRef::as_ref)),
                |r| r.get(0),
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn and_flattens_and_drops_all() {
        let f = MessageFilter::All
            .and(MessageFilter::Seen(true))
            .and(MessageFilter::Flagged(true))
            .and(MessageFilter::All);
        assert_eq!(
            f,
            MessageFilter::And(vec![
                MessageFilter::Seen(true),
                MessageFilter::Flagged(true)
            ])
        );
    }

    #[test]
    fn compiles_parameters_in_order() {
        let mut params = Vec::new();
        let sql = to_sql(
            &MessageFilter::Or(vec![
                MessageFilter::Tag("work".into()),
                MessageFilter::Not(Box::new(MessageFilter::Size(SizeCmp::Greater, 10))),
                MessageFilter::And(vec![]),
                MessageFilter::Or(vec![]),
            ]),
            &mut params,
        )
        .unwrap();
        assert_eq!(params.len(), 2);
        assert!(sql.contains("t.tag = ?1"), "{sql}");
        assert!(sql.contains("m.size > ?2"), "{sql}");
        assert!(sql.ends_with("OR 1 OR 0)"), "{sql}");
    }
}
