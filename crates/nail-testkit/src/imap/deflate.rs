//! A byte stream compressed with raw DEFLATE in both directions
//! (`COMPRESS=DEFLATE`, RFC 4978): the one definition in `nail_net`,
//! shared with the IMAP client.

pub(crate) use nail_net::deflate::DeflateIo;
