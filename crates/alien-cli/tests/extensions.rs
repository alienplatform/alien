//! `alien extensions build` and `alien extensions test` on the repository's extension examples.
//! Each test copies an example out of `examples/extensions/`, points it at this repository's SDK,
//! and runs the CLI in the copy.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// A copy of `examples/extensions/<name>` whose SDK dependency names this repository's SDK by its
/// absolute path, with the repository's toolchain file.
///
/// The copy lives outside the repository, as an extension does: cargo would otherwise read the
/// hooks crate's inherited dependencies from the repository's workspace, which the SDK's path
/// dependency loads.
fn example(name: &str) -> tempfile::TempDir {
    let copy = tempfile::Builder::new().prefix(name).tempdir().unwrap();
    copy_example(name, copy.path());
    copy
}

/// [`example`], into `to`.
fn copy_example(name: &str, to: &Path) {
    copy_dir(&repository().join("examples/extensions").join(name), to);
    std::fs::copy(
        repository().join("rust-toolchain.toml"),
        to.join("rust-toolchain.toml"),
    )
    .unwrap();
    let manifest = to.join("Cargo.toml");
    let sdk = repository().join("crates/alien-extension-sdk");
    let text = std::fs::read_to_string(&manifest).unwrap().replace(
        r#"path = "../../../crates/alien-extension-sdk""#,
        &format!("path = {:?}", sdk.display().to_string()),
    );
    std::fs::write(&manifest, text).unwrap();
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "target" || name == "generated" {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to.join(&name));
        } else {
            // Written afresh, so the copy is newer than whatever an earlier run left in the
            // target directory, and cargo rebuilds it.
            std::fs::write(to.join(&name), std::fs::read(entry.path()).unwrap()).unwrap();
        }
    }
}

/// The target directory the tests share, so each example's dependencies build once.
///
/// Copies of one example are the same packages to cargo, which decides a build is fresh by file
/// times, so a copy can get a build of another copy's sources. A test that changes a copy's
/// sources therefore uses an example no other test runs, or a change that can't alter another
/// test's result.
fn shared_target_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("extension-examples")
}

/// Runs the CLI in `directory`.
fn alien(directory: &Path, args: &[&str]) -> Output {
    command(directory, args).output().unwrap()
}

fn command(directory: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .args(args)
        .current_dir(directory)
        .env("CARGO_TARGET_DIR", shared_target_dir())
        .env_remove("RUST_LOG");
    command
}

/// A Cargo home in a directory of its own, whose registry is this machine's, so a build reads
/// the same dependency sources from another path.
#[cfg(unix)]
fn other_cargo_home() -> tempfile::TempDir {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo"));
    let other = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(cargo_home.join("registry"), other.path().join("registry")).unwrap();
    other
}

fn succeeds(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Adds a controller crate to an example: one binary that registers no controller.
fn add_controller(root: &Path) {
    std::fs::create_dir_all(root.join("controller/src")).unwrap();
    std::fs::write(
        root.join("controller/Cargo.toml"),
        "[package]\nname = \"backups-controller\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n",
    )
    .unwrap();
    // `--controllers` lists no types: the examples' types have no controller.
    std::fs::write(
        root.join("controller/src/main.rs"),
        "fn main() {\n    if std::env::args().any(|arg| arg == \"--controllers\") {\n        println!(\"{{}}\");\n    }\n}\n",
    )
    .unwrap();
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace(
            r#"members = ["hooks"]"#,
            r#"members = ["hooks", "controller"]"#,
        )
        .replace(
            "hooks = \"hooks\"\n",
            "hooks = \"hooks\"\ncontroller = \"controller\"\n",
        );
    std::fs::write(&manifest, text).unwrap();
}

fn zip_entry(zip: &[u8], name: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

/// Every file a build wrote, by its path under the example.
fn build_output(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("target/alien-extension"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.push(root.join("generated/index.ts"));
    files.sort();
    files
        .into_iter()
        .map(|file| {
            let bytes = std::fs::read(&file).unwrap();
            (file.strip_prefix(root).unwrap().to_path_buf(), bytes)
        })
        .collect()
}

// The bundle lists each controller target it built and `null` for the rest. The build names its
// target: without one it builds what this machine can build, which changes while another test
// installs this host's Rust target.
#[test]
fn build_writes_the_bundle_and_the_module_the_same_every_time() {
    let example = example("postgres-backup-retention");
    add_controller(example.path());
    let host = alien_core::BinaryTarget::current_os().to_string();
    let build = ["extensions", "build", "--target", host.as_str()];

    succeeds(alien(example.path(), &build));
    let first = build_output(example.path());
    let file = |name: &str| {
        first
            .iter()
            .find(|(path, _)| path == Path::new(name))
            .map(|(_, bytes)| bytes.as_slice())
    };
    let zip = file("target/alien-extension/backups-core.zip").unwrap();

    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_entry(zip, "manifest.json")).unwrap();
    assert_eq!(manifest["name"], "backups");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(
        manifest["protocol"],
        serde_json::json!({ "hooks": 1, "controller": 1 })
    );
    let hooks = zip_entry(zip, "hooks.wasm");
    assert_eq!(manifest["hooksSha256"], hex::encode(Sha256::digest(&hooks)));
    let mut binaries = 0;
    for target in ["windows-x64", "linux-x64", "linux-arm64", "darwin-arm64"] {
        let binary = file(&format!(
            "target/alien-extension/backups-controller-{target}"
        ));
        let expected = match binary {
            Some(bytes) => {
                binaries += 1;
                serde_json::json!({
                    "sha256": hex::encode(Sha256::digest(bytes)),
                    "size": bytes.len(),
                })
            }
            None => serde_json::Value::Null,
        };
        assert_eq!(manifest["controllers"][target], expected, "{target}");
    }
    // The zip, the binaries and the generated module.
    assert_eq!(first.len(), binaries + 2);

    let index_ts = std::str::from_utf8(file("generated/index.ts").unwrap()).unwrap();
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(zip)));
    assert!(
        index_ts.contains(&format!(r#""digest": "{digest}""#)),
        "{index_ts}"
    );
    assert!(index_ts.contains("export default extension"), "{index_ts}");

    succeeds(alien(example.path(), &build));
    assert_eq!(build_output(example.path()), first);
}

// The module's bytes, and so the bundle's digest, don't depend on where the extension lives,
// where it builds or where Cargo keeps its dependencies' sources: two copies in two directories,
// with two target directories and two Cargo homes, give one digest.
#[cfg(unix)]
#[test]
fn the_bundle_is_the_same_wherever_the_extension_builds() {
    let other_cargo_home = other_cargo_home();
    let other_target_dir = tempfile::tempdir().unwrap();
    let places = [
        (shared_target_dir(), None),
        (
            other_target_dir.path().to_path_buf(),
            Some(other_cargo_home.path()),
        ),
    ];
    let bundles: Vec<Vec<u8>> = places
        .into_iter()
        .map(|(target_dir, cargo_home)| {
            let example = example("postgres-backup-retention");
            let mut build = command(example.path(), &["extensions", "build"]);
            build.env("CARGO_TARGET_DIR", target_dir);
            if let Some(cargo_home) = cargo_home {
                build.env("CARGO_HOME", cargo_home);
            }
            succeeds(build.output().unwrap());
            std::fs::read(
                example
                    .path()
                    .join("target/alien-extension/backups-core.zip"),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(
        hex::encode(Sha256::digest(&bundles[0])),
        hex::encode(Sha256::digest(&bundles[1]))
    );
}

#[test]
fn test_fails_when_a_test_passes_natively_and_fails_in_the_sandbox() {
    let example = example("resource-tags");
    let output = succeeds(alien(example.path(), &["extensions", "test"]));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "tests give the same results natively and in the sandbox Alien runs hooks in: 1 ran in the sandbox, 0 natively only."
        ),
        "{stdout}"
    );

    // Tests one build doesn't run have no result there: they are listed, and nothing fails.
    let hooks = example.path().join("hooks/src/lib.rs");
    let mut source = std::fs::read_to_string(&hooks).unwrap();
    source.push_str(
        r#"
#[test]
#[should_panic]
fn should_panic_test() {
    panic!("expected");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_build_only() {}

#[test]
#[cfg_attr(target_family = "wasm", ignore = "reads the host's clock")]
fn ignored_on_wasm() {}

#[cfg(target_arch = "wasm32")]
#[test]
fn wasm_build_only() {}

#[test]
#[ignore]
fn ignored_everywhere() {}
"#,
    );
    std::fs::write(&hooks, &source).unwrap();
    let output = succeeds(alien(example.path(), &["extensions", "test"]));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in [
        "1 ran in the sandbox, 3 natively only, 1 in the sandbox only, 1 ignored in every build.",
        "should_panic_test: a #[should_panic] test, which libtest doesn't run on wasm32-wasip1",
        "native_build_only: not in the wasm32-wasip1 build",
        "ignored_on_wasm: ignored: reads the host's clock",
        "wasm_build_only: not in the native build",
    ] {
        assert!(stdout.contains(line), "{line}\n{stdout}");
    }

    // The sandbox has no files, so a test that reads one fails there.
    source.push_str(
        r#"
#[test]
fn reads_a_file() {
    assert!(std::fs::read_to_string("Cargo.toml").unwrap().contains("tags-hooks"));
}
"#,
    );
    std::fs::write(&hooks, source).unwrap();

    let output = alien(example.path(), &["extensions", "test"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("`reads_a_file` passes natively and fails in the sandbox"),
        "{stderr}"
    );
    assert!(stderr.contains("No such file or directory"), "{stderr}");
}

/// One copy for both changes: a copy of an example another test changes may get that test's
/// build, so the changes run in turn here.
#[test]
fn test_fails_when_an_importer_answers_differently_in_the_module_or_fails() {
    let example = example("budget-alert");
    let output = succeeds(alien(example.path(), &["extensions", "test"]));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("tests/import/budget/alert/aws/basic.json imports the same"),
        "{stdout}"
    );

    // The module's answer, and only the module's, changes.
    let hooks = example.path().join("hooks/src/lib.rs");
    let source = std::fs::read_to_string(&hooks).unwrap().replace(
        "ctx.ready((), BudgetAlertOutputs { budget_name: data.budget_name })",
        r#"let budget_name = if cfg!(target_arch = "wasm32") { format!("{}-module", data.budget_name) } else { data.budget_name };
        ctx.ready((), BudgetAlertOutputs { budget_name })"#,
    );
    assert!(source.contains("-module"), "the importer's line moved");
    std::fs::write(&hooks, source).unwrap();

    let output = alien(example.path(), &["extensions", "test"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the importer gives different results natively and in the module"),
        "{stderr}"
    );
    assert!(stderr.contains("my-app-costs-module"), "{stderr}");

    // The importer fails, natively and in the module: both errors are shown.
    let source = std::fs::read_to_string(&hooks).unwrap().replace(
        r#"let budget_name = if cfg!(target_arch = "wasm32") { format!("{}-module", data.budget_name) } else { data.budget_name };
        ctx.ready((), BudgetAlertOutputs { budget_name })"#,
        r#"let _ = (data, ctx);
        Err(AlienError::new(ErrorData::GenericError { message: "budget names starting with my- are reserved".into() }))"#,
    );
    assert!(source.contains("are reserved"), "the importer's line moved");
    std::fs::write(&hooks, source).unwrap();

    let output = alien(example.path(), &["extensions", "test"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("native: budget names starting with my- are reserved"),
        "{stderr}"
    );
    assert!(
        stderr.contains("module: ") && stderr.matches("are reserved").count() >= 2,
        "{stderr}"
    );
}

#[test]
fn test_fails_on_an_importer_without_fixtures() {
    let example = example("budget-alert");
    std::fs::remove_dir_all(example.path().join("tests/import")).unwrap();
    let output = alien(example.path(), &["extensions", "test"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the importer for budget/alert on aws has no fixtures in tests/import/budget/alert/aws/"),
        "{stderr}"
    );
}

/// An app folder whose alien.ts is the docs' Quickstart stack with a bucket in place of the
/// database, which the OSS CLI emits on every format. `node_modules/@alienplatform/core` is this
/// repository's package, built when it has no build yet.
#[cfg(unix)]
fn app(imports: &[(&str, &str)]) -> tempfile::TempDir {
    let app = tempfile::Builder::new().prefix("my-app").tempdir().unwrap();
    app_at(app.path(), imports);
    app
}

/// [`app`], in `dir`.
#[cfg(unix)]
fn app_at(dir: &Path, imports: &[(&str, &str)]) {
    let core = repository().join("packages/core");
    if !core.join("dist").exists() {
        let built = Command::new("pnpm")
            .args(["--filter", "@alienplatform/core", "build"])
            .current_dir(repository())
            .output()
            .unwrap();
        assert!(built.status.success(), "{}", output_text(&built));
    }
    std::fs::create_dir_all(dir.join("node_modules/@alienplatform")).unwrap();
    std::os::unix::fs::symlink(core, dir.join("node_modules/@alienplatform/core")).unwrap();
    write_alien_ts(dir, imports);
}

/// Writes `<dir>/alien.ts`, which imports each `(name, specifier)` and lists them in that order.
fn write_alien_ts(dir: &Path, imports: &[(&str, &str)]) {
    let mut source = String::from("import * as alien from \"@alienplatform/core\"\n");
    for (name, specifier) in imports {
        source.push_str(&format!("import {name} from {specifier:?}\n"));
    }
    let names: Vec<&str> = imports.iter().map(|(name, _)| *name).collect();
    let extensions = if names.is_empty() {
        String::new()
    } else {
        format!("  .extensions([{}])\n", names.join(", "))
    };
    source.push_str(&format!(
        "\nexport default new alien.Stack(\"my-app\")\n{extensions}  .add(new alien.Storage(\"data\").build(), \"frozen\")\n  .build()\n"
    ));
    std::fs::write(dir.join("alien.ts"), source).unwrap();
}

/// Copies `examples/extensions/<example>` to `<app>/<folder>` and builds it there, as the docs'
/// Quickstart does.
fn build_extension_in(app: &Path, example: &str, folder: &str) -> PathBuf {
    let root = app.join(folder);
    copy_example(example, &root);
    succeeds(alien(&root, &["extensions", "build"]));
    root
}

fn render(app: &Path, format: &[&str]) -> Output {
    let mut args = vec!["render", "--stack", "alien.ts"];
    args.extend_from_slice(format);
    alien(app, &args)
}

const FORMATS: [&[&str]; 4] = [
    &["--format", "cloudformation", "--json"],
    &["--format", "terraform", "--target", "aws"],
    &["--format", "terraform", "--target", "gcp"],
    &["--format", "helm"],
];

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// The core zip with `hooks.wasm` replaced by `wasm` and the manifest unchanged.
fn with_hooks_module(zip: &[u8], wasm: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let manifest = zip_entry(zip, "manifest.json");
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in [("manifest.json", manifest.as_slice()), ("hooks.wasm", wasm)] {
        archive.start_file(name, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Fails the render and writes no template.
fn refused(app: &Path) -> String {
    let output = alien(
        app,
        &[
            "render",
            "--format",
            "cloudformation",
            "--stack",
            "alien.ts",
            "--output",
            "out",
        ],
    );
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(!app.join("out").exists(), "{text}");
    assert!(
        text.contains("The bundle of extension 'backups' can't be used"),
        "{text}"
    );
    text
}

#[cfg(unix)]
#[test]
fn render_refuses_a_bundle_the_stack_does_not_pin() {
    let app = app(&[("backups", "./backups/generated")]);
    let root = build_extension_in(app.path(), "postgres-backup-retention", "backups");
    let root = root.canonicalize().unwrap();
    let zip_path = root.join("target/alien-extension/backups-core.zip");
    let index_ts = root.join("generated/index.ts");
    let zip = std::fs::read(&zip_path).unwrap();
    let generated = std::fs::read_to_string(&index_ts).unwrap();
    let pinned = sha256_digest(&zip);
    assert!(generated.contains(&pinned), "{generated}");

    // No bundle where the module sits.
    std::fs::remove_file(&zip_path).unwrap();
    let text = refused(app.path());
    let expected = format!(
        "alien.ts imports it from {}, and {} has no backups-core.zip. Run `alien extensions build` in {}",
        index_ts.display(),
        root.join("target/alien-extension").display(),
        root.display()
    );
    assert!(text.contains(&expected), "{expected}\n---\n{text}");

    // One changed byte: the stack pins the zip the build wrote, and this is another.
    let mut changed = zip.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    std::fs::write(&zip_path, &changed).unwrap();
    let text = refused(app.path());
    for expected in [
        format!("the stack pins {pinned}, and "),
        format!("backups-core.zip is {}", sha256_digest(&changed)),
        "Rebuild the extension with `alien extensions build`".to_string(),
    ] {
        assert!(text.contains(&expected), "{expected}: {text}");
    }

    // A zip the stack pins, whose hooks module is not the one its manifest lists: still a valid
    // module, with a custom section appended, so only the hash tells them apart.
    let mut wasm = zip_entry(&zip, "hooks.wasm");
    wasm.extend_from_slice(&[0, 2, 1, b'x']);
    let swapped = with_hooks_module(&zip, &wasm);
    std::fs::write(&zip_path, &swapped).unwrap();
    std::fs::write(
        &index_ts,
        generated.replace(&pinned, &sha256_digest(&swapped)),
    )
    .unwrap();
    let text = refused(app.path());
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_entry(&zip, "manifest.json")).unwrap();
    assert!(text.contains("did not load"), "{text}");
    assert!(
        text.contains(&format!(
            "The hooks module's sha256 is {}, but the release pins {}",
            hex::encode(Sha256::digest(&wasm)),
            manifest["hooksSha256"].as_str().unwrap()
        )),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn render_refuses_an_extension_whose_module_it_cannot_place() {
    let app = app(&[("backups", "./backups/generated")]);
    let root = build_extension_in(app.path(), "postgres-backup-retention", "backups");
    let generated = std::fs::read_to_string(root.join("generated/index.ts")).unwrap();

    // The manifest without the module: nothing says where its bundle is.
    let start = generated.find("= {").unwrap() + 2;
    let end = generated.find("\n\n//").unwrap();
    std::fs::write(
        root.join("generated/inline.ts"),
        format!("export default {}\n", &generated[start..end]),
    )
    .unwrap();
    write_alien_ts(app.path(), &[("backups", "./backups/generated/inline.ts")]);
    let text = refused(app.path());
    assert!(
        text.contains("alien.ts imported no module `alien extensions build` generated for it"),
        "{text}"
    );

    // Two copies of the module: either bundle could be the one the stack means.
    copy_dir(&root, &app.path().join("backups-copy"));
    std::fs::create_dir_all(app.path().join("backups-copy/generated")).unwrap();
    std::fs::write(
        app.path().join("backups-copy/generated/index.ts"),
        &generated,
    )
    .unwrap();
    std::fs::write(
        app.path().join("alien.ts"),
        std::fs::read_to_string(app.path().join("alien.ts"))
            .unwrap()
            .replace(
                "import backups from \"./backups/generated/inline.ts\"",
                "import backups from \"./backups/generated\"\nimport \"./backups-copy/generated\"",
            ),
    )
    .unwrap();
    let text = refused(app.path());
    assert!(
        text.contains("alien.ts imported its generated module from more than one place"),
        "{text}"
    );
}

// A stack without extensions reads no bundle, so an app with no extension folder renders.
#[cfg(unix)]
#[test]
fn render_without_extensions_reads_no_bundle() {
    let app = app(&[]);
    for format in FORMATS {
        let output = succeeds(render(app.path(), format));
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("Changed by extensions"),
            "{format:?}"
        );
    }
}

// Loading a configuration reads only well-formed extension-module entries, so a stack that lists
// no extensions loads whatever else wrote to the list; and a configuration without a default
// export says so.
#[cfg(unix)]
#[test]
fn render_loads_a_stack_whatever_else_wrote_to_the_modules_list() {
    let app = app(&[]);
    let alien_ts = app.path().join("alien.ts");
    let source = std::fs::read_to_string(&alien_ts).unwrap();
    for junk in ["\"junk\"", "[1, null, { name: 2 }]", "{ length: 1 }"] {
        std::fs::write(
            &alien_ts,
            format!(
                "(globalThis as any)[Symbol.for(\"alien.extensionModules\")] = {junk}\n{source}"
            ),
        )
        .unwrap();
        let output = succeeds(render(app.path(), FORMATS[3]));
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("Changed by extensions"),
            "{junk}"
        );
    }

    std::fs::write(&alien_ts, "export const stack = 1\n").unwrap();
    let output = render(app.path(), FORMATS[3]);
    assert!(!output.status.success());
    let text = output_text(&output);
    assert!(
        text.contains("alien.ts has no default export. Export the stack with `export default`"),
        "{text}"
    );
}

// The DNS zone template. Its controller crate's library builds into the controller module, which
// lists the zone's cloud controllers itself; the crate's binary lists the local one. The build
// writes the module and its digest into the core zip, and the origins the workspace declares
// into both manifests, the same bytes every time. Each importer fixture's state then loads into
// the module. Only this test changes `dns-zone`'s sources.
#[test]
fn the_controller_module_is_built_into_the_bundle_and_test_loads_each_import_into_it() {
    let example = example("dns-zone");
    let workspace = example.path().join("Cargo.toml");
    let declared = std::fs::read_to_string(&workspace).unwrap().replace(
        "types = \"types\"\n",
        "types = \"types\"\negress = [\"https://api.acme.dev\"]\n",
    );
    assert!(declared.contains("egress"), "the workspace's table moved");
    std::fs::write(&workspace, declared).unwrap();

    let host = alien_core::BinaryTarget::current_os().to_string();
    let build = ["extensions", "build", "--target", host.as_str()];
    succeeds(alien(example.path(), &build));
    let first = build_output(example.path());
    let file = |name: &str| {
        first
            .iter()
            .find(|(path, _)| path == Path::new(name))
            .map(|(_, bytes)| bytes.as_slice())
            .unwrap_or_else(|| panic!("the build wrote no {name}"))
    };
    let zip = file("target/alien-extension/acme-core.zip");
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_entry(zip, "manifest.json")).unwrap();
    let module = zip_entry(zip, "controller.wasm");
    assert_eq!(&module[..4], b"\0asm");
    assert_eq!(
        manifest["controllerModuleSha256"],
        hex::encode(Sha256::digest(&module))
    );
    // The module lists aws, azure and gcp, and this machine's binary lists local.
    assert_eq!(
        manifest["resourceTypes"]["acme/dns-zone"]["controllerPlatforms"],
        serde_json::json!(["aws", "azure", "gcp", "local"])
    );
    assert_eq!(
        manifest["protocol"],
        serde_json::json!({ "hooks": 1, "controller": 1 })
    );
    assert_eq!(
        manifest["egress"],
        serde_json::json!(["https://api.acme.dev"])
    );
    let binary = file(&format!("target/alien-extension/acme-controller-{host}"));
    assert_eq!(
        manifest["controllers"][&host]["sha256"],
        hex::encode(Sha256::digest(binary))
    );
    // The stack's manifest carries what the customer reviews: the same origins.
    let index_ts = std::str::from_utf8(file("generated/index.ts")).unwrap();
    assert!(
        index_ts.contains("\"egress\": [\n    \"https://api.acme.dev\"\n  ]"),
        "{index_ts}"
    );
    assert!(
        index_ts.contains(&format!(
            "\"digest\": \"sha256:{}\"",
            hex::encode(Sha256::digest(zip))
        )),
        "{index_ts}"
    );

    // A second build, which compiles the modules again when another test built this example
    // from its own copy in between, writes the same bytes.
    succeeds(alien(example.path(), &build));
    assert_eq!(build_output(example.path()), first);

    let output = succeeds(alien(example.path(), &["extensions", "test"]));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for platform in ["aws", "gcp", "azure"] {
        assert!(
            stdout.contains(&format!(
                "tests/import/acme/dns-zone/{platform}/basic.json loads into its controller unchanged"
            )),
            "{stdout}"
        );
    }

    let hooks = example.path().join("hooks/src/dns_zone.rs");
    let original = std::fs::read_to_string(&hooks).unwrap();
    let aws_fields =
        "AwsDnsZoneFields {\n                zone_id: Some(data.zone_id),\n            },";
    assert!(original.contains(aws_fields), "the AWS importer moved");

    // The importer saves the zone under a name the controller doesn't read.
    let renamed = original.replace(
        aws_fields,
        r#"{
                #[derive(serde::Serialize)]
                struct Renamed {
                    #[serde(rename = "zoneIdentifier")]
                    zone_id: Option<String>,
                }
                Renamed { zone_id: Some(data.zone_id) }
            },"#,
    );
    std::fs::write(&hooks, renamed).unwrap();
    let output = alien(example.path(), &["extensions", "test"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "tests/import/acme/dns-zone/aws/basic.json: the importer wrote `zoneIdentifier`, which the controller doesn't read"
        ),
        "{stderr}"
    );

    // The importer leaves the controller at a state it doesn't have.
    let wrong_state = original.replace(
        &format!("ctx.ready(\n            {aws_fields}\n            outputs,\n        )"),
        &format!(
            "let mut state = ctx.ready(\n            {aws_fields}\n            outputs,\n        )?;\n        state.internal_state.as_mut().unwrap()[\"state\"] = \"running\".into();\n        Ok(state)"
        ),
    );
    assert!(
        wrong_state.contains("\"running\""),
        "the AWS importer moved"
    );
    std::fs::write(&hooks, wrong_state).unwrap();
    let output = alien(example.path(), &["extensions", "test"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "tests/import/acme/dns-zone/aws/basic.json: the controller can't load the state the importer wrote"
        ),
        "{stderr}"
    );
}

/// A project as a fresh checkout has it: alien.ts imports `./budget/generated`, which the
/// template ignores, so it isn't there until the extension builds.
fn release_project() -> tempfile::TempDir {
    let project = tempfile::Builder::new()
        .prefix("release")
        .tempdir()
        .unwrap();
    let budget = example("budget-alert");
    copy_dir(budget.path(), &project.path().join("budget"));
    assert!(!project.path().join("budget/generated").exists());
    std::fs::write(
        project.path().join("alien.ts"),
        "import * as alien from \"@alienplatform/core\"\nimport budget from \"./budget/generated\"\n\nexport default new alien.Stack(\"my-app\").extensions([budget]).build()\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{ "name": "my-app", "private": true, "type": "module" }"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.path().join("node_modules/@alienplatform")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        repository().join("packages/core"),
        project.path().join("node_modules/@alienplatform/core"),
    )
    .unwrap();
    project
}

/// `alien release` against a manager at `manager`, with `path` as `PATH` when given.
fn release(directory: &Path, manager: &str, args: &[&str], path: Option<&str>) -> Output {
    let mut command = command(directory, &[&["release", "--no-git"], args].concat());
    command
        .env("ALIEN_MANAGER_URL", manager)
        .env("ALIEN_API_KEY", "test-key")
        .env("RUST_LOG", "info");
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().unwrap()
}

/// A manager that answers what a release asks before a push (the stacks here have no images to
/// push), with bundle routes that store nothing yet, and the mock of its release route for a body
/// that contains `body`.
fn manager<'a>(server: &'a httpmock::MockServer, body: &str) -> Manager<'a> {
    // httpmock hands a pooled server to the next test: what an earlier test uploaded to it is not
    // this release's.
    let this_server = server.address().to_string();
    CONTROLLERS_UPLOADED
        .lock()
        .unwrap()
        .retain(|host| *host != this_server);
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/v1/build-config");
        then.status(200)
            .json_body(serde_json::json!({ "repositoryName": "artifacts/default" }));
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/v1/image-repositories");
        then.status(200)
            .json_body(serde_json::json!({ "repository": "artifacts/default" }));
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::HEAD)
            .path_contains("/v1/extensions/bundles/sha256:");
        then.status(404);
    });
    let controllers = server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path_contains("/v1/extensions/bundles/sha256:")
            .matches(posts_a_controller);
        then.status(201);
    });
    // A core zip is stored only after a controller of its bundle on this server, when its
    // bundle has one; `alien release` uploads in that order.
    let cores_after_controllers = server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path_contains("/v1/extensions/bundles/sha256:")
            .query_param("object", "core")
            .matches(follows_a_controller);
        then.status(201);
    });
    let cores = server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path_contains("/v1/extensions/bundles/sha256:")
            .query_param("object", "core");
        then.status(201);
    });
    let created = server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/v1/releases")
            .body_contains(body);
        then.status(201).json_body(serde_json::json!({
            "id": "rel_1", "workspaceId": "default", "projectId": "default",
            "stack": {}, "createdAt": "2026-01-01T00:00:00Z", "setupFingerprints": {}
        }));
    });
    Manager {
        created,
        controllers,
        cores_after_controllers,
        cores,
    }
}

/// The mocks of a manager's release route and bundle uploads.
struct Manager<'a> {
    created: httpmock::Mock<'a>,
    controllers: httpmock::Mock<'a>,
    cores_after_controllers: httpmock::Mock<'a>,
    cores: httpmock::Mock<'a>,
}

/// The servers, by `host` header, that a controller binary was uploaded to.
static CONTROLLERS_UPLOADED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn query(request: &httpmock::prelude::HttpMockRequest, name: &str) -> Option<String> {
    request
        .query_params
        .iter()
        .flatten()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

fn host(request: &httpmock::prelude::HttpMockRequest) -> String {
    request
        .headers
        .iter()
        .flatten()
        .find(|(key, _)| key.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn posts_a_controller(request: &httpmock::prelude::HttpMockRequest) -> bool {
    let controller = query(request, "object").is_some_and(|object| object != "core");
    if controller {
        CONTROLLERS_UPLOADED.lock().unwrap().push(host(request));
    }
    controller
}

fn follows_a_controller(request: &httpmock::prelude::HttpMockRequest) -> bool {
    CONTROLLERS_UPLOADED
        .lock()
        .unwrap()
        .contains(&host(request))
}

// A stack without extensions releases whatever extension workspaces the project holds: one
// that doesn't build is never built.
#[cfg(unix)]
#[test]
fn release_of_a_stack_without_extensions_builds_no_workspace() {
    let project = release_project();
    std::fs::write(
        project.path().join("alien.ts"),
        "import * as alien from \"@alienplatform/core\"\nconst uploads = new alien.Storage(\"uploads\").build()\nexport default new alien.Stack(\"my-app\").add(uploads, \"frozen\").build()\n",
    )
    .unwrap();
    std::fs::create_dir_all(project.path().join("tools/wip-ext")).unwrap();
    std::fs::write(
        project.path().join("tools/wip-ext/Cargo.toml"),
        "[workspace]\nmembers = [\"hooks\"]\n\n[workspace.metadata.alien-extension]\nhooks = \"hooks\"\n",
    )
    .unwrap();
    let server = httpmock::MockServer::start();
    let manager = manager(&server, r#""uploads""#);
    let created = &manager.created;
    let output = succeeds(release(
        project.path(),
        &server.base_url(),
        &["--platforms", "aws"],
        None,
    ));
    created.assert_hits(1);
    assert_eq!(
        manager.controllers.hits() + manager.cores_after_controllers.hits() + manager.cores.hits(),
        0
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("Building the extension"), "{stderr}");
    assert!(!project.path().join("budget/generated").exists());
}
