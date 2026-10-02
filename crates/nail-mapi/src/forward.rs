//! What happens to a decoded `MAPISendMail(W)` call: relative attachment
//! paths are resolved against the calling program's working directory, the
//! checks that need no file system run (`MapiMessage::check`), the files
//! are copied to a private folder ([`crate::staging`]), the request goes to
//! the app over a [`Transport`] (the named pipe on Windows), and the copies
//! are removed once the app has answered.

use std::path::Path;

use nail_api::mapi::{
    MAPI_PROTOCOL_VERSION, MapiMessage, MapiPipeRequest, MapiPipeResponse, MapiResult,
};

use crate::staging::stage_attachments;

/// How a request reaches the app.
pub trait Transport {
    /// Sends one request and waits for the answer; `Err` is what to return
    /// to the program when the app could not be reached.
    fn send(&self, request: &MapiPipeRequest) -> Result<MapiPipeResponse, MapiResult>;
}

/// Forwards one message and returns what `MAPISendMail` should return.
pub fn forward(
    mut message: MapiMessage,
    transport: &dyn Transport,
    staging_root: &Path,
) -> MapiResult {
    for attachment in &mut message.attachments {
        let path = Path::new(&attachment.path);
        if !attachment.path.trim().is_empty() && !path.is_absolute() {
            match std::path::absolute(path)
                .ok()
                .and_then(|p| p.to_str().map(str::to_owned))
            {
                Some(absolute) => attachment.path = absolute,
                None => return MapiResult::AttachmentNotFound,
            }
        }
    }
    if let Err(result) = message.check() {
        return result;
    }
    let staged = match stage_attachments(&mut message, staging_root) {
        Ok(staged) => staged,
        Err(result) => return result,
    };
    let request = MapiPipeRequest {
        version: MAPI_PROTOCOL_VERSION,
        message,
    };
    let result = match transport.send(&request) {
        Ok(response) => response.result,
        Err(result) => result,
    };
    // The app has read the copies (or failed); remove them now.
    drop(staged);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::mapi::{MapiAttachment, MapiRecipient, MapiRecipientClass, MapiUi};
    use pretty_assertions::assert_eq;
    use std::cell::RefCell;
    use std::fs;

    /// Records the request and the attachments' content as the app would
    /// read them, then answers `result`.
    struct Recorder {
        result: Result<MapiResult, MapiResult>,
        seen: RefCell<Vec<(MapiPipeRequest, Vec<Vec<u8>>)>>,
    }

    impl Transport for Recorder {
        fn send(&self, request: &MapiPipeRequest) -> Result<MapiPipeResponse, MapiResult> {
            let files = request
                .message
                .attachments
                .iter()
                .map(|a| fs::read(&a.path).unwrap())
                .collect();
            self.seen.borrow_mut().push((request.clone(), files));
            self.result.map(|result| MapiPipeResponse { result })
        }
    }

    fn recorder(result: Result<MapiResult, MapiResult>) -> Recorder {
        Recorder {
            result,
            seen: RefCell::new(Vec::new()),
        }
    }

    fn message(attachments: Vec<MapiAttachment>) -> MapiMessage {
        MapiMessage {
            subject: Some("Hi".into()),
            body: None,
            recipients: vec![MapiRecipient {
                class: MapiRecipientClass::To,
                name: None,
                address: Some("SMTP:ada@x.example".into()),
            }],
            attachments,
            ui: MapiUi::None,
            program: Some("EXPLORER.EXE".into()),
        }
    }

    #[test]
    fn forwards_copies_and_cleans_up() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let file = src.path().join("photo.jpg");
        fs::write(&file, b"jpeg").unwrap();
        let transport = recorder(Ok(MapiResult::Success));
        let result = forward(
            message(vec![MapiAttachment {
                path: file.to_str().unwrap().into(),
                file_name: None,
            }]),
            &transport,
            root.path(),
        );
        assert_eq!(result, MapiResult::Success);
        let seen = transport.seen.borrow();
        let (request, files) = &seen[0];
        assert_eq!(request.version, MAPI_PROTOCOL_VERSION);
        assert_eq!(request.message.subject.as_deref(), Some("Hi"));
        let sent = &request.message.attachments[0];
        assert_ne!(Path::new(&sent.path), file);
        assert!(Path::new(&sent.path).starts_with(root.path()));
        assert_eq!(sent.file_name.as_deref(), Some("photo.jpg"));
        assert_eq!(files[0], b"jpeg");
        // Copies are gone after the answer; the caller's file is untouched.
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        assert!(file.exists());
    }

    #[test]
    fn app_answers_and_transport_failures_are_returned() {
        let root = tempfile::tempdir().unwrap();
        let transport = recorder(Ok(MapiResult::LoginFailure));
        assert_eq!(
            forward(message(Vec::new()), &transport, root.path()),
            MapiResult::LoginFailure
        );
        let transport = recorder(Err(MapiResult::Failure));
        assert_eq!(
            forward(message(Vec::new()), &transport, root.path()),
            MapiResult::Failure
        );
    }

    #[test]
    fn checks_run_before_anything_is_sent() {
        let root = tempfile::tempdir().unwrap();
        let transport = recorder(Ok(MapiResult::Success));
        let mut bad = message(Vec::new());
        bad.recipients[0].address = Some("not an address".into());
        assert_eq!(
            forward(bad, &transport, root.path()),
            MapiResult::InvalidRecips
        );
        let missing = message(vec![MapiAttachment {
            path: root.path().join("nope.txt").to_str().unwrap().into(),
            file_name: None,
        }]);
        assert_eq!(
            forward(missing, &transport, root.path()),
            MapiResult::AttachmentNotFound
        );
        let empty = message(vec![MapiAttachment {
            path: " ".into(),
            file_name: None,
        }]);
        assert_eq!(
            forward(empty, &transport, root.path()),
            MapiResult::AttachmentNotFound
        );
        assert!(transport.seen.borrow().is_empty());
    }

    #[test]
    fn relative_paths_resolve_against_the_working_directory() {
        let root = tempfile::tempdir().unwrap();
        // Cargo runs unit tests in the crate directory.
        let transport = recorder(Ok(MapiResult::Success));
        let result = forward(
            message(vec![MapiAttachment {
                path: "Cargo.toml".into(),
                file_name: None,
            }]),
            &transport,
            root.path(),
        );
        assert_eq!(result, MapiResult::Success);
        let seen = transport.seen.borrow();
        assert_eq!(
            seen[0].0.message.attachments[0].file_name.as_deref(),
            Some("Cargo.toml")
        );
        assert!(String::from_utf8_lossy(&seen[0].1[0]).contains("nail-mapi"));
    }
}
