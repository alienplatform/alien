use acme_types::DnsZone;
use alien_extension_sdk::prelude::*;

mod dns_zone;

fn register(ext: &mut Extension) {
    ext.resource::<DnsZone>()
        .permission_set(include_str!("../../permissions/dns-zone/provision.jsonc"))
        .permission_set(include_str!(
            "../../permissions/dns-zone/records-write.jsonc"
        ))
        .cloudformation_emitter(Platform::Aws, dns_zone::AwsCfEmitter)
        .terraform_emitter(Platform::Aws, dns_zone::AwsTfEmitter)
        .terraform_emitter(Platform::Gcp, dns_zone::GcpTfEmitter)
        .terraform_emitter(Platform::Azure, dns_zone::AzureTfEmitter)
        .helm_emitter(Platform::Kubernetes, dns_zone::KubernetesEmitter)
        .importer(Platform::Aws, dns_zone::AwsImporter)
        .importer(Platform::Gcp, dns_zone::GcpImporter)
        .importer(Platform::Azure, dns_zone::AzureImporter)
        .link(dns_zone::link);
}

alien_extension_sdk::export!("acme", register);
