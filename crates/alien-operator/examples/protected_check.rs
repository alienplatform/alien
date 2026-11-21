//! Runs the Operator's protected check against a running manager, as the Operator does
//! before every pass: fetch a release and the signer lists, verify them against the trust
//! the install kept, and keep what it accepted.
//!
//! ```text
//! protected_check <manager-url> <token> <project> <release-id> <platform> <state-file>
//! ```
//!
//! The first run for a state file starts an install that pins the project's first signer list.
//! Exit status 0: the release runs; 1: refused.

use std::collections::HashMap;
use std::sync::Arc;

use alien_core::sync::TargetDeployment;
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo,
    SignedStatement, StackSettings,
};
use alien_operator::protected::{InstallLedger, OperatorTrust, ReleaseSigning};
use alien_signing::{fingerprint, InstallId, Lookup, SigningSettings};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug)]
struct NoLedger;

#[async_trait]
impl InstallLedger for NoLedger {
    async fn load(&self) -> alien_operator::error::Result<Lookup> {
        Ok(Lookup::Absent)
    }
    async fn store(&self, _: &alien_signing::Ledger) -> alien_operator::error::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize)]
struct Lists {
    items: Vec<SignedStatement>,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 7 {
        eprintln!("usage: protected_check <manager-url> <token> <project> <release-id> <platform> <state-file>");
        std::process::exit(2);
    }
    let (url, token, project, release_id, platform, state_file) =
        (&args[1], &args[2], &args[3], &args[4], &args[5], &args[6]);
    let platform: Platform = platform.parse().expect("platform");
    let http = reqwest::Client::new();
    let get = |path: String| {
        let request = http.get(format!("{url}{path}")).bearer_auth(token);
        async move {
            request
                .send()
                .await
                .and_then(|response| response.error_for_status())
                .expect("manager request")
                .json::<serde_json::Value>()
                .await
                .expect("json")
        }
    };
    let lists: Lists =
        serde_json::from_value(get(format!("/v1/projects/{project}/signer-lists")).await)
            .expect("signer lists");
    let release = get(format!("/v1/releases/{release_id}")).await;
    let platform_key = platform.as_str();
    let info = ReleaseInfo {
        release_id: Some(release_id.clone()),
        version: None,
        description: None,
        stack: serde_json::from_value(release["stack"][platform_key].clone()).expect("stack"),
        signed: serde_json::from_value(release["signedStacks"][platform_key].clone()).ok(),
        signer_lists: lists.items.clone(),
    };

    let root = lists.items[0].open().expect("first signer list").digest;
    let signing = ReleaseSigning {
        settings: SigningSettings {
            fingerprint: fingerprint(&root),
            prefix: project.clone(),
            channel: "production".to_string(),
        },
        install: InstallId::Kubernetes {
            cluster: "8a2f0c4e-1111-2222-3333-444455556666".to_string(),
            namespace: "apps".to_string(),
            release: project.clone(),
        },
        ledger: Arc::new(NoLedger),
    };
    let stored: OperatorTrust = match std::fs::read(state_file) {
        Ok(bytes) => serde_json::from_slice(&bytes).expect("state file"),
        Err(_) => OperatorTrust::new(project),
    };
    let target = TargetDeployment {
        signed_config: None,
        release_info: info,
        config: DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .input_values(HashMap::new())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build(),
    };
    match signing.protect_target(platform, &serde_json::Map::new(), &stored, &target) {
        Ok((_, verified, kept)) => {
            std::fs::write(state_file, serde_json::to_vec_pretty(&kept).unwrap()).unwrap();
            println!(
                "RUNS {release_id}: sequence {} signed by {:?}, signer list v{}",
                verified.sequence,
                verified.signers,
                lists.items.len()
            );
        }
        Err(refusal) => {
            println!("REFUSED {release_id}: {refusal}");
            std::process::exit(1);
        }
    }
}
