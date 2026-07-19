//! AWS OpenSearch Serverless (next generation) scenarios.

use super::helpers::render_built_ins;
use alien_cloudformation::{CfRegistry, RegistrationMode};
use alien_core::{
    AwsOpenSearch, AwsOpenSearchCollectionType, PermissionProfile, Platform, ResourceLifecycle,
    ServiceAccount, Stack, StackSettings,
};

#[test]
fn aws_open_search_renders_next_gen_collection_with_data_access() {
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

    // The template must pin the next-generation serverless path: a collection
    // group with Generation NEXTGEN and the collection joined to it.
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let group = &template["Resources"]["ArticlesGroup"];
    assert_eq!(group["Type"], "AWS::OpenSearchServerless::CollectionGroup");
    assert_eq!(group["Properties"]["Generation"], "NEXTGEN");
    let collection = &template["Resources"]["Articles"];
    assert_eq!(collection["Type"], "AWS::OpenSearchServerless::Collection");
    assert_eq!(collection["Properties"]["Type"], "SEARCH");
    assert_eq!(
        collection["Properties"]["CollectionGroupName"], group["Properties"]["Name"],
        "collection must join the emitted group"
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
