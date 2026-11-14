//! Assembles an Azure sandbox image the way `alien release` does: the packaged agent layered onto
//! a base image, ENTRYPOINT preserved for the agent to start. Used by the Azure live proofs
//! (`scripts/azure-internet-live.sh`), where the base and the agent image sit in a plain-HTTP
//! local registry and the result is pushed to a registry that takes basic credentials.
//!
//! ```text
//! ACR_USER=... ACR_PASS=... cargo run -p alien-build --example azure-sandbox-assemble -- \
//!     <base-ref> <agent-image-ref> <target-ref>
//! ```

use alien_build::gcp_sandbox_image::build_azure_sandbox_image;
use dockdash::{Arch, BlobCache, ClientProtocol, Image, PullPolicy, PushOptions, RegistryAuth};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, base_ref, agent_ref, target] = args.as_slice() else {
        panic!("usage: azure-sandbox-assemble <base-ref> <agent-image-ref> <target-ref>");
    };
    let cache = BlobCache::new().expect("blob cache");
    let (base, _) = Image::builder()
        .from(base_ref)
        .platform("linux", &Arch::Amd64)
        .pull_policy(PullPolicy::Always)
        .protocol(ClientProtocol::Http)
        .auth(RegistryAuth::Anonymous)
        .blob_cache(cache.clone())
        .build()
        .await
        .expect("the base image pulls");
    let assembled = build_azure_sandbox_image(
        "azure-live",
        base,
        base_ref,
        agent_ref,
        ClientProtocol::Http,
        RegistryAuth::Anonymous,
        cache,
    )
    .await
    .expect("the sandbox image assembles");
    assembled
        .image
        .push(
            target,
            &PushOptions {
                auth: RegistryAuth::Basic(env("ACR_USER"), env("ACR_PASS")),
                protocol: ClientProtocol::Https,
                ..Default::default()
            },
        )
        .await
        .expect("the image pushes");
    println!("pushed {target}");
}
