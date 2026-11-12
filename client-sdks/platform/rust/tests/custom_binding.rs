use alien_platform_api::types::{ExternalBinding, ExternalBindings};
use serde_json::json;

#[test]
fn a_custom_binding_decodes_and_built_ins_keep_their_variant() {
    let custom = json!({ "type": "acme/dns-zone", "service": "route53", "zoneId": "Z1" });
    let decoded: ExternalBinding = serde_json::from_value(custom.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), custom);
    let dbg = format!("{decoded:?}");
    assert!(dbg.contains("Z1"), "{dbg}");

    let storage = json!({ "type": "storage", "service": "gcs", "bucketName": "demo" });
    let decoded: ExternalBinding = serde_json::from_value(storage.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), storage);
    assert!(!format!("{decoded:?}").contains("route53"));

    for bad in [
        json!({ "type": "alien/dns-zone", "service": "x" }),
        json!({ "type": "unknown", "service": "s3", "bucketName": "demo" }),
        json!({ "type": "acme/dns-zone", "zoneId": "Z1" }),
    ] {
        assert!(
            serde_json::from_value::<ExternalBinding>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    let map: ExternalBindings =
        serde_json::from_value(json!({ "zone": custom, "files": storage })).unwrap();
    assert_eq!(serde_json::to_value(map).unwrap()["zone"]["zoneId"], "Z1");
}
