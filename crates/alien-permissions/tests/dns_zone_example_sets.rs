mod common;

use std::collections::BTreeSet;

use alien_permission_types::PermissionSet;
use alien_permissions::{
    extension_identity_refusals,
    generators::{
        AwsRuntimePermissionsGenerator, AzureRuntimePermissionsGenerator, GcpBindingTargetScope,
        GcpRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use common::create_test_context;

/// The controller's calls Route53 authorizes against "*" alone: they take no resource ARN.
const UNSCOPABLE: &[&str] = &["route53:CreateHostedZone", "route53:ListHostedZonesByName"];

/// The controller's calls on a hosted zone it made.
const ZONE_CALLS: &[&str] = &[
    "route53:ListResourceRecordSets",
    "route53:ChangeResourceRecordSets",
    "route53:DeleteHostedZone",
];

/// The calls that write a zone's records.
const RECORD_CALLS: &[&str] = &[
    "route53:ChangeResourceRecordSets",
    "route53:ListResourceRecordSets",
];

const HOSTED_ZONES: &str = "arn:aws:route53:::hostedzone/*";

/// The dns-zone example's sets, which extension authors copy, and the test guest's copies of
/// them grant exactly the calls the controller makes: a hosted zone's calls on hosted zones, and
/// "*" only for the calls that need it.
#[test]
fn the_dns_zone_sets_grant_exactly_the_controller_calls() {
    let context: PermissionContext = create_test_context();
    let provision = (UNSCOPABLE, ZONE_CALLS);
    let records_write: (&[&str], &[&str]) = (&[], RECORD_CALLS);
    for (jsonc, (expected_on_any, expected_on_zones)) in [
        (
            include_str!(
                "../../../examples/extensions/dns-zone/permissions/dns-zone/provision.jsonc"
            ),
            provision,
        ),
        (
            include_str!(
                "../../alien-extension-sdk/tests/guests/acme/permissions/dns-zone/provision.jsonc"
            ),
            provision,
        ),
        (
            include_str!(
                "../../../examples/extensions/dns-zone/permissions/dns-zone/records-write.jsonc"
            ),
            records_write,
        ),
        (
            include_str!(
                "../../alien-extension-sdk/tests/guests/acme/permissions/dns-zone/records-write.jsonc"
            ),
            records_write,
        ),
    ] {
        let set: PermissionSet = json5::from_str(jsonc).unwrap();
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, target, &context)
                .unwrap();
            let mut on_any = BTreeSet::new();
            let mut on_zones = BTreeSet::new();
            for statement in policy.statement {
                let granted = if statement.resource == ["*"] {
                    &mut on_any
                } else {
                    assert_eq!(statement.resource, [HOSTED_ZONES], "{}", set.id);
                    &mut on_zones
                };
                granted.extend(statement.action);
            }
            let expected = |calls: &[&str]| -> BTreeSet<String> {
                calls.iter().map(|call| call.to_string()).collect()
            };
            assert_eq!(on_any, expected(expected_on_any), "{}: on *", set.id);
            assert_eq!(
                on_zones,
                expected(expected_on_zones),
                "{}: on hosted zones",
                set.id
            );
        }
    }
}

/// On Google Cloud and Azure the example's sets grant exactly the controllers' calls and a linked
/// workload's record writes, on the project and on the deployment's resource group, and
/// `provision` passes the check every set an extension's identity holds must pass.
#[test]
fn the_dns_zone_sets_grant_exactly_the_google_cloud_and_azure_calls() {
    let context: PermissionContext = create_test_context();
    let provision: PermissionSet = json5::from_str(include_str!(
        "../../../examples/extensions/dns-zone/permissions/dns-zone/provision.jsonc"
    ))
    .unwrap();
    let records_write: PermissionSet = json5::from_str(include_str!(
        "../../../examples/extensions/dns-zone/permissions/dns-zone/records-write.jsonc"
    ))
    .unwrap();
    let record_types = ["A", "AAAA", "CAA", "CNAME", "MX", "NS", "PTR", "SRV", "TXT"];
    let record_actions: Vec<String> = record_types
        .iter()
        .map(|record_type| format!("Microsoft.Network/dnszones/{record_type}/*"))
        .chain(["Microsoft.Network/dnszones/recordsets/read".to_string()])
        .collect();
    let sorted = |names: Vec<String>| names.into_iter().collect::<BTreeSet<_>>();
    let strings = |names: &[&str]| sorted(names.iter().map(|name| name.to_string()).collect());
    for (set, gcp, azure) in [
        (
            &provision,
            strings(&[
                "dns.managedZones.create",
                "dns.managedZones.get",
                "dns.managedZones.delete",
                "dns.changes.create",
                "dns.resourceRecordSets.list",
                "dns.resourceRecordSets.delete",
            ]),
            strings(&[
                "Microsoft.Network/dnszones/read",
                "Microsoft.Network/dnszones/write",
                "Microsoft.Network/dnszones/delete",
            ]),
        ),
        (
            &records_write,
            strings(&[
                "dns.changes.create",
                "dns.resourceRecordSets.create",
                "dns.resourceRecordSets.update",
                "dns.resourceRecordSets.delete",
                "dns.resourceRecordSets.list",
            ]),
            sorted(record_actions.clone()),
        ),
    ] {
        let role = GcpRuntimePermissionsGenerator::new()
            .generate_custom_role(set, &context)
            .unwrap();
        assert_eq!(sorted(role.included_permissions), gcp, "{}", set.id);
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let bindings = GcpRuntimePermissionsGenerator::new()
                .generate_bindings(set, target, &context)
                .unwrap()
                .bindings;
            assert_eq!(bindings.len(), 1, "{}", set.id);
            assert_eq!(
                bindings[0].target,
                GcpBindingTargetScope::Project,
                "{}",
                set.id
            );
            assert_eq!(bindings[0].condition, None, "{}", set.id);

            let plan = AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(set, target, &context)
                .unwrap();
            assert_eq!(plan.custom_roles.len(), 1, "{}", set.id);
            assert_eq!(
                sorted(plan.custom_roles[0].role_definition.actions.clone()),
                azure,
                "{}",
                set.id
            );
            assert_eq!(plan.bindings.len(), 1, "{}", set.id);
            assert_eq!(
                plan.bindings[0].scope,
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod",
                "{}",
                set.id
            );
        }
    }
    assert_eq!(
        extension_identity_refusals(&provision),
        Vec::<String>::new()
    );
}
