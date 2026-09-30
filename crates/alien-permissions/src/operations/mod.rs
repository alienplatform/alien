//! Reviewed operation capabilities and installation-scoped policy compilation.
//! This module has no infrastructure, network, or account dependency.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use alien_error::AlienError;
use alien_permission_types::{
    BindingConfiguration, GcpBindingSpec, PermissionSet, PermissionSetReference,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ErrorData, Result};

pub mod aws;
pub mod catalog;
pub mod gcp;
pub mod kubernetes;
pub mod types;

pub use types::*;

#[derive(Deserialize)]
struct Capability {
    actions: Vec<String>,
    resources: Vec<String>,
}
#[derive(Deserialize)]
struct ReviewedCatalog {
    aws: BTreeMap<String, Capability>,
    gcp: BTreeMap<String, String>,
}
fn catalog() -> &'static ReviewedCatalog {
    static CATALOG: OnceLock<ReviewedCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("catalog.json")).expect("reviewed catalog is valid")
    })
}

fn invalid<T>(message: impl Into<String>, action: &str, resource: &str) -> Result<T> {
    Err(AlienError::new(ErrorData::OperationPermissionInvalid {
        message: message.into(),
        action: action.into(),
        resource: resource.into(),
    }))
}

fn sorted(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
}

#[derive(Deserialize)]
#[serde(
    tag = "task",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Request {
    ResolvePlugin {
        plugin: DeclaredPlugin,
        #[serde(default)]
        origin: PluginOrigin,
    },
    RejectInlineAws {
        references: Vec<PermissionSetReference>,
    },
    AwsForCatalog {
        permission_sets: Vec<PermissionSet>,
    },
    ResolveAwsReferences {
        references: Vec<String>,
    },
    GcpScope {
        binding: BindingConfiguration<GcpBindingSpec>,
    },
    ValidateSets {
        permission_sets: Vec<PermissionSet>,
    },
    ValidateReferences {
        references: Vec<PermissionSetReference>,
    },
    UnreviewedReferences {
        references: Vec<PermissionSetReference>,
    },
    ResolveKubernetes {
        references: Vec<String>,
        declared_permissions: Option<KubernetesPermissions>,
        tier: String,
        attribution: Option<Attribution>,
    },
    ValidateKubernetes {
        permissions: KubernetesPermissions,
        tier: String,
        attribution: Option<Attribution>,
    },
    CollectAws {
        plugins: Vec<CatalogPlugin>,
    },
    CompactAws {
        statements: Vec<AwsStatement>,
    },
    CompileAws {
        statements: Vec<AwsStatement>,
        ceilings: AwsResourceCeilings,
    },
    CollectGcp {
        plugins: Vec<CatalogPlugin>,
    },
    CompileGcp {
        grants: Vec<GcpGrant>,
        ceilings: GcpResourceCeilings,
    },
    CollectKubernetes {
        plugins: Vec<CatalogPlugin>,
    },
    CompileKubernetes {
        grants: Vec<KubernetesGrant>,
        mode: KubernetesMode,
    },
}

fn value(value: impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| {
        AlienError::new(ErrorData::SerializationError {
            message: error.to_string(),
        })
    })
}

pub fn execute(request: Request) -> Result<Value> {
    match request {
        Request::ResolvePlugin { plugin, origin } => {
            value(catalog::resolve_plugin(plugin, origin)?)
        }
        Request::RejectInlineAws { references } => {
            catalog::reject_inline_aws(&references)?;
            value(true)
        }
        Request::AwsForCatalog { permission_sets } => value(aws::for_catalog(&permission_sets)),
        Request::ResolveAwsReferences { references } => value(catalog::resolve_aws(&references)?),
        Request::GcpScope { binding } => value(catalog::gcp_scope(&binding)?),
        Request::ValidateSets { permission_sets } => {
            catalog::validate_sets(&permission_sets)?;
            value(true)
        }
        Request::ValidateReferences { references } => {
            catalog::validate_references(&references)?;
            value(true)
        }
        Request::UnreviewedReferences { references } => value(catalog::unreviewed(&references)),
        Request::ResolveKubernetes {
            references,
            declared_permissions,
            tier,
            attribution,
        } => value(kubernetes::resolve(
            &references,
            declared_permissions,
            &tier,
            attribution.as_ref(),
        )?),
        Request::ValidateKubernetes {
            permissions,
            tier,
            attribution,
        } => {
            kubernetes::validate(&permissions, &tier, attribution.as_ref())?;
            value(permissions)
        }
        Request::CollectAws { plugins } => value(aws::collect(&plugins)),
        Request::CompactAws { statements } => value(aws::compact(&statements)),
        Request::CompileAws {
            statements,
            ceilings,
        } => value(aws::compile(&statements, &ceilings)?),
        Request::CollectGcp { plugins } => value(gcp::collect(&plugins)),
        Request::CompileGcp { grants, ceilings } => value(gcp::compile(&grants, &ceilings)?),
        Request::CollectKubernetes { plugins } => value(kubernetes::collect(&plugins)),
        Request::CompileKubernetes { grants, mode } => value(kubernetes::compile(&grants, mode)?),
    }
}

pub fn request_json(request: &str) -> String {
    let result = serde_json::from_str::<Request>(request)
        .map_err(|error| {
            AlienError::new(ErrorData::SerializationError {
                message: error.to_string(),
            })
        })
        .and_then(execute);
    match result {
        Ok(result) => serde_json::json!({"ok": true, "value": result}).to_string(),
        Err(error) => serde_json::json!({"ok": false, "error": error}).to_string(),
    }
}

#[cfg(test)]
mod tests;
