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
//! `tests::labels_match_the_wire` style checks live with each enum's
//! module (`assert_wire_labels!`), so a renamed variant cannot drift from
//! its catalog entry.

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
            $vis const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire name (as serde writes it).
            $vis fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }

            /// The Fluent id of the user-facing label.
            $vis fn message_id(self) -> String {
                format!("{}-{}", $prefix, self.as_str())
            }

            /// The variant with this wire name.
            $vis fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == text)
            }
        }

        impl $crate::labels::Labeled for $name {
            const NAME: &'static str = stringify!($name);
            const VALUES: &'static [Self] = Self::ALL;

            fn label_id(self) -> String {
                self.message_id()
            }
        }
    };
}

/// Adds `ALL`, `as_str()` and `parse()` to an existing fieldless enum whose
/// serde names are the listed wire names (the one table of those names;
/// `assert_wire_names` checks it against serde in tests).
macro_rules! wire_names {
    ($name:ident { $($variant:ident = $wire:literal),+ $(,)? }) => {
        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire name (as serde writes it; also the config-file name).
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }

            /// The variant with this wire name.
            pub fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == text)
            }
        }
    };
}

/// Checks that every variant's `as_str` is what serde writes and that the
/// names are unique (tests of [`wire_names!`] enums).
#[cfg(test)]
pub(crate) fn assert_wire_names<T>(all: &[T], as_str: fn(T) -> &'static str)
where
    T: Copy + serde::Serialize + std::fmt::Debug,
{
    let mut seen = std::collections::BTreeSet::new();
    for &v in all {
        assert_eq!(
            serde_json::to_value(v).expect("serializes"),
            serde_json::json!(as_str(v)),
            "{v:?}"
        );
        assert!(seen.insert(as_str(v)), "duplicate wire name {}", as_str(v));
    }
}

/// [`assert_labels`] for each listed type (tests only):
/// `assert_wire_labels!(BackupItem, BackupIssue);`.
#[cfg(test)]
macro_rules! assert_wire_labels {
    ($($ty:ty),+ $(,)?) => {
        $( $crate::labels::assert_labels(<$ty>::ALL, <$ty>::as_str, <$ty>::message_id); )+
    };
}

/// What every `labeled_enum!` provides, for code generic over them (the
/// generated TypeScript value lists and Fluent id tables,
/// `ts_constants::Constants::labeled`).
pub(crate) trait Labeled: Copy + serde::Serialize + 'static {
    /// The Rust (and TypeScript) type name.
    const NAME: &'static str;
    /// Every variant, in declaration order (`ALL`).
    const VALUES: &'static [Self];
    /// The Fluent id of the label (`message_id`).
    fn label_id(self) -> String;
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
        pub(super) enum Sample : "sample" {
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
        assert_wire_labels!(Sample);
    }
}
