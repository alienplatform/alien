use std::sync::Arc;

use alien_manager::{
    AlienManager,
    config::ManagerConfig,
    standalone_config::ManagerTomlConfig,
    stores::sqlite::{SqliteDatabase, SqliteTokenStore},
    traits::{CreateTokenParams, TokenStore, TokenType},
};
use httpmock::prelude::*;
use reqwest::{StatusCode, header};
use serde_json::json;
use sha2::{Digest, Sha256};

const TOKEN: &str = "ax_admin_test_manager_info";

fn sdk(url: &str) -> alien_manager_api::Client {
    let headers = header::HeaderMap::from_iter([(
        header::AUTHORIZATION,
        header::HeaderValue::from_static("Bearer ax_admin_test_manager_info"),
    )]);
    alien_manager_api::Client::new_with_client(
        url,
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap(),
    )
}

#[tokio::test]
async fn authenticated_manager_advertises_node_identity_support_only_when_opted_in() {
    for opt_in in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let db_path = directory.path().join("manager.db");
        let db = Arc::new(
            SqliteDatabase::new(&db_path.to_string_lossy())
                .await
                .unwrap(),
        );
        let tokens = Arc::new(SqliteTokenStore::new(db));
        tokens
            .create_token(CreateTokenParams {
                token_type: TokenType::Admin,
                key_prefix: TOKEN[..12].into(),
                key_hash: format!("{:x}", Sha256::digest(TOKEN.as_bytes())),
                deployment_group_id: None,
                deployment_id: None,
            })
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mut config = ManagerConfig {
            db_path: Some(db_path),
            state_dir: Some(directory.path().into()),
            base_url: Some(url.clone()),
            disable_deployment_loop: true,
            disable_heartbeat_loop: true,
            ..Default::default()
        };
        if opt_in {
            config.supports_aws_setup_node_identity = true;
        }
        let manager = AlienManager::builder(config)
            .token_store(tokens)
            .with_standalone_defaults(&ManagerTomlConfig::default())
            .await
            .unwrap()
            .build()
            .await
            .unwrap();
        let task = tokio::spawn(manager.start_with_listener(listener));
        let unauthenticated = reqwest::Client::new()
            .get(format!("{url}/v1/manager"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let info = sdk(&url).manager_info().send().await.unwrap().into_inner();
        assert_eq!(info.capabilities.aws_setup_node_identity, opt_in);
        assert_eq!(info.url, url);
        assert!(!info.capabilities.charts);
        assert!(!info.capabilities.tunnels);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn generated_client_defaults_old_manager_capability_to_false_and_preserves_explicit_values() {
    for value in [None, Some(false), Some(true)] {
        let server = MockServer::start_async().await;
        let mut response = json!({
            "url": "https://manager.example.com",
            "registryHost": "manager.example.com",
            "version": "0.1.0",
            "capabilities": { "tunnels": true, "charts": false }
        });
        if let Some(value) = value {
            response["capabilities"]["awsSetupNodeIdentity"] = json!(value);
        }
        let request = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/manager")
                    .header("authorization", "Bearer ax_admin_test_manager_info");
                then.status(200).json_body(response);
            })
            .await;
        let info = sdk(&server.base_url())
            .manager_info()
            .send()
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            info.capabilities.aws_setup_node_identity,
            value.unwrap_or(false)
        );
        assert!(info.capabilities.tunnels);
        assert!(!info.capabilities.charts);
        assert_eq!(info.version, "0.1.0");
        request.assert_hits_async(1).await;
    }
}
