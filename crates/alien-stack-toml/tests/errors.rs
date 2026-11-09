//! Every rejected alien.toml must say where: file, line and column of the offending value.

use std::path::Path;

use alien_stack_toml::ErrorData;

struct Located {
    file: String,
    line: usize,
    column: usize,
    message: String,
    snippet: String,
}

fn reject_in(dir: &Path, text: &str) -> Located {
    let error = alien_stack_toml::parse(text, "alien.toml", dir)
        .expect_err("the document must be rejected");
    assert_eq!(error.code, "ALIEN_TOML_INVALID", "{error}");
    match error.error.expect("error data") {
        ErrorData::InvalidDocument {
            file,
            line,
            column,
            message,
            snippet,
        } => Located {
            file,
            line,
            column,
            message,
            snippet,
        },
        other => panic!("unexpected error {other:?}"),
    }
}

fn reject(text: &str) -> Located {
    reject_in(Path::new("."), text)
}

const DAEMON: &str = r#"name = "acme-agent"

[daemon.agent]
build = { toolchain = "rust", binary-name = "acme-agent" }
"#;

#[track_caller]
fn assert_at(text: &str, line: usize, column: usize, message: &str) {
    let error = reject(text);
    assert_eq!(
        (error.line, error.column),
        (line, column),
        "wrong location for {message:?}: {}",
        error.message
    );
    assert!(
        error.message.contains(message),
        "expected {message:?} in {:?}",
        error.message
    );
    assert_eq!(error.file, "alien.toml");
}

#[test]
fn syntax_and_shape_errors() {
    assert_at("name = \"acme\nplatforms = []\n", 1, 13, "");
    assert_at("platforms = [\"machines\"]\n", 1, 1, "`name` is missing");
    assert_at(
        "nme = \"acme\"\n",
        1,
        1,
        "unknown key `nme` (did you mean 'name'?); expected one of: ai, artifact-registry",
    );
    assert_at(
        &format!("{DAEMON}hostaccess = true\n"),
        5,
        1,
        "unknown key `hostaccess` (did you mean 'host-access'?); expected one of: build, command",
    );
    assert_at(
        &format!("{DAEMON}cpu = [1]\n"),
        5,
        7,
        "expected a number or a size, found a list",
    );
    assert_at(
        "name = \"acme\"\nplatforms = [\"mars\"]\n",
        2,
        14,
        "unknown value 'mars'; expected one of: aws, gcp, azure, kubernetes, machines, local, test",
    );
    assert_at(
        &format!("{DAEMON}health-check = {{ port = \"web\" }}\n"),
        5,
        25,
        "expected a whole number, found a string",
    );
    assert_at(
        &format!("{DAEMON}health = {{ port = 8080 }}\n"),
        5,
        1,
        "unknown key `health` (did you mean 'health-check'?); expected one of: build, command",
    );
    assert_at(
        &format!("{DAEMON}host_access = true\n"),
        5,
        1,
        "unknown key `host_access` (did you mean 'host-access'?); expected one of: build, command",
    );
    assert_at(
        &DAEMON.replace("\"rust\"", "\"rusty\""),
        4,
        23,
        "unknown toolchain 'rusty' (did you mean 'rust'?); expected one of: rust, typescript, python, docker",
    );
}

#[test]
fn semantic_errors_point_at_the_value() {
    assert_at(
        &format!("{DAEMON}pool = \"gpu\"\n"),
        5,
        8,
        "unknown pool 'gpu'; declare it with a [pool.gpu] table",
    );
    assert_at(
        &format!(
            "{DAEMON}pool = \"gpu\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = 1\n"
        ),
        5,
        8,
        "unknown pool 'gpu'; declared pools: general",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16GB\"\nmachines = 1\n",
        5,
        10,
        "'16GB' is not a quantity",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\n",
        3,
        7,
        "a pool needs `machines`, a fixed count, or `autoscaling` with a minimum and a maximum",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = { default = 2, allowed = [3, 2] }\n",
        6,
        12,
        "needs min <= default <= max",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = 1\ndata-disk = { default = \"1500M\", allowed = [\"1Gi\", \"2Gi\"] }\n",
        7,
        13,
        "must be a whole number of GiB: 1500M",
    );
    assert_at(
        "name = \"acme\"\n\n[daemon.agent]\nmemory = \"1Gi\"\n",
        3,
        9,
        "daemon needs `build = { toolchain = \"rust\", binary-name = \"<binary>\" }` or `image = \"<reference>\"`",
    );
    assert_at(
        &format!("{DAEMON}stop-grace-period = \"24 hours\"\n"),
        5,
        21,
        "invalid duration \"24 hours\"; use whole numbers with ms, s, m, h or d",
    );
    assert_at(
        &format!("{DAEMON}stop-grace-period = \"48h\"\n"),
        5,
        21,
        "172800 seconds is outside 1 second to 24 hours",
    );
    assert_at(
        &format!("{DAEMON}memory = \"lots\"\n"),
        5,
        10,
        "'lots' is not a quantity",
    );
    assert_at(
        &format!("{DAEMON}links = [\"cache\"]\n"),
        5,
        10,
        "unknown resource 'cache'",
    );
    assert_at(
        &format!("{DAEMON}pool = \"genral\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = 1\n"),
        5,
        8,
        "unknown pool 'genral' (did you mean 'general'?); declared pools: general",
    );
    assert_at(
        &format!("{DAEMON}links = [\"snapshot\"]\n\n[storage.snapshots]\n"),
        5,
        10,
        "unknown resource 'snapshot' (did you mean 'snapshots'?)",
    );
    assert_at(
        &format!("{DAEMON}\n[storage.agent]\n"),
        6,
        10,
        "'agent' is already declared as a daemon on line 3",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = 1\n\n[storage.compute]\n",
        8,
        10,
        "'compute' is the ID of the compute cluster that holds the pools",
    );
}

#[test]
fn build_errors() {
    assert_at(
        "name = \"acme\"\n\n[daemon.agent]\nbuild = { toolchain = \"rust\" }\n",
        4,
        9,
        "missing field `binary-name`",
    );
    assert_at(
        &DAEMON.replace(" }", ", dockerfile = \"Dockerfile\" }"),
        4,
        59,
        "`dockerfile` applies to docker builds only, not rust",
    );
    assert_at(
        &format!("{DAEMON}image = \"ghcr.io/acme/agent:1\"\n"),
        5,
        9,
        "set either `build` (build from source) or `image` (a prebuilt image), not both",
    );
}

#[test]
fn runtime_resolves_and_rejects_bad_values() {
    let text = format!(
        "{DAEMON}runtime = {{ privileged = true, pid-namespace = \"private\", network-mode = \"appnet\", user = \"1000:1000\", mounts = [{{ source = \"/var/lib/acme\", target = \"/data\", options = \"ro\" }}] }}\n"
    );
    let stack = alien_stack_toml::parse(&text, "alien.toml", Path::new(".")).expect("resolves");
    let daemon = stack.resources["agent"]
        .config
        .downcast_ref::<alien_core::Daemon>()
        .expect("agent is a daemon");
    assert_eq!(
        daemon.runtime,
        Some(alien_core::DaemonRuntime {
            privileged: Some(true),
            pid_namespace: Some("private".to_string()),
            network_mode: Some("appnet".to_string()),
            mounts: vec![alien_core::DaemonRuntimeMount {
                source: "/var/lib/acme".to_string(),
                target: "/data".to_string(),
                options: Some("ro".to_string()),
            }],
            user: Some("1000:1000".to_string()),
        })
    );

    assert_at(
        &format!("{DAEMON}runtime = {{ network-mode = \"hots\" }}\n"),
        5,
        28,
        "runtime.networkMode must be 'host' or 'appnet'",
    );
    assert_at(
        &format!("{DAEMON}runtime = {{ pid-namespace = \"shared\" }}\n"),
        5,
        29,
        "runtime.pidNamespace must be 'host' or 'private'",
    );
    assert_at(
        &format!("{DAEMON}host-access = true\nruntime = {{ network-mode = \"host\" }}\n"),
        6,
        1,
        "`runtime` cannot be combined with `host-access`, which already sets the runtime; drop one",
    );
    assert_at(
        &format!("{DAEMON}runtime = {{ network = \"host\" }}\n"),
        5,
        13,
        "unknown key `network` (did you mean 'network-mode'?); expected one of: mounts, network-mode",
    );
}

#[test]
fn env_errors() {
    assert_at(
        &format!("{DAEMON}\n[daemon.agent.env]\nmemory = \"1Gi\"\n"),
        7,
        1,
        "'memory' is a setting of [daemon.agent], but it sits inside `env`, so it would be an environment variable; move it out of `env`, next to the other settings of [daemon.agent]",
    );
    assert_at(
        &format!("{DAEMON}\n[daemon.agent.env]\nAPI_TOKEN = {{ input = \"token\" }}\n"),
        7,
        23,
        "unknown input 'token'; declare it with an [input.token] table",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.env]\nAPI_TOKEN = {{ input = \"api_tokn\" }}\n\n[input.api_token]\nkind = \"secret\"\nlabel = \"Token\"\ndescription = \"Token.\"\n"
        ),
        7,
        23,
        "unknown input 'api_tokn' (did you mean 'api_token'?)",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.env]\napi_token = {{ input = \"api_token\" }}\n\n[input.api_token]\nkind = \"secret\"\nlabel = \"Token\"\ndescription = \"Token.\"\n"
        ),
        7,
        1,
        "'api_token' takes its value from an input, so it must be an environment variable name in uppercase",
    );
    assert_at(
        &format!("{DAEMON}\n[daemon.agent.env]\n\"BAD-NAME\" = \"x\"\n"),
        7,
        1,
        "'BAD-NAME' is not an environment variable name",
    );
    assert_at(
        &format!("{DAEMON}\n[daemon.agent.env]\nA = {{ input = \"x\", other = 1 }}\n"),
        7,
        20,
        "unknown key `other`; a reference to an input is `{ input = \"<id>\" }`, with an optional `type` of plain or secret",
    );
}

#[test]
fn endpoint_errors() {
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.public-endpoints]\napi = {{ port = 8080, host-label = \"@\" }}\nweb = {{ port = 8080, host-label = \"@\" }}\n"
        ),
        3,
        9,
        "only one apex public endpoint is allowed per resource; 'api' already uses hostLabel '@'",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.public-endpoints]\napi = {{ port = 8080 }}\nweb = {{ port = 9090 }}\n"
        ),
        3,
        9,
        "public endpoints on one daemon must currently route to the same backend port",
    );
}

#[test]
fn input_errors() {
    let input = |body: &str| format!("{DAEMON}\n[input.region]\nlabel = \"Region\"\n{body}\n");
    assert_at(
        &input("kind = \"string\""),
        6,
        8,
        "must include a description when the input is required",
    );
    assert_at(
        &format!("{DAEMON}\n[input.region]\nkind = \"string\"\n"),
        6,
        8,
        "missing field `label`",
    );
    assert_at(
        &input("kind = \"integer\"\nrequired = false\ndefault = 1.5"),
        10,
        11,
        "1.5 is not a whole number",
    );
    assert_at(
        &input("kind = \"boolean\"\nrequired = false\ndefault = \"yes\""),
        10,
        11,
        "expected a boolean for a Boolean input, got a string",
    );
    assert_at(
        &input("kind = \"number\"\nrequired = false\nmin-length = 2"),
        10,
        14,
        "applies to string and secret inputs only",
    );
    assert_at(
        &input("kind = \"enum\"\nrequired = false"),
        6,
        8,
        "an enum input needs at least one value",
    );
    assert_at(
        &input(
            "kind = \"string\"\nrequired = false\nenv = { name = \"REGION\", target-resources = [\"api\"] }",
        ),
        10,
        46,
        "unknown resource 'api' in `target-resources`",
    );
    assert_at(
        &input(
            "kind = \"string\"\nrequired = false\nenv = { name = \"REGION\", target-resources = [] }",
        ),
        10,
        45,
        "`target-resources` is empty",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[storage.files]\n\n[input.region]\nlabel = \"Region\"\nkind = \"string\"\nrequired = false\nenv = {{ name = \"REGION\", target-resources = [\"files\"] }}\n"
        ),
        12,
        46,
        "'files' is a storage, which takes no environment variables",
    );
    assert_at(
        &input("kind = \"string\"\nrequired = false\nenv = \"region\""),
        10,
        7,
        "env name 'region' must use uppercase letters, digits, and underscores",
    );
    assert_at(
        &format!("{DAEMON}\n[input.REGION]\nlabel = \"R\"\nkind = \"string\"\nrequired = false\n"),
        6,
        8,
        "the ID must not be all caps",
    );
}

#[test]
fn permission_errors() {
    assert_at(
        &format!("{DAEMON}permissions = \"runner\"\n"),
        5,
        15,
        "unknown permission profile 'runner'; declare it with a [permissions.runner] table",
    );
    assert_at(
        &format!("{DAEMON}\n[permissions.agent]\ncache = [\"kv/data-read\"]\n"),
        7,
        1,
        "unknown resource 'cache'; use a declared resource ID, or \"*\" for every resource",
    );
}

#[test]
fn inline_permission_errors() {
    assert_at(
        &format!("{DAEMON}permissions = {{ cache = [\"kv/data-read\"] }}\n"),
        5,
        17,
        "unknown resource 'cache'",
    );
    assert_at(
        &format!(
            "{DAEMON}permissions = {{ files = [\"storage/data-read\"] }}\n\n[storage.files]\n\n[permissions.agent]\n"
        ),
        5,
        15,
        "a [permissions.agent] table already exists, and these grants would also be named 'agent'",
    );
    assert_at(
        &format!("{DAEMON}permissions = \"agnt\"\n\n[permissions.agent]\n"),
        5,
        15,
        "unknown permission profile 'agnt' (did you mean 'agent'?)",
    );
}

#[test]
fn enabled_when_names_an_input() {
    assert_at(
        &format!("{DAEMON}enabled-when = \"with_agent\"\n"),
        5,
        16,
        "unknown input 'with_agent'; declare it with an [input.with_agent] table",
    );
    assert_at(
        &format!("{DAEMON}enabled-when = \"with_agnt\"\n\n[input.with_agent]\nlabel = \"A\"\nkind = \"boolean\"\nprovided-by = \"deployer\"\ndescription = \"x\"\n"),
        5,
        16,
        "unknown input 'with_agnt' (did you mean 'with_agent'?)",
    );
}

#[test]
fn env_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = reject_in(dir.path(), &format!("{DAEMON}env-file = \"app.env\"\n"));
    assert_eq!((missing.line, missing.column), (5, 12));
    assert!(
        missing.message.starts_with("cannot read env file"),
        "{}",
        missing.message
    );

    std::fs::write(dir.path().join("app.env"), "A=1\n# note\nnot a pair\n").unwrap();
    let malformed = reject_in(dir.path(), &format!("{DAEMON}env-file = \"app.env\"\n"));
    assert_eq!(
        malformed.file,
        dir.path().join("app.env").display().to_string(),
        "an env file error points into the env file"
    );
    assert_eq!((malformed.line, malformed.column), (3, 1));
    assert!(malformed.message.contains("expected KEY=value"));
}

#[test]
fn env_file_lists_read_in_order_and_inline_env_wins() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.env"), "ONE=a\nTWO=a\nTHREE=a\n").unwrap();
    std::fs::write(dir.path().join("b.env"), "TWO=b\nTHREE=b\n").unwrap();
    let text =
        format!("{DAEMON}env-file = [\"a.env\", \"b.env\"]\nenv = {{ THREE = \"inline\" }}\n");
    let stack = alien_stack_toml::parse(&text, "alien.toml", dir.path()).expect("stack resolves");
    let daemon = stack.resources["agent"]
        .config
        .downcast_ref::<alien_core::Daemon>()
        .expect("agent is a daemon");
    let expected: std::collections::HashMap<String, String> =
        [("ONE", "a"), ("TWO", "b"), ("THREE", "inline")]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
    assert_eq!(daemon.environment, expected);

    let missing = reject_in(
        dir.path(),
        &format!("{DAEMON}env-file = [\"a.env\", \"c.env\"]\n"),
    );
    assert_eq!((missing.line, missing.column), (5, 22));
    assert!(missing.message.contains("c.env"), "{}", missing.message);
}

#[test]
fn errors_show_the_source_line() {
    let error = reject(&format!("{DAEMON}pool = \"gpu\"\n"));
    assert_eq!(error.snippet, "  |\n5 | pool = \"gpu\"\n  |        ^^^^^");
}
