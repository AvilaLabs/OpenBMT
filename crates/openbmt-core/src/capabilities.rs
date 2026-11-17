//! Local authorization policy for scoped merchant credentials.
//!
//! This module evaluates metadata supplied by an authentication layer after
//! that layer has verified the credential. It does not mint credentials,
//! authenticate identities, persist revocations, or perform operations. A
//! caller must pass the current revocation state and enforce this decision at
//! each operation.

use std::collections::BTreeSet;

/// Caller-selected bound. This module imposes no default renewal interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationPolicy {
    pub max_lifetime_secs: u64,
}

/// Supported local merchant actions. Privileged wallet operations are
/// represented so callers can explicitly request and deny them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    InvoiceCreate,
    InvoiceStatusRead,
    ReportRead,
    RefundRequest,
    Spend,
    Sign,
    ChangeDestination,
    /// Any identifier not recognized by this version of the policy.
    Unknown,
}

impl Capability {
    /// Parse an exact capability identifier. New identifiers remain denied
    /// until this policy explicitly recognizes them.
    pub fn from_identifier(identifier: &str) -> Self {
        match identifier {
            "invoice:create" => Self::InvoiceCreate,
            "invoice:status:read" => Self::InvoiceStatusRead,
            "report:read" => Self::ReportRead,
            "refund:request" => Self::RefundRequest,
            "wallet:spend" => Self::Spend,
            "wallet:sign" => Self::Sign,
            "payment:destination:change" => Self::ChangeDestination,
            _ => Self::Unknown,
        }
    }

    fn identifier(self) -> Option<&'static str> {
        match self {
            Self::InvoiceCreate => Some("invoice:create"),
            Self::InvoiceStatusRead => Some("invoice:status:read"),
            Self::ReportRead => Some("report:read"),
            Self::RefundRequest => Some("refund:request"),
            Self::Spend => Some("wallet:spend"),
            Self::Sign => Some("wallet:sign"),
            Self::ChangeDestination => Some("payment:destination:change"),
            Self::Unknown => None,
        }
    }

    fn is_privileged(self) -> bool {
        matches!(
            self,
            Self::Spend | Self::Sign | Self::ChangeDestination | Self::Unknown
        )
    }
}

/// Store and optional register to which the verified credential is bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub store_id: String,
    pub register_id: Option<String>,
}

/// Verified credential fields required by the policy check.
///
/// `capabilities` are exact, versioned identifiers. Unknown identifiers never
/// grant authority. `credential_id` is used only to consult the caller's
/// revocation set; it is not itself a secret or authentication mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCredential {
    pub credential_id: String,
    pub scope: Scope,
    pub capabilities: BTreeSet<String>,
    pub issued_at: u64,
    pub expires_at: u64,
}

/// Requested merchant resource boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestScope {
    pub store_id: String,
    pub register_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    InvalidPolicy,
    InvalidMetadata,
    Revoked,
    NotYetValid,
    Expired,
    InvalidLifetime,
    WrongStore,
    WrongRegister,
    MissingCapability,
    PrivilegedOperation,
    UnknownCapability,
}

/// Authorize one operation using already verified credential metadata.
/// Expiry is exclusive: a credential is invalid at `now == expires_at`.
pub fn authorize(
    credential: &VerifiedCredential,
    revoked_credentials: &BTreeSet<String>,
    requested_scope: &RequestScope,
    capability: Capability,
    now: u64,
    policy: AuthorizationPolicy,
) -> Result<(), Denial> {
    if policy.max_lifetime_secs == 0 {
        return Err(Denial::InvalidPolicy);
    }
    if credential.credential_id.is_empty()
        || credential.credential_id.len() > 256
        || credential.scope.store_id.is_empty()
        || credential.scope.store_id.len() > 256
        || credential
            .scope
            .register_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
        || requested_scope.store_id.is_empty()
        || requested_scope.store_id.len() > 256
        || requested_scope
            .register_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
    {
        return Err(Denial::InvalidMetadata);
    }
    if capability == Capability::Unknown {
        return Err(Denial::UnknownCapability);
    }
    if capability.is_privileged() {
        return Err(Denial::PrivilegedOperation);
    }
    if revoked_credentials.contains(&credential.credential_id) {
        return Err(Denial::Revoked);
    }
    if credential.issued_at > now {
        return Err(Denial::NotYetValid);
    }
    if credential.expires_at <= now {
        return Err(Denial::Expired);
    }
    if credential.expires_at <= credential.issued_at
        || credential.expires_at - credential.issued_at > policy.max_lifetime_secs
    {
        return Err(Denial::InvalidLifetime);
    }
    if credential.scope.store_id != requested_scope.store_id {
        return Err(Denial::WrongStore);
    }
    if credential.scope.register_id != requested_scope.register_id {
        return Err(Denial::WrongRegister);
    }
    let identifier = capability
        .identifier()
        .expect("known non-unknown capability");
    if !credential.capabilities.contains(identifier) {
        return Err(Denial::MissingCapability);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (VerifiedCredential, RequestScope) {
        let caps = [
            "invoice:create",
            "invoice:status:read",
            "report:read",
            "refund:request",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let scope = Scope {
            store_id: "shop-a".into(),
            register_id: Some("front".into()),
        };
        (
            VerifiedCredential {
                credential_id: "cred-1".into(),
                scope: scope.clone(),
                capabilities: caps,
                issued_at: 100,
                expires_at: 200,
            },
            RequestScope {
                store_id: scope.store_id,
                register_id: scope.register_id,
            },
        )
    }

    fn check(
        c: &VerifiedCredential,
        s: &RequestScope,
        cap: Capability,
        now: u64,
    ) -> Result<(), Denial> {
        authorize(
            c,
            &BTreeSet::new(),
            s,
            cap,
            now,
            AuthorizationPolicy {
                max_lifetime_secs: 100,
            },
        )
    }

    #[test]
    fn cap_01_expiry_is_exclusive_and_credential_lifetime_is_bounded() {
        let (mut c, s) = fixture();
        assert_eq!(check(&c, &s, Capability::InvoiceCreate, 199), Ok(()));
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 200),
            Err(Denial::Expired)
        );
        c.expires_at = c.issued_at + 101;
        assert_eq!(
            check(&c, &s, Capability::ReportRead, 101),
            Err(Denial::InvalidLifetime)
        );
    }

    #[test]
    fn cap_02_revocation_is_per_credential() {
        let (c, s) = fixture();
        let revoked = ["cred-1".to_owned()].into_iter().collect();
        let policy = AuthorizationPolicy {
            max_lifetime_secs: 100,
        };
        assert_eq!(
            authorize(&c, &revoked, &s, Capability::ReportRead, 150, policy),
            Err(Denial::Revoked)
        );
        let unrelated = ["some-other-credential".to_owned()].into_iter().collect();
        assert_eq!(
            authorize(&c, &unrelated, &s, Capability::ReportRead, 150, policy),
            Ok(())
        );
    }

    #[test]
    fn cap_03_store_and_register_must_match() {
        let (c, mut s) = fixture();
        s.store_id = "shop-b".into();
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::WrongStore)
        );
        s.store_id = "shop-a".into();
        s.register_id = Some("back".into());
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::WrongRegister)
        );
        s.register_id = None;
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::WrongRegister)
        );
    }

    #[test]
    fn cap_03b_store_only_scope_is_exact_and_empty_metadata_is_invalid() {
        let (mut c, mut s) = fixture();
        c.scope.register_id = None;
        s.register_id = None;
        assert_eq!(check(&c, &s, Capability::InvoiceCreate, 150), Ok(()));
        s.register_id = Some("front".into());
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::WrongRegister)
        );
        c.scope.register_id = Some(String::new());
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::InvalidMetadata)
        );
        c.scope.register_id = None;
        c.scope.store_id.clear();
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::InvalidMetadata)
        );
        c.scope.store_id = "shop-a".into();
        c.credential_id.clear();
        assert_eq!(
            check(&c, &s, Capability::InvoiceCreate, 150),
            Err(Denial::InvalidMetadata)
        );
    }

    #[test]
    fn cap_04_scopes_are_independent_and_missing_scope_denies() {
        let (c, s) = fixture();
        assert_eq!(check(&c, &s, Capability::InvoiceCreate, 150), Ok(()));
        assert_eq!(check(&c, &s, Capability::InvoiceStatusRead, 150), Ok(()));
        assert_eq!(check(&c, &s, Capability::ReportRead, 150), Ok(()));
        let mut narrow = c.clone();
        narrow.capabilities.remove("report:read");
        assert_eq!(
            check(&narrow, &s, Capability::ReportRead, 150),
            Err(Denial::MissingCapability)
        );
    }

    #[test]
    fn cap_05_refund_request_never_authorizes_refund_spend() {
        let (c, s) = fixture();
        assert_eq!(check(&c, &s, Capability::RefundRequest, 150), Ok(()));
        assert_eq!(
            check(&c, &s, Capability::Spend, 150),
            Err(Denial::PrivilegedOperation)
        );
        assert_eq!(
            check(&c, &s, Capability::Sign, 150),
            Err(Denial::PrivilegedOperation)
        );
        assert_eq!(
            check(&c, &s, Capability::ChangeDestination, 150),
            Err(Denial::PrivilegedOperation)
        );
    }

    #[test]
    fn cap_06_unknown_capabilities_fail_closed() {
        let (c, s) = fixture();
        assert_eq!(
            Capability::from_identifier("report:export:v2"),
            Capability::Unknown
        );
        assert_eq!(
            check(&c, &s, Capability::from_identifier("report:export:v2"), 150),
            Err(Denial::UnknownCapability)
        );
        let mut future = c;
        future.capabilities.insert("wallet:sign:v2".into());
        assert_eq!(
            check(&future, &s, Capability::Sign, 150),
            Err(Denial::PrivilegedOperation)
        );
    }

    #[test]
    fn cap_07_future_issued_credentials_are_denied() {
        let (mut c, s) = fixture();
        c.issued_at = 151;
        assert_eq!(
            check(&c, &s, Capability::ReportRead, 150),
            Err(Denial::NotYetValid)
        );
    }

    #[test]
    fn cap_08_zero_lifetime_policy_fails_closed() {
        let (c, s) = fixture();
        assert_eq!(
            authorize(
                &c,
                &BTreeSet::new(),
                &s,
                Capability::ReportRead,
                150,
                AuthorizationPolicy {
                    max_lifetime_secs: 0
                }
            ),
            Err(Denial::InvalidPolicy)
        );
    }
}
