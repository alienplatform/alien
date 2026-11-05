use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Why a statement, signer list, anchor or workload was refused.
///
/// Every variant is a refusal an enforcer reports back to whoever sent the
/// input, so none of them is internal.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A key is not a valid ECDSA P-256 key in the expected encoding.
    #[error(
        code = "TRUST_KEY_INVALID",
        message = "Invalid signing key: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    KeyInvalid {
        /// What is wrong with the key.
        reason: String,
    },

    /// The bytes are not a well-formed signed statement.
    #[error(
        code = "TRUST_STATEMENT_MALFORMED",
        message = "Malformed statement: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    StatementMalformed {
        /// What is wrong with the statement.
        reason: String,
    },

    /// The statement is of another kind than the one expected.
    #[error(
        code = "TRUST_WRONG_KIND",
        message = "Expected a '{expected}' statement, got '{actual}'",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WrongKind {
        /// Kind the caller asked for.
        expected: String,
        /// Kind the statement declares.
        actual: String,
    },

    /// The statement names another project.
    #[error(
        code = "TRUST_WRONG_PROJECT",
        message = "The statement is for project '{actual}', not '{expected}'",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WrongProject {
        /// Project the anchor pins.
        expected: String,
        /// Project the statement names.
        actual: String,
    },

    /// The statement names another deployment, or none where one is required.
    #[error(
        code = "TRUST_WRONG_DEPLOYMENT",
        message = "The statement is for deployment '{actual}', not '{expected}'",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WrongDeployment {
        /// Deployment the anchor pins, or `none` for project-wide kinds.
        expected: String,
        /// Deployment the statement names, or `none`.
        actual: String,
    },

    /// A signature does not verify, or names a signer that may not sign.
    #[error(
        code = "TRUST_SIGNATURE_REFUSED",
        message = "Signature by '{signer}' refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    SignatureRefused {
        /// Signer id the signature names.
        signer: String,
        /// Why it was refused.
        reason: String,
    },

    /// The statement carries no signature that counts.
    #[error(
        code = "TRUST_UNSIGNED",
        message = "The statement is not signed by a key holding the '{role}' role",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Unsigned {
        /// Role the statement's kind requires.
        role: String,
    },

    /// The statement's sequence is not above the stored floor for its kind.
    #[error(
        code = "TRUST_SEQUENCE_REPLAYED",
        message = "The '{kind}' statement has sequence {sequence}, which is not above the accepted {floor}",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    SequenceReplayed {
        /// Statement kind.
        kind: String,
        /// Sequence of the refused statement.
        sequence: u64,
        /// Highest sequence already accepted for this kind.
        floor: u64,
    },

    /// The statement's `notAfter` has passed.
    #[error(
        code = "TRUST_EXPIRED",
        message = "The statement expired at {not_after}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Expired {
        /// When it stopped being valid, RFC 3339.
        not_after: String,
    },

    /// The statement's digest is on the signer list's deny-list.
    #[error(
        code = "TRUST_DENIED",
        message = "The statement {digest} is on the signer list's deny-list",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Denied {
        /// Digest of the statement payload.
        digest: String,
    },

    /// A signer list does not extend the trusted chain.
    #[error(
        code = "TRUST_SIGNER_LIST_REFUSED",
        message = "Signer list version {version} refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    SignerListRefused {
        /// Version the refused list claims.
        version: u64,
        /// Why it was refused.
        reason: String,
    },

    /// The statement was signed against a signer list version the enforcer
    /// has not been given.
    #[error(
        code = "TRUST_SIGNER_LIST_MISSING",
        message = "The statement needs signer list version {needed}, but only version {current} is trusted",
        retryable = "true",
        internal = "false",
        http_status_code = 409
    )]
    SignerListMissing {
        /// Version the statement names.
        needed: u64,
        /// Highest version the enforcer trusts.
        current: u64,
    },

    /// The anchor is missing, malformed or contradicts itself.
    #[error(
        code = "TRUST_ANCHOR_INVALID",
        message = "Invalid anchor: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    AnchorInvalid {
        /// What is wrong with the anchor.
        reason: String,
    },

    /// A workload asked for something its signed release does not allow.
    #[error(
        code = "TRUST_WORKLOAD_REFUSED",
        message = "Workload '{workload}' refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WorkloadRefused {
        /// Workload name.
        workload: String,
        /// Why it was refused.
        reason: String,
    },

    /// A statement body breaks a rule of its kind.
    #[error(
        code = "TRUST_BODY_REFUSED",
        message = "The '{kind}' statement was refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    BodyRefused {
        /// Statement kind.
        kind: String,
        /// Why it was refused.
        reason: String,
    },
}

/// Result type of this crate.
pub type Result<T> = alien_error::Result<T, ErrorData>;
