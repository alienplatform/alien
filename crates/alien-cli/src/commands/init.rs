use crate::error::{ErrorData, Result};
use crate::output::{can_prompt, prompt_confirm, prompt_select_index};
use crate::ui::{
    accent, command, contextual_heading, dim_label, success_line, FixedSteps, Spinner,
};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use tar::Archive;

#[derive(Parser, Debug, Clone)]
#[command(about = "Scaffold a new project from a template")]
pub struct InitArgs {
    /// Template to use (omit for interactive selection)
    pub template: Option<String>,

    /// Destination directory. Defaults to `alien` inside an existing repository
    /// and to the current directory when it is empty.
    pub directory: Option<String>,

    /// Overwrite target directory if it exists
    #[arg(long)]
    pub force: bool,
}

#[derive(Clone)]
struct TemplateInfo {
    name: String,
    description: String,
}

const KNOWN_TEMPLATES: &[(&str, &str)] = &[
    (
        "ai-chatbot-ts",
        "Build a streaming AI chatbot over private Postgres data.",
    ),
    (
        "ai-quickstart-ts",
        "Call models available in the deployment's cloud from a Worker.",
    ),
    (
        "remote-worker-ts",
        "Run a private worker near sensitive data, internal services, or specialized compute.",
    ),
    (
        "data-connector-ts",
        "Connect your product to data and services that are not publicly reachable.",
    ),
    (
        "event-pipeline-ts",
        "Process events and data close to the systems that produce them.",
    ),
    (
        "customer-kubernetes",
        "Run a service in your customers' Kubernetes clusters, including air-gapped ones.",
    ),
    (
        "webhook-api-ts",
        "Expose an HTTPS service that runs beside private data or infrastructure.",
    ),
    (
        "basic-worker-ts",
        "Start with one TypeScript worker and no infrastructure.",
    ),
    (
        "basic-worker-rs",
        "Start with one Rust worker and no infrastructure.",
    ),
    (
        "bare-metal-worker",
        "Rust job worker for Linux servers, alien.toml",
    ),
    (
        "byob-storage-ts",
        "Use customer-owned object storage from a hosted backend.",
    ),
    (
        "customer-keys-ts",
        "Encrypt data with a key controlled by each customer.",
    ),
    (
        "customer-models-ts",
        "Let each customer connect models from their cloud account.",
    ),
    (
        "github-agent",
        "Build a GitHub integration agent with a hosted dashboard.",
    ),
    (
        "nextjs-app",
        "Deploy a complete Next.js application as a Container.",
    ),
    (
        "extensions/postgres-iam-auth",
        "An extension that turns on IAM authentication for Alien's Postgres on every setup path.",
    ),
    (
        "extensions/postgres-backup-retention",
        "An extension that keeps Postgres backups for 30 days in every setup template.",
    ),
    (
        "extensions/resource-tags",
        "An extension that adds your tags to the resources Alien's controllers create.",
    ),
    (
        "extensions/budget-alert",
        "An extension that adds an AWS budget alert, a custom resource with no controller.",
    ),
    (
        "extensions/dns-zone",
        "An extension that adds a DNS zone, a custom resource with its own controllers.",
    ),
];

fn fallback_templates() -> Vec<TemplateInfo> {
    KNOWN_TEMPLATES
        .iter()
        .map(|(name, desc)| TemplateInfo {
            name: name.to_string(),
            description: desc.to_string(),
        })
        .collect()
}

async fn fetch_templates() -> Result<Vec<TemplateInfo>> {
    // The catalog is versioned with the CLI so every listed template is known
    // to have compatible metadata and source. Reading main at runtime could
    // expose templates a released CLI cannot understand, or silently return a
    // partial catalog when one metadata request fails.
    Ok(fallback_templates())
}

fn destination_for_init(cwd: &Path, directory: Option<&str>) -> PathBuf {
    if let Some(directory) = directory {
        return cwd.join(directory);
    }

    let has_files = cwd
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);

    if has_files {
        cwd.join("alien")
    } else {
        cwd.to_path_buf()
    }
}

fn display_directory(cwd: &Path, target_dir: &Path) -> String {
    target_dir
        .strip_prefix(cwd)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_string())
}

fn print_template_list(templates: &[TemplateInfo]) {
    let max_name_len = templates.iter().map(|t| t.name.len()).max().unwrap_or(0);

    for template in templates {
        println!(
            "  {:<width$}   {}",
            template.name,
            template.description,
            width = max_name_len
        );
    }
}

fn format_template_choices(templates: &[TemplateInfo]) -> Vec<String> {
    let max_name_len = templates.iter().map(|t| t.name.len()).max().unwrap_or(0);
    templates
        .iter()
        .map(|t| {
            if t.description.is_empty() {
                t.name.clone()
            } else {
                format!(
                    "{:<width$}   {}",
                    t.name,
                    t.description,
                    width = max_name_len
                )
            }
        })
        .collect()
}

fn find_closest_template<'a>(
    name: &str,
    templates: &'a [TemplateInfo],
) -> Option<&'a TemplateInfo> {
    let name_lower = name.to_ascii_lowercase();

    // Case-insensitive exact match
    if let Some(t) = templates
        .iter()
        .find(|t| t.name.to_ascii_lowercase() == name_lower)
    {
        return Some(t);
    }

    // Substring match
    if let Some(t) = templates.iter().find(|t| {
        t.name.to_ascii_lowercase().contains(&name_lower)
            || name_lower.contains(&t.name.to_ascii_lowercase())
    }) {
        return Some(t);
    }

    // Edit distance — accept if within half the template name length
    templates
        .iter()
        .filter_map(|t| {
            let dist = edit_distance(&name_lower, &t.name.to_ascii_lowercase());
            let threshold = (t.name.len() / 2).max(2);
            if dist <= threshold {
                Some((t, dist))
            } else {
                None
            }
        })
        .min_by_key(|(_, dist)| *dist)
        .map(|(t, _)| t)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];

    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b.len()]
}

fn package_manager_for(target_dir: &Path) -> Option<&'static str> {
    for directory in target_dir.ancestors() {
        if directory.join("pnpm-lock.yaml").exists() && which::which("pnpm").is_ok() {
            return Some("pnpm");
        }
        if (directory.join("bun.lock").exists() || directory.join("bun.lockb").exists())
            && which::which("bun").is_ok()
        {
            return Some("bun");
        }
        if directory.join("yarn.lock").exists() && which::which("yarn").is_ok() {
            return Some("yarn");
        }
        if directory.join("package-lock.json").exists() && which::which("npm").is_ok() {
            return Some("npm");
        }
    }

    ["pnpm", "bun", "npm", "yarn"]
        .into_iter()
        .find(|command| which::which(command).is_ok())
}

async fn install_dependencies(target_dir: &Path, package_manager: Option<&str>) -> Result<bool> {
    if !target_dir.join("package.json").exists() {
        return Ok(false);
    }

    let Some(cmd) = package_manager else {
        return Ok(false);
    };

    let output = tokio::process::Command::new(cmd)
        .arg("install")
        .current_dir(target_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "install dependencies".to_string(),
            file_path: target_dir.display().to_string(),
            reason: format!("Failed to run {cmd} install"),
        })?;

    Ok(output.status.success())
}

fn remove_unused_template_lockfile(target_dir: &Path, package_manager: Option<&str>) -> Result<()> {
    if package_manager == Some("pnpm") {
        return Ok(());
    }

    let lockfile = target_dir.join("pnpm-lock.yaml");
    if !lockfile.exists() {
        return Ok(());
    }

    std::fs::remove_file(&lockfile)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "remove unused template lockfile".to_string(),
            file_path: lockfile.display().to_string(),
            reason: "Failed to remove pnpm lockfile".to_string(),
        })
}

fn new_spinner(message: &str) -> Spinner {
    Spinner::new(message)
}

/// The repository archive templates come from: the release's tag for a release CLI, so the
/// template matches the crates [`alien_crates_reference`] pins it to, and `main` otherwise.
fn template_archive_url(version: &str) -> String {
    let reference = match release_tag(version) {
        Some(tag) => format!("refs/tags/{tag}"),
        None => "main".to_string(),
    };
    format!("https://codeload.github.com/alienplatform/alien/tar.gz/{reference}")
}

async fn download_and_extract(template: &str, target_dir: &Path) -> Result<()> {
    let url = &template_archive_url(crate::CLI_VERSION);

    let client = crate::http_client::client()?;
    let response = client
        .get(url)
        .header("User-Agent", "alien-cli")
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to download template archive".to_string(),
            url: Some(url.to_string()),
        })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::HttpRequestFailed {
            message: format!(
                "Failed to download template archive (HTTP {})",
                response.status()
            ),
            url: Some(url.to_string()),
        }));
    }

    let bytes =
        response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to read template archive".to_string(),
                url: Some(url.to_string()),
            })?;

    extract_template(&bytes, template, target_dir)
}

/// Writes `examples/<template>/` from a repository archive into `target_dir`, without its
/// `template.toml`, and pins its dependencies on Alien's crates to this CLI's release.
///
/// The archive's top directory is named after the reference (`alien-main`, or `alien-3.3.54` for
/// the tag `v3.3.54`), so it is skipped whatever its name.
fn extract_template(archive_gz: &[u8], template: &str, target_dir: &Path) -> Result<()> {
    let prefix = format!("examples/{}/", template);
    let decoder = GzDecoder::new(archive_gz);
    let mut archive = Archive::new(decoder);

    let mut found_any = false;
    for entry in archive
        .entries()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read archive".to_string(),
            file_path: "tar.gz".to_string(),
            reason: "Failed to read archive entries".to_string(),
        })?
    {
        let mut entry = entry
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read entry".to_string(),
                file_path: "tar.gz".to_string(),
                reason: "Failed to read archive entry".to_string(),
            })?;

        let path = entry
            .path()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read path".to_string(),
                file_path: "tar.gz".to_string(),
                reason: "Failed to read entry path".to_string(),
            })?;

        let path_str = path
            .components()
            .skip(1)
            .collect::<PathBuf>()
            .to_string_lossy()
            .to_string();

        if let Some(relative) = path_str.strip_prefix(&prefix) {
            if relative.is_empty() {
                continue;
            }

            // Skip template.toml — it's metadata for alien init, not part of the project
            if relative == "template.toml" {
                continue;
            }

            found_any = true;
            let dest = target_dir.join(relative);

            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).into_alien_error().context(
                    ErrorData::FileOperationFailed {
                        operation: "create directory".to_string(),
                        file_path: parent.display().to_string(),
                        reason: "Failed to create directory".to_string(),
                    },
                )?;
            }

            entry
                .unpack(&dest)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "extract".to_string(),
                    file_path: dest.display().to_string(),
                    reason: "Failed to extract file".to_string(),
                })?;
        }
    }

    if !found_any {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "template".to_string(),
            message: format!("Template '{}' not found in archive", template),
        }));
    }

    pin_alien_crates(target_dir)
}

/// The repository the extension templates' Alien crates come from in a scaffold.
const ALIEN_REPOSITORY: &str = "https://github.com/alienplatform/alien";

/// Rewrites each `name = { path = "../../../crates/name" }` dependency in the template's
/// `Cargo.toml` to the same crate in Alien's repository, at this CLI's release tag. In the
/// repository an extension template builds against the crates beside it.
fn pin_alien_crates(target_dir: &Path) -> Result<()> {
    let manifest = target_dir.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return Ok(());
    };
    let pinned = pin_alien_crate_lines(&text, crate::CLI_VERSION);
    if pinned == text {
        return Ok(());
    }
    std::fs::write(&manifest, pinned)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: manifest.display().to_string(),
            reason: "Failed to pin Alien's crates".to_string(),
        })
}

/// The repository's tag for a CLI at `version`, when the version is a release.
///
/// Each release tags the repository `v<version>`. A canary carries a pre-release suffix
/// (`3.3.31-00000001`) and has no tag: it builds from a pinned commit of the repository that its
/// version doesn't name, and that can trail `main`. A local build of an untagged commit reports
/// the version of the release it builds on, and so gets that release's tag.
fn release_tag(version: &str) -> Option<String> {
    let is_release = version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    is_release.then(|| format!("v{version}"))
}

/// The git reference of Alien's crates for a CLI at `version`, and the comment a scaffold needs
/// when there is no fixed one: without a release tag it follows `main`, as its templates do.
fn alien_crates_reference(version: &str) -> (String, Option<String>) {
    match release_tag(version) {
        Some(tag) => (format!(r#"tag = "{tag}""#), None),
        None => (
            r#"branch = "main""#.to_string(),
            Some(format!(
                "# alien {version} has no release tag, so this follows main. Pin a release with tag = \"v<version>\".\n"
            )),
        ),
    }
}

/// Rewrites the path dependencies on Alien's crates, and drops the comment lines right above
/// each: in a template they describe its build inside the repository, which a scaffold isn't.
fn pin_alien_crate_lines(text: &str, version: &str) -> String {
    let (reference, comment) = alien_crates_reference(version);
    let mut pinned = String::new();
    let mut comments = String::new();
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('#') {
            comments.push_str(line);
            continue;
        }
        let rewritten = line
            .trim_end()
            .split_once(" = ")
            .and_then(|(name, source)| {
                let crate_name = name.trim();
                (crate_name.starts_with("alien-")
                    && source == format!(r#"{{ path = "../../../crates/{crate_name}" }}"#))
                .then(|| {
                    let newline = if line.ends_with('\n') { "\n" } else { "" };
                    format!(
                        r#"{}{name} = {{ git = "{ALIEN_REPOSITORY}", {reference} }}{newline}"#,
                        comment.as_deref().unwrap_or("")
                    )
                })
            });
        match rewritten {
            Some(rewritten) => {
                comments.clear();
                pinned.push_str(&rewritten);
            }
            None => {
                pinned.push_str(&comments);
                comments.clear();
                pinned.push_str(line);
            }
        }
    }
    pinned.push_str(&comments);
    pinned
}

fn rewrite_package_json_name(target_dir: &Path, new_name: &str) -> Result<()> {
    let pkg_path = target_dir.join("package.json");
    if !pkg_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&pkg_path)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: pkg_path.display().to_string(),
            reason: "Failed to read package.json".to_string(),
        })?;

    let mut pkg: serde_json::Value =
        serde_json::from_str(&content)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "Failed to parse package.json".to_string(),
            })?;

    if let Some(obj) = pkg.as_object_mut() {
        obj.insert(
            "name".to_string(),
            serde_json::Value::String(new_name.to_string()),
        );
    }

    let output = serde_json::to_string_pretty(&pkg)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize package.json".to_string(),
        })?;

    std::fs::write(&pkg_path, format!("{}\n", output))
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: pkg_path.display().to_string(),
            reason: "Failed to write package.json".to_string(),
        })?;

    Ok(())
}

pub async fn init_task(args: InitArgs) -> Result<()> {
    println!(
        "{}",
        contextual_heading("Scaffolding new", "Alien project", &[])
    );
    println!();

    // 1. Fetch templates (with spinner)
    let spinner = new_spinner("Fetching templates...");
    let templates = fetch_templates().await?;
    spinner.finish_and_clear();

    if templates.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "No templates available.".to_string(),
        }));
    }

    // 2. Select template
    let selected = match args.template {
        Some(name) => {
            match templates.iter().find(|t| t.name == name) {
                Some(t) => t.clone(),
                None => {
                    // Try fuzzy matching
                    if let Some(closest) = find_closest_template(&name, &templates) {
                        if can_prompt() {
                            let confirmed = prompt_confirm(
                                &format!(
                                    "Unknown template \"{name}\". Did you mean \"{}\"?",
                                    closest.name
                                ),
                                true,
                            )?;
                            if confirmed {
                                closest.clone()
                            } else {
                                return Err(AlienError::new(ErrorData::UserCancelled));
                            }
                        } else {
                            eprintln!("Unknown template: {name}");
                            eprintln!("Did you mean: {}", closest.name);
                            eprintln!();
                            eprintln!("Available templates:");
                            print_template_list(&templates);
                            return Err(AlienError::new(ErrorData::ValidationError {
                                field: "template".to_string(),
                                message: format!("Unknown template '{name}'"),
                            }));
                        }
                    } else {
                        eprintln!("Unknown template: {name}\n");
                        eprintln!("Available templates:");
                        print_template_list(&templates);
                        return Err(AlienError::new(ErrorData::ValidationError {
                            field: "template".to_string(),
                            message: format!("Unknown template '{name}'"),
                        }));
                    }
                }
            }
        }
        None => {
            if can_prompt() {
                let display_items = format_template_choices(&templates);
                let index = prompt_select_index("Select a template:", &display_items)?;
                templates[index].clone()
            } else {
                eprintln!("Template is required in non-interactive mode.\n");
                eprintln!("Available templates:");
                print_template_list(&templates);
                eprintln!("\nUsage: alien init <template> [directory]");
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "Template is required in non-interactive mode".to_string(),
                }));
            }
        }
    };

    // 3. Determine target directory
    let current_dir =
        std::env::current_dir()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "get current directory".to_string(),
                file_path: ".".to_string(),
                reason: "Failed to get current directory".to_string(),
            })?;
    let target_dir = destination_for_init(&current_dir, args.directory.as_deref());
    let directory = display_directory(&current_dir, &target_dir);

    // 4. Validate directory
    if target_dir.exists() {
        let is_empty = target_dir
            .read_dir()
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);

        if !is_empty && !args.force {
            return Err(AlienError::new(ErrorData::FileOperationFailed {
                operation: "create project".to_string(),
                file_path: target_dir.display().to_string(),
                reason: "Directory already exists and is not empty. Use --force to overwrite."
                    .to_string(),
            }));
        }

        if !is_empty && args.force {
            let targets_current_directory = target_dir
                .canonicalize()
                .ok()
                .zip(current_dir.canonicalize().ok())
                .is_some_and(|(target, current)| target == current);
            if targets_current_directory {
                return Err(AlienError::new(ErrorData::FileOperationFailed {
                    operation: "overwrite project".to_string(),
                    file_path: target_dir.display().to_string(),
                    reason: "Refusing to overwrite the current directory. Choose a child directory instead."
                        .to_string(),
                }));
            }

            std::fs::remove_dir_all(&target_dir)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "remove directory".to_string(),
                    file_path: target_dir.display().to_string(),
                    reason: "Failed to remove existing directory".to_string(),
                })?;
        }
    }

    // Resolve this before downloading the template so the template's own
    // lockfile does not override the package manager used by the host repository.
    let package_manager = package_manager_for(&target_dir);

    // 5. Download, set up, and install dependencies
    let steps = FixedSteps::new(&[
        "Download template",
        "Set up project",
        "Install dependencies",
    ]);

    steps.activate(0, Some(selected.name.clone()));
    download_and_extract(&selected.name, &target_dir).await?;
    steps.complete(0, Some("downloaded".to_string()));

    steps.activate(1, Some(directory.clone()));
    let package_name = target_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&selected.name);
    rewrite_package_json_name(&target_dir, package_name)?;
    remove_unused_template_lockfile(&target_dir, package_manager)?;
    steps.complete(1, Some("ready".to_string()));

    steps.activate(2, Some("installing...".to_string()));
    let installed =
        if !target_dir.join("package.json").exists() || is_extension_workspace(&target_dir) {
            // An extension's Cargo workspace, whose package.json only publishes it, or an app in a
            // subdirectory: the next steps say what to run, and where.
            steps.skip(2, Some("none here".to_string()));
            false
        } else {
            match install_dependencies(&target_dir, package_manager).await {
                Ok(true) => {
                    steps.complete(2, Some("done".to_string()));
                    true
                }
                _ => {
                    steps.skip(2, Some("run manually".to_string()));
                    false
                }
            }
        };
    drop(steps);

    // 6. Print success
    println!("{}", success_line("Project created."));
    println!("{} {}", dim_label("Directory"), accent(&directory));
    println!("{} {}", dim_label("Template "), accent(&selected.name));
    println!();
    println!("{}", dim_label("Next steps"));
    for step in next_steps(&target_dir, &directory, installed, package_manager) {
        println!("  {}", command(&step));
    }

    Ok(())
}

/// What to run after scaffolding, from what the template is: an extension's workspace is tested
/// with `alien extensions test`; an app runs with `alien dev` from the directory with its
/// `alien.ts`, which may be a subdirectory of the template, and an `alien.toml` project is
/// released.
fn next_steps(
    target_dir: &Path,
    directory: &str,
    installed: bool,
    package_manager: Option<&str>,
) -> Vec<String> {
    let cd = |relative: &str| {
        let path = match (directory, relative) {
            (".", "") => return None,
            (".", relative) => relative.to_string(),
            (directory, "") => directory.to_string(),
            (directory, relative) => format!("{directory}/{relative}"),
        };
        Some(format!("cd {path}"))
    };
    if is_extension_workspace(target_dir) {
        return cd("")
            .into_iter()
            .chain(["alien extensions test".to_string()])
            .collect();
    }
    let Some(app) = app_directory(target_dir) else {
        return cd("").into_iter().collect();
    };
    let relative = app
        .strip_prefix(target_dir)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut steps: Vec<String> = cd(&relative).into_iter().collect();
    let installed_here = installed && relative.is_empty();
    if app.join("package.json").exists() && !installed_here {
        let package_manager = if relative.is_empty() {
            package_manager
        } else {
            package_manager_for(&app)
        };
        steps.push(format!("{} install", package_manager.unwrap_or("npm")));
    }
    // An alien.toml project has no package.json: nothing to install and no dev server, so it
    // goes straight to a release.
    steps.push(if app.join("package.json").exists() {
        "alien dev".to_string()
    } else {
        "alien release".to_string()
    });
    steps
}

fn is_extension_workspace(target_dir: &Path) -> bool {
    std::fs::read_to_string(target_dir.join("Cargo.toml"))
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .is_some_and(|manifest| {
            manifest
                .get("workspace")
                .and_then(|workspace| workspace.get("metadata"))
                .and_then(|metadata| metadata.get("alien-extension"))
                .is_some()
        })
}

/// The directory with the app's `alien.ts` or `alien.toml`: the template's own, or the first of
/// its subdirectories, two levels deep, that has one.
fn app_directory(target_dir: &Path) -> Option<PathBuf> {
    let has_config = |directory: &Path| {
        directory.join("alien.ts").is_file() || directory.join("alien.toml").is_file()
    };
    if has_config(target_dir) {
        return Some(target_dir.to_path_buf());
    }
    let mut level = vec![target_dir.to_path_buf()];
    for _ in 0..2 {
        let mut next: Vec<PathBuf> = level
            .iter()
            .filter_map(|directory| std::fs::read_dir(directory).ok())
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.is_dir()
                    && path.file_name().is_some_and(|name| {
                        name != "node_modules" && !name.to_string_lossy().starts_with('.')
                    })
            })
            .collect();
        next.sort();
        if let Some(app) = next.iter().find(|path| has_config(path)) {
            return Some(app.clone());
        }
        level = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        destination_for_init, display_directory, extract_template, next_steps,
        pin_alien_crate_lines, template_archive_url, KNOWN_TEMPLATES,
    };
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn catalog_contains_every_scaffoldable_example() {
        let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut metadata_templates = BTreeSet::new();

        let examples = fs::read_dir(&examples_dir).expect("examples directory");
        let extensions = fs::read_dir(examples_dir.join("extensions")).expect("extensions");
        for entry in examples.chain(extensions) {
            let entry = entry.expect("example entry");
            let metadata_path = entry.path().join("template.toml");
            if !metadata_path.is_file() {
                continue;
            }

            let metadata = fs::read_to_string(&metadata_path).expect("template metadata");
            let metadata: toml::Value = toml::from_str(&metadata).expect("valid template metadata");
            let name = metadata
                .get("name")
                .and_then(toml::Value::as_str)
                .expect("template name");
            metadata_templates.insert(name.to_string());
        }

        let catalog_templates = KNOWN_TEMPLATES
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect::<BTreeSet<_>>();

        assert_eq!(catalog_templates, metadata_templates);
    }

    #[test]
    fn initializes_into_alien_directory_inside_existing_repository() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(temporary.path().join("package.json"), "{}").expect("package file");

        let destination = destination_for_init(temporary.path(), None);

        assert_eq!(destination, temporary.path().join("alien"));
        assert_eq!(display_directory(temporary.path(), &destination), "alien");
    }

    #[test]
    fn initializes_in_place_inside_empty_directory() {
        let temporary = tempfile::tempdir().expect("temporary directory");

        let destination = destination_for_init(temporary.path(), None);

        assert_eq!(destination, temporary.path());
        assert_eq!(display_directory(temporary.path(), &destination), ".");
    }

    #[test]
    fn explicit_directory_wins() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(temporary.path().join("Cargo.toml"), "").expect("cargo file");

        let destination = destination_for_init(temporary.path(), Some("packages/private-runtime"));

        assert_eq!(
            destination,
            temporary.path().join("packages/private-runtime")
        );
    }

    #[test]
    fn points_alien_crates_at_the_release_tag_and_leaves_the_rest() {
        // The comment right above a rewritten line describes the repository's build, so it goes;
        // other comments stay.
        let manifest = r#"[workspace.dependencies]
# In the repository, the example builds against the SDK beside it.
alien-extension-sdk = { path = "../../../crates/alien-extension-sdk" }
# The types crate.
acme-types = { path = "types" }
alien-core = { path = "../../../crates/alien-other" }
"#;
        assert_eq!(
            pin_alien_crate_lines(manifest, "3.4.0"),
            r#"[workspace.dependencies]
alien-extension-sdk = { git = "https://github.com/alienplatform/alien", tag = "v3.4.0" }
# The types crate.
acme-types = { path = "types" }
alien-core = { path = "../../../crates/alien-other" }
"#
        );
    }

    // A canary's version has a pre-release suffix and no tag in the repository.
    #[test]
    fn a_cli_without_a_release_tag_follows_main_and_says_so() {
        let manifest = "alien-extension-sdk = { path = \"../../../crates/alien-extension-sdk\" }\n";
        assert_eq!(
            pin_alien_crate_lines(manifest, "3.3.31-00000001"),
            "# alien 3.3.31-00000001 has no release tag, so this follows main. Pin a release with tag = \"v<version>\".\n\
             alien-extension-sdk = { git = \"https://github.com/alienplatform/alien\", branch = \"main\" }\n"
        );
        for version in ["3.4", "3.4.0.1", "v3.4.0", "3.4.x"] {
            assert!(
                pin_alien_crate_lines(manifest, version).contains(r#"branch = "main""#),
                "{version}"
            );
        }
    }

    /// The repository's `main` archive as GitHub serves it, with only `examples/<template>/`.
    fn archive(template: &str) -> Vec<u8> {
        archive_at("alien-main", template)
    }

    /// The archive GitHub serves for a reference, whose top directory is `top`.
    fn archive_at(top: &str, template: &str) -> Vec<u8> {
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut directories = vec![examples.join(template)];
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap();
                if name == "target" || name == "generated" || name == "node_modules" {
                    continue;
                }
                if path.is_dir() {
                    directories.push(path);
                } else {
                    let relative = path.strip_prefix(&examples).unwrap();
                    builder
                        .append_path_with_name(
                            &path,
                            PathBuf::from(top).join("examples").join(relative),
                        )
                        .unwrap();
                }
            }
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn scaffolds_each_extension_template_against_the_release_tag() {
        let version = crate::CLI_VERSION;
        for template in [
            "extensions/postgres-iam-auth",
            "extensions/postgres-backup-retention",
            "extensions/resource-tags",
            "extensions/budget-alert",
            "extensions/dns-zone",
        ] {
            let target = tempfile::tempdir().unwrap();
            extract_template(&archive(template), template, target.path()).unwrap();

            assert!(!target.path().join("template.toml").exists(), "{template}");
            assert!(
                target.path().join("hooks/src/lib.rs").is_file(),
                "{template}"
            );
            let manifest = fs::read_to_string(target.path().join("Cargo.toml")).unwrap();
            assert!(
                manifest.contains(&format!(
                    "alien-extension-sdk = {{ git = \"https://github.com/alienplatform/alien\", tag = \"v{version}\" }}\n"
                )),
                "{template}:\n{manifest}"
            );
            assert!(
                !manifest.contains("../../crates"),
                "{template}:\n{manifest}"
            );
            assert!(
                !manifest.contains("In the") && !manifest.contains("repository"),
                "{template}:\n{manifest}"
            );
            assert_eq!(
                next_steps(target.path(), "my-extension", false, Some("pnpm")),
                vec!["cd my-extension", "alien extensions test"],
                "{template}"
            );
        }
    }

    #[test]
    fn a_release_cli_downloads_templates_from_its_tag() {
        assert_eq!(
            template_archive_url("3.4.0"),
            "https://codeload.github.com/alienplatform/alien/tar.gz/refs/tags/v3.4.0"
        );
        assert_eq!(
            template_archive_url("3.3.31-00000001"),
            "https://codeload.github.com/alienplatform/alien/tar.gz/main"
        );

        // A tag's archive is named after the version, not `main`.
        let target = tempfile::tempdir().unwrap();
        extract_template(
            &archive_at("alien-3.4.0", "extensions/postgres-backup-retention"),
            "extensions/postgres-backup-retention",
            target.path(),
        )
        .unwrap();
        assert!(target.path().join("hooks/src/lib.rs").is_file());
    }

    #[test]
    fn an_app_template_installs_and_runs_alien_dev() {
        let target = tempfile::tempdir().unwrap();
        extract_template(
            &archive("basic-worker-ts"),
            "basic-worker-ts",
            target.path(),
        )
        .unwrap();
        assert_eq!(
            next_steps(target.path(), "my-app", false, Some("pnpm")),
            vec!["cd my-app", "pnpm install", "alien dev"]
        );
        assert_eq!(
            next_steps(target.path(), ".", true, Some("pnpm")),
            vec!["alien dev"]
        );
    }

    // The app's alien.ts is in packages/remote-agent, with its own lockfile.
    #[test]
    fn an_app_in_a_subdirectory_starts_there() {
        let target = tempfile::tempdir().unwrap();
        extract_template(&archive("github-agent"), "github-agent", target.path()).unwrap();
        let steps = next_steps(target.path(), "agent", false, Some("pnpm"));
        assert_eq!(steps[0], "cd agent/packages/remote-agent");
        assert!(steps[1].ends_with(" install"), "{steps:?}");
        assert_eq!(steps[2], "alien dev");
        assert_eq!(steps.len(), 3);
    }

    // A declarative alien.toml project has no package.json to install.
    #[test]
    fn an_alien_toml_project_skips_install_and_releases() {
        let target = tempfile::tempdir().unwrap();
        std::fs::write(target.path().join("alien.toml"), "name = \"worker\"\n").unwrap();
        assert_eq!(
            next_steps(target.path(), "worker", false, Some("npm")),
            vec!["cd worker", "alien release"]
        );
    }
}
