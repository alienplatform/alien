//! Run one verifier request against a real GCP project or Azure resource
//! group from a workstation, for the machine roll tests: the same stores,
//! backends and checks as the deployed verifier, with the identity it would
//! learn from its cloud, the image pins and the setup-fixed network given
//! here instead.
//!
//! ```text
//! verifier-local gcp <project> <region> <own-name> <publisher> <minimum> <network-json> < request.json
//! verifier-local azure <resource-group-id> <vault-url> <own-name> <publisher> <minimum> <network-json> < request.json
//! ```
//!
//! GCP takes its access token from the file in `VERIFIER_GCP_TOKEN_FILE`;
//! Azure from `IDENTITY_ENDPOINT`/`IDENTITY_HEADER`, as on Container Apps.
//! When `SSL_CERT_FILE` is set, its certificates are trusted too (a
//! workstation behind a TLS proxy).
//! Prints the answer, or the refusal and exits 1. See
//! `scripts/gate-e2e/cloud_machines.py`.

use std::io::Read as _;
use std::sync::Arc;

use alien_gate::azure::{Azure, AzureBackend, AzureInstall};
use alien_gate::gcp::{Gcp, GcpBackend, GcpInstall, TokenSource};
use alien_gate::machines::ImagePins;
use alien_gate::plan::SetupNetwork;
use alien_gate::{describe, Verifier};
use alien_signing::InstallId;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cloud, first, second, own_name, publisher, minimum, network] = args.as_slice() else {
        eprintln!("usage: verifier-local gcp|azure <project|resource-group-id> <region|vault-url> <own-name> <publisher> <minimum> <network-json>");
        return std::process::ExitCode::from(2);
    };
    let pins = ImagePins {
        publisher: Some(publisher.clone()),
        minimum_created: Some(minimum.parse().expect("the minimum is RFC 3339")),
    };
    let network: SetupNetwork = serde_json::from_str(network).expect("the network is setup JSON");
    let http = http_client();
    let verifier = match cloud.as_str() {
        "gcp" => {
            let token = std::env::var("VERIFIER_GCP_TOKEN_FILE").expect("VERIFIER_GCP_TOKEN_FILE");
            let client =
                Gcp::new(TokenSource::File(token), first.clone(), second.clone()).with_http(http);
            let project = first.clone();
            Verifier {
                own_name: own_name.clone(),
                install_id: Arc::new(move |prefix: &str| InstallId::Gcp {
                    project: project.clone(),
                    prefix: prefix.to_string(),
                }),
                store: Arc::new(GcpInstall(client.clone())),
                backend: Arc::new(GcpBackend(client, pins)),
                vpc: None,
                machine_network: Some(network),
            }
        }
        "azure" => {
            let parts: Vec<&str> = first.trim_start_matches('/').split('/').collect();
            let ["subscriptions", subscription, "resourceGroups", group] = parts.as_slice() else {
                panic!("the resource group is /subscriptions/<id>/resourceGroups/<name>");
            };
            let (subscription, group) = (subscription.to_string(), group.to_string());
            let client = Azure::new(first.clone(), second.clone(), None).with_http(http);
            Verifier {
                own_name: own_name.clone(),
                install_id: Arc::new(move |_prefix: &str| InstallId::Azure {
                    subscription: subscription.clone(),
                    resource_group: group.clone(),
                }),
                store: Arc::new(AzureInstall(client.clone())),
                backend: Arc::new(AzureBackend(client, pins)),
                vpc: None,
                machine_network: Some(network),
            }
        }
        other => panic!("unknown cloud {other}"),
    };
    let mut request = String::new();
    std::io::stdin()
        .read_to_string(&mut request)
        .expect("read the request");
    let request = serde_json::from_str(&request).expect("the request is JSON");
    match verifier.handle(request).await {
        Ok(answer) => {
            println!("{answer}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", serde_json::json!({"errorMessage": describe(&error)}));
            std::process::ExitCode::from(1)
        }
    }
}

/// A client trusting the built-in roots and, when set, `SSL_CERT_FILE`.
fn http_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    if let Ok(path) = std::env::var("SSL_CERT_FILE") {
        let pem = std::fs::read(&path).expect("read SSL_CERT_FILE");
        for certificate in
            reqwest::Certificate::from_pem_bundle(&pem).expect("SSL_CERT_FILE is a PEM bundle")
        {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder.build().expect("build the HTTP client")
}
