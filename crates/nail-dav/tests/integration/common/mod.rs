//! Helpers shared by the DAV client tests.

use nail_testkit_net::MockDavServer;
use url::Url;

/// The URL of `path` on the mock server.
pub(crate) fn url(server: &MockDavServer, path: &str) -> Url {
    Url::parse(&server.url(path)).unwrap()
}

/// A calendar object with one event (`extra` lines before `SUMMARY`).
pub(crate) fn event(uid: &str, start: &str, end: &str, extra: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Test//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20261001T000000Z\r\nDTSTART:{start}\r\nDTEND:{end}\r\n{extra}SUMMARY:{uid}\r\n\
         END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}
