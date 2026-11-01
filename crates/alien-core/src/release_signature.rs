//! Signed releases.
//!
//! A developer signs each platform's release statement with an ECDSA P-256 key
//! that only they hold. The statement names the project, the platform, a
//! sequence that only grows, and the full stack with every image pinned by
//! digest. Whoever stores and forwards a release carries the statement's bytes
//! and the signature unchanged. A deployment in protected mode verifies the
//! bytes against the public keys its owner pinned, then reads the stack from
//! those bytes and nowhere else. The bytes are never re-serialized before
//! verifying, so no canonical JSON form is needed.
//!
//! The formats match what cloud key services produce, so the same key can live
//! in a KMS: a public key travels as `ecdsa-p256:<base64 DER SubjectPublicKeyInfo>`
//! and a signature as base64 DER ECDSA over the SHA-256 of the payload.

use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use p256::ecdsa::signature::{Signer as _, Verifier as _};
use p256::ecdsa::{DerSignature, SigningKey, VerifyingKey};
use p256::pkcs8::{
    DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::{
    Container, ContainerCode, Daemon, DaemonCode, Platform, Resource, Sandbox, SandboxCode,
    SignedRelease, Stack, Worker, WorkerCode,
};
use alien_error::AlienError;

/// Prefix of a release signing public key in text form.
pub const PUBLIC_KEY_PREFIX: &str = "ecdsa-p256:";

/// `type` of a version 1 release statement.
pub const STATEMENT_TYPE: &str = "alien-release/v1";

/// Resource types that run no code of their own, so a stack may carry them
/// without an image to check. A type missing from this list and from the
/// code-bearing types the verifier inspects is refused, so a resource type
/// added later can't carry an unchecked image.
const RESOURCE_TYPES_WITHOUT_CODE: &[&str] = &[
    "vault",
    "compute-cluster",
    "kubernetes-cluster",
    "storage",
    "key",
    "queue",
    "email",
    "kv",
    "postgres",
    "ai",
    "network",
    "build",
    "service-account",
    "artifact-registry",
    "service_activation",
    "remote-stack-management",
    "resource-access",
];

/// A developer's release signing key.
pub struct ReleaseSigningKey(SigningKey);

impl fmt::Debug for ReleaseSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReleaseSigningKey")
            .field(&self.public_key())
            .finish()
    }
}

impl ReleaseSigningKey {
    /// The key for 32 secret bytes, which must be a valid P-256 scalar.
    /// Random bytes are valid except with negligible probability.
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Result<Self> {
        SigningKey::from_slice(bytes)
            .map(Self)
            .map_err(|_| key_invalid("the secret bytes are not a valid P-256 key"))
    }

    /// Read a key from a PKCS#8 PEM file's contents.
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self> {
        SigningKey::from_pkcs8_pem(pem.trim())
            .map(Self)
            .map_err(|_| key_invalid("the file is not a PKCS#8 PEM ECDSA P-256 private key"))
    }

    /// The key as a PKCS#8 PEM file's contents.
    pub fn to_pkcs8_pem(&self) -> Result<String> {
        self.0
            .to_pkcs8_pem(LineEnding::LF)
            .map(|pem| pem.as_str().to_string())
            .map_err(|_| key_invalid("the key could not be encoded"))
    }

    /// The public key, in the text form a deployment pins.
    pub fn public_key(&self) -> String {
        let der = self
            .0
            .verifying_key()
            .to_public_key_der()
            .expect("a P-256 public key always encodes");
        format!("{PUBLIC_KEY_PREFIX}{}", STANDARD.encode(der.as_bytes()))
    }

    /// Sign `statement`, returning the bytes that were signed with the signature.
    pub fn sign(&self, statement: &ReleaseStatement) -> Result<SignedRelease> {
        let payload = serde_json::to_vec(statement).map_err(|error| {
            AlienError::new(ErrorData::ReleaseStatementInvalid {
                reason: format!("the statement could not be serialized: {error}"),
            })
        })?;
        let signature: DerSignature = self.0.sign(&payload);
        Ok(SignedRelease {
            payload: STANDARD.encode(&payload),
            signature: STANDARD.encode(signature.as_bytes()),
        })
    }
}

/// What a developer signs for one platform of a release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseStatement {
    /// Always [`STATEMENT_TYPE`]. Keeps a statement from being read as any
    /// other document the same key signs.
    #[serde(rename = "type")]
    pub statement_type: String,
    /// Project the release belongs to.
    pub project: String,
    /// Platform this stack is for.
    pub platform: Platform,
    /// Grows with every release, so an older release can't be replayed.
    /// The CLI uses the signing time in milliseconds.
    pub sequence: u64,
    /// The stack to deploy, with every image pinned by digest.
    pub stack: Stack,
    /// Operator image a deployment may update itself to, pinned by digest.
    #[serde(default)]
    pub operator_image: Option<String>,
}

/// A statement that passed verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRelease {
    /// The statement's sequence.
    pub sequence: u64,
    /// The stack, read from the verified bytes.
    pub stack: Stack,
    /// The signed operator image, if any.
    pub operator_image: Option<String>,
    /// Lowercase hex SHA-256 of the payload bytes, which identifies the release exactly.
    pub payload_sha256: String,
    /// Fingerprint of the key that signed it.
    pub key_fingerprint: String,
}

/// Why a release was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseRejection {
    /// The deployment requires a signed release and the release had none.
    #[error("the release is not signed")]
    Unsigned,
    /// The payload or signature isn't valid base64 or DER.
    #[error("the {0} is not valid encoding")]
    Malformed(String),
    /// No pinned key made the signature, or the payload changed after signing.
    #[error("the release was not signed by a trusted key, or was changed after signing")]
    UntrustedSignature,
    /// The signed bytes are not a release statement.
    #[error("the signed payload is not a release statement: {0}")]
    NotAStatement(String),
    /// The statement is for another project.
    #[error("the release is for project '{actual}', not '{expected}'")]
    WrongProject { expected: String, actual: String },
    /// The statement is for another platform.
    #[error("the release is for platform '{actual}', not '{expected}'")]
    WrongPlatform { expected: String, actual: String },
    /// An image isn't pinned by digest, so the registry could serve anything.
    #[error("resource '{resource}' uses image '{image}', which is not pinned by sha256 digest")]
    ImageNotPinned { resource: String, image: String },
    /// A resource is built from source, which a deployment can't check.
    #[error("resource '{0}' is built from source, which a signed release can't carry")]
    SourceCode(String),
    /// A resource type the verifier doesn't know how to check.
    #[error(
        "resource '{resource}' has type '{resource_type}', which a signed release can't carry"
    )]
    UnknownResourceType {
        resource: String,
        resource_type: String,
    },
    /// The release is older than one already applied.
    #[error("the release has sequence {actual}, older than the applied {applied}")]
    OlderSequence { applied: u64, actual: u64 },
    /// The release reuses the applied sequence with different bytes.
    #[error("the release reuses sequence {0} with different contents")]
    ReusedSequence(u64),
}

/// The public keys a deployment trusts.
#[derive(Clone)]
pub struct TrustedKeys(Vec<(VerifyingKey, String)>);

impl fmt::Debug for TrustedKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(_, fingerprint)| fingerprint))
            .finish()
    }
}

impl TrustedKeys {
    /// Parse public keys in text form. At least one is required.
    pub fn parse<S: AsRef<str>>(keys: &[S]) -> Result<Self> {
        if keys.is_empty() {
            return Err(key_invalid("at least one public key is required"));
        }
        keys.iter()
            .map(|key| {
                let key = key.as_ref().trim();
                let der = key
                    .strip_prefix(PUBLIC_KEY_PREFIX)
                    .ok_or_else(|| {
                        key_invalid(&format!(
                            "a public key must start with '{PUBLIC_KEY_PREFIX}'"
                        ))
                    })
                    .and_then(|encoded| {
                        STANDARD
                            .decode(encoded)
                            .map_err(|_| key_invalid("a public key is not valid base64"))
                    })?;
                let verifying_key = VerifyingKey::from_public_key_der(&der)
                    .map_err(|_| key_invalid("a public key is not a P-256 SubjectPublicKeyInfo"))?;
                Ok((verifying_key, fingerprint(&der)))
            })
            .collect::<Result<Vec<_>>>()
            .map(Self)
    }

    /// Fingerprints of the trusted keys, for logs.
    pub fn fingerprints(&self) -> Vec<&str> {
        self.0
            .iter()
            .map(|(_, fingerprint)| fingerprint.as_str())
            .collect()
    }

    /// Verify `signed` as a release of `project` for `platform`.
    pub fn verify(
        &self,
        project: &str,
        platform: Platform,
        signed: &SignedRelease,
    ) -> std::result::Result<VerifiedRelease, ReleaseRejection> {
        let payload = STANDARD
            .decode(signed.payload.trim())
            .map_err(|_| ReleaseRejection::Malformed("payload".to_string()))?;
        let signature = STANDARD
            .decode(signed.signature.trim())
            .ok()
            .and_then(|der| DerSignature::from_bytes(&der).ok())
            .ok_or_else(|| ReleaseRejection::Malformed("signature".to_string()))?;
        let key_fingerprint = self
            .0
            .iter()
            .find(|(key, _)| key.verify(&payload, &signature).is_ok())
            .map(|(_, fingerprint)| fingerprint.clone())
            .ok_or(ReleaseRejection::UntrustedSignature)?;

        let statement: ReleaseStatement = serde_json::from_slice(&payload)
            .map_err(|error| ReleaseRejection::NotAStatement(error.to_string()))?;
        if statement.statement_type != STATEMENT_TYPE {
            return Err(ReleaseRejection::NotAStatement(format!(
                "type '{}' is not '{STATEMENT_TYPE}'",
                statement.statement_type
            )));
        }
        if statement.project != project {
            return Err(ReleaseRejection::WrongProject {
                expected: project.to_string(),
                actual: statement.project,
            });
        }
        if statement.platform != platform {
            return Err(ReleaseRejection::WrongPlatform {
                expected: platform.as_str().to_string(),
                actual: statement.platform.as_str().to_string(),
            });
        }
        check_stack_images(&statement.stack)?;
        if let Some(image) = &statement.operator_image {
            if image_digest(image).is_none() {
                return Err(ReleaseRejection::ImageNotPinned {
                    resource: "operator".to_string(),
                    image: image.clone(),
                });
            }
        }

        Ok(VerifiedRelease {
            sequence: statement.sequence,
            stack: statement.stack,
            operator_image: statement.operator_image,
            payload_sha256: hex_sha256(&payload),
            key_fingerprint,
        })
    }
}

impl VerifiedRelease {
    /// Check that this release may replace one applied with `applied_sequence`
    /// and payload hash `applied_payload_sha256`: newer, or the same release again.
    pub fn check_newer_than(
        &self,
        applied_sequence: u64,
        applied_payload_sha256: &str,
    ) -> std::result::Result<(), ReleaseRejection> {
        if self.sequence < applied_sequence {
            return Err(ReleaseRejection::OlderSequence {
                applied: applied_sequence,
                actual: self.sequence,
            });
        }
        if self.sequence == applied_sequence && self.payload_sha256 != applied_payload_sha256 {
            return Err(ReleaseRejection::ReusedSequence(self.sequence));
        }
        Ok(())
    }
}

/// The image a resource runs, for the resource types that run one.
pub fn code_image(resource: &Resource) -> Option<&str> {
    if let Some(container) = resource.downcast_ref::<Container>() {
        match &container.code {
            ContainerCode::Image { image } => Some(image),
            ContainerCode::Source { .. } => None,
        }
    } else if let Some(worker) = resource.downcast_ref::<Worker>() {
        match &worker.code {
            WorkerCode::Image { image } => Some(image),
            WorkerCode::Source { .. } => None,
        }
    } else if let Some(daemon) = resource.downcast_ref::<Daemon>() {
        match &daemon.code {
            DaemonCode::Image { image } => Some(image),
            DaemonCode::Source { .. } => None,
        }
    } else if let Some(sandbox) = resource.downcast_ref::<Sandbox>() {
        match &sandbox.code {
            SandboxCode::Image { image, .. } => Some(image),
            SandboxCode::Source { .. } => None,
        }
    } else {
        None
    }
}

/// Mutable access to the image [`code_image`] returns.
pub fn code_image_mut(resource: &mut Resource) -> Option<&mut String> {
    // Test the type before borrowing mutably: returning a borrow out of an
    // `if let` chain of `downcast_mut` calls doesn't pass the stable borrow
    // checker.
    if resource.downcast_ref::<Container>().is_some() {
        match &mut resource.downcast_mut::<Container>()?.code {
            ContainerCode::Image { image } => Some(image),
            ContainerCode::Source { .. } => None,
        }
    } else if resource.downcast_ref::<Worker>().is_some() {
        match &mut resource.downcast_mut::<Worker>()?.code {
            WorkerCode::Image { image } => Some(image),
            WorkerCode::Source { .. } => None,
        }
    } else if resource.downcast_ref::<Daemon>().is_some() {
        match &mut resource.downcast_mut::<Daemon>()?.code {
            DaemonCode::Image { image } => Some(image),
            DaemonCode::Source { .. } => None,
        }
    } else if resource.downcast_ref::<Sandbox>().is_some() {
        match &mut resource.downcast_mut::<Sandbox>()?.code {
            SandboxCode::Image { image, .. } => Some(image),
            SandboxCode::Source { .. } => None,
        }
    } else {
        None
    }
}

/// `signed` with each image moved to where `carried` says the same resource's
/// image is, when both name the same digest. A carrier may serve an image from
/// another registry (a manager's pull proxy, a site's own registry), but only
/// the bytes the developer signed.
pub fn relocate_images(signed: &Stack, carried: &Stack) -> Stack {
    let mut relocated = signed.clone();
    for (id, entry) in relocated.resources_mut() {
        let Some(location) = carried
            .resources
            .get(id)
            .and_then(|carried| code_image(&carried.config))
        else {
            continue;
        };
        if let Some(image) = code_image_mut(&mut entry.config) {
            if image_digest(image).is_some() && image_digest(image) == image_digest(location) {
                *image = location.to_string();
            }
        }
    }
    relocated
}

/// The `sha256:<hex>` digest an image reference is pinned to, if it is.
pub fn image_digest(image: &str) -> Option<&str> {
    let (repository, digest) = image.rsplit_once('@')?;
    let hex = digest.strip_prefix("sha256:")?;
    let valid = !repository.is_empty()
        && !repository.chars().any(char::is_whitespace)
        && hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    valid.then_some(digest)
}

/// Fingerprint of a public key in text form, as logged and displayed:
/// `sha256:` and the first 16 hex characters of SHA-256 over its DER bytes.
pub fn key_fingerprint(public_key: &str) -> Result<String> {
    TrustedKeys::parse(&[public_key]).map(|keys| keys.0[0].1.clone())
}

fn check_stack_images(stack: &Stack) -> std::result::Result<(), ReleaseRejection> {
    for (id, entry) in &stack.resources {
        let config = &entry.config;
        let image = if let Some(container) = config.downcast_ref::<Container>() {
            match &container.code {
                ContainerCode::Image { image } => image,
                ContainerCode::Source { .. } => {
                    return Err(ReleaseRejection::SourceCode(id.clone()))
                }
            }
        } else if let Some(worker) = config.downcast_ref::<Worker>() {
            match &worker.code {
                WorkerCode::Image { image } => image,
                WorkerCode::Source { .. } => return Err(ReleaseRejection::SourceCode(id.clone())),
            }
        } else if let Some(daemon) = config.downcast_ref::<Daemon>() {
            match &daemon.code {
                DaemonCode::Image { image } => image,
                DaemonCode::Source { .. } => return Err(ReleaseRejection::SourceCode(id.clone())),
            }
        } else if let Some(sandbox) = config.downcast_ref::<Sandbox>() {
            if let Some(base) = &sandbox.private_base_image {
                require_pinned(id, base)?;
            }
            match &sandbox.code {
                SandboxCode::Image { image, .. } => image,
                SandboxCode::Source { .. } => return Err(ReleaseRejection::SourceCode(id.clone())),
            }
        } else {
            let resource_type = config.resource_type();
            if RESOURCE_TYPES_WITHOUT_CODE.contains(&resource_type.as_ref()) {
                continue;
            }
            return Err(ReleaseRejection::UnknownResourceType {
                resource: id.clone(),
                resource_type: resource_type.to_string(),
            });
        };
        require_pinned(id, image)?;
    }
    Ok(())
}

fn require_pinned(resource: &str, image: &str) -> std::result::Result<(), ReleaseRejection> {
    match image_digest(image) {
        Some(_) => Ok(()),
        None => Err(ReleaseRejection::ImageNotPinned {
            resource: resource.to_string(),
            image: image.to_string(),
        }),
    }
}

fn fingerprint(der: &[u8]) -> String {
    format!("sha256:{}", &hex_sha256(der)[..16])
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn key_invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ReleaseSigningKeyInvalid {
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AzureResourceGroup, ResourceLifecycle, ResourceSpec};

    const DIGEST: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn key(byte: u8) -> ReleaseSigningKey {
        ReleaseSigningKey::from_secret_bytes(&[byte; 32]).expect("valid scalar")
    }

    fn trusted(key: &ReleaseSigningKey) -> TrustedKeys {
        TrustedKeys::parse(&[key.public_key()]).expect("valid public key")
    }

    fn container(image: &str) -> Container {
        Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: image.to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("execution".to_string())
            .build()
    }

    fn statement(image: &str) -> ReleaseStatement {
        ReleaseStatement {
            statement_type: STATEMENT_TYPE.to_string(),
            project: "shop".to_string(),
            platform: Platform::Kubernetes,
            sequence: 1_700_000_000_000,
            stack: Stack::new("shop".to_string())
                .add(container(image), ResourceLifecycle::Live)
                .build(),
            operator_image: None,
        }
    }

    fn pinned() -> String {
        format!("registry.example.com/shop/api@{DIGEST}")
    }

    /// Sign arbitrary bytes, as an attacker holding the key (or a buggy signer) could.
    fn sign_bytes(key: &ReleaseSigningKey, payload: &[u8]) -> SignedRelease {
        let signature: DerSignature = key.0.sign(payload);
        SignedRelease {
            payload: STANDARD.encode(payload),
            signature: STANDARD.encode(signature.as_bytes()),
        }
    }

    fn verify(
        keys: &TrustedKeys,
        signed: &SignedRelease,
    ) -> std::result::Result<VerifiedRelease, ReleaseRejection> {
        keys.verify("shop", Platform::Kubernetes, signed)
    }

    #[test]
    fn a_signed_statement_verifies_and_yields_the_signed_stack() {
        let key = key(7);
        let statement = statement(&pinned());
        let signed = key.sign(&statement).unwrap();

        let verified = verify(&trusted(&key), &signed).expect("must verify");

        assert_eq!(verified.stack, statement.stack);
        assert_eq!(verified.sequence, statement.sequence);
        let payload = STANDARD.decode(&signed.payload).unwrap();
        assert_eq!(verified.payload_sha256, hex_sha256(&payload));
        assert_eq!(
            verified.key_fingerprint,
            key_fingerprint(&key.public_key()).unwrap()
        );
    }

    #[test]
    fn a_pem_key_round_trips() {
        let key = key(7);
        let pem = key.to_pkcs8_pem().unwrap();
        assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        let read = ReleaseSigningKey::from_pkcs8_pem(&pem).unwrap();
        assert_eq!(read.public_key(), key.public_key());
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        let key = key(7);
        let mut signed = key.sign(&statement(&pinned())).unwrap();
        let payload = String::from_utf8(STANDARD.decode(&signed.payload).unwrap()).unwrap();
        let tampered = payload.replace("\"port\":8080", "\"port\":8081");
        assert_ne!(payload, tampered, "the test must change the payload");
        signed.payload = STANDARD.encode(tampered);

        assert_eq!(
            verify(&trusted(&key), &signed),
            Err(ReleaseRejection::UntrustedSignature)
        );
    }

    #[test]
    fn another_key_is_rejected() {
        let signed = key(7).sign(&statement(&pinned())).unwrap();
        assert_eq!(
            verify(&trusted(&key(8)), &signed),
            Err(ReleaseRejection::UntrustedSignature)
        );
    }

    #[test]
    fn any_pinned_key_may_sign() {
        let signed = key(8).sign(&statement(&pinned())).unwrap();
        let keys = TrustedKeys::parse(&[key(7).public_key(), key(8).public_key()]).unwrap();
        verify(&keys, &signed).expect("the second key signed it");
    }

    #[test]
    fn a_missing_or_garbage_signature_is_rejected() {
        let key = key(7);
        let good = key.sign(&statement(&pinned())).unwrap();
        let truncated = &good.signature[..good.signature.len() / 2];
        for signature in ["", "not base64!", "AAAA", truncated] {
            let signed = SignedRelease {
                payload: good.payload.clone(),
                signature: signature.to_string(),
            };
            assert_eq!(
                verify(&trusted(&key), &signed),
                Err(ReleaseRejection::Malformed("signature".to_string())),
                "signature {signature:?}"
            );
        }
    }

    #[test]
    fn a_garbage_payload_is_rejected() {
        let key = key(7);
        let good = key.sign(&statement(&pinned())).unwrap();
        let signed = SignedRelease {
            payload: "%%%".to_string(),
            signature: good.signature,
        };
        assert_eq!(
            verify(&trusted(&key), &signed),
            Err(ReleaseRejection::Malformed("payload".to_string()))
        );
    }

    #[test]
    fn signed_bytes_that_are_not_a_statement_are_rejected() {
        let key = key(7);
        let payload = serde_json::to_vec(&statement(&pinned())).unwrap();
        let truncated = &payload[..payload.len() - 10];
        for bytes in [truncated, b"garbage".as_slice(), b"{}".as_slice()] {
            assert!(
                matches!(
                    verify(&trusted(&key), &sign_bytes(&key, bytes)),
                    Err(ReleaseRejection::NotAStatement(_))
                ),
                "payload {:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn another_statement_type_is_rejected() {
        let key = key(7);
        let mut other = statement(&pinned());
        other.statement_type = "attestation/v1".to_string();
        assert!(matches!(
            verify(&trusted(&key), &key.sign(&other).unwrap()),
            Err(ReleaseRejection::NotAStatement(_))
        ));
    }

    #[test]
    fn unknown_statement_fields_are_rejected() {
        let key = key(7);
        let mut value = serde_json::to_value(statement(&pinned())).unwrap();
        value["environment"] = serde_json::json!({"LD_PRELOAD": "/tmp/x.so"});
        let signed = sign_bytes(&key, &serde_json::to_vec(&value).unwrap());
        assert!(matches!(
            verify(&trusted(&key), &signed),
            Err(ReleaseRejection::NotAStatement(_))
        ));
    }

    #[test]
    fn another_project_or_platform_is_rejected() {
        let key = key(7);
        let mut other_project = statement(&pinned());
        other_project.project = "billing".to_string();
        assert_eq!(
            verify(&trusted(&key), &key.sign(&other_project).unwrap()),
            Err(ReleaseRejection::WrongProject {
                expected: "shop".to_string(),
                actual: "billing".to_string(),
            })
        );

        let mut other_platform = statement(&pinned());
        other_platform.platform = Platform::Aws;
        assert_eq!(
            verify(&trusted(&key), &key.sign(&other_platform).unwrap()),
            Err(ReleaseRejection::WrongPlatform {
                expected: "kubernetes".to_string(),
                actual: "aws".to_string(),
            })
        );
    }

    #[test]
    fn a_tag_only_image_is_rejected() {
        let key = key(7);
        for image in [
            "registry.example.com/shop/api:v1",
            "registry.example.com/shop/api@sha256:abc",
            "registry.example.com/shop/api@sha512:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "@sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        ] {
            assert_eq!(
                verify(&trusted(&key), &key.sign(&statement(image)).unwrap()),
                Err(ReleaseRejection::ImageNotPinned {
                    resource: "api".to_string(),
                    image: image.to_string(),
                })
            );
        }
    }

    #[test]
    fn a_tag_and_digest_image_is_accepted() {
        let key = key(7);
        let image = format!("registry.example.com/shop/api:v1@{DIGEST}");
        verify(&trusted(&key), &key.sign(&statement(&image)).unwrap()).expect("pinned");
    }

    #[test]
    fn an_unpinned_operator_image_is_rejected() {
        let key = key(7);
        let mut with_operator = statement(&pinned());
        with_operator.operator_image = Some("registry.example.com/operator:latest".to_string());
        assert!(matches!(
            verify(&trusted(&key), &key.sign(&with_operator).unwrap()),
            Err(ReleaseRejection::ImageNotPinned { resource, .. }) if resource == "operator"
        ));
    }

    #[test]
    fn source_code_is_rejected() {
        let key = key(7);
        let mut value = serde_json::to_value(statement(&pinned())).unwrap();
        value["stack"]["resources"]["api"]["config"]["code"] = serde_json::json!({
            "type": "source",
            "src": "./api",
            "toolchain": {"type": "docker"}
        });
        let parsed: ReleaseStatement = serde_json::from_value(value.clone())
            .expect("the test stack must still parse, or it proves nothing");
        assert!(matches!(
            &parsed.stack.resources["api"]
                .config
                .downcast_ref::<Container>()
                .unwrap()
                .code,
            ContainerCode::Source { .. }
        ));
        let signed = sign_bytes(&key, &serde_json::to_vec(&value).unwrap());
        assert_eq!(
            verify(&trusted(&key), &signed),
            Err(ReleaseRejection::SourceCode("api".to_string()))
        );
    }

    #[test]
    fn a_resource_type_the_verifier_does_not_know_is_rejected() {
        let key = key(7);
        let mut with_unknown = statement(&pinned());
        with_unknown.stack = Stack::new("shop".to_string())
            .add(container(&pinned()), ResourceLifecycle::Live)
            .add(
                AzureResourceGroup::new("rg".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        assert_eq!(
            verify(&trusted(&key), &key.sign(&with_unknown).unwrap()),
            Err(ReleaseRejection::UnknownResourceType {
                resource: "rg".to_string(),
                resource_type: "azure_resource_group".to_string(),
            })
        );
    }

    #[test]
    fn only_a_newer_or_identical_release_replaces_the_applied_one() {
        let key = key(7);
        let keys = trusted(&key);
        let applied = verify(&keys, &key.sign(&statement(&pinned())).unwrap()).unwrap();

        applied
            .check_newer_than(applied.sequence, &applied.payload_sha256)
            .expect("the same release again is accepted");
        applied
            .check_newer_than(applied.sequence - 1, "other")
            .expect("a newer release is accepted");

        assert_eq!(
            applied.check_newer_than(applied.sequence + 1, "other"),
            Err(ReleaseRejection::OlderSequence {
                applied: applied.sequence + 1,
                actual: applied.sequence,
            })
        );

        let mut same_sequence = statement(&pinned());
        same_sequence.stack.id = "shop-changed".to_string();
        let changed = verify(&keys, &key.sign(&same_sequence).unwrap()).unwrap();
        assert_eq!(
            changed.check_newer_than(applied.sequence, &applied.payload_sha256),
            Err(ReleaseRejection::ReusedSequence(applied.sequence))
        );
    }

    #[test]
    fn a_carrier_may_move_an_image_but_not_change_it() {
        let signed = statement(&pinned()).stack;
        let moved = format!("site-registry.local:5000/shop/api@{DIGEST}");
        let swapped = format!(
            "site-registry.local:5000/shop/api@sha256:{}",
            "e".repeat(64)
        );
        let carried = |image: &str| statement(image).stack;

        let relocated = relocate_images(&signed, &carried(&moved));
        assert_eq!(
            code_image(&relocated.resources["api"].config),
            Some(moved.as_str())
        );
        for other in [swapped.as_str(), "site-registry.local:5000/shop/api:v1"] {
            let relocated = relocate_images(&signed, &carried(other));
            assert_eq!(
                code_image(&relocated.resources["api"].config),
                Some(pinned().as_str()),
                "{other} must not replace the signed image"
            );
        }
    }

    #[test]
    fn malformed_public_keys_are_rejected() {
        let empty: [&str; 0] = [];
        for keys in [
            vec!["ed25519:AAAA".to_string()],
            vec!["ecdsa-p256:not base64".to_string()],
            vec!["ecdsa-p256:AAAA".to_string()],
        ] {
            let error = TrustedKeys::parse(&keys).expect_err("must be rejected");
            assert_eq!(error.code, "RELEASE_SIGNING_KEY_INVALID");
        }
        assert!(TrustedKeys::parse(&empty).is_err());
    }

    #[test]
    fn a_kms_style_public_key_verifies() {
        // KMS returns a SubjectPublicKeyInfo DER and a DER signature over SHA-256 of
        // the message. Build both with the plain RustCrypto API, independently of
        // `ReleaseSigningKey`, and check the verifier accepts them.
        let signing = SigningKey::from_slice(&[9; 32]).unwrap();
        let spki = signing.verifying_key().to_public_key_der().unwrap();
        let public_key = format!("ecdsa-p256:{}", STANDARD.encode(spki.as_bytes()));
        let payload = serde_json::to_vec(&statement(&pinned())).unwrap();
        let digest = Sha256::digest(&payload);
        let signature: p256::ecdsa::Signature =
            p256::ecdsa::signature::hazmat::PrehashSigner::sign_prehash(&signing, &digest).unwrap();
        let signed = SignedRelease {
            payload: STANDARD.encode(&payload),
            signature: STANDARD.encode(signature.to_der().as_bytes()),
        };
        verify(&TrustedKeys::parse(&[public_key]).unwrap(), &signed).expect("must verify");
    }
}
