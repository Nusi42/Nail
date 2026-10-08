//! The `ATTENDEE` parameter keywords (RFC 5545 §3.2) in one place: reading
//! `ROLE` / `PARTSTAT` / `CUTYPE` and writing them back, shared by the
//! calendar reader and writer here and by `nail-itip`.

use nail_api::pim::calendar::{Attendee, AttendeeKind, AttendeeRole, ParticipationStatus};

use crate::content::Property;

/// The role of a `ROLE` keyword (upper case; unknown = required).
#[must_use]
pub fn role_of(keyword: Option<&str>) -> AttendeeRole {
    match keyword {
        Some("CHAIR") => AttendeeRole::Chair,
        Some("OPT-PARTICIPANT") => AttendeeRole::Optional,
        Some("NON-PARTICIPANT") => AttendeeRole::NonParticipant,
        _ => AttendeeRole::Required,
    }
}

/// The `ROLE` keyword of a role.
#[must_use]
pub const fn role_keyword(role: AttendeeRole) -> &'static str {
    match role {
        AttendeeRole::Chair => "CHAIR",
        AttendeeRole::Required => "REQ-PARTICIPANT",
        AttendeeRole::Optional => "OPT-PARTICIPANT",
        AttendeeRole::NonParticipant => "NON-PARTICIPANT",
    }
}

/// The status of a `PARTSTAT` keyword (upper case; unknown = needs action).
#[must_use]
pub fn status_of(keyword: Option<&str>) -> ParticipationStatus {
    match keyword {
        Some("ACCEPTED") => ParticipationStatus::Accepted,
        Some("DECLINED") => ParticipationStatus::Declined,
        Some("TENTATIVE") => ParticipationStatus::Tentative,
        Some("DELEGATED") => ParticipationStatus::Delegated,
        _ => ParticipationStatus::NeedsAction,
    }
}

/// The `PARTSTAT` keyword of a status.
#[must_use]
pub const fn partstat(status: ParticipationStatus) -> &'static str {
    match status {
        ParticipationStatus::NeedsAction => "NEEDS-ACTION",
        ParticipationStatus::Accepted => "ACCEPTED",
        ParticipationStatus::Declined => "DECLINED",
        ParticipationStatus::Tentative => "TENTATIVE",
        ParticipationStatus::Delegated => "DELEGATED",
    }
}

/// The kind of a `CUTYPE` keyword (upper case; unknown = individual).
#[must_use]
pub fn kind_of(keyword: Option<&str>) -> AttendeeKind {
    match keyword {
        Some("GROUP") => AttendeeKind::Group,
        Some("RESOURCE") => AttendeeKind::Resource,
        Some("ROOM") => AttendeeKind::Room,
        Some("UNKNOWN") => AttendeeKind::Unknown,
        _ => AttendeeKind::Individual,
    }
}

/// The `CUTYPE` keyword of a kind (`None` for the default, individual).
#[must_use]
pub const fn cutype(kind: AttendeeKind) -> Option<&'static str> {
    match kind {
        AttendeeKind::Individual => None,
        AttendeeKind::Group => Some("GROUP"),
        AttendeeKind::Resource => Some("RESOURCE"),
        AttendeeKind::Room => Some("ROOM"),
        AttendeeKind::Unknown => Some("UNKNOWN"),
    }
}

/// The attendee of an `ATTENDEE` property `p` with its `email` and `name`
/// as the caller reads them; `keyword` gives a parameter's keyword (upper
/// case, as the caller normalizes it), `uri` a delegate's address (`None`
/// drops it).
pub fn read(
    p: &Property,
    email: String,
    name: Option<String>,
    keyword: impl Fn(&str) -> Option<String>,
    uri: impl Fn(&str) -> Option<String>,
) -> Attendee {
    let delegates = |param: &str| p.param_values(param).filter_map(&uri).collect();
    Attendee {
        email,
        name,
        role: role_of(keyword("ROLE").as_deref()),
        status: status_of(keyword("PARTSTAT").as_deref()),
        kind: kind_of(keyword("CUTYPE").as_deref()),
        rsvp: keyword("RSVP").as_deref() == Some("TRUE"),
        delegated_to: delegates("DELEGATED-TO"),
        delegated_from: delegates("DELEGATED-FROM"),
    }
}

/// An `ATTENDEE` property for `a` with every parameter Nail writes; `uri`
/// turns an address into the calendar user address (`mailto:`).
pub fn property(a: &Attendee, uri: impl Fn(&str) -> String) -> Property {
    let mut p = Property::new("ATTENDEE", uri(&a.email));
    if let Some(name) = &a.name {
        p = p.with_param("CN", name.clone());
    }
    p = p
        .with_param("ROLE", role_keyword(a.role))
        .with_param("PARTSTAT", partstat(a.status));
    if let Some(kind) = cutype(a.kind) {
        p = p.with_param("CUTYPE", kind);
    }
    if a.rsvp {
        p = p.with_param("RSVP", "TRUE");
    }
    let uris = |v: &[String]| v.iter().map(|e| uri(e)).collect::<Vec<_>>();
    p.with_param_values("DELEGATED-TO", uris(&a.delegated_to))
        .with_param_values("DELEGATED-FROM", uris(&a.delegated_from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_round_trip() {
        for role in [
            AttendeeRole::Chair,
            AttendeeRole::Required,
            AttendeeRole::Optional,
            AttendeeRole::NonParticipant,
        ] {
            assert_eq!(role_of(Some(role_keyword(role))), role);
        }
        for status in [
            ParticipationStatus::NeedsAction,
            ParticipationStatus::Accepted,
            ParticipationStatus::Declined,
            ParticipationStatus::Tentative,
            ParticipationStatus::Delegated,
        ] {
            assert_eq!(status_of(Some(partstat(status))), status);
        }
        for kind in [
            AttendeeKind::Individual,
            AttendeeKind::Group,
            AttendeeKind::Resource,
            AttendeeKind::Room,
            AttendeeKind::Unknown,
        ] {
            assert_eq!(kind_of(cutype(kind)), kind);
        }
    }

    #[test]
    fn unknown_keywords_fall_back() {
        assert_eq!(role_of(Some("X-BOSS")), AttendeeRole::Required);
        assert_eq!(status_of(None), ParticipationStatus::NeedsAction);
        assert_eq!(kind_of(Some("x")), AttendeeKind::Individual);
    }

    #[test]
    fn property_writes_every_parameter() {
        let a = Attendee {
            email: "bob@example.org".into(),
            name: Some("Bob".into()),
            role: AttendeeRole::Optional,
            status: ParticipationStatus::Accepted,
            kind: AttendeeKind::Room,
            rsvp: true,
            delegated_to: vec!["eve@example.org".into()],
            delegated_from: Vec::new(),
        };
        let p = property(&a, |e| format!("mailto:{e}"));
        assert_eq!(p.value, "mailto:bob@example.org");
        assert_eq!(p.param("CN"), Some("Bob"));
        assert_eq!(p.param("ROLE"), Some("OPT-PARTICIPANT"));
        assert_eq!(p.param("PARTSTAT"), Some("ACCEPTED"));
        assert_eq!(p.param("CUTYPE"), Some("ROOM"));
        assert_eq!(p.param("RSVP"), Some("TRUE"));
        assert_eq!(
            p.param_values("DELEGATED-TO").collect::<Vec<_>>(),
            ["mailto:eve@example.org"]
        );
        let plain = Attendee {
            kind: AttendeeKind::Individual,
            rsvp: false,
            ..a
        };
        let p = property(&plain, |e| format!("mailto:{e}"));
        assert_eq!(p.param("CUTYPE"), None);
        assert_eq!(p.param("RSVP"), None);
    }

    #[test]
    fn read_inverts_property() {
        let a = Attendee {
            email: "bob@example.org".into(),
            name: None,
            role: AttendeeRole::Chair,
            status: ParticipationStatus::Tentative,
            kind: AttendeeKind::Group,
            rsvp: true,
            delegated_to: vec!["eve@example.org".into()],
            delegated_from: vec!["dan@example.org".into()],
        };
        let p = property(&a, |e| format!("mailto:{e}"));
        let read_back = read(
            &p,
            a.email.clone(),
            None,
            |name| p.param(name).map(str::to_owned),
            |v| v.strip_prefix("mailto:").map(str::to_owned),
        );
        assert_eq!(read_back, a);
        let dropped = read(&p, a.email.clone(), None, |_| None, |_| None);
        assert_eq!(dropped.role, AttendeeRole::Required);
        assert!(dropped.delegated_to.is_empty() && !dropped.rsvp);
    }
}
