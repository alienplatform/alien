//! The resource types a stack file can declare, and how each one is read.
//!
//! Frontends that describe a stack in a file (`alien.toml`) need, per resource type, the
//! name of its table, its default lifecycle and how to turn a JSON body into the concrete
//! alien-core type. That knowledge lives here once, so a frontend gains a new resource type
//! by adding one entry.

use std::fmt;

use serde::de::DeserializeOwned;

use crate::resources::{
    Ai, ArtifactRegistry, AwsOpenSearch, Build, Container, Daemon, Email, Key, Kv, Postgres, Queue,
    Sandbox, ServiceAccount, Storage, Vault, Worker,
};
use crate::{Resource, ResourceDefinition, ResourceLifecycle};

/// A resource body that does not deserialize into its alien-core type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBodyError {
    /// Where in the body the problem is, for example `cpu.min` or `ports[0].port`. Empty when
    /// it concerns the body as a whole.
    pub path: String,
    /// What serde reported.
    pub message: String,
}

impl fmt::Display for ResourceBodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// One resource type a stack file can declare.
pub struct AuthorableType {
    /// Table name in a file, `daemon` in `[daemon.<id>]`. A `.` stands for the `/` of an
    /// experimental type (`experimental.aws-opensearch`).
    pub table: &'static str,
    /// The `ResourceType` string, as stacks carry it.
    pub resource_type: &'static str,
    /// Name of the config type in the generated OpenAPI document.
    pub schema: &'static str,
    /// Lifecycle a resource gets when the file does not say. Workloads are `live`, everything
    /// else is `frozen`.
    pub default_lifecycle: ResourceLifecycle,
    /// Whether the type runs on the compute cluster and so carries a `cluster` field.
    pub takes_cluster: bool,
    deserialize: fn(serde_json::Value) -> Result<Resource, ResourceBodyError>,
}

impl AuthorableType {
    /// Deserializes `body`, a resource without its `type` tag, into the concrete config type.
    ///
    /// Unlike deserializing a whole [`Resource`], the error carries the path of the offending
    /// field, so a file frontend can point at it.
    pub fn deserialize(&self, body: serde_json::Value) -> Result<Resource, ResourceBodyError> {
        (self.deserialize)(body)
    }
}

fn typed<T: ResourceDefinition + DeserializeOwned>(
    body: serde_json::Value,
) -> Result<Resource, ResourceBodyError> {
    serde_path_to_error::deserialize::<_, T>(body)
        .map(Resource::new)
        .map_err(|error| ResourceBodyError {
            path: match error.path().to_string().as_str() {
                "." => String::new(),
                path => path.to_string(),
            },
            message: error.inner().to_string(),
        })
}

macro_rules! authorable {
    ($table:literal, $resource_type:literal, $schema:literal, $ty:ty, $lifecycle:ident, $cluster:literal) => {
        AuthorableType {
            table: $table,
            resource_type: $resource_type,
            schema: $schema,
            default_lifecycle: ResourceLifecycle::$lifecycle,
            takes_cluster: $cluster,
            deserialize: typed::<$ty>,
        }
    };
}

/// Every resource type a stack file can declare, in the order the documentation lists them.
///
/// The `build` resource has the table name `build-environment`, because `build` is the key
/// that says how a workload's code is built.
pub static AUTHORABLE_TYPES: &[AuthorableType] = &[
    authorable!("container", "container", "Container", Container, Live, true),
    authorable!("daemon", "daemon", "Daemon", Daemon, Live, true),
    authorable!("worker", "worker", "Worker", Worker, Live, false),
    authorable!("build-environment", "build", "Build", Build, Live, false),
    authorable!("storage", "storage", "Storage", Storage, Frozen, false),
    authorable!("kv", "kv", "Kv", Kv, Frozen, false),
    authorable!("queue", "queue", "Queue", Queue, Frozen, false),
    authorable!("vault", "vault", "Vault", Vault, Frozen, false),
    authorable!("key", "key", "Key", Key, Frozen, false),
    authorable!("postgres", "postgres", "Postgres", Postgres, Frozen, false),
    authorable!("email", "email", "Email", Email, Frozen, false),
    authorable!("ai", "ai", "Ai", Ai, Frozen, false),
    authorable!("sandbox", "sandbox", "Sandbox", Sandbox, Frozen, false),
    authorable!(
        "artifact-registry",
        "artifact-registry",
        "ArtifactRegistry",
        ArtifactRegistry,
        Frozen,
        false
    ),
    authorable!(
        "service-account",
        "service-account",
        "ServiceAccount",
        ServiceAccount,
        Frozen,
        false
    ),
    authorable!(
        "experimental.aws-opensearch",
        "experimental/aws-opensearch",
        "AwsOpenSearch",
        AwsOpenSearch,
        Frozen,
        false
    ),
];

/// Resource types in the generated gateability manifest that are not declared in a file.
///
/// Empty today: every manifest type is authorable. The list exists so the registry test can
/// say why a manifest type would be missing rather than silently skipping it.
pub const MANIFEST_TYPES_NOT_AUTHORABLE: &[&str] = &[];

/// Authorable types that are not in the gateability manifest, because they cannot carry an
/// `enabled-when` gate: `build`, `artifact-registry` and `service-account` are derived from
/// the stack by Alien, and `key` is a plain customer-managed key.
pub const AUTHORABLE_TYPES_NOT_IN_MANIFEST: &[&str] =
    &["build", "key", "artifact-registry", "service-account"];

/// The authorable type with this table name.
pub fn authorable_type_by_table(table: &str) -> Option<&'static AuthorableType> {
    AUTHORABLE_TYPES.iter().find(|ty| ty.table == table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateability::MANIFEST_TYPES;
    use serde_json::json;

    fn minimal_body(resource_type: &str) -> serde_json::Value {
        let code = json!({ "type": "image", "image": "example/app:1" });
        match resource_type {
            "container" => json!({
                "id": "x", "links": [], "ports": [], "code": code, "permissions": "p",
                "cpu": { "min": "1", "desired": "1" }, "memory": { "min": "1Gi", "desired": "1Gi" }
            }),
            "daemon" => json!({ "id": "x", "links": [], "code": code }),
            "worker" => {
                json!({ "id": "x", "links": [], "triggers": [], "code": code, "permissions": "p" })
            }
            "build" => json!({ "id": "x", "links": [], "permissions": "p" }),
            "sandbox" => json!({ "id": "x", "code": code, "egress": "none", "lifecycle": {} }),
            "email" => json!({ "id": "x", "domains": [] }),
            "postgres" => {
                json!({ "id": "x", "version": "17", "storage": "20Gi", "highAvailability": false })
            }
            "service-account" => json!({ "id": "x", "stackPermissionSets": [] }),
            "experimental/aws-opensearch" => json!({ "id": "x" }),
            _ => json!({ "id": "x" }),
        }
    }

    #[test]
    fn registry_covers_every_manifest_type() {
        for manifest in MANIFEST_TYPES {
            let registered = AUTHORABLE_TYPES
                .iter()
                .any(|ty| ty.resource_type == *manifest);
            let excluded = MANIFEST_TYPES_NOT_AUTHORABLE.contains(manifest);
            assert!(
                registered != excluded,
                "gateability manifest type '{manifest}' must be in AUTHORABLE_TYPES, or listed \
                 in MANIFEST_TYPES_NOT_AUTHORABLE with a reason (registered: {registered})"
            );
        }
    }

    #[test]
    fn registry_extras_are_documented() {
        let mut extras: Vec<&str> = AUTHORABLE_TYPES
            .iter()
            .map(|ty| ty.resource_type)
            .filter(|resource_type| !MANIFEST_TYPES.contains(resource_type))
            .collect();
        extras.sort_unstable();
        let mut documented = AUTHORABLE_TYPES_NOT_IN_MANIFEST.to_vec();
        documented.sort_unstable();
        assert_eq!(extras, documented);
    }

    #[test]
    fn table_names_and_resource_types_are_unique() {
        for (index, ty) in AUTHORABLE_TYPES.iter().enumerate() {
            for other in &AUTHORABLE_TYPES[..index] {
                assert_ne!(ty.table, other.table);
                assert_ne!(ty.resource_type, other.resource_type);
                assert_ne!(ty.schema, other.schema);
            }
        }
    }

    #[test]
    fn every_type_deserializes_into_its_own_resource_type() {
        for ty in AUTHORABLE_TYPES {
            let resource = ty
                .deserialize(minimal_body(ty.resource_type))
                .unwrap_or_else(|error| panic!("{}: {error}", ty.table));
            assert_eq!(resource.resource_type().0, ty.resource_type, "{}", ty.table);
            assert_eq!(resource.id(), "x", "{}", ty.table);
        }
    }

    #[test]
    fn only_workloads_are_live_and_only_cluster_workloads_take_a_cluster() {
        let live: Vec<&str> = AUTHORABLE_TYPES
            .iter()
            .filter(|ty| ty.default_lifecycle == ResourceLifecycle::Live)
            .map(|ty| ty.table)
            .collect();
        assert_eq!(live, ["container", "daemon", "worker", "build-environment"]);
        let clustered: Vec<&str> = AUTHORABLE_TYPES
            .iter()
            .filter(|ty| ty.takes_cluster)
            .map(|ty| ty.table)
            .collect();
        assert_eq!(clustered, ["container", "daemon"]);
    }

    #[test]
    fn body_errors_carry_the_field_path() {
        let daemon = authorable_type_by_table("daemon").unwrap();
        let mut body = minimal_body("daemon");
        body["healthCheck"] = json!({ "port": "eighty" });
        let error = daemon
            .deserialize(body)
            .expect_err("a string is not a port");
        assert_eq!(error.path, "healthCheck.port");
        assert!(error.message.contains("invalid type"), "{}", error.message);

        let mut body = minimal_body("daemon");
        body["mystery"] = json!(1);
        let error = daemon.deserialize(body).expect_err("unknown field");
        assert_eq!(error.path, "mystery");
        assert!(error.message.contains("unknown field"), "{}", error.message);
        assert!(error.message.contains("mystery"), "{}", error.message);
    }
}
