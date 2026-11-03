//! Resolving an `alien.toml` document into the `Stack` the TypeScript SDK would produce.
//!
//! Each check here runs against the spanned document, so a rejected value is reported at
//! its line and column rather than as a field path in a resolved stack.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::Path;

use alien_core::stack_authoring::{
    self, ComputeChoice, ComputePool, CpuAmount, DataDiskSize, InputValue as AuthoredValue,
    PoolRequirements, PoolScale,
};
use alien_core::{
    ComputeChoiceRange, ComputeCluster, Daemon as DaemonConfig, DaemonCode, DaemonRuntime,
    ExposeProtocol, GpuSpec, HealthCheck, PermissionProfile, PermissionSetReference,
    PermissionsConfig, Platform, PublicEndpoint, Resource, ResourceEntry, ResourceLifecycle,
    ResourceRef, ResourceType, Stack, StackInputDefaultValue, StackInputDefinition,
    StackInputEnvironmentMapping, StackInputKind, StackInputProvider, StackInputValidation,
    Storage as StorageConfig, ToolchainConfig, APEX_HOST_LABEL,
};
use alien_error::AlienError;
use indexmap::IndexMap;
use toml::Spanned;

use crate::document::{
    self, Choice, Cpu, DataDisk, Document, Duration, EnvTarget, InputValue, Tables,
};
use crate::env_file;
use crate::error::{ErrorData, Result};
use crate::source::SourceFile;

/// ID of the compute cluster holding the pools when the document does not name one. It is
/// the name Alien gives a cluster it creates for a stack that declares none.
pub const DEFAULT_CLUSTER_ID: &str = "compute";

const MAX_RESOURCE_ID_LENGTH: usize = 64;
const MAX_STACK_NAME_LENGTH: usize = 128;

/// Resolves `document` into a stack. `base_dir` is the directory holding alien.toml, which
/// `env-file` paths are relative to.
pub fn resolve(document: Document, file: &SourceFile<'_>, base_dir: &Path) -> Result<Stack> {
    Resolver { file, base_dir }.resolve(document)
}

struct Resolver<'a> {
    file: &'a SourceFile<'a>,
    base_dir: &'a Path,
}

/// What other resources may refer to: a resource's type, by ID.
type Declared = IndexMap<String, (ResourceType, Range<usize>)>;

impl Resolver<'_> {
    fn error(&self, span: Range<usize>, message: impl Into<String>) -> AlienError<ErrorData> {
        self.file.error(span, message)
    }

    fn resolve(&self, document: Document) -> Result<Stack> {
        let Document {
            name,
            platforms,
            cluster,
            pool,
            daemon,
            storage,
            input,
            permissions,
        } = document;

        self.check_id(&name, "stack name", MAX_STACK_NAME_LENGTH)?;
        let platforms = platforms
            .map(|platforms| self.platform_list(platforms, None))
            .transpose()?;

        let pools = in_order(pool);
        let storages = in_order(storage);
        let daemons = in_order(daemon);

        // Every resource ID is stack-wide, whatever its type.
        let mut declared = Declared::new();
        let cluster_id = match (&cluster, pools.is_empty()) {
            (Some(cluster), true) => {
                return Err(self.error(
                    cluster.span(),
                    "`cluster` names the cluster that holds the pools, but there are no [pool.<name>] tables",
                ))
            }
            (Some(cluster), false) => {
                self.check_id(cluster, "cluster ID", MAX_RESOURCE_ID_LENGTH)?;
                Some(cluster.get_ref().clone())
            }
            (None, false) => Some(DEFAULT_CLUSTER_ID.to_string()),
            (None, true) => None,
        };
        if let Some(cluster_id) = &cluster_id {
            let span = cluster.as_ref().map_or(0..0, Spanned::span);
            declared.insert(cluster_id.clone(), (ComputeCluster::RESOURCE_TYPE, span));
        }
        // In file order, so a clash is reported at the later declaration.
        let mut ids: Vec<(&Spanned<String>, ResourceType)> = storages
            .iter()
            .map(|(id, _)| (id, StorageConfig::RESOURCE_TYPE))
            .chain(
                daemons
                    .iter()
                    .map(|(id, _)| (id, DaemonConfig::RESOURCE_TYPE)),
            )
            .collect();
        ids.sort_by_key(|(id, _)| id.span().start);
        for (id, resource_type) in ids {
            self.declare(&mut declared, id, resource_type)?;
        }

        let mut resources = IndexMap::new();
        let permissions = self.permissions(in_order(permissions), &declared)?;

        let mut inputs = Inputs::default();
        for (id, input) in in_order(input) {
            let definition =
                self.input(&id, input.into_inner(), &declared, platforms.as_deref())?;
            inputs.insert(self, id.span(), definition)?;
        }

        if let Some(cluster_id) = &cluster_id {
            let cluster = self.cluster(cluster_id, &pools)?;
            resources.insert(
                cluster_id.clone(),
                entry(Resource::new(cluster), ResourceLifecycle::Frozen),
            );
        }

        for (id, storage) in storages {
            let storage = storage.into_inner();
            let mut resource = entry(
                Resource::new(StorageConfig {
                    id: id.get_ref().clone(),
                    public_read: storage.public_read,
                    versioning: storage.versioning,
                    lifecycle_rules: Vec::new(),
                    cors_allowed_origins: Vec::new(),
                    notifications: Vec::new(),
                    encryption_key: None,
                }),
                storage.lifecycle.unwrap_or(ResourceLifecycle::Frozen),
            );
            resource.enabled_when = inputs.gate(
                self,
                &id,
                "storage bucket",
                storage.platforms,
                storage.enabled_when,
                platforms.as_deref(),
            )?;
            resources.insert(id.into_inner(), resource);
        }

        let pool_ids: Vec<&str> = pools.iter().map(|(id, _)| id.get_ref().as_str()).collect();
        for (id, daemon) in daemons {
            let daemon = self.daemon(
                &id,
                daemon.into_inner(),
                cluster_id.as_deref(),
                &pool_ids,
                &declared,
                &permissions,
            )?;
            if let Some(span) = daemon.identity_with_host_access {
                let targets = daemon
                    .scope
                    .as_ref()
                    .map(|scope| scope.get_ref().as_slice())
                    .or(platforms.as_deref());
                let cloud = [Platform::Aws, Platform::Gcp, Platform::Azure];
                if let Some(platform) = cloud
                    .iter()
                    .find(|cloud| targets.is_none_or(|targets| targets.contains(cloud)))
                {
                    return Err(self.error(
                        span,
                        format!(
                            "on {platform} a daemon with `host-access` could use the machine's credentials, so it cannot also have `permissions`; drop one, or limit the daemon's `platforms`"
                        ),
                    ));
                }
            }
            let mut resource = entry(Resource::new(daemon.config), daemon.lifecycle);
            resource.enabled_when = inputs.gate(
                self,
                &id,
                "daemon",
                daemon.scope,
                daemon.enabled_when,
                platforms.as_deref(),
            )?;
            for (key, secret) in daemon.secrets {
                inputs.add_secret(self, id.get_ref(), key, secret, platforms.as_deref())?;
            }
            resources.insert(id.into_inner(), resource);
        }

        let mut stack = Stack::new(name.into_inner()).build();
        stack.resources = resources;
        stack.supported_platforms = platforms;
        stack.permissions = permissions;
        stack.inputs = inputs.finish();
        Ok(stack)
    }

    fn declare(
        &self,
        declared: &mut Declared,
        id: &Spanned<String>,
        resource_type: ResourceType,
    ) -> Result<()> {
        self.check_id(id, "resource ID", MAX_RESOURCE_ID_LENGTH)?;
        if let Some((other_type, other_span)) = declared.get(id.get_ref()) {
            if *other_type == ComputeCluster::RESOURCE_TYPE {
                return Err(self.error(
                    id.span(),
                    format!(
                        "'{}' is the ID of the compute cluster that holds the pools; rename this resource or set a different top-level `cluster`",
                        id.get_ref()
                    ),
                ));
            }
            let line = self.line_of(other_span.start);
            return Err(self.error(
                id.span(),
                format!(
                    "'{}' is already declared as a {other_type} on line {line}; resource IDs are shared by every resource type",
                    id.get_ref()
                ),
            ));
        }
        declared.insert(id.get_ref().clone(), (resource_type, id.span()));
        Ok(())
    }

    fn line_of(&self, offset: usize) -> usize {
        self.file.text[..offset.min(self.file.text.len())]
            .matches('\n')
            .count()
            + 1
    }

    fn check_id(&self, id: &Spanned<String>, what: &str, max_length: usize) -> Result<()> {
        let value = id.get_ref();
        let valid_chars = value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if value.is_empty() || !valid_chars || value.len() > max_length {
            return Err(self.error(
                id.span(),
                format!(
                    "{what} '{value}' must be 1 to {max_length} letters, digits, hyphens or underscores"
                ),
            ));
        }
        Ok(())
    }

    /// A platform list; with `allowed`, every entry must be one of the stack's platforms.
    fn platform_list(
        &self,
        platforms: Spanned<Vec<Platform>>,
        allowed: Option<&[Platform]>,
    ) -> Result<Vec<Platform>> {
        let span = platforms.span();
        let platforms = platforms.into_inner();
        if platforms.is_empty() {
            return Err(self.error(
                span,
                "list at least one platform, or leave `platforms` out to allow every platform",
            ));
        }
        for (index, platform) in platforms.iter().enumerate() {
            if platforms[..index].contains(platform) {
                return Err(self.error(span, format!("'{platform}' is listed twice")));
            }
            if let Some(allowed) = allowed.filter(|allowed| !allowed.contains(platform)) {
                return Err(self.error(
                    span,
                    format!(
                        "'{platform}' is not one of the stack's platforms ({})",
                        join(allowed.iter().map(Platform::as_str))
                    ),
                ));
            }
        }
        Ok(platforms)
    }

    fn cluster(
        &self,
        cluster_id: &str,
        pools: &[(Spanned<String>, Spanned<document::Pool>)],
    ) -> Result<ComputeCluster> {
        let mut capacity_groups = Vec::new();
        let mut failure_domain_spread = BTreeMap::new();
        for (id, pool) in pools {
            self.check_id(id, "pool name", MAX_RESOURCE_ID_LENGTH)?;
            let pool = pool.get_ref();
            let scale = match (&pool.machines, &pool.autoscale) {
                (Some(machines), None) => PoolScale::Fixed {
                    machines: choice(machines.get_ref()),
                },
                (None, Some(autoscale)) => PoolScale::Autoscale {
                    min: choice(autoscale.get_ref().min.get_ref()),
                    max: choice(autoscale.get_ref().max.get_ref()),
                },
                (Some(_), Some(autoscale)) => {
                    return Err(self.error(
                        autoscale.span(),
                        "set either `machines` (a fixed count) or `autoscale`, not both",
                    ))
                }
                (None, None) => {
                    return Err(self.error(
                        id.span(),
                        "pool needs `machines = <count>` or `autoscale = { min = <count>, max = <count> }`",
                    ))
                }
            };
            let requirements = PoolRequirements {
                cpu: cpu_amount(pool.cpu.get_ref()),
                memory: pool.memory.get_ref().clone(),
                ephemeral_storage: pool.disk.as_ref().map(|disk| disk.get_ref().clone()),
                architecture: pool.arch,
                nested_virtualization: pool.nested_virtualization,
                gpu: pool.gpu.as_ref().map(|gpu| GpuSpec {
                    gpu_type: gpu.gpu_type.clone(),
                    count: gpu.count,
                }),
                data_disk: pool.data_disk.as_ref().map(|disk| match disk.get_ref() {
                    DataDisk::Size(size) => DataDiskSize::Fixed(size.clone()),
                    DataDisk::Range(range) => DataDiskSize::Range {
                        min: range.min.clone(),
                        default: range.default.clone(),
                        max: range.max.clone(),
                    },
                }),
            };
            let group = ComputePool {
                requirements,
                scale,
            }
            .to_capacity_group(id.get_ref())
            .map_err(|error| {
                let (field, reason) = definition_error(error);
                let span = match field.as_str() {
                    "cpu" => Some(pool.cpu.span()),
                    "memory" => Some(pool.memory.span()),
                    "ephemeralStorage" => pool.disk.as_ref().map(Spanned::span),
                    field if field.starts_with("dataDisk") => {
                        pool.data_disk.as_ref().map(Spanned::span)
                    }
                    "scale.machines" => pool.machines.as_ref().map(Spanned::span),
                    "scale.min" => pool.autoscale.as_ref().map(|a| a.get_ref().min.span()),
                    "scale.max" => pool.autoscale.as_ref().map(|a| a.get_ref().max.span()),
                    "scale" => pool.autoscale.as_ref().map(Spanned::span),
                    _ => None,
                };
                self.error(span.unwrap_or_else(|| id.span()), reason)
            })?;
            capacity_groups.push(group);

            if let Some(spread) = &pool.failure_domain_spread {
                let spread = stack_authoring::failure_domain_spread(
                    "failure-domain-spread",
                    *spread.get_ref(),
                )
                .map_err(|error| self.error(spread.span(), definition_error(error).1))?;
                failure_domain_spread.insert(id.get_ref().clone(), spread);
            }
        }

        let mut builder = ComputeCluster::new(cluster_id.to_string())
            .failure_domain_spread(failure_domain_spread);
        for group in capacity_groups {
            builder = builder.capacity_group(group);
        }
        Ok(builder.build())
    }

    fn daemon(
        &self,
        id: &Spanned<String>,
        daemon: document::Daemon,
        cluster_id: Option<&str>,
        pool_ids: &[&str],
        declared: &Declared,
        permissions: &PermissionsConfig,
    ) -> Result<ResolvedDaemon> {
        let daemon_id = id.get_ref();
        let code = match (daemon.build, daemon.image) {
            (Some(build), None) => {
                let build = build.into_inner();
                DaemonCode::Source {
                    src: build.src.unwrap_or_else(|| ".".to_string()),
                    toolchain: ToolchainConfig::Rust {
                        binary_name: build.rust,
                    },
                }
            }
            (None, Some(image)) => DaemonCode::Image {
                image: image.into_inner(),
            },
            (Some(_), Some(image)) => return Err(self.error(
                image.span(),
                "set either `build` (build from source) or `image` (a prebuilt image), not both",
            )),
            (None, None) => {
                return Err(self.error(
                    id.span(),
                    "daemon needs `build = { rust = \"<binary>\" }` or `image = \"<reference>\"`",
                ))
            }
        };

        let pool = match &daemon.pool {
            None => None,
            Some(pool) if pool_ids.contains(&pool.get_ref().as_str()) => {
                Some(pool.get_ref().clone())
            }
            Some(pool) if pool_ids.is_empty() => {
                return Err(self.error(
                    pool.span(),
                    format!(
                        "unknown pool '{}'; declare it with a [pool.{}] table",
                        pool.get_ref(),
                        pool.get_ref()
                    ),
                ))
            }
            Some(pool) => {
                return Err(self.error(
                    pool.span(),
                    format!(
                        "unknown pool '{}'; declared pools: {}",
                        pool.get_ref(),
                        join(pool_ids.iter().copied())
                    ),
                ))
            }
        };

        let cpu = match &daemon.cpu {
            Some(cpu) => cpu_amount(cpu.get_ref())
                .to_resource_spec("cpu")
                .map_err(|error| self.error(cpu.span(), definition_error(error).1))?,
            None => alien_core::default_daemon_cpu(),
        };
        let memory = match &daemon.memory {
            Some(memory) => stack_authoring::memory_resource_spec("memory", memory.get_ref())
                .map_err(|error| self.error(memory.span(), definition_error(error).1))?,
            None => alien_core::default_daemon_memory(),
        };

        let stop_grace_period_seconds = daemon
            .stop_grace_period
            .as_ref()
            .map(|period| {
                let seconds = self.seconds(period)?;
                stack_authoring::stop_grace_period_seconds("stop-grace-period", seconds)
                    .map_err(|error| self.error(period.span(), definition_error(error).1))
            })
            .transpose()?;

        let health_check = daemon
            .health
            .map(|health| self.health(health))
            .transpose()?;

        let environment = self.environment(&daemon.env, daemon.env_file.as_ref())?;

        let links = daemon
            .links
            .iter()
            .map(|link| match declared.get(link.get_ref()) {
                Some(_) if link.get_ref() == daemon_id => {
                    Err(self.error(link.span(), "a daemon cannot link to itself"))
                }
                Some((resource_type, _)) => Ok(ResourceRef::new(
                    resource_type.clone(),
                    link.get_ref().clone(),
                )),
                None => Err(self.error(
                    link.span(),
                    format!(
                        "unknown resource '{}'; declared resources: {}",
                        link.get_ref(),
                        join(
                            declared
                                .keys()
                                .filter(|declared| *declared != daemon_id)
                                .map(String::as_str)
                        )
                    ),
                )),
            })
            .collect::<Result<Vec<_>>>()?;

        let public_endpoints = self.endpoints(daemon_id, in_order(daemon.endpoints))?;

        let secrets = in_order(daemon.secrets);
        for (key, _) in &secrets {
            if !is_input_env_name(key.get_ref()) {
                return Err(self.error(
                    key.span(),
                    format!(
                        "secret '{}' must be an environment variable name: uppercase letters, digits and underscores",
                        key.get_ref()
                    ),
                ));
            }
            if environment.contains_key(key.get_ref()) {
                return Err(self.error(
                    key.span(),
                    format!(
                        "'{}' is both a secret and an environment variable of this daemon; keep one",
                        key.get_ref()
                    ),
                ));
            }
        }

        let identity_with_host_access = daemon
            .permissions
            .as_ref()
            .filter(|_| daemon.host_access)
            .map(Spanned::span);
        let profile = match daemon.permissions {
            Some(profile) if permissions.profiles.contains_key(profile.get_ref()) => {
                Some(profile.into_inner())
            }
            Some(profile) => {
                return Err(self.error(
                    profile.span(),
                    format!(
                        "unknown permission profile '{}'; declare it with a [permissions.{}] table",
                        profile.get_ref(),
                        profile.get_ref()
                    ),
                ))
            }
            None => None,
        };

        let config = DaemonConfig {
            id: daemon_id.clone(),
            links,
            public_endpoints,
            health_check,
            cluster: cluster_id.map(str::to_string),
            permissions: profile,
            code,
            cpu,
            memory,
            pool,
            command: daemon.command,
            stop_grace_period_seconds,
            runtime: daemon.host_access.then(DaemonRuntime::host_access),
            environment,
            commands_enabled: false,
        };
        Ok(ResolvedDaemon {
            config,
            lifecycle: daemon.lifecycle.unwrap_or(ResourceLifecycle::Live),
            scope: daemon.platforms,
            enabled_when: daemon.enabled_when,
            secrets,
            identity_with_host_access,
        })
    }

    fn input(
        &self,
        id: &Spanned<String>,
        input: document::Input,
        declared: &Declared,
        stack_platforms: Option<&[Platform]>,
    ) -> Result<StackInputDefinition> {
        let name = id.get_ref();
        if !is_input_id(name) || !name.chars().any(|c| c.is_ascii_lowercase()) {
            return Err(self.error(
                id.span(),
                format!(
                    "input ID '{name}' must start with a letter or underscore, use only letters, digits and underscores, and not be all caps"
                ),
            ));
        }
        let kind = StackInputKind::from(input.kind);

        let provided_by = match input.provided_by {
            None => vec![StackInputProvider::Developer],
            Some(providers) => {
                let span = providers.span();
                let providers = providers.into_inner().into_vec();
                if providers.is_empty() {
                    return Err(self.error(span, "list at least one provider"));
                }
                if providers.len() == 2 && providers[0] == providers[1] {
                    return Err(self.error(span, "a provider is listed twice"));
                }
                providers
                    .into_iter()
                    .map(StackInputProvider::from)
                    .collect()
            }
        };

        let required = input.required.unwrap_or(true);
        let description = match input.description {
            Some(description) => description,
            None if required => return Err(self.error(
                id.span(),
                "a required input needs a `description`; it is shown where the value is asked for",
            )),
            None => String::new(),
        };

        let default = input
            .default
            .as_ref()
            .map(|value| {
                stack_authoring::input_default(&kind, &authored(value.get_ref()))
                    .map_err(|error| self.error(value.span(), definition_error(error).1))
            })
            .transpose()?;

        let only_for = |span: Range<usize>, key: &str, kinds: &[StackInputKind], what: &str| {
            if kinds.contains(&kind) {
                Ok(())
            } else {
                Err(self.error(span, format!("`{key}` applies to {what} inputs only")))
            }
        };
        let text = [StackInputKind::String, StackInputKind::Secret];
        let numeric = [StackInputKind::Number, StackInputKind::Integer];
        let mut validation = StackInputValidation::default();
        if let Some(value) = input.min_length {
            only_for(value.span(), "min-length", &text, "string and secret")?;
            validation.min_length = Some(value.into_inner());
        }
        if let Some(value) = input.max_length {
            only_for(value.span(), "max-length", &text, "string and secret")?;
            validation.max_length = Some(value.into_inner());
        }
        if let Some(value) = input.pattern {
            only_for(value.span(), "pattern", &text, "string and secret")?;
            validation.pattern = Some(value.into_inner());
        }
        for (key, value, slot) in [
            ("min", input.min, &mut validation.min),
            ("max", input.max, &mut validation.max),
        ] {
            if let Some(value) = value {
                only_for(value.span(), key, &numeric, "number and integer")?;
                *slot = Some(
                    stack_authoring::input_number(key, &kind, &authored(value.get_ref()))
                        .map_err(|error| self.error(value.span(), definition_error(error).1))?,
                );
            }
        }
        match (input.values, kind == StackInputKind::Enum) {
            (Some(values), true) if values.get_ref().is_empty() => {
                return Err(self.error(values.span(), "list at least one value"))
            }
            (Some(values), true) => validation.values = Some(values.into_inner()),
            (Some(values), false) => {
                return Err(self.error(values.span(), "`values` applies to enum inputs only"))
            }
            (None, true) => {
                return Err(self.error(
                    id.span(),
                    "an enum input needs `values`, the values it accepts",
                ))
            }
            (None, false) => {}
        }
        if let Some(value) = input.min_items {
            only_for(
                value.span(),
                "min-items",
                &[StackInputKind::StringList],
                "string-list",
            )?;
            validation.min_items = Some(value.into_inner());
        }
        if let Some(value) = input.max_items {
            only_for(
                value.span(),
                "max-items",
                &[StackInputKind::StringList],
                "string-list",
            )?;
            validation.max_items = Some(value.into_inner());
        }

        let platforms = input
            .platforms
            .map(|platforms| self.platform_list(platforms, stack_platforms))
            .transpose()?;

        let env = match input.env {
            None => Vec::new(),
            Some(env) => {
                let span = env.span();
                env.into_inner()
                    .into_vec()
                    .into_iter()
                    .map(|target| self.env_mapping(span.clone(), target, declared))
                    .collect::<Result<Vec<_>>>()?
            }
        };

        Ok(StackInputDefinition {
            id: name.clone(),
            kind: kind.clone(),
            provided_by,
            required,
            label: input.label.unwrap_or_else(|| name.clone()),
            description,
            placeholder: input.placeholder,
            default,
            platforms,
            when: None,
            validation: stack_authoring::input_validation(&kind, validation),
            generate: None,
            env,
        })
    }

    fn env_mapping(
        &self,
        span: Range<usize>,
        target: EnvTarget,
        declared: &Declared,
    ) -> Result<StackInputEnvironmentMapping> {
        let (name, targets) = match target {
            EnvTarget::Name(name) => (name, None),
            EnvTarget::Mapping(mapping) => (mapping.name, mapping.targets),
        };
        if !is_input_env_name(&name) {
            return Err(self.error(
                span,
                format!(
                    "'{name}' must be an environment variable name: uppercase letters, digits and underscores"
                ),
            ));
        }
        if let Some(targets) = &targets {
            if targets.is_empty() {
                return Err(self.error(
                    span,
                    "`targets` is empty; leave it out to set the variable on every daemon",
                ));
            }
            for target in targets.iter().filter(|target| !target.ends_with('*')) {
                match declared.get(target) {
                    Some((resource_type, _)) if *resource_type == DaemonConfig::RESOURCE_TYPE => {}
                    Some((resource_type, _)) => {
                        return Err(self.error(
                            span,
                            format!(
                            "'{target}' is a {resource_type}, which takes no environment variables"
                        ),
                        ))
                    }
                    None => {
                        return Err(
                            self.error(span, format!("unknown resource '{target}' in `targets`"))
                        )
                    }
                }
            }
        }
        Ok(StackInputEnvironmentMapping {
            name,
            target_resources: targets,
            var_type: None,
        })
    }

    fn permissions(
        &self,
        profiles: Vec<(Spanned<String>, Spanned<document::Profile>)>,
        declared: &Declared,
    ) -> Result<PermissionsConfig> {
        let mut config = PermissionsConfig::new();
        for (name, grants) in profiles {
            self.check_id(&name, "permission profile name", MAX_RESOURCE_ID_LENGTH)?;
            let mut grants: Vec<_> = grants.into_inner().into_iter().collect();
            grants.sort_by_key(|(resource, _)| resource.span().start);
            let mut profile = PermissionProfile::new();
            for (resource, sets) in grants {
                if resource.get_ref() != "*" && !declared.contains_key(resource.get_ref()) {
                    return Err(self.error(
                        resource.span(),
                        format!(
                            "unknown resource '{}'; use a declared resource ID, or \"*\" for every resource",
                            resource.get_ref()
                        ),
                    ));
                }
                let sets = sets
                    .into_iter()
                    .map(|set| {
                        if set.get_ref().is_empty() {
                            Err(self.error(set.span(), "a permission set name cannot be empty"))
                        } else {
                            Ok(PermissionSetReference::from_name(set.into_inner()))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                profile.0.insert(resource.into_inner(), sets);
            }
            config.profiles.insert(name.into_inner(), profile);
        }
        Ok(config)
    }

    fn seconds(&self, duration: &Spanned<Duration>) -> Result<u64> {
        match duration.get_ref() {
            Duration::Seconds(seconds) => u64::try_from(*seconds)
                .map_err(|_| self.error(duration.span(), "a duration cannot be negative")),
            Duration::Text(text) => stack_authoring::parse_duration_seconds("duration", text)
                .map_err(|error| self.error(duration.span(), definition_error(error).1)),
        }
    }

    fn health(&self, health: Spanned<document::Health>) -> Result<HealthCheck> {
        let span = health.span();
        let health = health.into_inner();
        let defaults = HealthCheck::default();
        if health.port == 0 {
            return Err(self.error(span, "health `port` must be 1 to 65535"));
        }
        let timeout_seconds = match &health.timeout {
            Some(timeout) => {
                let seconds = self.seconds(timeout)?;
                u32::try_from(seconds)
                    .ok()
                    .filter(|seconds| *seconds >= 1)
                    .ok_or_else(|| {
                        self.error(timeout.span(), "health timeout must be at least 1 second")
                    })?
            }
            None => defaults.timeout_seconds,
        };
        Ok(HealthCheck {
            path: health.path.unwrap_or(defaults.path),
            port: Some(health.port),
            method: health.method.unwrap_or(defaults.method),
            timeout_seconds,
            failure_threshold: health
                .failure_threshold
                .unwrap_or(defaults.failure_threshold),
        })
    }

    /// `env-file` first, then `env` on top: inline values win.
    fn environment(
        &self,
        env: &IndexMap<Spanned<String>, Spanned<String>>,
        env_file: Option<&Spanned<String>>,
    ) -> Result<HashMap<String, String>> {
        let mut environment = HashMap::new();
        if let Some(env_file) = env_file {
            let path = self.base_dir.join(env_file.get_ref());
            let text = std::fs::read_to_string(&path).map_err(|error| {
                self.error(
                    env_file.span(),
                    format!("cannot read env file {}: {error}", path.display()),
                )
            })?;
            let variables = env_file::parse(&SourceFile {
                name: path.display().to_string(),
                text: &text,
            })?;
            environment.extend(variables);
        }
        for (key, value) in env {
            if !is_env_name(key.get_ref()) {
                return Err(self.error(
                    key.span(),
                    format!(
                        "'{}' is not an environment variable name; use letters, digits and underscores, not starting with a digit",
                        key.get_ref()
                    ),
                ));
            }
            environment.insert(key.get_ref().clone(), value.get_ref().clone());
        }
        Ok(environment)
    }

    fn endpoints(
        &self,
        daemon_id: &str,
        endpoints: Vec<(Spanned<String>, Spanned<document::Endpoint>)>,
    ) -> Result<Vec<PublicEndpoint>> {
        let mut resolved: Vec<PublicEndpoint> = Vec::new();
        for (name, endpoint) in endpoints {
            let span = endpoint.span();
            let endpoint = endpoint.into_inner();
            let public_endpoint = PublicEndpoint {
                name: name.get_ref().clone(),
                port: endpoint.port,
                protocol: ExposeProtocol::Http,
                host_label: endpoint.host,
                wildcard_subdomains: endpoint.wildcard,
            };
            public_endpoint
                .validate_for_resource(daemon_id)
                .map_err(|error| self.error(span.clone(), core_reason(error)))?;
            if let Some(first) = resolved.first().filter(|first| first.port != endpoint.port) {
                return Err(self.error(
                    span,
                    format!(
                        "all endpoints of a daemon must use the same port; '{}' uses {}",
                        first.name, first.port
                    ),
                ));
            }
            if public_endpoint.host_label.as_deref() == Some(APEX_HOST_LABEL) {
                if let Some(apex) = resolved
                    .iter()
                    .find(|other| other.host_label.as_deref() == Some(APEX_HOST_LABEL))
                {
                    return Err(self.error(
                        span,
                        format!(
                            "only one endpoint can use host \"@\"; '{}' already does",
                            apex.name
                        ),
                    ));
                }
            }
            resolved.push(public_endpoint);
        }
        Ok(resolved)
    }
}

/// A daemon resolved from its table, with what the stack-level pass still needs.
struct ResolvedDaemon {
    config: DaemonConfig,
    lifecycle: ResourceLifecycle,
    /// The daemon's own `platforms`, if it is scoped.
    scope: Option<Spanned<Vec<Platform>>>,
    /// The boolean input gating the daemon, if it names one.
    enabled_when: Option<Spanned<String>>,
    secrets: Vec<(Spanned<String>, Spanned<document::Secret>)>,
    /// Where `permissions` is set on a daemon that also has `host-access`.
    identity_with_host_access: Option<Range<usize>>,
}

/// Stack inputs the document declares through shorthand, in declaration order.
#[derive(Default)]
struct Inputs {
    definitions: IndexMap<String, StackInputDefinition>,
    /// For secrets: the settings and span that first declared each input, to merge or reject
    /// a second declaration of the same variable.
    secret_origins: HashMap<String, (document::Secret, Range<usize>)>,
}

impl Inputs {
    /// The input that gates a resource: one synthesized from `platforms`, or a declared
    /// boolean named by `enabled-when`.
    fn gate(
        &mut self,
        resolver: &Resolver<'_>,
        resource_id: &Spanned<String>,
        what: &str,
        scope: Option<Spanned<Vec<Platform>>>,
        enabled_when: Option<Spanned<String>>,
        stack_platforms: Option<&[Platform]>,
    ) -> Result<Option<String>> {
        match (scope, enabled_when) {
            (None, None) => Ok(None),
            (Some(scope), None) => self
                .add_platform_gate(resolver, resource_id, what, scope, stack_platforms)
                .map(Some),
            (Some(_), Some(enabled_when)) => Err(resolver.error(
                enabled_when.span(),
                "set `platforms` or `enabled-when`, not both; scope the gating input with its own `platforms`",
            )),
            (None, Some(enabled_when)) => {
                let id = enabled_when.get_ref();
                let reason = match self.definitions.get(id) {
                    None => Some(format!("unknown input '{id}'; declare it with an [input.{id}] table")),
                    Some(input) if input.kind != StackInputKind::Boolean => {
                        Some(format!("input '{id}' must be a boolean to decide whether the {what} exists"))
                    }
                    Some(input) if !input.provided_by.contains(&StackInputProvider::Deployer) => Some(format!(
                        "input '{id}' must be provided by the deployer (`provided-by = \"deployer\"`); otherwise setup never asks and the {what} is always created"
                    )),
                    Some(input) if !input.required && input.default.is_none() => Some(format!(
                        "input '{id}' must be required or have a `default`, so whether the {what} exists is always decided"
                    )),
                    Some(_) => None,
                };
                match reason {
                    Some(reason) => Err(resolver.error(enabled_when.span(), reason)),
                    None => Ok(Some(enabled_when.into_inner())),
                }
            }
        }
    }

    /// Scopes a resource to platforms with a deployer boolean that only exists there.
    fn add_platform_gate(
        &mut self,
        resolver: &Resolver<'_>,
        resource_id: &Spanned<String>,
        what: &str,
        scope: Spanned<Vec<Platform>>,
        stack_platforms: Option<&[Platform]>,
    ) -> Result<String> {
        let span = scope.span();
        let platforms = resolver.platform_list(scope, stack_platforms)?;
        let id = format!("{}Enabled", camel_case_resource_id(resource_id.get_ref()));
        if !is_input_id(&id) {
            return Err(resolver.error(
                span,
                format!(
                    "a {what} scoped to platforms needs an ID that starts with a letter; '{}' does not",
                    resource_id.get_ref()
                ),
            ));
        }
        self.insert(
            resolver,
            span,
            StackInputDefinition {
                id: id.clone(),
                kind: StackInputKind::Boolean,
                provided_by: vec![StackInputProvider::Deployer],
                required: true,
                label: format!("Create {}", resource_id.get_ref()),
                description: format!(
                    "Whether this deployment gets the '{}' {what}.",
                    resource_id.get_ref()
                ),
                placeholder: None,
                default: Some(StackInputDefaultValue::Boolean(true)),
                platforms: Some(platforms),
                when: None,
                validation: stack_authoring::input_validation(
                    &StackInputKind::Boolean,
                    StackInputValidation::default(),
                ),
                generate: None,
                env: Vec::new(),
            },
        )?;
        Ok(id)
    }

    fn add_secret(
        &mut self,
        resolver: &Resolver<'_>,
        daemon_id: &str,
        key: Spanned<String>,
        secret: Spanned<document::Secret>,
        stack_platforms: Option<&[Platform]>,
    ) -> Result<()> {
        let (span, secret) = (secret.span(), secret.into_inner());
        let name = key.get_ref();
        let id = camel_case_env_name(name);

        if let Some((first, first_span)) = self.secret_origins.get(&id) {
            if *first != secret {
                return Err(resolver.error(
                    span,
                    format!(
                        "secret '{name}' is also declared on line {} with different settings; a secret shared by several daemons must be declared the same way on each",
                        resolver.line_of(first_span.start)
                    ),
                ));
            }
            let definition = self
                .definitions
                .get_mut(&id)
                .expect("a recorded secret has a definition");
            definition.env[0]
                .target_resources
                .get_or_insert_with(Vec::new)
                .push(daemon_id.to_string());
            return Ok(());
        }

        let platforms = secret
            .platforms
            .clone()
            .map(|platforms| {
                resolver.platform_list(Spanned::new(span.clone(), platforms), stack_platforms)
            })
            .transpose()?;
        self.insert(
            resolver,
            key.span(),
            StackInputDefinition {
                id: id.clone(),
                kind: StackInputKind::Secret,
                provided_by: vec![StackInputProvider::Developer],
                required: secret.required.unwrap_or(true),
                label: secret.label.clone().unwrap_or_else(|| name.clone()),
                description: secret
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Sets the {name} environment variable.")),
                placeholder: None,
                default: None,
                platforms,
                when: None,
                validation: stack_authoring::input_validation(
                    &StackInputKind::Secret,
                    StackInputValidation::default(),
                ),
                generate: None,
                env: vec![StackInputEnvironmentMapping {
                    name: name.clone(),
                    target_resources: Some(vec![daemon_id.to_string()]),
                    var_type: None,
                }],
            },
        )?;
        self.secret_origins.insert(id, (secret, key.span()));
        Ok(())
    }

    fn insert(
        &mut self,
        resolver: &Resolver<'_>,
        span: Range<usize>,
        definition: StackInputDefinition,
    ) -> Result<()> {
        if let Some(existing) = self
            .definitions
            .keys()
            .find(|existing| existing.eq_ignore_ascii_case(&definition.id))
        {
            return Err(resolver.error(
                span,
                format!(
                    "this declares the stack input '{}', which clashes with the input '{existing}' declared earlier",
                    definition.id
                ),
            ));
        }
        self.definitions.insert(definition.id.clone(), definition);
        Ok(())
    }

    fn finish(self) -> Vec<StackInputDefinition> {
        self.definitions.into_values().collect()
    }
}

fn entry(config: Resource, lifecycle: ResourceLifecycle) -> ResourceEntry {
    ResourceEntry {
        display_name: None,
        description: None,
        config,
        lifecycle,
        dependencies: Vec::new(),
        remote_access: false,
        enabled_when: None,
    }
}

/// Tables in the order the file declares them.
fn in_order<T>(tables: Tables<T>) -> Vec<(Spanned<String>, Spanned<T>)> {
    let mut tables: Vec<_> = tables.into_iter().collect();
    tables.sort_by_key(|(id, _)| id.span().start);
    tables
}

fn choice(choice: &Choice) -> ComputeChoice {
    match choice {
        Choice::Exactly(count) => ComputeChoice::Exactly(*count),
        Choice::Range(range) => ComputeChoice::Range(ComputeChoiceRange {
            min: range.min,
            max: range.max,
            default: range.default,
        }),
    }
}

fn authored(value: &InputValue) -> AuthoredValue {
    match value {
        InputValue::String(text) => AuthoredValue::String(text.clone()),
        InputValue::Integer(number) => AuthoredValue::Integer(*number),
        InputValue::Float(number) => AuthoredValue::Float(*number),
        InputValue::Boolean(flag) => AuthoredValue::Boolean(*flag),
        InputValue::StringList(items) => AuthoredValue::StringList(items.clone()),
    }
}

fn cpu_amount(cpu: &Cpu) -> CpuAmount {
    match cpu {
        Cpu::Integer(cores) => CpuAmount::Integer(*cores),
        Cpu::Float(cores) => CpuAmount::Float(*cores),
        Cpu::Quantity(quantity) => CpuAmount::Quantity(quantity.clone()),
    }
}

/// The field and reason of an authoring error from `alien_core::stack_authoring`.
fn definition_error(error: AlienError<alien_core::ErrorData>) -> (String, String) {
    match error.error {
        Some(alien_core::ErrorData::StackDefinitionInvalid { field, reason }) => (field, reason),
        _ => (String::new(), error.message),
    }
}

/// The reason in an `alien_core` validation error, without the resource prefix.
fn core_reason(error: AlienError<alien_core::ErrorData>) -> String {
    match error.error {
        Some(alien_core::ErrorData::InvalidResourceUpdate { reason, .. }) => reason,
        _ => error.message,
    }
}

fn join<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Stack input env mappings take uppercase names only.
fn is_input_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.chars().any(|c| c.is_ascii_uppercase())
}

fn is_input_id(id: &str) -> bool {
    id.chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `AUTH_ACCESS_KEY` → `authAccessKey`.
fn camel_case_env_name(name: &str) -> String {
    let mut id = String::with_capacity(name.len());
    for (index, part) in name.split('_').filter(|part| !part.is_empty()).enumerate() {
        let part = part.to_ascii_lowercase();
        if index == 0 {
            id.push_str(&part);
        } else {
            id.push_str(&capitalize(&part));
        }
    }
    id
}

/// `snapshot-cache` → `snapshotCache`; the first part is kept as written.
fn camel_case_resource_id(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .enumerate()
        .map(|(index, part)| {
            if index == 0 {
                part.to_string()
            } else {
                capitalize(part)
            }
        })
        .collect()
}

fn capitalize(part: &str) -> String {
    let mut chars = part.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_ids_from_names() {
        assert_eq!(camel_case_env_name("AUTH_ACCESS_KEY"), "authAccessKey");
        assert_eq!(camel_case_env_name("TOKEN"), "token");
        assert_eq!(camel_case_env_name("_PRIVATE_KEY2"), "privateKey2");
        assert_eq!(camel_case_resource_id("snapshots"), "snapshots");
        assert_eq!(
            camel_case_resource_id("snapshot-cache_v2"),
            "snapshotCacheV2"
        );
    }
}
