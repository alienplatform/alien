//! Importer for AWS Worker (Lambda).

use alien_core::{
    import::{data::AwsWorkerImportData, ImportContext},
    Result, StackResourceState,
};

use crate::worker::{AwsWorkerController, AwsWorkerState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// AWS Lambda worker importer.
///
/// Public ingress (API Gateway HTTP API + custom domain + ALB) attaches
/// when the corresponding ImportData fields are populated. Custom domains
/// arriving via `BringYourOwn` certificates are not distinguishable from
/// stack-managed certs at this layer — `uses_custom_domain` is left false
/// and the heartbeat path discovers it via the deployment-config domain
/// metadata.
#[derive(Debug, Default)]
pub struct AwsWorkerImporter;

impl ResourceImporter for AwsWorkerImporter {
    type ImportData = AwsWorkerImportData;

    fn import(
        &self,
        data: AwsWorkerImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsWorkerController {
            state: AwsWorkerState::Ready,
            arn: Some(data.function_arn),
            url: data.url,
            worker_name: Some(data.function_name),
            event_source_mappings: data.event_source_mappings,
            // Domain / TLS metadata is rebuilt by the controller at heartbeat
            // time from `DeploymentConfig::domain_metadata`; ImportData only
            // carries identifiers, not certificate ARNs.
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            api_id: data.api_id,
            integration_id: data.integration_id,
            route_id: data.route_id,
            stage_name: data.stage_name,
            api_mapping_id: None,
            domain_name: None,
            load_balancer: None,
            certificate_issued_at: None,
            uses_custom_domain: false,
            s3_permission_statement_ids: data.s3_permission_statement_ids,
            eventbridge_rule_names: data.eventbridge_rule_names,
            eventbridge_permission_statement_ids: data.eventbridge_permission_statement_ids,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
