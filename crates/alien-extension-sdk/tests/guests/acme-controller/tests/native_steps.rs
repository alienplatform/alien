//! The docs' controller library in a native test, as an extension's own `cargo test` runs it:
//! `testing::answer` steps a controller with the test answering its calls in Alien's place.

use alien_extension_sdk::controller_module::testing::answer;
use alien_extension_sdk::controller_prelude::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn create(platform: &str, cloud: Value) -> Value {
    json!({
        "protocol": 1, "stepId": "step-1", "resourceType": "acme/dns-zone",
        "platform": platform, "resourcePrefix": "q6aa0b26",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {}, "state": null, "transitions": [],
        "cloud": cloud, "deadline": "2099-01-01T00:00:00Z"
    })
}

#[test]
fn the_google_cloud_controller_creates_the_zone_it_names() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let seen = sent.clone();
    let reply = answer(
        acme_controller::register,
        &create(
            "gcp",
            json!({ "platform": "gcp", "projectId": "the-project", "region": "us-central1" }),
        ),
        move |request| {
            seen.lock().unwrap().push(request.clone());
            Ok(json!({ "status": 200, "headers": [], "body": "{}" }))
        },
    )
    .unwrap();

    assert_eq!(reply["result"], "stepped", "{reply}");
    assert_eq!(reply["controller"]["status"], "running");
    assert_eq!(reply["controller"]["outputs"]["zoneId"], "q6aa0b26-zone");
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["cloud"], "gcp");
    assert_eq!(sent[0]["service"], "dns");
    assert_eq!(sent[0]["method"], "POST");
    assert_eq!(sent[0]["path"], "/dns/v1/projects/the-project/managedZones");
}

/// The AWS SDK's requests reach the test too, and a call Alien refuses fails the step for good.
#[test]
fn the_aws_controller_s_sdk_calls_reach_the_test() {
    let reply = answer(
        acme_controller::register,
        &create(
            "aws",
            json!({ "platform": "aws", "accountId": "123456789012", "region": "eu-west-1" }),
        ),
        |request| {
            assert_eq!(request["service"], "route53");
            assert_eq!(request["region"], "us-east-1");
            assert_eq!(request["path"], "/2013-04-01/hostedzone");
            Err("refused: not in this test".to_string())
        },
    )
    .unwrap();

    assert_eq!(reply["result"], "failed", "{reply}");
    assert_eq!(reply["error"]["retryable"], false);
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("caused by EXTENSION_CALL_REFUSED"),
        "{reply}"
    );
}

/// Two controllers for one type and platform leave the module unable to answer anything, so a
/// build that asks it for its controllers fails on them.
#[test]
fn a_type_registered_twice_on_a_platform_is_no_module() {
    fn twice(controllers: &mut Controllers) {
        acme_controller::register(controllers);
        acme_controller::register(controllers);
    }
    let error = answer(
        twice,
        &json!({ "protocol": 1, "op": "controllers" }),
        |_| Err("refused: listing makes no call".to_string()),
    )
    .unwrap_err();
    assert!(
        error.contains("two controllers are registered for acme/dns-zone on aws"),
        "{error}"
    );
}
