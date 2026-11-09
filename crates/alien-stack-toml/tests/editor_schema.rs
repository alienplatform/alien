//! The editor schema accepts what the resolver accepts and rejects what it rejects.

use std::path::PathBuf;

use alien_stack_toml::EDITOR_SCHEMA;
use jsonschema::Validator;
use serde_json::Value;

fn validator() -> Validator {
    let schema: Value = serde_json::from_str(EDITOR_SCHEMA).expect("the editor schema is JSON");
    jsonschema::validator_for(&schema).expect("the editor schema is a valid JSON schema")
}

fn problems(validator: &Validator, toml_text: &str) -> Vec<String> {
    let document: Value = toml::from_str(toml_text).expect("the test document is TOML");
    validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path))
        .collect()
}

fn fixtures() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut found: Vec<(String, String)> = std::fs::read_dir(&root)
        .expect("fixtures directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.join("alien.toml").exists())
        .map(|path| {
            let text =
                std::fs::read_to_string(path.join("alien.toml")).expect("alien.toml is readable");
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                text,
            )
        })
        .collect();
    found.sort();
    found
}

#[test]
fn every_fixture_passes_the_editor_schema() {
    let validator = validator();
    let fixtures = fixtures();
    assert!(
        fixtures.len() >= 10,
        "found only {} fixtures",
        fixtures.len()
    );
    let mut failures = Vec::new();
    for (name, text) in &fixtures {
        let found = problems(&validator, text);
        if !found.is_empty() {
            failures.push(format!("{name}:\n  {}", found.join("\n  ")));
        }
    }
    assert!(
        failures.is_empty(),
        "the editor schema rejects fixtures the resolver accepts:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_schema_rejects_what_the_resolver_rejects() {
    let validator = validator();
    let cases: &[(&str, &str, &str)] = &[
        ("unknown top-level key", "name = \"a\"\nplatform = [\"aws\"]\n", "platform"),
        ("missing name", "platforms = [\"aws\"]\n", "name"),
        (
            "unknown resource key",
            "name = \"a\"\n[storage.data]\nversioning = true\nbogus = 1\n",
            "bogus",
        ),
        (
            "camelCase key",
            "name = \"a\"\n[daemon.d]\nimage = \"x\"\nstopGracePeriod = 5\n",
            "stopGracePeriod",
        ),
        (
            "alien-core spelling of a unit key",
            "name = \"a\"\n[daemon.d]\nimage = \"x\"\nstop-grace-period-seconds = 5\n",
            "stop-grace-period-seconds",
        ),
        (
            "both image and build",
            "name = \"a\"\n[daemon.d]\nimage = \"x\"\nbuild = { toolchain = \"rust\", binary-name = \"d\" }\n",
            "oneOf",
        ),
        (
            "neither image nor build",
            "name = \"a\"\n[daemon.d]\ncpu = 1\n",
            "oneOf",
        ),
        (
            "unknown toolchain",
            "name = \"a\"\n[daemon.d]\nbuild = { toolchain = \"go\" }\n",
            "go",
        ),
        (
            "toolchain key of another toolchain",
            "name = \"a\"\n[daemon.d]\nbuild = { toolchain = \"rust\", binary-name = \"d\", dockerfile = \"x\" }\n",
            "dockerfile",
        ),
        (
            "bad duration",
            "name = \"a\"\n[daemon.d]\nimage = \"x\"\nstop-grace-period = \"soon\"\n",
            "soon",
        ),
        (
            "bad size",
            "name = \"a\"\n[worker.w]\nimage = \"x\"\nmemory = \"big\"\n",
            "big",
        ),
        (
            "range without default",
            "name = \"a\"\n[pool.p]\ncpu = 2\nmemory = \"4Gi\"\nmachines = { allowed = [1, 3] }\n",
            "/pool/p/machines",
        ),
        (
            "range written with min and max",
            "name = \"a\"\n[pool.p]\ncpu = 2\nmemory = \"4Gi\"\nmachines = { default = 2, min = 1, max = 3 }\n",
            "/pool/p/machines",
        ),
        (
            "trigger without a kind",
            "name = \"a\"\n[worker.w]\nimage = \"x\"\ntriggers = [{ events = [\"created\"] }]\n",
            "/worker/w/triggers/0",
        ),
        (
            "invalid resource ID",
            "name = \"a\"\n[storage.\"has space\"]\n",
            "has space",
        ),
        (
            "cluster as a string",
            "name = \"a\"\ncluster = \"compute\"\n",
            "cluster",
        ),
        (
            "unknown lifecycle",
            "name = \"a\"\n[storage.data]\nlifecycle = \"eternal\"\n",
            "eternal",
        ),
        (
            "environment written in the alien-core spelling",
            "name = \"a\"\n[daemon.d]\nimage = \"x\"\nenvironment = { A = \"b\" }\n",
            "environment",
        ),
        (
            "input reference with an unknown key",
            "name = \"a\"\n[daemon.d]\nimage = \"x\"\nenv = { A = { input = \"k\", secret = true } }\n",
            "secret",
        ),
    ];
    for (what, text, mentions) in cases {
        let found = problems(&validator, text);
        assert!(!found.is_empty(), "{what}: the schema accepts the document");
        assert!(
            found.iter().any(|problem| problem.contains(mentions)),
            "{what}: no problem mentions `{mentions}`:\n{}",
            found.join("\n")
        );
    }
}

#[test]
fn the_schema_accepts_the_shorthand() {
    let validator = validator();
    let text = r#"
name = "shorthand"
platforms = ["aws", "machines"]

[input.api_token]
kind = "secret"
label = "API token"
provided-by = "deployer"
env = "API_TOKEN"

[pool.general]
cpu = 4
memory = "16Gi"
machines = { default = 2, allowed = [1, 5] }
data-disk = "1Ti"

[daemon.agent]
build = { toolchain = "rust", binary-name = "agent" }
pool = "general"
cpu = 0.5
memory = "512Mi"
stop-grace-period = "1h30m"
host-access = true
env = { MODE = "fast", WORKERS = 4, VERBOSE = true, API_TOKEN = { input = "api_token" } }
env-file = ["a.env", "b.env"]
permissions = { "*" = ["storage/data-read"] }
[daemon.agent.public-endpoints]
api = { port = 8080, host-label = "@" }

[container.web]
image = "nginx"
cpu = { default = 1, allowed = [0.5, 2] }
memory = "1Gi"
ports = [80, { port = 443 }]
persistent-storage = "10Gi"

[worker.fn]
image = "fn"
triggers = [{ queue = "jobs" }, { storage = "data" }, { schedule = "0 * * * *" }]

[storage.data]
[queue.jobs]
"#;
    assert_eq!(problems(&validator, text), Vec::<String>::new());
}
