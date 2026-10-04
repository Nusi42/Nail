//! Smoke tests of the libraries WP72 builds on (WP70, ADR 0058): custom
//! WebDAV methods through reqwest with the ring TLS configuration,
//! namespaced multistatus parsing with quick-xml, Basic credentials.
#![cfg(test)]

use base64::Engine as _;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

const MULTISTATUS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/">
  <d:response>
    <d:href>/dav/ada/work/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Work</d:displayname>
        <cs:getctag>"42"</cs:getctag>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

#[test]
fn multistatus_is_parsed_with_namespaces() {
    let mut reader = NsReader::from_str(MULTISTATUS);
    reader.config_mut().trim_text(true);
    let mut current: Option<(String, String)> = None;
    let mut found = Vec::new();
    loop {
        match reader.read_resolved_event().unwrap() {
            (ResolveResult::Bound(ns), Event::Start(e)) => {
                let ns = AsRef::<str>::as_ref(&ns).to_owned();
                let local = AsRef::<str>::as_ref(&e.local_name()).to_owned();
                current = Some((ns, local));
            }
            (_, Event::Text(t)) => {
                if let Some((ns, local)) = current.take() {
                    found.push((ns, local, t.xml10_content().into_owned()));
                }
            }
            (_, Event::Eof) => break,
            _ => {}
        }
    }
    assert!(found.contains(&("DAV:".into(), "href".into(), "/dav/ada/work/".into())));
    assert!(found.contains(&("DAV:".into(), "displayname".into(), "Work".into())));
    assert!(found.contains(&(
        nail_dav::NS_CALENDARSERVER.into(),
        "getctag".into(),
        "\"42\"".into()
    )));
}

#[test]
fn webdav_methods_and_basic_auth() {
    let tls = nail_auth::tls_config().expect("ring TLS configuration");
    let client = nail_auth::http_client_builder(tls).build().unwrap();
    for method in ["PROPFIND", "REPORT", "MKCALENDAR", "PROPPATCH"] {
        let method = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let request = client
            .request(method.clone(), "https://dav.example.org/dav/")
            .header("Depth", "1")
            .basic_auth("ada", Some("secret"))
            .build()
            .unwrap();
        assert_eq!(request.method(), &method);
        let auth = request.headers()["authorization"].to_str().unwrap();
        assert_eq!(
            auth,
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("ada:secret")
            )
        );
    }
}
