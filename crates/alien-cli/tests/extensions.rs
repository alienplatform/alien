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

// The controller is built for the targets this machine can build without installing anything,
// which differ between machines; the bundle must list each one it built and `null` for the rest.
#[test]
fn build_writes_the_bundle_and_the_module_the_same_every_time() {
    let example = example("postgres-backup-retention");
    add_controller(example.path());

    succeeds(alien(example.path(), &["extensions", "build"]));
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

    succeeds(alien(example.path(), &["extensions", "build"]));
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

#[test]
fn test_fails_when_an_importer_answers_differently_in_the_module() {
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
fn app(imports: &[(&str, &str)]) -> tempfile::TempDir {
    let app = tempfile::Builder::new().prefix("my-app").tempdir().unwrap();
    app_at(app.path(), imports);
    app
}

/// [`app`], in `dir`.
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

fn assert_listing(output: &Output, lines: &[&str]) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = format!(
        "Changed by extensions, in the order the stack lists them:\n{}",
        lines
            .iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(stderr.contains(&expected), "{expected}\n---\n{stderr}");
}

// The docs' layout: each extension built in its own folder beside alien.ts, and `alien render`
// run from the app's folder with no flag. Render finds each bundle beside the module alien.ts
// imports and runs every extension's modifiers, in the stack's order, on every format. Backups
// and tags change no bucket, so each extension lists no changes.
#[test]
fn render_reads_each_bundle_beside_the_module_alien_ts_imports() {
    let app = app(&[
        ("backups", "./backups/generated"),
        ("tags", "./extensions/tags/generated"),
    ]);
    build_extension_in(app.path(), "postgres-backup-retention", "backups");
    build_extension_in(app.path(), "resource-tags", "extensions/tags");
    for format in FORMATS {
        let output = succeeds(render(app.path(), format));
        assert_listing(&output, &["backups: no changes", "tags: no changes"]);
    }
}

// An installed extension: a package that carries the generated module and the bundle, copied into
// node_modules as npm installs it, or linked there as pnpm does.
#[test]
fn render_reads_the_bundle_of_an_extension_installed_as_a_package() {
    let app = app(&[("backups", "@acme/backups"), ("tags", "@acme/tags")]);
    let built = tempfile::tempdir().unwrap();
    for (example, package) in [
        ("postgres-backup-retention", "backups"),
        ("resource-tags", "tags"),
    ] {
        let root = build_extension_in(built.path(), example, package);
        std::fs::write(
            root.join("package.json"),
            format!(r#"{{"name": "@acme/{package}", "main": "generated/index.ts"}}"#),
        )
        .unwrap();
    }
    let scope = app.path().join("node_modules/@acme");
    std::fs::create_dir_all(&scope).unwrap();
    copy_built(&built.path().join("backups"), &scope.join("backups"));
    std::os::unix::fs::symlink(built.path().join("tags"), scope.join("tags")).unwrap();

    for format in FORMATS {
        let output = succeeds(render(app.path(), format));
        assert_listing(&output, &["backups: no changes", "tags: no changes"]);
    }
}

/// Copies a built extension as a package carries it: its manifest, generated module and bundle.
fn copy_built(from: &Path, to: &Path) {
    for path in [
        "package.json",
        "generated/index.ts",
        "target/alien-extension/backups-core.zip",
    ] {
        std::fs::create_dir_all(to.join(path).parent().unwrap()).unwrap();
        std::fs::copy(from.join(path), to.join(path)).unwrap();
    }
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

// The module's URL percent-encodes its path, so a folder whose name has a space, a non-ASCII
// letter, a literal `%20` and a `#` must still lead render to the bundle beside it.
#[test]
fn render_reads_a_bundle_from_a_folder_whose_name_needs_escaping() {
    let parent = tempfile::tempdir().unwrap();
    let app = parent.path().join("my app \u{fc}%20 #x");
    std::fs::create_dir_all(&app).unwrap();
    app_at(&app, &[("backups", "./backups/generated")]);
    build_extension_in(&app, "postgres-backup-retention", "backups");
    let output = succeeds(render(&app, FORMATS[0]));
    assert_listing(&output, &["backups: no changes"]);
}

// Loading a configuration reads only well-formed extension-module entries, so a stack that lists
// no extensions loads whatever else wrote to the list; and a configuration without a default
// export says so.
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

// The DNS zone template: its controller builds for this machine, and each importer fixture's state
// loads into it. Only this test runs `dns-zone`, since it changes the copy's importer.
#[test]
fn test_loads_each_import_into_the_controller_and_fails_on_a_renamed_field_or_state() {
    let example = example("dns-zone");
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

// A stack without extensions releases as it always did, whatever extension workspaces the
// project holds: one that doesn't build is never built.
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

// A fresh checkout releases: the extension builds before alien.ts is evaluated, and the release
// pins the bundle the build wrote. A `--prebuilt` release then checks the importers the bundle
// ships, with no cargo on the machine: an edit to the hooks crate changes nothing, and an edit
// to a fixture the shipped importer can't read is refused.
#[cfg(unix)]
#[test]
fn release_builds_each_extension_first_and_checks_the_module_it_ships() {
    let project = release_project();
    // An extension workspace alien.ts doesn't list, already built once: the release never builds
    // it, and this one doesn't build.
    let tags = project.path().join("tags");
    std::fs::create_dir_all(tags.join("generated")).unwrap();
    std::fs::write(
        tags.join("Cargo.toml"),
        "[workspace]\nmembers = [\"hooks\"]\n\n[workspace.metadata.alien-extension]\nhooks = \"hooks\"\n",
    )
    .unwrap();
    std::fs::write(
        tags.join("generated/index.ts"),
        "const extension: ExtensionManifest = { \"name\": \"tags\" }\n\nexport default extension\n",
    )
    .unwrap();
    let server = httpmock::MockServer::start();
    let manager = manager(&server, r#""name":"budget""#);
    let created = &manager.created;

    let output = succeeds(release(
        project.path(),
        &server.base_url(),
        &["--platforms", "aws"],
        None,
    ));
    created.assert_hits(1);
    let built_tags = |output: &Output| {
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line.contains("Building the extension in") && line.ends_with("/tags"))
    };
    assert!(!built_tags(&output));
    // alien.ts is read twice: once before the fallback built budget, once after. Budget is the
    // only listed extension and is built, so it isn't read a third time.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.contains("alien_cli::config: Loading configuration from:"))
            .count(),
        2,
        "{stderr}"
    );
    let zip = std::fs::read(
        project
            .path()
            .join("budget/target/alien-extension/budget-core.zip"),
    )
    .unwrap();
    let built: serde_json::Value = serde_json::from_slice(
        &std::fs::read(project.path().join(".alien/build/aws/stack.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        built["extensions"][0]["digest"],
        format!("sha256:{}", hex::encode(Sha256::digest(&zip)))
    );

    // alien.ts now evaluates, and lists the extension: the release builds it again, and the
    // stack pins the new bundle.
    let hooks = project.path().join("budget/hooks/src/lib.rs");
    let source = std::fs::read_to_string(&hooks).unwrap();
    assert!(source.contains(r#""Threshold": 80"#), "the emitter moved");
    std::fs::write(
        &hooks,
        source.replace(r#""Threshold": 80"#, r#""Threshold": 90"#),
    )
    .unwrap();
    let output = succeeds(release(
        project.path(),
        &server.base_url(),
        &["--platforms", "aws"],
        None,
    ));
    created.assert_hits(2);
    assert!(!built_tags(&output));
    let rebuilt = std::fs::read(
        project
            .path()
            .join("budget/target/alien-extension/budget-core.zip"),
    )
    .unwrap();
    assert_ne!(rebuilt, zip);
    let built: serde_json::Value = serde_json::from_slice(
        &std::fs::read(project.path().join(".alien/build/aws/stack.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        built["extensions"][0]["digest"],
        format!("sha256:{}", hex::encode(Sha256::digest(&rebuilt)))
    );

    // No cargo on PATH, and a hooks crate that no longer builds: `--prebuilt` checks the module
    // that ships.
    let source = std::fs::read_to_string(&hooks).unwrap();
    std::fs::write(&hooks, source.replace("ctx.ready(", "does_not_compile(")).unwrap();
    let output = succeeds(release(
        project.path(),
        &server.base_url(),
        &["--prebuilt", "--platforms", "aws"],
        Some("/usr/bin:/bin"),
    ));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("The extension `budget`'s shipped importers pass on 1 fixtures"),
        "{stderr}"
    );
    created.assert_hits(3);

    // A fixture the shipped importer can't read.
    let fixture = project
        .path()
        .join("budget/tests/import/budget/alert/aws/basic.json");
    let text = std::fs::read_to_string(&fixture).unwrap();
    std::fs::write(&fixture, text.replace("budgetName", "name")).unwrap();
    let output = release(
        project.path(),
        &server.base_url(),
        &["--prebuilt", "--platforms", "aws"],
        Some("/usr/bin:/bin"),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tests/import/budget/alert/aws/basic.json: the importer failed"),
        "{stderr}"
    );
    created.assert_hits(3);
    // Budget's bundle has no controller: each release uploads its core zip.
    assert!(manager.cores.hits() >= 1);
}

/// A controller that loads any state back unchanged, as running, with the budget fixture's
/// outputs: the step protocol's `load` reply, for budget/alert on AWS only.
const FAKE_CONTROLLER: &str = r#"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    request = json.loads(line)
    if request["resourceType"] != "budget/alert" or request["platform"] != "aws":
        reply = {"protocol": 1, "refused": "no controller for this type and platform"}
    else:
        allowed = {"outcome": "allowed", "status": "updating"}
        reply = {"protocol": 1, "view": {
            "state": request["state"], "status": "running", "progress": None,
            "outputs": {"type": "budget/alert", "budgetName": "my-app-costs"},
            "bindingParams": None, "onFailureStatus": "refresh-failed",
            "updateStart": allowed, "deleteStart": allowed, "teardownStart": allowed}}
    print(json.dumps(reply), flush=True)
"#;

// The release loads each state the shipped importers write into the bundled controller for this
// machine's target, for each type the bundle's manifest registers a controller for. The bundle
// here is budget's with a controller for budget/alert on AWS added, as a prebuilt binary.
#[cfg(unix)]
#[test]
fn release_loads_each_shipped_import_into_the_bundled_controller() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;

    let project = release_project();
    let server = httpmock::MockServer::start();
    let manager = manager(&server, r#""name":"budget""#);
    let created = &manager.created;
    succeeds(release(
        project.path(),
        &server.base_url(),
        &["--platforms", "aws"],
        None,
    ));

    let dir = project.path().join("budget/target/alien-extension");
    let host = alien_core::BinaryTarget::current_os().to_string();
    let controller = dir.join(format!("budget-controller-{host}"));
    std::fs::write(&controller, FAKE_CONTROLLER).unwrap();
    std::fs::set_permissions(&controller, std::fs::Permissions::from_mode(0o755)).unwrap();

    let zip_path = dir.join("budget-core.zip");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(&zip_path).unwrap())).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&zip_entry(
        &std::fs::read(&zip_path).unwrap(),
        "manifest.json",
    ))
    .unwrap();
    manifest["protocol"]["controller"] = serde_json::json!(1);
    manifest["controllers"][&host] = serde_json::json!({
        "sha256": hex::encode(Sha256::digest(FAKE_CONTROLLER.as_bytes())),
        "size": FAKE_CONTROLLER.len(),
    });
    manifest["resourceTypes"]["budget/alert"]["controllerPlatforms"] = serde_json::json!(["aws"]);
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "manifest.json" {
            bytes = serde_json::to_vec(&manifest).unwrap();
        }
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let doctored = zip.finish().unwrap().into_inner();
    std::fs::write(&zip_path, &doctored).unwrap();

    let stack_path = project.path().join(".alien/build/aws/stack.json");
    let mut stack: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&stack_path).unwrap()).unwrap();
    stack["extensions"][0]["digest"] =
        serde_json::json!(format!("sha256:{}", hex::encode(Sha256::digest(&doctored))));
    stack["extensions"][0]["resourceTypes"] = manifest["resourceTypes"].clone();
    std::fs::write(&stack_path, serde_json::to_vec(&stack).unwrap()).unwrap();

    let output = succeeds(release(
        project.path(),
        &server.base_url(),
        &[
            "--prebuilt",
            "--platforms",
            "aws",
            "--allow-missing-controller-targets",
        ],
        None,
    ));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("The extension `budget`'s shipped importers pass on 1 fixtures; 1 load into its controller"),
        "{stderr}"
    );
    created.assert_hits(2);
    // Each release uploads what the manager lacks: the doctored bundle's controller, then its
    // core zip.
    assert!(manager.controllers.hits() >= 1);
    assert!(manager.cores_after_controllers.hits() >= 1);
}

/// `alien dev` on the docs' DNS zone: alien.ts lists the acme extension and adds a zone. The
/// extension builds first, its bundle goes to the local manager, and the manager runs the
/// extension's controller for the local platform as a process, which creates the zone.
#[cfg(unix)]
#[test]
fn dev_runs_the_zone_with_the_extensions_local_controller_as_a_process() {
    use std::time::{Duration, Instant};

    let project = tempfile::Builder::new().prefix("dev").tempdir().unwrap();
    let acme = example("dns-zone");
    copy_dir(acme.path(), &project.path().join("acme"));
    std::fs::write(
        project.path().join("alien.ts"),
        "import * as alien from \"@alienplatform/core\"\nimport acme, { DnsZone } from \"./acme/generated\"\n\nconst zone = new DnsZone(\"zone\").domain(\"internal.acme.dev\").build()\n\nexport default new alien.Stack(\"my-app\").add(zone, \"live\").extensions([acme]).build()\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{ "name": "my-app", "private": true, "type": "module" }"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.path().join("node_modules/@alienplatform")).unwrap();
    std::os::unix::fs::symlink(
        repository().join("packages/core"),
        project.path().join("node_modules/@alienplatform/core"),
    )
    .unwrap();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let status_file = project.path().join("status.json");
    let log_path = project.path().join("dev.log");
    let log = std::fs::File::create(&log_path).unwrap();
    struct Dev(std::process::Child);
    impl Drop for Dev {
        fn drop(&mut self) {
            if self.0.try_wait().unwrap().is_none() {
                self.0.kill().unwrap();
            }
            self.0.wait().unwrap();
        }
    }
    let mut dev = Dev(command(project.path(), &["dev", "--status-file"])
        .arg(&status_file)
        .args(["--port", &port.to_string()])
        .args([
            "--workspace",
            "example",
            "--project",
            "example",
            "--no-browser",
        ])
        .env_remove("ALIEN_API_KEY")
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap());

    let deadline = Instant::now() + Duration::from_secs(1200);
    let deployment_id = loop {
        let status = std::fs::read(&status_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        let agent = status
            .as_ref()
            .and_then(|status| status["agents"].as_object())
            .and_then(|agents| agents.values().next().cloned());
        if let Some(agent) = &agent {
            assert_ne!(
                agent["status"],
                "provisioning-failed",
                "{agent}\n{}",
                std::fs::read_to_string(&log_path).unwrap()
            );
            if agent["status"] == "running" {
                break agent["id"].as_str().unwrap().to_string();
            }
        }
        assert!(
            dev.0.try_wait().unwrap().is_none(),
            "alien dev exited: {}",
            std::fs::read_to_string(&log_path).unwrap()
        );
        assert!(
            Instant::now() < deadline,
            "the zone did not deploy: {}",
            std::fs::read_to_string(&log_path).unwrap()
        );
        std::thread::sleep(Duration::from_millis(200));
    };

    let deployment: serde_json::Value = tokio::runtime::Runtime::new().unwrap().block_on(async {
        reqwest::get(format!(
            "http://127.0.0.1:{port}/v1/deployments/{deployment_id}"
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
    });
    let zone = &deployment["stackState"]["resources"]["zone"];
    assert_eq!(zone["status"], "running", "{deployment:#}");
    // Only the extension's controller for the local platform writes this id, and Alien reaches
    // it only as a process.
    let prefix = deployment["stackState"]["resourcePrefix"].as_str().unwrap();
    assert_eq!(
        zone["outputs"]["zoneId"],
        format!("{prefix}-zone"),
        "{zone:#}"
    );
}
