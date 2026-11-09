//! Resolving an `alien.toml` document into the `Stack` the TypeScript SDK would produce.
//!
//! Each resource goes through the same steps: sugar rewrites its table, the canonical walker
//! reads it as the JSON of its alien-core type, and serde deserializes that into the type.
//! Every error carries the span of the text it is about, so it is reported at a line and a
//! column, whichever step found it.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use alien_core::stack_authoring::{
    authorable_type_by_table, custom_plugin, normalize_inputs, operations_config,
    stop_grace_period_seconds, validate_dynamic_container_image_resources,
    validate_dynamic_container_repositories, validate_resource_id, AuthorableType,
    AuthoredEnvMapping, AuthoredInput, AuthoredPool, ResourceBodyError, AUTHORABLE_TYPES,
};
use alien_core::{
    CapacityGroup, ComputeCluster, ManagementPermissions, OperationsConfig, PermissionProfile,
    PermissionsConfig, Platform, PluginOperationsConfig, Resource, ResourceEntry,
    ResourceLifecycle, Stack, StackInputEnvironmentVariableType,
};
use alien_error::AlienError;
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde_json::{json, Value as Json};

use crate::canonical::{author_key, Declared, SpanMap, Vocabulary, Walker};
use crate::error::{ErrorData, Result};
use crate::node::{Entry, Node, Table, Value};
use crate::schema::{self, Schema};
use crate::source::SourceFile;
use crate::sugar::{self, Context, Target};
use crate::suggest;

/// ID of the compute cluster holding the pools when the document does not name one. It is
/// the name Alien gives a cluster it creates for a stack that declares none.
pub const DEFAULT_CLUSTER_ID: &str = "compute";

const MAX_RESOURCE_ID_LENGTH: usize = 64;
const MAX_STACK_NAME_LENGTH: usize = 128;

const STACK_KEYS: [&str; 10] = [
    "name",
    "platforms",
    "dynamic-container-repositories",
    "dynamic-container-image-resources",
    "input",
    "cluster",
    "pool",
    "permissions",
    "management-permissions",
    "operations",
];

const CLUSTER_KEYS: [&str; 7] = [
    "id",
    "node-permissions",
    "node-permissions-platforms",
    "container-cidr",
    "display-name",
    "description",
    "lifecycle",
];

/// Resolves `root`, the parsed document, into a stack. `base_dir` is the directory holding
/// alien.toml, which `env-file` paths are relative to.
pub fn resolve(root: Node, file: &SourceFile<'_>, base_dir: &Path) -> Result<Stack> {
    Resolver { file, base_dir }.resolve(&root)
}

struct Resolver<'a> {
    file: &'a SourceFile<'a>,
    base_dir: &'a Path,
}

/// One `[<type>.<id>]` table of the document.
struct Declaration<'n> {
    ty: &'static AuthorableType,
    id: &'n str,
    id_span: Range<usize>,
    node: &'n Node,
}

/// What `[cluster]` and the `[pool.*]` tables make.
struct Cluster {
    id: String,
    entry: ResourceEntry,
}

/// What the keys every resource table accepts say.
struct EntryKeys {
    lifecycle: Option<ResourceLifecycle>,
    enabled_when: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    remote_access: bool,
}

impl Resolver<'_> {
    fn error(&self, span: Range<usize>, message: impl Into<String>) -> AlienError<ErrorData> {
        self.file.error(span, message)
    }

    fn resolve(&self, root: &Node) -> Result<Stack> {
        let root_table = root.as_table().expect("the document is a table");
        self.check_root_keys(root_table)?;

        let name = self.name(root_table)?;
        let platforms = root_table
            .get("platforms")
            .map(|node| self.platform_list(node, None))
            .transpose()?;

        let declarations = self.declarations(root_table)?;
        let cluster_node = self.cluster_node(root_table)?;
        let cluster_id = match &cluster_node {
            Some(node) => Some(self.cluster_id(node)?),
            None => None,
        };

        let mut seen: IndexMap<String, (String, Range<usize>)> = IndexMap::new();
        if let (Some(id), Some(node)) = (&cluster_id, &cluster_node) {
            seen.insert(
                id.clone(),
                (ComputeCluster::RESOURCE_TYPE.to_string(), node.span.clone()),
            );
        }
        for declaration in &declarations {
            self.declare(&mut seen, declaration)?;
        }
        let declared: Declared = seen
            .iter()
            .map(|(id, (resource_type, _))| (id.clone(), resource_type.clone()))
            .collect();

        let pools = self.pools(root_table)?;
        let pool_names: Vec<String> = pools.iter().map(|(name, _, _)| name.clone()).collect();
        let profile_nodes = table_entries(root_table.get("permissions"), "permission profiles")
            .map_err(|(span, message)| self.error(span, message))?;
        let profile_names: Vec<String> = profile_nodes
            .iter()
            .map(|entry| entry.key.clone())
            .collect();
        let input_nodes = table_entries(root_table.get("input"), "inputs")
            .map_err(|(span, message)| self.error(span, message))?;
        let input_ids: Vec<String> = input_nodes.iter().map(|entry| entry.key.clone()).collect();

        let mut context = Context {
            file: self.file,
            base_dir: self.base_dir,
            cluster_id: cluster_id.as_deref(),
            pools: &pool_names,
            profiles: &profile_names,
            inputs: &input_ids,
            env_refs: Vec::new(),
            inline_profiles: Vec::new(),
        };

        let mut resources: IndexMap<String, ResourceEntry> = IndexMap::new();
        for declaration in &declarations {
            let entry = self.resource(declaration, &mut context, &declared, &input_ids)?;
            resources.insert(declaration.id.to_string(), entry);
        }

        let mut permissions = PermissionsConfig::new();
        for entry in profile_nodes {
            self.check_id(
                &entry.key,
                &entry.key_span,
                "permission profile name",
                MAX_RESOURCE_ID_LENGTH,
            )?;
            let profile = self.profile(
                &["permissions".to_string(), entry.key.clone()],
                &entry.value,
                &declared,
            )?;
            permissions.profiles.insert(entry.key.clone(), profile);
        }
        for inline in std::mem::take(&mut context.inline_profiles) {
            let profile = self.profile(
                &["permissions".to_string(), inline.name.clone()],
                &inline.grants,
                &declared,
            )?;
            permissions.profiles.insert(inline.name, profile);
        }
        if let Some(node) = root_table.get("management-permissions") {
            permissions.management = self.management(node, &declared)?;
        }

        let inputs = self.inputs(
            &input_nodes,
            std::mem::take(&mut context.env_refs),
            platforms.as_deref(),
            &declared,
        )?;

        let cluster = match (cluster_id, cluster_node) {
            (Some(id), Some(node)) => Some(self.cluster(id, &node, &pools, &declared)?),
            _ => None,
        };
        if let Some(cluster) = cluster {
            let mut all = IndexMap::new();
            all.insert(cluster.id, cluster.entry);
            all.extend(resources);
            resources = all;
        }

        let mut stack = Stack::new(name).build();
        stack.resources = resources;
        stack.supported_platforms = platforms;
        stack.permissions = permissions;
        stack.inputs = inputs;
        stack.dynamic_container_repositories = self.dynamic_container_repositories(root_table)?;
        stack.dynamic_container_image_resources =
            self.container_ids(root_table, "dynamic-container-image-resources", &declared)?;
        stack.operations = root_table
            .get("operations")
            .map(|node| self.operations(node))
            .transpose()?;
        Ok(stack)
    }

    fn check_root_keys(&self, root: &Table) -> Result<()> {
        let mut allowed: Vec<&str> = STACK_KEYS.to_vec();
        allowed.extend(
            AUTHORABLE_TYPES
                .iter()
                .map(|ty| ty.table.split('.').next().unwrap_or(ty.table)),
        );
        allowed.sort_unstable();
        allowed.dedup();
        for entry in &root.entries {
            let key = entry.key.as_str();
            if key == "cluster" && entry.value.as_str().is_some() {
                return Err(self.error(
                    entry.value.span.clone(),
                    "`cluster` is a table: write `[cluster]` with `id = \"...\"`",
                ));
            }
            if key == "experimental" {
                self.check_experimental(&entry.value)?;
                continue;
            }
            if !allowed.contains(&key) {
                return Err(self.error(
                    entry.key_span.clone(),
                    format!(
                        "unknown key `{key}`{}; expected one of: {}",
                        suggest::hint(key, allowed.iter().copied()),
                        allowed.join(", ")
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_experimental(&self, node: &Node) -> Result<()> {
        let Some(table) = node.as_table() else {
            return Err(self.error(
                node.span.clone(),
                "`experimental` holds tables of experimental resource types, such as [experimental.aws-opensearch.<id>]",
            ));
        };
        let known: Vec<&str> = AUTHORABLE_TYPES
            .iter()
            .filter_map(|ty| ty.table.strip_prefix("experimental."))
            .collect();
        for entry in &table.entries {
            if !known.contains(&entry.key.as_str()) {
                return Err(self.error(
                    entry.key_span.clone(),
                    format!(
                        "unknown experimental resource type `{}`{}; expected one of: {}",
                        entry.key,
                        suggest::hint(&entry.key, known.iter().copied()),
                        known.join(", ")
                    ),
                ));
            }
        }
        Ok(())
    }

    fn name(&self, root: &Table) -> Result<String> {
        let Some(node) = root.get("name") else {
            return Err(self.error(
                root.entries
                    .first()
                    .map_or(0..0, |entry| entry.key_span.clone()),
                "`name` is missing: write `name = \"my-stack\"` at the top of the file",
            ));
        };
        let Some(name) = node.as_str() else {
            return Err(self.error(
                node.span.clone(),
                format!("expected a string, found {}", node.kind()),
            ));
        };
        self.check_id(name, &node.span, "stack name", MAX_STACK_NAME_LENGTH)?;
        Ok(name.to_string())
    }

    fn check_id(&self, id: &str, span: &Range<usize>, what: &str, max_length: usize) -> Result<()> {
        let valid_chars = id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if id.is_empty() || !valid_chars || id.len() > max_length {
            return Err(self.error(
                span.clone(),
                format!(
                    "{what} '{id}' must be 1 to {max_length} letters, digits, hyphens or underscores"
                ),
            ));
        }
        Ok(())
    }

    /// An ID that becomes part of cloud resource names, held to the rule the build preflight
    /// applies so a bad one fails here, at its location, rather than later without one.
    fn check_resource_id(&self, id: &str, span: &Range<usize>, what: &str) -> Result<()> {
        if let Err(reason) = validate_resource_id(id) {
            return Err(self.error(span.clone(), format!("{what} '{id}' {reason}")));
        }
        if id.len() > MAX_RESOURCE_ID_LENGTH {
            return Err(self.error(
                span.clone(),
                format!("{what} '{id}' must be at most {MAX_RESOURCE_ID_LENGTH} characters"),
            ));
        }
        Ok(())
    }

    fn declarations<'n>(&self, root: &'n Table) -> Result<Vec<Declaration<'n>>> {
        let mut declarations = Vec::new();
        for ty in AUTHORABLE_TYPES {
            let mut node = None;
            let mut segments = ty.table.split('.');
            let first = segments.next().expect("a table name is not empty");
            if let Some(found) = root.get(first) {
                node = Some(found);
                for segment in segments {
                    node = node
                        .and_then(Node::as_table)
                        .and_then(|table| table.get(segment));
                }
            }
            let Some(node) = node else {
                continue;
            };
            let Some(table) = node.as_table() else {
                return Err(self.error(
                    node.span.clone(),
                    format!(
                        "expected a table of {} resources: [{}.<id>]",
                        ty.table, ty.table
                    ),
                ));
            };
            for entry in &table.entries {
                declarations.push(Declaration {
                    ty,
                    id: &entry.key,
                    id_span: entry.key_span.clone(),
                    node: &entry.value,
                });
            }
        }
        declarations.sort_by_key(|declaration| declaration.id_span.start);
        Ok(declarations)
    }

    fn declare(
        &self,
        seen: &mut IndexMap<String, (String, Range<usize>)>,
        declaration: &Declaration<'_>,
    ) -> Result<()> {
        let id = declaration.id;
        self.check_resource_id(id, &declaration.id_span, "resource ID")?;
        if let Some((other_type, other_span)) = seen.get(id) {
            if *other_type == ComputeCluster::RESOURCE_TYPE.to_string() {
                return Err(self.error(
                    declaration.id_span.clone(),
                    format!(
                        "'{id}' is the ID of the compute cluster that holds the pools; rename this resource or set a different `id` in [cluster]"
                    ),
                ));
            }
            return Err(self.error(
                declaration.id_span.clone(),
                format!(
                    "'{id}' is already declared as a {other_type} on line {}; resource IDs are shared by every resource type",
                    self.line_of(other_span.start)
                ),
            ));
        }
        seen.insert(
            id.to_string(),
            (
                declaration.ty.resource_type.to_string(),
                declaration.id_span.clone(),
            ),
        );
        Ok(())
    }

    fn line_of(&self, offset: usize) -> usize {
        self.file.text[..offset.min(self.file.text.len())]
            .matches('\n')
            .count()
            + 1
    }

    /// A platform list; with `allowed`, every entry must be one of the stack's platforms.
    fn platform_list(&self, node: &Node, allowed: Option<&[Platform]>) -> Result<Vec<Platform>> {
        let schema =
            json!({ "type": "array", "items": { "$ref": "#/components/schemas/Platform" } });
        let (value, _) = self.walk(&[], node, Schema::inline(&schema), &Declared::new(), None)?;
        let platforms: Vec<Platform> =
            serde_json::from_value(value).expect("the walker checked the platform names");
        if platforms.is_empty() {
            return Err(self.error(
                node.span.clone(),
                "list at least one platform, or leave `platforms` out to allow every platform",
            ));
        }
        let items = node.as_array().expect("the walker checked the list");
        for (index, platform) in platforms.iter().enumerate() {
            let span = items[index].span.clone();
            if platforms[..index].contains(platform) {
                return Err(self.error(span, format!("'{platform}' is listed twice")));
            }
            if let Some(allowed) = allowed.filter(|allowed| !allowed.contains(platform)) {
                let names: Vec<&str> = allowed.iter().map(Platform::as_str).collect();
                return Err(self.error(
                    span,
                    format!(
                        "'{platform}' is not one of the stack's platforms ({})",
                        names.join(", ")
                    ),
                ));
            }
        }
        Ok(platforms)
    }

    fn string_list(&self, root: &Table, key: &str) -> Result<Vec<String>> {
        let Some(node) = root.get(key) else {
            return Ok(Vec::new());
        };
        let schema = json!({ "type": "array", "items": { "type": "string" } });
        let (value, _) = self.walk(&[], node, Schema::inline(&schema), &Declared::new(), None)?;
        Ok(serde_json::from_value(value).expect("the walker checked the strings"))
    }

    fn dynamic_container_repositories(&self, root: &Table) -> Result<Vec<String>> {
        const KEY: &str = "dynamic-container-repositories";
        let repositories = self.string_list(root, KEY)?;
        let Some(node) = root.get(KEY) else {
            return Ok(repositories);
        };
        validate_dynamic_container_repositories(&repositories)
            .map_err(|error| self.list_error(node, error))?;
        Ok(repositories)
    }

    /// An authoring error about the list at `node`, at the entry the error names (`field.2`)
    /// or at the whole list.
    fn list_error(
        &self,
        node: &Node,
        error: AlienError<alien_core::ErrorData>,
    ) -> AlienError<ErrorData> {
        let (field, reason) = definition_error(error);
        let item = field
            .rsplit_once('.')
            .and_then(|(_, index)| index.parse::<usize>().ok())
            .and_then(|index| node.as_array()?.get(index));
        self.error(
            item.map_or(node.span.clone(), |item| item.span.clone()),
            reason,
        )
    }

    fn container_ids(&self, root: &Table, key: &str, declared: &Declared) -> Result<Vec<String>> {
        let Some(node) = root.get(key) else {
            return Ok(Vec::new());
        };
        let Some(items) = node.as_array() else {
            return Err(self.error(
                node.span.clone(),
                format!("expected a list of container IDs, found {}", node.kind()),
            ));
        };
        let containers: Vec<&str> = declared
            .iter()
            .filter(|(_, resource_type)| resource_type.as_str() == "container")
            .map(|(id, _)| id.as_str())
            .collect();
        let mut ids = Vec::new();
        for item in items {
            let Some(id) = item.as_str() else {
                return Err(self.error(
                    item.span.clone(),
                    format!("expected the ID of a container, found {}", item.kind()),
                ));
            };
            if !containers.contains(&id) {
                return Err(self.error(
                    item.span.clone(),
                    format!(
                        "unknown container '{id}'{}; declared containers: {}",
                        suggest::hint(id, containers.iter().copied()),
                        containers.join(", ")
                    ),
                ));
            }
            ids.push(id.to_string());
        }
        validate_dynamic_container_image_resources(&ids)
            .map_err(|error| self.list_error(node, error))?;
        Ok(ids)
    }

    fn walk(
        &self,
        base: &[String],
        node: &Node,
        schema: Schema<'_>,
        declared: &Declared,
        vocabulary: Option<&Vocabulary>,
    ) -> Result<(Json, SpanMap)> {
        let mut walker = Walker::new(declared);
        match walker.walk_at(base, node, schema, vocabulary) {
            Ok(value) => Ok((value, walker.spans)),
            Err(failure) => Err(self.error(failure.span, failure.message)),
        }
    }

    /// Deserializes `value`, an error at the span of the part serde objected to.
    fn typed<T: DeserializeOwned>(
        &self,
        value: Json,
        spans: &SpanMap,
        base: &[String],
        fallback: Range<usize>,
    ) -> Result<T> {
        serde_path_to_error::deserialize::<_, T>(value).map_err(|error| {
            let path = error.path().to_string();
            let path = if path == "." { "" } else { path.as_str() };
            self.body_error(spans, base, path, &error.inner().to_string(), fallback)
        })
    }

    fn body_error(
        &self,
        spans: &SpanMap,
        base: &[String],
        path: &str,
        message: &str,
        fallback: Range<usize>,
    ) -> AlienError<ErrorData> {
        let segments: Vec<String> = path
            .split(['.', '[', ']'])
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect();
        let span = if segments.is_empty() {
            fallback
        } else {
            let mut full = base.to_vec();
            full.extend(segments);
            spans.locate(&full, false).unwrap_or(fallback)
        };
        self.error(span, author_message(message))
    }

    // ---------------------------------------------------------------- resources

    fn resource(
        &self,
        declaration: &Declaration<'_>,
        context: &mut Context<'_>,
        declared: &Declared,
        input_ids: &[String],
    ) -> Result<ResourceEntry> {
        let ty = declaration.ty;
        let id = declaration.id;
        let target = Target {
            ty,
            id,
            id_span: declaration.id_span.clone(),
        };
        let mut node = sugar::rewrite(context, &target, declaration.node.clone())?;
        let table = node.as_table_mut().expect("sugar keeps a table");
        let keys = self.entry_keys(table, input_ids)?;
        if ty.table == "sandbox" {
            sugar::sandbox_session(table);
        }

        let base = vec![ty.table.to_string(), id.to_string()];
        let vocabulary = sugar::vocabulary(ty.table);
        let (mut body, spans) = self.walk(
            &base,
            &node,
            schema::component(ty.schema),
            declared,
            Some(&vocabulary),
        )?;
        body.as_object_mut()
            .expect("a resource is an object")
            .insert("id".to_string(), Json::String(id.to_string()));
        self.check_stop_grace_period(&body, &spans, &base)?;
        let resource: Resource = ty.deserialize(body).map_err(|error: ResourceBodyError| {
            self.body_error(
                &spans,
                &base,
                &error.path,
                &error.message,
                declaration.id_span.clone(),
            )
        })?;

        resource.validate_update(&resource).map_err(|error| {
            let reason = match error.error {
                Some(alien_core::ErrorData::InvalidResourceUpdate { reason, .. }) => reason,
                _ => error.message,
            };
            let named = reason.split(' ').next().unwrap_or_default();
            let mut path = base.clone();
            path.extend(named.split('.').map(str::to_string));
            let span = spans
                .exact(&path)
                .unwrap_or_else(|| declaration.id_span.clone());
            self.error(span, author_message(&reason))
        })?;

        Ok(ResourceEntry {
            display_name: keys.display_name,
            description: keys.description,
            config: resource,
            lifecycle: keys.lifecycle.unwrap_or(ty.default_lifecycle),
            dependencies: Vec::new(),
            remote_access: keys.remote_access,
            enabled_when: keys.enabled_when,
        })
    }

    fn check_stop_grace_period(&self, body: &Json, spans: &SpanMap, base: &[String]) -> Result<()> {
        let Some(seconds) = body.get("stopGracePeriodSeconds").and_then(Json::as_u64) else {
            return Ok(());
        };
        stop_grace_period_seconds("stopGracePeriodSeconds", seconds)
            .map(drop)
            .map_err(|error| {
                let mut path = base.to_vec();
                path.push("stopGracePeriodSeconds".to_string());
                let span = span_at(spans, &path);
                self.error(span, definition_error(error).1)
            })
    }

    fn entry_keys(&self, table: &mut Table, input_ids: &[String]) -> Result<EntryKeys> {
        let text = |node: &Node| -> Result<String> {
            node.as_str().map(str::to_string).ok_or_else(|| {
                self.error(
                    node.span.clone(),
                    format!("expected a string, found {}", node.kind()),
                )
            })
        };
        let lifecycle = match table.remove("lifecycle") {
            None => None,
            Some(entry) => {
                let value = text(&entry.value)?;
                match value.as_str() {
                    "frozen" => Some(ResourceLifecycle::Frozen),
                    "live" => Some(ResourceLifecycle::Live),
                    _ => {
                        return Err(self.error(
                            entry.value.span,
                            format!(
                                "unknown lifecycle '{value}'{}; expected one of: frozen, live",
                                suggest::hint(&value, ["frozen", "live"])
                            ),
                        ))
                    }
                }
            }
        };
        let enabled_when = match table.remove("enabled-when") {
            None => None,
            Some(entry) => {
                let input = text(&entry.value)?;
                if !input_ids.contains(&input) {
                    return Err(self.error(
                        entry.value.span,
                        format!(
                            "unknown input '{input}'{}; declare it with an [input.{input}] table",
                            suggest::hint(&input, input_ids.iter().map(String::as_str))
                        ),
                    ));
                }
                Some(input)
            }
        };
        let display_name = table
            .remove("display-name")
            .map(|entry| text(&entry.value))
            .transpose()?;
        let description = table
            .remove("description")
            .map(|entry| text(&entry.value))
            .transpose()?;
        let remote_access = match table.remove("remote-access") {
            None => false,
            Some(entry) => match entry.value.value {
                Value::Boolean(flag) => flag,
                _ => {
                    return Err(self.error(
                        entry.value.span.clone(),
                        format!("expected true or false, found {}", entry.value.kind()),
                    ))
                }
            },
        };
        Ok(EntryKeys {
            lifecycle,
            enabled_when,
            display_name,
            description,
            remote_access,
        })
    }

    // ---------------------------------------------------------------- cluster

    fn cluster_node(&self, root: &Table) -> Result<Option<Node>> {
        match root.get("cluster") {
            Some(node) if node.as_table().is_some() => Ok(Some(node.clone())),
            Some(node) => Err(self.error(
                node.span.clone(),
                format!("expected a table, found {}", node.kind()),
            )),
            None => {
                let pool = root.entry("pool");
                Ok(pool.map(|entry| Node::table(entry.key_span.clone(), Table::default())))
            }
        }
    }

    fn cluster_id(&self, node: &Node) -> Result<String> {
        let table = node.as_table().expect("checked by cluster_node");
        match table.get("id") {
            None => Ok(DEFAULT_CLUSTER_ID.to_string()),
            Some(id) => {
                let Some(text) = id.as_str() else {
                    return Err(self.error(
                        id.span.clone(),
                        format!("expected a string, found {}", id.kind()),
                    ));
                };
                self.check_resource_id(text, &id.span, "cluster ID")?;
                Ok(text.to_string())
            }
        }
    }

    /// The pools: name, where the name is written, and the table.
    fn pools(&self, root: &Table) -> Result<Vec<(String, Range<usize>, Node)>> {
        let entries = table_entries(root.get("pool"), "pools")
            .map_err(|(span, message)| self.error(span, message))?;
        Ok(entries
            .into_iter()
            .map(|entry| (entry.key, entry.key_span, entry.value))
            .collect())
    }

    fn cluster(
        &self,
        id: String,
        node: &Node,
        pools: &[(String, Range<usize>, Node)],
        declared: &Declared,
    ) -> Result<Cluster> {
        let mut node = node.clone();
        let table = node.as_table_mut().expect("checked by cluster_node");

        for entry in &table.entries {
            let key = entry.key.as_str();
            if !CLUSTER_KEYS.contains(&key) {
                let hidden = match key {
                    "capacity-groups" => Some("pools are [pool.<name>] tables"),
                    "dynamic-container-pool" => Some("set `dynamic-containers = true` on a pool"),
                    "failure-domain-spread" => Some("set `failure-domain-spread` on a pool"),
                    "selected-failure-domains" => Some("setup selects them; they are not written"),
                    _ => None,
                };
                let message = match hidden {
                    Some(hint) => format!("`{key}` is not written in [cluster]: {hint}"),
                    None => format!(
                        "unknown key `{key}`{}; expected one of: {}",
                        suggest::hint(key, CLUSTER_KEYS),
                        CLUSTER_KEYS.join(", ")
                    ),
                };
                return Err(self.error(entry.key_span.clone(), message));
            }
        }
        let span = node.span.clone();
        let table = node.as_table_mut().expect("checked by cluster_node");
        table.insert_missing("id", span.clone(), Node::string(span, id.clone()));
        let keys = self.entry_keys(table, &[])?;
        if keys.enabled_when.is_some() || keys.remote_access {
            unreachable!("the keys were checked above");
        }

        let base = vec!["cluster".to_string()];
        let (mut body, spans) = self.walk(
            &base,
            &node,
            schema::component("ComputeCluster"),
            declared,
            None,
        )?;

        let mut groups: Vec<CapacityGroup> = Vec::new();
        let mut spread = serde_json::Map::new();
        let mut dynamic_pool: Option<String> = None;
        for (name, name_span, pool) in pools {
            self.check_id(name, name_span, "pool name", MAX_RESOURCE_ID_LENGTH)?;
            let (group, pool_spread, dynamic) = self.pool(name, name_span, pool, declared)?;
            if dynamic {
                if let Some(first) = &dynamic_pool {
                    return Err(self.error(
                        name_span.clone(),
                        format!(
                            "only one pool can set `dynamic-containers`; '{first}' already does"
                        ),
                    ));
                }
                dynamic_pool = Some(name.clone());
            }
            if let Some(pool_spread) = pool_spread {
                spread.insert(name.clone(), json!(pool_spread));
            }
            groups.push(group);
        }
        let object = body.as_object_mut().expect("a cluster is an object");
        object.insert(
            "capacityGroups".to_string(),
            serde_json::to_value(&groups).expect("capacity groups serialize"),
        );
        if !spread.is_empty() {
            object.insert("failureDomainSpread".to_string(), Json::Object(spread));
        }
        if let Some(pool) = dynamic_pool {
            object.insert("dynamicContainerPool".to_string(), Json::String(pool));
        }
        let cluster: ComputeCluster = self.typed(body, &spans, &base, node.span.clone())?;
        let mut entry =
            ResourceEntry::new(cluster, keys.lifecycle.unwrap_or(ResourceLifecycle::Frozen));
        entry.display_name = keys.display_name;
        entry.description = keys.description;
        Ok(Cluster { id, entry })
    }

    /// A pool as a capacity group, its failure-domain spread, and whether containers created
    /// after installation use it.
    fn pool(
        &self,
        name: &str,
        name_span: &Range<usize>,
        node: &Node,
        declared: &Declared,
    ) -> Result<(CapacityGroup, Option<u8>, bool)> {
        let mut node = node.clone();
        let Some(table) = node.as_table_mut() else {
            return Err(self.error(
                node.span.clone(),
                format!("expected a table, found {}", node.kind()),
            ));
        };
        let dynamic = match table.remove("dynamic-containers") {
            None => false,
            Some(entry) => match entry.value.value {
                Value::Boolean(flag) => flag,
                _ => {
                    return Err(self.error(
                        entry.value.span.clone(),
                        format!("expected true or false, found {}", entry.value.kind()),
                    ))
                }
            },
        };

        let base = vec!["pool".to_string(), name.to_string()];
        let (body, spans) = self.walk(
            &base,
            &node,
            schema::component("AuthoredPool"),
            declared,
            None,
        )?;
        let authored: AuthoredPool = self.typed(body, &spans, &base, name_span.clone())?;

        let at = |field: &str| -> Range<usize> {
            let path: Vec<&str> = match field {
                "scale.machines" => vec!["machines"],
                "scale.min" => vec!["autoscaling", "min"],
                "scale.max" => vec!["autoscaling", "max"],
                "scale" => vec!["autoscaling"],
                field if field.starts_with("dataDisk") => vec!["dataDisk"],
                field => vec![field],
            };
            let mut full = base.clone();
            full.extend(path.into_iter().map(str::to_string));
            spans.exact(&full).unwrap_or_else(|| name_span.clone())
        };
        let group = authored
            .to_compute_pool()
            .and_then(|pool| pool.to_capacity_group(name))
            .map_err(|error| {
                let (field, reason) = definition_error(error);
                self.error(at(&field), reason)
            })?;
        let spread = authored.failure_domain_spread().map_err(|error| {
            let (field, reason) = definition_error(error);
            self.error(at(&field), reason)
        })?;
        Ok((group, spread, dynamic))
    }

    // ---------------------------------------------------------------- inputs

    fn inputs(
        &self,
        nodes: &[Entry],
        env_refs: Vec<sugar::EnvRef>,
        stack_platforms: Option<&[Platform]>,
        declared: &Declared,
    ) -> Result<Vec<alien_core::StackInputDefinition>> {
        let mut authored: IndexMap<String, AuthoredInput> = IndexMap::new();
        let mut maps: HashMap<String, SpanMap> = HashMap::new();
        for entry in nodes {
            let mut node = entry.value.clone();
            if node.as_table().is_none() {
                return Err(self.error(
                    node.span.clone(),
                    format!("expected a table, found {}", node.kind()),
                ));
            }
            sugar::input(&mut node);
            let base = vec!["input".to_string(), entry.key.clone()];
            let (body, spans) = self.walk(
                &base,
                &node,
                schema::component("AuthoredInput"),
                &Declared::new(),
                None,
            )?;
            let input: AuthoredInput = self.typed(body, &spans, &base, entry.key_span.clone())?;
            if let (Some(platforms), Some(stack)) = (&input.platforms, stack_platforms) {
                if let Some(outside) = platforms.iter().find(|platform| !stack.contains(platform)) {
                    let names: Vec<&str> = stack.iter().map(Platform::as_str).collect();
                    let mut path = base.clone();
                    path.push("platforms".to_string());
                    return Err(self.error(
                        spans
                            .locate(&path, false)
                            .unwrap_or_else(|| entry.key_span.clone()),
                        format!(
                            "'{outside}' is not one of the stack's platforms ({})",
                            names.join(", ")
                        ),
                    ));
                }
            }
            self.check_env_targets(&entry.key, &input, &spans, declared)?;
            authored.insert(entry.key.clone(), input);
            maps.insert(entry.key.clone(), spans);
        }

        for reference in env_refs {
            let input = authored
                .get_mut(&reference.input)
                .expect("sugar checked that the input exists");
            let var_type: Option<StackInputEnvironmentVariableType> = reference
                .var_type
                .map(|text| serde_json::from_value(Json::String(text)))
                .transpose()
                .expect("sugar checked the type");
            let shared = input.env.iter_mut().find(|mapping| {
                mapping.name == reference.name
                    && mapping.var_type == var_type
                    && mapping.target_resources.is_some()
            });
            match shared {
                Some(mapping) => mapping
                    .target_resources
                    .get_or_insert_with(Vec::new)
                    .push(reference.target),
                None => input.env.push(AuthoredEnvMapping {
                    name: reference.name,
                    target_resources: Some(vec![reference.target]),
                    var_type,
                }),
            }
        }

        normalize_inputs(authored).map_err(|error| {
            let (field, reason) = definition_error(error);
            let mut segments = field.split('.').map(str::to_string);
            segments.next();
            let id = segments.next().unwrap_or_default();
            let mut path = vec!["input".to_string(), id.clone()];
            path.extend(segments);
            let entry = nodes.iter().find(|entry| entry.key == id);
            let span = maps
                .get(&id)
                .filter(|_| path.len() > 2)
                .and_then(|spans| spans.exact(&path))
                .or_else(|| entry.map(|entry| entry.key_span.clone()))
                .or_else(|| nodes.first().map(|first| first.key_span.clone()))
                .expect("inputs are only normalized when the document declares one");
            self.error(span, reason)
        })
    }

    fn check_env_targets(
        &self,
        id: &str,
        input: &AuthoredInput,
        spans: &SpanMap,
        declared: &Declared,
    ) -> Result<()> {
        for (index, mapping) in input.env.iter().enumerate() {
            let Some(targets) = &mapping.target_resources else {
                continue;
            };
            let at = |target: Option<usize>| {
                let mut path = vec![
                    "input".to_string(),
                    id.to_string(),
                    "env".to_string(),
                    index.to_string(),
                    "targetResources".to_string(),
                ];
                path.extend(target.map(|target| target.to_string()));
                span_at(spans, &path)
            };
            if targets.is_empty() {
                return Err(self.error(
                    at(None),
                    "`target-resources` is empty; leave it out to set the variable on every workload",
                ));
            }
            for (position, target) in targets.iter().enumerate() {
                if target.ends_with('*') {
                    continue;
                }
                match declared.get(target) {
                    Some(resource_type) if is_workload(resource_type) => {}
                    Some(resource_type) => {
                        return Err(self.error(
                            at(Some(position)),
                            format!(
                            "'{target}' is a {resource_type}, which takes no environment variables"
                        ),
                        ))
                    }
                    None => {
                        return Err(self.error(
                            at(Some(position)),
                            format!(
                                "unknown resource '{target}' in `target-resources`{}",
                                suggest::hint(target, declared.keys().map(String::as_str))
                            ),
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- permissions

    fn profile(
        &self,
        base: &[String],
        node: &Node,
        declared: &Declared,
    ) -> Result<PermissionProfile> {
        let Some(table) = node.as_table() else {
            return Err(self.error(
                node.span.clone(),
                format!(
                    "expected a table of grants, such as `{{ <resource-id> = [\"storage/data-read\"] }}`, found {}",
                    node.kind()
                ),
            ));
        };
        for entry in &table.entries {
            if entry.key != "*" && !declared.contains_key(&entry.key) {
                return Err(self.error(
                    entry.key_span.clone(),
                    format!(
                        "unknown resource '{}'{}; use a declared resource ID, or \"*\" for every resource",
                        entry.key,
                        suggest::hint(&entry.key, declared.keys().map(String::as_str))
                    ),
                ));
            }
            if let Some(sets) = entry.value.as_array() {
                for set in sets {
                    if set.as_str() == Some("") {
                        return Err(
                            self.error(set.span.clone(), "a permission set name cannot be empty")
                        );
                    }
                }
            }
        }
        let (body, spans) = self.walk(
            base,
            node,
            schema::component("PermissionProfile"),
            declared,
            None,
        )?;
        self.typed(body, &spans, base, node.span.clone())
    }

    fn management(&self, node: &Node, declared: &Declared) -> Result<ManagementPermissions> {
        if let Some(table) = node.as_table() {
            for entry in &table.entries {
                if matches!(entry.key.as_str(), "extend" | "override") {
                    let base = vec!["management-permissions".to_string(), entry.key.clone()];
                    self.profile(&base, &entry.value, declared)?;
                }
            }
        }
        let base = vec!["management-permissions".to_string()];
        let (body, spans) = self.walk(
            &base,
            node,
            schema::component("ManagementPermissions"),
            declared,
            None,
        )?;
        self.typed(body, &spans, &base, node.span.clone())
    }

    // ---------------------------------------------------------------- operations

    fn operations(&self, node: &Node) -> Result<OperationsConfig> {
        let Some(table) = node.as_table() else {
            return Err(self.error(
                node.span.clone(),
                format!("expected a table of plugins, found {}", node.kind()),
            ));
        };
        let mut plugins: IndexMap<String, PluginOperationsConfig> = IndexMap::new();
        let mut custom = Vec::new();
        for entry in &table.entries {
            if entry.key != "custom" {
                let base = vec!["operations".to_string(), entry.key.clone()];
                plugins.insert(entry.key.clone(), self.plugin(&base, &entry.value, &[])?);
                continue;
            }
            let Some(items) = entry.value.as_array() else {
                return Err(self.error(
                    entry.value.span.clone(),
                    "write custom plugins as [[operations.custom]] tables, or as a list of \"name@version\"",
                ));
            };
            for (index, item) in items.iter().enumerate() {
                match &item.value {
                    Value::String(spec) => custom.push(custom_plugin(spec).map_err(|error| {
                        self.error(item.span.clone(), definition_error(error).1)
                    })?),
                    Value::Table(fields) => {
                        let text = |key: &str| -> Result<String> {
                            let Some(value) = fields.get(key) else {
                                return Err(self.error(
                                    item.span.clone(),
                                    format!("a custom plugin needs `name` and `version`; `{key}` is missing"),
                                ));
                            };
                            value.as_str().map(str::to_string).ok_or_else(|| {
                                self.error(
                                    value.span.clone(),
                                    format!("expected a string, found {}", value.kind()),
                                )
                            })
                        };
                        let name = text("name")?;
                        let version = text("version")?;
                        let base = vec![
                            "operations".to_string(),
                            "custom".to_string(),
                            index.to_string(),
                        ];
                        let config = self.plugin(&base, &item.clone(), &["name", "version"])?;
                        custom.push(alien_core::CustomPluginOperationsConfig {
                            name,
                            version,
                            config,
                        });
                    }
                    _ => {
                        return Err(self.error(
                            item.span.clone(),
                            format!(
                                "expected a table or \"name@version\", found {}",
                                item.kind()
                            ),
                        ))
                    }
                }
            }
        }
        operations_config(plugins, custom)
            .map_err(|error| self.error(node.span.clone(), definition_error(error).1))
    }

    /// A plugin's table: `approval` holds its approval rules, every other key is a setting.
    fn plugin(
        &self,
        base: &[String],
        node: &Node,
        skip: &[&str],
    ) -> Result<PluginOperationsConfig> {
        let Some(table) = node.as_table() else {
            return Err(self.error(
                node.span.clone(),
                format!("expected a table, found {}", node.kind()),
            ));
        };
        let mut settings = Table::default();
        let mut written = Table::default();
        for entry in &table.entries {
            if skip.contains(&entry.key.as_str()) {
                continue;
            }
            if entry.key == "approval" {
                written.entries.push(entry.clone());
            } else if entry.key == "settings" {
                return Err(self.error(
                    entry.key_span.clone(),
                    "a plugin's settings are written directly in its table, next to `approval`",
                ));
            } else {
                settings.entries.push(entry.clone());
            }
        }
        written.insert(
            "settings",
            node.span.clone(),
            Node::table(node.span.clone(), settings),
        );
        let synthetic = Node::table(node.span.clone(), written);
        let (body, spans) = self.walk(
            base,
            &synthetic,
            schema::component("PluginOperationsConfig"),
            &Declared::new(),
            None,
        )?;
        self.typed(body, &spans, base, node.span.clone())
    }
}

fn is_workload(resource_type: &str) -> bool {
    sugar::WORKLOADS.iter().any(|table| {
        authorable_type_by_table(table).is_some_and(|ty| ty.resource_type == resource_type)
    })
}

/// Where the part of a resource at `path` was written, or the closest enclosing part. The
/// walker records the span of the node it starts at, so some ancestor always has one.
fn span_at(spans: &SpanMap, path: &[String]) -> Range<usize> {
    spans
        .locate(path, false)
        .expect("the walker records the span of the node it starts at")
}

/// The tables under a top-level key such as `pool` or `input`, in file order.
fn table_entries(
    node: Option<&Node>,
    what: &str,
) -> std::result::Result<Vec<Entry>, (Range<usize>, String)> {
    let Some(node) = node else {
        return Ok(Vec::new());
    };
    let Some(table) = node.as_table() else {
        return Err((
            node.span.clone(),
            format!("expected a table of {what}, found {}", node.kind()),
        ));
    };
    Ok(table.entries.clone())
}

/// The field and reason of an authoring error from `alien_core::stack_authoring`.
pub(crate) fn definition_error(error: AlienError<alien_core::ErrorData>) -> (String, String) {
    match error.error {
        Some(alien_core::ErrorData::StackDefinitionInvalid { field, reason }) => (field, reason),
        _ => (String::new(), error.message),
    }
}

/// A serde message with the names it quotes in the author's spelling: `memoryMb` is `memory`.
fn author_message(message: &str) -> String {
    message
        .split('`')
        .enumerate()
        .map(|(index, part)| {
            let property = index % 2 == 1
                && part.starts_with(|c: char| c.is_ascii_lowercase())
                && part.chars().any(|c| c.is_ascii_uppercase())
                && part.chars().all(|c| c.is_ascii_alphanumeric());
            if property {
                author_key(part)
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("`")
}
