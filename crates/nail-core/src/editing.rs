//! M5d in-app editors (ADR 0131): the undo history (`GetEditHistory`,
//! `UndoEdit`, `RedoEdit`) and the theme editor's requests
//! (`GetThemeDetails`, `EditTheme`, `DuplicateTheme`, `RenameTheme`,
//! `DeleteTheme`). Layout edits stay `UpdateLayout` (`customize.rs`).
//!
//! The work is done by [`nail_config::ConfigManager`] (the history in
//! `edit_history.rs`, the theme operations in `theme_edit.rs` — WP184's
//! slot); this module broadcasts the resulting config updates and an
//! [`Event::EditHistoryChanged`] after every write of a scope
//! ([`broadcast_history`], also used by `customize.rs` and `appearance.rs`).

use nail_api::editing::HistoryScope;
use nail_api::{ApiError, Event, Request, Response};
use nail_config::ConfigManager;

use crate::error::Result;
use crate::service::Service;

/// Sends the scope's history (after a write, an undo / redo, or a change of
/// its files on disk).
pub(crate) fn broadcast_history(service: &Service, scope: HistoryScope) {
    service.broadcast(Event::EditHistoryChanged {
        history: service.config().edit_history(scope),
    });
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    on_config(service, request, handle_blocking).await
}

/// Runs `handler` for a request on the config files: off the async
/// runtime, since the config manager does blocking file I/O.
pub(crate) async fn on_config(
    service: &Service,
    request: Request,
    handler: fn(&Service, &ConfigManager, Request) -> Result<Response>,
) -> Result<Response> {
    let config = service.config().clone();
    let service = service.clone();
    tokio::task::spawn_blocking(move || handler(&service, &config, request)).await?
}

fn handle_blocking(
    service: &Service,
    config: &ConfigManager,
    request: Request,
) -> Result<Response> {
    Ok(match request {
        Request::GetEditHistory { scope } => Response::EditHistory {
            history: config.edit_history(scope),
        },
        Request::UndoEdit { scope, force } => {
            let (history, updates) = config.undo_edit(scope, force)?;
            service.broadcast_config(&updates);
            service.broadcast(Event::EditHistoryChanged {
                history: history.clone(),
            });
            Response::EditHistory { history }
        }
        Request::RedoEdit { scope, force } => {
            let (history, updates) = config.redo_edit(scope, force)?;
            service.broadcast_config(&updates);
            service.broadcast(Event::EditHistoryChanged {
                history: history.clone(),
            });
            Response::EditHistory { history }
        }
        Request::GetThemeDetails { theme_id } => Response::ThemeDetails {
            details: Box::new(config.theme_details(&theme_id)?),
        },
        Request::EditTheme {
            target,
            patches,
            base_revision,
        } => {
            let (appearance, updates) =
                config.edit_theme(&target, &patches, base_revision.as_deref())?;
            service.broadcast_config(&updates);
            broadcast_history(service, HistoryScope::Appearance);
            Response::Themes {
                appearance: Box::new(appearance),
            }
        }
        Request::DuplicateTheme { theme_id, name } => {
            let (theme_id, appearance, updates) = config.duplicate_theme(&theme_id, &name)?;
            service.broadcast_config(&updates);
            broadcast_history(service, HistoryScope::Appearance);
            Response::ThemeSaved {
                theme_id,
                appearance: Box::new(appearance),
            }
        }
        Request::RenameTheme {
            theme_id,
            name,
            new_id,
        } => {
            let (theme_id, appearance, updates) =
                config.rename_theme(&theme_id, &name, new_id.as_deref())?;
            service.broadcast_config(&updates);
            broadcast_history(service, HistoryScope::Appearance);
            Response::ThemeSaved {
                theme_id,
                appearance: Box::new(appearance),
            }
        }
        Request::DeleteTheme { theme_id } => {
            let (appearance, updates) = config.delete_theme(&theme_id)?;
            service.broadcast_config(&updates);
            broadcast_history(service, HistoryScope::Appearance);
            Response::Themes {
                appearance: Box::new(appearance),
            }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to the editing module",
                other.wire_type()
            ))
            .into());
        }
    })
}

#[cfg(test)]
mod tests {
    use nail_api::editing::{EditKind, HistoryScope, ThemeEditTarget, ThemePatch};
    use nail_api::{ErrorCode, Event, LayoutNodeRef, LayoutPatch, Request, Response};

    use crate::service::Service;

    /// A service on temporary config / data directories (no watcher).
    fn service() -> (Service, tempfile::TempDir, tempfile::TempDir) {
        let t = crate::testing::TempService::quiet();
        (t.service, t.config, t.data)
    }

    fn history(response: Response) -> nail_api::editing::EditHistory {
        match response {
            Response::EditHistory { history } => history,
            other => panic!("expected EditHistory, got {other:?}"),
        }
    }

    fn error_id(response: &Response) -> (ErrorCode, Option<String>) {
        match response {
            Response::Error {
                code, localized, ..
            } => (*code, localized.as_ref().map(|l| l.id.clone())),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn divider(sizes: &[&str]) -> LayoutPatch {
        LayoutPatch::SplitSizes {
            node: LayoutNodeRef {
                window: "mail".into(),
                breakpoint: 0,
                path: vec![],
            },
            sizes: sizes.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[tokio::test]
    async fn layout_edits_undo_and_redo_with_events() {
        let (service, dir, _data) = service();
        let mut events = service.subscribe();
        let start = history(
            service
                .handle(Request::GetEditHistory {
                    scope: HistoryScope::Layout,
                })
                .await,
        );
        assert!(start.undo.is_empty(), "{:?}", start.undo);
        assert_eq!(start.revision, nail_api::editing::NO_REVISION);

        let answer = service
            .handle(Request::UpdateLayout {
                patches: vec![divider(&["200px", "1fr", "1fr"])],
                base_revision: Some(start.revision.clone()),
            })
            .await;
        assert_eq!(answer, Response::Done);
        let written = std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap();
        assert!(written.contains("200px 1fr 1fr"));
        let after = history(
            service
                .handle(Request::GetEditHistory {
                    scope: HistoryScope::Layout,
                })
                .await,
        );
        assert_eq!(after.undo.len(), 1);
        assert_eq!(after.undo[0].kind, EditKind::SplitSizes);
        assert_ne!(after.revision, start.revision);
        let mut saw_history = false;
        while let Ok(event) = events.try_recv() {
            saw_history |=
                matches!(event, Event::EditHistoryChanged { history } if history.undo.len() == 1);
        }
        assert!(saw_history, "UpdateLayout sends EditHistoryChanged");

        // A stale revision is refused.
        let stale = service
            .handle(Request::UpdateLayout {
                patches: vec![divider(&["300px", "1fr", "1fr"])],
                base_revision: Some(start.revision),
            })
            .await;
        assert_eq!(
            error_id(&stale),
            (ErrorCode::Conflict, Some("edit-issue-conflict".to_owned()))
        );

        let undone = history(
            service
                .handle(Request::UndoEdit {
                    scope: HistoryScope::Layout,
                    force: false,
                })
                .await,
        );
        assert!(undone.undo.is_empty() && undone.redo.len() == 1);
        assert!(
            !dir.path().join("layout.kdl").exists(),
            "the file did not exist before"
        );
        let redone = history(
            service
                .handle(Request::RedoEdit {
                    scope: HistoryScope::Layout,
                    force: false,
                })
                .await,
        );
        assert_eq!(redone.undo.len(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap(),
            written
        );

        // Changed on disk: undo refuses unless forced.
        std::fs::write(
            dir.path().join("layout.kdl"),
            "layout \"x\" extends=\"default\" {\n}\n",
        )
        .unwrap();
        let refused = service
            .handle(Request::UndoEdit {
                scope: HistoryScope::Layout,
                force: false,
            })
            .await;
        assert_eq!(error_id(&refused).0, ErrorCode::Conflict);
        let forced = history(
            service
                .handle(Request::UndoEdit {
                    scope: HistoryScope::Layout,
                    force: true,
                })
                .await,
        );
        assert!(forced.undo.is_empty(), "{:?}", forced.undo);
    }

    #[tokio::test]
    async fn theme_requests_reach_the_config_manager() {
        let (service, _dir, _data) = service();
        let unknown = service
            .handle(Request::GetThemeDetails {
                theme_id: "nope".into(),
            })
            .await;
        assert_eq!(error_id(&unknown).0, ErrorCode::NotFound);
        let builtin = service
            .handle(Request::DeleteTheme {
                theme_id: "frost".into(),
            })
            .await;
        assert_eq!(
            error_id(&builtin),
            (
                ErrorCode::InvalidRequest,
                Some("edit-issue-not-editable".to_owned())
            )
        );
        // No patches: nothing to write, still an answer.
        let edit = service
            .handle(Request::EditTheme {
                target: ThemeEditTarget::User,
                patches: vec![],
                base_revision: None,
            })
            .await;
        assert!(matches!(edit, Response::Themes { .. }), "{edit:?}");
    }

    /// WP185: the theme operations of WP184 through the core — duplicate,
    /// edit with the details' revision, a stale revision, undo.
    #[tokio::test]
    async fn theme_operations_write_files_and_undo() {
        let (service, dir, _data) = service();
        let mut events = service.subscribe();
        let saved = service
            .handle(Request::DuplicateTheme {
                theme_id: "frost".into(),
                name: "My Frost".into(),
            })
            .await;
        let Response::ThemeSaved { theme_id, .. } = saved else {
            panic!("expected ThemeSaved, got {saved:?}");
        };
        assert_eq!(theme_id, "my-frost");
        let file = dir.path().join("themes").join("my-frost.kdl");
        assert!(file.is_file());

        let details = service
            .handle(Request::GetThemeDetails {
                theme_id: theme_id.clone(),
            })
            .await;
        let Response::ThemeDetails { details } = details else {
            panic!("expected ThemeDetails, got {details:?}");
        };
        assert!(details.editable);
        let patch = ThemePatch::SetToken {
            category: "color".into(),
            name: "accent".into(),
            value: Some("#aa3366".into()),
            mode: None,
        };
        let edited = service
            .handle(Request::EditTheme {
                target: ThemeEditTarget::Theme {
                    theme_id: theme_id.clone(),
                },
                patches: vec![patch.clone()],
                base_revision: Some(details.revision.clone()),
            })
            .await;
        assert!(matches!(edited, Response::Themes { .. }), "{edited:?}");
        assert!(std::fs::read_to_string(&file).unwrap().contains("#aa3366"));

        // The old revision is stale now.
        let stale = service
            .handle(Request::EditTheme {
                target: ThemeEditTarget::Theme { theme_id },
                patches: vec![patch],
                base_revision: Some(details.revision),
            })
            .await;
        assert_eq!(error_id(&stale).0, ErrorCode::Conflict);

        let undone = history(
            service
                .handle(Request::UndoEdit {
                    scope: HistoryScope::Appearance,
                    force: false,
                })
                .await,
        );
        assert_eq!(undone.redo.len(), 1);
        assert!(!std::fs::read_to_string(&file).unwrap().contains("#aa3366"));
        let mut saw_history = false;
        while let Ok(event) = events.try_recv() {
            saw_history |= matches!(event, Event::EditHistoryChanged { .. });
        }
        assert!(saw_history);
    }
}
