//! Checks a create request's mounts against the sandbox's declaration, before anything starts.
//!
//! The declaration names each mount path and its type; the request supplies, per `s3files` path,
//! the file system, access point, region and credentials. Errors name paths and fields only:
//! never a credential value.

use std::collections::BTreeMap;

use alien_core::SandboxMount;
use alien_error::AlienError;

use crate::error::{ErrorData, Result};
use crate::traits::{SandboxMountCredentials, SandboxMountSource};

fn refuse(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxMountsInvalid {
        reason: reason.into(),
    })
}

/// A backend without mounts refuses a request that carries any, rather than dropping them: a
/// caller that passed credentials expects a filesystem.
pub fn refuse_unsupported(
    request: &crate::traits::CreateSandboxRequest,
    operation: &str,
) -> Result<()> {
    if request.mounts.is_empty() {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::OperationNotSupported {
        operation: operation.to_string(),
        reason: "this sandbox backend has no mounts; mounts are available on AWS only".to_string(),
    }))
}

/// Refuses a credential set that cannot be used: an empty field or an unreadable expiry.
pub fn validate_credentials(path: &str, credentials: &SandboxMountCredentials) -> Result<()> {
    for (name, value) in [
        ("accessKeyId", &credentials.access_key_id),
        ("secretAccessKey", &credentials.secret_access_key),
        ("sessionToken", &credentials.session_token),
    ] {
        if value.is_empty() {
            return Err(refuse(format!("mount '{path}': credentials.{name} is empty")));
        }
    }
    let expiry = chrono::DateTime::parse_from_rfc3339(&credentials.expiration).map_err(|_| {
        refuse(format!(
            "mount '{path}': credentials.expiration must be an RFC 3339 timestamp"
        ))
    })?;
    if expiry.timestamp() <= chrono::Utc::now().timestamp() {
        return Err(refuse(format!(
            "mount '{path}': the credentials have already expired"
        )));
    }
    Ok(())
}

/// Every declared `s3files` path needs exactly one source, and nothing else may be supplied.
pub fn validate_sources(
    declared: &[SandboxMount],
    supplied: &BTreeMap<String, SandboxMountSource>,
) -> Result<()> {
    for path in supplied.keys() {
        match declared.iter().find(|mount| mount.path() == path) {
            None => {
                return Err(refuse(format!(
                    "'{path}' is not a declared mount path"
                )))
            }
            Some(SandboxMount::Tmpfs { .. }) => {
                return Err(refuse(format!(
                    "'{path}' is a tmpfs mount and takes no source"
                )))
            }
            Some(SandboxMount::S3files { .. }) => {}
        }
    }
    for mount in declared {
        if let SandboxMount::S3files { path } = mount {
            let source = supplied
                .get(path)
                .ok_or_else(|| refuse(format!("the declared s3files mount '{path}' needs a source")))?;
            if !source.file_system_id.starts_with("fs-")
                || !source
                    .file_system_id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return Err(refuse(format!(
                    "mount '{path}': fileSystemId must look like fs-0123456789abcdef0"
                )));
            }
            if !source.access_point_id.starts_with("fsap-")
                || !source
                    .access_point_id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return Err(refuse(format!(
                    "mount '{path}': accessPointId must look like fsap-0123456789abcdef0"
                )));
            }
            if source.region.is_empty()
                || !source
                    .region
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            {
                return Err(refuse(format!("mount '{path}': region is not a valid region name")));
            }
            if source.availability_zone_id.is_none() {
                return Err(refuse(format!(
                    "mount '{path}': availabilityZoneId is missing: pass the zone id of a mount target of this file system that the sandbox's network reaches (for example use2-az1; the S3 Files mount target listing shows it as availabilityZoneId)"
                )));
            }
            if let Some(zone) = &source.availability_zone_id {
                let mut parts = zone.splitn(2, "-az");
                let valid = matches!(
                    (parts.next(), parts.next()),
                    (Some(prefix), Some(number))
                        if !prefix.is_empty()
                            && prefix.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                            && !number.is_empty()
                            && number.bytes().all(|c| c.is_ascii_digit())
                );
                if !valid {
                    return Err(refuse(format!(
                        "mount '{path}': availabilityZoneId must look like use2-az1"
                    )));
                }
            }
            validate_credentials(path, &source.credentials)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SandboxMountSource {
        SandboxMountSource {
            file_system_id: "fs-0123456789abcdef0".into(),
            access_point_id: "fsap-0123456789abcdef0".into(),
            region: "us-east-1".into(),
            availability_zone_id: Some("use1-az1".into()),
            credentials: SandboxMountCredentials {
                access_key_id: "AKIDEXAMPLE".into(),
                secret_access_key: "SECRETVALUE".into(),
                session_token: "TOKENVALUE".into(),
                expiration: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            },
        }
    }

    fn declared() -> Vec<SandboxMount> {
        vec![
            SandboxMount::S3files {
                path: "/workspace".into(),
            },
            SandboxMount::Tmpfs {
                path: "/workspace/tmp".into(),
                size: "8Gi".into(),
            },
        ]
    }

    fn supplied() -> BTreeMap<String, SandboxMountSource> {
        BTreeMap::from([("/workspace".to_string(), source())])
    }

    #[test]
    fn a_complete_request_passes() {
        validate_sources(&declared(), &supplied()).unwrap();
    }

    #[test]
    fn a_declared_s3files_mount_without_a_source_is_refused() {
        let error = validate_sources(&declared(), &BTreeMap::new()).unwrap_err();
        assert_eq!(error.code, "SANDBOX_MOUNTS_INVALID");
        assert!(error.message.contains("/workspace"));
    }

    #[test]
    fn an_undeclared_or_tmpfs_path_is_refused() {
        let mut extra = supplied();
        extra.insert("/other".into(), source());
        assert!(validate_sources(&declared(), &extra).is_err());
        let mut tmp = supplied();
        tmp.insert("/workspace/tmp".into(), source());
        assert!(validate_sources(&declared(), &tmp).is_err());
        assert!(validate_sources(&[], &supplied()).is_err());
    }

    #[test]
    fn malformed_fields_are_refused_without_echoing_secrets() {
        for mutate in [
            (|s: &mut SandboxMountSource| s.file_system_id = "xx-1".into()) as fn(&mut _),
            |s| s.access_point_id = "fs-1".into(),
            |s| s.region = "US East".into(),
            |s| s.availability_zone_id = Some("use2".into()),
            |s| s.availability_zone_id = None,
            |s| s.credentials.session_token.clear(),
            |s| s.credentials.expiration = "tomorrow".into(),
            |s| s.credentials.expiration = "2001-01-01T00:00:00Z".into(),
        ] {
            let mut sources = supplied();
            mutate(sources.get_mut("/workspace").unwrap());
            let error = validate_sources(&declared(), &sources).unwrap_err();
            for secret in ["SECRETVALUE", "TOKENVALUE", "AKIDEXAMPLE"] {
                assert!(!error.message.contains(secret), "{}", error.message);
            }
        }
    }

    #[test]
    fn debug_output_hides_credentials() {
        let text = format!("{:?}", source());
        for secret in ["SECRETVALUE", "TOKENVALUE", "AKIDEXAMPLE"] {
            assert!(!text.contains(secret));
        }
    }
}
