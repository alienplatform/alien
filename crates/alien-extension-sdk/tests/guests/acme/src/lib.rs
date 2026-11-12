use acme_types::DnsZone;
use alien_extension_sdk::prelude::*;

mod dns_zone;
mod kubernetes;
mod network_policy;
mod postgres_iam_auth;

fn register(ext: &mut Extension) {
    // The database flag.
    ext.cloudformation_modifier(postgres_iam_auth::CloudFormation);
    ext.terraform_modifier(postgres_iam_auth::Terraform);
    ext.aws_interceptor(postgres_iam_auth::Rds);
    ext.gcp_interceptor(postgres_iam_auth::CloudSql);
    ext.azure_interceptor(postgres_iam_auth::FlexibleServer);

    // The DNS zone. Its controllers are registered in controller/src/main.rs.
    ext.resource::<DnsZone>()
        .permission_set(include_str!("../permissions/dns-zone/provision.jsonc"))
        .permission_set(include_str!("../permissions/dns-zone/records-write.jsonc"))
        .cloudformation_emitter(Platform::Aws, dns_zone::AwsCfEmitter)
        .terraform_emitter(Platform::Aws, dns_zone::AwsTfEmitter)
        .terraform_emitter(Platform::Gcp, dns_zone::GcpTfEmitter)
        .terraform_emitter(Platform::Azure, dns_zone::AzureTfEmitter)
        .helm_emitter(Platform::Kubernetes, dns_zone::KubernetesEmitter)
        .importer(Platform::Aws, dns_zone::AwsImporter)
        .importer(Platform::Gcp, dns_zone::GcpImporter)
        .importer(Platform::Azure, dns_zone::AzureImporter)
        .requires_services(Platform::Gcp, ["dns.googleapis.com"])
        .requires_services(Platform::Azure, ["Microsoft.Network"])
        .link(dns_zone::link);

    // From the template modifiers guide and the tags example: the kinds the full example lacks.
    ext.helm_modifier(network_policy::NetworkPolicy);
    ext.kubernetes_interceptor(kubernetes::KubernetesLabels);

    // Not in the docs: a hook tests steer through the template's description, to check that
    // its panic reaches Alien, that Alien refuses a template that loses or retypes a resource,
    // and that modifiers run in the order the stack lists extensions.
    ext.cloudformation_modifier(OnRequest);
}

/// Does what the template's description starts with, and leaves every other template as it was:
/// `panic`; `remove <logical id>`; `retype <logical id> <type>`; `backups <days>`, which sets
/// every RDS cluster's backup retention. A setup package's description starts with the display
/// name the test picks.
struct OnRequest;

impl CloudFormationModifier for OnRequest {
    fn modify(&self, mut template: CfTemplate) -> Result<CfTemplate> {
        let request = template.description.clone().unwrap_or_default();
        match request.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["panic", ..] => panic!("the template asked this hook to panic"),
            ["remove", logical_id, ..] => {
                template.resources.shift_remove(*logical_id);
            }
            ["retype", logical_id, resource_type, ..] => {
                if let Some(resource) = template.resources.get_mut(*logical_id) {
                    resource.resource_type = resource_type.to_string();
                }
            }
            ["backups", days, ..] => {
                let days: i32 = days.parse().expect("a number of days");
                for cluster in template.resources_of_type_mut("AWS::RDS::DBCluster") {
                    cluster.set_property("BackupRetentionPeriod", days);
                }
            }
            _ => {}
        }
        Ok(template)
    }
}

alien_extension_sdk::export!("acme", register);
