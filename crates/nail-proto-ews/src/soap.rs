//! SOAP envelopes: request headers (`RequestServerVersion`,
//! `TimeZoneContext`), the `ServerVersionInfo` of answers, response
//! messages with their `ResponseClass` / `ResponseCode`, SOAP faults
//! (`ErrorServerBusy` with `BackOffMilliseconds`,
//! `ErrorInvalidServerVersion`) and the stream of envelopes a
//! `GetStreamingEvents` answer carries.

use std::fmt;

use crate::error::EwsError;
use crate::xml::{self, Element, escape};

/// The SOAP namespace of EWS messages (`m:`).
pub const MESSAGES_NS: &str = "http://schemas.microsoft.com/exchange/services/2006/messages";

/// The SOAP namespace of EWS types (`t:`).
pub const TYPES_NS: &str = "http://schemas.microsoft.com/exchange/services/2006/types";

/// The SOAP 1.1 envelope namespace.
pub const SOAP_NS: &str = "http://schemas.xmlsoap.org/soap/envelope/";

/// The namespace of EWS error details in faults (`e:`).
pub const ERRORS_NS: &str = "http://schemas.microsoft.com/exchange/services/2006/errors";

/// The schema version Nail asks for (`RequestServerVersion`): Exchange
/// 2016 and later (2019, Subscription Edition) all accept `Exchange2016`.
pub const DEFAULT_SERVER_VERSION: &str = "Exchange2016";

/// The version Nail falls back to when a server refuses
/// [`DEFAULT_SERVER_VERSION`] (`ErrorInvalidServerVersion`; Exchange 2013).
pub const FALLBACK_SERVER_VERSION: &str = "Exchange2013_SP1";

/// Wraps an EWS operation body in a SOAP envelope with the
/// `RequestServerVersion` header.
pub fn envelope(server_version: &str, body: &str) -> String {
    envelope_with(server_version, None, body)
}

/// Wraps an EWS operation body in a SOAP envelope with the
/// `RequestServerVersion` header and, when given, a `TimeZoneContext`
/// (a Windows zone id: times without an offset in the request are wall
/// times in that zone).
pub fn envelope_with(server_version: &str, time_zone: Option<&str>, body: &str) -> String {
    let zone = time_zone.map_or_else(String::new, |tz| {
        format!(
            r#"<t:TimeZoneContext><t:TimeZoneDefinition Id="{}"/></t:TimeZoneContext>"#,
            escape(tz)
        )
    });
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<soap:Envelope xmlns:soap="{soap}" xmlns:t="{types}" xmlns:m="{messages}">"#,
            r#"<soap:Header><t:RequestServerVersion Version="{version}"/>{zone}</soap:Header>"#,
            r#"<soap:Body>{body}</soap:Body></soap:Envelope>"#
        ),
        soap = SOAP_NS,
        types = TYPES_NS,
        messages = MESSAGES_NS,
        version = escape(server_version),
        zone = zone,
        body = body,
    )
}

/// The server's `ServerVersionInfo` header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerVersion {
    /// Major version (15 for Exchange 2013 – Subscription Edition).
    pub major: u32,
    /// Minor version (0 = 2013, 1 = 2016, 2 = 2019 / SE).
    pub minor: u32,
    /// Major build number.
    pub major_build: u32,
    /// Minor build number.
    pub minor_build: u32,
    /// The schema version string (`V2017_07_11`), if sent.
    pub schema: Option<String>,
}

impl ServerVersion {
    /// Reads the header element.
    pub fn from_element(info: &Element) -> Option<Self> {
        let number = |name: &str| info.attr(name).and_then(|v| v.parse().ok());
        Some(Self {
            major: number("MajorVersion")?,
            minor: number("MinorVersion").unwrap_or(0),
            major_build: number("MajorBuildNumber").unwrap_or(0),
            minor_build: number("MinorBuildNumber").unwrap_or(0),
            schema: info.attr("Version").map(str::to_owned),
        })
    }

    /// The product name of the version, where known (`Exchange 2019`).
    pub fn product(&self) -> Option<&'static str> {
        match (self.major, self.minor) {
            (15, 0) => Some("Exchange 2013"),
            (15, 1) => Some("Exchange 2016"),
            (15, 2) => Some("Exchange 2019"),
            (14, _) => Some("Exchange 2010"),
            _ => None,
        }
    }
}

impl fmt::Display for ServerVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.major_build, self.minor_build
        )
    }
}

/// A parsed SOAP answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoapResponse {
    /// The server version from the header, if sent.
    pub server_version: Option<ServerVersion>,
    /// The operation's response element (the first child of `Body`).
    pub body: Element,
}

/// The class of a response message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseClass {
    /// Done.
    Success,
    /// Done with a warning (treated as success).
    Warning,
    /// Failed; see the response code.
    Error,
}

/// One response message (`m:…ResponseMessage`) of an answer.
#[derive(Debug, Clone, Copy)]
pub struct ResponseMessage<'a> {
    /// The class.
    pub class: ResponseClass,
    /// The response code (`NoError`, `ErrorItemNotFound`, …).
    pub code: &'a str,
    /// The server's message text.
    pub text: &'a str,
    /// The element itself (its payload children).
    pub element: &'a Element,
}

impl<'a> ResponseMessage<'a> {
    /// Reads a response message element (also used for answers that carry
    /// `ResponseClass` on the operation element itself, like
    /// `GetInboxRulesResponse`).
    pub fn read(element: &'a Element) -> Self {
        let class = match element.attr("ResponseClass") {
            Some("Error") => ResponseClass::Error,
            Some("Warning") => ResponseClass::Warning,
            _ => ResponseClass::Success,
        };
        Self {
            class,
            code: element.child_text("ResponseCode").unwrap_or("NoError"),
            text: element.child_text("MessageText").unwrap_or(""),
            element,
        }
    }

    /// `BackOffMilliseconds` from the message's `MessageXml`.
    pub fn back_off_ms(&self) -> Option<u64> {
        back_off_from(self.element)
    }

    /// Whether the message failed.
    pub fn is_error(&self) -> bool {
        self.class == ResponseClass::Error
    }

    /// The error of a failed message, `Ok` for success and warnings.
    pub fn check(&self) -> Result<(), EwsError> {
        if self.is_error() {
            Err(self.error())
        } else {
            Ok(())
        }
    }

    /// The error this message reports (whatever its class).
    pub fn error(&self) -> EwsError {
        EwsError::from_response_code(self.code, self.text, self.back_off_ms())
    }
}

fn back_off_from(element: &Element) -> Option<u64> {
    element.child("MessageXml").and_then(|xml| {
        xml.children
            .iter()
            .find(|v| v.attr("Name") == Some("BackOffMilliseconds"))
            .and_then(|v| v.text.trim().parse().ok())
    })
}

/// Every response message of an operation answer
/// (`…Response/ResponseMessages/*`).
pub fn response_messages(body: &Element) -> Vec<ResponseMessage<'_>> {
    body.child("ResponseMessages")
        .map(|list| list.children.iter().map(ResponseMessage::read).collect())
        .unwrap_or_default()
}

/// The single response message of an answer, checked: an error message
/// becomes its [`EwsError`].
pub fn single_message(body: &Element) -> Result<&Element, EwsError> {
    let messages = response_messages(body);
    let first = messages
        .first()
        .ok_or_else(|| EwsError::Protocol(format!("{} without a response message", body.name)))?;
    first.check()?;
    Ok(first.element)
}

/// A SOAP fault (`soap:Fault`): the EWS response code from its detail,
/// the fault string and `BackOffMilliseconds`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// `faultcode`.
    pub fault_code: String,
    /// `faultstring`.
    pub message: String,
    /// The EWS `ResponseCode` of the detail (`ErrorServerBusy`, …).
    pub response_code: Option<String>,
    /// `BackOffMilliseconds` for `ErrorServerBusy`.
    pub back_off_ms: Option<u64>,
}

impl Fault {
    /// The error this fault means.
    pub fn error(&self) -> EwsError {
        let from_fault_code = self
            .fault_code
            .rsplit(':')
            .next()
            .filter(|c| c.starts_with("Error"));
        match self.response_code.as_deref().or(from_fault_code) {
            Some(code) => EwsError::from_response_code(code, &self.message, self.back_off_ms),
            None if self.fault_code.contains("Client") => EwsError::Rejected {
                code: self.fault_code.clone(),
                message: self.message.clone(),
            },
            None => EwsError::Unavailable(self.message.clone(), None),
        }
    }
}

/// What an EWS answer document holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A response.
    Response(SoapResponse),
    /// A SOAP fault.
    Fault(Fault, Option<ServerVersion>),
}

/// Parses an answer document.
pub fn parse_answer(document: &str) -> Result<Answer, EwsError> {
    let root = xml::parse(document)?;
    if root.name != "Envelope" {
        return Err(EwsError::Protocol(format!(
            "expected a SOAP envelope, got <{}>",
            root.name
        )));
    }
    let server_version = root
        .child("Header")
        .and_then(|h| h.child("ServerVersionInfo"))
        .and_then(ServerVersion::from_element);
    let body = root
        .child("Body")
        .ok_or_else(|| EwsError::Protocol("SOAP envelope without a body".to_owned()))?;
    let first = body
        .children
        .first()
        .ok_or_else(|| EwsError::Protocol("empty SOAP body".to_owned()))?;
    if first.name == "Fault" {
        let detail = first.child("detail");
        let response_code = detail
            .and_then(|d| d.child_value("ResponseCode"))
            .or_else(|| {
                // Some servers only name the code in the fault code
                // (`a:ErrorSchemaValidation`).
                first
                    .child_text("faultcode")
                    .and_then(|c| c.rsplit(':').next())
                    .filter(|c| c.starts_with("Error"))
                    .map(str::to_owned)
            });
        let fault = Fault {
            fault_code: first.child_value("faultcode").unwrap_or_default(),
            message: first
                .child_value("faultstring")
                .or_else(|| detail.and_then(|d| d.child_value("Message")))
                .unwrap_or_default(),
            response_code,
            back_off_ms: detail.and_then(back_off_from),
        };
        return Ok(Answer::Fault(fault, server_version));
    }
    let mut body = body.clone();
    let operation = body.children.swap_remove(0);
    Ok(Answer::Response(SoapResponse {
        server_version,
        body: operation,
    }))
}

/// Splits complete SOAP envelopes off the front of a streaming answer
/// buffer (`GetStreamingEvents` writes one envelope per notification batch
/// into one chunked HTTP answer). Answers the complete envelopes; the rest
/// stays in `buffer`.
pub fn split_envelopes(buffer: &mut String) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(end) = find_envelope_end(buffer) {
        let rest = buffer.split_off(end);
        let document = std::mem::replace(buffer, rest);
        let trimmed = document.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_owned());
        }
    }
    out
}

/// The byte offset just after the first `</[prefix:]Envelope>` close tag.
fn find_envelope_end(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(pos) = text[from..].find("Envelope>") {
        let at = from + pos;
        let open = text[..at].rfind('<')?;
        let tag = &text[open..at];
        if tag.starts_with("</")
            && tag[2..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
            && (tag.len() == 2 || tag.ends_with(':'))
        {
            return Some(at + "Envelope>".len());
        }
        from = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">"#,
        r#"<s:Header><h:ServerVersionInfo MajorVersion="15" MinorVersion="2" MajorBuildNumber="1544" "#,
        r#"MinorBuildNumber="4" Version="V2017_07_11" xmlns:h="http://schemas.microsoft.com/exchange/services/2006/types"/></s:Header>"#,
        r#"<s:Body><m:GetItemResponse xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages">"#,
        r#"<m:ResponseMessages>"#,
        r#"<m:GetItemResponseMessage ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode><m:Items/></m:GetItemResponseMessage>"#,
        r#"<m:GetItemResponseMessage ResponseClass="Error"><m:MessageText>The specified object was not found in the store.</m:MessageText>"#,
        r#"<m:ResponseCode>ErrorItemNotFound</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey><m:Items/></m:GetItemResponseMessage>"#,
        r#"</m:ResponseMessages></m:GetItemResponse></s:Body></s:Envelope>"#
    );

    const BUSY_FAULT: &str = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault>"#,
        r#"<faultcode xmlns:a="http://schemas.microsoft.com/exchange/services/2006/types">a:ErrorServerBusy</faultcode>"#,
        r#"<faultstring xml:lang="en-US">The server cannot service this request right now. Try again later.</faultstring>"#,
        r#"<detail><e:ResponseCode xmlns:e="http://schemas.microsoft.com/exchange/services/2006/errors">ErrorServerBusy</e:ResponseCode>"#,
        r#"<e:Message xmlns:e="http://schemas.microsoft.com/exchange/services/2006/errors">The server cannot service this request right now. Try again later.</e:Message>"#,
        r#"<t:MessageXml xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types">"#,
        r#"<t:Value Name="BackOffMilliseconds">29219</t:Value></t:MessageXml></detail>"#,
        r#"</s:Fault></s:Body></s:Envelope>"#
    );

    #[test]
    fn envelopes_carry_version_and_zone() {
        let soap = envelope(DEFAULT_SERVER_VERSION, "<m:GetFolder/>");
        assert!(soap.starts_with("<?xml"));
        assert!(soap.contains(r#"<t:RequestServerVersion Version="Exchange2016"/>"#));
        assert!(soap.contains("<soap:Body><m:GetFolder/></soap:Body>"));
        assert!(!soap.contains("TimeZoneContext"));
        let zoned = envelope_with(
            FALLBACK_SERVER_VERSION,
            Some("W. Europe Standard Time"),
            "<m:X/>",
        );
        assert!(zoned.contains(r#"<t:TimeZoneDefinition Id="W. Europe Standard Time"/>"#));
        assert!(zoned.contains("Exchange2013_SP1"));
        assert!(xml::parse(&zoned).is_ok());
    }

    #[test]
    fn reads_versions_and_response_messages() {
        let Answer::Response(response) = parse_answer(RESPONSE).unwrap() else {
            panic!("expected a response");
        };
        let version = response.server_version.unwrap();
        assert_eq!(version.to_string(), "15.2.1544.4");
        assert_eq!(version.product(), Some("Exchange 2019"));
        assert_eq!(version.schema.as_deref(), Some("V2017_07_11"));
        assert_eq!(response.body.name, "GetItemResponse");
        let messages = response_messages(&response.body);
        assert_eq!(messages.len(), 2);
        assert!(messages[0].check().is_ok());
        assert_eq!(messages[1].code, "ErrorItemNotFound");
        assert!(matches!(messages[1].check(), Err(EwsError::NotFound(_))));
        assert!(single_message(&response.body).is_ok());
    }

    #[test]
    fn reads_busy_faults() {
        let Answer::Fault(fault, _) = parse_answer(BUSY_FAULT).unwrap() else {
            panic!("expected a fault");
        };
        assert_eq!(fault.response_code.as_deref(), Some("ErrorServerBusy"));
        assert_eq!(fault.back_off_ms, Some(29_219));
        assert_eq!(fault.error(), EwsError::RateLimited(Some(30)));
        let schema = Fault {
            fault_code: "a:ErrorSchemaValidation".into(),
            message: "bad".into(),
            response_code: None,
            back_off_ms: None,
        };
        assert!(matches!(schema.error(), EwsError::Rejected { .. }));
        assert!(matches!(
            parse_answer("<html/>"),
            Err(EwsError::Protocol(_))
        ));
        assert!(matches!(
            parse_answer(r#"<s:Envelope xmlns:s="x"><s:Body/></s:Envelope>"#),
            Err(EwsError::Protocol(_))
        ));
    }

    #[test]
    fn splits_streamed_envelopes() {
        let one = r#"<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/"><Body><A/></Body></Envelope>"#;
        let two = r#"<soap11:Envelope xmlns:soap11="http://schemas.xmlsoap.org/soap/envelope/"><soap11:Body><B/></soap11:Body></soap11:Envelope>"#;
        let mut buffer = format!("{one}\r\n{two}<soap11:Envel");
        let docs = split_envelopes(&mut buffer);
        assert_eq!(docs, [one.to_owned(), two.to_owned()]);
        assert_eq!(buffer, "<soap11:Envel");
        buffer.push_str("ope><x>Envelope></x></soap11:Envelope>");
        let docs = split_envelopes(&mut buffer);
        assert_eq!(docs.len(), 1);
        assert!(buffer.is_empty());
    }
}
