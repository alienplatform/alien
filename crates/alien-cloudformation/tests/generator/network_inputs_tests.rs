//! Evaluate deploy-time inputs, not just the unresolved template snapshot.
use std::collections::HashMap;

use alien_cloudformation::{CfTemplate, CloudFormationTarget, RegistrationMode};
use alien_core::{
    import::data::AwsNetworkImportData, Network, NetworkSettings, ResourceLifecycle, Stack,
    StackSettings,
};
use serde_json::{json, Value};

use super::helpers::render_built_ins_template;

// Fail on unsupported intrinsics so new generator behavior cannot silently bypass
// the assertions. Conditions come from the generated template, not test answers.
fn evaluate(value: &Value, template: &CfTemplate, parameters: &HashMap<String, Value>) -> Value {
    let eval = |v| evaluate(v, template, parameters);
    match value {
        Value::Object(fields) => {
            if let Some(name) = fields.get("Ref") {
                if name == "AWS::NoValue" {
                    return Value::Null;
                }
                return parameters[name.as_str().unwrap()].clone();
            }
            if let Some(args) = fields.get("Fn::If") {
                let condition =
                    serde_json::to_value(&template.conditions[args[0].as_str().unwrap()]).unwrap();
                return eval(
                    &args[if eval(&condition).as_bool().unwrap() {
                        1
                    } else {
                        2
                    }],
                );
            }
            if let Some(value) = fields.get("Fn::ToJsonString") {
                return json!(serde_json::to_string(&eval(value)).unwrap());
            }
            if let Some(args) = fields.get("Fn::Equals") {
                return json!(eval(&args[0]) == eval(&args[1]));
            }
            if let Some(args) = fields.get("Fn::Not") {
                return json!(!eval(&args[0]).as_bool().unwrap());
            }
            if let Some(args) = fields.get("Fn::Join") {
                let values = eval(&args[1]);
                return json!(values
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(args[0].as_str().unwrap()));
            }
            if let Some(args) = fields.get("Fn::Select") {
                return eval(&args[1])[args[0].as_u64().unwrap() as usize].clone();
            }
            assert!(
                !fields.keys().any(|key| key.starts_with("Fn::")),
                "unsupported expression: {value}"
            );
            Value::Object(fields.iter().map(|(k, v)| (k.clone(), eval(v))).collect())
        }
        Value::Array(values) => Value::Array(values.iter().map(eval).collect()),
        other => other.clone(),
    }
}

#[test]
fn existing_network_registration_resolves_optional_lists_for_both_network_origins() {
    for authored in [
        NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        },
        NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-test".into(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec![],
            security_group_ids: vec![],
        },
    ] {
        let stack = Stack::new("network-inputs".into())
            .add(
                Network::new("net".into())
                    .settings(authored.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let (template, _) = render_built_ins_template(
            &stack,
            StackSettings {
                network: Some(authored),
                ..Default::default()
            },
            RegistrationMode::Both {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".into(),
                callback_url: None,
            },
            CloudFormationTarget::Aws,
            "aws",
            "existing network input evaluation",
        );
        let properties = &template.resources["DeploymentRegistration"].properties;
        let settings_expression = serde_json::to_value(&properties["StackSettings"]).unwrap();
        let imports_expression = serde_json::to_value(&properties["Resources"]).unwrap();
        for (public, private, groups) in [
            ("", "subnet-private", ""),
            ("subnet-public", "", "sg-first,sg-second"),
            ("subnet-public", "subnet-private-a,subnet-private-b", ""),
        ] {
            let mut parameters: HashMap<String, Value> = template
                .parameters
                .iter()
                .filter_map(|(name, p)| {
                    p.default
                        .as_ref()
                        .map(|d| (name.clone(), serde_json::to_value(d).unwrap()))
                })
                .collect();
            for (name, csv) in [
                ("PublicSubnetIds", public),
                ("PrivateSubnetIds", private),
                ("SecurityGroupIds", groups),
            ] {
                parameters.insert(
                    name.into(),
                    json!(csv.split(',').map(str::trim).collect::<Vec<_>>()),
                );
            }
            parameters.insert("VpcId".into(), json!("vpc-test"));
            parameters.insert("NetworkMode".into(), json!("use-existing"));
            let network_expression = &settings_expression["network"];
            let network = evaluate(network_expression, &template, &parameters);
            // Check the raw payload before serde normalization can hide an emitter bug.
            assert_eq!(
                network["public_subnet_ids"],
                json!(if public.is_empty() {
                    vec![]
                } else {
                    public.split(',').collect::<Vec<_>>()
                })
            );
            assert_eq!(
                network["private_subnet_ids"],
                json!(private
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>())
            );
            assert_eq!(
                network["security_group_ids"],
                json!(groups
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>())
            );
            let output_settings: Value = serde_json::from_str(
                evaluate(
                    &serde_json::to_value(&template.outputs["DeploymentStackSettings"].value)
                        .unwrap(),
                    &template,
                    &parameters,
                )
                .as_str()
                .unwrap(),
            )
            .unwrap();
            assert_eq!(output_settings["network"], network);
            let settings: NetworkSettings = serde_json::from_value(network).unwrap();
            let imports = evaluate(&imports_expression, &template, &parameters);
            let output_imports: Value = serde_json::from_str(
                evaluate(
                    &serde_json::to_value(&template.outputs["DeploymentResources"].value).unwrap(),
                    &template,
                    &parameters,
                )
                .as_str()
                .unwrap(),
            )
            .unwrap();
            assert_eq!(output_imports, imports);
            let data: AwsNetworkImportData =
                serde_json::from_value(imports[0]["importData"].clone()).unwrap();
            let NetworkSettings::ByoVpcAws {
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
                ..
            } = settings
            else {
                panic!("existing network settings");
            };
            assert_eq!(data.public_subnet_ids, public_subnet_ids);
            assert_eq!(data.private_subnet_ids, private_subnet_ids);
            assert_eq!(data.security_group_id, security_group_ids.first().cloned());
        }
    }
}

#[test]
fn application_string_list_inputs_preserve_empty_lists_at_registration() {
    let mut input = alien_core::StackInputDefinition::deployer_boolean(
        "zones",
        "Zones",
        "Optional zones",
        Some(false),
    );
    input.kind = alien_core::StackInputKind::StringList;
    input.default = Some(alien_core::StackInputDefaultValue::StringList(vec![]));
    let stack = Stack::new("list-inputs".into()).inputs(vec![input]).build();
    let (template, _) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        RegistrationMode::CustomResource {
            lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".into(),
            callback_url: None,
        },
        CloudFormationTarget::Aws,
        "aws",
        "application list input evaluation",
    );
    let expression = serde_json::to_value(
        &template.resources["DeploymentRegistration"].properties["InputValues"],
    )
    .unwrap();
    for (supplied, expected) in [
        (json!([""]), json!([])),
        (json!(["zone-a", "zone-b"]), json!(["zone-a", "zone-b"])),
    ] {
        let parameters = HashMap::from([("InputZones".into(), supplied)]);
        assert_eq!(
            evaluate(&expression, &template, &parameters)["zones"],
            expected
        );
    }
}
