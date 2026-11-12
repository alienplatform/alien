//! The Operator reads a chart's `infrastructure` values as `ExternalBindings` (alien-operator
//! cli.rs, from the chart's `external-bindings.json`). The JSON below is what
//! `alien render --format helm` writes for the full example's zone, after `toJson`:
//! `examples/onprem.yaml`'s `infrastructure`.

use alien_core::ExternalBindings;

#[test]
fn the_operator_reads_the_full_example_s_zone_binding() {
    let chart_infrastructure =
        r#"{"zone":{"type":"acme/dns-zone","service":"route53","zoneId":"your-zone-id"}}"#;
    let parsed = serde_json::from_str::<ExternalBindings>(chart_infrastructure);
    assert!(parsed.is_ok(), "{parsed:?}");
}

#[test]
fn the_operator_reads_a_built_in_binding() {
    let chart_infrastructure = r#"{"data":{"type":"storage","service":"s3","bucketName":"b"}}"#;
    let parsed = serde_json::from_str::<ExternalBindings>(chart_infrastructure);
    assert!(parsed.is_ok(), "{parsed:?}");
}
