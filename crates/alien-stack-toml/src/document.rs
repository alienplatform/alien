//! The `alien.toml` document, as written.
//!
//! These types describe the file's shape only. They keep source spans on every value a
//! later check can reject, so [`crate::resolve`] can point at the offending line. The JSON
//! schema editors use for completion is generated from these same types.

use std::collections::BTreeMap;
use std::fmt;

use alien_core::instance_catalog::Architecture;
use alien_core::{Platform, ResourceLifecycle, StackInputKind, StackInputProvider};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::de::{self, value::MapAccessDeserializer, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use toml::Spanned;

/// Named tables (`[daemon.<id>]`) keyed by their spanned id.
pub type Tables<T> = IndexMap<Spanned<String>, Spanned<T>>;

/// A stack written as `alien.toml`.
///
/// It describes the same resources as an `alien.ts` file and resolves to the same stack.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
#[schemars(title = "alien.toml")]
pub struct Document {
    /// Stack name. Letters, digits, hyphens and underscores, up to 128 characters.
    #[schemars(with = "String")]
    pub name: Spanned<String>,

    /// Platforms this stack can be deployed to. Omit to allow every platform.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Platform>>")]
    pub platforms: Option<Spanned<Vec<Platform>>>,

    /// ID of the compute cluster that holds the pools. Defaults to "compute". Set it when
    /// moving an existing stack to alien.toml so its cluster keeps its ID.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub cluster: Option<Spanned<String>>,

    /// Machine pools: what hardware each pool's machines need and how many run.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Pool>")]
    pub pool: Tables<Pool>,

    /// Daemons: one process on every machine of their pool.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Daemon>")]
    pub daemon: Tables<Daemon>,

    /// Object storage buckets.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Storage>")]
    pub storage: Tables<Storage>,

    /// Stack inputs: values provided per deployment, by the developer or the deployer.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Input>")]
    pub input: Tables<Input>,

    /// Permission profiles, by name. Each maps a resource ID (or "*" for every resource)
    /// to the permission sets granted on it, such as `["storage/data-read"]`.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, BTreeMap<String, Vec<String>>>")]
    pub permissions: Tables<Profile>,
}

/// A permission profile: permission sets granted per resource ID.
pub type Profile = IndexMap<Spanned<String>, Vec<Spanned<String>>>;

/// A machine pool. On machines deployments the pool name is what `join --capacity-group` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Pool {
    /// CPU cores each machine needs, for example 4.
    #[schemars(with = "Cpu")]
    pub cpu: Spanned<Cpu>,

    /// Memory each machine needs, for example "16Gi".
    #[schemars(with = "String")]
    pub memory: Spanned<String>,

    /// Size of the disk holding the operating system. Defaults to "20Gi".
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub disk: Option<Spanned<String>>,

    /// An extra empty block device each machine must provide: one size such as "1Ti", or
    /// a range each deployment picks from, `{ min = "0Gi", default = "200Gi", max = "4Ti" }`.
    #[serde(default)]
    #[schemars(with = "Option<DataDisk>")]
    pub data_disk: Option<Spanned<DataDisk>>,

    /// CPU architecture of the machines.
    #[serde(default)]
    pub arch: Option<Architecture>,

    /// Whether machines must support nested virtualization (to run VMs inside them).
    #[serde(default)]
    pub nested_virtualization: Option<bool>,

    /// A GPU each machine needs.
    #[serde(default)]
    pub gpu: Option<Gpu>,

    /// A fixed number of machines: `2`, or a range each deployment picks from,
    /// `{ min = 1, max = 5, default = 2 }`. Set this or `autoscale`.
    #[serde(default)]
    #[schemars(with = "Option<Choice>")]
    pub machines: Option<Spanned<Choice>>,

    /// Autoscale between a minimum and a maximum number of machines. Set this or `machines`.
    #[serde(default)]
    #[schemars(with = "Option<Autoscale>")]
    pub autoscale: Option<Spanned<Autoscale>>,

    /// How many failure domains (zones) the pool's machines spread across, 1 to 255.
    #[serde(default)]
    #[schemars(with = "Option<u8>")]
    pub failure_domain_spread: Option<Spanned<i64>>,
}

/// A GPU requirement.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Gpu {
    /// GPU model, for example "nvidia-l4".
    #[serde(rename = "type")]
    pub gpu_type: String,
    /// Number of GPUs per machine.
    pub count: u32,
}

/// Autoscaling bounds.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Autoscale {
    /// Fewest machines: a number, or a range each deployment picks from.
    #[schemars(with = "Choice")]
    pub min: Spanned<Choice>,
    /// Most machines: a number, or a range each deployment picks from.
    #[schemars(with = "Choice")]
    pub max: Spanned<Choice>,
}

/// A daemon: a long-running process on every machine of its pool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Daemon {
    /// Build the daemon from source, for example `{ rust = "acme-agent" }`. Set this or `image`.
    #[serde(default)]
    #[schemars(with = "Option<Build>")]
    pub build: Option<Spanned<Build>>,

    /// Run a prebuilt container image. Set this or `build`.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub image: Option<Spanned<String>>,

    /// Pool to run on. Omit to run on every pool.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub pool: Option<Spanned<String>>,

    /// CPU cores reserved for the daemon. Defaults to 0.1.
    #[serde(default)]
    #[schemars(with = "Option<Cpu>")]
    pub cpu: Option<Spanned<Cpu>>,

    /// Memory reserved for the daemon, for example "4Gi". Defaults to "128Mi".
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub memory: Option<Spanned<String>>,

    /// Command to run instead of the image's default.
    #[serde(default)]
    pub command: Option<Vec<String>>,

    /// Full access to the host: privileged, the host's PID namespace and network, the host's
    /// `/` mounted at `/host`, running as root (uid 0).
    #[serde(default)]
    pub host_access: bool,

    /// How long the daemon may take to stop on updates, drains and deletes: seconds, or a
    /// duration such as "10m" or "24h". At most 24 hours.
    #[serde(default)]
    #[schemars(with = "Option<Duration>")]
    pub stop_grace_period: Option<Spanned<Duration>>,

    /// HTTP health check.
    #[serde(default)]
    #[schemars(with = "Option<Health>")]
    pub health: Option<Spanned<Health>>,

    /// Environment variables. These win over `env-file`.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, String>")]
    pub env: IndexMap<Spanned<String>, Spanned<String>>,

    /// A file of `KEY=value` lines, relative to alien.toml, read when the stack is loaded.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub env_file: Option<Spanned<String>>,

    /// Resources this daemon uses, by ID.
    #[serde(default)]
    #[schemars(with = "Vec<String>")]
    pub links: Vec<Spanned<String>>,

    /// Permission profile that gives the daemon a cloud identity. Omit for none. On AWS,
    /// GCP and Azure it cannot be combined with `host-access`.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub permissions: Option<Spanned<String>>,

    /// Public HTTP endpoints, by name.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Endpoint>")]
    pub endpoints: Tables<Endpoint>,

    /// Secrets the developer provides, by environment variable name.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Secret>")]
    pub secrets: Tables<Secret>,

    /// Who owns the daemon after setup. Defaults to "live" (each release updates it).
    #[serde(default)]
    pub lifecycle: Option<ResourceLifecycle>,

    /// Create this daemon only on these platforms. On them, the deployer may still decline it.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Platform>>")]
    pub platforms: Option<Spanned<Vec<Platform>>>,

    /// Create this daemon only when this deployer-provided boolean `[input.<id>]` is true.
    /// Set this or `platforms`.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub enabled_when: Option<Spanned<String>>,
}

/// How to build a daemon from source.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Build {
    /// Name of the Rust binary to build with Cargo.
    pub rust: String,
    /// Source directory, relative to where `alien` runs. Defaults to ".".
    #[serde(default)]
    pub src: Option<String>,
}

/// An HTTP health check.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Health {
    /// Port to check.
    pub port: u16,
    /// Path to check. Defaults to "/health".
    #[serde(default)]
    pub path: Option<String>,
    /// HTTP method. Defaults to "GET".
    #[serde(default)]
    pub method: Option<String>,
    /// How long one check may take: seconds, or a duration such as "2s". Defaults to 1 second.
    #[serde(default)]
    #[schemars(with = "Option<Duration>")]
    pub timeout: Option<Spanned<Duration>>,
    /// Consecutive failures before the daemon counts as unhealthy. Defaults to 3.
    #[serde(default)]
    pub failure_threshold: Option<u32>,
}

/// A public HTTP endpoint.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Endpoint {
    /// Port the daemon serves on.
    pub port: u16,
    /// Host label under the deployment's domain. "@" is the domain itself. Defaults to the
    /// endpoint name.
    #[serde(default)]
    pub host: Option<String>,
    /// Also route every subdomain of the host label.
    #[serde(default)]
    pub wildcard: bool,
}

/// A secret the developer provides; it reaches the daemon as an environment variable.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Secret {
    /// Shown where the value is asked for. Defaults to the variable name.
    #[serde(default)]
    pub label: Option<String>,
    /// What the secret is for.
    #[serde(default)]
    pub description: Option<String>,
    /// Whether a deployment needs a value. Defaults to true.
    #[serde(default)]
    pub required: Option<bool>,
    /// Ask for the secret only on these platforms.
    #[serde(default)]
    pub platforms: Option<Vec<Platform>>,
}

/// An object storage bucket.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Storage {
    /// Allow anyone to read objects without credentials.
    #[serde(default)]
    pub public_read: bool,
    /// Keep previous versions of overwritten objects.
    #[serde(default)]
    pub versioning: bool,
    /// Who owns the bucket. Defaults to "frozen" (setup creates it).
    #[serde(default)]
    pub lifecycle: Option<ResourceLifecycle>,
    /// Create this bucket only on these platforms. On them, the deployer may still decline it.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Platform>>")]
    pub platforms: Option<Spanned<Vec<Platform>>>,

    /// Create this bucket only when this deployer-provided boolean `[input.<id>]` is true.
    /// Set this or `platforms`.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub enabled_when: Option<Spanned<String>>,
}

/// CPU cores: a number such as 2 or 0.5, or a quantity string such as "500m".
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[schemars(untagged)]
pub enum Cpu {
    Integer(i64),
    Float(f64),
    Quantity(String),
}

impl<'de> Deserialize<'de> for Cpu {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CpuVisitor;
        impl Visitor<'_> for CpuVisitor {
            type Value = Cpu;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "a number of CPU cores such as 2 or 0.5, or a quantity such as \"500m\"",
                )
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Cpu, E> {
                Ok(Cpu::Integer(v))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Cpu, E> {
                Ok(Cpu::Float(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Cpu, E> {
                Ok(Cpu::Quantity(v.to_string()))
            }
        }
        deserializer.deserialize_any(CpuVisitor)
    }
}

/// A duration: whole seconds, or a string such as "90s", "10m" or "24h".
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[schemars(untagged)]
pub enum Duration {
    Seconds(i64),
    Text(String),
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DurationVisitor;
        impl Visitor<'_> for DurationVisitor {
            type Value = Duration;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("whole seconds, or a duration such as \"10m\" or \"24h\"")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Duration, E> {
                Ok(Duration::Seconds(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Duration, E> {
                Ok(Duration::Text(v.to_string()))
            }
        }
        deserializer.deserialize_any(DurationVisitor)
    }
}

/// A count: one number, or a range a deployment picks from.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[schemars(untagged)]
pub enum Choice {
    Exactly(u32),
    Range(ChoiceRange),
}

/// A range a deployment picks from; `default` applies when it does not pick.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChoiceRange {
    pub min: u32,
    pub max: u32,
    pub default: u32,
}

impl<'de> Deserialize<'de> for Choice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ChoiceVisitor;
        impl<'de> Visitor<'de> for ChoiceVisitor {
            type Value = Choice;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, or a range such as { min = 1, max = 5, default = 2 }")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Choice, E> {
                u32::try_from(v)
                    .map(Choice::Exactly)
                    .map_err(|_| E::invalid_value(de::Unexpected::Signed(v), &self))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Choice, A::Error> {
                ChoiceRange::deserialize(MapAccessDeserializer::new(map)).map(Choice::Range)
            }
        }
        deserializer.deserialize_any(ChoiceVisitor)
    }
}

/// A data disk size: one size, or a range a deployment picks from.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[schemars(untagged)]
pub enum DataDisk {
    Size(String),
    Range(DataDiskRange),
}

/// Data disk sizes a deployment picks from, as quantities such as "50Gi" or "1Ti".
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DataDiskRange {
    pub min: String,
    pub default: String,
    pub max: String,
}

impl<'de> Deserialize<'de> for DataDisk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DataDiskVisitor;
        impl<'de> Visitor<'de> for DataDiskVisitor {
            type Value = DataDisk;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "a size such as \"1Ti\", or a range such as { min = \"0Gi\", default = \"200Gi\", max = \"4Ti\" }",
                )
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<DataDisk, E> {
                Ok(DataDisk::Size(v.to_string()))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<DataDisk, A::Error> {
                DataDiskRange::deserialize(MapAccessDeserializer::new(map)).map(DataDisk::Range)
            }
        }
        deserializer.deserialize_any(DataDiskVisitor)
    }
}

/// A stack input: a value provided per deployment. Its ID is the table name.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Input {
    /// What kind of value it is.
    pub kind: InputKind,
    /// Who provides the value: "developer", "deployer", or both. Defaults to "developer".
    #[serde(default)]
    #[schemars(with = "Option<OneOrMany<Provider>>")]
    pub provided_by: Option<Spanned<OneOrMany<Provider>>>,
    /// Whether a deployment needs a value. Defaults to true.
    #[serde(default)]
    pub required: Option<bool>,
    /// Shown where the value is asked for. Defaults to the input ID.
    #[serde(default)]
    pub label: Option<String>,
    /// What the value is for. Required for required inputs.
    #[serde(default)]
    pub description: Option<String>,
    /// Example shown in an empty field.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Value used when none is provided.
    #[serde(default)]
    #[schemars(with = "Option<InputValue>")]
    pub default: Option<Spanned<InputValue>>,
    /// Ask for the input only on these platforms.
    #[serde(default)]
    #[schemars(with = "Option<Vec<Platform>>")]
    pub platforms: Option<Spanned<Vec<Platform>>>,
    /// Environment variables that receive the value: a name, `{ name, targets }`, or a list.
    /// Without `targets` every daemon gets it.
    #[serde(default)]
    #[schemars(with = "Option<OneOrMany<EnvTarget>>")]
    pub env: Option<Spanned<OneOrMany<EnvTarget>>>,
    /// Shortest accepted string (string and secret inputs).
    #[serde(default)]
    #[schemars(with = "Option<u32>")]
    pub min_length: Option<Spanned<u32>>,
    /// Longest accepted string (string and secret inputs).
    #[serde(default)]
    #[schemars(with = "Option<u32>")]
    pub max_length: Option<Spanned<u32>>,
    /// Regular expression the value must match (string and secret inputs).
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub pattern: Option<Spanned<String>>,
    /// Smallest accepted number (number and integer inputs).
    #[serde(default)]
    #[schemars(with = "Option<f64>")]
    pub min: Option<Spanned<InputValue>>,
    /// Largest accepted number (number and integer inputs).
    #[serde(default)]
    #[schemars(with = "Option<f64>")]
    pub max: Option<Spanned<InputValue>>,
    /// The accepted values (enum inputs).
    #[serde(default)]
    #[schemars(with = "Option<Vec<String>>")]
    pub values: Option<Spanned<Vec<String>>>,
    /// Fewest accepted items (string-list inputs).
    #[serde(default)]
    #[schemars(with = "Option<u32>")]
    pub min_items: Option<Spanned<u32>>,
    /// Most accepted items (string-list inputs).
    #[serde(default)]
    #[schemars(with = "Option<u32>")]
    pub max_items: Option<Spanned<u32>>,
}

/// The kind of value an input takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InputKind {
    String,
    Secret,
    Number,
    Integer,
    Boolean,
    Enum,
    StringList,
}

impl From<InputKind> for StackInputKind {
    fn from(kind: InputKind) -> Self {
        match kind {
            InputKind::String => StackInputKind::String,
            InputKind::Secret => StackInputKind::Secret,
            InputKind::Number => StackInputKind::Number,
            InputKind::Integer => StackInputKind::Integer,
            InputKind::Boolean => StackInputKind::Boolean,
            InputKind::Enum => StackInputKind::Enum,
            InputKind::StringList => StackInputKind::StringList,
        }
    }
}

/// Who provides an input's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    /// The developer, for each deployment.
    Developer,
    /// Whoever installs the deployment.
    Deployer,
}

impl From<Provider> for StackInputProvider {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Developer => StackInputProvider::Developer,
            Provider::Deployer => StackInputProvider::Deployer,
        }
    }
}

/// One value or a list of them.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    pub fn into_vec(self) -> Vec<T> {
        match self {
            OneOrMany::One(one) => vec![one],
            OneOrMany::Many(many) => many,
        }
    }
}

/// An environment variable that receives an input's value.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EnvTarget {
    /// The variable name; every daemon gets it.
    Name(String),
    /// The variable name and the resources that get it.
    Mapping(EnvMapping),
}

/// An environment variable for chosen resources.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct EnvMapping {
    /// The variable name.
    pub name: String,
    /// IDs of the resources that get it. A trailing `*` matches a prefix.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
}

/// A value as written in TOML: string, number, boolean, or list of strings.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[schemars(untagged)]
pub enum InputValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    StringList(Vec<String>),
}

impl<'de> Deserialize<'de> for InputValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InputValueVisitor;
        impl<'de> Visitor<'de> for InputValueVisitor {
            type Value = InputValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string, number, boolean, or list of strings")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<InputValue, E> {
                Ok(InputValue::String(v.to_string()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<InputValue, E> {
                Ok(InputValue::Integer(v))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<InputValue, E> {
                Ok(InputValue::Float(v))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<InputValue, E> {
                Ok(InputValue::Boolean(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<InputValue, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<String>()? {
                    items.push(item);
                }
                Ok(InputValue::StringList(items))
            }
        }
        deserializer.deserialize_any(InputValueVisitor)
    }
}
