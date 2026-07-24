//! AWS OpenSearch Serverless scenarios.

use super::helpers::render_built_ins;
use alien_cloudformation::{CfRegistry, RegistrationMode};
use alien_core::{
    import::EmitContext, AwsOpenSearch, AwsOpenSearchCapacity, AwsOpenSearchCapacityRange,
    AwsOpenSearchCollectionType, AwsOpenSearchImportData, PermissionProfile, Platform,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings,
};
use indexmap::IndexMap;

#[test]
fn aws_open_search_renders_collection_with_data_access() {
    let stack = Stack::new("search-stack".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource("articles", ["experimental/aws-opensearch/data-access"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AwsOpenSearch::new("articles".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws opensearch serverless",
    );

    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let group = &template["Resources"]["ArticlesGroup"];
    assert_eq!(group["Type"], "AWS::OpenSearchServerless::CollectionGroup");
    assert_eq!(group["Properties"]["Generation"], "NEXTGEN");
    assert_eq!(group["Properties"]["StandbyReplicas"], "ENABLED");
    assert!(
        group["Properties"].get("CapacityLimits").is_none(),
        "capacity must remain omitted by default"
    );

    let network = &template["Resources"]["ArticlesNetworkPolicy"];
    let network_policy = network["Properties"]["Policy"]["Fn::Sub"][0]
        .as_str()
        .expect("network policy should be a Sub template string");
    let network_document: serde_json::Value =
        serde_json::from_str(network_policy).expect("network policy should be valid JSON");
    assert!(
        network_document.is_array(),
        "AOSS network policies require a top-level JSON array"
    );

    let collection = &template["Resources"]["Articles"];
    assert_eq!(collection["Type"], "AWS::OpenSearchServerless::Collection");
    assert_eq!(collection["Properties"]["Type"], "SEARCH");
    assert_eq!(
        collection["Properties"]["CollectionGroupName"],
        group["Properties"]["Name"]
    );
    assert_eq!(
        collection["Properties"]["EncryptionConfig"]["AWSOwnedKey"],
        true
    );
    assert_eq!(
        collection["DependsOn"],
        serde_json::json!(["ArticlesGroup"])
    );
    // Data access wiring: the SA role is a principal of the data-access
    // policy and gets aoss:APIAccessAll pinned to this collection's ARN.
    let access = &template["Resources"]["ArticlesDataAccessPolicy"];
    assert_eq!(access["Type"], "AWS::OpenSearchServerless::AccessPolicy");
    let access_policy = access["Properties"]["Policy"]["Fn::Sub"][0]
        .as_str()
        .expect("data access policy should be a Sub template string");
    assert!(access_policy.contains("aoss:ReadDocument"));
    assert!(access_policy.contains("${Principal0}"));

    insta::assert_snapshot!("aws_open_search", yaml);
}

#[test]
fn aws_open_search_renders_collection_group_capacity_limits() {
    let stack = Stack::new("search-stack".to_string())
        .add(
            AwsOpenSearch::new("articles".to_string())
                .capacity(AwsOpenSearchCapacity {
                    indexing: Some(AwsOpenSearchCapacityRange {
                        min_ocu: Some(2),
                        max_ocu: Some(16),
                    }),
                    search: Some(AwsOpenSearchCapacityRange {
                        min_ocu: Some(2),
                        max_ocu: Some(32),
                    }),
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws opensearch serverless capacity",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let limits = &template["Resources"]["ArticlesGroup"]["Properties"]["CapacityLimits"];
    assert_eq!(limits["MinIndexingCapacityInOcu"], 2);
    assert_eq!(limits["MaxIndexingCapacityInOcu"], 16);
    assert_eq!(limits["MinSearchCapacityInOcu"], 2);
    assert_eq!(limits["MaxSearchCapacityInOcu"], 32);
}

#[test]
fn aws_open_search_rejects_invalid_capacity_before_rendering() {
    let stack = Stack::new("search-stack".to_string())
        .add(
            AwsOpenSearch::new("articles".to_string())
                .capacity(AwsOpenSearchCapacity {
                    indexing: None,
                    search: Some(AwsOpenSearchCapacityRange {
                        min_ocu: Some(3),
                        max_ocu: None,
                    }),
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let error = alien_cloudformation::generate_cloudformation_template(
        &stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &CfRegistry::built_in(),
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
        },
    )
    .expect_err("unsupported OCU values must fail generation");

    assert!(error.to_string().contains("minOcu"));
    assert!(error.to_string().contains("unsupported"));
}

#[test]
fn aws_open_search_vector_collection_sets_vectorsearch_type() {
    let stack = Stack::new("vector-stack".to_string())
        .add(
            AwsOpenSearch::new("embeddings".to_string())
                .collection_type(AwsOpenSearchCollectionType::VectorSearch)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws opensearch vector collection",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    assert_eq!(
        template["Resources"]["Embeddings"]["Properties"]["Type"],
        "VECTORSEARCH"
    );
    // No data-access grants -> no access policy and no IAM policies.
    assert!(template["Resources"]
        .get("EmbeddingsDataAccessPolicy")
        .is_none());
}

#[test]
fn aws_open_search_rejects_id_unusable_as_collection_name() {
    for bad_id in [
        "Search",
        "s",
        "with_underscore",
        "this-id-is-way-too-long-for-aoss",
    ] {
        let stack = Stack::new("bad-id".to_string())
            .add(
                AwsOpenSearch::new(bad_id.to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let result = alien_cloudformation::generate_cloudformation_template(
            &stack,
            alien_cloudformation::CloudFormationOptions {
                registry: &CfRegistry::built_in(),
                target: alien_cloudformation::CloudFormationTarget::Aws,
                stack_settings: StackSettings::default(),
                setup_target: "aws".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                registration: RegistrationMode::OutputsFallback,
                description: None,
            },
        );

        let err = result.expect_err(&format!("id '{bad_id}' must be rejected"));
        assert!(
            err.to_string().contains("is invalid"),
            "unexpected error for id '{bad_id}': {err}"
        );
    }
}

#[test]
fn aws_open_search_has_no_emitter_on_other_platforms() {
    // Experimental resources are provider-specific: only an AWS emitter is
    // registered, so any other platform must fail with the typed
    // ImportRegistrationMissing error rather than silently skipping.
    let registry = CfRegistry::built_in();
    for platform in [Platform::Gcp, Platform::Azure, Platform::Kubernetes] {
        let err = registry
            .require(&AwsOpenSearch::RESOURCE_TYPE, platform)
            .err()
            .unwrap_or_else(|| panic!("{platform:?} must have no AwsOpenSearch emitter"));
        assert_eq!(err.code, "IMPORT_REGISTRATION_MISSING");
    }
}

/// `emit_import_ref` and `AwsOpenSearchImportData` are two halves of one
/// contract: the manager-side importer deserializes exactly what the deployed
/// setup stack resolves this expression to. Resolving every intrinsic to a
/// placeholder string and parsing the result catches key or shape drift
/// between the emitter and the import struct at test time.
#[test]
fn aws_open_search_import_ref_matches_import_data_contract() {
    let stack = Stack::new("search-stack".to_string())
        .add(
            AwsOpenSearch::new("articles".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (_, entry) = stack
        .resources()
        .find(|(id, _)| id.as_str() == "articles")
        .expect("articles resource");
    let names: IndexMap<String, String> =
        IndexMap::from([("articles".to_string(), "Articles".to_string())]);
    let ctx = EmitContext {
        stack: &stack,
        resource: entry,
        resource_id: "articles",
        platform: Platform::Aws,
        stack_settings: &StackSettings::default(),
        names: &names,
    };

    let registry = CfRegistry::built_in();
    let emitter = registry
        .require(&AwsOpenSearch::RESOURCE_TYPE, Platform::Aws)
        .expect("aws-opensearch emitter should be registered");
    let import_ref = emitter
        .emit_import_ref(&ctx)
        .expect("import ref should render");
    let import_json = serde_json::to_value(&import_ref).expect("import ref should serialize");

    let resolved = resolve_cfn_intrinsics(import_json);
    let data: AwsOpenSearchImportData = serde_json::from_value(resolved)
        .expect("resolved import ref must deserialize into AwsOpenSearchImportData");
    assert!(!data.collection_name.is_empty());
    assert!(!data.collection_id.is_empty());
    assert!(!data.collection_arn.is_empty());
    assert!(!data.endpoint.is_empty());
}

/// Replace every CloudFormation intrinsic (`Ref` / `Fn::*`) with a
/// placeholder string — the shape the payload has after the deployed stack
/// resolves it.
fn resolve_cfn_intrinsics(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let is_intrinsic = map.len() == 1
                && map
                    .keys()
                    .next()
                    .is_some_and(|key| key == "Ref" || key.starts_with("Fn::"));
            if is_intrinsic {
                serde_json::Value::String("resolved".to_string())
            } else {
                serde_json::Value::Object(
                    map.into_iter()
                        .map(|(key, value)| (key, resolve_cfn_intrinsics(value)))
                        .collect(),
                )
            }
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(resolve_cfn_intrinsics).collect())
        }
        other => other,
    }
}
