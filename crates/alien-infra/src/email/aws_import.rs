//! Importer for AWS Email (SES).
//!
//! Maps the typed [`AwsEmailImportData`] payload emitted by the
//! CloudFormation generator's `emit_import_ref` into an
//! [`AwsEmailController`] pinned at its terminal `Ready` state. Like every
//! importer this is a pure data mapping — no SES calls, no liveness
//! verification; the outputs claim exactly what setup handed over.

use alien_core::{
    import::{data::AwsEmailImportData, ImportContext},
    EmailDkimToken, EmailDomainOutputs, Result, StackResourceState,
};

use crate::email::{AwsEmailController, AwsEmailState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// AWS SES email importer.
#[derive(Debug, Default)]
pub struct AwsEmailImporter;

impl ResourceImporter for AwsEmailImporter {
    type ImportData = AwsEmailImportData;

    fn import(
        &self,
        data: AwsEmailImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let domains = data
            .domains
            .into_iter()
            .map(|(domain, domain_data)| {
                let dkim_tokens = domain_data
                    .dkim_tokens
                    .into_iter()
                    .map(|token| EmailDkimToken {
                        name: token.name,
                        value: token.value,
                    })
                    .collect();
                (domain, EmailDomainOutputs { dkim_tokens })
            })
            .collect();

        let controller = AwsEmailController {
            state: AwsEmailState::Ready,
            configuration_set: Some(data.configuration_set),
            domains,
            rule_set_name: data.rule_set_name,
            region: Some(ctx.region.to_string()),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
