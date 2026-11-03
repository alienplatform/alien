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
build = { rust = "acme-agent" }
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
    assert_at("platforms = [\"machines\"]\n", 1, 1, "missing field `name`");
    assert_at("nme = \"acme\"\n", 1, 1, "unknown field `nme`");
    assert_at(
        &format!("{DAEMON}hostaccess = true\n"),
        5,
        1,
        "unknown field `hostaccess`",
    );
    assert_at(
        &format!("{DAEMON}cpu = [1]\n"),
        5,
        7,
        "a number of CPU cores such as 2 or 0.5",
    );
    assert_at(
        "name = \"acme\"\nplatforms = [\"mars\"]\n",
        2,
        14,
        "unknown variant `mars`",
    );
    assert_at(
        &format!("{DAEMON}health = {{ path = \"/health\" }}\n"),
        5,
        10,
        "missing field `port`",
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
        "pool needs `machines = <count>` or `autoscale",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = { min = 3, max = 2, default = 2 }\n",
        6,
        12,
        "needs min <= default <= max",
    );
    assert_at(
        "name = \"acme\"\n\n[pool.general]\ncpu = 4\nmemory = \"16Gi\"\nmachines = 1\ndata-disk = { min = \"1Gi\", default = \"1500M\", max = \"2Gi\" }\n",
        7,
        13,
        "must be a whole number of GiB: 1500M",
    );
    assert_at(
        "name = \"acme\"\n\n[daemon.agent]\nmemory = \"1Gi\"\n",
        3,
        9,
        "daemon needs `build = { rust = \"<binary>\" }` or `image = \"<reference>\"`",
    );
    assert_at(
        &format!("{DAEMON}stop-grace-period = \"24 hours\"\n"),
        5,
        21,
        "'24 hours' is not a duration",
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
fn scope_secret_and_endpoint_errors() {
    assert_at(
        &format!("{DAEMON}platforms = [\"aws\"]\n").replace(
            "name = \"acme-agent\"",
            "name = \"acme-agent\"\nplatforms = [\"machines\"]",
        ),
        6,
        13,
        "'aws' is not one of the stack's platforms (machines)",
    );
    assert_at(
        &format!("{DAEMON}\n[daemon.agent.secrets]\napi_key = {{}}\n"),
        7,
        1,
        "secret 'api_key' must be an environment variable name",
    );
    assert_at(
        &format!("{DAEMON}env = {{ API_KEY = \"x\" }}\n\n[daemon.agent.secrets]\nAPI_KEY = {{}}\n"),
        8,
        1,
        "'API_KEY' is both a secret and an environment variable",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.secrets]\nAPI_KEY = {{ label = \"Key\" }}\n\n[daemon.other]\nimage = \"x\"\n\n[daemon.other.secrets]\nAPI_KEY = {{}}\n"
        ),
        13,
        11,
        "secret 'API_KEY' is also declared on line 7 with different settings",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.endpoints]\napi = {{ port = 8080, host = \"@\" }}\nweb = {{ port = 8080, host = \"@\" }}\n"
        ),
        8,
        7,
        "only one endpoint can use host \"@\"; 'api' already does",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[daemon.agent.endpoints]\napi = {{ port = 8080 }}\nweb = {{ port = 9090 }}\n"
        ),
        8,
        7,
        "all endpoints of a daemon must use the same port",
    );
}

#[test]
fn input_errors() {
    let input = |body: &str| format!("{DAEMON}\n[input.region]\n{body}\n");
    assert_at(
        &input("kind = \"string\"\nlabel = \"Region\""),
        6,
        8,
        "a required input needs a `description`",
    );
    assert_at(
        &input("kind = \"integer\"\nrequired = false\ndefault = 1.5"),
        9,
        11,
        "1.5 is not a whole number",
    );
    assert_at(
        &input("kind = \"boolean\"\nrequired = false\ndefault = \"yes\""),
        9,
        11,
        "expected a boolean",
    );
    assert_at(
        &input("kind = \"number\"\nrequired = false\nmin-length = 2"),
        9,
        14,
        "`min-length` applies to string and secret inputs only",
    );
    assert_at(
        &input("kind = \"enum\"\nrequired = false"),
        6,
        8,
        "an enum input needs `values`",
    );
    assert_at(
        &input(
            "kind = \"string\"\nrequired = false\nenv = { name = \"REGION\", targets = [\"api\"] }",
        ),
        9,
        7,
        "unknown resource 'api' in `targets`",
    );
    assert_at(
        &input("kind = \"string\"\nrequired = false\nenv = \"region\""),
        9,
        7,
        "'region' must be an environment variable name",
    );
    assert_at(
        &format!("{DAEMON}\n[input.REGION]\nkind = \"string\"\nrequired = false\n"),
        6,
        8,
        "input ID 'REGION' must",
    );
    assert_at(
        &format!(
            "{DAEMON}\n[input.apiKey]\nkind = \"string\"\nrequired = false\n\n[daemon.agent.secrets]\nAPI_KEY = {{}}\n"
        ),
        11,
        1,
        "clashes with the input 'apiKey'",
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
fn host_access_cannot_have_a_cloud_identity() {
    let text =
        format!("{DAEMON}host-access = true\npermissions = \"agent\"\n\n[permissions.agent]\n");
    assert_at(
        &text.replace(
            "name = \"acme-agent\"",
            "name = \"acme-agent\"\nplatforms = [\"machines\", \"gcp\"]",
        ),
        7,
        15,
        "on gcp a daemon with `host-access` could use the machine's credentials",
    );
    let machines_only = text.replace(
        "name = \"acme-agent\"",
        "name = \"acme-agent\"\nplatforms = [\"machines\"]",
    );
    let stack = alien_stack_toml::parse(&machines_only, "alien.toml", Path::new("."))
        .expect("machines daemons get no cloud identity, so the pair is allowed");
    assert_eq!(stack.permissions.profiles.len(), 1);
}

#[test]
fn gate_errors() {
    let gate = |input: &str| {
        format!("{DAEMON}enabled-when = \"withAgent\"\n\n[input.withAgent]\n{input}\n")
    };
    assert_at(
        &format!("{DAEMON}enabled-when = \"withAgent\"\n"),
        5,
        16,
        "unknown input 'withAgent'; declare it with an [input.withAgent] table",
    );
    assert_at(
        &gate("kind = \"string\"\nprovided-by = \"deployer\"\ndescription = \"x\""),
        5,
        16,
        "input 'withAgent' must be a boolean",
    );
    assert_at(
        &gate("kind = \"boolean\"\ndescription = \"x\""),
        5,
        16,
        "input 'withAgent' must be provided by the deployer",
    );
    assert_at(
        &gate("kind = \"boolean\"\nprovided-by = \"deployer\"\nrequired = false"),
        5,
        16,
        "input 'withAgent' must be required or have a `default`",
    );
    assert_at(
        &format!("{DAEMON}platforms = [\"aws\"]\nenabled-when = \"withAgent\"\n"),
        6,
        16,
        "set `platforms` or `enabled-when`, not both",
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
fn errors_show_the_source_line() {
    let error = reject(&format!("{DAEMON}pool = \"gpu\"\n"));
    assert_eq!(error.snippet, "  |\n5 | pool = \"gpu\"\n  |        ^^^^^");
}
