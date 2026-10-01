//! The few Windows time zones the mock server understands (for
//! `TimeZoneContext` writes and `StartTimeZone` answers), with EU / US
//! daylight-saving rules.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dst {
    None,
    Eu,
    Us,
}

/// A zone: Windows id, display name, standard offset (local − UTC,
/// minutes) and daylight-saving rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    /// The Windows id.
    pub id: &'static str,
    name: &'static str,
    standard: i64,
    dst: Dst,
}

const ZONES: [Zone; 7] = [
    Zone {
        id: "UTC",
        name: "(UTC) Coordinated Universal Time",
        standard: 0,
        dst: Dst::None,
    },
    Zone {
        id: "W. Europe Standard Time",
        name: "(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna",
        standard: 60,
        dst: Dst::Eu,
    },
    Zone {
        id: "Romance Standard Time",
        name: "(UTC+01:00) Brussels, Copenhagen, Madrid, Paris",
        standard: 60,
        dst: Dst::Eu,
    },
    Zone {
        id: "GMT Standard Time",
        name: "(UTC+00:00) Dublin, Edinburgh, Lisbon, London",
        standard: 0,
        dst: Dst::Eu,
    },
    Zone {
        id: "Eastern Standard Time",
        name: "(UTC-05:00) Eastern Time (US & Canada)",
        standard: -300,
        dst: Dst::Us,
    },
    Zone {
        id: "Pacific Standard Time",
        name: "(UTC-08:00) Pacific Time (US & Canada)",
        standard: -480,
        dst: Dst::Us,
    },
    Zone {
        id: "Tokyo Standard Time",
        name: "(UTC+09:00) Osaka, Sapporo, Tokyo",
        standard: 540,
        dst: Dst::None,
    },
];

/// The zone of a Windows id.
pub fn zone(id: &str) -> Option<Zone> {
    ZONES
        .iter()
        .copied()
        .find(|z| z.id.eq_ignore_ascii_case(id))
}

fn nth_weekday(year: i32, month: u32, weekday: Weekday, n: u8) -> NaiveDate {
    NaiveDate::from_weekday_of_month_opt(year, month, weekday, n.min(4)).unwrap_or_default()
}

fn last_sunday(year: i32, month: u32) -> NaiveDate {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .unwrap_or_default();
    let mut day = next - Duration::days(1);
    while day.weekday() != Weekday::Sun {
        day -= Duration::days(1);
    }
    day
}

impl Zone {
    /// Local − UTC in minutes at a UTC time.
    pub fn offset_at(&self, utc: NaiveDateTime) -> i64 {
        let year = utc.year();
        let daylight = match self.dst {
            Dst::None => false,
            Dst::Eu => {
                let start = last_sunday(year, 3)
                    .and_hms_opt(1, 0, 0)
                    .unwrap_or_default();
                let end = last_sunday(year, 10)
                    .and_hms_opt(1, 0, 0)
                    .unwrap_or_default();
                utc >= start && utc < end
            }
            Dst::Us => {
                let start = nth_weekday(year, 3, Weekday::Sun, 2)
                    .and_hms_opt(2, 0, 0)
                    .unwrap_or_default()
                    - Duration::minutes(self.standard);
                let end = nth_weekday(year, 11, Weekday::Sun, 1)
                    .and_hms_opt(2, 0, 0)
                    .unwrap_or_default()
                    - Duration::minutes(self.standard + 60);
                utc >= start && utc < end
            }
        };
        self.standard + if daylight { 60 } else { 0 }
    }

    /// The UTC time of a wall time.
    pub fn to_utc(self, local: NaiveDateTime) -> NaiveDateTime {
        let guess = local - Duration::minutes(self.standard);
        local - Duration::minutes(self.offset_at(guess))
    }

    /// The `StartTimeZone` element.
    pub fn element(&self, tag: &str) -> String {
        let bias = |minutes: i64| {
            let sign = if minutes > 0 { "-" } else { "" };
            let m = minutes.abs();
            format!("{sign}PT{}H{}M", m / 60, m % 60)
        };
        let daylight = if self.dst == Dst::None {
            String::new()
        } else {
            format!(
                r#"<t:Period Bias="{}" Name="Daylight" Id="trule:Microsoft/Registry/{}/Daylight"/>"#,
                bias(self.standard + 60),
                self.id
            )
        };
        format!(
            r#"<t:{tag} Id="{}" Name="{}"><t:Periods><t:Period Bias="{}" Name="Standard" Id="trule:Microsoft/Registry/{}/Standard"/>{daylight}</t:Periods></t:{tag}>"#,
            self.id,
            crate::xml::escape(self.name),
            bias(self.standard),
            self.id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    #[test]
    fn offsets_follow_daylight_saving() {
        let berlin = zone("W. Europe Standard Time").unwrap();
        assert_eq!(berlin.offset_at(at("2026-01-15T12:00:00")), 60);
        assert_eq!(berlin.offset_at(at("2026-07-15T12:00:00")), 120);
        assert_eq!(berlin.offset_at(at("2026-10-25T00:59:00")), 120);
        assert_eq!(berlin.offset_at(at("2026-10-25T01:00:00")), 60);
        assert_eq!(
            berlin.to_utc(at("2026-10-05T09:00:00")),
            at("2026-10-05T07:00:00")
        );
        assert_eq!(
            berlin.to_utc(at("2026-12-05T09:00:00")),
            at("2026-12-05T08:00:00")
        );
        let new_york = zone("eastern standard time").unwrap();
        assert_eq!(new_york.offset_at(at("2026-07-01T12:00:00")), -240);
        assert_eq!(
            new_york.to_utc(at("2026-01-05T09:00:00")),
            at("2026-01-05T14:00:00")
        );
        assert!(zone("Mars Standard Time").is_none());
        let xml = berlin.element("StartTimeZone");
        assert!(xml.contains(r#"Bias="-PT1H0M" Name="Standard""#));
        assert!(xml.contains(r#"Bias="-PT2H0M" Name="Daylight""#));
        assert!(
            zone("UTC")
                .unwrap()
                .element("StartTimeZone")
                .contains(r#"Bias="PT0H0M""#)
        );
    }
}
