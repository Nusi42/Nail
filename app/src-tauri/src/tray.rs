//! The tray icon (M4b WP113, ADR 0087): Nail keeps running there when the
//! window is closed (close to tray) or when it started hidden with
//! Windows. Its menu — Open Nail, New message, Check mail, Quit — is in
//! the core's language (`tray-*` in core-desktop.ftl). A left click opens
//! the window.

use nail_api::LocalizedMessage;
use nail_api::launch::{LaunchRequest, MailtoLink};

/// What a tray menu entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Show and focus the main window.
    Open,
    /// Open an empty composer.
    NewMessage,
    /// Synchronize every account now.
    CheckMail,
    /// End Nail (installs a downloaded update first).
    Quit,
}

impl TrayAction {
    /// The entries in menu order (a separator before Quit).
    pub const ALL: [TrayAction; 4] = [Self::Open, Self::NewMessage, Self::CheckMail, Self::Quit];

    /// The menu item id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Open => "tray-open",
            Self::NewMessage => "tray-new-message",
            Self::CheckMail => "tray-check-mail",
            Self::Quit => "tray-quit",
        }
    }

    /// The action of a menu item id.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The Fluent message of its label (the id doubles as the message id).
    pub fn label(self) -> LocalizedMessage {
        LocalizedMessage::new(self.id())
    }

    /// The launch request it pushes, if any.
    pub fn launch_request(self) -> Option<LaunchRequest> {
        match self {
            Self::NewMessage => Some(LaunchRequest::Compose {
                mailto: MailtoLink::default(),
            }),
            _ => None,
        }
    }

    /// Whether it brings the window up.
    pub fn shows_window(self) -> bool {
        matches!(self, Self::Open | Self::NewMessage)
    }
}

/// The tooltip's message.
pub fn tooltip() -> LocalizedMessage {
    LocalizedMessage::new("tray-tooltip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn ids_round_trip() {
        for action in TrayAction::ALL {
            assert_eq!(TrayAction::from_id(action.id()), Some(action));
        }
        assert_eq!(TrayAction::from_id("other"), None);
        assert!(TrayAction::NewMessage.shows_window());
        assert!(!TrayAction::CheckMail.shows_window());
        assert!(matches!(
            TrayAction::NewMessage.launch_request(),
            Some(LaunchRequest::Compose { mailto }) if mailto == MailtoLink::default()
        ));
        assert_eq!(TrayAction::Quit.launch_request(), None);
    }

    /// Every label and the tooltip exist in English (no raw ids in the
    /// tray); the German catalog may be empty (per-key English fallback).
    #[test]
    fn labels_are_localized() {
        for locale in ["en", "de"] {
            let data = tempfile::tempdir().unwrap();
            let config = tempfile::tempdir().unwrap();
            let args = crate::AppArgs {
                locale: Some(locale.into()),
                ..crate::AppArgs::in_dirs(data.path(), config.path(), Some("demo"))
            };
            let service = crate::open_service(&args).unwrap();
            for message in TrayAction::ALL
                .iter()
                .map(|a| a.label())
                .chain(std::iter::once(tooltip()))
            {
                let text = service.render_localized(&message);
                assert!(
                    !text.is_empty() && text != message.id,
                    "{}: {text}",
                    message.id
                );
            }
            assert_eq!(
                service.render_localized(&TrayAction::Quit.label()),
                "Quit Nail"
            );
        }
    }
}
