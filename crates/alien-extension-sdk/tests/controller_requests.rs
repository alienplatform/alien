//! The requests a controller module builds, read as Alien's host reads them. The host refuses a
//! request with a field it doesn't know, so each builder must write exactly the host's fields.

use alien_core::Platform;
use alien_extension_sdk::controller_prelude::{CloudRequest, HttpRequest};
use alien_infra::step_protocol::wasm;
use serde_json::json;

fn as_the_host_reads(request: &CloudRequest) -> wasm::CloudRequest {
    serde_json::from_value(serde_json::to_value(request).unwrap())
        .expect("the host reads the request")
}

#[test]
fn each_cloud_s_builder_names_the_cloud_and_service_the_host_expects() {
    let aws = as_the_host_reads(
        &CloudRequest::aws("api.ecr", "POST", "/")
            .region("eu-west-1")
            .header("content-type", "application/x-amz-json-1.1")
            .body("{}"),
    );
    assert_eq!(
        (aws.cloud, aws.service.as_str(), aws.region.as_deref()),
        (Platform::Aws, "api.ecr", Some("eu-west-1"))
    );
    assert_eq!((aws.method.as_str(), aws.path.as_str()), ("POST", "/"));
    assert_eq!(aws.body.as_deref(), Some("{}"));
    assert_eq!(aws.body_base64, None);

    let gcp = as_the_host_reads(
        &CloudRequest::gcp("dns", "GET", "/dns/v1/projects/p/managedZones")
            .query("maxResults", "10"),
    );
    assert_eq!(
        (gcp.cloud, gcp.service.as_str(), gcp.region),
        (Platform::Gcp, "dns", None)
    );
    assert_eq!(gcp.query, [("maxResults".to_string(), "10".to_string())]);
    assert_eq!((gcp.body, gcp.body_base64), (None, None));

    let azure = as_the_host_reads(
        &CloudRequest::azure("PUT", "/subscriptions/s/resourceGroups/g")
            .json(&json!({ "location": "westeurope" }))
            .unwrap(),
    );
    assert_eq!(
        (azure.cloud, azure.service.as_str()),
        (Platform::Azure, "management")
    );
    assert_eq!(
        azure.headers,
        [("content-type".to_string(), "application/json".to_string())]
    );
    assert_eq!(azure.body.as_deref(), Some(r#"{"location":"westeurope"}"#));
}

/// A body that is not text travels in base64, in the field the host decodes, and never beside
/// a text body.
#[test]
fn a_body_that_is_not_text_is_sent_in_base64() {
    let cloud = as_the_host_reads(&CloudRequest::aws("s3", "PUT", "/b/k").body_bytes(&[0xff, 0]));
    assert_eq!(
        (cloud.body, cloud.body_base64.as_deref()),
        (None, Some("/wA="))
    );
    let text = as_the_host_reads(&CloudRequest::aws("s3", "PUT", "/b/k").body_bytes(b"text"));
    assert_eq!(
        (text.body.as_deref(), text.body_base64),
        (Some("text"), None)
    );

    let http: wasm::HttpRequest = serde_json::from_value(
        serde_json::to_value(
            HttpRequest::new("POST", "https://api.acme.dev/v1/zones")
                .header("authorization", "Bearer key")
                .body_bytes(&[0xff, 0]),
        )
        .unwrap(),
    )
    .expect("the host reads the request");
    assert_eq!(
        (http.method.as_str(), http.url.as_str()),
        ("POST", "https://api.acme.dev/v1/zones")
    );
    assert_eq!(
        (http.body, http.body_base64.as_deref()),
        (None, Some("/wA="))
    );
}
