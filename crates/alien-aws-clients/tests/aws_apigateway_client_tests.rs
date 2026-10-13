//! HTTP contract tests for private-domain identity isolation. Real provisioning,
//! VPC endpoint association, traffic and propagation are qualified separately.
use std::collections::HashMap;

use alien_aws_clients::{
    apigateway::{
        ApiGatewayApi, ApiGatewayClient, CreateBasePathMappingRequest,
        CreateDomainNameAccessAssociationRequest, CreatePrivateDomainNameRequest,
        CreatePrivateRestApiRequest,
    },
    credential_provider::AwsCredentialProvider,
    ServiceOverrides,
};
use alien_core::{AwsClientConfig, AwsCredentials};
use httpmock::{prelude::*, Method::PATCH};
use serde_json::json;

fn client(server: &MockServer) -> ApiGatewayClient {
    ApiGatewayClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "111122223333".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret-key".to_string(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("apigateway".to_string(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn private_domain_lifecycle_keeps_same_named_public_domain_isolated() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let hostname = "api.example.com";
    let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;
    let response = json!({"domainName":hostname,"domainNameId":"private-id","domainNameArn":"arn:aws:apigateway:us-east-1:111122223333:/domainnames/api.example.com+private-id","domainNameStatus":"AVAILABLE","certificateArn":"owned-cert","tags":{"CreateAttempt":"attempt-1"}});
    let create = server
        .mock_async(|when, then| {
            when.method(POST).path("/domainnames").json_body(json!({
                "domainName":hostname,"certificateArn":"owned-cert",
                "endpointConfiguration":{"types":["PRIVATE"]},"securityPolicy":"TLS_1_2",
                "policy":policy,"tags":{"CreateAttempt":"attempt-1"}
            }));
            then.status(201).json_body(response.clone());
        })
        .await;
    let public = server
        .mock_async(|when, then| {
            when.path("/domainnames/api.example.com")
                .matches(|request| {
                    request
                        .query_params
                        .as_ref()
                        .is_none_or(|params| !params.iter().any(|(key, _)| key == "domainNameId"))
                });
            then.status(500).body("public domain must remain untouched");
        })
        .await;
    let get = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/domainnames/api.example.com")
                .query_param("domainNameId", "private-id");
            then.status(200).json_body(response.clone());
        })
        .await;
    let patch = server.mock_async(|when, then| {
        when.method(PATCH).path("/domainnames/api.example.com").query_param("domainNameId","private-id")
            .json_body(json!({"patchOperations":[{"op":"replace","path":"/certificateArn","value":"renewed-cert"}]}));
        then.status(200).json_body(response.clone());
    }).await;
    let mapping = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/domainnames/api.example.com/basepathmappings")
                .query_param("domainNameId", "private-id")
                .json_body(json!({"restApiId":"private-api","stage":"live"}));
            then.status(201)
                .json_body(json!({"restApiId":"private-api","stage":"live","basePath":"(none)"}));
        })
        .await;
    let unmap = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/domainnames/api.example.com/basepathmappings/%28none%29")
                .query_param("domainNameId", "private-id");
            then.status(202);
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/domainnames/api.example.com")
                .query_param("domainNameId", "private-id");
            then.status(202);
        })
        .await;
    let domain = api
        .create_private_domain_name(CreatePrivateDomainNameRequest {
            domain_name: hostname.to_string(),
            certificate_arn: "owned-cert".to_string(),
            policy: policy.to_string(),
            tags: HashMap::from([("CreateAttempt".to_string(), "attempt-1".to_string())]),
        })
        .await
        .unwrap();
    let id = domain.domain_name_id.unwrap();
    assert_eq!(id, "private-id");
    assert_eq!(
        api.get_private_domain_name(hostname, &id)
            .await
            .unwrap()
            .domain_name_status
            .as_deref(),
        Some("AVAILABLE")
    );
    api.update_private_domain_certificate(hostname, &id, "renewed-cert")
        .await
        .unwrap();
    let mapped = api
        .create_private_base_path_mapping(
            hostname,
            &id,
            CreateBasePathMappingRequest {
                rest_api_id: "private-api".to_string(),
                stage: "live".to_string(),
                base_path: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(mapped.rest_api_id.as_deref(), Some("private-api"));
    api.delete_private_base_path_mapping(hostname, &id, "(none)")
        .await
        .unwrap();
    api.delete_private_domain_name(hostname, &id).await.unwrap();
    for request in [&create, &get, &patch, &mapping, &unmap, &delete] {
        request.assert_hits(1);
    }
    public.assert_hits(0);
}

#[tokio::test]
async fn missing_private_domain_id_fails_before_any_http_request() {
    let server = MockServer::start_async().await;
    let unexpected = server
        .mock_async(|_, then| {
            then.status(500);
        })
        .await;
    let api = client(&server);
    for id in ["", "  "] {
        assert!(api
            .get_private_domain_name("api.example.com", id)
            .await
            .is_err());
        assert!(api
            .delete_private_domain_name("api.example.com", id)
            .await
            .is_err());
        assert!(api
            .update_private_domain_certificate("api.example.com", id, "cert")
            .await
            .is_err());
        assert!(api
            .create_private_base_path_mapping(
                "api.example.com",
                id,
                CreateBasePathMappingRequest {
                    rest_api_id: "api".to_string(),
                    stage: "live".to_string(),
                    base_path: None,
                }
            )
            .await
            .is_err());
        assert!(api
            .delete_private_base_path_mapping("api.example.com", id, "(none)")
            .await
            .is_err());
    }
    unexpected.assert_hits(0);
}

#[tokio::test]
async fn access_associations_preserve_ownership_tags_and_pagination_cursor() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let domain_arn =
        "arn:aws:apigateway:us-east-1:111122223333:/domainnames/api.example.com+private-id";
    let association_arn = "arn:aws:apigateway:us-east-1:111122223333:/domainnameaccessassociations/api.example.com+private-id/association-id";
    let item = json!({"domainNameArn":domain_arn,"domainNameAccessAssociationArn":association_arn,
        "accessAssociationSource":"vpce-owned","accessAssociationSourceType":"VPCE","tags":{"CreateAttempt":"attempt-1"}});
    let create = server.mock_async(|when,then| {
        when.method(POST).path("/domainnameaccessassociations").json_body(json!({
            "domainNameArn":domain_arn,"accessAssociationSource":"vpce-owned","accessAssociationSourceType":"VPCE","tags":{"CreateAttempt":"attempt-1"}
        }));
        then.status(201).json_body(item.clone());
    }).await;
    let cursor = "opaque+/cursor=";
    let first = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/domainnameaccessassociations")
                .query_param("limit", "500")
                .query_param("resourceOwner", "SELF")
                .matches(|request| {
                    request
                        .query_params
                        .as_ref()
                        .is_none_or(|params| !params.iter().any(|(key, _)| key == "position"))
                });
            then.status(200)
                .json_body(json!({"item":[],"position":cursor}));
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/domainnameaccessassociations")
                .query_param("limit", "500")
                .query_param("resourceOwner", "SELF")
                .query_param("position", cursor);
            then.status(200).json_body(json!({"item":[item]}));
        })
        .await;
    let encoded = format!(
        "/domainnameaccessassociations/{}",
        urlencoding::encode(association_arn)
    );
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE).path(encoded);
            then.status(202);
        })
        .await;
    let created = api
        .create_domain_name_access_association(CreateDomainNameAccessAssociationRequest {
            domain_name_arn: domain_arn.to_string(),
            vpc_endpoint_id: "vpce-owned".to_string(),
            tags: HashMap::from([("CreateAttempt".to_string(), "attempt-1".to_string())]),
        })
        .await
        .unwrap();
    assert_eq!(
        created.domain_name_access_association_arn.as_deref(),
        Some(association_arn)
    );
    let page = api.get_domain_name_access_associations(None).await.unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.position.as_deref(), Some(cursor));
    let page = api
        .get_domain_name_access_associations(page.position)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.position.is_none());
    assert_eq!(
        page.items[0]
            .tags
            .as_ref()
            .unwrap()
            .get("CreateAttempt")
            .map(String::as_str),
        Some("attempt-1")
    );
    api.delete_domain_name_access_association(association_arn)
        .await
        .unwrap();
    for request in [&create, &first, &second, &delete] {
        request.assert_hits(1);
    }
}

#[tokio::test]
async fn private_domain_dependency_drain_returns_to_controller_without_hidden_retries() {
    let server = MockServer::start_async().await;
    let request = server.mock_async(|when,then| {
        when.method(DELETE).path("/domainnames/api.example.com").query_param("domainNameId","private-id");
        then.status(409).json_body(json!({"__type":"ConflictException","message":"Domain name access association is still attached"}));
    }).await;
    let error = client(&server)
        .delete_private_domain_name("api.example.com", "private-id")
        .await
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
    ));
    request.assert_hits(1);
}

#[tokio::test]
async fn private_rest_api_applies_vpc_policy_before_exposing_any_stage() {
    let server = MockServer::start_async().await;
    let policy = json!({"Version":"2012-10-17","Statement":[{
        "Effect":"Allow","Principal":"*","Action":"execute-api:Invoke","Resource":"execute-api:/*",
        "Condition":{"StringEquals":{"aws:SourceVpce":"vpce-owned"}}
    }]})
    .to_string();
    let create = server.mock_async(|when,then| {
        when.method(POST).path("/restapis").json_body(json!({
            "name":"private-worker","endpointConfiguration":{"types":["PRIVATE"],"vpcEndpointIds":["vpce-owned"]},
            "policy":policy,"binaryMediaTypes":["*/*"],"disableExecuteApiEndpoint":true,"tags":{"CreateAttempt":"attempt-1"}
        }));
        then.status(201).json_body(json!({"id":"private-api","rootResourceId":"root-owned","name":"private-worker"}));
    }).await;
    let created = client(&server)
        .create_private_rest_api(CreatePrivateRestApiRequest {
            name: "private-worker".to_string(),
            binary_media_types: Some(vec!["*/*".into()]),
            vpc_endpoint_ids: vec!["vpce-owned".to_string()],
            policy,
            disable_execute_api_endpoint: true,
            tags: HashMap::from([("CreateAttempt".to_string(), "attempt-1".to_string())]),
        })
        .await
        .unwrap();
    assert_eq!(created.id.as_deref(), Some("private-api"));
    assert_eq!(created.root_resource_id.as_deref(), Some("root-owned"));
    create.assert_hits(1);
}

#[tokio::test]
async fn ambiguous_private_api_creation_is_not_blindly_resent_by_sdk() {
    let server = MockServer::start_async().await;
    let committed = server
        .mock_async(|when, then| {
            when.method(POST).path("/restapis");
            // Model a committed create whose response is lost at the provider edge.
            then.status(500)
                .json_body(json!({"message":"response unavailable"}));
        })
        .await;
    client(&server)
        .create_private_rest_api(CreatePrivateRestApiRequest {
            name: "private-worker".to_string(),
            binary_media_types: Some(vec!["*/*".into()]),
            vpc_endpoint_ids: vec!["vpce-owned".to_string()],
            policy: "{}".to_string(),
            disable_execute_api_endpoint: true,
            tags: HashMap::from([("CreateAttempt".to_string(), "attempt-1".to_string())]),
        })
        .await
        .unwrap_err();
    committed.assert_hits(1);
}

#[tokio::test]
async fn discovery_retains_provider_ids_tags_configuration_and_opaque_cursors() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let cursor = "opaque+/cursor=";
    let rest = json!({"id":"owned-api","name":"private-worker","rootResourceId":"root-owned",
        "tags":{"CreateAttempt":"attempt-1"},"endpointConfiguration":{"types":["PRIVATE"],"vpcEndpointIds":["vpce-owned"]},
        "policy":"{\"Statement\":[]}","disableExecuteApiEndpoint":true});
    let list_apis = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/restapis")
                .header("accept", "application/json")
                .query_param("limit", "500")
                .query_param("position", cursor);
            then.status(200)
                .json_body(json!({"item":[rest],"position":cursor}));
        })
        .await;
    let get_api = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/restapis/owned-api")
                .header("accept", "application/json");
            then.status(200).json_body(rest.clone());
        })
        .await;
    let domains = server.mock_async(|when,then| {
        when.method(GET).path("/domainnames").header("accept","application/json").query_param("limit","500").query_param("resourceOwner","SELF").query_param("position",cursor);
        then.status(200).json_body(json!({"item":[
            {"domainName":"api.example.com","tags":{"CreateAttempt":"foreign-attempt"}},
            {"domainName":"api.example.com","domainNameId":"private-id","domainNameArn":"owned-private-domain-arn","tags":{"CreateAttempt":"attempt-1"}}
        ],"position":cursor}));
    }).await;
    let page = api.get_rest_apis(Some(cursor.to_string())).await.unwrap();
    assert_eq!(page.position.as_deref(), Some(cursor));
    assert_eq!(page.items.len(), 1);
    let details = api
        .get_rest_api_details(page.items[0].id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&details).unwrap().get("tags"),
        Some(&json!({"CreateAttempt":"attempt-1"}))
    );
    assert_eq!(details.disable_execute_api_endpoint, Some(true));
    let endpoint = details.endpoint_configuration.unwrap();
    assert_eq!(endpoint.types, ["PRIVATE"]);
    assert_eq!(endpoint.vpc_endpoint_ids, ["vpce-owned"]);
    let page = api
        .get_domain_names(Some(cursor.to_string()))
        .await
        .unwrap();
    assert_eq!(page.position.as_deref(), Some(cursor));
    assert_eq!(page.items.len(), 2);
    assert!(page.items[0].domain_name_id.is_none());
    assert_eq!(page.items[1].domain_name_id.as_deref(), Some("private-id"));
    assert_eq!(
        page.items[1]
            .tags
            .as_ref()
            .unwrap()
            .get("CreateAttempt")
            .map(String::as_str),
        Some("attempt-1")
    );
    for request in [&list_apis, &get_api, &domains] {
        request.assert_hits(1);
    }
}

#[tokio::test]
async fn unexpected_hypermedia_collections_fail_instead_of_hiding_owned_children() {
    let server = MockServer::start_async().await;
    let unexpected = server
        .mock_async(|when, then| {
            when.method(GET).header("accept", "application/json");
            then.status(200)
                .json_body(json!({"_embedded":{"item":[{"id":"owned-api"}]},"_links":{}}));
        })
        .await;
    let api = client(&server);
    assert!(api.get_rest_apis(None).await.is_err());
    assert!(api.get_domain_names(None).await.is_err());
    assert!(api.get_domain_name_access_associations(None).await.is_err());
    unexpected.assert_hits(3);
}

#[tokio::test]
async fn lost_response_recovery_reads_resource_paths_method_and_selected_deployment() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let cursor = "opaque+/cursor=";
    let resources = server.mock_async(|when, then| {
        when.method(GET).path("/restapis/owned/resources")
            .header("accept", "application/json").query_param("limit", "500")
            .query_param("position", cursor);
        then.status(200).json_body(json!({"item":[{"id":"proxy","parentId":"root","pathPart":"{proxy+}","path":"/{proxy+}"}],"position":cursor}));
    }).await;
    let method = server.mock_async(|when, then| {
        when.method(GET).path("/restapis/owned/resources/proxy/methods/ANY");
        then.status(200).json_body(json!({"httpMethod":"ANY","authorizationType":"NONE","apiKeyRequired":false,
            "methodIntegration":{"type":"AWS_PROXY","httpMethod":"POST","uri":"owned-lambda-stream-uri","responseTransferMode":"STREAM","timeoutInMillis":90000}}));
    }).await;
    let stage = server
        .mock_async(|when, then| {
            when.method(GET).path("/restapis/owned/stages/live");
            then.status(200)
                .json_body(json!({"stageName":"live","deploymentId":"committed-deployment"}));
        })
        .await;
    let page = api
        .get_resources("owned", Some(cursor.into()))
        .await
        .unwrap();
    assert_eq!(page.position.as_deref(), Some(cursor));
    assert_eq!(page.items.len(), 1);
    let resource = &page.items[0];
    assert_eq!(resource.parent_id.as_deref(), Some("root"));
    assert_eq!(resource.path.as_deref(), Some("/{proxy+}"));
    assert_eq!(resource.path_part.as_deref(), Some("{proxy+}"));
    let found = api
        .get_method("owned", resource.id.as_deref().unwrap(), "ANY")
        .await
        .unwrap();
    assert_eq!(found.http_method.as_deref(), Some("ANY"));
    assert_eq!(found.authorization_type.as_deref(), Some("NONE"));
    assert_eq!(found.api_key_required, Some(false));
    let integration = found.method_integration.unwrap();
    assert_eq!(integration.integration_type, "AWS_PROXY");
    assert_eq!(integration.integration_http_method, "POST");
    assert_eq!(integration.uri, "owned-lambda-stream-uri");
    assert_eq!(
        integration.response_transfer_mode.as_deref(),
        Some("STREAM")
    );
    assert_eq!(integration.timeout_in_millis, Some(90000));
    let found = api.get_stage("owned", "live").await.unwrap();
    assert_eq!(found.stage_name.as_deref(), Some("live"));
    assert_eq!(found.deployment_id.as_deref(), Some("committed-deployment"));
    for request in [&resources, &method, &stage] {
        request.assert_hits_async(1).await;
    }
}

#[tokio::test]
async fn policy_update_uses_exact_policy_string_and_keeps_provider_denial_distinct_from_absence() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let policy = json!({"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"execute-api:Invoke","Resource":"*","Condition":{"StringEquals":{"aws:SourceVpce":"vpce-owned"}}}]}).to_string();
    let patch = server
        .mock_async(|when, then| {
            when.method(PATCH).path("/restapis/owned").json_body(
                json!({"patchOperations":[{"op":"replace","path":"/policy","value":policy}]}),
            );
            then.status(200)
                .json_body(json!({"id":"owned","policy":policy}));
        })
        .await;
    let denied = server
        .mock_async(|when, then| {
            when.method(GET).path("/restapis/owned/stages/denied");
            then.status(403)
                .json_body(json!({"__type":"AccessDeniedException","message":"denied"}));
        })
        .await;
    let absent = server
        .mock_async(|when, then| {
            when.method(GET).path("/restapis/owned/stages/absent");
            then.status(404)
                .json_body(json!({"__type":"NotFoundException","message":"missing"}));
        })
        .await;
    let updated = api.update_rest_api_policy("owned", &policy).await.unwrap();
    assert_eq!(updated.id.as_deref(), Some("owned"));
    assert_eq!(updated.policy.as_deref(), Some(policy.as_str()));
    let denial = api.get_stage("owned", "denied").await.unwrap_err();
    let absence = api.get_stage("owned", "absent").await.unwrap_err();
    assert_ne!(denial.code, absence.code);
    assert_eq!(absence.code, "REMOTE_RESOURCE_NOT_FOUND");
    for request in [&patch, &denied, &absent] {
        request.assert_hits_async(1).await;
    }
}
