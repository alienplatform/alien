//! Rendering a protected stack's setup templates the way a signed release records them.

use alien_cli_common::setup_templates::{
    file_map_bytes, package_stack_settings, render_setup_template, render_setup_templates,
    SetupHostingOptions, SetupRenderOptions,
};
use alien_cloudformation::CfRegistry;
use alien_core::{SetupPins, Stack, StackSettings, StackSigning};
use alien_helm::HelmRegistry;
use alien_signing::release::SETUP_TEMPLATES;
use alien_terraform::TfRegistry;

const FINGERPRINT: &str = "SHA256:9f2c3b1a77d0e5c20b19a4f36c8e2d5100112233445566778899aabbccddeeff";

const PUBLISHER: &str = "111122223333";
const VERIFIER: &str =
    "verifier@sha256:5d41402abc4b2a76b9719d911017c592ae9e2b6d7c5c7f2a3f1e0d9c8b7a6f50";

fn pins() -> SetupPins {
    SetupPins {
        aws_publisher: Some(PUBLISHER.to_string()),
        aws_verifier_image: Some(VERIFIER.to_string()),
        gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
        azure_verifier_image: Some(format!(
            "registry.example/verifier@sha256:{}",
            "7f".repeat(32)
        )),
        operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
    }
}

fn hosting() -> SetupHostingOptions {
    serde_json::from_value(serde_json::json!({
        "registration": { "mode": "outputsFallback" }
    }))
    .expect("hosting options parse")
}

/// A stack with storage only: protected, it gets the signing settings and management's limits,
/// and no verifier, since nothing it runs is the verifier's to write.
fn stack() -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "minimal",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("fixture stack parses")
}

/// [`stack`] with a function, which a protected install's verifier writes.
fn stack_with_function() -> Stack {
    let mut stack = stack();
    stack.resources.insert(
        "api".to_string(),
        serde_json::from_value(serde_json::json!({
            "config": {
                "code": {
                    "image": "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "type": "image"
                },
                "id": "api", "memoryMb": 512, "permissions": "execution",
                "timeoutSeconds": 180, "type": "worker", "links": [], "triggers": [],
                "environment": {}, "commandsEnabled": false
            },
            "lifecycle": "live", "dependencies": [], "remoteAccess": false
        }))
        .expect("fixture function parses"),
    );
    stack
}

async fn render(
    fingerprint: &str,
) -> std::collections::BTreeMap<String, alien_cli_common::setup_templates::RenderedSetupTemplate> {
    render_stack(fingerprint, &stack_with_function()).await
}

async fn render_stack(
    fingerprint: &str,
    stack: &Stack,
) -> std::collections::BTreeMap<String, alien_cli_common::setup_templates::RenderedSetupTemplate> {
    let signing = StackSigning {
        fingerprint: fingerprint.to_string(),
        channel: "production".to_string(),
    };
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let rendered = render_setup_templates(
        stack,
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .hosting(hosting())
            .pins(pins())
            .build(),
    )
    .await
    .expect("protected setup templates render");
    // `cfn-lint`, `terraform validate` and `helm lint` run on these in
    // scripts/setup-render-diff/lint-protected.sh.
    if let Some(dir) = std::env::var_os("SETUP_TEMPLATES_OUT") {
        for (name, template) in &rendered {
            std::fs::write(std::path::Path::new(&dir).join(name), &template.bytes)
                .expect("write rendered template");
        }
    }
    rendered
}

#[tokio::test]
async fn renders_every_setup_template_the_same_way_twice() {
    let first = render(FINGERPRINT).await;
    let second = render(FINGERPRINT).await;

    assert_eq!(first.keys().map(String::as_str).collect::<Vec<_>>(), {
        let mut names = SETUP_TEMPLATES.to_vec();
        names.sort();
        names
    });
    assert_eq!(first, second, "rendering is deterministic");

    let other = render(&FINGERPRINT.replace("9f2c", "0000")).await;
    for name in SETUP_TEMPLATES {
        assert_ne!(
            first[*name].setup_template().sha256,
            other[*name].setup_template().sha256,
            "{name}: the digest covers the fingerprint"
        );
    }
}

#[tokio::test]
async fn cloudformation_pins_the_fingerprint_and_leaves_it_off_the_allow_list() {
    let rendered = render(FINGERPRINT).await;
    let cloudformation = &rendered["cloudformation"];
    let template: serde_yaml::Value =
        serde_yaml::from_slice(&cloudformation.bytes).expect("template is YAML");

    let parameter = &template["Parameters"]["SigningFingerprint"];
    assert_eq!(
        parameter["AllowedValues"],
        serde_yaml::to_value([FINGERPRINT]).unwrap()
    );
    assert_eq!(parameter["Default"].as_str(), Some(FINGERPRINT));
    assert!(!cloudformation
        .allowed
        .iter()
        .any(|name| name == "SigningFingerprint"));
    assert!(cloudformation
        .allowed
        .iter()
        .any(|name| name == "ManagingRoleArn"));
    let template_parameters = template["Parameters"].as_mapping().unwrap().len();
    assert_eq!(cloudformation.allowed.len(), template_parameters - 1);

    let terraform = &rendered["terraform-aws"];
    assert!(!terraform
        .allowed
        .iter()
        .any(|name| name == "signing_fingerprint"));
    let helm = &rendered["helm"];
    assert!(!helm.allowed.iter().any(|name| name == "signing"));
}

#[tokio::test]
async fn cloudformation_runs_the_pinned_verifier_and_limits_management() {
    let rendered = render(FINGERPRINT).await;
    let template: serde_yaml::Value =
        serde_yaml::from_slice(&rendered["cloudformation"].bytes).expect("template is YAML");
    let resources = &template["Resources"];

    let verifier = &resources["SigningVerifier"]["Properties"];
    assert_eq!(
        verifier["Code"]["ImageUri"]["Fn::Sub"].as_str(),
        Some(format!("{PUBLISHER}.dkr.ecr.${{AWS::Region}}.amazonaws.com/{VERIFIER}").as_str())
    );
    assert_eq!(
        verifier["FunctionName"]["Fn::Sub"].as_str(),
        Some("${AWS::StackName}-verifier")
    );
    let role = &resources["SigningVerifierRole"]["Properties"];
    assert!(
        role.get("PermissionsBoundary").is_none(),
        "the verifier's role has no boundary"
    );

    let limits = &resources["ManagementLimits"]["Properties"];
    assert_eq!(
        limits["ManagedPolicyName"]["Fn::Sub"].as_str(),
        Some("${AWS::StackName}-management-limits")
    );
    assert_eq!(limits["Roles"][0]["Ref"].as_str(), Some("ManagementRole"));
    let sids: Vec<&str> = limits["PolicyDocument"]["Statement"]
        .as_sequence()
        .expect("statements")
        .iter()
        .map(|statement| statement["Sid"].as_str().expect("sid"))
        .collect();
    assert_eq!(
        sids,
        [
            "OnlyVerifierChangesFunctions",
            "OnlyVerifierLaunchesMachines",
            "OnlyVerifierPassesRolesToEc2",
            "NoCommandsOnMachines",
            "NoUnsignedMicrovms",
            "NoUnsignedBuilds",
            "OnlySetupChangesWhatRolesMayDo",
            "OnlySetupChangesTheVerifier",
            "OnlySetupChangesTheseLimits",
            "CallTheVerifier",
            "ReadThisInstallsSigningSettings",
        ]
    );
}

#[tokio::test]
async fn a_build_without_pins_refuses_to_render_protected_aws_setup() {
    let signing = StackSigning {
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    };
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let error = render_setup_templates(
        &stack_with_function(),
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .hosting(hosting())
            .pins(SetupPins::default())
            .build(),
    )
    .await
    .expect_err("no publisher, no protected AWS setup");
    let text = format!("{error:?}");
    assert!(text.contains("no machine-image publisher"), "{text}");
}

/// Every pin but the Azure verifier image: the protected Azure module is refused and names the
/// missing pin; the other clouds still render.
#[tokio::test]
async fn a_build_without_the_azure_verifier_image_refuses_protected_azure_setup() {
    let signing = StackSigning {
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    };
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let options = SetupRenderOptions::builder()
        .signing(&signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(hosting())
        .pins(SetupPins {
            azure_verifier_image: None,
            ..pins()
        })
        .build();
    let settings = StackSettings {
        signing: Some(signing.clone()),
        ..Default::default()
    };
    let error = render_setup_template(
        "terraform-azure",
        &stack_with_function(),
        &settings,
        &options,
    )
    .await
    .expect_err("no Azure verifier image, no protected Azure setup");
    let text = format!("{error:?}");
    assert!(
        text.contains("no Azure verifier image pinned by digest"),
        "{text}"
    );
    render_setup_template("terraform-aws", &stack_with_function(), &settings, &options)
        .await
        .expect("protected AWS setup needs no Azure pin");
}

/// Hosting options come from a service the install doesn't trust. None of them may change what
/// a protected template locks: the limits, the verifier, its role and the signing settings stay
/// exactly as they are with the plainest options.
#[tokio::test]
async fn hosting_options_cannot_weaken_the_protected_locks() {
    let locked = |rendered: &std::collections::BTreeMap<
        String,
        alien_cli_common::setup_templates::RenderedSetupTemplate,
    >| {
        let template: serde_yaml::Value =
            serde_yaml::from_slice(&rendered["cloudformation"].bytes).expect("template is YAML");
        let resources = &template["Resources"];
        let guard = resources
            .as_mapping()
            .expect("resources")
            .values()
            .filter(|resource| resource["Type"] == "AWS::IAM::ManagedPolicy")
            .flat_map(|policy| {
                policy["Properties"]["PolicyDocument"]["Statement"]
                    .as_sequence()
                    .cloned()
                    .unwrap_or_default()
            })
            .filter(|statement| statement["Sid"] == "OnlySetupChangesSigningSettings")
            .collect::<Vec<_>>();
        assert_eq!(guard.len(), 1, "one /signed/ guard on the management role");
        (
            [
                "ManagementLimits",
                "SigningVerifier",
                "SigningVerifierRole",
                "SigningFingerprintParameter",
                "SigningStateParameter",
            ]
            .map(|id| resources[id].clone()),
            guard,
            template["Parameters"]["SigningFingerprint"].clone(),
        )
    };
    let baseline = locked(&render(FINGERPRINT).await);

    let hostile = [
        serde_json::json!({
            "registration": {
                "mode": "regionalBoth",
                "lambdaArnsByRegion": {
                    "us-east-1": "arn:aws:lambda:us-east-1:999999999999:function:anything"
                },
                "callbackUrl": "https://attacker.example/"
            },
            "description": "Resources:\n  Evil: {Type: AWS::IAM::Role}",
            "managementTrust": {
                "requireExternalId": true,
                "allowedManagingRoleArns": ["arn:aws:iam::999999999999:role/anyone"]
            },
            "supportedAwsRegions": ["us-east-1"]
        }),
        serde_json::json!({
            "registration": {
                "mode": "customResource",
                "lambdaArn": "arn:aws:lambda:us-east-1:999999999999:function:anything"
            }
        }),
    ];
    for options in hostile {
        let hosting: SetupHostingOptions =
            serde_json::from_value(options.clone()).expect("hosting options parse");
        let signing = StackSigning {
            fingerprint: FINGERPRINT.to_string(),
            channel: "production".to_string(),
        };
        let (cloudformation, terraform, helm) = (
            CfRegistry::built_in(),
            TfRegistry::built_in(),
            HelmRegistry::built_in(),
        );
        let rendered = render_setup_templates(
            &stack_with_function(),
            SetupRenderOptions::builder()
                .signing(&signing)
                .cloudformation_registry(&cloudformation)
                .terraform_registry(&terraform)
                .helm_registry(&helm)
                .hosting(hosting)
                .pins(pins())
                .build(),
        )
        .await
        .expect("renders with hostile hosting options");
        assert_eq!(locked(&rendered), baseline, "{options}");
    }
}

#[test]
fn hosting_options_refuse_unknown_fields() {
    let error = serde_json::from_value::<SetupHostingOptions>(serde_json::json!({
        "registration": { "mode": "outputsFallback" },
        "extraPolicies": []
    }))
    .expect_err("an unknown hosting option is refused");
    assert!(error.to_string().contains("extraPolicies"), "{error}");
}

/// The hosting service computes the same fingerprint from the stack JSON; these values come from
/// its implementation for the fixture stacks.
#[test]
fn setup_fingerprints_match_the_hosting_service() {
    let stacks: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/setup-fingerprint-stacks.json"))
            .expect("fixture stacks");
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/setup-fingerprint-expected.json"))
            .expect("expected fingerprints");
    let mut checked = 0;
    for (name, stack) in &stacks {
        for platform in ["aws", "gcp", "azure", "kubernetes"] {
            let (fingerprint, version) =
                alien_cli_common::setup_fingerprint::setup_fingerprint(stack, platform);
            assert_eq!(
                fingerprint,
                expected[name][platform]["fingerprint"].as_str().unwrap(),
                "{name} {platform}"
            );
            assert_eq!(version, 1);
            checked += 1;
        }
    }
    assert_eq!(checked, 8);
}

/// Module and chart bytes are the sorted JSON object of their files, so anyone holding the
/// files rebuilds the signed bytes: here they decode back to the rendered files, and
/// reordering the files changes nothing.
#[tokio::test]
async fn file_map_bytes_rebuild_the_rendered_module() {
    let rendered = render(&format!("SHA256:{}", "9f".repeat(32))).await;
    let module = &rendered["terraform-gcp"];
    let files: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&module.bytes).expect("module bytes are a JSON file map");
    assert!(
        files.keys().any(|path| path.ends_with(".tf")),
        "{:?}",
        files.keys()
    );
    assert_eq!(file_map_bytes(&files), module.bytes);

    let tricky = std::collections::BTreeMap::from([
        ("z.tf".to_string(), "b = \"\\u00e9 é\"\n\ttab".to_string()),
        ("a/b.tf".to_string(), "</script> & \u{2028}".to_string()),
    ]);
    assert_eq!(
        file_map_bytes(&tricky),
        serde_json::to_vec(&tricky).unwrap()
    );
}

/// The `resource` addresses of a rendered Terraform module.
fn terraform_resources(template: &[u8]) -> std::collections::BTreeSet<String> {
    let files: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(template).expect("a module is a JSON file map");
    files
        .values()
        .flat_map(|contents| contents.lines())
        .filter_map(|line| line.strip_prefix("resource \""))
        .map(|rest| {
            rest.trim_end_matches(" {")
                .replace("\" \"", ".")
                .replace('"', "")
        })
        .collect()
}

/// Protecting an install replaces its unprotected module with the protected one, so the
/// protected module keeps everything the unprotected one creates for the same stack, the
/// network the unprotected module offers included, and only adds the signing resources.
#[tokio::test]
async fn a_protected_module_keeps_every_resource_of_the_unprotected_one() {
    let rendered = render_stack(FINGERPRINT, &stack()).await;
    let terraform = TfRegistry::built_in();
    let cloudformation = CfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let signing = StackSigning {
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    };
    let options = SetupRenderOptions::builder()
        .signing(&signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(hosting())
        .pins(pins())
        .build();
    for name in ["terraform-aws", "terraform-gcp", "terraform-azure"] {
        // What the unprotected package is rendered with.
        let settings = package_stack_settings(name, &stack()).expect("package settings");
        let unprotected = render_setup_template(name, &stack(), &settings, &options)
            .await
            .expect("unprotected module renders");
        let protected = terraform_resources(&rendered[name].bytes);
        let missing: Vec<_> = terraform_resources(&unprotected.bytes)
            .into_iter()
            .filter(|address| !protected.contains(address))
            .collect();
        assert_eq!(missing, Vec::<String>::new(), "{name}");
        for variable in &unprotected.allowed {
            if variable.starts_with("network") || variable == "availability_zones" {
                assert!(
                    rendered[name].allowed.contains(variable),
                    "{name}: the protected module lets the customer choose {variable}"
                );
            }
        }
    }
}

/// The expression the last `key = <expression>` in whitespace-collapsed HCL sets: up to the
/// next `,`, closing bracket or `<key> =` at its own bracket depth.
fn hcl_value<'a>(text: &'a str, key: &str) -> &'a str {
    let start = text
        .rfind(&format!(" {key} = "))
        .unwrap_or_else(|| panic!("no {key} in {text}"))
        + key.len()
        + 4;
    let rest = &text[start..];
    let next_key = |at: usize| {
        let word: String = rest[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        !word.is_empty() && rest[at + 1 + word.len()..].starts_with(" = ")
    };
    let mut depth = 0i32;
    for (offset, character) in rest.char_indices() {
        match character {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' if depth == 0 => return rest[..offset].trim(),
            ']' | ')' | '}' => depth -= 1,
            ',' if depth == 0 => return rest[..offset].trim(),
            ' ' if depth == 0 && next_key(offset) => return rest[..offset].trim(),
            _ => {}
        }
    }
    rest.trim()
}

/// A protected install's verifier puts functions in the subnets setup reports for the network,
/// the ones the worker controller puts an unprotected install's functions in, so protecting an
/// install keeps its functions in its network.
#[tokio::test]
async fn the_verifier_runs_functions_in_the_subnets_setup_reports_for_the_network() {
    let rendered = render(FINGERPRINT).await;
    for (name, reported) in [
        ("terraform-aws", "privateSubnetIds"),
        ("terraform-gcp", "subnetSelfLinks"),
    ] {
        let files: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&rendered[name].bytes).expect("a module is a JSON file map");
        let signing = files["verifier.tf"]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let at = signing
            .find("VERIFIER_FUNCTION_VPC")
            .unwrap_or_else(|| panic!("{name}: the verifier is given no function network"));
        let given = hcl_value(&signing[at..], "subnetIds");
        let locals = files["locals.tf"]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(given, hcl_value(&locals, reported), "{name}");
    }
}

/// A stack that only gives access to resources running elsewhere: its unprotected package
/// creates no network and registers telemetry off, so the protected module doesn't either.
#[tokio::test]
async fn an_access_only_stack_gets_no_network_when_protected() {
    let mut stack = stack();
    stack
        .resources
        .get_mut("assets")
        .expect("assets")
        .remote_access = true;
    let signing = StackSigning {
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    };
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let rendered = render_setup_templates(
        &stack,
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .hosting(hosting())
            .pins(pins())
            .build(),
    )
    .await
    .expect("protected setup templates render");
    for name in ["terraform-aws", "terraform-gcp", "terraform-azure"] {
        let networks: Vec<String> = terraform_resources(&rendered[name].bytes)
            .into_iter()
            .filter(|address| address.contains("default_network"))
            .collect();
        assert_eq!(networks, Vec::<String>::new(), "{name}");
        assert!(
            !rendered[name]
                .allowed
                .iter()
                .any(|variable| variable == "network_mode"),
            "{name}: {:?}",
            rendered[name].allowed
        );
    }
}

/// The value `expression` of a rendered CloudFormation template takes when the stack is created
/// with every parameter at its default: `Ref`, `Fn::Join`, `Fn::If` and the condition
/// functions, as CloudFormation evaluates them.
fn evaluate_cloudformation(
    template: &serde_yaml::Value,
    expression: &serde_yaml::Value,
) -> serde_yaml::Value {
    use serde_yaml::Value;
    let parameter = |name: &str| -> Value {
        let parameter = &template["Parameters"][name];
        assert!(
            !parameter.is_null(),
            "{name} is not a parameter of the template"
        );
        match &parameter["Default"] {
            Value::Number(number) => Value::String(number.to_string()),
            Value::Null => Value::String(String::new()),
            default => default.clone(),
        }
    };
    let condition = |name: &str| -> bool {
        evaluate_cloudformation(template, &template["Conditions"][name])
            .as_bool()
            .unwrap_or_else(|| panic!("condition {name} isn't a boolean"))
    };
    let Value::Mapping(fields) = expression else {
        return match expression {
            Value::Sequence(items) => Value::Sequence(
                items
                    .iter()
                    .map(|item| evaluate_cloudformation(template, item))
                    .collect(),
            ),
            other => other.clone(),
        };
    };
    let Some((function, arguments)) = fields.iter().next() else {
        return expression.clone();
    };
    let arguments_list = || arguments.as_sequence().cloned().unwrap_or_default();
    match function.as_str().unwrap_or_default() {
        "Ref" => parameter(arguments.as_str().expect("Ref names a parameter")),
        "Condition" => Value::Bool(condition(arguments.as_str().expect("condition name"))),
        "Fn::If" => {
            let arguments = arguments_list();
            let taken = if condition(arguments[0].as_str().expect("condition name")) {
                &arguments[1]
            } else {
                &arguments[2]
            };
            evaluate_cloudformation(template, taken)
        }
        // CloudFormation compares scalars as strings: a Number parameter's Ref is a string.
        "Fn::Equals" => {
            let arguments = arguments_list();
            let text = |value: Value| match value {
                Value::String(text) => text,
                other => serde_yaml::to_string(&other).unwrap().trim().to_string(),
            };
            Value::Bool(
                text(evaluate_cloudformation(template, &arguments[0]))
                    == text(evaluate_cloudformation(template, &arguments[1])),
            )
        }
        "Fn::Not" => Value::Bool(
            !evaluate_cloudformation(template, &arguments_list()[0])
                .as_bool()
                .expect("Fn::Not takes a condition"),
        ),
        function @ ("Fn::And" | "Fn::Or") => {
            let values: Vec<bool> = arguments_list()
                .iter()
                .map(|argument| {
                    evaluate_cloudformation(template, argument)
                        .as_bool()
                        .expect("a condition")
                })
                .collect();
            Value::Bool(if function == "Fn::And" {
                values.iter().all(|value| *value)
            } else {
                values.iter().any(|value| *value)
            })
        }
        "Fn::Join" => {
            let arguments = arguments_list();
            let delimiter = arguments[0].as_str().expect("delimiter");
            let items = evaluate_cloudformation(template, &arguments[1]);
            let items: Vec<String> = items
                .as_sequence()
                .expect("Fn::Join takes a list")
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    Value::Sequence(_) => panic!("a list inside Fn::Join"),
                    other => serde_yaml::to_string(other).unwrap().trim().to_string(),
                })
                .collect();
            Value::String(items.join(delimiter))
        }
        other => panic!("{other} isn't evaluated here"),
    }
}

/// A protected stack registers as protected: the settings the CloudFormation template reports
/// carry the fingerprint it pins and the channel it was signed for. (Without them the service
/// recorded a stack that protected itself as unprotected.)
#[tokio::test]
async fn a_protected_cloudformation_stack_registers_its_signing_settings() {
    let rendered = render(FINGERPRINT).await;
    let template: serde_yaml::Value =
        serde_yaml::from_slice(&rendered["cloudformation"].bytes).expect("template is YAML");
    let reported = evaluate_cloudformation(
        &template,
        &template["Outputs"]["DeploymentStackSettings"]["Value"],
    );
    let settings: serde_json::Value = serde_json::from_str(
        reported
            .as_str()
            .expect("the settings are reported as JSON text"),
    )
    .expect("the reported settings are JSON");
    let signing: Option<StackSigning> = serde_json::from_value(settings["signing"].clone())
        .expect("the reported signing settings parse as the service parses them");
    assert_eq!(
        signing,
        Some(StackSigning {
            fingerprint: FINGERPRINT.to_string(),
            channel: "production".to_string(),
        })
    );
}

/// The protected CloudFormation template a stack is created from when the service registers it
/// through its custom resource, as the console's quick-create does, with `pins`.
async fn custom_resource_template(pins: SetupPins) -> String {
    let signing = StackSigning {
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    };
    let hosting: SetupHostingOptions = serde_json::from_value(serde_json::json!({
        "registration": {
            "mode": "customResource",
            "lambdaArn": "arn:aws:lambda:us-east-1:999988887777:function:register"
        }
    }))
    .expect("hosting options parse");
    let cloudformation = CfRegistry::built_in();
    let terraform = TfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let rendered = render_setup_templates(
        &stack_with_function(),
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .hosting(hosting)
            .pins(pins)
            .build(),
    )
    .await
    .expect("protected setup templates render");
    String::from_utf8(rendered["cloudformation"].bytes.clone()).expect("UTF-8 template")
}

/// CloudFormation re-runs the registration custom resource only when its properties change. A
/// signed setup update may change only what the install runs (here, the verifier image), so the
/// resource carries the digest of the rest of the template: the service hears of every setup
/// update when it runs, and of none when nothing changed.
#[tokio::test]
async fn every_protected_setup_update_runs_the_registration_again() {
    let revision = |template: &str| -> String {
        let parsed: serde_yaml::Value = serde_yaml::from_str(template).expect("template is YAML");
        parsed["Resources"]["DeploymentRegistration"]["Properties"]["SetupRevision"]
            .as_str()
            .expect("the registration carries a SetupRevision")
            .to_string()
    };
    let first = custom_resource_template(pins()).await;
    let again = custom_resource_template(pins()).await;
    assert_eq!(
        revision(&first),
        revision(&again),
        "nothing changed, nothing re-runs"
    );

    let mut newer = pins();
    newer.aws_verifier_image = Some(format!("verifier@sha256:{}", "1a".repeat(32)));
    let update = custom_resource_template(newer).await;
    assert_ne!(
        revision(&first),
        revision(&update),
        "a new verifier image re-runs the registration"
    );

    // It is the digest of the template without it.
    let line = format!("      SetupRevision: {}\n", revision(&first));
    assert!(first.contains(&line), "{line}");
    let without = first.replacen(&line, "", 1);
    assert_eq!(
        revision(&first),
        format!(
            "sha256:{}",
            hex::encode(<sha2::Sha256 as sha2::Digest>::digest(without.as_bytes()))
        )
    );
}

/// A protected stack whose workloads are none of the verifier's (here, storage only) gets the
/// signing settings and management's limits, and no verifier.
#[tokio::test]
async fn a_protected_stack_without_functions_or_machines_gets_no_verifier() {
    let rendered = render_stack(FINGERPRINT, &stack()).await;
    let template: serde_yaml::Value =
        serde_yaml::from_slice(&rendered["cloudformation"].bytes).expect("template is YAML");
    let resources = template["Resources"].as_mapping().expect("resources");
    assert!(resources.contains_key("ManagementLimits"));
    assert!(!resources.contains_key("SigningVerifier"));
    assert!(!resources.contains_key("SigningVerifierRole"));
    for name in ["terraform-aws", "terraform-gcp", "terraform-azure"] {
        let files: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&rendered[name].bytes).expect("a module is a JSON file map");
        assert!(!files.contains_key("verifier.tf"), "{name}");
        assert!(
            !files
                .values()
                .any(|file| file.contains("\"signing_verifier\"")),
            "{name}: no verifier block"
        );
        assert!(files.contains_key("signing.tf"), "{name}");
    }
}
