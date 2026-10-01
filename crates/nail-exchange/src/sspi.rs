//! Windows sign-in through SSPI (plan M3c WP93; contract ADR 0072).
//!
//! [`SspiNegotiate`] is the [`NegotiateAuth`] the EWS engine (and
//! Autodiscover) use for `auth "windows"`: `AcquireCredentialsHandleW`
//! with the signed-in Windows user (no secret stored) or explicit
//! credentials (`DOMAIN\user` / `user@domain` and a password from the
//! keychain), then one `InitializeSecurityContextW` call per round of the
//! HTTP 401 handshake ([`crate::negotiate::send_signed_in`]). The package
//! is Negotiate (Kerberos, falling back to NTLM), Kerberos or NTLM
//! ([`SspiPackage`]). No NTLM or Kerberos code of our own: Windows does the
//! cryptography (plan "Scope decisions for M3"). Off Windows every call
//! answers `unsupported`.

use std::fmt;
use std::sync::Arc;

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, SspiPackage};
use nail_api::engine::EngineError;
use nail_api::exchange::engine::{NegotiateAuth, NegotiateSession};
use nail_api::localized::LocalizedMessage;

use crate::negotiate::{Password, WindowsLogin};
use crate::texts;

/// What SSPI says about a security package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    /// The package's name as SSPI reports it (`Negotiate`, `Kerberos`,
    /// `NTLM`).
    pub name: String,
    /// The largest token the package produces (bytes).
    pub max_token: u32,
}

/// Asks SSPI about `package`. Off Windows: `unsupported`.
pub fn package_info(package: SspiPackage) -> Result<PackageInfo, EngineError> {
    imp::package_info(package)
}

/// Whose credentials sign in.
#[derive(Clone, PartialEq, Eq)]
pub enum SspiCredentials {
    /// The signed-in Windows user (single sign-on; nothing stored).
    CurrentUser,
    /// Explicit credentials.
    Explicit {
        /// `DOMAIN\user`, `user@domain` or `user`.
        username: String,
        /// The password.
        password: Password,
    },
}

impl fmt::Debug for SspiCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentUser => f.write_str("CurrentUser"),
            Self::Explicit { username, .. } => f
                .debug_struct("Explicit")
                .field("username", username)
                .finish_non_exhaustive(),
        }
    }
}

/// A user name split for `SEC_WINNT_AUTH_IDENTITY`: `DOMAIN\user` →
/// (`user`, `DOMAIN`); `user@domain` stays whole with an empty domain
/// (SSPI accepts UPNs that way).
pub fn split_username(username: &str) -> (&str, &str) {
    match username.split_once('\\') {
        Some((domain, user)) => (user, domain),
        None => (username, ""),
    }
}

/// HTTP Negotiate / Kerberos / NTLM through SSPI.
#[derive(Debug, Clone)]
pub struct SspiNegotiate {
    package: SspiPackage,
    credentials: SspiCredentials,
}

impl SspiNegotiate {
    /// The signed-in Windows user with `package`.
    pub fn current_user(package: SspiPackage) -> Self {
        Self {
            package,
            credentials: SspiCredentials::CurrentUser,
        }
    }

    /// Explicit credentials with `package`.
    pub fn with_password(
        username: impl Into<String>,
        password: Password,
        package: SspiPackage,
    ) -> Self {
        Self {
            package,
            credentials: SspiCredentials::Explicit {
                username: username.into(),
                password,
            },
        }
    }

    /// The sign-in an `auth "windows"` node describes: without a user name
    /// the signed-in user; with one, `password` (from the keychain) must be
    /// given (`auth_failed` with `exchange-signin-needs-password`
    /// otherwise). Other auth kinds: `invalid_request`.
    pub fn from_auth(auth: &AuthConfig, password: Option<Password>) -> Result<Self, EngineError> {
        let AuthConfig::Windows {
            username, package, ..
        } = auth
        else {
            return Err(EngineError::permanent(
                ErrorCode::InvalidRequest,
                "not a Windows sign-in",
            ));
        };
        let package = package.unwrap_or_default();
        match username.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            None => Ok(Self::current_user(package)),
            Some(user) => match password {
                Some(password) => Ok(Self::with_password(user, password, package)),
                None => Err(EngineError::permanent(
                    ErrorCode::AuthFailed,
                    "Windows sign-in with a user name needs its password",
                )
                .with_localized(LocalizedMessage::new(texts::SIGNIN_NEEDS_PASSWORD))),
            },
        }
    }

    /// The package.
    pub fn package(&self) -> SspiPackage {
        self.package
    }

    /// The credentials.
    pub fn credentials(&self) -> &SspiCredentials {
        &self.credentials
    }

    /// The HTTP login: this package for `Negotiate` challenges, NTLM for
    /// servers that offer only `NTLM` (unless the package is Kerberos,
    /// which must not fall back).
    pub fn login(&self) -> WindowsLogin {
        let ntlm: Option<Arc<dyn NegotiateAuth>> = match self.package {
            SspiPackage::Negotiate => Some(Arc::new(Self {
                package: SspiPackage::Ntlm,
                credentials: self.credentials.clone(),
            })),
            SspiPackage::Ntlm | SspiPackage::Kerberos => None,
        };
        WindowsLogin {
            negotiate: Arc::new(self.clone()),
            ntlm,
        }
    }
}

impl NegotiateAuth for SspiNegotiate {
    fn start(&self, spn: &str) -> Result<Box<dyn NegotiateSession>, EngineError> {
        imp::start(self.package, &self.credentials, spn)
    }
}

fn sspi_error(code: ErrorCode, message: String) -> EngineError {
    EngineError::permanent(code, message).with_localized(LocalizedMessage::new(
        if code == ErrorCode::AuthFailed {
            texts::SIGNIN_REJECTED
        } else {
            texts::SIGNIN_WINDOWS
        },
    ))
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use std::ffi::c_void;

    use nail_api::exchange::engine::NegotiateStep;
    use windows::Win32::Foundation::{
        SEC_E_INTERNAL_ERROR, SEC_E_INVALID_TOKEN, SEC_E_LOGON_DENIED, SEC_E_NO_CREDENTIALS,
        SEC_E_OK, SEC_E_UNKNOWN_CREDENTIALS, SEC_E_WRONG_PRINCIPAL, SEC_I_COMPLETE_AND_CONTINUE,
        SEC_I_COMPLETE_NEEDED, SEC_I_CONTINUE_NEEDED,
    };
    use windows::Win32::Security::Authentication::Identity::{
        AcquireCredentialsHandleW, CompleteAuthToken, DeleteSecurityContext, FreeContextBuffer,
        FreeCredentialsHandle, ISC_REQ_CONNECTION, ISC_REQ_FLAGS, ISC_REQ_MUTUAL_AUTH,
        InitializeSecurityContextW, QuerySecurityPackageInfoW, SECBUFFER_TOKEN, SECBUFFER_VERSION,
        SECPKG_CRED_OUTBOUND, SECURITY_NATIVE_DREP, SecBuffer, SecBufferDesc,
    };
    use windows::Win32::Security::Credentials::SecHandle;
    use windows::core::{HRESULT, HSTRING, PCWSTR};

    use super::{
        EngineError, ErrorCode, NegotiateSession, PackageInfo, SspiCredentials, SspiPackage,
        split_username, sspi_error,
    };

    pub(super) fn package_info(package: SspiPackage) -> Result<PackageInfo, EngineError> {
        let name = HSTRING::from(package.sspi_name());
        // SAFETY: `name` outlives the call; on success SSPI returns a
        // buffer it owns, which is read once and freed with
        // `FreeContextBuffer` below.
        let info = unsafe { QuerySecurityPackageInfoW(&name) }.map_err(|e| {
            EngineError::permanent(
                ErrorCode::Unsupported,
                format!("SSPI package {}: {e}", package.sspi_name()),
            )
        })?;
        if info.is_null() {
            return Err(EngineError::permanent(
                ErrorCode::Unsupported,
                format!("SSPI package {} not found", package.sspi_name()),
            ));
        }
        // SAFETY: `info` is a valid `SecPkgInfoW` returned by SSPI; its
        // `Name` is a NUL-terminated wide string inside the same buffer.
        let result = unsafe {
            let record = &*info;
            PackageInfo {
                name: PCWSTR(record.Name).to_string().unwrap_or_default(),
                max_token: record.cbMaxToken,
            }
        };
        // SAFETY: `info` came from `QuerySecurityPackageInfoW` and is
        // freed exactly once.
        let _ = unsafe { FreeContextBuffer(info.cast()) };
        Ok(result)
    }

    /// `SEC_WINNT_AUTH_IDENTITY_W` (rpcdce.h), laid out as Windows expects;
    /// declared here so the crate needs no `Win32_System_Rpc` feature.
    #[repr(C)]
    struct AuthIdentity {
        user: *mut u16,
        user_length: u32,
        domain: *mut u16,
        domain_length: u32,
        password: *mut u16,
        password_length: u32,
        flags: u32,
    }

    /// `SEC_WINNT_AUTH_IDENTITY_UNICODE`.
    const AUTH_IDENTITY_UNICODE: u32 = 2;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn code_of(status: HRESULT) -> ErrorCode {
        if [
            SEC_E_LOGON_DENIED,
            SEC_E_NO_CREDENTIALS,
            SEC_E_UNKNOWN_CREDENTIALS,
            SEC_E_INVALID_TOKEN,
            SEC_E_WRONG_PRINCIPAL,
        ]
        .contains(&status)
        {
            ErrorCode::AuthFailed
        } else {
            ErrorCode::Internal
        }
    }

    /// One security context (one HTTP connection).
    pub(super) struct Session {
        credentials: SecHandle,
        context: Option<SecHandle>,
        target: Vec<u16>,
        max_token: usize,
        done: bool,
        package: SspiPackage,
    }

    // SAFETY: the handles are plain values owned by this session; SSPI
    // contexts may be used from any thread as long as calls do not overlap,
    // which `&mut self` guarantees.
    unsafe impl Send for Session {}

    impl std::fmt::Debug for Session {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SspiSession")
                .field("package", &self.package)
                .field("started", &self.context.is_some())
                .field("done", &self.done)
                .finish()
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // SAFETY: both handles were created by SSPI for this session
            // and are released exactly once here.
            unsafe {
                if let Some(context) = self.context.take() {
                    let _ = DeleteSecurityContext(&context);
                }
                let _ = FreeCredentialsHandle(&self.credentials);
            }
        }
    }

    pub(super) fn start(
        package: SspiPackage,
        credentials: &SspiCredentials,
        spn: &str,
    ) -> Result<Box<dyn NegotiateSession>, EngineError> {
        let info = package_info(package)?;
        let package_name = HSTRING::from(package.sspi_name());
        let mut handle = SecHandle::default();
        let mut expiry = 0i64;
        let acquired = match credentials {
            SspiCredentials::CurrentUser => {
                // SAFETY: every pointer is valid for the call; `handle`
                // receives the credentials handle.
                unsafe {
                    AcquireCredentialsHandleW(
                        PCWSTR::null(),
                        &package_name,
                        SECPKG_CRED_OUTBOUND,
                        None,
                        None,
                        None,
                        None,
                        &mut handle,
                        Some(&mut expiry),
                    )
                }
            }
            SspiCredentials::Explicit { username, password } => {
                let (user, domain) = split_username(username);
                let mut user = wide(user);
                let mut domain = wide(domain);
                let mut secret = wide(password.expose());
                let identity = AuthIdentity {
                    user: user.as_mut_ptr(),
                    user_length: u32::try_from(user.len()).unwrap_or(0),
                    domain: domain.as_mut_ptr(),
                    domain_length: u32::try_from(domain.len()).unwrap_or(0),
                    password: secret.as_mut_ptr(),
                    password_length: u32::try_from(secret.len()).unwrap_or(0),
                    flags: AUTH_IDENTITY_UNICODE,
                };
                // SAFETY: `identity` and the buffers it points to live
                // until the call returns (SSPI copies them).
                let result = unsafe {
                    AcquireCredentialsHandleW(
                        PCWSTR::null(),
                        &package_name,
                        SECPKG_CRED_OUTBOUND,
                        None,
                        Some(std::ptr::from_ref(&identity).cast::<c_void>()),
                        None,
                        None,
                        &mut handle,
                        Some(&mut expiry),
                    )
                };
                // Do not leave the password in memory longer than needed.
                secret.iter_mut().for_each(|c| *c = 0);
                result
            }
        };
        acquired.map_err(|e| {
            sspi_error(
                code_of(e.code()),
                format!("AcquireCredentialsHandle ({}): {e}", package.sspi_name()),
            )
        })?;
        let mut target = wide(spn);
        target.push(0);
        Ok(Box::new(Session {
            credentials: handle,
            context: None,
            target,
            max_token: usize::try_from(info.max_token).unwrap_or(65_535).max(1024),
            done: false,
            package,
        }))
    }

    impl NegotiateSession for Session {
        fn step(&mut self, server_token: Option<&[u8]>) -> Result<NegotiateStep, EngineError> {
            if self.done && server_token.is_none() {
                return Ok(NegotiateStep::Done(None));
            }
            if self.context.is_some() && server_token.is_none() {
                return Err(sspi_error(
                    ErrorCode::AuthFailed,
                    "the server sent no token to continue the handshake".into(),
                ));
            }
            let mut input_bytes = server_token.map(<[u8]>::to_vec).unwrap_or_default();
            let mut input_buffer = SecBuffer {
                cbBuffer: u32::try_from(input_bytes.len()).unwrap_or(0),
                BufferType: SECBUFFER_TOKEN,
                pvBuffer: input_bytes.as_mut_ptr().cast(),
            };
            let input_desc = SecBufferDesc {
                ulVersion: SECBUFFER_VERSION,
                cBuffers: 1,
                pBuffers: &mut input_buffer,
            };
            let mut output_bytes = vec![0u8; self.max_token];
            let mut output_buffer = SecBuffer {
                cbBuffer: u32::try_from(output_bytes.len()).unwrap_or(0),
                BufferType: SECBUFFER_TOKEN,
                pvBuffer: output_bytes.as_mut_ptr().cast(),
            };
            let mut output_desc = SecBufferDesc {
                ulVersion: SECBUFFER_VERSION,
                cBuffers: 1,
                pBuffers: &mut output_buffer,
            };
            let flags: ISC_REQ_FLAGS = ISC_REQ_CONNECTION | ISC_REQ_MUTUAL_AUTH;
            let mut new_context = self.context.unwrap_or_default();
            let mut attributes = 0u32;
            let mut expiry = 0i64;
            // SAFETY: the handles are valid (or absent on the first call),
            // `target` is NUL-terminated, the buffers outlive the call and
            // the output buffer has room for the package's largest token.
            let status = unsafe {
                InitializeSecurityContextW(
                    Some(&self.credentials),
                    self.context.as_ref().map(std::ptr::from_ref),
                    Some(self.target.as_ptr()),
                    flags,
                    0,
                    SECURITY_NATIVE_DREP,
                    server_token
                        .is_some()
                        .then_some(std::ptr::from_ref(&input_desc)),
                    0,
                    Some(&mut new_context),
                    Some(&mut output_desc),
                    &mut attributes,
                    Some(&mut expiry),
                )
            };
            // The first successful call creates the context; later calls
            // update it in place.
            if self.context.is_none() && status.is_ok() {
                self.context = Some(new_context);
            }
            let complete = |desc: &SecBufferDesc, context: &SecHandle| {
                // SAFETY: `context` is the live context and `desc` its
                // output token.
                unsafe { CompleteAuthToken(context, desc) }
                    .map_err(|e| sspi_error(ErrorCode::Internal, format!("CompleteAuthToken: {e}")))
            };
            let token_len = usize::try_from(output_buffer.cbBuffer)
                .unwrap_or(0)
                .min(output_bytes.len());
            output_bytes.truncate(token_len);
            if status == SEC_E_OK {
                self.done = true;
                Ok(NegotiateStep::Done(
                    (!output_bytes.is_empty()).then_some(output_bytes),
                ))
            } else if status == SEC_I_CONTINUE_NEEDED {
                Ok(NegotiateStep::Continue(output_bytes))
            } else if status == SEC_I_COMPLETE_NEEDED || status == SEC_I_COMPLETE_AND_CONTINUE {
                let context = self
                    .context
                    .ok_or_else(|| sspi_error(ErrorCode::Internal, "no security context".into()))?;
                complete(&output_desc, &context)?;
                if status == SEC_I_COMPLETE_NEEDED {
                    self.done = true;
                    Ok(NegotiateStep::Done(
                        (!output_bytes.is_empty()).then_some(output_bytes),
                    ))
                } else {
                    Ok(NegotiateStep::Continue(output_bytes))
                }
            } else {
                let status = if status.is_ok() {
                    SEC_E_INTERNAL_ERROR
                } else {
                    status
                };
                Err(sspi_error(
                    code_of(status),
                    format!(
                        "InitializeSecurityContext ({}): {}",
                        self.package.sspi_name(),
                        windows::core::Error::from_hresult(status)
                    ),
                ))
            }
        }
    }

    /// The server side of a handshake (`AcceptSecurityContext` with the
    /// signed-in user's inbound credentials), so tests can run the real
    /// client against a real SSPI server in-process: no network.
    #[cfg(test)]
    pub(crate) mod server {
        use windows::Win32::Security::Authentication::Identity::{
            ASC_REQ_CONNECTION, AcceptSecurityContext, SECPKG_CRED_INBOUND,
        };

        use super::*;

        pub(crate) struct ServerContext {
            credentials: SecHandle,
            context: Option<SecHandle>,
            max_token: usize,
        }

        impl Drop for ServerContext {
            fn drop(&mut self) {
                // SAFETY: handles from SSPI, released once.
                unsafe {
                    if let Some(context) = self.context.take() {
                        let _ = DeleteSecurityContext(&context);
                    }
                    let _ = FreeCredentialsHandle(&self.credentials);
                }
            }
        }

        // SAFETY: plain handles used from one thread at a time (`&mut`).
        unsafe impl Send for ServerContext {}

        impl ServerContext {
            pub(crate) fn new(package: SspiPackage) -> Result<Self, String> {
                let info = package_info(package).map_err(|e| e.message)?;
                let name = HSTRING::from(package.sspi_name());
                let mut credentials = SecHandle::default();
                // SAFETY: valid pointers for the call.
                unsafe {
                    AcquireCredentialsHandleW(
                        PCWSTR::null(),
                        &name,
                        SECPKG_CRED_INBOUND,
                        None,
                        None,
                        None,
                        None,
                        &mut credentials,
                        None,
                    )
                }
                .map_err(|e| e.to_string())?;
                Ok(Self {
                    credentials,
                    context: None,
                    max_token: usize::try_from(info.max_token).unwrap_or(65_535).max(1024),
                })
            }

            /// Accepts a client token: `Ok((done, reply token))`.
            pub(crate) fn accept(&mut self, token: &[u8]) -> Result<(bool, Vec<u8>), String> {
                let mut input = token.to_vec();
                let mut input_buffer = SecBuffer {
                    cbBuffer: u32::try_from(input.len()).unwrap_or(0),
                    BufferType: SECBUFFER_TOKEN,
                    pvBuffer: input.as_mut_ptr().cast(),
                };
                let input_desc = SecBufferDesc {
                    ulVersion: SECBUFFER_VERSION,
                    cBuffers: 1,
                    pBuffers: &mut input_buffer,
                };
                let mut output = vec![0u8; self.max_token];
                let mut output_buffer = SecBuffer {
                    cbBuffer: u32::try_from(output.len()).unwrap_or(0),
                    BufferType: SECBUFFER_TOKEN,
                    pvBuffer: output.as_mut_ptr().cast(),
                };
                let mut output_desc = SecBufferDesc {
                    ulVersion: SECBUFFER_VERSION,
                    cBuffers: 1,
                    pBuffers: &mut output_buffer,
                };
                let mut new_context = self.context.unwrap_or_default();
                let mut attributes = 0u32;
                // SAFETY: valid handles and buffers that outlive the call.
                let status = unsafe {
                    AcceptSecurityContext(
                        Some(&self.credentials),
                        self.context.as_ref().map(std::ptr::from_ref),
                        Some(&input_desc),
                        ASC_REQ_CONNECTION,
                        SECURITY_NATIVE_DREP,
                        Some(&mut new_context),
                        Some(&mut output_desc),
                        &mut attributes,
                        None,
                    )
                };
                if self.context.is_none() && status.is_ok() {
                    self.context = Some(new_context);
                }
                let len = usize::try_from(output_buffer.cbBuffer)
                    .unwrap_or(0)
                    .min(output.len());
                output.truncate(len);
                if status == SEC_E_OK {
                    Ok((true, output))
                } else if status == SEC_I_CONTINUE_NEEDED {
                    Ok((false, output))
                } else {
                    Err(windows::core::Error::from_hresult(status).to_string())
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{
        EngineError, ErrorCode, NegotiateSession, PackageInfo, SspiCredentials, SspiPackage,
    };

    fn unsupported(package: SspiPackage) -> EngineError {
        EngineError::permanent(
            ErrorCode::Unsupported,
            format!("Windows sign-in ({}) needs Windows", package.sspi_name()),
        )
    }

    pub(super) fn package_info(package: SspiPackage) -> Result<PackageInfo, EngineError> {
        Err(unsupported(package))
    }

    pub(super) fn start(
        package: SspiPackage,
        _credentials: &SspiCredentials,
        _spn: &str,
    ) -> Result<Box<dyn NegotiateSession>, EngineError> {
        Err(unsupported(package))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::SecretRef;

    #[test]
    fn usernames_split_for_sspi() {
        assert_eq!(split_username("CORP\\ada"), ("ada", "CORP"));
        assert_eq!(split_username("ada@corp.example"), ("ada@corp.example", ""));
        assert_eq!(split_username("ada"), ("ada", ""));
    }

    #[test]
    fn sign_in_from_the_account_node() {
        let current = AuthConfig::Windows {
            username: None,
            secret: None,
            package: None,
        };
        let auth = SspiNegotiate::from_auth(&current, None).unwrap();
        assert_eq!(auth.credentials(), &SspiCredentials::CurrentUser);
        assert_eq!(auth.package(), SspiPackage::Negotiate);
        let login = auth.login();
        assert!(
            login.ntlm.is_some(),
            "Negotiate falls back to NTLM-only servers"
        );

        let explicit = AuthConfig::Windows {
            username: Some("CORP\\ada".into()),
            secret: SecretRef::parse("keychain:nail/work/password").ok(),
            package: Some(SspiPackage::Kerberos),
        };
        let error = SspiNegotiate::from_auth(&explicit, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::AuthFailed);
        assert_eq!(error.localized.unwrap().id, texts::SIGNIN_NEEDS_PASSWORD);
        let auth = SspiNegotiate::from_auth(&explicit, Some(Password::new("hunter2"))).unwrap();
        assert!(!format!("{auth:?}").contains("hunter2"));
        assert!(auth.login().ntlm.is_none(), "Kerberos never falls back");

        let error = SspiNegotiate::from_auth(&AuthConfig::None, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }

    #[cfg(windows)]
    #[test]
    fn sspi_knows_negotiate_kerberos_and_ntlm() {
        for package in [
            SspiPackage::Negotiate,
            SspiPackage::Kerberos,
            SspiPackage::Ntlm,
        ] {
            let info = package_info(package).unwrap();
            assert!(
                info.name.eq_ignore_ascii_case(package.sspi_name()),
                "{info:?}"
            );
            assert!(info.max_token > 0, "{info:?}");
        }
    }

    /// NTLM's first token is a NEGOTIATE_MESSAGE: no network, no server,
    /// the signed-in user's credentials handle only.
    #[cfg(windows)]
    #[test]
    fn ntlm_first_token_is_a_negotiate_message() {
        use nail_api::exchange::engine::NegotiateStep;
        let auth = SspiNegotiate::current_user(SspiPackage::Ntlm);
        let mut session = auth.start("HTTP/localhost").unwrap();
        let NegotiateStep::Continue(token) = session.step(None).unwrap() else {
            panic!("NTLM needs a second round");
        };
        assert_eq!(&token[..8], b"NTLMSSP\0");
        assert_eq!(token[8], 1, "message type 1");
        // Continuing without the server's challenge is an error, not a crash.
        assert_eq!(session.step(None).unwrap_err().code, ErrorCode::AuthFailed);
        // A garbage challenge is refused by SSPI.
        let mut session = auth.start("HTTP/localhost").unwrap();
        session.step(None).unwrap();
        assert!(session.step(Some(b"not a challenge")).is_err());
    }

    /// Explicit credentials work the same (the password is only checked by
    /// the server, so the first token is produced offline).
    #[cfg(windows)]
    #[test]
    fn explicit_credentials_produce_a_token() {
        use nail_api::exchange::engine::NegotiateStep;
        let auth = SspiNegotiate::with_password(
            "NAILTEST\\nobody",
            Password::new("not-a-real-password"),
            SspiPackage::Ntlm,
        );
        let mut session = auth.start("HTTP/localhost").unwrap();
        assert!(
            matches!(session.step(None).unwrap(), NegotiateStep::Continue(t) if t.starts_with(b"NTLMSSP\0"))
        );
    }

    /// The real client against a real SSPI server in-process, through the
    /// HTTP 401 loop (no network): what an EWS request with
    /// `auth "windows"` does against IIS.
    #[cfg(windows)]
    mod handshake {
        use std::sync::Mutex;

        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;
        use nail_api::engine::BoxFuture;
        use url::Url;

        use super::super::imp::server::ServerContext;
        use super::*;
        use crate::http::{ExchangeHttp, HttpError, HttpRequest, HttpResponse};
        use crate::negotiate::{HttpLogin, SignInError, send_signed_in};

        struct SspiServer {
            package: SspiPackage,
            scheme: &'static str,
            context: Mutex<Option<ServerContext>>,
            rounds: Mutex<u32>,
        }

        impl fmt::Debug for SspiServer {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("SspiServer")
            }
        }

        impl SspiServer {
            fn new(package: SspiPackage, scheme: &'static str) -> Self {
                Self {
                    package,
                    scheme,
                    context: Mutex::new(None),
                    rounds: Mutex::new(0),
                }
            }

            fn answer(&self, request: &HttpRequest) -> HttpResponse {
                *self.rounds.lock().unwrap() += 1;
                let token = request
                    .headers
                    .iter()
                    .find(|(n, _)| n == "Authorization")
                    .and_then(|(_, v)| v.split_once(' '))
                    .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(self.scheme))
                    .and_then(|(_, t)| BASE64.decode(t).ok());
                let Some(token) = token else {
                    return HttpResponse::new(401, "").header("WWW-Authenticate", self.scheme);
                };
                let mut context = self.context.lock().unwrap();
                let server =
                    context.get_or_insert_with(|| ServerContext::new(self.package).unwrap());
                match server.accept(&token) {
                    Ok((true, reply)) => {
                        let response = HttpResponse::new(200, "signed in");
                        if reply.is_empty() {
                            response
                        } else {
                            response.header(
                                "WWW-Authenticate",
                                format!("{} {}", self.scheme, BASE64.encode(reply)),
                            )
                        }
                    }
                    Ok((false, reply)) => HttpResponse::new(401, "").header(
                        "WWW-Authenticate",
                        format!("{} {}", self.scheme, BASE64.encode(reply)),
                    ),
                    Err(_) => HttpResponse::new(401, "").header("WWW-Authenticate", self.scheme),
                }
            }
        }

        impl ExchangeHttp for SspiServer {
            fn send<'a>(
                &'a self,
                request: &'a HttpRequest,
            ) -> BoxFuture<'a, Result<HttpResponse, HttpError>> {
                let answer = self.answer(request);
                Box::pin(async move { Ok(answer) })
            }
        }

        fn url() -> HttpRequest {
            HttpRequest::get(Url::parse("https://localhost/EWS/Exchange.asmx").unwrap())
        }

        #[tokio::test]
        async fn real_ntlm_handshake_with_the_signed_in_user() {
            let server = SspiServer::new(SspiPackage::Ntlm, "NTLM");
            let login = HttpLogin::Windows(SspiNegotiate::current_user(SspiPackage::Ntlm).login());
            let answer = send_signed_in(&server, url(), &login).await.unwrap();
            assert_eq!(answer.status, 200);
            assert_eq!(
                *server.rounds.lock().unwrap(),
                3,
                "anonymous, negotiate, authenticate"
            );
        }

        #[tokio::test]
        async fn real_negotiate_handshake_with_the_signed_in_user() {
            let server = SspiServer::new(SspiPackage::Negotiate, "Negotiate");
            let login =
                HttpLogin::Windows(SspiNegotiate::current_user(SspiPackage::Negotiate).login());
            let answer = send_signed_in(&server, url(), &login).await.unwrap();
            assert_eq!(answer.status, 200);
        }

        #[tokio::test]
        async fn wrong_credentials_are_rejected() {
            let server = SspiServer::new(SspiPackage::Ntlm, "NTLM");
            let auth = SspiNegotiate::with_password(
                "NAILTEST-NO-SUCH-DOMAIN\\nobody",
                Password::new("certainly-wrong"),
                SspiPackage::Ntlm,
            );
            let error = send_signed_in(&server, url(), &HttpLogin::Windows(auth.login()))
                .await
                .unwrap_err();
            assert_eq!(error, SignInError::Rejected);
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn sspi_is_windows_only() {
        assert_eq!(
            package_info(SspiPackage::Negotiate).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            SspiNegotiate::current_user(SspiPackage::Ntlm)
                .start("HTTP/x")
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
}
