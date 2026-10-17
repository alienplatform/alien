use super::*;
use crate::core::controller_test::SingleControllerExecutor;
use crate::gcp_https_ingress::tests::{provider, Provider};
use crate::{GcpWorkerController, MockPlatformServiceProvider};
use alien_core::{
    DnsRecordStatus, DomainMetadata, NetworkSettings, Platform, StackSettings, WorkerCode,
    WorkerPublicEndpoint,
};
use alien_gcp_clients::compute::{Subnetwork, SubnetworkPurpose, SubnetworkRole};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn pending_private_graph_does_not_block_public_certificate_and_reload() {
    pending_private_graph(false).await;
}

#[tokio::test]
async fn public_certificate_failure_preserves_private_progress_and_serving_frontend() {
    pending_private_graph(true).await;
}

async fn pending_private_graph(lose_certificate_response: bool) {
    let graphs = Arc::new(Mutex::new(Provider::default()));
    let mut compute = provider(&graphs);
    let mut network = GcpNetworkController::mock_ready("default-network");
    network.network_self_link = Some(
        "https://www.googleapis.com/compute/v1/projects/test-project-123/global/networks/test-vpc"
            .into(),
    );
    network.subnetwork_self_link = Some("https://www.googleapis.com/compute/v1/projects/test-project-123/regions/us-central1/subnetworks/test-subnet".into());
    network.proxy_subnet_self_link = Some("https://www.googleapis.com/compute/v1/projects/test-project-123/regions/us-central1/subnetworks/test-proxy".into());
    network.proxy_subnet_cidr = Some("10.20.0.0/23".into());
    let network_link = network.network_self_link.clone();
    compute.expect_get_subnetwork().returning(move |_, name| {
        assert_eq!(name, "test-proxy");
        Ok(Subnetwork {
            purpose: Some(SubnetworkPurpose::RegionalManagedProxy),
            role: Some(SubnetworkRole::Active),
            network: network_link.clone(),
            ip_cidr_range: Some("10.20.0.0/23".into()),
            ..Default::default()
        })
    });
    let compute = Arc::new(compute);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_compute_client()
        .returning(move |_| Ok(compute.clone()));
    let services = Arc::new(services);
    let worker = Worker::new("worker".into())
        .code(WorkerCode::Image {
            image: "example.com/worker:v1".into(),
        })
        .permissions("default-profile".into())
        .public_endpoint(WorkerPublicEndpoint {
            name: "api".into(),
            host_label: None,
            wildcard_subdomains: false,
        })
        .build();
    let mut metadata: DomainMetadata = serde_json::from_value(serde_json::json!({
        "baseDomain":"test.example.com", "publicSubdomain":"test", "hostedZoneId":"test-zone",
        "resources": {"worker": {"fqdn":"worker.test.example.com", "certificateId":"cert",
          "certificateStatus":"issued", "dnsStatus":"active", "certificateChain":"old-certificate", "privateKey":"test-key",
          "issuedAt":"2024-01-01T00:00:00Z", "endpoints":{}, "aliases":[], "privateEndpoints":{
            "api":{"fqdn":"private.worker.test.example.com", "certificateId":"cert", "certificateStatus":"issued",
              "dnsStatus":"pending", "certificateChain":"expanded-certificate", "privateKey":"test-key", "issuedAt":"2024-01-01T00:00:00Z"}
          }}}
    })).unwrap();
    let mut ingress = GcpWorkerIngress::default();
    let mut retained: Option<Vec<(String, serde_json::Value)>> = None;
    let mut failures = 0;
    for access in [EndpointAccess::Internet, EndpointAccess::Both] {
        if access == EndpointAccess::Both {
            metadata
                .resources
                .get_mut("worker")
                .unwrap()
                .certificate_chain = Some("expanded-certificate".into());
            graphs.lock().unwrap().pending_regional_operations = true;
            graphs.lock().unwrap().lose_public_certificate_response = lose_certificate_response;
        }
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker.clone())
            .controller(GcpWorkerController::default())
            .platform(Platform::Gcp)
            .service_provider(services.clone())
            .with_dependency(
                Network::new("default-network".into())
                    .settings(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
                network.clone(),
            )
            .stack_settings(StackSettings {
                endpoint_access: access,
                ..Default::default()
            })
            .domain_metadata(metadata.clone())
            .build()
            .await
            .unwrap();
        let mut ready = false;
        for _ in 0..30 {
            let result = executor.with_context(|ctx| {
                futures::executor::block_on(ingress.reconcile(ctx, &worker, "neg", true))
            });
            match result {
                Ok(value) => ready = value,
                Err(error) => {
                    assert!(lose_certificate_response, "unexpected error: {error}");
                    failures += 1;
                    assert_eq!(failures, 1);
                    let saved = serde_json::to_value(&ingress).unwrap();
                    assert!(!saved["private"]["operation"].is_null());
                    assert_eq!(ingress.applied_access, Some(EndpointAccess::Internet));
                    for (name, value) in retained.as_ref().unwrap() {
                        assert_eq!(graphs.lock().unwrap().objects.get(name), Some(value));
                    }
                    ingress = serde_json::from_value(saved).unwrap();
                }
            }
            if ready {
                break;
            }
        }
        if access == EndpointAccess::Internet {
            assert!(ready);
            retained = Some(
                graphs
                    .lock()
                    .unwrap()
                    .objects
                    .iter()
                    .filter(|(name, _)| name.starts_with("frontend/"))
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>(),
            );
            continue;
        }
        assert!(!ready);
        let saved = serde_json::to_value(&ingress).unwrap();
        assert!(!saved["private"]["operation"].is_null());
        assert!(ingress.private_endpoints.is_empty());
        assert_eq!(ingress.applied_access, Some(EndpointAccess::Internet));
        {
            let cloud = graphs.lock().unwrap();
            let certificates: Vec<_> = cloud
                .objects
                .iter()
                .filter(|(name, _)| name.starts_with("certificate/public-"))
                .collect();
            assert_eq!(
                certificates.len(),
                1,
                "old public certificate was not retired"
            );
            let (name, certificate) = certificates[0];
            assert_eq!(
                certificate["selfManaged"]["certificate"],
                "expanded-certificate"
            );
            let certificate_name = name.strip_prefix("certificate/").unwrap();
            let proxies: Vec<_> = cloud
                .objects
                .iter()
                .filter(|(name, _)| name.starts_with("proxy/public-"))
                .collect();
            assert_eq!(proxies.len(), 1);
            let references = proxies[0].1["sslCertificates"].as_array().unwrap();
            assert_eq!(references.len(), 1);
            assert!(references[0]
                .as_str()
                .unwrap()
                .ends_with(&format!("/sslCertificates/{certificate_name}")));
        }
        ingress = serde_json::from_value(saved).unwrap();
        graphs.lock().unwrap().pending_regional_operations = false;
        for _ in 0..30 {
            assert!(!executor
                .with_context(|ctx| futures::executor::block_on(
                    ingress.reconcile(ctx, &worker, "neg", true)
                ))
                .unwrap());
        }
        assert!(!ingress.private_endpoints.is_empty());
        assert_eq!(ingress.applied_access, Some(EndpointAccess::Internet));
        for (name, value) in retained.as_ref().unwrap() {
            assert_eq!(graphs.lock().unwrap().objects.get(name), Some(value));
        }
        metadata
            .resources
            .get_mut("worker")
            .unwrap()
            .private_endpoints
            .get_mut("api")
            .unwrap()
            .dns_status = DnsRecordStatus::Active;
        executor.set_domain_metadata(metadata.clone());
        assert!(executor
            .with_context(|ctx| futures::executor::block_on(
                ingress.reconcile(ctx, &worker, "neg", true)
            ))
            .unwrap());
        assert_eq!(ingress.applied_access, Some(EndpointAccess::Both));
        for _ in 0..40 {
            if executor
                .with_context(|ctx| futures::executor::block_on(ingress.delete(ctx, "worker")))
                .unwrap()
            {
                break;
            }
        }
        assert!(graphs.lock().unwrap().objects.is_empty());
    }
    assert_eq!(failures, usize::from(lose_certificate_response));
}
