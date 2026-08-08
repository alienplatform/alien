//! Dev helper: render an operator manifest to stdout so it can be `kubectl
//! apply`d. Used to redeploy the demo operator with the latest RBAC/CRD/image.
//! Not shipped — a local convenience only.
//!
//! Usage:
//!   cargo run -p alien-helm --example render_operator_manifest -- \
//!     <manager_url> <image> <project_name> <environment_name> <namespace>

use alien_helm::{
    generate_operator_manifest, OperatorManifestOptions, OperatorOutputFormat, OperatorPermission,
    OperatorScope,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let manager_url = args.get(1).map(String::as_str).unwrap_or("https://manager.example.com");
    let image = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("public.ecr.aws/example/operator:1.0.0");
    let project_name = args.get(3).map(String::as_str).unwrap_or("demo");
    let environment_name = args.get(4).map(String::as_str).unwrap_or("test3");
    let namespace = args.get(5).map(String::as_str).unwrap_or("default");

    // Token/key are only embedded in the operator Secret, which we do NOT
    // re-apply (the existing one is reused). Placeholder values keep the render
    // valid; the CRD + RBAC + Deployment docs are what we apply.
    let manifest = generate_operator_manifest(OperatorManifestOptions {
        manager_url,
        group_token: "PLACEHOLDER_GROUP_TOKEN",
        encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        image,
        log_collector: None,
        project_name,
        environment_name: Some(environment_name),
        install_namespace: Some(namespace),
        label_domain: None,
        scope: OperatorScope::Namespace,
        label_selector: None,
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    })
    .expect("manifest should render");

    print!("{manifest}");
}
