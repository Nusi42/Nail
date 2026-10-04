//! The layout editor's and toolbar customizer's write-back patches (M5d,
//! ADR 0131): move / swap / dock panes, add / remove widgets, hide / show,
//! presets, window reset, toolbar items between the bar and "More".
//!
//! **Slot of WP184.** The contract routes every M5d [`LayoutPatch`] here
//! from [`crate::write_back`] (`LayoutEditor::apply`); until WP184 lands each
//! one answers `unsupported` with `edit-issue-not-ready`. WP184 implements
//! them on the [`LayoutEditor`] (format and comment preserving, with golden
//! files), following the semantics documented on each patch in
//! `nail_api::customize::LayoutPatch` and `nail_api::editing`.

use nail_api::LayoutPatch;
use nail_api::editing::EditIssue;

use crate::l10n::msg;
use crate::write_back::{EditError, LayoutEditor};

/// Applies one M5d patch to the document being edited.
pub(crate) fn apply(_editor: &mut LayoutEditor, patch: &LayoutPatch) -> Result<(), EditError> {
    Err(EditError::Unsupported(
        msg(&EditIssue::NotReady.message_id()).arg("op", patch.kind().as_str()),
    ))
}

#[cfg(test)]
mod tests {
    use nail_api::editing::NewWidget;
    use nail_api::{ErrorCode, LayoutNodeRef};

    use crate::write_back::apply_layout_patches;

    #[test]
    fn m5d_patches_are_not_ready_in_the_contract() {
        let patch = nail_api::LayoutPatch::AddWidget {
            widget: NewWidget {
                kind: "mini-month".into(),
                id: None,
                label: None,
                options: serde_json::Map::new(),
            },
            to: nail_api::editing::LayoutPlacement::Into {
                parent: LayoutNodeRef {
                    window: "mail".into(),
                    breakpoint: 0,
                    path: vec![0],
                },
                index: 0,
            },
        };
        let err = apply_layout_patches(None, &[patch], "layout.kdl").unwrap_err();
        let api: nail_api::ApiError = err.into();
        assert_eq!(api.code, ErrorCode::Unsupported);
        assert_eq!(
            api.localized.as_deref().map(|l| l.id.as_str()),
            Some("edit-issue-not-ready")
        );
    }
}
