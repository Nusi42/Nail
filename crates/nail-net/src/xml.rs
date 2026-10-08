//! A small namespace-aware XML tree on [`quick_xml`] for protocol bodies
//! (WebDAV multistatus answers and the mock DAV server's requests, EWS SOAP
//! envelopes), and escaping for the bodies Nail writes.
//!
//! Protocol documents are small and shallow; a tree keeps the protocol code
//! readable. Limits guard against hostile servers: at most `max_depth`
//! levels ([`MAX_DEPTH`] by default) and [`MAX_ELEMENTS`] elements per
//! document; DOCTYPEs and custom entities are refused.
//!
//! [`parse_tree`] builds any [`XmlNode`], so a crate with its own element
//! type (EWS matches by local name and writes elements back) shares the
//! parser; [`Element`] is the namespace-aware tree WebDAV uses.

use std::borrow::Cow;

use quick_xml::events::{BytesRef, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

/// Deepest element nesting accepted by [`parse`].
pub const MAX_DEPTH: usize = 64;
/// Most elements accepted in one document.
pub const MAX_ELEMENTS: usize = 2_000_000;

/// XML namespaces of WebDAV and its extensions, for the DAV client and the
/// mock DAV server.
pub mod ns {
    /// The WebDAV namespace.
    pub const DAV: &str = "DAV:";
    /// The CalDAV namespace (RFC 4791).
    pub const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
    /// The CardDAV namespace (RFC 6352).
    pub const CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
    /// The CalendarServer namespace (`getctag`).
    pub const CALENDARSERVER: &str = "http://calendarserver.org/ns/";
    /// Apple's iCal namespace (`calendar-color`).
    pub const APPLE_ICAL: &str = "http://apple.com/ns/ical/";
}

/// A document that is not well-formed XML (or breaks the limits).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed XML: {0}")]
pub struct XmlError(pub String);

/// An element type [`parse_tree`] can build.
pub trait XmlNode: Sized {
    /// A new element `{ns}name` with its attributes (local name, unescaped
    /// value; namespace declarations left out).
    fn open(ns: String, name: String, attrs: Vec<(String, String)>) -> Self;
    /// The element's own text, to append character data to.
    fn text_mut(&mut self) -> &mut String;
    /// Appends a finished child element.
    fn push_child(&mut self, child: Self);
}

/// The value of attribute `name` in a list of (local name, value) pairs.
#[must_use]
pub fn attr_value<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// An element: namespace, local name, child elements and its own text
/// (character data and CDATA of the element itself, entities resolved).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    /// Namespace URI (`""` when unbound).
    pub ns: String,
    /// Local name.
    pub name: String,
    /// Child elements in document order.
    pub children: Vec<Element>,
    /// The element's own text.
    pub text: String,
    /// Attributes (local name, unescaped value); namespace declarations
    /// are left out.
    pub attrs: Vec<(String, String)>,
}

impl XmlNode for Element {
    fn open(ns: String, name: String, attrs: Vec<(String, String)>) -> Self {
        Self {
            ns,
            name,
            children: Vec::new(),
            text: String::new(),
            attrs,
        }
    }

    fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    fn push_child(&mut self, child: Self) {
        self.children.push(child);
    }
}

impl Element {
    /// The value of attribute `name` (local name, any prefix).
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        attr_value(&self.attrs, name)
    }

    /// Whether the element is `{ns}name`.
    #[must_use]
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }

    /// The first child `{ns}name`.
    #[must_use]
    pub fn child(&self, ns: &str, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.is(ns, name))
    }

    /// Every child `{ns}name`.
    pub fn children_named<'a>(
        &'a self,
        ns: &'a str,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.is(ns, name))
    }

    /// The trimmed text of the first child `{ns}name`.
    #[must_use]
    pub fn child_text(&self, ns: &str, name: &str) -> Option<&str> {
        self.child(ns, name).map(|c| c.text.trim())
    }

    /// Follows a path of `(ns, name)` steps through first matches.
    #[must_use]
    pub fn path(&self, steps: &[(&str, &str)]) -> Option<&Element> {
        let mut at = self;
        for (ns, name) in steps {
            at = at.child(ns, name)?;
        }
        Some(at)
    }
}

fn resolve_reference(reference: &BytesRef<'_>) -> Result<String, XmlError> {
    if let Some(c) = reference
        .resolve_char_ref()
        .map_err(|e| XmlError(e.to_string()))?
    {
        return Ok(c.to_string());
    }
    let name = reference.xml10_content();
    let text = match name.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        other => return Err(XmlError(format!("unknown entity &{other};"))),
    };
    Ok(text.to_owned())
}

fn namespace(resolved: &ResolveResult<'_>) -> String {
    match resolved {
        ResolveResult::Bound(ns) => ns.into_inner().to_owned(),
        _ => String::new(),
    }
}

fn attributes(
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<Vec<(String, String)>, XmlError> {
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| XmlError(e.to_string()))?;
        let key = AsRef::<str>::as_ref(&attr.key);
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = AsRef::<str>::as_ref(&attr.key.local_name()).to_owned();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| XmlError(e.to_string()))?
            .into_owned();
        attrs.push((local, value));
    }
    Ok(attrs)
}

/// Parses a document into its root [`Element`] (at most [`MAX_DEPTH`]
/// levels).
pub fn parse(document: &str) -> Result<Element, XmlError> {
    parse_tree(document, MAX_DEPTH)
}

/// Parses a document into its root element of type `N`, with at most
/// `max_depth` levels and [`MAX_ELEMENTS`] elements.
pub fn parse_tree<N: XmlNode>(document: &str, max_depth: usize) -> Result<N, XmlError> {
    let mut reader = NsReader::from_str(document);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<N> = Vec::new();
    let mut root: Option<N> = None;
    let mut count = 0usize;
    loop {
        let (resolved, event) = reader
            .read_resolved_event()
            .map_err(|e| XmlError(e.to_string()))?;
        match event {
            Event::Start(start) => {
                count += 1;
                if stack.len() >= max_depth || count > MAX_ELEMENTS {
                    return Err(XmlError("document too deep or too large".to_owned()));
                }
                if root.is_some() {
                    return Err(XmlError("content after the root element".to_owned()));
                }
                let name = AsRef::<str>::as_ref(&start.local_name()).to_owned();
                let attrs = attributes(&start)?;
                stack.push(N::open(namespace(&resolved), name, attrs));
            }
            Event::End(_) => {
                let done = stack
                    .pop()
                    .ok_or_else(|| XmlError("unbalanced end tag".to_owned()))?;
                match stack.last_mut() {
                    Some(parent) => parent.push_child(done),
                    None => root = Some(done),
                }
            }
            Event::Text(text) => {
                if let Some(top) = stack.last_mut() {
                    top.text_mut().push_str(&text.xml10_content());
                } else if !text.xml10_content().trim().is_empty() {
                    return Err(XmlError("text outside the root element".to_owned()));
                }
            }
            Event::CData(data) => {
                if let Some(top) = stack.last_mut() {
                    top.text_mut().push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                let text = resolve_reference(&reference)?;
                if let Some(top) = stack.last_mut() {
                    top.text_mut().push_str(&text);
                }
            }
            Event::DocType(_) => {
                return Err(XmlError("DOCTYPE is not allowed".to_owned()));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(XmlError("unexpected end of document".to_owned()));
    }
    root.ok_or_else(|| XmlError("empty document".to_owned()))
}

/// Escapes text for element content or attribute values.
#[must_use]
pub fn escape(text: &str) -> Cow<'_, str> {
    quick_xml::escape::escape(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::ns::{CALDAV as NS_CALDAV, DAV as NS_DAV};

    #[test]
    fn parses_namespaces_text_entities_and_cdata() {
        let doc = r#"<?xml version="1.0"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response>
    <href>/a%20b/x.ics</href>
    <propstat><prop>
      <C:calendar-data C:content-type="text/calendar" version="2.0">BEGIN:VCALENDAR&#13;
SUMMARY:Tom &amp; Jerry &lt;3</C:calendar-data>
      <displayname><![CDATA[a <b>]]></displayname>
      <empty/>
    </prop></propstat>
  </response>
</multistatus>"#;
        let root = parse(doc).unwrap();
        assert!(root.is(NS_DAV, "multistatus"));
        let response = root.child(NS_DAV, "response").unwrap();
        assert_eq!(response.child_text(NS_DAV, "href"), Some("/a%20b/x.ics"));
        let prop = response
            .path(&[(NS_DAV, "propstat"), (NS_DAV, "prop")])
            .unwrap();
        assert_eq!(
            prop.child(NS_CALDAV, "calendar-data").unwrap().text,
            "BEGIN:VCALENDAR\r\nSUMMARY:Tom & Jerry <3"
        );
        assert_eq!(prop.child_text(NS_DAV, "displayname"), Some("a <b>"));
        assert_eq!(prop.child(NS_DAV, "empty").unwrap().children.len(), 0);
        let data = prop.child(NS_CALDAV, "calendar-data").unwrap();
        assert_eq!(data.attr("content-type"), Some("text/calendar"));
        assert_eq!(data.attr("version"), Some("2.0"));
        assert_eq!(data.attr("missing"), None);
        assert!(root.attrs.is_empty(), "{:?}", root.attrs);
        assert_eq!(root.children_named(NS_DAV, "response").count(), 1);
    }

    #[test]
    fn rejects_malformed_documents() {
        for bad in [
            "",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>",
            "<a>&unknown;</a>",
            "text<a/>",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "<a>".repeat(MAX_DEPTH + 1) + &"</a>".repeat(MAX_DEPTH + 1);
        assert!(parse(&deep).is_err());
        let ok = "<a>".repeat(MAX_DEPTH + 1) + &"</a>".repeat(MAX_DEPTH + 1);
        assert!(parse_tree::<Element>(&ok, MAX_DEPTH + 1).is_ok());
    }

    #[test]
    fn unbound_prefixless_elements_have_no_namespace() {
        let root = parse("<a><b>x</b></a>").unwrap();
        assert_eq!(root.ns, "");
        assert_eq!(root.child_text("", "b"), Some("x"));
        assert_eq!(escape("a<b&\"c\""), "a&lt;b&amp;&quot;c&quot;");
    }

    #[test]
    fn attribute_lookup() {
        let attrs = vec![("a".to_owned(), "1".to_owned())];
        assert_eq!(attr_value(&attrs, "a"), Some("1"));
        assert_eq!(attr_value(&attrs, "b"), None);
    }
}
