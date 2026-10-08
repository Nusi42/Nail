//! Small SQL helpers several store modules share (wave Q2, ADR 0171).

/// SQLite's current time as RFC 3339 UTC with milliseconds
/// (`updated_at` / `created_at` columns).
pub(crate) const SQL_NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// The `strftime` format of RFC 3339 UTC with whole seconds, quoted for SQL
/// (`strftime({SQL_RFC3339}, column, 'unixepoch')`).
pub(crate) const SQL_RFC3339: &str = "'%Y-%m-%dT%H:%M:%SZ'";

/// Escapes `%`, `_` and `\` for a `LIKE … ESCAPE '\'` pattern.
pub(crate) fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Every row of the statement as a tuple of its columns (`(A, B, …)`).
pub(crate) fn collect_rows<T>(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> crate::Result<Vec<T>>
where
    T: for<'r> TryFrom<&'r rusqlite::Row<'r>, Error = rusqlite::Error>,
{
    let rows = stmt.query_map(params, |r| T::try_from(r))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The blob hashes the statement's first column holds.
pub(crate) fn blob_hashes(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> crate::Result<Vec<crate::blob::BlobHash>> {
    let hashes = stmt
        .query_map(params, |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    hashes
        .iter()
        .map(|h| crate::blob::BlobHash::parse(h))
        .collect()
}

/// `key -> (total, unread)` from the rows `key, total, unread` of `select`
/// over an account (message counts per tag or label).
pub(crate) fn counts_by_key(
    conn: &rusqlite::Connection,
    select: &str,
    account: &nail_api::AccountId,
) -> crate::Result<std::collections::HashMap<String, (u32, u32)>> {
    let mut stmt = conn.prepare_cached(select)?;
    let rows = stmt.query_map([account.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The serde name of a unit enum value ([`Error::InvalidData`] for a
/// value serde does not write as a string).
pub(crate) fn enum_name<T: serde::Serialize>(value: T) -> crate::Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(crate::Error::InvalidData(format!(
            "not a string enum: {other}"
        ))),
    }
}

/// [`enum_name`], `""` for a value without one.
pub(crate) fn enum_str<T: serde::Serialize>(value: &T) -> String {
    enum_name(value).unwrap_or_default()
}

/// A unit enum value from its serde name in column `idx`.
pub(crate) fn enum_from<T: serde::de::DeserializeOwned>(
    idx: usize,
    s: String,
) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// [`enum_from`] of the first column.
pub(crate) fn enum_parse<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    enum_from(0, text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "kebab-case")]
        enum Kind {
            OnDemand,
        }
        assert_eq!(enum_name(Kind::OnDemand).unwrap(), "on-demand");
        assert_eq!(enum_str(&Kind::OnDemand), "on-demand");
        assert_eq!(enum_str(&1), "");
        assert!(enum_name(1).is_err());
        assert_eq!(enum_parse::<Kind>("on-demand").unwrap(), Kind::OnDemand);
        assert!(enum_from::<Kind>(2, "other".into()).is_err());
    }

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(escape_like("50%_a\\b"), "50\\%\\_a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn sql_times_have_the_expected_form() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let now: String = conn
            .query_row(&format!("SELECT {SQL_NOW}"), [], |r| r.get(0))
            .unwrap();
        assert!(now.ends_with('Z') && now.len() == 24, "{now}");
        let epoch: String = conn
            .query_row(
                &format!("SELECT strftime({SQL_RFC3339}, 0, 'unixepoch')"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(epoch, "1970-01-01T00:00:00Z");
    }
}
