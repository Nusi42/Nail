//! IANA → Windows time zone ids for EWS writes (ADR 0078).
//!
//! EWS expands a series in its `StartTimeZone`, which must be a Windows
//! id: an event written in UTC moves by an hour at every daylight-saving
//! change. [`windows_zone`] picks the Windows id of an IANA zone: CLDR's
//! `windowsZones` table (territory `001`, plus common other cities), else a
//! Windows zone whose IANA zone keeps the same UTC offsets through the
//! event's year and the next (so the series expands the same).

use std::str::FromStr;

use chrono::{Duration, NaiveDate, Offset, TimeZone};
use chrono_tz::Tz;

/// CLDR `windowsZones` (territory `001`): Windows id → IANA zone.
pub const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("Dateline Standard Time", "Etc/GMT+12"),
    ("UTC-11", "Etc/GMT+11"),
    ("Aleutian Standard Time", "America/Adak"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Marquesas Standard Time", "Pacific/Marquesas"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("UTC-09", "Etc/GMT+9"),
    ("Pacific Standard Time (Mexico)", "America/Tijuana"),
    ("UTC-08", "Etc/GMT+8"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Mountain Standard Time (Mexico)", "America/Mazatlan"),
    ("Mountain Standard Time", "America/Denver"),
    ("Yukon Standard Time", "America/Whitehorse"),
    ("Central America Standard Time", "America/Guatemala"),
    ("Central Standard Time", "America/Chicago"),
    ("Easter Island Standard Time", "Pacific/Easter"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("Canada Central Standard Time", "America/Regina"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Eastern Standard Time (Mexico)", "America/Cancun"),
    ("Eastern Standard Time", "America/New_York"),
    ("Haiti Standard Time", "America/Port-au-Prince"),
    ("Cuba Standard Time", "America/Havana"),
    ("US Eastern Standard Time", "America/Indiana/Indianapolis"),
    ("Turks And Caicos Standard Time", "America/Grand_Turk"),
    ("Paraguay Standard Time", "America/Asuncion"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Venezuela Standard Time", "America/Caracas"),
    ("Central Brazilian Standard Time", "America/Cuiaba"),
    ("SA Western Standard Time", "America/La_Paz"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("Tocantins Standard Time", "America/Araguaina"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("SA Eastern Standard Time", "America/Cayenne"),
    ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
    ("Greenland Standard Time", "America/Nuuk"),
    ("Montevideo Standard Time", "America/Montevideo"),
    ("Magallanes Standard Time", "America/Punta_Arenas"),
    ("Saint Pierre Standard Time", "America/Miquelon"),
    ("Bahia Standard Time", "America/Bahia"),
    ("UTC-02", "Etc/GMT+2"),
    ("Azores Standard Time", "Atlantic/Azores"),
    ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
    ("UTC", "Etc/UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("Sao Tome Standard Time", "Africa/Sao_Tome"),
    ("Morocco Standard Time", "Africa/Casablanca"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Jordan Standard Time", "Asia/Amman"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Middle East Standard Time", "Asia/Beirut"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("Syria Standard Time", "Asia/Damascus"),
    ("West Bank Standard Time", "Asia/Hebron"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("South Sudan Standard Time", "Africa/Juba"),
    ("Kaliningrad Standard Time", "Europe/Kaliningrad"),
    ("Sudan Standard Time", "Africa/Khartoum"),
    ("Libya Standard Time", "Africa/Tripoli"),
    ("Namibia Standard Time", "Africa/Windhoek"),
    ("Arabic Standard Time", "Asia/Baghdad"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("Belarus Standard Time", "Europe/Minsk"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("E. Africa Standard Time", "Africa/Nairobi"),
    ("Volgograd Standard Time", "Europe/Volgograd"),
    ("Iran Standard Time", "Asia/Tehran"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Astrakhan Standard Time", "Europe/Astrakhan"),
    ("Azerbaijan Standard Time", "Asia/Baku"),
    ("Russia Time Zone 3", "Europe/Samara"),
    ("Mauritius Standard Time", "Indian/Mauritius"),
    ("Saratov Standard Time", "Europe/Saratov"),
    ("Georgian Standard Time", "Asia/Tbilisi"),
    ("Caucasus Standard Time", "Asia/Yerevan"),
    ("Afghanistan Standard Time", "Asia/Kabul"),
    ("West Asia Standard Time", "Asia/Tashkent"),
    ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("Qyzylorda Standard Time", "Asia/Qyzylorda"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Sri Lanka Standard Time", "Asia/Colombo"),
    ("Nepal Standard Time", "Asia/Kathmandu"),
    ("Central Asia Standard Time", "Asia/Bishkek"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("Omsk Standard Time", "Asia/Omsk"),
    ("Myanmar Standard Time", "Asia/Yangon"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("Altai Standard Time", "Asia/Barnaul"),
    ("W. Mongolia Standard Time", "Asia/Hovd"),
    ("North Asia Standard Time", "Asia/Krasnoyarsk"),
    ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
    ("Tomsk Standard Time", "Asia/Tomsk"),
    ("China Standard Time", "Asia/Shanghai"),
    ("North Asia East Standard Time", "Asia/Irkutsk"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Ulaanbaatar Standard Time", "Asia/Ulaanbaatar"),
    ("Aus Central W. Standard Time", "Australia/Eucla"),
    ("Transbaikal Standard Time", "Asia/Chita"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("North Korea Standard Time", "Asia/Pyongyang"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("Yakutsk Standard Time", "Asia/Yakutsk"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("West Pacific Standard Time", "Pacific/Port_Moresby"),
    ("Tasmania Standard Time", "Australia/Hobart"),
    ("Vladivostok Standard Time", "Asia/Vladivostok"),
    ("Lord Howe Standard Time", "Australia/Lord_Howe"),
    ("Bougainville Standard Time", "Pacific/Bougainville"),
    ("Russia Time Zone 10", "Asia/Srednekolymsk"),
    ("Magadan Standard Time", "Asia/Magadan"),
    ("Norfolk Standard Time", "Pacific/Norfolk"),
    ("Sakhalin Standard Time", "Asia/Sakhalin"),
    ("Central Pacific Standard Time", "Pacific/Guadalcanal"),
    ("Russia Time Zone 11", "Asia/Kamchatka"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("UTC+12", "Etc/GMT-12"),
    ("Fiji Standard Time", "Pacific/Fiji"),
    ("Chatham Islands Standard Time", "Pacific/Chatham"),
    ("UTC+13", "Etc/GMT-13"),
    ("Tonga Standard Time", "Pacific/Tongatapu"),
    ("Samoa Standard Time", "Pacific/Apia"),
    ("Line Islands Standard Time", "Pacific/Kiritimati"),
];

/// Other IANA zones of CLDR's table (and their old names) → Windows id.
const OTHER_CITIES: &[(&str, &str)] = &[
    ("Europe/Amsterdam", "W. Europe Standard Time"),
    ("Europe/Andorra", "W. Europe Standard Time"),
    ("Europe/Luxembourg", "W. Europe Standard Time"),
    ("Europe/Malta", "W. Europe Standard Time"),
    ("Europe/Monaco", "W. Europe Standard Time"),
    ("Europe/Oslo", "W. Europe Standard Time"),
    ("Europe/Rome", "W. Europe Standard Time"),
    ("Europe/San_Marino", "W. Europe Standard Time"),
    ("Europe/Stockholm", "W. Europe Standard Time"),
    ("Europe/Vaduz", "W. Europe Standard Time"),
    ("Europe/Vatican", "W. Europe Standard Time"),
    ("Europe/Vienna", "W. Europe Standard Time"),
    ("Europe/Zurich", "W. Europe Standard Time"),
    ("Europe/Busingen", "W. Europe Standard Time"),
    ("Europe/Gibraltar", "W. Europe Standard Time"),
    ("Europe/Brussels", "Romance Standard Time"),
    ("Europe/Copenhagen", "Romance Standard Time"),
    ("Europe/Madrid", "Romance Standard Time"),
    ("Europe/Belgrade", "Central Europe Standard Time"),
    ("Europe/Bratislava", "Central Europe Standard Time"),
    ("Europe/Ljubljana", "Central Europe Standard Time"),
    ("Europe/Podgorica", "Central Europe Standard Time"),
    ("Europe/Prague", "Central Europe Standard Time"),
    ("Europe/Tirane", "Central Europe Standard Time"),
    ("Europe/Sarajevo", "Central European Standard Time"),
    ("Europe/Skopje", "Central European Standard Time"),
    ("Europe/Zagreb", "Central European Standard Time"),
    ("Europe/Dublin", "GMT Standard Time"),
    ("Europe/Lisbon", "GMT Standard Time"),
    ("Europe/Guernsey", "GMT Standard Time"),
    ("Europe/Isle_of_Man", "GMT Standard Time"),
    ("Europe/Jersey", "GMT Standard Time"),
    ("Atlantic/Canary", "GMT Standard Time"),
    ("Atlantic/Faroe", "GMT Standard Time"),
    ("Atlantic/Madeira", "GMT Standard Time"),
    ("Europe/Helsinki", "FLE Standard Time"),
    ("Europe/Kiev", "FLE Standard Time"),
    ("Europe/Mariehamn", "FLE Standard Time"),
    ("Europe/Riga", "FLE Standard Time"),
    ("Europe/Sofia", "FLE Standard Time"),
    ("Europe/Tallinn", "FLE Standard Time"),
    ("Europe/Vilnius", "FLE Standard Time"),
    ("Europe/Athens", "GTB Standard Time"),
    ("Asia/Nicosia", "GTB Standard Time"),
    ("America/Detroit", "Eastern Standard Time"),
    ("America/Toronto", "Eastern Standard Time"),
    ("America/Nassau", "Eastern Standard Time"),
    ("America/Indianapolis", "US Eastern Standard Time"),
    ("America/Vancouver", "Pacific Standard Time"),
    ("America/Edmonton", "Mountain Standard Time"),
    ("America/Boise", "Mountain Standard Time"),
    ("America/Winnipeg", "Central Standard Time"),
    ("America/Buenos_Aires", "Argentina Standard Time"),
    ("America/Godthab", "Greenland Standard Time"),
    ("Asia/Calcutta", "India Standard Time"),
    ("Asia/Katmandu", "Nepal Standard Time"),
    ("Asia/Rangoon", "Myanmar Standard Time"),
    ("Asia/Hong_Kong", "China Standard Time"),
    ("Asia/Macau", "China Standard Time"),
    ("Asia/Jakarta", "SE Asia Standard Time"),
    ("Asia/Ho_Chi_Minh", "SE Asia Standard Time"),
    ("Asia/Saigon", "SE Asia Standard Time"),
    ("Asia/Kuala_Lumpur", "Singapore Standard Time"),
    ("Asia/Manila", "Singapore Standard Time"),
    ("Asia/Almaty", "Central Asia Standard Time"),
    ("Asia/Kuwait", "Arab Standard Time"),
    ("Asia/Qatar", "Arab Standard Time"),
    ("Asia/Muscat", "Arabian Standard Time"),
    ("Australia/Melbourne", "AUS Eastern Standard Time"),
    ("Australia/Canberra", "AUS Eastern Standard Time"),
    ("Pacific/Guam", "West Pacific Standard Time"),
    ("Africa/Algiers", "W. Central Africa Standard Time"),
    ("Africa/Tunis", "W. Central Africa Standard Time"),
    ("Africa/Addis_Ababa", "E. Africa Standard Time"),
    ("Africa/Harare", "South Africa Standard Time"),
    ("Africa/Maputo", "South Africa Standard Time"),
    ("Africa/Abidjan", "Greenwich Standard Time"),
    ("Africa/Accra", "Greenwich Standard Time"),
    ("Africa/Dakar", "Greenwich Standard Time"),
];

/// The Windows id of an IANA zone for an event in `year` (see the module
/// docs); `None` when no Windows zone keeps the same offsets.
pub fn windows_zone(tz: Tz, year: i32) -> Option<&'static str> {
    let name = tz.name();
    if let Some((windows, _)) = WINDOWS_ZONES.iter().find(|(_, iana)| *iana == name) {
        return Some(windows);
    }
    if let Some((_, windows)) = OTHER_CITIES.iter().find(|(iana, _)| *iana == name) {
        return Some(windows);
    }
    WINDOWS_ZONES.iter().find_map(|(windows, iana)| {
        let candidate = Tz::from_str(iana).ok()?;
        same_offsets(tz, candidate, year).then_some(*windows)
    })
}

/// Whether two zones have the same UTC offset every hour of `year` and
/// the next (so a series expands the same in both).
fn same_offsets(a: Tz, b: Tz, year: i32) -> bool {
    let Some(start) = NaiveDate::from_ymd_opt(year, 1, 1).and_then(|d| d.and_hms_opt(0, 0, 0))
    else {
        return false;
    };
    let hours = 2 * 366 * 24;
    // Days first (cheap rejection), then every hour around changes.
    let offset = |tz: Tz, at: chrono::NaiveDateTime| {
        tz.offset_from_utc_datetime(&at).fix().local_minus_utc()
    };
    if (0..(2 * 366)).any(|d| {
        let at = start + Duration::days(d);
        offset(a, at) != offset(b, at)
    }) {
        return false;
    }
    (0..hours).all(|h| {
        let at = start + Duration::hours(h);
        offset(a, at) == offset(b, at)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_zone_is_known() {
        for &(windows, iana) in WINDOWS_ZONES.iter().chain(
            OTHER_CITIES
                .iter()
                .map(|(i, w)| (*w, *i))
                .collect::<Vec<_>>()
                .iter(),
        ) {
            assert!(Tz::from_str(iana).is_ok(), "{iana} ({windows})");
            assert!(
                WINDOWS_ZONES.iter().any(|(w, _)| *w == windows),
                "{windows} is not a primary id"
            );
        }
    }

    #[test]
    fn iana_zones_map_to_windows_ids() {
        assert_eq!(
            windows_zone(Tz::Europe__Berlin, 2026),
            Some("W. Europe Standard Time")
        );
        assert_eq!(
            windows_zone(Tz::Europe__Vienna, 2026),
            Some("W. Europe Standard Time")
        );
        assert_eq!(
            windows_zone(Tz::America__New_York, 2026),
            Some("Eastern Standard Time")
        );
        assert_eq!(
            windows_zone(Tz::Asia__Kolkata, 2026),
            Some("India Standard Time")
        );
        // Not in the tables: a zone with the same offsets.
        let zone = windows_zone(Tz::America__Louisville, 2026).unwrap();
        let iana = WINDOWS_ZONES.iter().find(|(w, _)| *w == zone).unwrap().1;
        assert!(same_offsets(
            Tz::America__Louisville,
            Tz::from_str(iana).unwrap(),
            2026
        ));
        assert!(!same_offsets(Tz::Europe__London, Tz::Europe__Berlin, 2026));
    }
}
