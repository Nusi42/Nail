//! Checks of the Fluent catalogs the engine crates' texts live in
//! (`locales/en/core-*.ftl` complete, `locales/de/*` empty for now): one
//! definition of the checks every error type's tests run.

/// Asserts that the English catalog `english` (the text of `file`) has a
/// message for every id and that the German one (`german`) is still empty.
///
/// # Panics
///
/// When an id is missing or the German catalog has a message.
pub fn assert_catalog<'a>(
    english: &str,
    german: &str,
    file: &str,
    ids: impl IntoIterator<Item = &'a str>,
) {
    for id in ids {
        assert!(
            english.lines().any(|l| l.starts_with(&format!("{id} ="))),
            "{id} missing in locales/en/{file}"
        );
    }
    assert!(
        german
            .lines()
            .all(|l| l.trim().is_empty() || l.starts_with('#')),
        "locales/de/{file} stays empty for now"
    );
}

/// Asserts that every error's user-facing text id (`text_id`) is one of
/// `listed` and that its English diagnostic (`Display`) is not empty.
///
/// # Panics
///
/// When an id is not listed or a diagnostic is empty.
pub fn assert_listed<E: std::fmt::Display>(
    errors: &[E],
    text_id: impl Fn(&E) -> String,
    listed: &[&str],
) {
    for error in errors {
        let id = text_id(error);
        assert!(listed.contains(&id.as_str()), "{id} is not listed");
        assert!(!error.to_string().is_empty(), "{id}: empty diagnostic");
    }
}

/// The test that an error type's texts are complete: every error's text id
/// is in `$ty::MESSAGE_IDS` (plus `$extra` ids) and every listed id has an
/// English message in `locales/en/$file`, while the German catalog stays
/// empty. Use inside a test module with a function `all()` listing one
/// error of each variant.
#[macro_export]
macro_rules! error_texts_test {
    ($ty:ident, $file:literal $(, $extra:expr)?) => {
        #[test]
        fn texts_are_listed_and_in_the_catalog() {
            $crate::catalog::assert_listed(&all(), |e| e.localized().id, &$ty::MESSAGE_IDS);
            $crate::catalog::assert_catalog(
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales/en/", $file)),
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales/de/", $file)),
                $file,
                $ty::MESSAGE_IDS.into_iter()$(.chain($extra))?,
            );
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_catalogs_pass() {
        assert_catalog("a-b = A\nc = C\n", "# empty\n\n", "x.ftl", ["a-b", "c"]);
        assert_listed(&["text"], |_| "a-b".to_owned(), &["a-b"]);
    }

    #[test]
    #[should_panic(expected = "d missing in locales/en/x.ftl")]
    fn a_missing_id_fails() {
        assert_catalog("a-b = A\n", "", "x.ftl", ["d"]);
    }

    #[test]
    #[should_panic(expected = "stays empty")]
    fn a_german_message_fails() {
        assert_catalog("", "a = B\n", "x.ftl", []);
    }

    #[test]
    #[should_panic(expected = "is not listed")]
    fn an_unlisted_id_fails() {
        assert_listed(&["t"], |_| "z".to_owned(), &["a"]);
    }
}
