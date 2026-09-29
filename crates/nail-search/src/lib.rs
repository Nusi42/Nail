//! Search for Nail.
//!
//! **Stub (M1a WP10).** Work package WP13 fills this crate in:
//!
//! * a [`tantivy`] index over headers, decoded body text and attachment
//!   names, with incremental add / remove;
//! * the query language (`from: to: subject: has:attachment is:unread
//!   is:flagged tag: account: folder: before: after: size>`, quotes,
//!   `AND` / `OR` / `NOT`, parentheses) with a hand-written parser that
//!   reports [`nail_api::search::SearchError`]s with UTF-16 positions
//!   ([`nail_api::search::utf16_offset`]).
//!
//! Expected shape: `docs/adr/0012-m1a-contract.md`.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use tantivy::schema::{STORED, Schema, TEXT};
    use tantivy::{Index, doc};

    #[test]
    fn tantivy_indexes_in_ram() {
        let mut builder = Schema::builder();
        let subject = builder.add_text_field("subject", TEXT | STORED);
        let index = Index::create_in_ram(builder.build());
        let mut writer = index.writer(15_000_000).unwrap();
        writer
            .add_document(doc!(subject => "Quarterly report"))
            .unwrap();
        writer.commit().unwrap();
        let searcher = index.reader().unwrap().searcher();
        assert_eq!(searcher.num_docs(), 1);
        assert_eq!(super::CRATE, "nail-search");
    }
}
