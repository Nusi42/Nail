//! Errors of the composer engine.

/// Why a message could not be built or a text could not be rendered.
#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    /// An attachment's blob is not in the blob store.
    #[error("attachment {filename:?}: blob {blob} is missing")]
    MissingBlob {
        /// File name of the attachment.
        filename: String,
        /// Its blob hash.
        blob: String,
    },
    /// The draft's identity has no usable sender address.
    #[error("the sending identity has no address")]
    NoSender,
    /// The mail stylesheet could not be applied.
    #[error("mail stylesheet: {0}")]
    Stylesheet(String),
    /// Writing the MIME structure failed.
    #[error("building the message: {0}")]
    Io(#[from] std::io::Error),
}
