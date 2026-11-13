//! Preview capabilities for backends where the agent itself is the issuer (GCP Agent Platform).
//!
//! The caller proves it may act on the sandbox over the cloud-authenticated control path and asks
//! for a grant for one declared port. The agent answers with an HMAC over the sandbox name, this
//! boot, the port and the expiry, keyed by a random key made at boot that never leaves the guest.
//! A recreated container has a new key, so every capability of the old one fails closed.
//!
//! Neither the key nor a capability value is ever logged.

use std::sync::Mutex;

use alien_core::sandbox_image::{AGENT_PORT, PREVIEW_CAPABILITY_MAX_SECONDS};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Why a grant or a capability was refused. Never carries a capability value.
#[derive(Debug, PartialEq, Eq)]
pub enum PreviewRefusal {
    PortNotPreviewable,
    SandboxMismatch,
    NotGranted,
    Malformed,
    Expired,
    Forged,
}

impl PreviewRefusal {
    pub fn message(&self) -> &'static str {
        match self {
            Self::PortNotPreviewable => "that port is not a declared preview port",
            Self::SandboxMismatch => "this boot serves a different sandbox",
            Self::NotGranted => "no preview was granted on this boot",
            Self::Malformed => "the preview capability is malformed",
            Self::Expired => "the preview capability expired",
            Self::Forged => "the preview capability is not valid for this sandbox and port",
        }
    }
}

/// A granted capability and the unix time it stops working.
#[derive(Debug)]
pub struct Grant {
    pub capability: String,
    pub expires_at: i64,
}

/// The per-boot issuer and verifier.
pub struct PreviewAuthority {
    key: [u8; 32],
    boot_id: String,
    /// Fixed by the first grant: one boot serves one sandbox.
    sandbox: Mutex<Option<String>>,
}

impl Default for PreviewAuthority {
    fn default() -> Self {
        Self::new(
            std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map(|id| id.trim().to_string())
                .unwrap_or_default(),
        )
    }
}

impl PreviewAuthority {
    pub fn new(boot_id: String) -> Self {
        let mut key = [0u8; 32];
        key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self {
            key,
            boot_id,
            sandbox: Mutex::new(None),
        }
    }

    fn mac(&self, sandbox: &str, port: u16, expires_at: i64) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("any key length is accepted");
        mac.update(
            format!(
                "alien-preview-v1\n{sandbox}\n{}\n{port}\n{expires_at}",
                self.boot_id
            )
            .as_bytes(),
        );
        mac
    }

    /// Grants a capability for `port` of `sandbox`, valid until the earlier of now plus the
    /// maximum and `deadline` (the session's own deadline, when it has one).
    pub fn grant(
        &self,
        sandbox: &str,
        port: u16,
        declared: &[u16],
        deadline: Option<i64>,
        now: i64,
    ) -> Result<Grant, PreviewRefusal> {
        if port == AGENT_PORT || !declared.contains(&port) {
            return Err(PreviewRefusal::PortNotPreviewable);
        }
        {
            let mut bound = self.sandbox.lock().expect("sandbox name lock");
            match bound.as_deref() {
                Some(name) if name != sandbox => return Err(PreviewRefusal::SandboxMismatch),
                Some(_) => {}
                None => *bound = Some(sandbox.to_string()),
            }
        }
        let mut expires_at = now + PREVIEW_CAPABILITY_MAX_SECONDS;
        if let Some(deadline) = deadline {
            expires_at = expires_at.min(deadline);
        }
        if expires_at <= now {
            return Err(PreviewRefusal::Expired);
        }
        let tag = hex::encode(self.mac(sandbox, port, expires_at).finalize().into_bytes());
        Ok(Grant {
            capability: format!("v1.{expires_at}.{tag}"),
            expires_at,
        })
    }

    /// Checks `capability` for `port`. Returns the unix time it expires at.
    pub fn verify(&self, capability: &str, port: u16, now: i64) -> Result<i64, PreviewRefusal> {
        let sandbox = self
            .sandbox
            .lock()
            .expect("sandbox name lock")
            .clone()
            .ok_or(PreviewRefusal::NotGranted)?;
        let mut parts = capability.split('.');
        let (Some("v1"), Some(expires), Some(tag), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(PreviewRefusal::Malformed);
        };
        let expires_at: i64 = expires.parse().map_err(|_| PreviewRefusal::Malformed)?;
        let tag = hex::decode(tag).map_err(|_| PreviewRefusal::Malformed)?;
        let expected = self.mac(&sandbox, port, expires_at).finalize().into_bytes();
        if !bool::from(expected.as_slice().ct_eq(&tag)) {
            return Err(PreviewRefusal::Forged);
        }
        if expires_at <= now {
            return Err(PreviewRefusal::Expired);
        }
        Ok(expires_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn grant(authority: &PreviewAuthority, port: u16, deadline: Option<i64>) -> Grant {
        authority
            .grant("sandbox-a", port, &[8080, 8081], deadline, NOW)
            .unwrap()
    }

    #[test]
    fn a_grant_verifies_for_its_port_until_it_expires() {
        let authority = PreviewAuthority::new("boot-1".into());
        let granted = grant(&authority, 8080, None);
        assert_eq!(granted.expires_at, NOW + 900);
        assert_eq!(
            authority.verify(&granted.capability, 8080, NOW + 1),
            Ok(NOW + 900)
        );
        assert_eq!(
            authority.verify(&granted.capability, 8080, NOW + 900),
            Err(PreviewRefusal::Expired)
        );
    }

    #[test]
    fn a_grant_never_outlives_the_session_deadline() {
        let authority = PreviewAuthority::new("boot-1".into());
        assert_eq!(grant(&authority, 8080, Some(NOW + 60)).expires_at, NOW + 60);
        assert_eq!(
            authority.grant("sandbox-a", 8080, &[8080], Some(NOW), NOW).unwrap_err(),
            PreviewRefusal::Expired
        );
    }

    #[test]
    fn a_capability_for_one_port_does_not_admit_another() {
        let authority = PreviewAuthority::new("boot-1".into());
        let granted = grant(&authority, 8080, None);
        assert_eq!(
            authority.verify(&granted.capability, 8081, NOW + 1),
            Err(PreviewRefusal::Forged)
        );
    }

    #[test]
    fn forged_and_tampered_capabilities_are_refused() {
        let authority = PreviewAuthority::new("boot-1".into());
        let granted = grant(&authority, 8080, None);
        let forged = format!("v1.{}.{}", NOW + 900, "00".repeat(32));
        assert_eq!(authority.verify(&forged, 8080, NOW + 1), Err(PreviewRefusal::Forged));
        // Extending the expiry invalidates the MAC.
        let tag = granted.capability.rsplit('.').next().unwrap();
        let extended = format!("v1.{}.{tag}", NOW + 99_999);
        assert_eq!(authority.verify(&extended, 8080, NOW + 1), Err(PreviewRefusal::Forged));
        assert_eq!(authority.verify("garbage", 8080, NOW), Err(PreviewRefusal::Malformed));
    }

    #[test]
    fn a_capability_does_not_survive_a_new_boot() {
        let old = PreviewAuthority::new("boot-1".into());
        let granted = grant(&old, 8080, None);
        let new = PreviewAuthority::new("boot-1".into());
        // The new boot has granted nothing yet, then grants for the same sandbox and boot id.
        assert_eq!(
            new.verify(&granted.capability, 8080, NOW + 1),
            Err(PreviewRefusal::NotGranted)
        );
        grant(&new, 8081, None);
        assert_eq!(
            new.verify(&granted.capability, 8080, NOW + 1),
            Err(PreviewRefusal::Forged)
        );
    }

    #[test]
    fn a_capability_for_one_sandbox_is_not_usable_on_another() {
        let a = PreviewAuthority::new("boot-1".into());
        let granted = grant(&a, 8080, None);
        // Another sandbox cannot be granted on this boot, and its own boot has its own key.
        assert_eq!(
            a.grant("sandbox-b", 8080, &[8080], None, NOW).unwrap_err(),
            PreviewRefusal::SandboxMismatch
        );
        let b = PreviewAuthority::new("boot-2".into());
        b.grant("sandbox-b", 8080, &[8080], None, NOW).unwrap();
        assert!(b.verify(&granted.capability, 8080, NOW + 1).is_err());
    }

    #[test]
    fn the_agents_own_port_and_undeclared_ports_are_not_granted() {
        let authority = PreviewAuthority::new("boot-1".into());
        for port in [AGENT_PORT, 9999] {
            assert_eq!(
                authority.grant("sandbox-a", port, &[AGENT_PORT, 8080], None, NOW).unwrap_err(),
                PreviewRefusal::PortNotPreviewable
            );
        }
    }
}
