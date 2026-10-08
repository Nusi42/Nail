//! Smoke tests of the libraries WP72 builds on (WP70, ADR 0058): custom
//! WebDAV methods through reqwest with the ring TLS configuration,
//! namespaced multistatus parsing (`nail_net::xml`), Basic credentials.
#![cfg(test)]

use nail_dav::{NS_CALENDARSERVER, NS_DAV};

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
    let root = nail_dav::xml::parse(MULTISTATUS).unwrap();
    let response = root.child(NS_DAV, "response").unwrap();
    let prop = response
        .path(&[(NS_DAV, "propstat"), (NS_DAV, "prop")])
        .unwrap();
    let found: Vec<(String, String, String)> = std::iter::once(response)
        .chain(&prop.children)
        .flat_map(|e| std::iter::once(e).chain(&e.children))
        .map(|e| (e.ns.clone(), e.name.clone(), e.text.trim().to_owned()))
        .collect();
    assert!(found.contains(&("DAV:".into(), "href".into(), "/dav/ada/work/".into())));
    assert!(found.contains(&("DAV:".into(), "displayname".into(), "Work".into())));
    assert!(found.contains(&(NS_CALENDARSERVER.into(), "getctag".into(), "\"42\"".into())));
}

#[test]
fn webdav_methods_and_basic_auth() {
    let client = nail_auth::http_client().unwrap();
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
        assert_eq!(auth, nail_net::http::basic("ada", "secret"));
    }
}
