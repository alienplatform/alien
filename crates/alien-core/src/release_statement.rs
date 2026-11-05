//! Release statements for stacks.
//!
//! A signed release is a `release/v2` statement from `alien-signing`.
//! Its body names every workload exactly, for enforcers that build workloads
//! one by one, and carries the whole stack, for enforcers that deploy from it.
//! [`release_body`] builds that body from a pushed stack; [`signed_stack`]
//! reads the stack back out of a verified body and checks it agrees with the
//! workloads beside it.

use std::collections::BTreeMap;

use alien_error::AlienError;
use alien_signing::release::{
    Bounds, Configurable, FunctionSettings, Mount, ProvidedEnv, Runtime, Security, WorkloadKind,
};
use alien_signing::{ReleaseBody, Workload};

use crate::error::{ErrorData, Result};
use crate::{
    Container, ContainerCode, Daemon, DaemonCode, Platform, Resource, Sandbox, SandboxCode, Stack,
    Worker, WorkerCode,
};

/// Resource types that run no code of their own. A type missing from this
/// list and from the code-bearing types is refused, so a resource type added
/// later can't carry an unchecked image.
const RESOURCE_TYPES_WITHOUT_CODE: &[&str] = &[
    "vault",
    "compute-cluster",
    "kubernetes-cluster",
    "storage",
    "key",
    "queue",
    "email",
    "kv",
    "postgres",
    "ai",
    "network",
    "build",
    "service-account",
    "artifact-registry",
    "service_activation",
    "remote-stack-management",
    "resource-access",
];

/// Environment names the deployment fills in for every workload: binding and
/// runtime variables, telemetry endpoints and the listening port. Their values
/// depend on resources that don't exist when the release is signed.
const PROVIDED_ENV_PREFIXES: &[&str] = &["ALIEN_", "OTEL_"];
const PROVIDED_ENV_NAMES: &[&str] = &["PORT"];

/// What a release lets deployment config change.
#[derive(Debug, Clone, Default)]
pub struct ReleaseOptions {
    /// Environment names `config/v1` statements may set on every workload.
    pub configurable_env: Vec<String>,
    /// Developer input ids `config/v1` statements may set.
    pub configurable_inputs: Vec<String>,
}

/// The `release/v2` body for `stack` on `platform`. Every image must be
/// pinned by digest and built before the release, so source code, unpinned
/// images and resource types nothing can check are refused here, where the
/// developer sees why.
pub fn release_body(
    platform: Platform,
    stack: &Stack,
    options: &ReleaseOptions,
) -> Result<ReleaseBody> {
    let mut workloads = Vec::new();
    for (id, entry) in stack.resources() {
        if let Some(mut workload) = workload(id, &entry.config, options)? {
            add_input_env(&mut workload, stack, platform);
            workloads.push(workload);
        }
    }
    let mut configurable_env = options.configurable_env.clone();
    let mut configurable_inputs = options.configurable_inputs.clone();
    for input in stack
        .inputs
        .iter()
        .filter(|input| developer_input(input, platform))
    {
        if !configurable_inputs.contains(&input.id) {
            configurable_inputs.push(input.id.clone());
        }
        for mapping in input.env.iter().filter(|mapping| plain(mapping)) {
            if !configurable_env.contains(&mapping.name) {
                configurable_env.push(mapping.name.clone());
            }
        }
    }
    // Configurable names apply to every workload, so the body reads back
    // the same from the stack and the release's options.
    for workload in &mut workloads {
        workload.configurable_env = configurable_env.clone();
    }
    let body = ReleaseBody {
        platform: platform.as_str().to_string(),
        workloads,
        configurable: Configurable {
            env: configurable_env,
            inputs: configurable_inputs,
        },
        components: BTreeMap::new(),
        remote_access: Vec::new(),
        dynamic: None,
        plugins: BTreeMap::new(),
        signed_commands: Vec::new(),
        stack: Some(serde_json::to_value(stack).map_err(|error| {
            statement_invalid(format!("the stack could not be serialized: {error}"))
        })?),
    };
    body.validate()
        .map_err(|error| statement_invalid(error.message))?;
    Ok(body)
}

/// The stack a verified release carries, after checking it is for `platform`
/// and that each code-bearing resource runs exactly the image of the workload
/// of the same name.
pub fn signed_stack(body: &ReleaseBody, platform: Platform) -> Result<Stack> {
    if body.platform != platform.as_str() {
        return Err(statement_invalid(format!(
            "the release is for platform '{}', not '{}'",
            body.platform,
            platform.as_str()
        )));
    }
    let value = body
        .stack
        .clone()
        .ok_or_else(|| statement_invalid("the release carries no stack"))?;
    let stack: Stack = serde_json::from_value(value)
        .map_err(|error| statement_invalid(format!("the signed stack does not parse: {error}")))?;
    let expected = release_body(
        platform,
        &stack,
        &ReleaseOptions {
            configurable_env: body.configurable.env.clone(),
            configurable_inputs: body.configurable.inputs.clone(),
        },
    )?;
    if expected.workloads != body.workloads {
        return Err(statement_invalid(
            "the signed workloads do not match the signed stack",
        ));
    }
    Ok(stack)
}

/// Env names a workload takes from deployer inputs: the enforcer fills them
/// from the customer's store. (Developer inputs become configurable names,
/// set by signed config. Secret inputs keep their own path: the value is
/// never signed, and the stack binds the name.)
fn add_input_env(workload: &mut Workload, stack: &Stack, platform: Platform) {
    for input in stack
        .inputs
        .iter()
        .filter(|input| input.applies_to(platform))
    {
        for mapping in input
            .env
            .iter()
            .filter(|mapping| plain(mapping) && mapping.targets(&workload.name))
        {
            if input
                .provided_by
                .contains(&crate::StackInputProvider::Deployer)
            {
                workload
                    .deployer_env
                    .insert(mapping.name.clone(), input.id.clone());
            }
        }
    }
}

fn developer_input(input: &crate::StackInputDefinition, platform: Platform) -> bool {
    input.applies_to(platform)
        && !input
            .provided_by
            .contains(&crate::StackInputProvider::Deployer)
}

fn plain(mapping: &crate::StackInputEnvironmentMapping) -> bool {
    !matches!(
        mapping.var_type,
        Some(crate::StackInputEnvironmentVariableType::Secret)
    )
}

fn workload(id: &str, config: &Resource, options: &ReleaseOptions) -> Result<Option<Workload>> {
    let base = |kind: WorkloadKind, image: &str| Workload {
        name: id.to_string(),
        kind,
        image: image.to_string(),
        command: None,
        env: BTreeMap::new(),
        provided_env: ProvidedEnv {
            names: PROVIDED_ENV_NAMES
                .iter()
                .map(|name| name.to_string())
                .collect(),
            prefixes: PROVIDED_ENV_PREFIXES
                .iter()
                .map(|prefix| prefix.to_string())
                .collect(),
        },
        configurable_env: options.configurable_env.clone(),
        deployer_env: BTreeMap::new(),
        secrets: Vec::new(),
        runtime: Runtime::default(),
        volume_mount_path: None,
        identity: None,
        bounds: Bounds::default(),
        function: None,
    };
    if let Some(container) = config.downcast_ref::<Container>() {
        let image = pinned_image(id, container_image(id, container)?)?;
        let mut workload = base(WorkloadKind::Container, image);
        workload.command = container.command.clone();
        workload.env = container.environment.clone().into_iter().collect();
        workload.runtime = Runtime {
            security: container.security.as_ref().map(|security| Security {
                profile: if container.egress.is_some() {
                    "restrictedInternet".to_string()
                } else {
                    "restricted".to_string()
                },
                run_as_user: security.run_as_user,
                run_as_group: security.run_as_group,
                read_only_root_filesystem: security.read_only_root_filesystem,
            }),
            egress: container.egress.map(|_| "internet".to_string()),
            ..Runtime::default()
        };
        workload.volume_mount_path = container
            .persistent_storage
            .as_ref()
            .map(|storage| storage.mount_path.clone());
        workload.identity = Some(container.permissions.clone());
        let choices = container.resource_choices.as_ref();
        workload.bounds = Bounds {
            max_cpu: Some(
                choices
                    .and_then(|choices| choices.cpu.as_ref())
                    .map(|range| range.max.clone())
                    .unwrap_or_else(|| container.cpu.desired.clone()),
            ),
            max_memory: Some(
                choices
                    .and_then(|choices| choices.memory.as_ref())
                    .map(|range| range.max.clone())
                    .unwrap_or_else(|| container.memory.desired.clone()),
            ),
            max_replicas: container
                .autoscaling
                .as_ref()
                .map(|autoscaling| autoscaling.max)
                .or(container.replicas),
        };
        return Ok(Some(workload));
    }
    if let Some(daemon) = config.downcast_ref::<Daemon>() {
        let image = match &daemon.code {
            DaemonCode::Image { image } => pinned_image(id, image)?,
            DaemonCode::Source { .. } => return Err(source_code(id)),
        };
        let mut workload = base(WorkloadKind::Daemon, image);
        workload.command = daemon.command.clone();
        workload.env = daemon.environment.clone().into_iter().collect();
        if let Some(runtime) = &daemon.runtime {
            workload.runtime = Runtime {
                privileged: runtime.privileged.unwrap_or(false),
                pid_namespace: runtime.pid_namespace.clone(),
                network_mode: runtime.network_mode.clone(),
                user: runtime.user.clone(),
                mounts: runtime
                    .mounts
                    .iter()
                    .map(|mount| Mount {
                        source: mount.source.clone(),
                        target: mount.target.clone(),
                        options: mount.options.clone(),
                    })
                    .collect(),
                security: None,
                egress: None,
            };
        }
        workload.identity = daemon.permissions.clone();
        workload.bounds = Bounds {
            max_cpu: Some(daemon.cpu.desired.clone()),
            max_memory: Some(daemon.memory.desired.clone()),
            max_replicas: None,
        };
        return Ok(Some(workload));
    }
    if let Some(worker) = config.downcast_ref::<Worker>() {
        let image = match &worker.code {
            WorkerCode::Image { image } => pinned_image(id, image)?,
            WorkerCode::Source { .. } => return Err(source_code(id)),
        };
        let mut workload = base(WorkloadKind::Function, image);
        workload.env = worker.environment.clone().into_iter().collect();
        workload.identity = Some(worker.permissions.clone());
        workload.function = Some(FunctionSettings {
            memory_mb: worker.memory_mb,
            timeout_seconds: worker.timeout_seconds,
            architecture: None,
        });
        return Ok(Some(workload));
    }
    if config.downcast_ref::<Sandbox>().is_some() {
        return Err(statement_invalid(format!(
            "resource '{id}' is a sandbox; signed releases don't carry sandboxes yet, because their image is assembled in the registry"
        )));
    }
    let resource_type = config.resource_type();
    if RESOURCE_TYPES_WITHOUT_CODE.contains(&resource_type.as_ref()) {
        return Ok(None);
    }
    Err(statement_invalid(format!(
        "resource '{id}' has type '{resource_type}', which a signed release can't carry"
    )))
}

fn container_image<'a>(id: &str, container: &'a Container) -> Result<&'a str> {
    match &container.code {
        ContainerCode::Image { image } => Ok(image),
        ContainerCode::Source { .. } => Err(source_code(id)),
    }
}

fn pinned_image<'a>(id: &str, image: &'a str) -> Result<&'a str> {
    if image_digest(image).is_none() {
        return Err(statement_invalid(format!(
            "resource '{id}' uses image '{image}', which is not pinned by sha256 digest"
        )));
    }
    Ok(image)
}

/// The image a resource runs, for the resource types that run one.
pub fn code_image(resource: &Resource) -> Option<&str> {
    if let Some(container) = resource.downcast_ref::<Container>() {
        match &container.code {
            ContainerCode::Image { image } => Some(image),
            ContainerCode::Source { .. } => None,
        }
    } else if let Some(worker) = resource.downcast_ref::<Worker>() {
        match &worker.code {
            WorkerCode::Image { image } => Some(image),
            WorkerCode::Source { .. } => None,
        }
    } else if let Some(daemon) = resource.downcast_ref::<Daemon>() {
        match &daemon.code {
            DaemonCode::Image { image } => Some(image),
            DaemonCode::Source { .. } => None,
        }
    } else if let Some(sandbox) = resource.downcast_ref::<Sandbox>() {
        match &sandbox.code {
            SandboxCode::Image { image, .. } => Some(image),
            SandboxCode::Source { .. } => None,
        }
    } else {
        None
    }
}

/// Mutable access to the image [`code_image`] returns.
pub fn code_image_mut(resource: &mut Resource) -> Option<&mut String> {
    // Test the type before borrowing mutably: returning a borrow out of an
    // `if let` chain of `downcast_mut` calls doesn't pass the stable borrow
    // checker.
    if resource.downcast_ref::<Container>().is_some() {
        match &mut resource.downcast_mut::<Container>()?.code {
            ContainerCode::Image { image } => Some(image),
            ContainerCode::Source { .. } => None,
        }
    } else if resource.downcast_ref::<Worker>().is_some() {
        match &mut resource.downcast_mut::<Worker>()?.code {
            WorkerCode::Image { image } => Some(image),
            WorkerCode::Source { .. } => None,
        }
    } else if resource.downcast_ref::<Daemon>().is_some() {
        match &mut resource.downcast_mut::<Daemon>()?.code {
            DaemonCode::Image { image } => Some(image),
            DaemonCode::Source { .. } => None,
        }
    } else if resource.downcast_ref::<Sandbox>().is_some() {
        match &mut resource.downcast_mut::<Sandbox>()?.code {
            SandboxCode::Image { image, .. } => Some(image),
            SandboxCode::Source { .. } => None,
        }
    } else {
        None
    }
}

/// `signed` with each image moved to where `carried` says the same resource's
/// image is, when both name the same digest. A carrier may serve an image from
/// another registry (a manager's pull proxy, a site's own registry), but only
/// the bytes the developer signed.
pub fn relocate_images(signed: &Stack, carried: &Stack) -> Stack {
    let mut relocated = signed.clone();
    for (id, entry) in relocated.resources_mut() {
        let Some(location) = carried
            .resources
            .get(id)
            .and_then(|carried| code_image(&carried.config))
        else {
            continue;
        };
        if let Some(image) = code_image_mut(&mut entry.config) {
            if image_digest(image).is_some() && image_digest(image) == image_digest(location) {
                *image = location.to_string();
            }
        }
    }
    relocated
}

/// The `sha256:<hex>` digest an image reference is pinned to, if it is.
pub fn image_digest(image: &str) -> Option<&str> {
    let (repository, digest) = image.rsplit_once('@')?;
    let valid = !repository.is_empty()
        && !repository.chars().any(char::is_whitespace)
        && alien_signing::release::is_sha256_digest(digest);
    valid.then_some(digest)
}

fn source_code(id: &str) -> AlienError<ErrorData> {
    statement_invalid(format!(
        "resource '{id}' is built from source, which a signed release can't carry; build it first"
    ))
}

fn statement_invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ReleaseStatementInvalid {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceLifecycle, ResourceSpec};

    const DIGEST: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn container(image: &str) -> Container {
        Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: image.to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("execution".to_string())
            .environment(std::collections::HashMap::from([(
                "LOG_LEVEL".to_string(),
                "info".to_string(),
            )]))
            .build()
    }

    fn stack(image: &str) -> Stack {
        Stack::new("shop".to_string())
            .add(container(image), ResourceLifecycle::Live)
            .build()
    }

    fn pinned() -> String {
        format!("registry.example.com/shop/api@{DIGEST}")
    }

    #[test]
    fn a_pushed_stack_becomes_workloads_that_build_back_to_the_stack() {
        let stack = stack(&pinned());
        let body = release_body(Platform::Aws, &stack, &ReleaseOptions::default()).unwrap();
        assert_eq!(body.workloads.len(), 1);
        let api = &body.workloads[0];
        assert_eq!(api.name, "api");
        assert_eq!(api.image, pinned());
        assert_eq!(api.env["LOG_LEVEL"], "info");
        assert_eq!(api.identity.as_deref(), Some("execution"));
        assert_eq!(api.bounds.max_cpu.as_deref(), Some("1"));
        assert_eq!(api.bounds.max_memory.as_deref(), Some("1Gi"));
        assert!(api.provided_env.allows("ALIEN_STORAGE_BINDING"));
        assert!(!api.provided_env.allows("NODE_OPTIONS"));
        assert_eq!(signed_stack(&body, Platform::Aws).unwrap(), stack);
    }

    #[test]
    fn stack_inputs_become_configurable_or_deployer_env() {
        let mut stack = stack(&pinned());
        stack.inputs = serde_json::from_value(serde_json::json!([
            {"id": "logLevel", "kind": "string", "providedBy": ["developer"], "required": false,
             "label": "l", "description": "d", "env": [{"name": "LOG_LEVEL", "targetResources": ["api"]}]},
            {"id": "databaseUrl", "kind": "string", "providedBy": ["deployer"], "required": false,
             "label": "l", "description": "d", "env": [{"name": "DATABASE_URL", "targetResources": ["api"]}]},
        ]))
        .unwrap();
        // LOG_LEVEL is also fixed in the container's environment, which a
        // developer input can't override.
        let mut container = container(&pinned());
        container.environment.clear();
        stack.resources.get_mut("api").unwrap().config = alien_core_resource(container);
        let body = release_body(Platform::Aws, &stack, &ReleaseOptions::default()).unwrap();
        let api = &body.workloads[0];
        assert_eq!(api.deployer_env["DATABASE_URL"], "databaseUrl");
        assert!(api.configurable_env.contains(&"LOG_LEVEL".to_string()));
        assert_eq!(body.configurable.inputs, vec!["logLevel".to_string()]);
        assert_eq!(signed_stack(&body, Platform::Aws).unwrap(), stack);
    }

    fn alien_core_resource(container: Container) -> Resource {
        Resource::new(container)
    }

    #[test]
    fn unsigned_code_is_refused_before_signing() {
        let error = release_body(
            Platform::Aws,
            &stack("registry.example.com/shop/api:latest"),
            &ReleaseOptions::default(),
        )
        .expect_err("a tag is refused");
        assert!(error.message.contains("not pinned"), "{}", error.message);

        let options = ReleaseOptions {
            configurable_env: vec!["LD_PRELOAD".to_string()],
            configurable_inputs: Vec::new(),
        };
        release_body(Platform::Aws, &stack(&pinned()), &options)
            .expect_err("a loader variable can't be configurable");
    }

    #[test]
    fn a_stack_that_disagrees_with_its_workloads_is_refused() {
        let mut body =
            release_body(Platform::Aws, &stack(&pinned()), &ReleaseOptions::default()).unwrap();
        let other = format!("registry.example.com/shop/other@{DIGEST}");
        body.stack = Some(serde_json::to_value(stack(&other)).unwrap());
        signed_stack(&body, Platform::Aws).expect_err("a swapped stack is refused");

        let body =
            release_body(Platform::Aws, &stack(&pinned()), &ReleaseOptions::default()).unwrap();
        signed_stack(&body, Platform::Gcp).expect_err("another platform is refused");
    }

    #[test]
    fn images_move_between_registries_only_with_the_same_digest() {
        let signed = stack(&pinned());
        let carried = stack(&format!("proxy.example.com/shop/api@{DIGEST}"));
        let relocated = relocate_images(&signed, &carried);
        assert_eq!(
            code_image(&relocated.resources["api"].config),
            Some(format!("proxy.example.com/shop/api@{DIGEST}").as_str())
        );

        let other_digest = format!("proxy.example.com/shop/api@sha256:{}", "0".repeat(64));
        let relocated = relocate_images(&signed, &stack(&other_digest));
        assert_eq!(
            code_image(&relocated.resources["api"].config),
            Some(pinned().as_str())
        );
    }
}
