//! Live Windows sign-in against a real Exchange Server (run by the maintainer; ignored
//! by default because it needs a server and a domain account).
//!
//! ```text
//! set NAIL_EWS_LIVE_URL=https://mail.corp.example/EWS/Exchange.asmx
//! rem optional: another user than the signed-in one
//! set NAIL_EWS_LIVE_USER=CORP\ada
//! set NAIL_EWS_LIVE_PASSWORD=…
//! rem optional: negotiate (default), kerberos or ntlm
//! set NAIL_EWS_LIVE_PACKAGE=negotiate
//! cargo test -p nail-exchange --test sspi_live -- --ignored
//! ```
//!
//! It sends one `GET` to the EWS endpoint with the HTTP Negotiate
//! handshake (`SspiNegotiate` over the real `ReqwestHttp`) and expects the
//! server to accept the login (any answer but `401`; EWS answers a `GET`
//! with its service description or `405`).
#![cfg(test)]

use nail_api::accounts::SspiPackage;
use nail_exchange::http::{HttpRequest, ReqwestHttp};
use nail_exchange::negotiate::{HttpLogin, Password, send_signed_in};
use nail_exchange::sspi::SspiNegotiate;

#[tokio::test]
#[ignore = "needs a live Exchange Server: set NAIL_EWS_LIVE_URL"]
async fn windows_sign_in_against_a_live_exchange_server() {
    let Ok(url) = std::env::var("NAIL_EWS_LIVE_URL") else {
        panic!("set NAIL_EWS_LIVE_URL to the server's EWS URL");
    };
    let package = match std::env::var("NAIL_EWS_LIVE_PACKAGE").as_deref() {
        Ok("kerberos") => SspiPackage::Kerberos,
        Ok("ntlm") => SspiPackage::Ntlm,
        _ => SspiPackage::Negotiate,
    };
    let auth = match std::env::var("NAIL_EWS_LIVE_USER") {
        Ok(user) => SspiNegotiate::with_password(
            user,
            Password::new(std::env::var("NAIL_EWS_LIVE_PASSWORD").unwrap_or_default()),
            package,
        ),
        Err(_) => SspiNegotiate::current_user(package),
    };
    let http = ReqwestHttp::system().expect("HTTP client");
    let request = HttpRequest::get(url.parse().expect("a URL"));
    let answer = send_signed_in(&http, request, &HttpLogin::Windows(auth.login()))
        .await
        .expect("signed in");
    assert_ne!(answer.status, 401, "the server rejected the login");
}
