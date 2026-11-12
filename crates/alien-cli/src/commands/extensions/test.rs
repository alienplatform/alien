//! `alien extensions test`: the extension's own tests, then the hooks crate's tests and its
//! importers natively and in the sandbox Alien runs hooks in.
//!
//! A hook runs sandboxed, with no files, network or environment, a clock at 0 and a seeded random
//! source, so a hook that passes natively can still answer differently there. The hooks crate's
//! tests are built for wasm32-wasip1 and each one runs in that sandbox, as a command, and its
//! result is compared with the native run's. The importers run on their fixtures natively, in a
//! harness that links the hooks crate, and in the module `alien extensions build` builds, and the
//! two outputs must be the same bytes. Each import whose type has a controller then loads into
//! the controller, which must read it back unchanged.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use alien_core::hooks::{HookKind, HooksManifest, ImportInput, ImportOutput};
use alien_core::{Platform, StackSettings};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_hooks_runtime::{CommandExit, CommandModule, Limits, Runtime, TestBinary};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{
    build_hooks_module, build_host_controller, controller_platforms, read_workspace,
    ExtensionWorkspace, HooksModule,
};
use crate::error::{ErrorData, Result};
use crate::ui::{dim_label, success_line};
use alien_infra::step_protocol::protocol::{ControllerView, LoadResult};
use alien_infra::step_protocol::StdioTransport;

/// Where an importer's fixtures live, under the workspace root: one directory per resource type
/// and platform, with one JSON file per fixture.
const FIXTURES_DIR: &str = "tests/import";

pub async fn test_task(directory: &Path) -> Result<()> {
    let workspace = read_workspace(directory)?;

    let status = Command::new("cargo")
        .args(["test", "--workspace"])
        .current_dir(&workspace.root)
        .status()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "could not run 'cargo test' in '{}'",
                workspace.root.display()
            ),
        })?;
    if !status.success() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!(
                "'cargo test --workspace' failed (exit code {})",
                status.code().unwrap_or(-1)
            ),
        }));
    }

    let tests = compare_test_runs(&workspace)?;
    if !tests.differences.is_empty() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: tests.differences.join("\n"),
        }));
    }

    let imports = check_imports(&workspace).await?;

    print_test_comparison(&workspace.hooks.name, &tests);
    println!(
        "{}",
        success_line(&format!(
            "The {} extension's {} hooks declare the same natively and in the module.",
            imports.extension, imports.hooks
        ))
    );
    for fixture in &imports.fixtures {
        println!(
            "{} {fixture} imports the same natively and in the module",
            dim_label("Importer fixture"),
        );
    }
    for fixture in &imports.loaded {
        println!(
            "{} {fixture} loads into its controller unchanged",
            dim_label("Importer fixture")
        );
    }
    Ok(())
}

/// How each test of the hooks crate came out natively and in the hooks sandbox.
#[derive(Debug, Default)]
struct TestComparison {
    /// Tests that ran both ways with the same result.
    sandboxed: usize,
    /// Tests that ran natively only, each with why the sandbox didn't run it.
    native_only: Vec<(String, String)>,
    /// Tests that passed in the sandbox and didn't run natively, each with why.
    sandbox_only: Vec<(String, String)>,
    /// Tests every build that has them ignores.
    ignored: usize,
    /// Tests that passed one way and failed the other, and tests that failed in the sandbox
    /// without a native run.
    differences: Vec<String>,
}

/// What libtest did with one test.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
    /// Not run, with libtest's reason when it printed one.
    Ignored(Option<String>),
}

/// Runs each test of the hooks crate natively and in the hooks sandbox, one test per run, and
/// compares the results. Only a test that passes one way and fails the other is a difference: a
/// test one build doesn't run has no result there to compare.
///
/// One test per run because a wasm32-wasip1 binary aborts on a panic: a failing test ends its
/// run, and the next test needs a run of its own.
fn compare_test_runs(workspace: &ExtensionWorkspace) -> Result<TestComparison> {
    let native_output = Command::new("cargo")
        .args(["test", "--no-run", "--message-format", "json", "--package"])
        .arg(&workspace.hooks.name)
        .current_dir(&workspace.root)
        .stderr(Stdio::inherit())
        .output()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not run 'cargo test --no-run'".to_string(),
        })?;
    if !native_output.status.success() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!("building {}'s tests failed", workspace.hooks.name),
        }));
    }
    let native = alien_hooks_runtime::test_binaries(&native_output.stdout);
    let modules = alien_hooks_runtime::build_test_modules(
        &workspace.root.join("Cargo.toml"),
        &workspace.hooks.name,
        &workspace.target_dir,
    )
    .map_err(|message| AlienError::new(ErrorData::ExtensionBuildFailed { message }))?;

    let runtime = Runtime::new().context(ErrorData::ExtensionTestFailed {
        message: "Alien's hooks runtime did not start".to_string(),
    })?;
    let mut comparison = TestComparison::default();
    for module in &modules {
        if !native
            .iter()
            .any(|binary| binary.target == module.target && binary.kinds == module.kinds)
        {
            let sandboxed = load_sandboxed(&runtime, module)?;
            for test in list_sandboxed(&sandboxed, module)? {
                comparison.run_in_sandbox_only(
                    &sandboxed,
                    module,
                    test,
                    "its target has no native tests".to_string(),
                );
            }
        }
    }
    for binary in &native {
        let module = modules
            .iter()
            .find(|module| module.target == binary.target && module.kinds == binary.kinds);
        let native_tests = list_native(workspace, binary)?;
        let Some(module) = module else {
            for test in native_tests {
                comparison
                    .native_only
                    .push((test, "its target has no wasm32-wasip1 tests".to_string()));
            }
            continue;
        };
        let sandboxed = load_sandboxed(&runtime, module)?;
        let module_tests = list_sandboxed(&sandboxed, module)?;
        for test in module_tests
            .iter()
            .filter(|test| !native_tests.contains(test))
        {
            comparison.run_in_sandbox_only(
                &sandboxed,
                module,
                test.clone(),
                "not in the native build".to_string(),
            );
        }
        for test in &native_tests {
            if !module_tests.contains(test) {
                comparison
                    .native_only
                    .push((test.clone(), "not in the wasm32-wasip1 build".to_string()));
                continue;
            }
            let native_outcome = run_native(workspace, binary, test)?;
            let output = sandboxed.run(&test_args(&module.target, test), &Limits::TEST_RUN);
            let sandbox_outcome =
                outcome(test, &output.stdout, output.exit == CommandExit::Code(0));
            match (&native_outcome, &sandbox_outcome) {
                (Outcome::Ignored(_), Outcome::Ignored(_)) => comparison.ignored += 1,
                (_, Outcome::Ignored(reason)) => comparison.native_only.push((
                    test.clone(),
                    reason
                        .clone()
                        .unwrap_or_else(|| "ignored in the wasm32-wasip1 build".to_string()),
                )),
                (Outcome::Ignored(reason), Outcome::Failed) => {
                    comparison.differences.push(format!(
                        "`{test}` is {} natively and fails in the sandbox Alien runs hooks in ({}):\n{}{}",
                        reason.as_deref().unwrap_or("ignored"),
                        describe(&output.exit),
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ))
                }
                (Outcome::Ignored(reason), Outcome::Passed) => comparison.sandbox_only.push((
                    test.clone(),
                    reason
                        .clone()
                        .unwrap_or_else(|| "ignored natively".to_string()),
                )),
                (native, sandbox) if native == sandbox => comparison.sandboxed += 1,
                (native, _) => {
                    let (native, sandbox) = if *native == Outcome::Passed {
                        ("passes", "fails")
                    } else {
                        ("fails", "passes")
                    };
                    comparison.differences.push(format!(
                        "`{test}` {native} natively and {sandbox} in the sandbox Alien runs hooks in ({}):\n{}{}",
                        describe(&output.exit),
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            }
        }
    }
    Ok(comparison)
}

impl TestComparison {
    /// Runs a test the native build doesn't run. Nothing natively compares with it, but it must
    /// pass.
    fn run_in_sandbox_only(
        &mut self,
        sandboxed: &CommandModule,
        module: &TestBinary,
        test: String,
        reason: String,
    ) {
        let output = sandboxed.run(&test_args(&module.target, &test), &Limits::TEST_RUN);
        match outcome(&test, &output.stdout, output.exit == CommandExit::Code(0)) {
            Outcome::Passed => self.sandbox_only.push((test, reason)),
            Outcome::Ignored(_) => self.ignored += 1,
            Outcome::Failed => self.differences.push(format!(
                "`{test}` ({reason}) fails in the sandbox Alien runs hooks in ({}):\n{}{}",
                describe(&output.exit),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )),
        }
    }
}

fn load_sandboxed(runtime: &Runtime, module: &TestBinary) -> Result<CommandModule> {
    let wasm = std::fs::read(&module.path)
        .into_alien_error()
        .context(file_error("read", &module.path))?;
    runtime
        .load_command(&wasm)
        .context(ErrorData::ExtensionTestFailed {
            message: format!(
                "the wasm32-wasip1 tests of `{}` did not load",
                module.target
            ),
        })
}

/// libtest's arguments to run `test` alone, on one thread, as the sandbox has no others. Its
/// output isn't captured, because a wasm32-wasip1 test that panics aborts before libtest would
/// print what it captured.
fn test_args<'a>(program: &'a str, test: &'a str) -> [&'a str; 7] {
    [
        program,
        "--exact",
        test,
        "--test-threads=1",
        "--nocapture",
        "--color",
        "never",
    ]
}

/// The test names in `--list --format terse` output.
fn test_names(listing: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(listing)
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_string)
        .collect()
}

/// What a run of `test` alone did: libtest prints `test <name> ... ignored[, <reason>]` for a
/// test it doesn't run, and otherwise the run's exit says whether it passed.
fn outcome(test: &str, stdout: &[u8], succeeded: bool) -> Outcome {
    let stdout = String::from_utf8_lossy(stdout);
    let name = format!("test {test}");
    for line in stdout.lines() {
        let Some(rest) = line.strip_prefix(&name) else {
            continue;
        };
        // libtest names a #[should_panic] test `<name> - should panic` in its result line.
        let should_panic = rest.starts_with(" - should panic");
        let rest = rest.strip_prefix(" - should panic").unwrap_or(rest);
        let Some(ignored) = rest.strip_prefix(" ... ignored") else {
            continue;
        };
        return Outcome::Ignored(match ignored.strip_prefix(", ") {
            Some(reason) => Some(format!("ignored: {reason}")),
            // wasm32-wasip1 aborts on a panic, so libtest can't catch the one the test expects.
            None if should_panic => Some(
                "a #[should_panic] test, which libtest doesn't run on wasm32-wasip1".to_string(),
            ),
            None => None,
        });
    }
    if succeeded {
        Outcome::Passed
    } else {
        Outcome::Failed
    }
}

fn list_native(workspace: &ExtensionWorkspace, binary: &TestBinary) -> Result<Vec<String>> {
    let output = Command::new(&binary.path)
        .args(["--list", "--format", "terse"])
        .current_dir(&workspace.hooks.dir)
        .output()
        .into_alien_error()
        .context(file_error("run", &binary.path))?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!(
                "listing the native tests of `{}` failed (exit code {}): {}",
                binary.target,
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr)
            ),
        }));
    }
    Ok(test_names(&output.stdout))
}

fn list_sandboxed(sandboxed: &CommandModule, module: &TestBinary) -> Result<Vec<String>> {
    let output = sandboxed.run(
        &[module.target.as_str(), "--list", "--format", "terse"],
        &Limits::TEST_RUN,
    );
    if output.exit != CommandExit::Code(0) {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!(
                "listing the wasm32-wasip1 tests of `{}` failed ({}): {}",
                module.target,
                describe(&output.exit),
                String::from_utf8_lossy(&output.stderr)
            ),
        }));
    }
    Ok(test_names(&output.stdout))
}

/// Runs one test natively, from the crate's directory as `cargo test` does.
fn run_native(workspace: &ExtensionWorkspace, binary: &TestBinary, test: &str) -> Result<Outcome> {
    let args = test_args("", test);
    let output = Command::new(&binary.path)
        .args(&args[1..])
        .current_dir(&workspace.hooks.dir)
        .output()
        .into_alien_error()
        .context(file_error("run", &binary.path))?;
    Ok(outcome(test, &output.stdout, output.status.success()))
}

fn describe(exit: &CommandExit) -> String {
    match exit {
        CommandExit::Code(code) => format!("exit code {code}"),
        CommandExit::Trapped(message) => format!("trapped: {message}"),
        CommandExit::DeadlineExceeded => format!(
            "stopped at the {} s deadline",
            Limits::TEST_RUN.deadline.as_secs()
        ),
        CommandExit::MemoryLimitExceeded(message) => format!("out of memory: {message}"),
    }
}

fn print_test_comparison(crate_name: &str, tests: &TestComparison) {
    if tests.sandboxed + tests.native_only.len() + tests.sandbox_only.len() + tests.ignored == 0 {
        println!("The {crate_name} crate has no tests.");
        return;
    }
    let mut counts = vec![
        format!("{} ran in the sandbox", tests.sandboxed),
        format!("{} natively only", tests.native_only.len()),
    ];
    if !tests.sandbox_only.is_empty() {
        counts.push(format!("{} in the sandbox only", tests.sandbox_only.len()));
    }
    if tests.ignored > 0 {
        counts.push(format!("{} ignored in every build", tests.ignored));
    }
    println!(
        "{}",
        success_line(&format!(
            "The {crate_name} tests give the same results natively and in the sandbox Alien runs hooks in: {}.",
            counts.join(", ")
        ))
    );
    for (test, reason) in &tests.native_only {
        println!("{} {test}: {reason}", dim_label("Natively only"));
    }
    for (test, reason) in &tests.sandbox_only {
        println!("{} {test}: {reason}", dim_label("Sandbox only"));
    }
}

/// What [`check_imports`] or [`check_shipped_imports`] checked.
pub(crate) struct ImportCheck {
    pub extension: String,
    /// How many hooks the module declares.
    pub hooks: usize,
    /// The importer fixtures whose import passed.
    pub fixtures: Vec<String>,
    /// Those of them whose type has a controller on their platform, which loaded unchanged.
    pub loaded: Vec<String>,
}

/// The controller binary to load imports into, and the platforms each type it runs is
/// registered on.
type Controller<'a> = (&'a Path, &'a indexmap::IndexMap<String, Vec<Platform>>);

/// The importer check `alien extensions test` runs: every importer has a fixture, each fixture
/// imports the same natively and in the module this builds, and the controller this builds reads
/// back what each import wrote, for a type with a controller on the fixture's platform.
pub(crate) async fn check_imports(workspace: &ExtensionWorkspace) -> Result<ImportCheck> {
    let module = build_hooks_module(workspace)?;
    let calls = importer_calls(&workspace.root, &module.manifest)?;
    let results = compare_hook_calls(workspace, &module, &calls)?;
    let controller = match &workspace.controller {
        Some(controller) => {
            let binary = build_host_controller(workspace, controller)?;
            let registered = controller_platforms(&binary)?;
            Some((binary, registered))
        }
        None => None,
    };
    finish_import_check(
        &module.manifest,
        &calls,
        &results,
        controller
            .as_ref()
            .map(|(binary, registered)| (binary.as_path(), registered)),
    )
    .await
}

/// The same check on what a bundle ships, for `alien release`: the importer fixtures under
/// `root` run in the module `wasm`, and each state loads into `controller`, the bundled binary
/// for this machine's target. It builds nothing, so the module that ships is the one checked.
pub(crate) async fn check_shipped_imports(
    root: &Path,
    wasm: &[u8],
    sha256: &str,
    controller: Option<Controller<'_>>,
) -> Result<ImportCheck> {
    let module = Runtime::new()
        .and_then(|runtime| runtime.load(wasm, sha256))
        .context(ErrorData::ExtensionTestFailed {
            message: "the bundle's hooks.wasm did not load in Alien's hooks runtime".to_string(),
        })?;
    let calls = importer_calls(root, &module.manifest)?;
    let results: Vec<Value> = calls
        .iter()
        .map(|call| {
            match module
                .call(
                    &call.hook,
                    call.input.to_string().as_bytes(),
                    &Limits::for_kind(call.kind),
                )
                .map_err(|error| error.to_string())
                .and_then(|output| String::from_utf8(output).map_err(|error| error.to_string()))
            {
                Ok(output) => json!({ "output": output }),
                Err(error) => json!({ "error": error }),
            }
        })
        .collect();
    finish_import_check(&module.manifest, &calls, &results, controller).await
}

/// Checks each import's result, then loads each state into `controller`.
async fn finish_import_check(
    manifest: &HooksManifest,
    calls: &[HookCall],
    results: &[Value],
    controller: Option<Controller<'_>>,
) -> Result<ImportCheck> {
    let problems: Vec<String> = calls
        .iter()
        .zip(results)
        .filter_map(|(call, result)| {
            check_import(result)
                .err()
                .map(|problem| format!("{}: {problem}", call.fixture))
        })
        .collect();
    if !problems.is_empty() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: problems.join("\n"),
        }));
    }
    let mut problems = Vec::new();
    let loaded = match controller {
        Some((binary, registered)) => {
            load_imports(binary, registered, calls, results, &mut problems).await
        }
        None => Vec::new(),
    };
    if !problems.is_empty() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: problems.join("\n"),
        }));
    }
    Ok(ImportCheck {
        extension: manifest.extension.clone(),
        hooks: manifest.hooks.len(),
        fixtures: calls.iter().map(|call| call.fixture.clone()).collect(),
        loaded,
    })
}

/// One hook call, made natively by the harness and in the module by this CLI.
struct HookCall {
    /// The fixture it comes from, relative to the workspace root.
    fixture: String,
    hook: String,
    kind: HookKind,
    resource_type: String,
    platform: String,
    input: Value,
}

/// An importer fixture: a resource's config in alien.ts, and what its setup template returned.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImporterFixture {
    resource: Map<String, Value>,
    import_data: Value,
}

/// A call per importer fixture. Every importer needs a fixture: without one, a field the
/// importer and the controller name differently reaches a customer unseen.
fn importer_calls(root: &Path, manifest: &HooksManifest) -> Result<Vec<HookCall>> {
    let mut calls = Vec::new();
    let mut problems = Vec::new();
    for hook in manifest
        .hooks
        .iter()
        .filter(|hook| hook.kind == HookKind::Importer)
    {
        let route = |key: &str| {
            hook.routing
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        let (resource_type, platform) = (route("resourceType"), route("platform"));
        let relative = Path::new(FIXTURES_DIR).join(resource_type).join(platform);
        let fixtures = json_files(&root.join(&relative))?;
        if fixtures.is_empty() {
            problems.push(format!(
                "the importer for {resource_type} on {platform} has no fixtures in {}/",
                relative.display()
            ));
        }
        for path in fixtures {
            let fixture = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            match import_input(&path, resource_type, platform) {
                Ok(input) => calls.push(HookCall {
                    fixture,
                    hook: hook.name.clone(),
                    kind: hook.kind,
                    resource_type: resource_type.to_string(),
                    platform: platform.to_string(),
                    input,
                }),
                Err(problem) => problems.push(format!("{fixture}: {problem}")),
            }
        }
    }
    if problems.is_empty() {
        Ok(calls)
    } else {
        Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: problems.join("\n"),
        }))
    }
}

/// The `.json` files in `dir`, sorted, or none when it doesn't exist.
fn json_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry
            .into_alien_error()
            .context(file_error("read", dir))?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// The importer's input for a fixture: the resource as a frozen stack entry, set up from the
/// template on `platform`.
fn import_input(
    path: &Path,
    resource_type: &str,
    platform: &str,
) -> std::result::Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let fixture: ImporterFixture = serde_json::from_str(&text).map_err(|error| {
        format!("{error}; a fixture holds `resource`, the resource's config in alien.ts, and `importData`")
    })?;
    let resource_id = fixture
        .resource
        .get("id")
        .and_then(Value::as_str)
        .ok_or("`resource` has no `id`")?
        .to_string();
    let mut config = fixture.resource;
    config.insert("type".to_string(), resource_type.into());
    let platform: Platform = serde_json::from_value(platform.into())
        .map_err(|_| format!("`{platform}` is not a platform"))?;
    serde_json::to_value(ImportInput {
        resource_id,
        platform,
        region: String::new(),
        stack_settings: StackSettings::default(),
        management_config: None,
        resource: json!({ "config": config, "lifecycle": "frozen", "dependencies": [] }),
        import_data: fixture.import_data,
    })
    .map_err(|error| error.to_string())
}

/// How long the controller may take to answer `load`, which reads the state and makes no call.
const LOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Sends each import whose type has a controller to that controller, as Alien does when it
/// imports a resource, and adds a problem for each import the controller doesn't read back
/// unchanged, and for one on a platform its type has no controller on. Returns the fixtures that
/// loaded.
async fn load_imports(
    controller: &Path,
    registered: &indexmap::IndexMap<String, Vec<Platform>>,
    calls: &[HookCall],
    results: &[Value],
    problems: &mut Vec<String>,
) -> Vec<String> {
    let transport = StdioTransport::new(controller.display().to_string(), Vec::new(), Vec::new());
    let mut loaded = Vec::new();
    for (call, result) in calls.iter().zip(results) {
        let found = match load_import(&transport, registered, call, result).await {
            Ok(Some(found)) => found,
            // The type has no controller anywhere: nothing reads the state it writes.
            Ok(None) => continue,
            Err(problem) => vec![problem],
        };
        if found.is_empty() {
            loaded.push(call.fixture.clone());
        }
        problems.extend(
            found
                .into_iter()
                .map(|problem| format!("{}: {problem}", call.fixture)),
        );
    }
    loaded
}

/// What differs when one import's state loads into the controller, or `None` for a type with no
/// controller on any platform.
async fn load_import(
    transport: &StdioTransport,
    registered: &indexmap::IndexMap<String, Vec<Platform>>,
    call: &HookCall,
    result: &Value,
) -> std::result::Result<Option<Vec<String>>, String> {
    let platform =
        serde_json::from_value::<Platform>(call.platform.as_str().into()).map_err(|_| {
            format!(
                "the importer's platform `{}` is not one Alien knows",
                call.platform
            )
        })?;
    let platforms = registered
        .get(&call.resource_type)
        .filter(|platforms| !platforms.is_empty());
    let Some(platforms) = platforms else {
        return Ok(None);
    };
    if !platforms.contains(&platform) {
        return Err(format!(
            "{} has a controller on {}, and none on {platform}, where this importer writes the controller's state",
            call.resource_type,
            platforms
                .iter()
                .map(|platform| platform.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let output = result["output"]
        .as_str()
        .ok_or_else(|| "the importer returned no output".to_string())?;
    let imported = serde_json::from_str::<ImportOutput>(output)
        .map_err(|error| format!("the importer's output did not read: {error}"))?;
    Ok(Some(
        match transport
            .load(
                &call.resource_type,
                platform,
                imported.internal_state.clone(),
                LOAD_TIMEOUT,
            )
            .await
        {
            Err(failure) => vec![format!(
                "the controller didn't answer `load` ({}): {}",
                failure.code, failure.message
            )],
            Ok(LoadResult::Refused(message)) => vec![format!(
                "the controller can't load the state the importer wrote: {message}"
            )],
            Ok(LoadResult::View(view)) => compare_load(&imported, &view),
        },
    ))
}

/// The keys of a controller's saved state that the controller, not the importer, owns.
const CONTROLLER_STATE_KEYS: [&str; 3] = ["type", "_controllerStateVersion", "internalStayCount"];

/// What differs between the state an importer wrote and the controller's view of it: a key the
/// controller dropped or the importer didn't write, a value the controller changed, and a status
/// or outputs the two report differently.
fn compare_load(imported: &ImportOutput, view: &ControllerView) -> Vec<String> {
    let mut problems = Vec::new();
    let empty = Map::new();
    // A side that isn't an object would compare as empty and pass; it is a problem itself.
    if !imported.internal_state.is_object() {
        problems.push(format!(
            "the importer wrote {} where the controller's fields belong, not a JSON object",
            imported.internal_state
        ));
    }
    if !view.state.is_object() {
        problems.push(format!(
            "the controller's state is {}, not a JSON object",
            view.state
        ));
    }
    let wrote = imported.internal_state.as_object().unwrap_or(&empty);
    let read = view.state.as_object().unwrap_or(&empty);
    let read: Map<String, Value> = read
        .iter()
        .filter(|(key, _)| !CONTROLLER_STATE_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    for (key, value) in wrote {
        match read.get(key) {
            None => problems.push(format!(
                "the importer wrote `{key}`, which the controller doesn't read"
            )),
            Some(read) if read != value => problems.push(format!(
                "the controller reads `{key}` as {read}, and the importer wrote {value}"
            )),
            Some(_) => {}
        }
    }
    for (key, value) in &read {
        if !wrote.contains_key(key) {
            problems.push(format!(
                "the importer didn't write `{key}`; the controller starts with it as {value}"
            ));
        }
    }
    if view.status != imported.status {
        problems.push(format!(
            "the importer reports the status {:?}, and the controller {:?}",
            imported.status, view.status
        ));
    }
    let view_outputs = view.outputs.as_ref().and_then(|outputs| {
        let mut value = serde_json::to_value(outputs).ok()?;
        if let Value::Object(fields) = &mut value {
            fields.remove("type");
        }
        Some(value)
    });
    if view_outputs != imported.outputs {
        problems.push(format!(
            "the importer reports the outputs {}, and the controller {}",
            imported.outputs.clone().unwrap_or(Value::Null),
            view_outputs.unwrap_or(Value::Null)
        ));
    }
    problems
}

/// Checks one import: the same output natively and in the module, and outputs a linked workload
/// can read.
fn check_import(result: &Value) -> std::result::Result<(), String> {
    if let Some(output) = result["output"].as_str() {
        let output: ImportOutput = serde_json::from_str(output)
            .map_err(|error| format!("the importer's output did not read: {error}"))?;
        return match output.outputs {
            Some(_) => Ok(()),
            None => Err("the importer reported no outputs".to_string()),
        };
    }
    if let Some(differ) = result.get("differ") {
        return Err(format!(
            "the importer gives different results natively and in the module:\n  native: {}\n  module: {}",
            differ["native"], differ["module"]
        ));
    }
    Err(format!("the importer failed: {}", result["error"]))
}

/// The harness's source. It links the hooks crate natively, and prints the hooks it declares and
/// what each call returns. It needs only crates the extension's graph already has, so it builds
/// from the extension's lockfile with no new download.
const HARNESS_MAIN: &str = r#"use serde_json::{json, Value};

fn main() {
    let request: Value = serde_json::from_reader(std::io::stdin()).expect("the request reads");
    let extension = hooks::extension();
    let report = match extension.manifest() {
        Err(error) => json!({ "error": error }),
        Ok(manifest) => {
            let results: Vec<Value> = request["calls"]
                .as_array()
                .expect("a list of calls")
                .iter()
                .map(|call| {
                    let hook = call["hook"].as_str().expect("a hook name");
                    let input = call["input"].as_str().expect("an input");
                    match extension.call(hook, input.as_bytes()) {
                        Ok(output) => json!({ "output": String::from_utf8_lossy(&output) }),
                        Err(error) => json!({ "error": error }),
                    }
                })
                .collect();
            json!({ "manifest": manifest, "results": results })
        }
    };
    println!("{report}");
}
"#;

/// Runs each call natively, through the harness, and in the module with the limits Alien gives
/// its kind, and returns one result per call: the output when both give the same bytes, the two
/// sides when they differ, or the errors when both fail.
fn compare_hook_calls(
    workspace: &ExtensionWorkspace,
    module: &HooksModule,
    calls: &[HookCall],
) -> Result<Vec<Value>> {
    let inputs: Vec<String> = calls.iter().map(|call| call.input.to_string()).collect();
    let native = run_harness(workspace, calls, &inputs)?;
    let native_manifest: HooksManifest = serde_json::from_value(native["manifest"].clone())
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "the test harness printed a manifest this CLI can't read".to_string(),
        })?;
    if native_manifest != module.manifest {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!(
                "the module declares other hooks than the crate does natively:\n  native: {}\n  module: {}",
                native["manifest"],
                serde_json::to_string(&module.manifest).unwrap_or_default()
            ),
        }));
    }
    let native_results = native["results"].as_array().cloned().unwrap_or_default();
    if native_results.len() != calls.len() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!(
                "the test harness answered {} of {} calls",
                native_results.len(),
                calls.len()
            ),
        }));
    }
    let side = |result: &std::result::Result<String, String>| match result {
        Ok(output) => json!({ "output": output }),
        Err(error) => json!({ "error": error }),
    };
    Ok(calls
        .iter()
        .zip(&inputs)
        .zip(&native_results)
        .map(|((call, input), native)| {
            let native = match native["output"].as_str() {
                Some(output) => Ok(output.to_string()),
                None => Err(native["error"].as_str().unwrap_or_default().to_string()),
            };
            let sandboxed = module
                .module
                .call(&call.hook, input.as_bytes(), &Limits::for_kind(call.kind))
                .map_err(|error| error.to_string())
                .and_then(|output| String::from_utf8(output).map_err(|error| error.to_string()));
            match (&native, &sandboxed) {
                (Ok(native), Ok(sandboxed)) if native == sandboxed => json!({ "output": native }),
                (Err(native), Err(sandboxed)) => {
                    json!({ "error": native, "moduleError": sandboxed })
                }
                _ => json!({ "differ": { "native": side(&native), "module": side(&sandboxed) } }),
            }
        })
        .collect())
}

/// Builds and runs the harness on the calls, and returns its report.
///
/// One harness per hooks package, in the target directory: each run rewrites it, so a workspace
/// tested again reuses its build.
fn run_harness(
    workspace: &ExtensionWorkspace,
    calls: &[HookCall],
    inputs: &[String],
) -> Result<Value> {
    let sdk = workspace.sdk_dependency.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "the hooks crate {} does not depend on alien-extension-sdk",
                workspace.hooks.name
            ),
        })
    })?;
    let package = format!("alien-extension-harness-{}", workspace.hooks.name);
    let dir = workspace.target_dir.join(&package);
    // Two runs on copies of one extension in a shared target directory share this harness, so
    // the second waits until the first is done with it.
    std::fs::create_dir_all(&dir)
        .into_alien_error()
        .context(file_error("create", &dir))?;
    let lock_path = dir.join("harness.lock");
    let lock = std::fs::File::create(&lock_path)
        .into_alien_error()
        .context(file_error("create", &lock_path))?;
    lock.lock()
        .into_alien_error()
        .context(file_error("lock", &lock_path))?;
    let quoted = |value: &str| toml::Value::String(value.to_string()).to_string();
    write_file(
        &dir.join("Cargo.toml"),
        format!(
            r#"# Written by `alien extensions test`.
[workspace]

[package]
name = {name}
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
hooks = {{ package = {hooks}, path = {path} }}
alien-extension-sdk = {sdk}
serde_json = "1"
"#,
            name = quoted(&package),
            hooks = quoted(&workspace.hooks.name),
            path = quoted(&workspace.hooks.dir.display().to_string()),
        )
        .as_bytes(),
    )?;
    write_file(&dir.join("src/main.rs"), HARNESS_MAIN.as_bytes())?;
    // From the extension's lockfile, the dependencies the harness shares with the extension
    // resolve to the versions the extension locked.
    let lock = workspace.root.join("Cargo.lock");
    std::fs::copy(&lock, dir.join("Cargo.lock"))
        .into_alien_error()
        .context(file_error("copy", &lock))?;

    let request = json!({
        "calls": calls
            .iter()
            .zip(inputs)
            .map(|(call, input)| json!({ "hook": call.hook, "input": input }))
            .collect::<Vec<_>>(),
    });
    // Run from the workspace root, so the harness builds with the extension's toolchain, into its
    // target directory.
    let mut child = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&workspace.target_dir)
        .current_dir(&workspace.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not run the test harness with cargo".to_string(),
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(request.to_string().as_bytes())
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "could not send the test harness its calls".to_string(),
            })?;
    }
    let output =
        child
            .wait_with_output()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "the test harness did not finish".to_string(),
            })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: format!(
                "the test harness failed (exit code {}); its output is above",
                output.status.code().unwrap_or(-1)
            ),
        }));
    }
    let report: Value = serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "the test harness printed a report this CLI can't read".to_string(),
        })?;
    if let Some(error) = report["error"].as_str() {
        return Err(AlienError::new(ErrorData::ExtensionTestFailed {
            message: error.to_string(),
        }));
    }
    Ok(report)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .into_alien_error()
            .context(file_error("create", parent))?;
    }
    std::fs::write(path, bytes)
        .into_alien_error()
        .context(file_error("write", path))
}

fn file_error(operation: &str, path: &Path) -> ErrorData {
    ErrorData::FileOperationFailed {
        operation: operation.to_string(),
        file_path: path.display().to_string(),
        reason: format!("could not {operation} it"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::hooks::HookEntry;

    fn importer(resource_type: &str, platform: &str) -> HooksManifest {
        let mut routing = Map::new();
        routing.insert("resourceType".to_string(), resource_type.into());
        routing.insert("platform".to_string(), platform.into());
        HooksManifest {
            extension: "budget".to_string(),
            hooks: vec![HookEntry {
                name: format!("importer/{resource_type}/{platform}"),
                kind: HookKind::Importer,
                routing,
            }],
            resource_types: Default::default(),
        }
    }

    fn view(
        state: Value,
        status: alien_core::ResourceStatus,
        zone_id: Option<&str>,
    ) -> ControllerView {
        use alien_infra::step_protocol::protocol::TransitionPreview;
        let preview = TransitionPreview::Allowed { status };
        ControllerView {
            state,
            status,
            progress: None,
            outputs: zone_id.map(|zone_id| {
                alien_core::ResourceOutputs::new(
                    alien_core::CustomResourceOutputs::from_untagged(
                        "acme/dns-zone",
                        json!({ "zoneId": zone_id }),
                    )
                    .unwrap(),
                )
            }),
            binding_params: None,
            on_failure_status: status,
            update_start: preview.clone(),
            delete_start: preview.clone(),
            teardown_start: preview,
        }
    }

    fn imported(state: Value) -> ImportOutput {
        ImportOutput {
            internal_state: state,
            status: alien_core::ResourceStatus::Running,
            outputs: Some(json!({ "zoneId": "Z123" })),
        }
    }

    /// An importer on a platform its type's controller doesn't run on writes a state nothing
    /// reads, which the check reports instead of skipping; a type with no controller anywhere
    /// loads nothing.
    #[tokio::test]
    async fn an_import_on_a_platform_with_no_controller_is_reported() {
        let call = HookCall {
            fixture: "tests/import/acme/dns-zone/azure/basic.json".to_string(),
            hook: "importer".to_string(),
            kind: HookKind::Importer,
            resource_type: "acme/dns-zone".to_string(),
            platform: "azure".to_string(),
            input: Value::Null,
        };
        let transport = StdioTransport::new("/nonexistent/controller", Vec::new(), Vec::new());
        let mut registered = indexmap::IndexMap::new();
        registered.insert(
            "acme/dns-zone".to_string(),
            vec![Platform::Aws, Platform::Gcp],
        );
        let problem = load_import(&transport, &registered, &call, &Value::Null)
            .await
            .unwrap_err();
        assert_eq!(
            problem,
            "acme/dns-zone has a controller on aws, gcp, and none on azure, where this importer writes the controller's state"
        );
        registered.insert("acme/dns-zone".to_string(), Vec::new());
        assert_eq!(
            load_import(&transport, &registered, &call, &Value::Null).await,
            Ok(None)
        );
        let unknown = HookCall {
            platform: "mars".to_string(),
            ..call
        };
        assert!(load_import(&transport, &registered, &unknown, &Value::Null)
            .await
            .unwrap_err()
            .contains("`mars` is not one Alien knows"));
    }

    #[test]
    fn an_import_the_controller_reads_back_unchanged_passes() {
        let state = json!({ "state": "ready", "zoneId": "Z123" });
        let mut read = state.clone();
        read["type"] = json!("acme/dns-zone");
        read["_controllerStateVersion"] = json!(1);
        let view = view(read, alien_core::ResourceStatus::Running, Some("Z123"));
        assert!(compare_load(&imported(state), &view).is_empty());
    }

    // The importer names the field `hostedZoneId`, the controller `zoneId`.
    #[test]
    fn a_renamed_field_fails() {
        let view = view(
            json!({ "state": "ready", "zoneId": null }),
            alien_core::ResourceStatus::Running,
            None,
        );
        let problems = compare_load(
            &imported(json!({ "state": "ready", "hostedZoneId": "Z123" })),
            &view,
        );
        let problems = problems.join("\n");
        assert!(
            problems.contains("wrote `hostedZoneId`, which the controller doesn't read"),
            "{problems}"
        );
        assert!(problems.contains("didn't write `zoneId`"), "{problems}");
        assert!(problems.contains("outputs"), "{problems}");
    }

    /// Fields that aren't a JSON object would compare as none on either side, and pass.
    #[test]
    fn fields_that_are_not_an_object_fail() {
        let view = view(json!(null), alien_core::ResourceStatus::Running, None);
        let problems = compare_load(&imported(json!("Z123")), &view).join("\n");
        assert!(problems.contains("not a JSON object"), "{problems}");
        assert!(
            problems.contains("the controller's state is null"),
            "{problems}"
        );
    }

    #[test]
    fn a_changed_value_or_status_fails() {
        let view = view(
            json!({ "state": "createStart", "zoneId": "Z123" }),
            alien_core::ResourceStatus::Provisioning,
            Some("Z123"),
        );
        let problems = compare_load(
            &imported(json!({ "state": "ready", "zoneId": "Z123" })),
            &view,
        )
        .join("\n");
        assert!(
            problems.contains("reads `state` as \"createStart\""),
            "{problems}"
        );
        assert!(problems.contains("status"), "{problems}");
    }

    #[test]
    fn reads_what_libtest_did_with_a_test() {
        assert_eq!(
            outcome("a", b"running 1 test\ntest a ... ok\n", true),
            Outcome::Passed
        );
        assert_eq!(
            outcome("a", b"running 1 test\ntest a ... ", false),
            Outcome::Failed
        );
        assert_eq!(
            outcome("a", b"test a ... ignored\n", true),
            Outcome::Ignored(None)
        );
        assert_eq!(
            outcome("a", b"test a ... ignored, needs a cloud account\n", true),
            Outcome::Ignored(Some("ignored: needs a cloud account".to_string()))
        );
        assert_eq!(
            outcome("a", b"test a - should panic ... ignored\n", true),
            Outcome::Ignored(Some(
                "a #[should_panic] test, which libtest doesn't run on wasm32-wasip1".to_string()
            ))
        );
        // Another test's line doesn't count.
        assert_eq!(
            outcome("a", b"test ab ... ignored\n", true),
            Outcome::Passed
        );
    }

    #[test]
    fn reads_the_test_names_libtest_lists() {
        let listing =
            b"aws::adds_tags_after_alien_tags: test\nreads_a_file: test\nbench_it: bench\n";
        assert_eq!(
            test_names(listing),
            vec![
                "aws::adds_tags_after_alien_tags".to_string(),
                "reads_a_file".to_string()
            ]
        );
    }

    #[test]
    fn an_importer_without_fixtures_fails() {
        let root = tempfile::tempdir().unwrap();
        let error = importer_calls(root.path(), &importer("budget/alert", "aws"))
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("has no fixtures in tests/import/budget/alert/aws/"),
            "{error}"
        );
    }

    #[test]
    fn a_fixture_becomes_the_importers_input() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("tests/import/budget/alert/aws");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("basic.json"),
            r#"{ "resource": { "id": "costs", "monthlyLimitUsd": 500 }, "importData": { "budgetName": "b" } }"#,
        )
        .unwrap();
        let calls = importer_calls(root.path(), &importer("budget/alert", "aws")).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].fixture, "tests/import/budget/alert/aws/basic.json");
        assert_eq!(calls[0].hook, "importer/budget/alert/aws");
        let input: ImportInput = serde_json::from_value(calls[0].input.clone()).unwrap();
        assert_eq!(input.resource_id, "costs");
        assert_eq!(input.platform, Platform::Aws);
        assert_eq!(input.import_data, json!({ "budgetName": "b" }));
        assert_eq!(
            input.resource,
            json!({
                "config": { "id": "costs", "monthlyLimitUsd": 500, "type": "budget/alert" },
                "lifecycle": "frozen",
                "dependencies": []
            })
        );
    }

    #[test]
    fn a_fixture_needs_the_resource_id() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("tests/import/budget/alert/aws");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("no-id.json"),
            r#"{ "resource": {}, "importData": {} }"#,
        )
        .unwrap();
        let error = importer_calls(root.path(), &importer("budget/alert", "aws"))
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("`resource` has no `id`"),
            "{error}"
        );
    }

    #[test]
    fn a_difference_between_native_and_module_fails() {
        let same = json!({ "output": r#"{"internalState":{"state":"ready"},"status":"running","outputs":{"budgetName":"b"}}"# });
        check_import(&same).unwrap();
        let differ =
            json!({ "differ": { "native": { "output": "a" }, "module": { "output": "b" } } });
        let problem = check_import(&differ).unwrap_err();
        assert!(problem.contains("natively and in the module"), "{problem}");
        let no_outputs =
            json!({ "output": r#"{"internalState":{},"status":"running","outputs":null}"# });
        assert!(check_import(&no_outputs)
            .unwrap_err()
            .contains("no outputs"));
    }
}
