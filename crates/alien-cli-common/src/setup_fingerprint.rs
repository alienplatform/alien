//! The setup fingerprint: a digest of what a stack's setup creates, which registration reports
//! and the hosting service compares with each release to tell whether setup must run again.
//!
//! The service that hosts the setup templates computes the same value from the stack JSON a
//! release sends, so this must match it byte for byte: a canonical JSON (keys sorted, absent
//! values left out) of the stack projected onto what setup owns, hashed with SHA-256. Live
//! workloads count only by what setup provisions for them, not their code, command or
//! environment.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// Version of the setup contract and of the fingerprint scheme.
pub const SETUP_FINGERPRINT_VERSION: u32 = 1;

/// Per-platform revision of what setup generates; a bump asks every install to run setup again.
fn generator_revision(platform: &str) -> Option<u32> {
    match platform {
        "aws" => Some(4),
        "gcp" => Some(1),
        "azure" => Some(3),
        _ => None,
    }
}

const LIVE_WORKLOADS: &[&str] = &["container", "worker", "daemon", "sandbox"];
const LIVE_WORKLOAD_NOISE: &[&str] = &[
    "code",
    "command",
    "commandsEnabled",
    "environment",
    "readinessProbe",
    "healthCheck",
];
/// Repositories counted under their name from before a rename, so a renamed release changes
/// nothing setup owns.
const PREVIOUS_REPOSITORY_NAMES: &[(&str, &str)] = &[
    ("artifacts-", "alien-artifacts-"),
    ("primary-artifacts-", "alien-primary-artifacts-"),
    ("sandbox-cache/artifacts-", "sandbox-alien/alien-artifacts-"),
];

/// The fingerprint of `stack` (its JSON as a release sends it) for `platform`, with the
/// version of the scheme.
pub fn setup_fingerprint(stack: &Value, platform: &str) -> (String, u32) {
    let mut manifest = Map::new();
    manifest.insert("version".into(), json!(SETUP_FINGERPRINT_VERSION));
    manifest.insert("target".into(), json!(platform));
    manifest.insert("platform".into(), json!(platform));
    if let Some(revision) = generator_revision(platform) {
        manifest.insert("setupGeneratorRevision".into(), json!(revision));
    }
    manifest.insert("setupContract".into(), projection(stack, platform));
    let digest = Sha256::digest(stable_json(&Value::Object(manifest)).as_bytes());
    (hex::encode(digest), SETUP_FINGERPRINT_VERSION)
}

fn projection(stack: &Value, platform: &str) -> Value {
    let mut resources = Map::new();
    let mut live = Map::new();
    if let Some(entries) = stack.get("resources").and_then(Value::as_object) {
        for (id, entry) in entries {
            let Some(fields) = entry.as_object() else {
                resources.insert(id.clone(), entry.clone());
                continue;
            };
            resources.insert(id.clone(), normalized_entry(fields, platform));
            let config = fields.get("config").and_then(Value::as_object);
            let kind = config
                .and_then(|config| config.get("type"))
                .and_then(Value::as_str);
            if let (Some(config), Some(kind)) = (config, kind) {
                if fields.get("lifecycle") == Some(&json!("live")) && LIVE_WORKLOADS.contains(&kind)
                {
                    let mut entry = fields.clone();
                    entry.insert("config".into(), Value::Object(live_config(config)));
                    live.insert(id.clone(), Value::Object(entry));
                }
            }
        }
    }
    let mut projection = Map::new();
    for key in ["id", "permissions", "supportedPlatforms"] {
        if let Some(value) = stack.get(key) {
            projection.insert(key.into(), value.clone());
        }
    }
    projection.insert("resources".into(), Value::Object(resources));
    projection.insert("liveWorkloadRequirements".into(), Value::Object(live));
    Value::Object(projection)
}

fn normalized_entry(entry: &Map<String, Value>, platform: &str) -> Value {
    let Some(config) = entry.get("config").and_then(Value::as_object) else {
        return Value::Object(entry.clone());
    };
    let kind = config.get("type").and_then(Value::as_str);
    let lifecycle = entry.get("lifecycle").and_then(Value::as_str);
    let mut entry = entry.clone();
    // Neither cloud's setup renders a Frozen sandbox's image. Azure builds a registry image into
    // a disk image with a grant a catalog-only setup never gave, so the kind of image counts.
    if (platform == "azure" || platform == "gcp")
        && lifecycle == Some("frozen")
        && kind == Some("sandbox")
    {
        let mut rest = config.clone();
        let code = rest.remove("code");
        if platform == "azure" && code.as_ref().is_some_and(is_registry_image) {
            rest.insert("azureImageKind".into(), json!("registry"));
        }
        entry.insert("config".into(), Value::Object(rest));
        return Value::Object(entry);
    }
    if lifecycle != Some("live") || !kind.is_some_and(|kind| LIVE_WORKLOADS.contains(&kind)) {
        return Value::Object(entry);
    }
    entry.insert("config".into(), Value::Object(live_config(config)));
    Value::Object(entry)
}

fn live_config(config: &Map<String, Value>) -> Map<String, Value> {
    let mut normalized: Map<String, Value> = config
        .iter()
        .filter(|(key, _)| !LIVE_WORKLOAD_NOISE.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if config.get("type") != Some(&json!("sandbox")) {
        return normalized;
    }
    // A Live sandbox's image and lifetime are runtime-owned; the build role's registry grant
    // names one repository, so it stays.
    normalized.remove("lifecycle");
    if let Some(image) = config.get("privateBaseImage").and_then(Value::as_str) {
        let repository = private_base_image_repository(image);
        let renamed = match repository.split_once('/') {
            None => repository,
            Some((host, path)) => format!("{host}/{}", renamed_repository(path)),
        };
        normalized.insert("privateBaseImage".into(), json!(renamed));
    }
    normalized
}

/// A catalog name is bare (`ubuntu`); anything naming a registry, tag or digest is an image.
fn is_registry_image(code: &Value) -> bool {
    code.get("image")
        .and_then(Value::as_str)
        .is_some_and(|image| image.contains(['/', ':', '@']))
}

fn renamed_repository(repository: &str) -> String {
    PREVIOUS_REPOSITORY_NAMES
        .iter()
        .find(|(from, _)| repository.starts_with(from))
        .map(|(from, to)| format!("{to}{}", &repository[from.len()..]))
        .unwrap_or_else(|| repository.to_string())
}

/// Registry host and repository of an ECR image, without tag or digest; anything else whole.
fn private_base_image_repository(image: &str) -> String {
    let Some((host, path)) = image.split_once('/') else {
        return image.to_string();
    };
    let Some(ecr_at) = host.find(".dkr.ecr.") else {
        return image.to_string();
    };
    let account = &host[..ecr_at];
    let rest = &host[ecr_at + ".dkr.ecr.".len()..];
    let Some(region) = [".amazonaws.com.cn", ".amazonaws.com"]
        .iter()
        .find_map(|suffix| rest.strip_suffix(suffix))
    else {
        return image.to_string();
    };
    let region_ok = region == "{region}"
        || (!region.is_empty()
            && region
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-')));
    if account.len() != 12 || !account.bytes().all(|byte| byte.is_ascii_digit()) || !region_ok {
        return image.to_string();
    }
    let repository = match path.find('@') {
        Some(at) => &path[..at],
        None => {
            let last_slash = path.rfind('/').map_or(0, |slash| slash + 1);
            match path[last_slash..].find(':') {
                Some(tag) => &path[..last_slash + tag],
                None => path,
            }
        }
    };
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
    };
    if !repository.split('/').all(segment_ok) {
        return image.to_string();
    }
    format!("{host}/{repository}")
}

/// JSON with object keys sorted and no whitespace, as JavaScript's `JSON.stringify` writes
/// each value.
fn stable_json(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(stable_json).collect::<Vec<_>>().join(",")
        ),
        Value::Object(fields) => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        stable_json(&fields[key])
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Value::Number(number) => match number.as_f64() {
            // JavaScript writes an integral double without a fraction.
            Some(float) if number.is_f64() && float.fract() == 0.0 && float.abs() < 1e21 => {
                format!("{}", float as i64)
            }
            _ => number.to_string(),
        },
        other => other.to_string(),
    }
}
