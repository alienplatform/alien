use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Why a statement, signer list, setting, ledger or workload was refused.
///
/// Every variant is a refusal an enforcer reports back to whoever sent the
/// input, so none of them is internal.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A key is not a valid ECDSA P-256 key in the expected encoding.
    #[error(
        code = "SIGNING_KEY_INVALID",
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
        code = "SIGNING_STATEMENT_MALFORMED",
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
        code = "SIGNING_WRONG_KIND",
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
        code = "SIGNING_WRONG_PROJECT",
        message = "The statement is for project '{actual}', not '{expected}'",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WrongProject {
        /// Project of the pinned signer list.
        expected: String,
        /// Project the statement names.
        actual: String,
    },

    /// The statement names another install, or none where one is required.
    #[error(
        code = "SIGNING_WRONG_INSTALL",
        message = "The statement is for install '{actual}', not '{expected}'",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WrongInstall {
        /// This install, or `none` for project-wide kinds.
        expected: String,
        /// Install the statement names, or `none`.
        actual: String,
    },

    /// A release is for another channel than the one setup pinned.
    #[error(
        code = "SIGNING_WRONG_CHANNEL",
        message = "The release is for channel '{actual}', but this install follows '{expected}'",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    WrongChannel {
        /// Channel setup pinned.
        expected: String,
        /// Channel the release names.
        actual: String,
    },

    /// A signature does not verify, or names a signer that may not sign.
    #[error(
        code = "SIGNING_SIGNATURE_REFUSED",
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
        code = "SIGNING_UNSIGNED",
        message = "The statement is not signed by a key holding the '{role}' role",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Unsigned {
        /// Role the statement's kind requires.
        role: String,
    },

    /// The statement's `notAfter` has passed.
    #[error(
        code = "SIGNING_EXPIRED",
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
        code = "SIGNING_DENIED",
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
        code = "SIGNING_SIGNER_LIST_REFUSED",
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
        code = "SIGNING_SIGNER_LIST_MISSING",
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

    /// The signing settings setup wrote are malformed or contradict where
    /// they were found.
    #[error(
        code = "SIGNING_SETTINGS_INVALID",
        message = "Invalid signing settings: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    SettingsInvalid {
        /// What is wrong with the settings.
        reason: String,
    },

    /// Whether this install is protected can't be decided from what the
    /// enforcer could read, so it runs nothing new.
    #[error(
        code = "SIGNING_SETTINGS_UNDECIDED",
        message = "Can't tell whether this install is protected: {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    SettingsUndecided {
        /// What the enforcer saw.
        reason: String,
    },

    /// The install accepted signed state under one trust root and now finds
    /// another, or none. Only a local operator command resets it.
    #[error(
        code = "SIGNING_TRUST_ROOT_CHANGED",
        message = "This install accepted state under {accepted} and now finds {found}; it runs nothing new until an operator resets it",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    TrustRootChanged {
        /// The fingerprint or install id the install accepted state under.
        accepted: String,
        /// What it finds now, or `none`.
        found: String,
    },

    /// The install's ledger is missing, malformed or forks from what this
    /// enforcer holds.
    #[error(
        code = "SIGNING_LEDGER_REFUSED",
        message = "The install's ledger was refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    LedgerRefused {
        /// Why.
        reason: String,
    },

    /// A workload asked for something its signed release does not allow.
    #[error(
        code = "SIGNING_WORKLOAD_REFUSED",
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
        code = "SIGNING_BODY_REFUSED",
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
