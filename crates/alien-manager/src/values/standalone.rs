//! Value access for a standalone manager: callers use the same tokens and
//! deployment rights as for every other endpoint.

use std::sync::Arc;

use alien_error::AlienError;
use async_trait::async_trait;
use axum::http::HeaderMap;

use super::{ValueAccess, ValueAccessAuthorizer, ValueGrant, ValueScope};
use crate::auth::{Authz, Subject};
use crate::error::{ErrorData, Result};
use crate::traits::{AuthValidator, DeploymentStore};

pub struct StandaloneValueAccess {
    auth_validator: Arc<dyn AuthValidator>,
    authz: Arc<dyn Authz>,
    deployment_store: Arc<dyn DeploymentStore>,
}

impl StandaloneValueAccess {
    pub fn new(
        auth_validator: Arc<dyn AuthValidator>,
        authz: Arc<dyn Authz>,
        deployment_store: Arc<dyn DeploymentStore>,
    ) -> Self {
        Self {
            auth_validator,
            authz,
            deployment_store,
        }
    }

    async fn subject(&self, headers: &HeaderMap) -> Result<Subject> {
        let validated = self
            .auth_validator
            .validate(headers)
            .await
            .map_err(crate::routes::auth::validation_error)?;
        validated.ok_or_else(|| ErrorData::unauthorized("Authentication required"))
    }
}

#[async_trait]
impl ValueAccessAuthorizer for StandaloneValueAccess {
    async fn authorize(
        &self,
        headers: &HeaderMap,
        scope: &ValueScope,
        needed: ValueAccess,
    ) -> std::result::Result<ValueGrant, AlienError<ErrorData>> {
        let subject = self.subject(headers).await?;
        let Some(deployment_id) = scope.deployment_id() else {
            return Err(ErrorData::forbidden(format!(
                "This manager keeps values for deployments only, not for '{scope}'"
            )));
        };
        if needed == ValueAccess::Migrate {
            return Err(ErrorData::forbidden(
                "This manager does not support migrating values",
            ));
        }
        let deployment = self
            .deployment_store
            .get_deployment(&subject, deployment_id)
            .await
            .map_err(|e| ErrorData::internal(e.message))?
            .ok_or_else(|| ErrorData::not_found_deployment(deployment_id))?;
        let allowed = match needed {
            ValueAccess::Read => self.authz.can_read_deployment(&subject, &deployment),
            ValueAccess::Write => self.authz.can_update_deployment(&subject, &deployment),
            ValueAccess::Migrate => false,
        };
        if !allowed {
            return Err(ErrorData::forbidden(format!(
                "You may not {} values of deployment '{deployment_id}'",
                needed.as_str()
            )));
        }
        Ok(ValueGrant::all_slots(needed))
    }
}
