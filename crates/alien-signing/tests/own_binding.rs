//! A workload's own binding tells its code where to reach itself, so the enforcer accepts
//! only what the install and the cloud decide.

use alien_signing::{check_own_binding, BindingContext, OwnBindingFacts};
use serde_json::{json, Value};

const PUBLIC: &str = "https://api.acme.example";
const RUN_APP: &str = "https://acme-prod-api-abc.a.run.app";
const LAMBDA_URL: &str = "https://abc.lambda-url.us-east-1.on.aws/";

fn facts(platform: &str) -> OwnBindingFacts {
    OwnBindingFacts {
        context: Some(BindingContext {
            platform: platform.to_string(),
            prefix: "acme-prod".to_string(),
            account: match platform {
                "aws" => "111111111111",
                "gcp" => "acme-project",
                _ => "00000000-0000-0000-0000-000000000000",
            }
            .to_string(),
            region: Some(
                match platform {
                    "aws" => "us-east-1",
                    "gcp" => "us-central1",
                    _ => "eastus",
                }
                .to_string(),
            ),
        }),
        resource_name: "acme-prod-api".to_string(),
        resource_group: Some("acme-rg".to_string()),
        location: Some("us-central1".to_string()),
        urls: match platform {
            "aws" => vec![
                LAMBDA_URL.to_string(),
                "https://abc123.execute-api.us-east-1.amazonaws.com".to_string(),
            ],
            "machines" => vec![PUBLIC.to_string()],
            _ => vec![RUN_APP.to_string()],
        },
    }
}

fn aws() -> Value {
    json!({
        "service": "lambda",
        "workerName": "acme-prod-api",
        "region": "us-east-1",
        "url": LAMBDA_URL,
    })
}

fn gcp() -> Value {
    json!({
        "service": "cloudrun",
        "projectId": "acme-project",
        "serviceName": "acme-prod-api",
        "location": "us-central1",
        "privateUrl": RUN_APP,
        "publicUrl": RUN_APP,
    })
}

fn azure() -> Value {
    json!({
        "service": "containerapp",
        "subscriptionId": "00000000-0000-0000-0000-000000000000",
        "resourceGroupName": "acme-rg",
        "containerAppName": "acme-prod-api",
        "privateUrl": RUN_APP,
        "publicUrl": RUN_APP,
    })
}

fn machines() -> Value {
    json!({
        "containerName": "acme-prod-api",
        "internalUrl": "http://acme-prod-api.svc:8080",
        "internalUrls": {"8080": "http://acme-prod-api.svc:8080"},
        "publicUrl": PUBLIC,
    })
}

fn check(platform: &str, binding: &Value) -> Result<(), String> {
    check_own_binding(&binding.to_string(), &facts(platform))
}

#[test]
fn the_binding_a_controller_writes_for_its_own_resource_passes() {
    for (platform, binding) in [
        ("aws", aws()),
        ("gcp", gcp()),
        ("azure", azure()),
        ("machines", machines()),
    ] {
        check(platform, &binding).unwrap_or_else(|e| panic!("{platform}: {e}"));
    }
    let mut without_urls = aws();
    without_urls.as_object_mut().unwrap().remove("url");
    check("aws", &without_urls).expect("a Lambda binding without a URL");
    let mut binding = aws();
    binding["url"] = json!("https://abc123.execute-api.us-east-1.amazonaws.com");
    check("aws", &binding).expect("the address of an API in the install's account");
}

#[test]
fn a_binding_that_points_anywhere_else_is_refused() {
    let attacker = json!("https://collector.attacker.example");
    let cases: Vec<(&str, Value, &str, Value, &str)> = vec![
        (
            "aws",
            aws(),
            "workerName",
            json!("acme-prod-other"),
            "workerName",
        ),
        ("aws", aws(), "region", json!("eu-west-1"), "region"),
        ("aws", aws(), "url", attacker.clone(), "its url is"),
        ("aws", aws(), "service", json!("cloudrun"), "service"),
        (
            "gcp",
            gcp(),
            "projectId",
            json!("other-project"),
            "projectId",
        ),
        (
            "gcp",
            gcp(),
            "serviceName",
            json!("acme-prod-other"),
            "serviceName",
        ),
        ("gcp", gcp(), "location", json!("europe-west1"), "location"),
        ("gcp", gcp(), "privateUrl", attacker.clone(), "privateUrl"),
        ("gcp", gcp(), "publicUrl", attacker.clone(), "publicUrl"),
        (
            "azure",
            azure(),
            "subscriptionId",
            json!("other"),
            "subscriptionId",
        ),
        (
            "azure",
            azure(),
            "resourceGroupName",
            json!("other-rg"),
            "resourceGroupName",
        ),
        (
            "azure",
            azure(),
            "containerAppName",
            json!("acme-prod-other"),
            "containerAppName",
        ),
        (
            "azure",
            azure(),
            "privateUrl",
            attacker.clone(),
            "privateUrl",
        ),
        ("azure", azure(), "publicUrl", attacker.clone(), "publicUrl"),
        (
            "machines",
            machines(),
            "containerName",
            json!("other"),
            "containerName",
        ),
        (
            "machines",
            machines(),
            "service",
            json!("https://attacker.example"),
            "service",
        ),
        (
            "machines",
            machines(),
            "internalUrl",
            json!("http://other.svc:8080"),
            "internalUrl",
        ),
        (
            "machines",
            machines(),
            "internalUrls",
            json!({"8080": "http://other.svc:8080"}),
            "internalUrls",
        ),
        (
            "machines",
            machines(),
            "internalUrls",
            json!("http://acme-prod-api.svc:8080"),
            "internalUrls",
        ),
        (
            "machines",
            machines(),
            "publicUrl",
            attacker.clone(),
            "publicUrl",
        ),
        ("aws", aws(), "callback", attacker.clone(), "field callback"),
        ("gcp", gcp(), "callback", attacker.clone(), "field callback"),
        (
            "azure",
            azure(),
            "callback",
            attacker.clone(),
            "field callback",
        ),
        (
            "machines",
            machines(),
            "callback",
            attacker,
            "field callback",
        ),
    ];
    for (platform, mut binding, field, value, says) in cases {
        binding[field] = value;
        let error = check(platform, &binding).expect_err(&format!("{platform} {field}"));
        assert!(error.contains(says), "{platform} {field}: {error}");
    }
}

/// A URL the control plane names, such as a custom or generated domain, is no address the
/// enforcer read from the cloud, so it can't fill a display field or a destination, and a host
/// that only looks like one of AWS's gateway hosts, in another account, passes no more.
#[test]
fn a_url_the_control_plane_names_is_no_url_the_cloud_reports() {
    let forged_gateway = "https://evil1.execute-api.us-east-1.amazonaws.com";
    for (platform, mut binding, field, value) in [
        ("aws", aws(), "url", PUBLIC),
        ("aws", aws(), "url", forged_gateway),
        (
            "aws",
            aws(),
            "url",
            "https://evil1.lambda-url.us-east-1.on.aws",
        ),
        ("gcp", gcp(), "privateUrl", PUBLIC),
        ("gcp", gcp(), "publicUrl", PUBLIC),
        ("azure", azure(), "privateUrl", PUBLIC),
        ("azure", azure(), "publicUrl", PUBLIC),
        ("machines", machines(), "publicUrl", "https://evil.example"),
    ] {
        binding[field] = json!(value);
        let error = check(platform, &binding).expect_err(&format!("{platform} {field} {value}"));
        assert!(error.contains(field), "{platform} {field}: {error}");
    }
}

#[test]
fn the_urls_a_binding_must_carry_cannot_be_left_out() {
    for (platform, field) in [("gcp", "privateUrl"), ("azure", "privateUrl")] {
        let mut binding = if platform == "gcp" { gcp() } else { azure() };
        binding.as_object_mut().unwrap().remove(field);
        let error = check(platform, &binding).expect_err(platform);
        assert!(error.contains("no privateUrl"), "{platform}: {error}");
    }
}

#[test]
fn a_binding_the_enforcer_has_nothing_to_check_against_is_refused() {
    let error =
        check_own_binding(&gcp().to_string(), &OwnBindingFacts::default()).expect_err("no install");
    assert!(error.contains("knows no install"), "{error}");
    let error = check("kubernetes", &json!({"name": "api"})).expect_err("kubernetes");
    assert!(error.contains("no own binding"), "{error}");
    let error = check_own_binding("api", &facts("gcp")).expect_err("not JSON");
    assert!(error.contains("isn't a JSON object"), "{error}");
}
