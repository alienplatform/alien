//! Project linking functionality for connecting local directories to Alien projects
//!
//! Similar to Vercel's .vercel directory, this creates a .alien directory containing:
//! - workspace: The workspace ID/name
//! - project_id: The linked project ID
//! - project_name: The project name (for display)

use crate::auth::AuthHttp;
use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::git_utils;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types;
use alien_platform_api::SdkResultExt;
use ratatui::{prelude::*, widgets::Paragraph, TerminalOptions, Viewport};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

const ALIEN_DIR: &str = ".alien";
const PROJECT_FILE: &str = "project.json";

/// Project link configuration stored in .alien/project.json
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLink {
    /// The workspace containing this project
    pub workspace: String,
    /// The project ID
    pub project_id: String,
    /// The project name (for display purposes)
    pub project_name: String,
    /// Optional root directory within the project
    pub root_directory: Option<String>,
}

impl ProjectLink {
    /// Create a new project link
    pub fn new(workspace: String, project_id: String, project_name: String) -> Self {
        Self {
            workspace,
            project_id,
            project_name,
            root_directory: None,
        }
    }

    /// Set the root directory for this project link
    #[allow(dead_code)]
    pub fn with_root_directory(mut self, root_directory: Option<String>) -> Self {
        self.root_directory = root_directory;
        self
    }
}

/// Status of project linking for a directory
#[derive(Debug)]
pub enum ProjectLinkStatus {
    /// Directory is linked to a project
    Linked(ProjectLink),
    /// Directory is not linked to any project
    NotLinked,
    /// Error reading link status
    Error(String),
}

/// Get the project link status for the current directory
pub fn get_project_link_status<P: AsRef<Path>>(dir: P) -> ProjectLinkStatus {
    let alien_dir = dir.as_ref().join(ALIEN_DIR);
    let project_file = alien_dir.join(PROJECT_FILE);

    if !project_file.exists() {
        return ProjectLinkStatus::NotLinked;
    }

    match fs::read_to_string(&project_file) {
        Ok(content) => match serde_json::from_str::<ProjectLink>(&content) {
            Ok(link) => ProjectLinkStatus::Linked(link),
            Err(e) => ProjectLinkStatus::Error(format!("Invalid project link file: {}", e)),
        },
        Err(e) => ProjectLinkStatus::Error(format!("Failed to read project link file: {}", e)),
    }
}

/// Save a project link to the .alien directory
pub fn save_project_link<P: AsRef<Path>>(dir: P, link: &ProjectLink) -> Result<()> {
    let alien_dir = dir.as_ref().join(ALIEN_DIR);
    let project_file = alien_dir.join(PROJECT_FILE);

    // Create .alien directory if it doesn't exist
    fs::create_dir_all(&alien_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: ALIEN_DIR.to_string(),
            reason: format!("Failed to create {} directory", ALIEN_DIR),
        })?;

    // Write project link file
    let content = serde_json::to_string_pretty(link)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize project link".to_string(),
        })?;

    fs::write(&project_file, content)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: project_file.display().to_string(),
            reason: format!("Failed to write {}", project_file.display()),
        })?;

    // Add .alien to .gitignore if it exists and doesn't already contain it
    add_to_gitignore(dir.as_ref())?;

    Ok(())
}

/// Remove project link from directory
pub fn remove_project_link<P: AsRef<Path>>(dir: P) -> Result<()> {
    let alien_dir = dir.as_ref().join(ALIEN_DIR);
    let project_file = alien_dir.join(PROJECT_FILE);

    // Only remove the project.json file, not the entire .alien directory
    // This preserves build artifacts and other important files
    if project_file.exists() {
        fs::remove_file(&project_file).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "remove".to_string(),
                file_path: project_file.display().to_string(),
                reason: format!("Failed to remove {}", project_file.display()),
            },
        )?;
    }

    Ok(())
}

/// Add .alien to .gitignore if it exists and doesn't already contain it
fn add_to_gitignore<P: AsRef<Path>>(dir: P) -> Result<()> {
    let gitignore_path = dir.as_ref().join(".gitignore");

    // Only update if .gitignore exists
    if !gitignore_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&gitignore_path)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: gitignore_path.display().to_string(),
            reason: "Failed to read .gitignore".to_string(),
        })?;

    // Check if .alien is already in .gitignore
    if content.lines().any(|line| line.trim() == ".alien") {
        return Ok(());
    }

    // Add .alien to .gitignore
    let new_content = if content.ends_with('\n') {
        format!("{}.alien\n", content)
    } else {
        format!("{}\n.alien\n", content)
    };

    fs::write(&gitignore_path, new_content)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: gitignore_path.display().to_string(),
            reason: "Failed to update .gitignore".to_string(),
        })?;

    Ok(())
}

/// Interactive project selection with TUI similar to workspace selection
pub async fn interactive_project_selection(
    http: &AuthHttp,
    workspace: &str,
    suggested_name: Option<&str>,
) -> Result<types::ProjectListItemResponse> {
    let client = http.sdk_client();

    // List existing projects in the workspace
    let workspace_param = types::ListProjectsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "Invalid workspace name".to_string(),
        })?;
    let response = client
        .list_projects()
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list projects".to_string(),
            url: None,
        })?;

    let existing_projects = response.into_inner().items;

    // If no existing projects, skip to creating new one
    if existing_projects.is_empty() {
        return create_new_project(client, workspace, suggested_name, get_current_dir()?).await;
    }

    // Check if we can use TUI (TTY available)
    if !std::io::stderr().is_terminal() || !std::io::stdout().is_terminal() {
        return project_selection_console(client, workspace, &existing_projects, suggested_name)
            .await;
    }

    // Use TUI for project selection
    project_selection_tui(client, workspace, existing_projects, suggested_name).await
}

/// Console-based project selection fallback
async fn project_selection_console(
    client: &alien_platform_api::Client,
    workspace: &str,
    existing_projects: &[types::ProjectListItemResponse],
    suggested_name: Option<&str>,
) -> Result<types::ProjectListItemResponse> {
    println!(
        "Found {} existing project(s) in workspace '{}':",
        existing_projects.len(),
        workspace
    );
    for (i, project) in existing_projects.iter().enumerate() {
        println!("  [{}] {}", i + 1, project.name.as_str());
    }
    println!("  [{}] Create new project", existing_projects.len() + 1);

    print!("Select an option [1-{}]: ", existing_projects.len() + 1);
    use std::io::{self, Write};
    let _ = io::stdout().flush();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: "stdin".to_string(),
            reason: "Failed to read user input".to_string(),
        })?;

    let idx: usize =
        input
            .trim()
            .parse()
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "selection".to_string(),
                message: "Expected a number".to_string(),
            })?;

    if idx == 0 || idx > existing_projects.len() + 1 {
        return Err(AlienError::new(ErrorData::UserCancelled));
    }

    if idx == existing_projects.len() + 1 {
        // Create new project
        create_new_project(client, workspace, suggested_name, get_current_dir()?).await
    } else {
        // Use existing project
        Ok(existing_projects[idx - 1].clone())
    }
}

/// TUI-based project selection
async fn project_selection_tui(
    client: &alien_platform_api::Client,
    workspace: &str,
    existing_projects: Vec<types::ProjectListItemResponse>,
    suggested_name: Option<&str>,
) -> Result<types::ProjectListItemResponse> {
    // Create choices: existing projects + "Create new project" option
    let mut choices = Vec::new();
    for project in &existing_projects {
        choices.push(format!("{}", project.name.as_str()));
    }
    choices.push("Create new project".to_string());

    let terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(choices.len() as u16 + 3), // Title + empty line + options + bottom margin
    });

    let result = project_selection_tui_impl(terminal, choices).await;
    ratatui::restore();

    match result {
        Ok(selected_idx) => {
            if selected_idx == existing_projects.len() {
                // Create new project was selected
                create_new_project(client, workspace, suggested_name, get_current_dir()?).await
            } else {
                // Existing project was selected
                Ok(existing_projects[selected_idx].clone())
            }
        }
        Err(e) => Err(e),
    }
}

/// TUI implementation for project selection
async fn project_selection_tui_impl(
    mut terminal: ratatui::DefaultTerminal,
    choices: Vec<String>,
) -> Result<usize> {
    let mut selected = 0;

    loop {
        terminal
            .draw(|frame: &mut ratatui::Frame| {
                let area = frame.area();

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(0)
                    .constraints([
                        Constraint::Length(1),                    // Title with prompt
                        Constraint::Length(1),                    // Empty line
                        Constraint::Length(choices.len() as u16), // Options
                        Constraint::Length(1),                    // Bottom margin
                    ])
                    .split(area);

                // Combined title and prompt
                let title_prompt = Paragraph::new("Link to existing project or create new one:")
                    .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
                frame.render_widget(title_prompt, chunks[0]);

                // Simple list without borders
                for (i, choice) in choices.iter().enumerate() {
                    let prefix = if selected == i { "▶ " } else { "  " };
                    let line = format!("{}{}", prefix, choice);
                    let style = if selected == i {
                        Style::default().fg(Color::Rgb(34, 197, 94)).bold()
                    } else {
                        Style::default().fg(Color::Rgb(156, 163, 175))
                    };

                    let item = Paragraph::new(line).style(style);
                    let item_area = Rect {
                        x: chunks[2].x,
                        y: chunks[2].y + i as u16,
                        width: chunks[2].width,
                        height: 1,
                    };
                    frame.render_widget(item, item_area);
                }
            })
            .into_alien_error()
            .context(ErrorData::TuiOperationFailed {
                message: "Failed to draw TUI".to_string(),
            })?;

        // Handle input
        if crossterm::event::poll(Duration::from_millis(100))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "poll".to_string(),
                file_path: "stdin".to_string(),
                reason: "Failed to poll for input events".to_string(),
            })?
        {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: "stdin".to_string(),
                reason: "Failed to read input event".to_string(),
            })? {
                match key.code {
                    crossterm::event::KeyCode::Up => {
                        if selected > 0 {
                            selected -= 1;
                        } else {
                            selected = choices.len() - 1;
                        }
                    }
                    crossterm::event::KeyCode::Down => {
                        if selected < choices.len() - 1 {
                            selected += 1;
                        } else {
                            selected = 0;
                        }
                    }
                    crossterm::event::KeyCode::Enter => {
                        return Ok(selected);
                    }
                    crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                        return Err(AlienError::new(ErrorData::UserCancelled));
                    }
                    crossterm::event::KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        return Err(AlienError::new(ErrorData::UserCancelled));
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Create a new project with optional suggested name
async fn create_new_project<P: AsRef<Path>>(
    client: &alien_platform_api::Client,
    workspace: &str,
    suggested_name: Option<&str>,
    dir: P,
) -> Result<types::ProjectListItemResponse> {
    let project_name = if let Some(name) = suggested_name {
        // Present suggested name and allow editing
        print!("Project name [{}]: ", name);
        use std::io::{self, Write};
        let _ = io::stdout().flush();

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: "stdin".to_string(),
                reason: "Failed to read user input".to_string(),
            })?;

        let trimmed_input = input.trim();
        if trimmed_input.is_empty() {
            // User pressed Enter without typing anything - use suggested name
            name.to_string()
        } else {
            // User provided a custom name
            trimmed_input.to_string()
        }
    } else {
        // Ask for project name
        print!("Enter project name: ");
        use std::io::{self, Write};
        let _ = io::stdout().flush();

        let mut name = String::new();
        io::stdin()
            .read_line(&mut name)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: "stdin".to_string(),
                reason: "Failed to read project name".to_string(),
            })?;
        name.trim().to_string()
    };

    if project_name.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "project_name".to_string(),
            message: "Project name cannot be empty".to_string(),
        }));
    }

    // Detect git repository information
    let git_repository = match git_utils::detect_git_repository(&dir) {
        Ok(Some(repo_info)) => {
            let provider_name = match repo_info.type_ {
                types::CreateProjectBodyGitRepositoryType::Github => "github",
            };
            println!(
                "🔍 Detected git repository: {} ({})",
                *repo_info.repo, provider_name
            );
            Some(repo_info)
        }
        Ok(None) => {
            println!(
                "ℹ️  No supported git repository detected (only GitHub is currently supported)"
            );
            None
        }
        Err(e) => {
            println!("⚠️  Warning: Failed to detect git repository: {}", e);
            None
        }
    };

    // Create the project
    let create_request = types::CreateProjectBody {
        name: types::CreateProjectBodyName::try_from(project_name.clone())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "project_name".to_string(),
                message: "Invalid project name format".to_string(),
            })?,
        git_repository,
        root_directory: None,
        packages_config: None,
    };

    let workspace_param = types::CreateProjectWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "Invalid workspace name".to_string(),
        })?;
    let response = client
        .create_project()
        .workspace(&workspace_param)
        .body(&create_request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create project".to_string(),
            url: None,
        })?;
    let response_inner = response.into_inner();

    println!("✅ Created new project: {}", project_name);

    // Convert response to ProjectListItemResponse format
    // Note: We need to convert the newtype wrappers manually since they're different types
    Ok(types::ProjectListItemResponse {
        id: types::ProjectListItemResponseId::try_from(response_inner.id.as_str())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "project_id".to_string(),
                message: "Invalid project ID in response".to_string(),
            })?,
        name: types::ProjectListItemResponseName::try_from(response_inner.name.as_str())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "project_name".to_string(),
                message: "Invalid project name in response".to_string(),
            })?,
        workspace_id: types::ProjectListItemResponseWorkspaceId::try_from(
            response_inner.workspace_id.as_str(),
        )
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace_id".to_string(),
            message: "Invalid workspace ID in response".to_string(),
        })?,
        domain_id: response_inner
            .domain_id
            .map(|did| {
                types::ProjectListItemResponseDomainId::try_from(did.as_str())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "domain_id".to_string(),
                        message: "Invalid domain ID in response".to_string(),
                    })
            })
            .transpose()?,
        created_at: response_inner.created_at,
        root_directory: response_inner
            .root_directory
            .map(|rd| {
                types::ProjectListItemResponseRootDirectory::try_from(rd.as_str())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "root_directory".to_string(),
                        message: "Invalid root directory in response".to_string(),
                    })
            })
            .transpose()?,
        git_repository: response_inner
            .git_repository
            .map(
                |gr| -> Result<types::ProjectListItemResponseGitRepository> {
                    Ok(types::ProjectListItemResponseGitRepository {
                        repo: types::ProjectListItemResponseGitRepositoryRepo::try_from(
                            gr.repo.as_str(),
                        )
                        .into_alien_error()
                        .context(ErrorData::ValidationError {
                            field: "git_repository_repo".to_string(),
                            message: "Invalid git repository in response".to_string(),
                        })?,
                        type_: match gr.type_ {
                            types::CreateProjectResponseGitRepositoryType::Github => {
                                types::ProjectListItemResponseGitRepositoryType::Github
                            }
                        },
                    })
                },
            )
            .transpose()?,
        deployment_page_background: None,
        packages_config: None,
        deployment_count: Some(0.0), // New project has no deployments yet
        latest_release: None.into(), // New project has no releases yet
    })
}

/// Suggest a project name based on the current directory
pub fn suggest_project_name<P: AsRef<Path>>(dir: P) -> String {
    dir.as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("alien-project")
        .to_string()
        .replace(" ", "-")
        .to_lowercase()
}

/// Ensure project is linked, prompt for linking if not
pub async fn ensure_project_linked<P: AsRef<Path>>(
    dir: P,
    http: &AuthHttp,
    workspace: &str,
) -> Result<ProjectLink> {
    match get_project_link_status(&dir) {
        ProjectLinkStatus::Linked(link) => {
            // Verify the project still exists
            let client = http.sdk_client();
            let workspace_param = types::GetProjectWorkspace::try_from(workspace)
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: "workspace".to_string(),
                    message: "Invalid workspace name".to_string(),
                })?;
            let project_id_param =
                types::ProjectIdOrNamePathParam::try_from(link.project_id.clone())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "project".to_string(),
                        message: "Invalid project format".to_string(),
                    })?;
            match client
                .get_project()
                .id_or_name(&project_id_param)
                .workspace(&workspace_param)
                .send()
                .await
            {
                Ok(_) => Ok(link),
                Err(_) => {
                    // Project no longer exists, remove stale link and re-link
                    println!("⚠️  Linked project no longer exists, re-linking...");
                    remove_project_link(&dir)?;
                    link_project(dir, http, workspace).await
                }
            }
        }
        ProjectLinkStatus::NotLinked => link_project(dir, http, workspace).await,
        ProjectLinkStatus::Error(err) => Err(AlienError::new(ErrorData::ProjectLinkInvalid {
            message: err,
        })),
    }
}

/// Link a directory to a project
async fn link_project<P: AsRef<Path>>(
    dir: P,
    http: &AuthHttp,
    workspace: &str,
) -> Result<ProjectLink> {
    let suggested_name = suggest_project_name(&dir);
    let dir_display = dir.as_ref().display();

    // Ask for confirmation to set up the project
    print!("Set up and link \"{}\"? [Y/n] ", dir_display);
    use std::io::{self, Write};
    let _ = io::stdout().flush();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: "stdin".to_string(),
            reason: "Failed to read user input".to_string(),
        })?;

    if input.trim().to_lowercase() == "n" || input.trim().to_lowercase() == "no" {
        return Err(AlienError::new(ErrorData::UserCancelled));
    }

    let project = interactive_project_selection(http, workspace, Some(&suggested_name)).await?;

    let link = ProjectLink::new(
        workspace.to_string(),
        project.id.as_str().to_string(),
        project.name.as_str().to_string(),
    );

    save_project_link(&dir, &link)?;

    println!(
        "🔗 Linked to {}/{} (created {} and added it to .gitignore)",
        workspace,
        project.name.as_str(),
        ALIEN_DIR
    );

    Ok(link)
}

/// Get project by name from the workspace
pub async fn get_project_by_name(
    http: &AuthHttp,
    workspace: &str,
    project_name: &str,
) -> Result<ProjectLink> {
    let client = http.sdk_client();

    // List projects in the workspace
    let workspace_param = types::ListProjectsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: format!("Invalid workspace name format: '{}'", workspace),
        })?;

    let response = client
        .list_projects()
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list projects".to_string(),
            url: None,
        })?;
    let projects_response = response.into_inner();

    // Find project by name
    let project = projects_response
        .items
        .into_iter()
        .find(|p| p.name.as_str() == project_name)
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidProjectName {
                project_name: project_name.to_string(),
                reason: format!(
                    "Project '{}' not found in workspace '{}'",
                    project_name, workspace
                ),
            })
        })?;

    // Create a ProjectLink-like structure
    Ok(ProjectLink::new(
        workspace.to_string(),
        project.id.as_str().to_string(),
        project.name.as_str().to_string(),
    ))
}
