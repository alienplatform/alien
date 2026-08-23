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

async fn download_and_extract(template: &str, target_dir: &Path) -> Result<()> {
    let url = "https://codeload.github.com/alienplatform/alien/tar.gz/main";
    let prefix = format!("alien-main/examples/{}/", template);

    let client = reqwest::Client::new();
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

    let decoder = GzDecoder::new(&bytes[..]);
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

        let path_str = path.to_string_lossy().to_string();

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

    Ok(())
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
    let installed = match install_dependencies(&target_dir, package_manager).await {
        Ok(true) => {
            steps.complete(2, Some("done".to_string()));
            true
        }
        _ => {
            steps.skip(2, Some("run manually".to_string()));
            false
        }
    };
    drop(steps);

    // 6. Print success
    println!("{}", success_line("Project created."));
    println!("{} {}", dim_label("Directory"), accent(&directory));
    println!("{} {}", dim_label("Template "), accent(&selected.name));
    println!();
    println!("{}", dim_label("Next steps"));
    if directory != "." {
        println!("  cd {directory}");
    }
    if !installed {
        println!("  {} install", package_manager.unwrap_or("npm"));
    }
    println!("  {}", command("alien dev"));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{destination_for_init, display_directory};
    use std::fs;

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
}
