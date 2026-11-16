//! The verified setup intent copies the import fields into the registration's
//! metadata, where the stack's own management role reads them back.

use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::{
    add_verified_setup_intent, CfExpression, CfParameter, CloudFormationTarget,
};
use alien_core::{RemoteStackManagement, ResourceLifecycle, Stack, StackSettings, Storage};
use serde_json::Value;

fn template() -> alien_cloudformation::CfTemplate {
    let stack = Stack::new("intent".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "setup intent",
    )
    .0
}

#[test]
fn intent_carries_the_import_fields_and_a_read_policy_on_the_own_stack() {
    let mut template = template();
    add_verified_setup_intent(&mut template).unwrap();
    let yaml = alien_cloudformation::to_yaml(&template).unwrap();
    super::helpers::lint(&yaml).assert_ok("setup intent");

    let registration = &template.resources["DeploymentRegistration"];
    let intent = &registration.metadata["SetupIntent"];
    assert_eq!(intent["version"], "1");
    assert_eq!(
        intent["installId"],
        serde_json::json!({"Ref": "AWS::StackId"})
    );
    assert_eq!(
        intent["audience"],
        serde_json::json!({"Ref": "SetupManagerAudience"})
    );
    let source = intent["source"].as_object().unwrap();
    for field in [
        "DeploymentName",
        "ResourcePrefix",
        "Platform",
        "Region",
        "SetupTarget",
        "SetupImportFormatVersion",
        "SetupFingerprint",
        "SetupFingerprintVersion",
        "ManagementConfig",
        "StackSettings",
        "Resources",
    ] {
        assert!(source.contains_key(field), "intent lacks {field}");
    }
    assert!(!source.contains_key("SourceKind"));
    // Metadata is static, so CloudFormation only invokes the resource on a
    // real change. The identical property makes an intent change an update.
    assert_eq!(
        serde_json::to_value(&registration.properties["SetupIntent"]).unwrap(),
        *intent
    );

    let policy = serde_json::to_value(&template.resources["SetupIntentReadPolicy"]).unwrap();
    let statement = &policy["Properties"]["PolicyDocument"]["Statement"][0];
    assert_eq!(statement["Action"], "cloudformation:DescribeStackResource");
    assert_eq!(
        statement["Resource"],
        serde_json::json!({"Ref": "AWS::StackId"})
    );
    assert_eq!(
        policy["Properties"]["Roles"][0]["Ref"],
        Value::from("ManagementRole")
    );
}

#[test]
fn intent_refuses_a_sensitive_parameter_and_leaves_the_template_unchanged() {
    let mut template = template();
    template.parameters.insert(
        "Hidden".into(),
        CfParameter {
            parameter_type: "String".into(),
            description: None,
            default: None,
            allowed_values: None,
            allowed_pattern: None,
            constraint_description: None,
            min_length: None,
            max_length: None,
            min_value: None,
            max_value: None,
            no_echo: Some(true),
        },
    );
    template
        .resources
        .get_mut("DeploymentRegistration")
        .unwrap()
        .properties
        .insert("DeploymentName".into(), CfExpression::ref_("Hidden"));
    let before = alien_cloudformation::to_yaml(&template).unwrap();
    assert!(add_verified_setup_intent(&mut template).is_err());
    assert_eq!(alien_cloudformation::to_yaml(&template).unwrap(), before);
}

/// The registration `Resources` of the verified intent are JSON text assembled with `Fn::Join`.
/// An element masked with `AWS::NoValue` would survive there as a literal `null`, and the importer
/// refuses a null where it expects a subnet id or a zone name. Resolve the text for each zone
/// count and require the same typed payload the manager imports.
#[test]
fn intent_resources_carry_no_null_for_any_zone_count() {
    use super::helpers::{resolve, Declined};
    use alien_core::{AwsNetworkImportData, Network, NetworkSettings};
    use std::collections::HashMap;

    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("intent".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network settings"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (mut template, _) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "setup intent network",
    );
    add_verified_setup_intent(&mut template).unwrap();
    let resources = template.resources["DeploymentRegistration"].metadata["SetupIntent"]["source"]
        ["Resources"]
        .clone();
    // Resources is `Fn::Join("", ["[", Fn::Join(",", [<one JSON text per resource>]), "]"])`.
    // Resolve the network's own text; the other resources have no zone-dependent parts.
    let entries = &resources["Fn::Join"][1][1]["Fn::Join"][1];
    let network_text = entries
        .as_array()
        .expect("one JSON text per resource")
        .iter()
        .find(|entry| entry.to_string().contains("default-network"))
        .expect("the network is registered")
        .clone();
    let resources: CfExpression = serde_json::from_value(network_text).unwrap();
    let native_resources = {
        let list = serde_json::to_value(
            &template.resources["DeploymentRegistration"].properties["Resources"],
        )
        .unwrap();
        let network = list
            .as_array()
            .expect("native resources are a list")
            .iter()
            .find(|entry| entry["id"] == "default-network")
            .expect("the network is registered")
            .clone();
        serde_json::from_value::<CfExpression>(network).unwrap()
    };

    // (zones, network mode, subnet and security group lists supplied)
    let mut scenarios = Vec::new();
    for zones in 1..=3usize {
        scenarios.push((zones, "create-new", false));
    }
    scenarios.push((2, "use-existing", true));
    scenarios.push((2, "use-existing", false));
    scenarios.push((2, "use-default", false));
    for (zones, mode, supplied) in scenarios {
        let conditions: HashMap<&str, bool> = template
            .conditions
            .keys()
            .map(|name| {
                let answer = match name.as_str() {
                    "NetworkModeCreate" => mode == "create-new",
                    "NetworkModeUseExisting" => mode == "use-existing",
                    "NetworkCreateUseAz2" | "NetworkUseAz2" => zones >= 2,
                    "NetworkCreateUseAz3" | "NetworkUseAz3" => zones >= 3,
                    "HasPublicSubnetIds" | "HasPrivateSubnetIds" | "HasSecurityGroupIds" => {
                        supplied
                    }
                    _ => false,
                };
                (name.as_str(), answer)
            })
            .collect();
        let Some(CfExpression::String(text)) = resolve(&resources, &conditions, Declined::Removed)
        else {
            panic!("registration resources should resolve to text");
        };
        let network: Value = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("network is not JSON ({mode}, {zones}): {error}"));
        let data: AwsNetworkImportData = serde_json::from_value(network["importData"].clone())
            .unwrap_or_else(|error| {
                panic!(
                    "import data ({mode}, {zones} zones) does not deserialize: {error}\n{network}"
                )
            });
        if mode == "create-new" {
            // The registration's native property, which the token flow sends, resolves to
            // the same lists: a changed template text alone must not change what is delivered.
            let native_network = serde_json::to_value(
                resolve(&native_resources, &conditions, Declined::Removed)
                    .expect("native network resolves"),
            )
            .unwrap();
            let expect_refs = |prefix: &str| -> Vec<Value> {
                (1..=zones)
                    .map(|n| serde_json::json!({ "Ref": format!("{prefix}{n}") }))
                    .collect()
            };
            let expect_zones: Vec<Value> = (0..zones)
                .map(|n| {
                    serde_json::json!({
                        "Fn::Select": [n, { "Fn::GetAZs": { "Ref": "AWS::Region" } }]
                    })
                })
                .collect();
            let native_list = |field: &str| -> Vec<Value> {
                native_network["importData"][field]
                    .as_array()
                    .unwrap_or_else(|| panic!("{field} is a list"))
                    .clone()
            };
            // Exact ids in zone order: swapped public/private lists, repeated ids or
            // reordered zones must fail, not only a wrong length.
            assert_eq!(
                native_list("publicSubnetIds"),
                expect_refs("DefaultNetworkPublicSubnet"),
                "native publicSubnetIds for {zones} zones"
            );
            assert_eq!(
                native_list("privateSubnetIds"),
                expect_refs("DefaultNetworkPrivateSubnet"),
                "native privateSubnetIds for {zones} zones"
            );
            assert_eq!(
                native_list("availabilityZones"),
                expect_zones,
                "native availabilityZones for {zones} zones"
            );
            // The verified text path resolves to placeholders in the same order.
            let placeholders = |prefix: &str| -> Vec<String> {
                (1..=zones).map(|n| format!("${{{prefix}{n}}}")).collect()
            };
            assert_eq!(
                data.public_subnet_ids,
                placeholders("DefaultNetworkPublicSubnet"),
                "text publicSubnetIds for {zones} zones"
            );
            assert_eq!(
                data.private_subnet_ids,
                placeholders("DefaultNetworkPrivateSubnet"),
                "text privateSubnetIds for {zones} zones"
            );
            assert_eq!(
                data.availability_zones,
                (0..zones)
                    .map(|n| format!("${{Select{n}}}"))
                    .collect::<Vec<_>>(),
                "text availabilityZones for {zones} zones"
            );
        }
    }
}
