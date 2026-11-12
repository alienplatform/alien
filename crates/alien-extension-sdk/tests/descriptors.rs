//! Each resource type's descriptor, which the module's manifest carries for `alien extensions
//! build`: what `#[alien_resource]` and `register` declare, and the checks on it.

use acme_types::{AwsDnsZoneFields, AwsDnsZoneImport, DnsZone, DnsZoneOutputs};
use alien_core::hooks::{HookKind, ImportInput};
use alien_extension_sdk::prelude::*;
use alien_extension_sdk::testing::check_link_permission_sets;

const PROVISION: &str = r#"{
  // JSONC, as the docs write permission sets.
  "id": "acme/dns-zone/provision",
  "description": "Create, read and delete the zone",
  "platforms": { "aws": [{ "grant": { "actions": ["route53:CreateHostedZone"] },
    "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } } }] }
}"#;

const RECORDS_WRITE: &str = r#"{
  "id": "acme/dns-zone/records-write",
  "description": "Write the zone's records",
  "platforms": { "aws": [{ "grant": { "actions": ["route53:ChangeResourceRecordSets"] },
    "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } } }] }
}"#;

fn descriptor(extension: &Extension) -> Value {
    let manifest = extension.manifest().unwrap_or_else(|e| panic!("{e}"));
    serde_json::to_value(&manifest.resource_types[&DnsZone::RESOURCE_TYPE.resource_type()]).unwrap()
}

#[test]
fn the_full_examples_descriptor_comes_from_its_type_and_register() {
    let descriptor = descriptor(&acme_hooks::extension());
    assert_eq!(descriptor["lifecycles"], json!(["frozen", "live"]));
    assert_eq!(descriptor["holdsData"], false);
    assert_eq!(
        descriptor["setup"],
        json!({
            "cloudformation": ["aws"],
            "terraform": ["aws", "gcp", "azure"],
            "helm": ["kubernetes"],
            "importers": ["aws", "gcp", "azure"],
        })
    );
    // The controller binary lists these; `alien extensions build` adds them.
    assert_eq!(descriptor["controllerPlatforms"], json!([]));
    let ids: Vec<&str> = descriptor["permissionSets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|set| set["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["acme/dns-zone/provision", "acme/dns-zone/records-write"]
    );
    assert_eq!(
        descriptor["link"],
        json!({ "permissionSets": ["acme/dns-zone/records-write"] })
    );
    assert_eq!(
        descriptor["services"],
        json!({ "gcp": ["dns.googleapis.com"], "azure": ["Microsoft.Network"] })
    );
    let config = &descriptor["schemas"]["config"];
    assert_eq!(config["type"], "object");
    assert_eq!(config["required"], json!(["id", "domain"]));
    assert_eq!(
        config["properties"]["domain"]["description"],
        "The zone's domain, for example \"internal.acme.dev\"."
    );
    assert_eq!(
        descriptor["schemas"]["outputs"]["properties"]["zoneId"]["type"],
        "string"
    );
}

#[test]
fn macro_writes_the_type_alien_reads() {
    let zone = DnsZone::new("zone".to_string())
        .domain("internal.acme.dev".to_string())
        .build();
    assert_eq!(zone.get_resource_type().0, "acme/dns-zone");
    assert!(!zone.replace_after_failed_create_is_safe());
    let other = DnsZone::new("other".to_string())
        .domain("internal.acme.dev".to_string())
        .build();
    let error = zone.validate_update(&other).unwrap_err();
    assert!(error.message.contains("'id' field is immutable"), "{error}");
    let outputs = ResourceOutputs::new(DnsZoneOutputs {
        zone_id: "Z1".to_string(),
    });
    assert_eq!(
        serde_json::to_value(&outputs).unwrap(),
        json!({ "type": "acme/dns-zone", "zoneId": "Z1" })
    );
}

fn zone_with_link(link: fn(&DnsZoneOutputs) -> Link) -> Extension {
    Extension::new("acme", move |ext| {
        ext.resource::<DnsZone>()
            .permission_set(PROVISION)
            .permission_set(RECORDS_WRITE)
            .link(link);
    })
}

/// A link grants only the type's own sets: a built-in set such as `storage/data-read`, or
/// another extension's, would give a workload access the type never declared.
#[test]
fn a_link_granting_a_set_the_type_does_not_declare_fails_the_manifest() {
    for foreign in ["storage/data-read", "other/zone/records-write"] {
        let ext = zone_with_link(match foreign {
            "storage/data-read" => |_| Link::new().permission_set("storage/data-read"),
            _ => |_| Link::new().permission_set("other/zone/records-write"),
        });
        let error = ext.manifest().unwrap_err().message;
        assert!(
            error.contains(&format!(
                "`acme/dns-zone`'s link grants `{foreign}`, which is not one of the type's own permission sets"
            )),
            "{error}"
        );
    }
    let own = zone_with_link(|_| Link::new().permission_set("acme/dns-zone/records-write"));
    own.manifest().unwrap();
}

/// A set's name becomes its file in the bundle, so it is one plain segment.
#[test]
fn a_permission_set_name_that_is_not_a_plain_segment_fails_the_manifest() {
    for id in [
        "acme/dns-zone/../../x",
        "acme/dns-zone/Write",
        "acme/dns-zone/a_b",
        "acme/dns-zone/",
    ] {
        let ext = Extension::new("acme", |ext| {
            ext.resource::<DnsZone>()
                .permission_set(&PROVISION.replace("acme/dns-zone/provision", id));
        });
        let error = ext.manifest().unwrap_err().message;
        assert!(
            error.contains(&format!(
                "the permission set `{id}` of `acme/dns-zone` must be named"
            )),
            "{id}: {error}"
        );
    }
}

#[test]
fn a_permission_set_named_for_another_type_fails_the_manifest() {
    let ext = Extension::new("acme", |ext| {
        ext.resource::<DnsZone>()
            .permission_set(&PROVISION.replace("acme/dns-zone/provision", "acme/provision"));
    });
    let error = ext.manifest().unwrap_err().message;
    assert!(
        error.contains("must be named `acme/dns-zone/<name>`"),
        "{error}"
    );
    let ext = Extension::new("acme", |ext| {
        ext.resource::<DnsZone>().permission_set("{ not json");
    });
    assert!(ext
        .manifest()
        .unwrap_err()
        .message
        .contains("is not a permission set"));
}

struct ImportsWith(fn(AwsDnsZoneImport, &ImportContext<'_>) -> Result<StackResourceState>);

impl ResourceImporter for ImportsWith {
    type ImportData = AwsDnsZoneImport;

    fn import(
        &self,
        data: AwsDnsZoneImport,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        (self.0)(data, ctx)
    }
}

fn import(
    importer: ImportsWith,
    link: fn(&DnsZoneOutputs) -> Link,
) -> (Extension, std::result::Result<Value, String>) {
    let ext = Extension::new("acme", move |ext| {
        ext.resource::<DnsZone>()
            .permission_set(PROVISION)
            .permission_set(RECORDS_WRITE)
            .importer(Platform::Aws, importer)
            .link(link);
    });
    let fixture = fixture();
    let hook = ext
        .manifest()
        .unwrap()
        .hooks
        .into_iter()
        .find(|entry| entry.kind == HookKind::Importer)
        .unwrap()
        .name;
    let output = ext
        .call(&hook, &serde_json::to_vec(&fixture).unwrap())
        .map(|output| serde_json::from_slice(&output).unwrap())
        .map_err(|error| error.message);
    (ext, output)
}

fn fixture() -> ImportInput {
    ImportInput {
        resource_id: "zone".into(),
        platform: Platform::Aws,
        region: "us-east-1".into(),
        stack_settings: Default::default(),
        management_config: None,
        resource: json!({
            "config": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
            "lifecycle": "frozen",
            "dependencies": []
        }),
        import_data: json!({ "zoneId": "Z123" }),
    }
}

fn records_write(_: &DnsZoneOutputs) -> Link {
    Link::new().permission_set("acme/dns-zone/records-write")
}

#[test]
fn ready_saves_no_fields_for_unit_and_refuses_none() {
    let (_, unit) = import(
        ImportsWith(|data, ctx| {
            ctx.ready(
                (),
                DnsZoneOutputs {
                    zone_id: data.zone_id,
                },
            )
        }),
        records_write,
    );
    assert_eq!(unit.unwrap()["internalState"], json!({ "state": "ready" }));

    let (_, none) = import(
        ImportsWith(|data, ctx| {
            ctx.ready(
                None::<AwsDnsZoneFields>,
                DnsZoneOutputs {
                    zone_id: data.zone_id,
                },
            )
        }),
        records_write,
    );
    let error = none.unwrap_err();
    assert!(
        error.contains("the controller's fields must serialize as a JSON object"),
        "{error}"
    );
}

#[test]
fn a_link_whose_sets_depend_on_the_outputs_fails_the_fixture_check() {
    let ready = ImportsWith(|data, ctx| {
        ctx.ready(
            AwsDnsZoneFields {
                zone_id: Some(data.zone_id.clone()),
            },
            DnsZoneOutputs {
                zone_id: data.zone_id,
            },
        )
    });
    let (ext, _) = import(ready, records_write);
    let manifest = ext.manifest().unwrap();
    check_link_permission_sets(
        &manifest,
        |hook, input| ext.call(hook, input).map_err(|error| error.message),
        &[fixture()],
    )
    .unwrap();

    let ready = ImportsWith(|data, ctx| {
        ctx.ready(
            AwsDnsZoneFields {
                zone_id: Some(data.zone_id.clone()),
            },
            DnsZoneOutputs {
                zone_id: data.zone_id,
            },
        )
    });
    // Placeholder outputs have an empty zone id, so `register` sees no sets; a fixture has one.
    let (ext, _) = import(ready, |outputs| {
        if outputs.zone_id.is_empty() {
            Link::new()
        } else {
            Link::new().permission_set("acme/dns-zone/records-write")
        }
    });
    let manifest = ext.manifest().unwrap();
    let error = check_link_permission_sets(
        &manifest,
        |hook, input| ext.call(hook, input).map_err(|error| error.message),
        &[fixture()],
    )
    .unwrap_err();
    assert!(
        error.contains("a link's permission sets can't depend on the outputs"),
        "{error}"
    );
}

fn nonempty<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    let url = String::deserialize(d)?;
    if url.is_empty() {
        return Err(serde::de::Error::custom("an endpoint is never empty"));
    }
    Ok(url)
}

#[alien_resource(type = "probe/endpoint", outputs = EndpointOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointOutputs {
    #[serde(deserialize_with = "nonempty")]
    pub url: String,
}

/// `register` reads a link's permission sets by running it on placeholder outputs, so outputs
/// that refuse an empty string or a zero can't have a link.
#[test]
fn a_link_on_outputs_that_refuse_placeholders_fails_the_manifest() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<Endpoint>()
            .link(|outputs| Link::new().env("PROBE_URL", &outputs.url));
    });
    let error = ext.manifest().unwrap_err().message;
    assert!(
        error.contains(
            "`probe/endpoint`'s link can't run on placeholder outputs to read its permission sets: an endpoint is never empty"
        ),
        "{error}"
    );
}

#[alien_resource(type = "probe/zone", outputs = NestedZoneOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NestedZone {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ZoneKind {
    Public,
    Private,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NameServer {
    pub host: String,
    pub ttl: u32,
}

/// Outputs with every shape utoipa writes for Rust's types: a nested struct, an optional one,
/// a list, a map, a unit enum, an optional string and an unsigned integer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NestedZoneOutputs {
    pub zone_id: String,
    pub primary: NameServer,
    pub secondary: Option<NameServer>,
    pub servers: Vec<NameServer>,
    pub tags: std::collections::HashMap<String, String>,
    pub kind: ZoneKind,
    pub comment: Option<String>,
    pub records: u64,
}

/// The outputs schema the SDK writes for nested types is one a custom binding is checked
/// against: the outputs' own JSON passes, and a nested field of the wrong type fails.
#[test]
fn a_binding_of_nested_outputs_is_checked_against_the_sdk_s_schema() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<NestedZone>();
    });
    let manifest = ext.manifest().unwrap_or_else(|e| panic!("{e}"));
    let descriptor = &manifest.resource_types[&NestedZone::RESOURCE_TYPE.resource_type()];
    let server = NameServer {
        host: "ns1.example".to_string(),
        ttl: 60,
    };
    let binding = |outputs: Value| {
        let mut value = json!({ "type": "probe/zone", "service": "route53" });
        value
            .as_object_mut()
            .unwrap()
            .extend(outputs.as_object().unwrap().clone());
        match serde_json::from_value::<alien_core::ExternalBinding>(value).unwrap() {
            alien_core::ExternalBinding::Custom(custom) => custom,
            other => panic!("{other:?}"),
        }
    };
    for secondary in [None, Some(server.clone())] {
        let outputs = serde_json::to_value(NestedZoneOutputs {
            zone_id: "Z1".to_string(),
            primary: server.clone(),
            secondary,
            servers: vec![server.clone()],
            tags: [("team".to_string(), "dns".to_string())].into(),
            kind: ZoneKind::Private,
            comment: None,
            records: 3,
        })
        .unwrap();
        binding(outputs.clone())
            .validate(descriptor)
            .unwrap_or_else(|e| {
                panic!(
                    "{outputs}: {e}\nschema: {}",
                    json!(descriptor.schemas.outputs)
                )
            });

        let mut broken = outputs.clone();
        broken["primary"]["ttl"] = json!("sixty");
        broken["kind"] = json!("hidden");
        let error = binding(broken).validate(descriptor).unwrap_err();
        assert!(
            error.contains("'primary.ttl' is string") && error.contains("'kind' is \"hidden\""),
            "{error}"
        );
    }
}

#[alien_resource(type = "probe/flat-zone", outputs = FlatZoneOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlatZone {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ZoneLocation {
    pub region: String,
    pub account_id: String,
}

/// Outputs that flatten a shared struct, which utoipa writes as an `allOf`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlatZoneOutputs {
    pub zone_id: String,
    #[serde(flatten)]
    pub location: ZoneLocation,
}

/// A binding of outputs that flatten a struct passes with the outputs' own JSON, and a field
/// neither part names is still refused.
#[test]
fn a_binding_of_flattened_outputs_is_checked_against_the_sdk_s_schema() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<FlatZone>();
    });
    let manifest = ext.manifest().unwrap_or_else(|e| panic!("{e}"));
    let descriptor = &manifest.resource_types[&FlatZone::RESOURCE_TYPE.resource_type()];
    let binding = |outputs: Value| {
        let mut value = json!({ "type": "probe/flat-zone", "service": "route53" });
        value
            .as_object_mut()
            .unwrap()
            .extend(outputs.as_object().unwrap().clone());
        match serde_json::from_value::<alien_core::ExternalBinding>(value).unwrap() {
            alien_core::ExternalBinding::Custom(custom) => custom,
            other => panic!("{other:?}"),
        }
    };
    let outputs = serde_json::to_value(FlatZoneOutputs {
        zone_id: "Z1".to_string(),
        location: ZoneLocation {
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
        },
    })
    .unwrap();
    let schema = json!(descriptor.schemas.outputs);
    assert!(schema.get("allOf").is_some(), "{schema}");
    binding(outputs.clone())
        .validate(descriptor)
        .unwrap_or_else(|e| panic!("{outputs}: {e}\nschema: {schema}"));

    let mut extra = outputs;
    extra["zoneID"] = json!("Z1");
    let error = binding(extra).validate(descriptor).unwrap_err();
    assert!(
        error.contains("'zoneID' is not one of the outputs its schema names"),
        "{error}"
    );
}

#[alien_resource(type = "probe/optional-zone", outputs = OptionalLocationOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionalLocationZone {
    pub id: String,
}

/// Outputs that flatten an `Option`, whose fields a binding carries only when it is `Some`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionalLocationOutputs {
    pub zone_id: String,
    #[serde(flatten)]
    pub location: Option<ZoneLocation>,
}

#[test]
fn outputs_that_flatten_an_option_fail_the_manifest() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<OptionalLocationZone>();
    });
    let error = ext.manifest().unwrap_err().message;
    assert!(
        error.contains("`probe/optional-zone`'s outputs flatten an Option or an enum, whose fields depend on the value, which Alien can't check a binding against; flatten a struct and make its fields optional instead"),
        "{error}"
    );
}

#[alien_resource(type = "probe/mid-zone", outputs = MidZoneOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MidZone {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MidOptionalLocation {
    pub region: String,
    #[serde(flatten)]
    pub location: Option<ZoneLocation>,
}

/// Outputs whose plain field has a type that flattens an `Option`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MidZoneOutputs {
    pub zone_id: String,
    pub mid: MidOptionalLocation,
}

#[test]
fn a_field_whose_type_flattens_an_option_fails_the_manifest() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<MidZone>();
    });
    let error = ext.manifest().unwrap_err().message;
    assert!(
        error.contains("`probe/mid-zone`'s outputs flatten an Option or an enum"),
        "{error}"
    );
}

#[alien_resource(type = "probe/fine-zone", outputs = FineZoneOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FineZone {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ZoneRecord {
    Address { ip: String },
    Alias { target: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NestedLocation {
    pub location: ZoneLocation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MidPlainLocation {
    pub name: String,
    #[serde(flatten)]
    pub location: ZoneLocation,
}

/// Outputs with the shapes a binding can be checked against: optional fields, an internally
/// tagged enum field, a map, and flattened structs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FineZoneOutputs {
    pub comment: Option<String>,
    pub secondary: Option<NameServer>,
    pub record: ZoneRecord,
    pub tags: std::collections::HashMap<String, String>,
    #[serde(flatten)]
    pub location: ZoneLocation,
    #[serde(flatten)]
    pub nested: NestedLocation,
    pub mid: Option<MidPlainLocation>,
}

#[test]
fn outputs_that_flatten_only_structs_pass_the_manifest() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<FineZone>();
    });
    ext.manifest().unwrap_or_else(|error| panic!("{error}"));
}

/// The custom binding of `resource_type` that carries `outputs`.
fn custom_binding(resource_type: &str, outputs: Value) -> alien_core::CustomExternalBinding {
    let mut value = json!({ "type": resource_type, "service": "route53" });
    value
        .as_object_mut()
        .unwrap()
        .extend(outputs.as_object().unwrap().clone());
    match serde_json::from_value::<alien_core::ExternalBinding>(value).unwrap() {
        alien_core::ExternalBinding::Custom(custom) => custom,
        other => panic!("{other:?}"),
    }
}

#[alien_resource(type = "probe/map-zone", outputs = MapBesideStructOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MapZone {
    pub id: String,
}

/// Outputs that flatten a struct and a map side by side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MapBesideStructOutputs {
    pub zone_id: String,
    #[serde(flatten)]
    pub location: ZoneLocation,
    #[serde(flatten)]
    pub labels: std::collections::HashMap<String, String>,
}

#[alien_resource(type = "probe/mid-map-zone", outputs = MidMapZoneOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MidMapZone {
    pub id: String,
}

/// Outputs whose field has a type that flattens a struct and a map side by side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MidMapZoneOutputs {
    pub zone_id: String,
    pub mid: MapBesideStructOutputs,
}

#[alien_resource(type = "probe/nested-map-zone", outputs = MapBesideNestedStructOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NestedMapZone {
    pub id: String,
}

/// Outputs that flatten a map beside a struct that itself flattens a struct, whose schema names
/// its fields only through its own `allOf`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MapBesideNestedStructOutputs {
    pub zone_id: String,
    #[serde(flatten)]
    pub mid: MidPlainLocation,
    #[serde(flatten)]
    pub labels: std::collections::HashMap<String, String>,
}

#[test]
fn outputs_that_flatten_a_map_beside_a_struct_fail_the_manifest() {
    for (resource_type, extension) in [
        (
            "probe/map-zone",
            Extension::new("probe", |ext| {
                ext.resource::<MapZone>();
            }),
        ),
        (
            "probe/mid-map-zone",
            Extension::new("probe", |ext| {
                ext.resource::<MidMapZone>();
            }),
        ),
        (
            "probe/nested-map-zone",
            Extension::new("probe", |ext| {
                ext.resource::<NestedMapZone>();
            }),
        ),
    ] {
        let error = extension.manifest().unwrap_err().message;
        assert!(
            error.contains(&format!(
                "`{resource_type}`'s outputs flatten a map beside a struct's fields, which Alien can't check a binding against; put the map under a named field instead of flattening it"
            )),
            "{error}"
        );
    }
}

#[alien_resource(type = "probe/labels-zone", outputs = FlatLabelsOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelsZone {
    pub id: String,
}

/// Outputs that flatten a map and no struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlatLabelsOutputs {
    pub zone_id: String,
    #[serde(flatten)]
    pub labels: std::collections::HashMap<String, String>,
}

#[alien_resource(type = "probe/tags-zone", outputs = TagsOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagsZone {
    pub id: String,
}

/// Outputs with a map under a named field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagsOutputs {
    pub zone_id: String,
    pub tags: std::collections::HashMap<String, String>,
}

/// A map flattened on its own, or under a named field, passes the manifest, and a binding with
/// entries in it is checked against the map's value type.
#[test]
fn a_binding_of_a_map_alone_or_under_a_field_passes() {
    let labels = Extension::new("probe", |ext| {
        ext.resource::<LabelsZone>();
    });
    let labels = labels.manifest().unwrap_or_else(|e| panic!("{e}"));
    let labels = &labels.resource_types[&LabelsZone::RESOURCE_TYPE.resource_type()];
    let outputs = serde_json::to_value(FlatLabelsOutputs {
        zone_id: "Z1".to_string(),
        labels: [("team".to_string(), "dns".to_string())].into(),
    })
    .unwrap();
    custom_binding("probe/labels-zone", outputs)
        .validate(labels)
        .unwrap_or_else(|e| panic!("{e}"));
    let error = custom_binding("probe/labels-zone", json!({ "zoneId": "Z1", "team": 7 }))
        .validate(labels)
        .unwrap_err();
    assert!(error.contains("'team' is number"), "{error}");

    let tags = Extension::new("probe", |ext| {
        ext.resource::<TagsZone>();
    });
    let tags = tags.manifest().unwrap_or_else(|e| panic!("{e}"));
    let tags = &tags.resource_types[&TagsZone::RESOURCE_TYPE.resource_type()];
    let outputs = serde_json::to_value(TagsOutputs {
        zone_id: "Z1".to_string(),
        tags: [("team".to_string(), "dns".to_string())].into(),
    })
    .unwrap();
    custom_binding("probe/tags-zone", outputs)
        .validate(tags)
        .unwrap_or_else(|e| panic!("{e}"));
}

#[alien_resource(type = "probe/default-zone", outputs = DefaultFieldOutputs, lifecycles = [Frozen], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefaultZone {
    pub id: String,
}

/// Outputs whose field named like a schema keyword has an inline type that flattens an `Option`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefaultFieldOutputs {
    pub zone_id: String,
    #[schema(inline)]
    pub default: MidOptionalLocation,
}

#[test]
fn a_field_named_default_is_still_checked_for_a_flattened_option() {
    let ext = Extension::new("probe", |ext| {
        ext.resource::<DefaultZone>();
    });
    let error = ext.manifest().unwrap_err().message;
    assert!(
        error.contains("`probe/default-zone`'s outputs flatten an Option or an enum"),
        "{error}"
    );
}
