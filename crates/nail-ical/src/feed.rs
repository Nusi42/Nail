//! ICS subscriptions (`calendar-subscription` sources): `webcal:` URLs and
//! feed parsing.
//!
//! Feeds are read like any calendar, with two conventions of published
//! calendars applied: floating times are placed in the feed's
//! `X-WR-TIMEZONE` (Google, Outlook.com and most CMS exports write local
//! times with that header), and the feed's name, colour and refresh
//! interval (`REFRESH-INTERVAL` / `X-PUBLISHED-TTL`) are reported.

use nail_api::pim::PimTime;

use crate::content;
use crate::ical::{ParseOptions, ParsedCalendar, parse_calendar};

/// Refresh interval when a feed names none, minutes (as the source
/// settings default).
pub const DEFAULT_REFRESH_MINUTES: u32 = 360;

/// Shortest refresh interval a feed can ask for, minutes (feeds that ask
/// for every minute are polled every 15).
pub const MIN_REFRESH_MINUTES: u32 = 15;

/// Reads a feed's text.
pub fn parse_feed(text: &str) -> ParsedCalendar {
    parse_calendar(
        text,
        ParseOptions {
            floating_zone: None,
            use_calendar_zone: true,
        },
    )
}

/// Reads a feed's bytes (UTF-8, UTF-16 with BOM, else Windows-1252).
pub fn parse_feed_bytes(bytes: &[u8]) -> ParsedCalendar {
    parse_feed(&content::decode_bytes(bytes).0)
}

/// The refresh interval to use: the source's setting, else the feed's,
/// else [`DEFAULT_REFRESH_MINUTES`]; never below [`MIN_REFRESH_MINUTES`].
pub fn refresh_minutes(configured: Option<u32>, feed: &ParsedCalendar) -> u32 {
    configured
        .or(feed.refresh_minutes)
        .unwrap_or(DEFAULT_REFRESH_MINUTES)
        .max(MIN_REFRESH_MINUTES)
}

fn is_loopback(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]"
    )
}

/// The HTTP URL to fetch for a subscription URL: `webcal://` and
/// `webcals://` become `https://` (plain `http://` only for loopback test
/// servers); `https://` stays. `None` for anything else.
pub fn fetch_url(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if rest.is_empty() || rest.starts_with('/') || url.chars().any(char::is_whitespace) {
        return None;
    }
    let host = nail_api::pim::authority_host(rest);
    match scheme.to_ascii_lowercase().as_str() {
        "webcal" | "http" if is_loopback(host) => Some(format!("http://{rest}")),
        "https" | "webcals" | "webcal" => Some(format!("https://{rest}")),
        _ => None,
    }
}

/// The start of the first event, for a subscription preview.
pub fn first_start(feed: &ParsedCalendar) -> Option<PimTime> {
    feed.events
        .iter()
        .map(|e| e.fields.start.clone())
        .min_by(|a, b| a.date_part().cmp(b.date_part()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOWN_FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Example CMS//EN\r\nX-WR-CALNAME:Town events\r\nX-WR-TIMEZONE:America/Chicago\r\nREFRESH-INTERVAL;VALUE=DURATION:P1D\r\n\
BEGIN:VEVENT\r\nUID:1@cms\r\nDTSTART:20261031T180000\r\nDTEND:20261031T210000\r\nSUMMARY:Halloween parade\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:2@cms\r\nDTSTART;VALUE=DATE:20261126\r\nSUMMARY:Thanksgiving\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn feeds_use_their_zone_and_interval() {
        let feed = parse_feed(TOWN_FEED);
        assert_eq!(feed.name.as_deref(), Some("Town events"));
        assert_eq!(feed.refresh_minutes, Some(1440));
        assert_eq!(
            feed.events[0].fields.start,
            PimTime::zoned("2026-10-31T18:00:00", "America/Chicago")
        );
        assert_eq!(feed.events[1].fields.start, PimTime::date("2026-11-26"));
        assert_eq!(refresh_minutes(None, &feed), 1440);
        assert_eq!(refresh_minutes(Some(5), &feed), MIN_REFRESH_MINUTES);
        assert_eq!(
            refresh_minutes(None, &ParsedCalendar::default()),
            DEFAULT_REFRESH_MINUTES
        );
        assert_eq!(
            first_start(&feed),
            Some(PimTime::zoned("2026-10-31T18:00:00", "America/Chicago"))
        );
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(TOWN_FEED.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(parse_feed_bytes(&utf16).events.len(), 2);
    }

    #[test]
    fn subscription_urls() {
        assert_eq!(
            fetch_url("webcal://calendar.example.org/h.ics").as_deref(),
            Some("https://calendar.example.org/h.ics")
        );
        assert_eq!(
            fetch_url("WEBCALS://x.example/a").as_deref(),
            Some("https://x.example/a")
        );
        assert_eq!(
            fetch_url("https://x.example/a").as_deref(),
            Some("https://x.example/a")
        );
        assert_eq!(
            fetch_url("webcal://127.0.0.1:8080/a.ics").as_deref(),
            Some("http://127.0.0.1:8080/a.ics")
        );
        assert_eq!(
            fetch_url("http://localhost/a").as_deref(),
            Some("http://localhost/a")
        );
        assert_eq!(fetch_url("http://x.example/a"), None);
        assert_eq!(fetch_url("ftp://x.example/a"), None);
        assert_eq!(fetch_url("webcal:///a"), None);
        assert_eq!(fetch_url("webcal://a b/"), None);
    }
}
