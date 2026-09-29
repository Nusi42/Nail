//! Certificate chain building and validation.
//!
//! Nail builds the path itself (leaf → intermediates → trust anchor) from
//! the certificates of the message, the keyring and the system, and checks
//! each link: signatures, validity at a given time, CA constraints (basic
//! constraints, key usage, path length), unknown critical extensions, and
//! the leaf's key usage / extended key usage for the purpose (RFC 5280
//! section 6 in the subset S/MIME needs; RFC 8550 section 4). Trust
//! anchors come from a [`TrustAnchors`] implementation: the Windows trust
//! store in the app (`windows::WindowsTrustAnchors`), a fixed list in tests.
//!
//! Not evaluated in M2 (ADR 0031): revocation (always reported as
//! "not checked"), name constraints and policy constraints (a certificate
//! with such a critical extension makes the chain `invalid`, never
//! silently valid), and network retrieval of missing intermediates (AIA).

use nail_api::security::{CertificateChain, ChainState, RevocationState};
use x509_cert::name::Name;

use crate::cert::{Cert, Purpose};
use crate::oids;

/// The longest chain Nail builds (leaf and anchor included).
pub const MAX_CHAIN_LEN: usize = 8;

/// Where trusted roots (and system intermediates) come from.
pub trait TrustAnchors: Send + Sync {
    /// Trusted certificates whose subject is `subject`.
    fn anchors_for(&self, subject: &Name) -> Vec<Cert>;

    /// Intermediate CA certificates the system knows with this subject
    /// (the Windows "CA" store); none by default.
    fn intermediates_for(&self, _subject: &Name) -> Vec<Cert> {
        Vec::new()
    }

    /// Whether the system explicitly distrusts `cert` (the Windows
    /// "Disallowed" store); a chain through it is `invalid`.
    fn is_distrusted(&self, _cert: &Cert) -> bool {
        false
    }

    /// Whether `cert` itself is trusted directly.
    fn is_anchor(&self, cert: &Cert) -> bool {
        self.anchors_for(&cert.inner().tbs_certificate.subject)
            .iter()
            .any(|a| a == cert)
    }
}

/// A fixed set of trust anchors (tests, or an explicit list).
#[derive(Debug, Clone, Default)]
pub struct FixedTrustAnchors {
    roots: Vec<Cert>,
    intermediates: Vec<Cert>,
    distrusted: Vec<Cert>,
}

impl FixedTrustAnchors {
    /// Trusts exactly these certificates.
    pub fn new(roots: Vec<Cert>) -> Self {
        Self {
            roots,
            intermediates: Vec::new(),
            distrusted: Vec::new(),
        }
    }

    /// Adds untrusted intermediates the "system" knows (like the Windows
    /// "CA" store).
    pub fn with_intermediates(mut self, intermediates: Vec<Cert>) -> Self {
        self.intermediates = intermediates;
        self
    }

    /// Explicitly distrusts these certificates.
    pub fn with_distrusted(mut self, distrusted: Vec<Cert>) -> Self {
        self.distrusted = distrusted;
        self
    }
}

impl TrustAnchors for FixedTrustAnchors {
    fn anchors_for(&self, subject: &Name) -> Vec<Cert> {
        self.roots
            .iter()
            .filter(|c| &c.inner().tbs_certificate.subject == subject)
            .cloned()
            .collect()
    }

    fn intermediates_for(&self, subject: &Name) -> Vec<Cert> {
        self.intermediates
            .iter()
            .filter(|c| &c.inner().tbs_certificate.subject == subject)
            .cloned()
            .collect()
    }

    fn is_distrusted(&self, cert: &Cert) -> bool {
        self.distrusted.contains(cert)
    }
}

/// Outcome of a validation: the protocol summary and the certificates of
/// the path (leaf first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    /// The summary for the UI.
    pub chain: CertificateChain,
    /// The path that was built, leaf first.
    pub path: Vec<Cert>,
}

impl Validation {
    /// The overall state.
    pub fn state(&self) -> ChainState {
        self.chain.state
    }
}

enum End {
    Anchored,
    SelfSigned,
    Incomplete,
    BadSignature,
}

/// Builds and checks the chain of `leaf` for `purpose` at `at` (Unix
/// seconds). `pool` holds candidate intermediates (message and keyring
/// certificates; the leaf itself may be among them).
pub fn validate(
    leaf: &Cert,
    pool: &[Cert],
    anchors: &dyn TrustAnchors,
    purpose: Purpose,
    at: i64,
) -> Validation {
    let mut path = vec![leaf.clone()];
    let mut anchor_index = None;
    let end = loop {
        let current = path.last().cloned().unwrap_or_else(|| leaf.clone());
        if anchors.is_anchor(&current) {
            anchor_index = Some(path.len() - 1);
            break End::Anchored;
        }
        if path.len() >= MAX_CHAIN_LEN {
            break End::Incomplete;
        }
        let issuer_name = &current.inner().tbs_certificate.issuer;
        let mut saw_candidate = false;
        if let Some(anchor) = anchors
            .anchors_for(issuer_name)
            .into_iter()
            .inspect(|_| saw_candidate = true)
            .find(|a| current.verify_signed_by(a).is_ok())
        {
            path.push(anchor);
            anchor_index = Some(path.len() - 1);
            break End::Anchored;
        }
        let mut candidates: Vec<Cert> = pool
            .iter()
            .filter(|c| &c.inner().tbs_certificate.subject == issuer_name)
            .cloned()
            .chain(anchors.intermediates_for(issuer_name))
            .filter(|c| !path.contains(c))
            .collect();
        // Prefer the issuer the authority key identifier names.
        if let Some(aki) = current.authority_key_id() {
            candidates.sort_by_key(|c| !c.matches_key_id(&aki));
        }
        saw_candidate |= !candidates.is_empty();
        if let Some(next) = candidates
            .into_iter()
            .find(|c| current.verify_signed_by(c).is_ok())
        {
            path.push(next);
            continue;
        }
        if current.is_self_issued() {
            break if current.verify_signed_by(&current).is_ok() {
                End::SelfSigned
            } else {
                End::BadSignature
            };
        }
        break if saw_candidate {
            End::BadSignature
        } else {
            End::Incomplete
        };
    };

    let state = if path.iter().any(|c| anchors.is_distrusted(c)) {
        ChainState::Invalid
    } else {
        judge(&path, anchor_index, &end, purpose, at)
    };
    Validation {
        chain: CertificateChain {
            state,
            revocation: RevocationState::NotChecked,
            certificates: path.iter().map(Cert::summary).collect(),
        },
        path,
    }
}

fn judge(path: &[Cert], anchor: Option<usize>, end: &End, purpose: Purpose, at: i64) -> ChainState {
    if matches!(end, End::BadSignature) {
        return ChainState::Invalid;
    }
    if path
        .iter()
        .any(|c| !c.unhandled_critical_extensions().is_empty())
    {
        return ChainState::Invalid;
    }
    // CA certificates between the leaf and the anchor (the anchor itself
    // is trusted as configured, like old v1 roots without extensions).
    let mut wrong_usage = false;
    for (i, ca) in path.iter().enumerate().skip(1) {
        if Some(i) == anchor {
            break;
        }
        let Some((true, path_len)) = ca.basic_constraints() else {
            return ChainState::Invalid;
        };
        if ca.key_usage().is_some_and(|ku| !ku.key_cert_sign()) {
            return ChainState::Invalid;
        }
        // Non-self-issued intermediates below this CA.
        let below = path[1..i].iter().filter(|c| !c.is_self_issued()).count();
        if path_len.is_some_and(|max| below > usize::from(max)) {
            return ChainState::Invalid;
        }
        // Extended key usage on a CA restricts what it may issue for
        // (the Windows chain engine's rule).
        if ca.extended_key_usage().is_some_and(|ekus| {
            !ekus.contains(&oids::KP_EMAIL_PROTECTION)
                && !ekus.contains(&oids::ANY_EXTENDED_KEY_USAGE)
        }) {
            wrong_usage = true;
        }
    }
    match end {
        End::SelfSigned => return ChainState::UntrustedRoot,
        End::Incomplete => return ChainState::Incomplete,
        End::Anchored | End::BadSignature => {}
    }
    if path.iter().any(|c| !c.valid_at(at)) {
        return ChainState::Expired;
    }
    if wrong_usage || !path[0].usable_for(purpose) {
        return ChainState::WrongUsage;
    }
    ChainState::Valid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{AT, fixture_cert, test_anchors};

    fn check(leaf: &str, purpose: Purpose) -> Validation {
        let pool = vec![fixture_cert("intermediate-ca.crt")];
        validate(&fixture_cert(leaf), &pool, &test_anchors(), purpose, AT)
    }

    #[test]
    fn valid_chain_to_the_test_root() {
        let v = check("alice.crt", Purpose::Sign);
        assert_eq!(v.state(), ChainState::Valid);
        assert_eq!(v.chain.revocation, RevocationState::NotChecked);
        let subjects: Vec<_> = v
            .chain
            .certificates
            .iter()
            .map(|c| c.subject.clone())
            .collect();
        assert_eq!(
            subjects,
            [
                "CN=Alice Example,O=Nail Test",
                "CN=Nail Test Email CA,O=Nail Test",
                "CN=Nail Test Root CA,O=Nail Test"
            ]
        );
        assert_eq!(
            check("bob.crt", Purpose::Encrypt).state(),
            ChainState::Valid
        );
    }

    #[test]
    fn missing_intermediate_is_incomplete() {
        let v = validate(
            &fixture_cert("alice.crt"),
            &[],
            &test_anchors(),
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::Incomplete);
        assert_eq!(v.path.len(), 1);
    }

    #[test]
    fn system_intermediates_complete_the_chain() {
        let anchors = test_anchors().with_intermediates(vec![fixture_cert("intermediate-ca.crt")]);
        let v = validate(&fixture_cert("alice.crt"), &[], &anchors, Purpose::Sign, AT);
        assert_eq!(v.state(), ChainState::Valid);
    }

    #[test]
    fn expired_and_not_yet_valid() {
        assert_eq!(
            check("carol.crt", Purpose::Sign).state(),
            ChainState::Expired
        );
        let pool = vec![fixture_cert("intermediate-ca.crt")];
        let early = validate(
            &fixture_cert("alice.crt"),
            &pool,
            &test_anchors(),
            Purpose::Sign,
            1_000_000_000, // 2001, before the 2020 validity start
        );
        assert_eq!(early.state(), ChainState::Expired);
    }

    #[test]
    fn usage_is_checked_per_purpose() {
        assert_eq!(
            check("dave.crt", Purpose::Sign).state(),
            ChainState::WrongUsage
        );
        assert_eq!(
            check("erin.crt", Purpose::Sign).state(),
            ChainState::WrongUsage
        );
        assert_eq!(
            check("erin.crt", Purpose::Encrypt).state(),
            ChainState::Valid
        );
        assert_eq!(check("erin.crt", Purpose::Any).state(), ChainState::Valid);
    }

    #[test]
    fn untrusted_root() {
        let evil = fixture_cert("evil-root-ca.crt");
        let v = validate(
            &fixture_cert("mallory.crt"),
            std::slice::from_ref(&evil),
            &test_anchors(),
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::UntrustedRoot);
        assert_eq!(v.path.len(), 2);
        // Without the root in hand it is only incomplete.
        let v = validate(
            &fixture_cert("mallory.crt"),
            &[],
            &test_anchors(),
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::Incomplete);
        // Trusting the evil root makes it valid: trust is the anchors' job.
        let v = validate(
            &fixture_cert("mallory.crt"),
            &[],
            &FixedTrustAnchors::new(vec![evil]),
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::Valid);
    }

    #[test]
    fn a_directly_trusted_leaf_is_valid() {
        let alice = fixture_cert("alice.crt");
        let v = validate(
            &alice,
            &[],
            &FixedTrustAnchors::new(vec![alice.clone()]),
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::Valid);
        assert_eq!(v.path.len(), 1);
    }

    #[test]
    fn a_forged_link_is_invalid() {
        // An "intermediate" with the right name but another key: the
        // evil root re-labelled cannot have signed alice.
        let alice = fixture_cert("alice.crt");
        let mut der = alice.der().to_vec();
        let pos = der.windows(5).position(|w| w == b"Alice").unwrap();
        der[pos] = b'M';
        let forged = Cert::from_der(&der).unwrap();
        let pool = vec![fixture_cert("intermediate-ca.crt")];
        let v = validate(&forged, &pool, &test_anchors(), Purpose::Sign, AT);
        assert_eq!(v.state(), ChainState::Invalid);
    }

    fn check_with(leaf: &str, extra: &str) -> Validation {
        let pool = vec![fixture_cert("intermediate-ca.crt"), fixture_cert(extra)];
        validate(
            &fixture_cert(leaf),
            &pool,
            &test_anchors(),
            Purpose::Sign,
            AT,
        )
    }

    #[test]
    fn a_leaf_cannot_act_as_a_ca() {
        // heidi is issued by "Not A CA", an end-entity certificate.
        let v = check_with("heidi.crt", "not-ca.crt");
        assert_eq!(v.path.len(), 4);
        assert_eq!(v.state(), ChainState::Invalid);
    }

    #[test]
    fn path_length_constraint_is_enforced() {
        // grace → sub CA → intermediate (pathlen 0) → root.
        let v = check_with("grace.crt", "sub-ca.crt");
        assert_eq!(v.path.len(), 4);
        assert_eq!(v.state(), ChainState::Invalid);
    }

    #[test]
    fn unknown_critical_extensions_are_not_accepted() {
        let ivan = fixture_cert("ivan.crt");
        assert_eq!(ivan.unhandled_critical_extensions().len(), 1);
        assert_eq!(
            check("ivan.crt", Purpose::Sign).state(),
            ChainState::Invalid
        );
    }

    #[test]
    fn a_distrusted_certificate_invalidates_the_chain() {
        let anchors = test_anchors().with_distrusted(vec![fixture_cert("intermediate-ca.crt")]);
        let pool = vec![fixture_cert("intermediate-ca.crt")];
        let v = validate(
            &fixture_cert("alice.crt"),
            &pool,
            &anchors,
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::Invalid);
    }

    #[test]
    fn chains_stop_at_the_length_limit() {
        // A pool that only offers the leaf itself makes no progress.
        let alice = fixture_cert("alice.crt");
        let v = validate(
            &alice,
            std::slice::from_ref(&alice),
            &test_anchors(),
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::Incomplete);
        assert!(v.path.len() <= MAX_CHAIN_LEN);
    }
}
