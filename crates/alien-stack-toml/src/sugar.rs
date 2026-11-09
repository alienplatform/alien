//! The shorthand `alien.toml` allows on top of alien-core's types.
//!
//! Each rewrite turns the table the author wrote into the table the canonical walker reads
//! (alien-core's own shape, with kebab-case keys). The list is closed and kind-specific:
//! anything not named here is read as it stands. Where sugar covers a field, the field's
//! alien-core spelling is rejected, so there is one way to write each thing.
//!
//! A node a rewrite makes up carries the span of the node it came from, so a later error
//! still points at something the author wrote.

use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use alien_core::instance_catalog;
use alien_core::stack_authoring::{memory_resource_spec, AuthorableType};
use alien_core::DaemonRuntime;
use serde_json::Value as Json;

use crate::canonical::{allowed_keys, Declared, Vocabulary};
use crate::env_file;
use crate::error::Result;
use crate::node::{Entry, Node, Table, Value};
use crate::resolve::definition_error;
use crate::schema::{self, camel_to_kebab};
use crate::source::SourceFile;
use crate::suggest;

/// What a resource table may hold besides the fields of its type.
pub const ENTRY_KEYS: [&str; 5] = [
    "lifecycle",
    "enabled-when",
    "display-name",
    "description",
    "remote-access",
];

pub const WORKLOADS: [&str; 4] = ["container", "daemon", "worker", "build-environment"];
const WITH_CODE: [&str; 4] = ["container", "daemon", "worker", "sandbox"];

/// What the sugar of one resource needs to know about the rest of the document.
pub struct Context<'a> {
    pub file: &'a SourceFile<'a>,
    pub base_dir: &'a Path,
    pub cluster_id: Option<&'a str>,
    pub pools: &'a [String],
    pub profiles: &'a [String],
    pub inputs: &'a [String],
    pub resources: &'a Declared,
    pub env_refs: Vec<EnvRef>,
    pub inline_profiles: Vec<InlineProfile>,
}

/// `NAME = { input = "<id>" }` in a workload's `env`.
pub struct EnvRef {
    pub input: String,
    pub name: String,
    pub target: String,
    pub var_type: Option<String>,
    /// The `{ input = ".." }` value, which errors about the reference point at.
    pub span: Range<usize>,
}

/// Grants written on a workload, which make a profile named after it.
pub struct InlineProfile {
    pub name: String,
    pub grants: Node,
}

/// The resource a rewrite is for.
pub struct Target<'a> {
    pub ty: &'a AuthorableType,
    pub id: &'a str,
    pub id_span: Range<usize>,
}

impl Context<'_> {
    fn error(
        &self,
        span: Range<usize>,
        message: impl Into<String>,
    ) -> alien_error::AlienError<crate::ErrorData> {
        self.file.error(span, message)
    }
}

/// The keys the author may write at the top of a table of this kind, beyond its fields.
pub fn vocabulary(kind: &str) -> Vocabulary {
    let mut hidden: Vec<&'static str> = Vec::new();
    let mut extra: Vec<&'static str> = ENTRY_KEYS.to_vec();
    if WORKLOADS.contains(&kind) {
        hidden.push("environment");
        extra.extend(["env", "env-file"]);
    }
    if WITH_CODE.contains(&kind) {
        hidden.push("code");
        extra.extend(["image", "build"]);
    }
    match kind {
        "container" => hidden.extend(["resource-choices", "cluster", "stateful"]),
        "daemon" => {
            hidden.push("cluster");
            extra.push("host-access");
        }
        "sandbox" => extra.push("session"),
        _ => {}
    }
    Vocabulary { hidden, extra }
}

/// Every key the author may write in a table of this resource type.
pub fn settings(ty: &AuthorableType) -> Vec<String> {
    let schema = schema::component(ty.schema);
    let properties = schema.properties().expect("a resource type has properties");
    allowed_keys(
        properties.keys().map(String::as_str),
        Some(&vocabulary(ty.table)),
    )
}

/// Rewrites the table of one resource into the table the walker reads.
pub fn rewrite(ctx: &mut Context<'_>, target: &Target<'_>, mut node: Node) -> Result<Node> {
    let table_span = node.span.clone();
    let Value::Table(table) = &mut node.value else {
        return Err(ctx.error(
            node.span.clone(),
            format!("expected a table, found {}", node.kind()),
        ));
    };
    let kind = target.ty.table;

    reject_canonical(ctx, table, kind)?;

    if WITH_CODE.contains(&kind) {
        code(ctx, target, table)?;
    }
    if kind == "container" || kind == "daemon" {
        resource_specs(ctx, table, kind == "container")?;
        pool(ctx, table)?;
        if let Some(cluster) = ctx.cluster_id {
            table.insert(
                "cluster",
                table_span.clone(),
                Node::string(table_span.clone(), cluster),
            );
        }
    }
    match kind {
        "container" => container(ctx, table)?,
        "daemon" => daemon(ctx, table)?,
        "worker" => worker(ctx, table)?,
        "storage" => storage(table),
        "email" => email(table),
        _ => {}
    }
    if WORKLOADS.contains(&kind) {
        links(ctx, target, table)?;
        environment(ctx, target, table)?;
        permissions(ctx, target, table)?;
    }
    Ok(node)
}

fn reject_canonical(ctx: &Context<'_>, table: &Table, kind: &str) -> Result<()> {
    let mut rules: Vec<(&str, &str)> = Vec::new();
    if WITH_CODE.contains(&kind) {
        rules.push((
            "code",
            "write `image = \"<reference>\"` or `build = { toolchain = \"rust\", binary-name = \"<binary>\" }`",
        ));
    }
    if WORKLOADS.contains(&kind) {
        rules.push((
            "environment",
            "write `env`, whose values are strings, numbers or `{ input = \"<id>\" }`",
        ));
    }
    if kind == "container" {
        rules.push((
            "resource-choices",
            "write a range on `cpu` or `memory`: `cpu = { default = 1, allowed = [0.5, 2] }`",
        ));
        rules.push(("stateful", "it follows from `persistent-storage`"));
    }
    if kind == "container" || kind == "daemon" {
        rules.push((
            "cluster",
            "the cluster is the [cluster] table, and containers and daemons join it",
        ));
    }
    for (key, hint) in rules {
        if let Some(entry) = table.entry(key) {
            return Err(ctx.error(
                entry.key_span.clone(),
                format!("`{key}` is not written in alien.toml: {hint}"),
            ));
        }
    }
    Ok(())
}

/// The text of a string or a number, which is how the stack spells a cpu or a size.
fn text_of(node: &Node) -> Option<String> {
    match &node.value {
        Value::String(text) => Some(text.clone()),
        _ => node.number_text(),
    }
}

fn stringified(node: &Node) -> Node {
    match text_of(node) {
        Some(text) if node.as_str().is_none() => Node::string(node.span.clone(), text),
        _ => node.clone(),
    }
}

fn spec(span: Range<usize>, text: &str) -> Node {
    let mut table = Table::default();
    for key in ["min", "desired"] {
        table.insert(key, span.clone(), Node::string(span.clone(), text));
    }
    Node::table(span, table)
}

fn code(ctx: &Context<'_>, target: &Target<'_>, table: &mut Table) -> Result<()> {
    let kind = target.ty.table;
    let code = match (table.remove("image"), table.remove("build")) {
        (Some(image), None) => {
            let mut code = Table::default();
            let span = image.value.span.clone();
            code.insert("type", image.key_span.clone(), Node::string(span.clone(), "image"));
            code.insert("image", image.key_span.clone(), image.value);
            Entry {
                key: "code".into(),
                key_span: image.key_span,
                value: Node::table(span, code),
            }
        }
        (None, Some(build)) => source(ctx, build)?,
        (Some(image), Some(_)) => {
            return Err(ctx.error(
                image.value.span,
                "set either `build` (build from source) or `image` (a prebuilt image), not both",
            ))
        }
        (None, None) => {
            return Err(ctx.error(
                target.id_span.clone(),
                format!(
                    "{kind} needs `build = {{ toolchain = \"rust\", binary-name = \"<binary>\" }}` or `image = \"<reference>\"`"
                ),
            ))
        }
    };
    table.insert("code", code.key_span, code.value);
    Ok(())
}

fn source(ctx: &Context<'_>, build: Entry) -> Result<Entry> {
    let span = build.value.span.clone();
    let Value::Table(mut fields) = build.value.value else {
        return Err(ctx.error(
            span,
            "expected a table such as { toolchain = \"rust\", binary-name = \"app\" }",
        ));
    };
    let Some(toolchain) = fields.remove("toolchain") else {
        return Err(ctx.error(
            span,
            "name the toolchain: `build = { toolchain = \"rust\", binary-name = \"<binary>\" }`; one of rust, typescript, python, docker",
        ));
    };
    let tags: Vec<String> = schema::component("ToolchainConfig")
        .one_of()
        .expect("toolchains are a union")
        .iter()
        .filter_map(|branch| branch.tag())
        .map(camel_to_kebab)
        .collect();
    let Some(name) = toolchain.value.as_str() else {
        return Err(ctx.error(
            toolchain.value.span.clone(),
            format!("`toolchain` must be a string, one of: {}", tags.join(", ")),
        ));
    };
    if !tags.iter().any(|tag| tag == name) {
        let advice = if name == "go" {
            "; a Go build uses `toolchain = \"docker\"`".to_string()
        } else {
            suggest::hint(name, tags.iter().map(String::as_str))
        };
        return Err(ctx.error(
            toolchain.value.span.clone(),
            format!(
                "unknown toolchain '{name}'{advice}; expected one of: {}",
                tags.join(", ")
            ),
        ));
    }

    check_toolchain_keys(ctx, name, &fields)?;

    let mut src = fields.remove("src").unwrap_or_else(|| Entry {
        key: "src".into(),
        key_span: span.clone(),
        value: Node::string(span.clone(), "."),
    });
    // The build reads `src` relative to the working directory, but alien.toml documents it
    // relative to the file, so a config in a subdirectory names the directory next to it.
    if let Value::String(path) = &mut src.value.value {
        *path = relative_to_file(ctx.base_dir, path);
    }
    let mut toolchain_table = Table::default();
    toolchain_table.insert("type", toolchain.key_span.clone(), toolchain.value);
    toolchain_table.entries.extend(fields.entries);

    let mut code = Table::default();
    code.insert(
        "type",
        build.key_span.clone(),
        Node::string(span.clone(), "source"),
    );
    code.insert("src", src.key_span, src.value);
    code.insert(
        "toolchain",
        toolchain.key_span,
        Node::table(span.clone(), toolchain_table),
    );
    Ok(Entry {
        key: "code".into(),
        key_span: build.key_span,
        value: Node::table(span, code),
    })
}

/// `path` as a path from the directory of alien.toml (`base_dir`), written the way the
/// working directory sees it: `.` when they are the same directory, no `./` or `..` noise.
fn relative_to_file(base_dir: &Path, path: &str) -> String {
    let mut parts: Vec<Component<'_>> = Vec::new();
    let full = base_dir.join(path);
    for component in full.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(parts.last(), Some(Component::Normal(_))) => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let joined: PathBuf = parts.iter().collect();
    if joined.as_os_str().is_empty() {
        ".".to_string()
    } else {
        joined.display().to_string()
    }
}

fn check_toolchain_keys(ctx: &Context<'_>, name: &str, fields: &Table) -> Result<()> {
    let branches = schema::component("ToolchainConfig")
        .one_of()
        .expect("toolchains are a union");
    let properties = |tag: &str| -> Vec<String> {
        branches
            .iter()
            .find(|branch| branch.tag().map(camel_to_kebab).as_deref() == Some(tag))
            .and_then(|branch| branch.resolved().properties())
            .map(|properties| {
                properties
                    .keys()
                    .filter(|key| key.as_str() != "type")
                    .map(|key| camel_to_kebab(key))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut allowed = vec!["toolchain".to_string(), "src".to_string()];
    allowed.extend(properties(name));
    for entry in &fields.entries {
        if allowed.contains(&entry.key) {
            continue;
        }
        let other_toolchain = branches
            .iter()
            .filter_map(|branch| branch.tag().map(camel_to_kebab))
            .find(|tag| properties(tag).contains(&entry.key));
        let message = match other_toolchain {
            Some(tag) => format!("`{}` applies to {tag} builds only, not {name}", entry.key),
            None => format!(
                "unknown key `{}`{}; a {name} build takes: {}",
                entry.key,
                suggest::hint(&entry.key, allowed.iter().map(String::as_str)),
                allowed.join(", ")
            ),
        };
        return Err(ctx.error(entry.key_span.clone(), message));
    }
    Ok(())
}

/// `cpu = 2` and `memory = "4Gi"` become a `{ min, desired }` pair; on a container a range
/// (`{ default = 1, allowed = [0.5, 2] }`) also becomes the choices a deployer may make.
fn resource_specs(ctx: &Context<'_>, table: &mut Table, ranges: bool) -> Result<()> {
    let mut choices = Table::default();
    for field in ["cpu", "memory"] {
        let Some(entry) = table.remove(field) else {
            continue;
        };
        let span = entry.value.span.clone();
        let text = match &entry.value.value {
            Value::Table(range) if ranges => {
                let Some(default) = range.get("default") else {
                    return Err(ctx.error(
                        span,
                        "a range needs `default`, the value used when none is chosen",
                    ));
                };
                let text = text_of(default).ok_or_else(|| {
                    ctx.error(
                        default.span.clone(),
                        format!("expected a number or a size, found {}", default.kind()),
                    )
                })?;
                let mut written = Table::default();
                for item in &range.entries {
                    let value = match &item.value.value {
                        Value::Array(bounds) => Node::array(
                            item.value.span.clone(),
                            bounds.iter().map(stringified).collect(),
                        ),
                        _ => stringified(&item.value),
                    };
                    written.insert(&item.key, item.key_span.clone(), value);
                }
                choices.insert(
                    field,
                    entry.key_span.clone(),
                    Node::table(span.clone(), written),
                );
                text
            }
            Value::Table(_) => {
                return Err(ctx.error(
                    span,
                    format!("a daemon's `{field}` is one value, such as `{field} = 1`"),
                ))
            }
            _ => text_of(&entry.value).ok_or_else(|| {
                ctx.error(
                    span.clone(),
                    format!("expected a number or a size, found {}", entry.value.kind()),
                )
            })?,
        };
        check_resource_spec(ctx, field, &text, &span)?;
        table.insert(field, entry.key_span, spec(span, &text));
    }
    if !choices.entries.is_empty() {
        let span = choices.entries[0].value.span.clone();
        let key_span = choices.entries[0].key_span.clone();
        table.insert("resource-choices", key_span, Node::table(span, choices));
    }
    Ok(())
}

fn check_resource_spec(
    ctx: &Context<'_>,
    field: &str,
    text: &str,
    span: &Range<usize>,
) -> Result<()> {
    if field == "memory" {
        return memory_resource_spec(field, text)
            .map(drop)
            .map_err(|error| ctx.error(span.clone(), definition_error(error).1));
    }
    match instance_catalog::parse_cpu(text) {
        Ok(cores) if text.trim() == text && cores.is_finite() && cores > 0.0 => Ok(()),
        _ => Err(ctx.error(
            span.clone(),
            format!("'{text}' is not an amount of CPU; write a number above zero, such as 2 or 0.5, or millicores such as \"500m\""),
        )),
    }
}

fn pool(ctx: &Context<'_>, table: &Table) -> Result<()> {
    let Some(pool) = table.get("pool") else {
        return Ok(());
    };
    let Some(name) = pool.as_str() else {
        return Ok(());
    };
    if ctx.pools.iter().any(|declared| declared == name) {
        return Ok(());
    }
    let message = if ctx.pools.is_empty() {
        format!("unknown pool '{name}'; declare it with a [pool.{name}] table")
    } else {
        format!(
            "unknown pool '{name}'{}; declared pools: {}",
            suggest::hint(name, ctx.pools.iter().map(String::as_str)),
            ctx.pools.join(", ")
        )
    };
    Err(ctx.error(pool.span.clone(), message))
}

fn container(ctx: &Context<'_>, table: &mut Table) -> Result<()> {
    if let Some(ports) = table.get_mut("ports") {
        if let Value::Array(items) = &mut ports.value {
            for item in items {
                if let Value::Integer(port) = item.value {
                    *item = port_table(item.span.clone(), port);
                }
            }
        }
    }
    endpoints(ctx, table, "container")?;

    if let Some(tunnel) = table.get_mut("tunnel") {
        if let Value::Integer(port) = tunnel.value {
            *tunnel = port_table(tunnel.span.clone(), port);
        }
        if let Some(port) = tunnel.as_table().and_then(|t| integer(t.get("port"))) {
            let span = tunnel.span.clone();
            add_port(table, port, span);
        }
    }

    if let Some(entry) = table.remove("persistent-storage") {
        let span = entry.value.span.clone();
        let mut storage = match entry.value.value {
            Value::String(size) => {
                let mut storage = Table::default();
                storage.insert(
                    "size",
                    entry.key_span.clone(),
                    Node::string(span.clone(), size),
                );
                storage
            }
            Value::Table(storage) => storage,
            _ => {
                return Err(ctx.error(
                    span,
                    format!(
                        "expected a size such as \"10Gi\" or a table, found {}",
                        entry.value.kind()
                    ),
                ))
            }
        };
        storage.insert_missing(
            "mount-path",
            span.clone(),
            Node::string(span.clone(), "/data"),
        );
        let backups = storage.remove("backups");
        if let Some(backups) = backups {
            let backup_span = backups.value.span.clone();
            let mut written = match backups.value.value {
                Value::Boolean(enabled) => {
                    let mut written = Table::default();
                    written.insert("enabled", backups.key_span.clone(), Node::boolean(backup_span.clone(), enabled));
                    written
                }
                Value::Table(written) => written,
                _ => {
                    return Err(ctx.error(
                        backup_span,
                        format!(
                            "expected true, false or a table such as {{ interval = \"12h\" }}, found {}",
                            backups.value.kind()
                        ),
                    ))
                }
            };
            written.insert_missing(
                "enabled",
                backup_span.clone(),
                Node::boolean(backup_span.clone(), true),
            );
            written.insert_missing(
                "interval",
                backup_span.clone(),
                Node::integer(backup_span.clone(), 24),
            );
            written.insert_missing(
                "retention",
                backup_span.clone(),
                Node::integer(backup_span.clone(), 7),
            );
            storage.insert(
                "backups",
                backups.key_span,
                Node::table(backup_span, written),
            );
        }
        table.insert(
            "persistent-storage",
            entry.key_span.clone(),
            Node::table(span.clone(), storage),
        );
        table.insert_missing("stateful", entry.key_span, Node::boolean(span, true));
    }

    if let Some(autoscaling) = table.get_mut("autoscaling") {
        let span = autoscaling.span.clone();
        if let Value::Table(scale) = &mut autoscaling.value {
            if let Some(min) = scale.get("min").cloned() {
                scale.insert_missing("desired", span, min);
            }
        }
    }
    Ok(())
}

fn integer(node: Option<&Node>) -> Option<i64> {
    match node?.value {
        Value::Integer(number) => Some(number),
        _ => None,
    }
}

fn port_table(span: Range<usize>, port: i64) -> Node {
    let mut table = Table::default();
    table.insert("port", span.clone(), Node::integer(span.clone(), port));
    Node::table(span, table)
}

/// Makes sure a container's `ports` lists `port`, which a public endpoint or a tunnel needs.
fn add_port(table: &mut Table, port: i64, span: Range<usize>) {
    if table.get("ports").is_none() {
        table.insert("ports", span.clone(), Node::array(span.clone(), Vec::new()));
    }
    if let Some(Value::Array(items)) = table.get_mut("ports").map(|ports| &mut ports.value) {
        let listed = items
            .iter()
            .any(|item| item.as_table().and_then(|t| integer(t.get("port"))) == Some(port));
        if !listed {
            items.push(port_table(span, port));
        }
    }
}

/// `public-endpoints` keyed by name becomes the list of endpoints that carry their name.
fn endpoints(ctx: &Context<'_>, table: &mut Table, kind: &str) -> Result<()> {
    let Some(entry) = table.remove("public-endpoints") else {
        return Ok(());
    };
    if matches!(entry.value.value, Value::Array(_)) && kind != "worker" {
        return Err(ctx.error(
            entry.value.span,
            "write the endpoints as a table keyed by name: [container.<id>.public-endpoints.<name>]",
        ));
    }
    let span = entry.value.span.clone();
    let named: Vec<(String, Range<usize>, Node)> = match entry.value.value {
        Value::Array(names) if kind == "worker" => names
            .into_iter()
            .map(|name| match &name.value {
                Value::String(text) => {
                    let text = text.clone();
                    Ok((
                        text,
                        name.span.clone(),
                        Node::table(name.span, Table::default()),
                    ))
                }
                _ => Err(ctx.error(
                    name.span.clone(),
                    format!("expected an endpoint name, found {}", name.kind()),
                )),
            })
            .collect::<Result<_>>()?,
        Value::Table(endpoints) => endpoints
            .entries
            .into_iter()
            .map(|endpoint| (endpoint.key, endpoint.key_span, endpoint.value))
            .collect(),
        _ => {
            return Err(ctx.error(
                span,
                format!(
                    "expected a table keyed by endpoint name, found {}",
                    entry.value.kind()
                ),
            ))
        }
    };

    let mut list = Vec::new();
    for (name, name_span, endpoint) in named {
        let endpoint_span = endpoint.span.clone();
        let Value::Table(mut fields) = endpoint.value else {
            return Err(ctx.error(
                endpoint_span,
                format!(
                    "expected a table such as {{ port = 8080 }}, found {}",
                    endpoint.kind()
                ),
            ));
        };
        if let Some(duplicate) = fields.entry("name") {
            return Err(ctx.error(
                duplicate.key_span.clone(),
                "the endpoint's name is the key of its table; remove `name`",
            ));
        }
        if kind != "worker" {
            fields.insert_missing(
                "protocol",
                endpoint_span.clone(),
                Node::string(endpoint_span.clone(), "http"),
            );
            if let Some(port) = integer(fields.get("port")).filter(|_| kind == "container") {
                add_port(table, port, endpoint_span.clone());
            }
        }
        let mut endpoint = Table::default();
        endpoint.insert("name", name_span.clone(), Node::string(name_span, name));
        endpoint.entries.extend(fields.entries);
        list.push(Node::table(endpoint_span, endpoint));
    }
    table.insert("public-endpoints", entry.key_span, Node::array(span, list));
    Ok(())
}

fn daemon(ctx: &Context<'_>, table: &mut Table) -> Result<()> {
    endpoints(ctx, table, "daemon")?;
    let Some(host_access) = table.remove("host-access") else {
        return Ok(());
    };
    let Value::Boolean(enabled) = host_access.value.value else {
        return Err(ctx.error(
            host_access.value.span.clone(),
            format!("expected true or false, found {}", host_access.value.kind()),
        ));
    };
    if let Some(runtime) = table.entry("runtime") {
        return Err(ctx.error(
            runtime.key_span.clone(),
            "`runtime` cannot be combined with `host-access`, which already sets the runtime; drop one",
        ));
    }
    if enabled {
        let runtime =
            serde_json::to_value(DaemonRuntime::host_access()).expect("a runtime serializes");
        table.insert(
            "runtime",
            host_access.key_span,
            from_json(&runtime, host_access.value.span),
        );
    }
    Ok(())
}

/// A JSON value as a node, with the keys written the way an author writes them.
fn from_json(value: &Json, span: Range<usize>) -> Node {
    match value {
        Json::String(text) => Node::string(span, text.clone()),
        Json::Bool(flag) => Node::boolean(span, *flag),
        Json::Number(number) => Node::integer(span, number.as_i64().expect("a whole number")),
        Json::Array(items) => Node::array(
            span.clone(),
            items
                .iter()
                .map(|item| from_json(item, span.clone()))
                .collect(),
        ),
        Json::Object(fields) => {
            let mut table = Table::default();
            for (key, value) in fields {
                table.insert(
                    &camel_to_kebab(key),
                    span.clone(),
                    from_json(value, span.clone()),
                );
            }
            Node::table(span, table)
        }
        Json::Null => unreachable!("a runtime has no nulls"),
    }
}

fn worker(ctx: &Context<'_>, table: &mut Table) -> Result<()> {
    endpoints(ctx, table, "worker")?;
    let Some(triggers) = table.get_mut("triggers") else {
        return Ok(());
    };
    let Value::Array(items) = &mut triggers.value else {
        return Ok(());
    };
    for item in items {
        let span = item.span.clone();
        let Value::Table(trigger) = &mut item.value else {
            continue;
        };
        if let Some(tag) = trigger.entry("type") {
            return Err(ctx.error(
                tag.key_span.clone(),
                "a trigger is written `{ queue = \"<id>\" }`, `{ storage = \"<id>\" }` or `{ schedule = \"<cron>\" }`; `type` is implied",
            ));
        }
        if let Some(cron) = trigger.entry("cron") {
            return Err(ctx.error(
                cron.key_span.clone(),
                "write the cron expression as `schedule`",
            ));
        }
        let tag = ["queue", "storage", "schedule"]
            .into_iter()
            .find(|tag| trigger.contains(tag));
        let Some(tag) = tag else {
            return Err(ctx.error(span, "a trigger needs `queue`, `storage` or `schedule`"));
        };
        trigger.insert("type", span.clone(), Node::string(span.clone(), tag));
        match tag {
            "storage" => trigger.insert_missing(
                "events",
                span.clone(),
                Node::array(span.clone(), vec![Node::string(span.clone(), "created")]),
            ),
            "schedule" => {
                let schedule = trigger.remove("schedule").expect("the tag was found");
                trigger.insert("cron", schedule.key_span, schedule.value);
            }
            _ => {}
        }
    }
    Ok(())
}

fn storage(table: &mut Table) {
    let Some(notifications) = table.get_mut("notifications") else {
        return;
    };
    let Value::Array(items) = &mut notifications.value else {
        return;
    };
    for item in items {
        let span = item.span.clone();
        if let Value::Table(notification) = &mut item.value {
            notification.insert_missing(
                "events",
                span.clone(),
                Node::array(span.clone(), vec![Node::string(span, "created")]),
            );
        }
    }
}

fn email(table: &mut Table) {
    for (key, field) in [("inbound", "storage"), ("events", "queue")] {
        let Some(node) = table.get_mut(key) else {
            continue;
        };
        if let Value::String(id) = &node.value {
            let span = node.span.clone();
            let mut written = Table::default();
            written.insert(field, span.clone(), Node::string(span.clone(), id.clone()));
            *node = Node::table(span, written);
        }
    }
}

/// A sandbox's `session` is the `lifecycle` of its type. It is moved there once the keys every
/// resource accepts have been read, because those include a `lifecycle` of their own.
pub fn sandbox_session(table: &mut Table) {
    if let Some(session) = table.remove("session") {
        table.insert("lifecycle", session.key_span, session.value);
    }
}

fn links(ctx: &Context<'_>, target: &Target<'_>, table: &Table) -> Result<()> {
    let Some(Value::Array(links)) = table.get("links").map(|links| &links.value) else {
        return Ok(());
    };
    for link in links {
        if link.as_str() == Some(target.id) {
            return Err(ctx.error(
                link.span.clone(),
                format!("a {} cannot link to itself", kind_name(target.ty.table)),
            ));
        }
    }
    Ok(())
}

fn kind_name(table: &str) -> &str {
    match table {
        "build-environment" => "build environment",
        other => other,
    }
}

/// `env` and `env-file` become the `environment` map; a value that names an input is kept
/// out of it and recorded as a reference.
fn environment(ctx: &mut Context<'_>, target: &Target<'_>, table: &mut Table) -> Result<()> {
    let env = table.remove("env");
    let files = table.remove("env-file");
    let key_span = match (&env, &files) {
        (Some(env), _) => env.key_span.clone(),
        (None, Some(files)) => files.key_span.clone(),
        (None, None) => return Ok(()),
    };
    let mut variables = Table::default();
    let set = |variables: &mut Table, name: &str, span: Range<usize>, value: Node| {
        variables.remove(name);
        variables.insert(name, span, value);
    };

    if let Some(files) = files {
        let paths: Vec<&Node> = match &files.value.value {
            Value::String(_) => vec![&files.value],
            Value::Array(paths) => paths.iter().collect(),
            _ => {
                return Err(ctx.error(
                    files.value.span.clone(),
                    format!(
                        "expected a path or a list of paths, found {}",
                        files.value.kind()
                    ),
                ))
            }
        };
        for path in paths {
            let Some(relative) = path.as_str() else {
                return Err(ctx.error(
                    path.span.clone(),
                    format!("expected a path, found {}", path.kind()),
                ));
            };
            let full = env_file_path(ctx, path.span.clone(), relative)?;
            let text = std::fs::read_to_string(&full).map_err(|error| {
                ctx.error(
                    path.span.clone(),
                    format!("cannot read env file {}: {error}", full.display()),
                )
            })?;
            let parsed = env_file::parse(&SourceFile {
                name: ctx.base_dir.join(relative).display().to_string(),
                text: &text,
            })?;
            for (name, value) in parsed {
                set(
                    &mut variables,
                    &name,
                    files.key_span.clone(),
                    Node::string(path.span.clone(), value),
                );
            }
        }
    }

    if let Some(env) = env {
        let Value::Table(entries) = env.value.value.clone() else {
            return Err(ctx.error(
                env.value.span.clone(),
                format!("expected a table of variables, found {}", env.value.kind()),
            ));
        };
        let settings = settings(target.ty);
        for entry in entries.entries {
            let name = entry.key.as_str();
            if !is_env_name(name) {
                return Err(ctx.error(
                    entry.key_span,
                    format!(
                        "'{name}' is not an environment variable name; use letters, digits and underscores, not starting with a digit"
                    ),
                ));
            }
            if settings.iter().any(|setting| setting == name) {
                return Err(ctx.error(
                    entry.key_span,
                    format!(
                        "'{name}' is a setting of [{kind}.{id}], but it sits inside `env`, so it would be an environment variable; move it out of `env`, next to the other settings of [{kind}.{id}]",
                        kind = target.ty.table,
                        id = target.id,
                    ),
                ));
            }
            let span = entry.value.span.clone();
            match &entry.value.value {
                Value::String(_) => set(&mut variables, name, entry.key_span, entry.value),
                Value::Integer(_) | Value::Float(_) | Value::Boolean(_) => {
                    let text = match &entry.value.value {
                        Value::Boolean(flag) => flag.to_string(),
                        _ => entry.value.number_text().expect("a number"),
                    };
                    set(&mut variables, name, entry.key_span, Node::string(span, text));
                }
                Value::Table(reference) => {
                    let reference = input_reference(ctx, target, name, &entry, reference)?;
                    variables.remove(name);
                    ctx.env_refs.push(reference);
                }
                _ => {
                    return Err(ctx.error(
                        span,
                        format!(
                            "expected a string, a number, a boolean or {{ input = \"<id>\" }}, found {}",
                            entry.value.kind()
                        ),
                    ))
                }
            }
        }
    }

    let span = variables
        .entries
        .first()
        .map_or(key_span.clone(), |first| first.value.span.clone());
    table.insert("environment", key_span, Node::table(span, variables));
    Ok(())
}

/// The file `relative` names, which must stay inside the directory of alien.toml: an env file
/// is read at build time, so a path that leaves the project would read arbitrary files.
fn env_file_path(ctx: &Context<'_>, span: Range<usize>, relative: &str) -> Result<PathBuf> {
    let requested = Path::new(relative);
    if requested.is_absolute() || requested.has_root() {
        return Err(ctx.error(
            span,
            format!(
                "env-file '{relative}' is an absolute path; use a path relative to the directory of alien.toml"
            ),
        ));
    }
    let unreadable = |path: &Path, error: std::io::Error| {
        ctx.error(
            span.clone(),
            format!("cannot read env file {}: {error}", path.display()),
        )
    };
    let full = ctx.base_dir.join(requested);
    let resolved = full
        .canonicalize()
        .map_err(|error| unreadable(&full, error))?;
    let root = ctx
        .base_dir
        .canonicalize()
        .map_err(|error| unreadable(ctx.base_dir, error))?;
    if !resolved.starts_with(&root) {
        return Err(ctx.error(
            span,
            format!(
                "env-file '{relative}' is outside the directory of alien.toml; keep env files inside the project"
            ),
        ));
    }
    Ok(resolved)
}

fn input_reference(
    ctx: &Context<'_>,
    target: &Target<'_>,
    name: &str,
    entry: &Entry,
    reference: &Table,
) -> Result<EnvRef> {
    for field in &reference.entries {
        if field.key != "input" && field.key != "type" {
            return Err(ctx.error(
                field.key_span.clone(),
                format!(
                    "unknown key `{}`{}; a reference to an input is `{{ input = \"<id>\" }}`, with an optional `type` of plain or secret",
                    field.key,
                    suggest::hint(&field.key, ["input", "type"])
                ),
            ));
        }
    }
    let Some(input) = reference.get("input") else {
        return Err(ctx.error(
            entry.value.span.clone(),
            "a reference to an input is `{ input = \"<id>\" }`",
        ));
    };
    let Some(id) = input.as_str() else {
        return Err(ctx.error(
            input.span.clone(),
            format!("expected the ID of an input, found {}", input.kind()),
        ));
    };
    if !ctx.inputs.iter().any(|declared| declared == id) {
        return Err(ctx.error(
            input.span.clone(),
            format!(
                "unknown input '{id}'{}; declare it with an [input.{id}] table",
                suggest::hint(id, ctx.inputs.iter().map(String::as_str))
            ),
        ));
    }
    if !is_input_env_name(name) {
        return Err(ctx.error(
            entry.key_span.clone(),
            format!(
                "'{name}' takes its value from an input, so it must be an environment variable name in uppercase: letters, digits and underscores"
            ),
        ));
    }
    let var_type = match reference.get("type") {
        None => None,
        Some(kind) => match kind.as_str() {
            Some(kind @ ("plain" | "secret")) => Some(kind.to_string()),
            _ => return Err(ctx.error(kind.span.clone(), "`type` is plain or secret")),
        },
    };
    Ok(EnvRef {
        input: id.to_string(),
        name: name.to_string(),
        target: target.id.to_string(),
        var_type,
        span: entry.value.span.clone(),
    })
}

/// The profile a workload runs with: a named one, or grants written in place, which make a
/// profile named after the workload. A workload that says nothing gets an empty profile,
/// except a daemon, which has no cloud identity unless it asks for one.
fn permissions(ctx: &mut Context<'_>, target: &Target<'_>, table: &mut Table) -> Result<()> {
    let id = target.id;
    let kind = target.ty.table;
    let own_profile =
        |ctx: &mut Context<'_>, table: &mut Table, key_span: Range<usize>, grants: Node| {
            let span = grants.span.clone();
            ctx.inline_profiles.push(InlineProfile {
                name: id.to_string(),
                grants,
            });
            table.insert("permissions", key_span, Node::string(span, id));
        };
    let clash = |ctx: &Context<'_>, span: Range<usize>| {
        ctx.error(
            span,
            format!(
                "a [permissions.{id}] table already exists, and these grants would also be named '{id}'; use `permissions = \"{id}\"` or rename one"
            ),
        )
    };
    match table.remove("permissions") {
        None if kind == "daemon" => {}
        None => {
            let span = target.id_span.clone();
            if ctx.profiles.iter().any(|name| name == id) {
                return Err(clash(ctx, span));
            }
            own_profile(
                ctx,
                table,
                span.clone(),
                Node::table(span, Table::default()),
            );
        }
        Some(entry) => match &entry.value.value {
            Value::String(name) => {
                if !ctx.profiles.iter().any(|declared| declared == name) {
                    return Err(ctx.error(
                        entry.value.span.clone(),
                        format!(
                            "unknown permission profile '{name}'{}; declare it with a [permissions.{name}] table, or write the grants here: permissions = {{ <resource-id> = [\"storage/data-read\"] }}",
                            suggest::hint(name, ctx.profiles.iter().map(String::as_str))
                        ),
                    ));
                }
                table.insert("permissions", entry.key_span, entry.value);
            }
            Value::Table(grants) => {
                if ctx.profiles.iter().any(|name| name == id) {
                    return Err(clash(ctx, entry.value.span.clone()));
                }
                let settings = settings(target.ty);
                for grant in &grants.entries {
                    if settings.contains(&grant.key) && !ctx.resources.contains_key(&grant.key) {
                        return Err(ctx.error(
                            grant.key_span.clone(),
                            format!(
                                "'{key}' is a setting of [{kind}.{id}], but it sits inside `permissions`, where a key is a resource ID, and no resource has that ID; move it out of `permissions`, next to the other settings of [{kind}.{id}]",
                                key = grant.key,
                            ),
                        ));
                    }
                }
                own_profile(ctx, table, entry.key_span, entry.value);
            }
            _ => {
                return Err(ctx.error(
                    entry.value.span.clone(),
                    format!(
                        "expected a profile name or a table of grants, found {}",
                        entry.value.kind()
                    ),
                ))
            }
        },
    }
    Ok(())
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
    alien_core::stack_authoring::is_input_env_name(name)
        && name.chars().any(|c| c.is_ascii_uppercase())
}

/// A stack input: the strings and numbers that stand for a list or a table become one.
pub fn input(node: &mut Node) {
    let Value::Table(table) = &mut node.value else {
        return;
    };
    if let Some(provided) = table.get_mut("provided-by") {
        if provided.as_str().is_some() {
            let span = provided.span.clone();
            *provided = Node::array(span, vec![provided.clone()]);
        }
    }
    if let Some(when) = table.get_mut("when") {
        if let Value::String(_) = when.value {
            let span = when.span.clone();
            let mut condition = Table::default();
            condition.insert("input", span.clone(), when.clone());
            condition.insert("equals", span.clone(), Node::boolean(span.clone(), true));
            *when = Node::table(span, condition);
        }
    }
    if let Some(generate) = table.get_mut("generate") {
        if let Value::Integer(_) = generate.value {
            let span = generate.span.clone();
            let mut written = Table::default();
            written.insert("length", span.clone(), generate.clone());
            *generate = Node::table(span, written);
        }
    }
    if let Some(env) = table.get_mut("env") {
        let span = env.span.clone();
        let mappings = match &env.value {
            Value::Array(items) => items.clone(),
            _ => vec![env.clone()],
        };
        let mappings = mappings
            .into_iter()
            .map(|mapping| match &mapping.value {
                Value::String(_) => {
                    let mut written = Table::default();
                    written.insert("name", mapping.span.clone(), mapping.clone());
                    Node::table(mapping.span.clone(), written)
                }
                _ => mapping,
            })
            .collect();
        *env = Node::array(span, mappings);
    }
}
