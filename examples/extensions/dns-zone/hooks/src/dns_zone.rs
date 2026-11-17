use acme_types::{
    AwsDnsZoneFields, AwsDnsZoneImport, AzureDnsZoneFields, AzureDnsZoneImport, DnsZone,
    DnsZoneOutputs, GcpDnsZoneFields, GcpDnsZoneImport,
};
use alien_extension_sdk::prelude::*;

pub struct AwsCfEmitter;

impl CfEmitter for AwsCfEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let zone = resource_config::<DnsZone>(ctx, DnsZone::RESOURCE_TYPE)?;
        let mut hosted_zone = CfResource::new(
            required_logical_id(ctx)?.to_string(),
            "AWS::Route53::HostedZone".into(),
        );
        hosted_zone
            .properties
            .insert("Name".into(), CfExpression::String(zone.domain.clone()));
        Ok(vec![hosted_zone])
    }

    /// The template output the importer reads after initial setup.
    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([(
            "zoneId",
            CfExpression::ref_(required_logical_id(ctx)?),
        )]))
    }
}

/// The zone on AWS in the Terraform module.
pub struct AwsTfEmitter;

impl TfEmitter for AwsTfEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let zone = downcast::<DnsZone>(ctx, DnsZone::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(TfFragment::default().with_resource(resource_block(
            "aws_route53_zone",
            label,
            [attr("name", expr::template(zone.domain.clone()))],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([(
            "zoneId",
            expr::traversal(["aws_route53_zone", label, "zone_id"]),
        )]))
    }
}

/// The zone on Google Cloud in the Terraform module.
pub struct GcpTfEmitter;

impl TfEmitter for GcpTfEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let zone = downcast::<DnsZone>(ctx, DnsZone::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(TfFragment::default().with_resource(resource_block(
            "google_dns_managed_zone",
            label,
            [
                // A zone name allows lowercase letters, digits and dashes, as the prefix and the id use.
                attr("name", resource_prefix_template(ctx.resource_id)),
                attr("dns_name", expr::template(format!("{}.", zone.domain))),
            ],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([(
            "zoneName",
            expr::traversal(["google_dns_managed_zone", label, "name"]),
        )]))
    }
}

/// The zone on Azure in the Terraform module.
pub struct AzureTfEmitter;

impl TfEmitter for AzureTfEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let zone = downcast::<DnsZone>(ctx, DnsZone::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(TfFragment::default().with_resource(resource_block(
            "azurerm_dns_zone",
            label,
            [
                attr("name", expr::template(zone.domain.clone())),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
            ],
        )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([(
            "zoneId",
            expr::traversal(["azurerm_dns_zone", label, "id"]),
        )]))
    }
}

pub struct KubernetesEmitter;

impl HelmEmitter for KubernetesEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        Ok(
            HelmFragment::default().with_infrastructure(InfrastructureValue {
                id: ctx.resource_id.to_string(),
                binding_type: "acme/dns-zone".to_string(),
                service: "route53".to_string(),
                fields: indexmap! { "zoneId".to_string() => "your-zone-id".to_string() },
            }),
        )
    }
}

pub struct AwsImporter;

impl ResourceImporter for AwsImporter {
    type ImportData = AwsDnsZoneImport;

    fn import(
        &self,
        data: AwsDnsZoneImport,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let outputs = DnsZoneOutputs {
            zone_id: data.zone_id.clone(),
        };
        ctx.ready(
            AwsDnsZoneFields {
                zone_id: Some(data.zone_id),
            },
            outputs,
        )
    }
}

pub struct GcpImporter;

impl ResourceImporter for GcpImporter {
    type ImportData = GcpDnsZoneImport;

    fn import(
        &self,
        data: GcpDnsZoneImport,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let outputs = DnsZoneOutputs {
            zone_id: data.zone_name.clone(),
        };
        ctx.ready(
            GcpDnsZoneFields {
                zone_name: Some(data.zone_name),
            },
            outputs,
        )
    }
}

pub struct AzureImporter;

impl ResourceImporter for AzureImporter {
    type ImportData = AzureDnsZoneImport;

    fn import(
        &self,
        data: AzureDnsZoneImport,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let outputs = DnsZoneOutputs {
            zone_id: data.zone_id.clone(),
        };
        ctx.ready(
            AzureDnsZoneFields {
                zone_id: Some(data.zone_id),
            },
            outputs,
        )
    }
}

/// What a linked workload gets.
pub fn link(outputs: &DnsZoneOutputs) -> Link {
    Link::new()
        .env("ACME_ZONE_ID", &outputs.zone_id)
        .permission_set("acme/dns-zone/records-write")
}
