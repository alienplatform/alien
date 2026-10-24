use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// A managed PostgreSQL database. The target platform decides the backend
/// (AWS Aurora Serverless v2, GCP Cloud SQL, Azure Flexible Server, or an
/// embedded native process on Local); the database is never publicly reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Postgres {
    #[builder(start_fn)]
    pub id: String,
    /// Major engine version: "15" | "16" | "17". Default "17".
    #[builder(default = default_version())]
    #[serde(default = "default_version")]
    #[cfg_attr(feature = "openapi", schema(default = default_version))]
    pub version: String,
    /// Requested vCPUs (e.g. "0.5", "2"). None = smallest available tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    /// Requested memory (e.g. "1Gi", "8Gi"). None = smallest available tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// Allocated storage (e.g. "20Gi"). Default "20Gi". Grow-only.
    #[builder(default = default_storage())]
    #[serde(default = "default_storage")]
    #[cfg_attr(feature = "openapi", schema(default = default_storage))]
    pub storage: String,
    /// Multi-AZ / regional / zone-redundant high availability. Default false.
    #[builder(default)]
    #[serde(default)]
    pub high_availability: bool,
    /// Key used to encrypt database storage, snapshots, and performance data.
    /// Cannot be changed after creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<ResourceRef>,
    /// Cloud backend selector; None resolves to the platform default. The field
    /// exists so a single per-cloud controller can dispatch on it without a later
    /// schema change — the registry allows only one controller per (resource, cloud).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<PostgresBackend>,
}

/// Cloud backend for a Postgres resource. Only Aurora Serverless v2 ships in v1;
/// the enum reserves the slot so provisioned RDS is an additive variant later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PostgresBackend {
    AuroraServerlessV2,
}

impl Postgres {
    /// The resource type identifier for Postgres.
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("postgres");

    /// Aurora parameter groups are tied to the engine's major version, even when a
    /// specific minor release is requested.
    pub fn aurora_parameter_group_family(&self) -> Result<String> {
        let major = self
            .version
            .split('.')
            .next()
            .filter(|major| !major.is_empty() && major.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| {
                AlienError::new(ErrorData::OperationNotSupported {
                    operation: "Aurora parameter group family".to_string(),
                    reason: format!("invalid Postgres engine version '{}'", self.version),
                })
            })?;
        Ok(format!("aurora-postgresql{major}"))
    }

    /// Requested positive finite vCPU count, including millicore quantities.
    pub fn requested_cpu(&self) -> Result<Option<f64>> {
        self.cpu
            .as_deref()
            .map(|value| {
                crate::instance_catalog::parse_cpu(value)
                    .ok()
                    .filter(|cpu| cpu.is_finite() && *cpu > 0.0)
                    .ok_or_else(|| self.invalid_size("cpu", value))
            })
            .transpose()
    }

    /// Requested memory in GiB. Binary and decimal units retain their meanings;
    /// plain numbers denote bytes, as in other resource quantities.
    pub fn requested_memory_gib(&self) -> Result<Option<f64>> {
        self.memory
            .as_deref()
            .map(|value| self.size_gib("memory", value))
            .transpose()
    }

    /// Requested disk size in GiB, rounded up to a whole provider allocation unit.
    pub fn requested_storage_gib(&self) -> Result<u64> {
        Ok(self.size_gib("storage", &self.storage)?.ceil() as u64)
    }

    fn size_gib(&self, field: &str, value: &str) -> Result<f64> {
        let quantity = value.trim();
        // Accept the conventional GiB/GB spellings as aliases of Gi/G.
        let quantity = quantity.strip_suffix('B').unwrap_or(quantity);
        crate::instance_catalog::parse_memory_bytes(quantity)
            .ok()
            .filter(|bytes| *bytes > 0)
            .map(|bytes| bytes as f64 / 1024_f64.powi(3))
            .ok_or_else(|| self.invalid_size(field, value))
    }

    fn invalid_size(&self, field: &str, value: &str) -> alien_error::AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("Postgres {field} sizing"),
            reason: format!("'{value}' must be a positive finite resource quantity"),
        })
    }

    /// Aurora Serverless v2 ceiling in half-ACU increments (2 GiB per ACU).
    /// The supported engine configuration allows up to 256 ACU; never clamp a
    /// larger request downward. The minimum ceiling is 1 ACU. An unspecified ceiling remains 2 ACU.
    pub fn aurora_max_capacity_acu(&self) -> Result<f64> {
        let Some(gib) = self.requested_memory_gib()? else {
            return Ok(2.0);
        };
        let capacity = (gib.ceil() / 2.0).max(1.0);
        if capacity > 256.0 {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "Aurora memory sizing".to_string(),
                reason: "request exceeds the supported maximum of 512 GiB (256 ACU)".to_string(),
            }));
        }
        Ok(capacity)
    }

    /// Returns the database's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

fn default_version() -> String {
    "17".to_string()
}

fn default_storage() -> String {
    "20Gi".to_string()
}

/// The fixed inner database name for **cloud** Postgres backends.
/// AWS rejects hyphens in `DatabaseName`, and the cluster/instance/server name already
/// carries the resource id, so the inner database is a fixed label rather than the id.
/// Every cloud emitter and controller derives its database name from this single constant
/// so a frozen import always binds to a database that exists. (Local names its database
/// after the resource id and does not use this.)
pub const POSTGRES_DATABASE_NAME: &str = "app";

/// The administrator created for managed cloud Postgres databases.
pub const POSTGRES_ADMIN_USERNAME: &str = "app_admin";

/// Outputs generated by a successfully provisioned Postgres database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PostgresOutputs {
    /// Host (or host:port) the database listens on; never publicly reachable.
    pub endpoint: String,
    /// The default database created for this resource. On cloud backends this is the fixed
    /// label `POSTGRES_DATABASE_NAME` (`app`); on Local it is the resource id.
    pub database: String,
    /// Listening port, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

impl ResourceOutputsDefinition for PostgresOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Postgres::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<PostgresOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for Postgres {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn replace_after_failed_create_is_safe(&self) -> bool {
        // Deleting it destroys the database contents.
        false
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // The network dependency is added by infrastructure preflights on cloud platforms and
        // is irrelevant on Local; cloud controllers reach the network via
        // require_dependency() at runtime, so nothing is declared statically here.
        self.encryption_key.iter().cloned().collect()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_pg = new_config
            .as_any()
            .downcast_ref::<Postgres>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_pg.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        if self.encryption_key != new_pg.encryption_key {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'encryptionKey' field is immutable once the resource exists"
                    .to_string(),
            }));
        }

        // `backend` selects the physical engine, so it is immutable once the resource exists. Reject
        // ANY change including `None -> Some(...)`: `None` already resolves to the platform default, and
        // once a second variant exists a `None -> Some(other)` edit would silently switch the live engine.
        if self.backend != new_pg.backend {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'backend' field is immutable once the resource exists".to_string(),
            }));
        }

        // Storage and version are monotonic: no cloud supports a volume shrink or an in-place
        // engine downgrade, so reject rather than let Update reach an impossible state. Parse
        // explicitly and fail on a malformed value — a value we can't parse must NOT silently skip
        // the guard, or a shrink/downgrade would slip through unchecked (fail-open).
        let old_storage =
            crate::instance_catalog::parse_memory_bytes(&self.storage).map_err(|e| {
                AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: format!(
                        "current storage '{}' is not a valid size: {e}",
                        self.storage
                    ),
                })
            })?;
        let new_storage =
            crate::instance_catalog::parse_memory_bytes(&new_pg.storage).map_err(|e| {
                AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: format!(
                        "requested storage '{}' is not a valid size: {e}",
                        new_pg.storage
                    ),
                })
            })?;
        if new_storage < old_storage {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: format!(
                    "storage cannot shrink (from '{}' to '{}')",
                    self.storage, new_pg.storage
                ),
            }));
        }

        let old_version = self.version.parse::<u32>().map_err(|_| {
            AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: format!(
                    "current engine version '{}' is not a numeric major",
                    self.version
                ),
            })
        })?;
        let new_version = new_pg.version.parse::<u32>().map_err(|_| {
            AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: format!(
                    "requested engine version '{}' is not a numeric major",
                    new_pg.version
                ),
            })
        })?;
        if new_version < old_version {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: format!(
                    "engine version cannot downgrade (from '{}' to '{}')",
                    self.version, new_pg.version
                ),
            }));
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Postgres>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aurora_family_uses_the_major_of_pinned_minor_versions() {
        for (version, family) in [
            ("15", "aurora-postgresql15"),
            ("16.8", "aurora-postgresql16"),
            ("17.7", "aurora-postgresql17"),
        ] {
            let pg = Postgres::new("db".to_string())
                .version(version.to_string())
                .build();
            assert_eq!(pg.aurora_parameter_group_family().unwrap(), family);
        }
        for version in ["", ".17", "latest", "17x"] {
            let pg = Postgres::new("db".to_string())
                .version(version.to_string())
                .build();
            assert!(pg.aurora_parameter_group_family().is_err());
        }
    }

    #[test]
    fn builder_applies_defaults() {
        let pg = Postgres::new("db".to_string()).build();
        assert_eq!(pg.id, "db");
        assert_eq!(pg.version, "17");
        assert_eq!(pg.storage, "20Gi");
        assert!(!pg.high_availability);
        assert_eq!(pg.cpu, None);
        assert_eq!(pg.backend, None);
    }

    #[test]
    fn resource_type_is_postgres() {
        assert_eq!(Postgres::RESOURCE_TYPE.as_ref(), "postgres");
    }

    #[test]
    fn validate_update_rejects_id_change() {
        let original = Postgres::new("db".to_string()).build();
        let renamed = Postgres::new("other".to_string()).build();
        let err = original
            .validate_update(&renamed)
            .expect_err("changing the id must be rejected");
        assert!(err.to_string().contains("'id' field is immutable"));
    }

    #[test]
    fn validate_update_rejects_backend_change_including_none_to_some() {
        // `None` already resolves to the platform default, so even making it explicit is rejected —
        // backend is pinned once the resource exists.
        let default_backend = Postgres::new("db".to_string()).build();
        let explicit = Postgres::new("db".to_string())
            .backend(PostgresBackend::AuroraServerlessV2)
            .build();
        let err = default_backend
            .validate_update(&explicit)
            .expect_err("None -> Some(backend) must be rejected");
        assert!(err.to_string().contains("'backend' field is immutable"));
        // A no-op (unchanged backend) is allowed.
        assert!(explicit.validate_update(&explicit).is_ok());
    }

    #[test]
    fn validate_update_rejects_storage_shrink_and_allows_growth() {
        let original = Postgres::new("db".to_string())
            .storage("100Gi".to_string())
            .build();
        let shrunk = Postgres::new("db".to_string())
            .storage("20Gi".to_string())
            .build();
        let grown = Postgres::new("db".to_string())
            .storage("200Gi".to_string())
            .build();

        let err = original
            .validate_update(&shrunk)
            .expect_err("shrinking storage must be rejected");
        assert!(err.to_string().contains("storage cannot shrink"));
        assert!(original.validate_update(&grown).is_ok());
    }

    #[test]
    fn validate_update_rejects_version_downgrade_and_allows_upgrade() {
        let original = Postgres::new("db".to_string())
            .version("16".to_string())
            .build();
        let downgrade = Postgres::new("db".to_string())
            .version("15".to_string())
            .build();
        let upgrade = Postgres::new("db".to_string())
            .version("17".to_string())
            .build();

        let err = original
            .validate_update(&downgrade)
            .expect_err("version downgrade must be rejected");
        assert!(err.to_string().contains("engine version cannot downgrade"));
        assert!(original.validate_update(&upgrade).is_ok());
    }

    #[test]
    fn serializes_with_camel_case_and_backend() {
        let pg = Postgres::new("db".to_string())
            .backend(PostgresBackend::AuroraServerlessV2)
            .high_availability(true)
            .build();
        let json = serde_json::to_value(&pg).unwrap();
        assert_eq!(json["highAvailability"], true);
        assert_eq!(json["backend"]["type"], "auroraServerlessV2");

        let roundtrip: Postgres = serde_json::from_value(json).unwrap();
        assert_eq!(pg, roundtrip);
    }

    #[test]
    fn outputs_roundtrip() {
        let outputs = PostgresOutputs {
            endpoint: "127.0.0.1".to_string(),
            database: "db".to_string(),
            port: Some(5432),
        };
        let json = serde_json::to_string(&outputs).unwrap();
        let deserialized: PostgresOutputs = serde_json::from_str(&json).unwrap();
        assert_eq!(outputs, deserialized);
    }
}

#[cfg(test)]
mod sizing_tests {
    use super::*;

    #[test]
    fn memory_units_and_aurora_half_steps_agree() {
        for (quantity, gib, acu) in [
            ("512Mi", 0.5, 1.0),
            ("1Gi", 1.0, 1.0),
            ("1.5Gi", 1.5, 1.0),
            ("3Gi", 3.0, 1.5),
            ("3.9Gi", 3.9, 2.0),
            ("8Gi", 8.0, 4.0),
            ("8GiB", 8.0, 4.0),
            ("512Gi", 512.0, 256.0),
            ("1073741824", 1.0, 1.0),
            ("1GB", 1_000_000_000.0 / 1_073_741_824.0, 1.0),
        ] {
            let pg = Postgres::new("db".into()).memory(quantity.into()).build();
            assert!(
                (pg.requested_memory_gib().unwrap().unwrap() - gib).abs() < 1e-8,
                "{quantity}"
            );
            assert_eq!(pg.aurora_max_capacity_acu().unwrap(), acu, "{quantity}");
        }
        assert_eq!(
            Postgres::new("db".into())
                .build()
                .aurora_max_capacity_acu()
                .unwrap(),
            2.0
        );
    }

    #[test]
    fn sizing_rejects_invalid_nonpositive_nonfinite_and_overflow() {
        for quantity in [
            "", "0", "0Gi", "-1Gi", "NaNGi", "infGi", "1e100Gi", "8unknown",
        ] {
            let pg = Postgres::new("db".into()).memory(quantity.into()).build();
            assert!(pg.requested_memory_gib().is_err(), "{quantity}");
            assert!(pg.aurora_max_capacity_acu().is_err(), "{quantity}");
        }
        assert!(Postgres::new("db".into())
            .memory("513Gi".into())
            .build()
            .aurora_max_capacity_acu()
            .is_err());
        for cpu in ["0", "-1", "NaN", "inf", "invalid"] {
            assert!(
                Postgres::new("db".into())
                    .cpu(cpu.into())
                    .build()
                    .requested_cpu()
                    .is_err(),
                "{cpu}"
            );
        }
        assert_eq!(
            Postgres::new("db".into())
                .cpu("500m".into())
                .build()
                .requested_cpu()
                .unwrap(),
            Some(0.5)
        );
        assert_eq!(
            Postgres::new("db".into())
                .storage("1536Mi".into())
                .build()
                .requested_storage_gib()
                .unwrap(),
            2
        );
    }
}
