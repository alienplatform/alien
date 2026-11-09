//! Release statements for stacks.
//!
//! A signed release is a `release/v2` statement from `alien-signing`.
//! Its body names every workload exactly, for enforcers that build workloads
//! one by one, and carries the whole stack, for enforcers that deploy from it.
//! It also names its channel, the enforcer builds it runs with, and the
//! digests of the setup templates rendered for it.
//! [`release_body`] builds that body from a pushed stack; [`signed_stack`]
//! reads the stack back out of a verified body and checks it agrees with the
//! workloads beside it.

use std::collections::BTreeMap;

use alien_error::AlienError;
use alien_signing::release::{
    Bounds, Configurable, FunctionSettings, Mount, Runtime, Security, WorkloadKind,
};
use alien_signing::{ConfigBody, ReleaseBody, SetupTemplate, Workload};

use crate::error::{ErrorData, Result};
use crate::runtime_environment::*;
use crate::{
    Container, ContainerCode, Daemon, DaemonCode, Platform, Resource, ResourceRef, Sandbox,
    SandboxCode, Stack, Worker, WorkerCode,
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

/// Environment names the deployment fills in for every workload, exactly:
/// runtime variables and the listening port. Their values depend on the
/// deployment, and none of them chooses code or a resource. Bindings are not
/// here: the enforcer computes them, and the secrets list it builds itself.
const PROVIDED_ENV_NAMES: &[&str] = &[
    "PORT",
    ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME,
    ENV_ALIEN_CURRENT_WORKER_BINDING_NAME,
    ENV_ALIEN_DEPLOYMENT_TYPE,
    ENV_ALIEN_LAMBDA_MODE,
    ENV_ALIEN_TRANSPORT,
    ENV_AWS_ACCOUNT_ID,
    ENV_AWS_REGION,
    ENV_AZURE_CLIENT_ID,
    ENV_AZURE_REGION,
    ENV_AZURE_SUBSCRIPTION_ID,
    ENV_AZURE_TENANT_ID,
    ENV_GCP_PROJECT_ID,
    ENV_GCP_REGION,
    ENV_GOOGLE_CLOUD_PROJECT,
    ENV_ALIEN_WORKER_GRPC_ADDRESS,
    ENV_ALIEN_QUEUE_BATCH_CONFIG,
    ENV_ALIEN_WORKER_TIMEOUT_SECONDS,
    ENV_ALIEN_COMMANDS_TOKEN,
    ENV_ALIEN_COMMANDS_TOKEN_FILE,
    ENV_ALIEN_COMMANDS_LEASE_SECONDS,
    ENV_ALIEN_COMMANDS_MAX_LEASES,
    ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS,
    ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS,
    ENV_ALIEN_COMMANDS_POLL_JITTER,
    ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS,
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
    ENV_ALIEN_COMMANDS_URL,
    ENV_ALIEN_DEPLOYMENT_ID,
    ENV_ALIEN_DEPLOYMENT_NAME,
    ENV_ALIEN_DEPLOYMENT_TOKEN,
    ENV_ALIEN_MANAGER_URL,
    ENV_ALIEN_RESOURCE_ID,
    ENV_ALIEN_PUBLIC_ENDPOINTS_JSON,
    ENV_ALIEN_SECRET_ENV_REVISION,
];

/// What a release carries beyond the stack.
#[derive(Debug, Clone, Default)]
pub struct ReleaseOptions {
    /// Release channel, such as `production`.
    pub channel: String,
    /// Environment names `config/v1` statements may set on every workload.
    pub configurable_env: Vec<String>,
    /// Developer input ids `config/v1` statements may set.
    pub configurable_inputs: Vec<String>,
    /// Digest of each enforcer build, by component name.
    pub components: BTreeMap<String, String>,
    /// Digests of the rendered setup templates, by template name.
    pub setup: BTreeMap<String, SetupTemplate>,
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
    // In name order: the stored stack comes back with its resources in id order, whatever
    // order they were declared in, and the workloads it builds must be these.
    workloads.sort_by(|a, b| a.name.cmp(&b.name));
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
        channel: options.channel.clone(),
        workloads,
        configurable: Configurable {
            env: configurable_env,
            inputs: configurable_inputs,
        },
        components: options.components.clone(),
        setup: options.setup.clone(),
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
            channel: body.channel.clone(),
            configurable_env: body.configurable.env.clone(),
            configurable_inputs: body.configurable.inputs.clone(),
            components: body.components.clone(),
            setup: body.setup.clone(),
        },
    )?;
    if expected.workloads != body.workloads {
        return Err(statement_invalid(
            "the signed workloads do not match the signed stack",
        ));
    }
    Ok(stack)
}

/// The plain env mappings of the stack's developer inputs, resolved from the values a signed
/// config carries (defaults included). Each variable keeps its mapping's `target_resources`.
/// Deployer inputs never travel in a signed config: `deployer_values` only decides which
/// inputs apply, when a developer input is conditional on one, and no deployer mapping is
/// returned. Secret mappings are left out: their values are never signed.
pub fn signed_input_variables(
    stack: &Stack,
    platform: Platform,
    developer_values: &BTreeMap<String, serde_json::Value>,
    deployer_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<Vec<crate::EnvironmentVariable>> {
    let mut values = std::collections::HashMap::new();
    let mut inputs = Vec::with_capacity(stack.inputs.len());
    for input in &stack.inputs {
        let developer = developer_input(input, platform);
        let value = if developer {
            developer_values.get(&input.id)
        } else {
            deployer_values.get(&input.id)
        };
        if let Some(value) = value {
            values.insert(input.id.clone(), value.clone());
        }
        let mut input = input.clone();
        if !developer {
            input.env.clear();
        }
        inputs.push(input);
    }
    Ok(
        crate::resolve_stack_input_environment_variables(&inputs, &values, platform)?
            .into_iter()
            .filter(|variable| variable.var_type == crate::EnvironmentVariableType::Plain)
            .collect(),
    )
}

/// The deployer inputs whose value decides whether a developer input applies (its `when`
/// names them). The install's own store holds them, so whoever builds a signed config's env
/// reads these and passes them to [`signed_config_env`].
pub fn gating_deployer_inputs(body: &ReleaseBody) -> Result<Vec<String>> {
    if body.stack.is_none() {
        return Ok(Vec::new());
    }
    let platform: Platform = body.platform.parse().map_err(|_| {
        statement_invalid(format!("the release names no platform '{}'", body.platform))
    })?;
    let stack = signed_stack(body, platform)?;
    let mut gating = std::collections::BTreeSet::new();
    for input in stack
        .inputs
        .iter()
        .filter(|input| developer_input(input, platform))
    {
        let Some(condition) = &input.when else {
            continue;
        };
        if stack
            .inputs
            .iter()
            .any(|other| other.id == condition.input && !developer_input(other, platform))
        {
            gating.insert(condition.input.clone());
        }
    }
    Ok(gating.into_iter().collect())
}

/// The env a signed config sets on `workload`: the values of the release's developer inputs,
/// resolved through the signed stack's env mappings (with each input's default), then the
/// config's own `env`, which wins. A controller and the verifier that checks it, the machine
/// agent and the Kubernetes operator all build the configured env with this, so an input signed
/// by `set-inputs` reaches the worker the same way it does on an unprotected install. Deployer
/// inputs and secret mappings are not here: they never travel in a signed config.
/// `deployer_values` holds the install's own values of [`gating_deployer_inputs`]: a developer
/// input that is conditional on a deployer input applies as the customer set that input. A
/// verifier reads them from the install's store; a controller uses the deployment's input
/// values. A value that differs between the two only makes the verifier refuse the write.
pub fn signed_config_env(
    body: &ReleaseBody,
    workload: &str,
    config: &ConfigBody,
    deployer_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    if body.stack.is_some() {
        let platform: Platform = body.platform.parse().map_err(|_| {
            statement_invalid(format!("the release names no platform '{}'", body.platform))
        })?;
        let stack = signed_stack(body, platform)?;
        for variable in signed_input_variables(&stack, platform, &config.inputs, deployer_values)? {
            if crate::targets_resource(&variable.target_resources, workload) {
                env.insert(variable.name, variable.value);
            }
        }
    }
    env.extend(config.env.clone());
    Ok(env)
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
        provided_env: PROVIDED_ENV_NAMES
            .iter()
            .map(|name| name.to_string())
            .collect(),
        bindings: BTreeMap::new(),
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
        workload.bindings = bindings(&container.links);
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
        workload.bindings = bindings(&daemon.links);
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
        workload.bindings = bindings(&worker.links);
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

/// The resources a workload links to, by id, with their type.
fn bindings(links: &[ResourceRef]) -> BTreeMap<String, String> {
    links
        .iter()
        .map(|link| (link.id.clone(), link.resource_type.to_string()))
        .collect()
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

    fn production() -> ReleaseOptions {
        ReleaseOptions {
            channel: "production".to_string(),
            ..ReleaseOptions::default()
        }
    }

    #[test]
    fn a_pushed_stack_becomes_workloads_that_build_back_to_the_stack() {
        let stack = stack(&pinned());
        let body = release_body(Platform::Aws, &stack, &production()).unwrap();
        assert_eq!(body.workloads.len(), 1);
        let api = &body.workloads[0];
        assert_eq!(api.name, "api");
        assert_eq!(api.image, pinned());
        assert_eq!(api.env["LOG_LEVEL"], "info");
        assert_eq!(api.identity.as_deref(), Some("execution"));
        assert_eq!(api.bounds.max_cpu.as_deref(), Some("1"));
        assert_eq!(api.bounds.max_memory.as_deref(), Some("1Gi"));
        assert_eq!(body.channel, "production");
        assert!(api
            .provided_env
            .contains(&"ALIEN_DEPLOYMENT_ID".to_string()));
        assert!(
            !api.provided_env
                .iter()
                .any(|name| name.ends_with("_BINDING")
                    || name == ENV_ALIEN_SECRETS
                    || name.starts_with("OTEL_")),
            "bindings, the secrets list and telemetry are never the control plane's: {:?}",
            api.provided_env
        );
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
        let body = release_body(Platform::Aws, &stack, &production()).unwrap();
        let api = &body.workloads[0];
        assert_eq!(api.deployer_env["DATABASE_URL"], "databaseUrl");
        assert!(api.configurable_env.contains(&"LOG_LEVEL".to_string()));
        assert_eq!(body.configurable.inputs, vec!["logLevel".to_string()]);
        assert_eq!(signed_stack(&body, Platform::Aws).unwrap(), stack);
    }

    /// `set-inputs` signs developer input values, not env: the env a signed config sets is
    /// what the stack's mappings make of them (defaults included), for the workloads the
    /// mapping targets, with the config's own `env` winning.
    #[test]
    fn a_signed_config_sets_the_env_its_inputs_map_to() {
        let mut stack = stack(&pinned());
        stack.inputs = serde_json::from_value(serde_json::json!([
            {"id": "logLevel", "kind": "string", "providedBy": ["developer"], "required": false,
             "label": "l", "description": "d", "default": {"type": "string", "value": "info"},
             "env": [{"name": "LOG_LEVEL", "targetResources": ["api"]}]},
            {"id": "region", "kind": "string", "providedBy": ["developer"], "required": false,
             "label": "l", "description": "d",
             "env": [{"name": "REGION", "targetResources": ["api"]}, {"name": "ELSEWHERE", "targetResources": ["other"]}]},
            {"id": "token", "kind": "secret", "providedBy": ["developer"], "required": false,
             "label": "l", "description": "d", "env": [{"name": "TOKEN", "targetResources": ["api"]}]},
            {"id": "databaseUrl", "kind": "string", "providedBy": ["deployer"], "required": false,
             "label": "l", "description": "d", "default": {"type": "string", "value": "postgres://default"},
             "env": [{"name": "DATABASE_URL", "targetResources": ["api"]}]},
        ]))
        .unwrap();
        let mut api = container(&pinned());
        api.environment.clear();
        stack.resources.get_mut("api").unwrap().config = alien_core_resource(api);
        let body = release_body(Platform::Aws, &stack, &production()).unwrap();
        let config = |inputs: serde_json::Value, env: &[(&str, &str)]| ConfigBody {
            inputs: serde_json::from_value(inputs).unwrap(),
            env: env
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            ..Default::default()
        };

        let env = signed_config_env(
            &body,
            "api",
            &config(serde_json::json!({"region": "eu", "token": "s3cret"}), &[]),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            env,
            BTreeMap::from([
                ("LOG_LEVEL".to_string(), "info".to_string()),
                ("REGION".to_string(), "eu".to_string()),
            ]),
            "the default applies, secrets and deployer inputs never travel in a signed config, \
             and a mapping aimed at another resource isn't this workload's"
        );

        let env = signed_config_env(
            &body,
            "api",
            &config(
                serde_json::json!({"logLevel": "debug"}),
                &[("LOG_LEVEL", "trace")],
            ),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(env["LOG_LEVEL"], "trace", "the config's own env wins");
    }

    /// A developer input gated on a deployer input applies as the customer set that input,
    /// whatever its default, wherever the env is built from the install's own values.
    #[test]
    fn a_developer_input_gated_on_a_deployer_input_follows_the_installs_value() {
        let mut stack = stack(&pinned());
        stack.inputs = serde_json::from_value(serde_json::json!([
            {"id": "audit", "kind": "boolean", "providedBy": ["deployer"], "required": false,
             "label": "l", "description": "d", "default": {"type": "boolean", "value": true}},
            {"id": "auditSink", "kind": "string", "providedBy": ["developer"], "required": false,
             "label": "l", "description": "d", "when": {"input": "audit", "equals": true},
             "env": [{"name": "AUDIT_SINK", "targetResources": ["api"]}]},
            {"id": "logLevel", "kind": "string", "providedBy": ["developer"], "required": false,
             "label": "l", "description": "d", "env": [{"name": "LOG_LEVEL", "targetResources": ["api"]}]},
        ]))
        .unwrap();
        let mut api = container(&pinned());
        api.environment.clear();
        stack.resources.get_mut("api").unwrap().config = alien_core_resource(api);
        let body = release_body(Platform::Aws, &stack, &production()).unwrap();
        assert_eq!(
            gating_deployer_inputs(&body).unwrap(),
            vec!["audit".to_string()]
        );

        let config = ConfigBody {
            inputs: serde_json::from_value(
                serde_json::json!({"auditSink": "https://audit.example"}),
            )
            .unwrap(),
            ..Default::default()
        };
        let with = |audit: Option<&str>| {
            let values = audit
                .map(|audit| {
                    std::collections::HashMap::from([(
                        "audit".to_string(),
                        serde_json::Value::String(audit.to_string()),
                    )])
                })
                .unwrap_or_default();
            signed_config_env(&body, "api", &config, &values).unwrap()
        };
        assert_eq!(with(Some("true"))["AUDIT_SINK"], "https://audit.example");
        assert_eq!(
            with(None)["AUDIT_SINK"],
            "https://audit.example",
            "the default is on"
        );
        assert!(
            !with(Some("false")).contains_key("AUDIT_SINK"),
            "the customer turned it off"
        );
    }

    fn alien_core_resource(container: Container) -> Resource {
        Resource::new(container)
    }

    #[test]
    fn unsigned_code_is_refused_before_signing() {
        let error = release_body(
            Platform::Aws,
            &stack("registry.example.com/shop/api:latest"),
            &production(),
        )
        .expect_err("a tag is refused");
        assert!(error.message.contains("not pinned"), "{}", error.message);

        let options = ReleaseOptions {
            configurable_env: vec!["LD_PRELOAD".to_string()],
            ..production()
        };
        release_body(Platform::Aws, &stack(&pinned()), &options)
            .expect_err("a loader variable can't be configurable");
    }

    /// The stored stack comes back with its resources in id order, whatever order the vendor
    /// declared them in, so a release built from a stack declared out of order must still read
    /// back as its own stack. (Before, signing such a release failed: "the signed workloads do
    /// not match the signed stack".)
    #[test]
    fn a_stack_declared_out_of_order_reads_back_as_its_own() {
        let mut web = container(&pinned());
        web.id = "web".to_string();
        let mut agent = container(&pinned());
        agent.id = "agent".to_string();
        let stack = Stack::new("shop".to_string())
            .add(web, ResourceLifecycle::Live)
            .add(agent, ResourceLifecycle::Live)
            .build();
        let body = release_body(Platform::Aws, &stack, &production()).unwrap();
        assert_eq!(
            body.workloads
                .iter()
                .map(|workload| workload.name.as_str())
                .collect::<Vec<_>>(),
            ["agent", "web"],
            "workloads are in name order"
        );
        let back = signed_stack(&body, Platform::Aws).expect("the release reads back as its stack");
        // The order the stack's resources read back in follows serde_json's map order, which a
        // workspace build can switch to insertion order; only which resources come back matters.
        let mut ids = back
            .resources()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(ids, ["agent", "web"]);
    }

    #[test]
    fn a_stack_that_disagrees_with_its_workloads_is_refused() {
        let mut body = release_body(Platform::Aws, &stack(&pinned()), &production()).unwrap();
        let other = format!("registry.example.com/shop/other@{DIGEST}");
        body.stack = Some(serde_json::to_value(stack(&other)).unwrap());
        signed_stack(&body, Platform::Aws).expect_err("a swapped stack is refused");

        let body = release_body(Platform::Aws, &stack(&pinned()), &production()).unwrap();
        signed_stack(&body, Platform::Gcp).expect_err("another platform is refused");
    }

    #[test]
    fn links_become_bindings_the_enforcer_computes() {
        let mut container = container(&pinned());
        container.links.push(ResourceRef::new(
            crate::ResourceType::from("storage"),
            "uploads".to_string(),
        ));
        let stack = Stack::new("shop".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();
        let body = release_body(Platform::Aws, &stack, &production()).unwrap();
        assert_eq!(
            body.workloads[0].bindings,
            BTreeMap::from([("uploads".to_string(), "storage".to_string())])
        );
        let mut released_without_channel = body.clone();
        released_without_channel.channel = String::new();
        released_without_channel
            .validate()
            .expect_err("a release names its channel");
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
