//! The built-in themes (`defaults/themes/*.kdl`; M5a, ADR 0102).
//!
//! Each built-in is an ordinary theme file extending the shipped base
//! (`defaults/theme.kdl`). This module embeds them and resolves them; the
//! theme list with user themes, the selection in the user's `theme.kdl`
//! and Follow Windows are WP152's (`nail_api::appearance` has the rules).

use nail_api::appearance::{self, ThemeInfo, ThemeKind, ThemeSource};
use nail_api::{ResolvedTheme, ThemeMode};

use crate::error::ConfigError;
use crate::theme::ThemeFile;

/// One embedded built-in theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTheme {
    /// The id (`nail_api::appearance::BUILTIN_THEMES`).
    pub id: &'static str,
    /// The file as named in errors and source locations.
    pub file: &'static str,
    /// The KDL text.
    pub kdl: &'static str,
}

/// The built-in themes, in gallery order (the order of
/// `nail_api::appearance::BUILTIN_THEMES`).
pub const BUILTIN_THEME_FILES: &[BuiltinTheme] = &[
    BuiltinTheme {
        id: "frost",
        file: "defaults/themes/frost.kdl",
        kdl: include_str!("../../../defaults/themes/frost.kdl"),
    },
    BuiltinTheme {
        id: "sage",
        file: "defaults/themes/sage.kdl",
        kdl: include_str!("../../../defaults/themes/sage.kdl"),
    },
    BuiltinTheme {
        id: "graphite",
        file: "defaults/themes/graphite.kdl",
        kdl: include_str!("../../../defaults/themes/graphite.kdl"),
    },
    BuiltinTheme {
        id: "nocturne",
        file: "defaults/themes/nocturne.kdl",
        kdl: include_str!("../../../defaults/themes/nocturne.kdl"),
    },
    BuiltinTheme {
        id: "high-contrast",
        file: "defaults/themes/high-contrast.kdl",
        kdl: include_str!("../../../defaults/themes/high-contrast.kdl"),
    },
];

/// The built-in theme with this id.
pub fn builtin_theme(id: &str) -> Option<&'static BuiltinTheme> {
    BUILTIN_THEME_FILES.iter().find(|t| t.id == id)
}

/// Resolves a built-in theme onto the base (`None` for an unknown id).
pub fn resolve_builtin_theme(id: &str) -> Option<Result<ResolvedTheme, ConfigError>> {
    builtin_theme(id).map(|t| crate::parse_theme(t.kdl, t.file))
}

/// A theme's kind from its `mode` (`system` / none reads as light: the base
/// values are the light ones).
pub fn kind_of_mode(mode: ThemeMode) -> ThemeKind {
    match mode {
        ThemeMode::Dark => ThemeKind::Dark,
        ThemeMode::HighContrast => ThemeKind::HighContrast,
        ThemeMode::Light | ThemeMode::System => ThemeKind::Light,
    }
}

/// The gallery entries of the built-in themes.
pub fn builtin_theme_infos() -> Vec<ThemeInfo> {
    BUILTIN_THEME_FILES
        .iter()
        .map(|t| {
            let file = ThemeFile::parse(t.kdl, t.file)
                .unwrap_or_else(|e| panic!("built-in theme {} is invalid: {e}", t.id));
            ThemeInfo {
                id: t.id.to_owned(),
                name: file.name.value.clone(),
                label: appearance::builtin_theme_label(t.id),
                kind: kind_of_mode(file.mode.map_or(ThemeMode::System, Into::into)),
                source: ThemeSource::Builtin,
                extends: file.extends.clone(),
                file: t.file.to_owned(),
                problem: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_match_the_contract_list() {
        let ids: Vec<_> = BUILTIN_THEME_FILES.iter().map(|t| t.id).collect();
        let contract: Vec<_> = appearance::BUILTIN_THEMES
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(ids, contract);
        for (info, (id, kind)) in builtin_theme_infos().iter().zip(appearance::BUILTIN_THEMES) {
            assert_eq!(info.id, *id);
            assert_eq!(
                info.kind, *kind,
                "{id}: mode does not match the contract kind"
            );
            assert_eq!(info.extends.as_deref(), Some(appearance::BASE_THEME_ID));
            assert_eq!(
                info.label.as_deref(),
                Some(format!("theme-name-{id}").as_str())
            );
            assert!(info.usable());
        }
    }

    #[test]
    fn every_builtin_resolves() {
        for t in BUILTIN_THEME_FILES {
            let theme = resolve_builtin_theme(t.id).unwrap().unwrap();
            assert!(theme.tokens.get("color", "accent").is_some(), "{}", t.id);
        }
        assert!(resolve_builtin_theme("nope").is_none());
    }

    #[test]
    fn kinds_from_modes() {
        assert_eq!(kind_of_mode(ThemeMode::System), ThemeKind::Light);
        assert_eq!(kind_of_mode(ThemeMode::Dark), ThemeKind::Dark);
        assert_eq!(
            kind_of_mode(ThemeMode::HighContrast),
            ThemeKind::HighContrast
        );
    }
}
