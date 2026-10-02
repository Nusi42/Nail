//! A helper for the many small "kebab-case wire name + Fluent id" enums of
//! the M4c contract (ADR 0094).
//!
//! `labeled_enum!` declares a fieldless enum whose variants each carry their
//! wire name, and derives the usual traits plus:
//!
//! * `ALL` — every variant in declaration order;
//! * `as_str()` — the wire name (the same string serde writes);
//! * `message_id()` — `<prefix>-<wire name>`, the Fluent id the UI shows.
//!
//! `tests::wire_names_match_serde` style checks live with each enum's
//! module (`assert_labels`), so a renamed variant cannot drift from its
//! catalog entry.

/// Declares a labeled enum (see the module docs).
macro_rules! labeled_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident : $prefix:literal {
            $( $(#[$vmeta:meta])* $variant:ident = $wire:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize, ts_rs::TS,
        )]
        $vis enum $name {
            $( $(#[$vmeta])* #[serde(rename = $wire)] $variant ),+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire name (as serde writes it).
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }

            /// The Fluent id of the user-facing label.
            pub fn message_id(self) -> String {
                format!("{}-{}", $prefix, self.as_str())
            }

            /// The variant with this wire name.
            pub fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == text)
            }
        }
    };
}

/// Checks that every variant's `as_str` is what serde writes, round-trips
/// through `parse`, and gives a valid Fluent id (tests only).
#[cfg(test)]
pub(crate) fn assert_labels<T>(all: &[T], as_str: fn(T) -> &'static str, id: fn(T) -> String)
where
    T: Copy + serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let mut seen = std::collections::BTreeSet::new();
    for &v in all {
        let wire = serde_json::to_value(v).expect("serializes");
        assert_eq!(wire, serde_json::json!(as_str(v)), "{v:?}");
        let back: T = serde_json::from_value(wire).expect("parses");
        assert_eq!(back, v);
        assert!(seen.insert(as_str(v)), "duplicate wire name {}", as_str(v));
        assert!(
            crate::localized::is_message_id(&id(v)),
            "{v:?}: bad Fluent id {}",
            id(v)
        );
    }
}

#[cfg(test)]
mod tests {
    labeled_enum! {
        /// A test enum.
        pub enum Sample : "sample" {
            /// One.
            One = "one",
            /// Two words.
            TwoWords = "two-words",
        }
    }

    #[test]
    fn generated_helpers() {
        assert_eq!(Sample::ALL, &[Sample::One, Sample::TwoWords]);
        assert_eq!(Sample::TwoWords.as_str(), "two-words");
        assert_eq!(Sample::TwoWords.message_id(), "sample-two-words");
        assert_eq!(Sample::parse("one"), Some(Sample::One));
        assert_eq!(Sample::parse("three"), None);
        super::assert_labels(Sample::ALL, Sample::as_str, Sample::message_id);
    }
}
