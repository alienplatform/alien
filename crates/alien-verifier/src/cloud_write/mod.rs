//! A controller's own cloud write, checked field by field against the signed release and sent
//! to the cloud unchanged.
//!
//! On a protected install a worker controller builds the same request it builds for an
//! unprotected one and hands it to the verifier instead of calling the cloud. The verifier
//! checks the statements, then every field of the request against the cloud's table (see
//! [`crate::fields`]), sends it as it is, and reads back what decides what runs.

pub mod cloud_run;
pub mod container_apps;
pub mod env;
pub mod lambda;

use alien_signing::{ReleaseBody, SignedStatement, Workload};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alien_error::AlienError;

use crate::error::{ErrorData, Result};
use crate::plan::{functions, Place, Reads};
use crate::trust::Accepted;

/// Apply one cloud write a controller built for a signed workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyWrite {
    /// The release, as the vendor signed it.
    pub release: SignedStatement,
    /// The signer list chain, from version 1.
    pub signer_lists: Vec<SignedStatement>,
    /// This install's config, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<SignedStatement>,
    /// The signed workload the write is for.
    pub workload: String,
    /// The write, exactly as the controller would send it to the cloud.
    pub write: CloudWrite,
}

/// One cloud write, with the request body as the cloud client serialises it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum CloudWrite {
    /// Lambda CreateFunction.
    CreateFunction(Value),
    /// Lambda UpdateFunctionCode.
    #[serde(rename_all = "camelCase")]
    UpdateFunctionCode {
        function_name: String,
        request: Value,
    },
    /// Lambda UpdateFunctionConfiguration.
    #[serde(rename_all = "camelCase")]
    UpdateFunctionConfiguration {
        function_name: String,
        request: Value,
    },
    /// Lambda AddPermission.
    #[serde(rename_all = "camelCase")]
    AddPermission {
        function_name: String,
        request: Value,
    },
    /// Lambda PutFunctionConcurrency.
    #[serde(rename_all = "camelCase")]
    PutFunctionConcurrency {
        function_name: String,
        reserved_concurrent_executions: u32,
    },
    /// Lambda DeleteFunctionConcurrency.
    #[serde(rename_all = "camelCase")]
    DeleteFunctionConcurrency { function_name: String },
    /// Cloud Run services.create, with the service id it is created under.
    #[serde(rename_all = "camelCase")]
    CreateService { service_id: String, service: Value },
    /// Cloud Run services.patch of the whole service.
    #[serde(rename_all = "camelCase")]
    ReplaceService { name: String, service: Value },
    /// Container Apps createOrUpdate.
    #[serde(rename_all = "camelCase")]
    CreateContainerApp { name: String, app: Value },
    /// Container Apps update (PATCH).
    #[serde(rename_all = "camelCase")]
    UpdateContainerApp { name: String, app: Value },
}

impl CloudWrite {
    /// The cloud operation, for messages.
    pub fn operation(&self) -> &'static str {
        match self {
            CloudWrite::CreateFunction(_) => "CreateFunction",
            CloudWrite::UpdateFunctionCode { .. } => "UpdateFunctionCode",
            CloudWrite::UpdateFunctionConfiguration { .. } => "UpdateFunctionConfiguration",
            CloudWrite::AddPermission { .. } => "AddPermission",
            CloudWrite::PutFunctionConcurrency { .. } => "PutFunctionConcurrency",
            CloudWrite::DeleteFunctionConcurrency { .. } => "DeleteFunctionConcurrency",
            CloudWrite::CreateService { .. } => "CreateService",
            CloudWrite::ReplaceService { .. } => "ReplaceService",
            CloudWrite::CreateContainerApp { .. } => "CreateContainerApp",
            CloudWrite::UpdateContainerApp { .. } => "UpdateContainerApp",
        }
    }
}

/// What the verifier knows about the workload a write is for.
pub struct Facts<'a> {
    pub place: &'a Place,
    pub accepted: &'a Accepted,
    pub workload: &'a Workload,
    pub reads: &'a Reads,
    /// `<prefix>-<workload>`, the only name the write may touch.
    pub own_name: String,
}

pub(crate) fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// The signed function workload `name` of `release`, and the name its function has in this
/// install. The verifier's own name is never a workload's.
pub fn own_workload<'a>(
    place: &Place,
    release: &'a ReleaseBody,
    name: &str,
) -> Result<(&'a Workload, String)> {
    let workload = functions(release)
        .find(|workload| workload.name == name)
        .ok_or_else(|| {
            refused(format!(
                "workload '{name}' is not a function of the signed release"
            ))
        })?;
    let own_name = place.function_name(name)?;
    if own_name == format!("{}-verifier", place.prefix) {
        return Err(refused(format!(
            "workload '{name}' would be the install's verifier"
        )));
    }
    Ok((workload, own_name))
}

/// A write that names a function must name the workload's own.
pub fn check_target(facts: &Facts<'_>, named: &str, operation: &str) -> Result<()> {
    if named != facts.own_name {
        return Err(refused(format!(
            "{operation} names function {named}, not {}, the name this install gives workload '{}'",
            facts.own_name, facts.workload.name
        )));
    }
    Ok(())
}
