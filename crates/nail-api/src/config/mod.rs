//! Resolved configuration models shared by `nail-config` (producer) and the
//! UI renderer (consumer). See `docs/adr/0003-config-models.md`.

pub mod layout;
pub mod theme;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A position in a configuration file, used by the inspector and by
/// validation errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct SourceLocation {
    /// File name as shown to the user, e.g. `"theme.kdl"`.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column (in characters).
    pub column: u32,
}
