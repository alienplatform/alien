//! Who writes a service account's IAM role, and how: the one decision every write of a
//! service-account role goes through (create, update, grants, stale-grant removal, delete).
//!
//! Every service-account role is written under the deployment's permissions boundary and tagged
//! `managed-by=runtime`, so management can keep it current. The one exception is the role of the
//! sandbox egress gateway Alien manages: the deployment boundary denies every role under it the
//! gateway configuration, which the gateway itself reads. That role is written as the
//! CloudFormation and Terraform setups write it: unbounded, tagged `managed-by=setup`, and only
//! with setup credentials. Management cannot adopt or change it.

use alien_aws_clients::iam::{IamApi, IamRoleWriter};
use alien_core::{
    sandbox_egress::{is_managed_gateway, SANDBOX_EGRESS_GATEWAY_ID},
    Container, Stack,
};

use crate::core::ResourceControllerContext;
use crate::error::Result;

/// How a service account's IAM role is written.
pub(crate) struct AwsServiceAccountRoleWrite {
    /// The deployment's permissions boundary, if it has one. A setup-owned role is never
    /// written under it; the writer removes it where an earlier release attached it.
    deployment_boundary: Option<String>,
    setup_owned: bool,
}

impl AwsServiceAccountRoleWrite {
    /// The write mode for the role of service account `service_account_id` in this deployment.
    pub(crate) fn for_service_account(
        ctx: &ResourceControllerContext<'_>,
        service_account_id: &str,
    ) -> Result<Self> {
        Ok(Self {
            deployment_boundary: ctx.aws_permissions_boundary_arn()?,
            setup_owned: is_managed_gateway_service_account(ctx.desired_stack, service_account_id),
        })
    }

    /// The mode a service account controller recorded for its own role, or the stack's
    /// decision. A role once written setup-owned stays so while it is deleted, when the
    /// stack may no longer hold the gateway.
    pub(crate) fn for_own_role(
        ctx: &ResourceControllerContext<'_>,
        service_account_id: &str,
        recorded_setup_owned: bool,
    ) -> Result<Self> {
        let mut write = Self::for_service_account(ctx, service_account_id)?;
        write.setup_owned |= recorded_setup_owned;
        Ok(write)
    }

    pub(crate) fn setup_owned(&self) -> bool {
        self.setup_owned
    }

    pub(crate) fn writer<'a>(&'a self, iam: &'a dyn IamApi) -> IamRoleWriter<'a> {
        if self.setup_owned {
            IamRoleWriter::setup_owned(
                iam,
                self.deployment_boundary.as_deref(),
                service_account_type(),
            )
        } else {
            IamRoleWriter::new(iam, self.deployment_boundary.as_deref())
        }
    }
}

/// `ServiceAccount::RESOURCE_TYPE`, as the `resource-type` tag the setups write.
fn service_account_type() -> &'static str {
    "service-account"
}

/// Whether `service_account_id` is the service account of the sandbox egress gateway Alien
/// manages for this stack: the stack has that gateway (it has an internet sandbox), and the
/// account is the one of the gateway's own permission profile. Preparation refuses an
/// application profile with the gateway's name, and any other workload on the gateway's profile.
pub(crate) fn is_managed_gateway_service_account(stack: &Stack, service_account_id: &str) -> bool {
    if !is_managed_gateway(stack, SANDBOX_EGRESS_GATEWAY_ID) {
        return false;
    }
    let Some(gateway) = stack
        .resources
        .get(SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|entry| entry.config.downcast_ref::<Container>())
    else {
        return false;
    };
    service_account_id.strip_suffix("-sa") == Some(gateway.permissions.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::ServiceAccount;

    #[test]
    fn setup_tags_name_the_service_account_type() {
        assert_eq!(
            service_account_type(),
            ServiceAccount::RESOURCE_TYPE.as_ref()
        );
    }
}
