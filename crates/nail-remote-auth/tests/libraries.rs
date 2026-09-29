//! Smoke tests of the libraries WP41 builds on (WP40, ADR 0036): they
//! compile and work on Windows MSVC without OpenSSL, and produce what the
//! store accepts.

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use nail_api::remote::RemoteGrant;
use nail_api::remote::{DEVICE_COOKIE, SESSION_COOKIE};
use nail_store::Store;
use totp_rs::{Algorithm, Builder};
use webauthn_rp::PublicKeyCredentialCreationOptions;
use webauthn_rp::request::auth::DiscoverableCredentialRequestOptions;
use webauthn_rp::request::register::UserHandle64;
use webauthn_rp::request::{AsciiDomain, RpId};

#[test]
fn argon2id_hashes_are_phc_strings_the_store_accepts() {
    let hash = Argon2::default()
        .hash_password(b"correct horse battery staple")
        .unwrap()
        .to_string();
    assert!(hash.starts_with("$argon2id$v=19$"), "{hash}");
    let parsed = PasswordHash::new(&hash).unwrap();
    assert!(
        Argon2::default()
            .verify_password(b"correct horse battery staple", &parsed)
            .is_ok()
    );
    assert!(
        Argon2::default()
            .verify_password(b"wrong", &parsed)
            .is_err()
    );

    let store = Store::open_in_memory().unwrap();
    let user = store
        .create_remote_user("ada", None, &RemoteGrant::default(), 1)
        .unwrap();
    let user = store
        .set_remote_password_hash(&user.id, Some(&hash), 2)
        .unwrap();
    assert!(user.has_password);
}

#[test]
fn totp_matches_rfc_6238_and_builds_otpauth_urls() {
    // RFC 6238 appendix B, SHA-1, 8 digits, T = 59 s.
    let rfc = Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(8)
        .with_skew(0)
        .with_step_duration(30)
        .with_secret(b"12345678901234567890".to_vec())
        .build()
        .unwrap();
    assert_eq!(rfc.generate(59).to_string(), "94287082");

    let secret = vec![7_u8; 20];
    let totp = Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(secret)
        .with_account_name("ada")
        .with_issuer(Some("Nail"))
        .build()
        .unwrap();
    let code = totp.generate(1_000_000).to_string();
    // ±1 step is accepted; the returned counter feeds the replay guard.
    assert_eq!(totp.check(&code, 1_000_000), Some(1_000_000 / 30));
    assert!(totp.check(&code, 1_000_000 + 30).is_some());
    assert!(totp.check(&code, 1_000_000 + 90).is_none());
    let url = totp.to_url().unwrap();
    assert!(url.starts_with("otpauth://totp/Nail:ada?"), "{url}");
    assert!(url.contains("issuer=Nail"));
}

#[test]
fn webauthn_ceremonies_start_for_a_host_name() {
    let rp_id = RpId::Domain(AsciiDomain::try_from("nail.local".to_owned()).unwrap());
    let user = UserHandle64::new();
    let (_server, client) =
        PublicKeyCredentialCreationOptions::first_passkey_with_blank_user_info(&rp_id, &user)
            .start_ceremony()
            .unwrap();
    let options = serde_json::to_value(&client).unwrap();
    let text = options.to_string();
    assert!(text.contains("nail.local"), "{text}");
    assert!(text.contains("challenge"), "{text}");

    let (_server, client) = DiscoverableCredentialRequestOptions::passkey(&rp_id)
        .start_ceremony()
        .unwrap();
    let options = serde_json::to_value(&client).unwrap();
    assert!(options.to_string().contains("challenge"));
    // Browsers refuse IP addresses as relying-party ids, but the library
    // accepts them: the host name comes from the settings, which reject
    // IP addresses (`RemoteUiSettingsIssue::InvalidHostName`).
    assert!(AsciiDomain::try_from("192.168.1.2".to_owned()).is_ok());
    let settings = nail_api::remote::RemoteUiSettings {
        host_name: Some("192.168.1.2".into()),
        ..Default::default()
    };
    assert_eq!(
        settings.issues(),
        [nail_api::remote::RemoteUiSettingsIssue::InvalidHostName]
    );
}

#[test]
fn cookie_attributes_for_the_session_cookie() {
    let cookie = cookie::Cookie::build((SESSION_COOKIE, "token"))
        .http_only(true)
        .secure(true)
        .same_site(cookie::SameSite::Strict)
        .path("/")
        .build();
    let text = cookie.to_string();
    for part in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(text.contains(part), "{text}");
    }
    assert!(SESSION_COOKIE.starts_with("__Host-") && DEVICE_COOKIE.starts_with("__Host-"));
}
