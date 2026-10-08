//! The namespace-aware XML tree for WebDAV bodies (multistatus answers,
//! error bodies) and escaping for the request bodies Nail writes: one
//! definition in `nail_net::xml`, shared with the mock DAV server.

pub use nail_net::xml::{Element, MAX_DEPTH, MAX_ELEMENTS, XmlError, escape, parse};
