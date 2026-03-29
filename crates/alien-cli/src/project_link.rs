//! Project linking functionality for connecting local directories to Alien projects.

use crate::auth::AuthHttp;
use crate::error::{ErrorData, Result};
use crate::git_utils;
use crate::output::{can_prompt, prompt_confirm, prompt_select, prompt_text};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types;
use alien_platform_api::SdkResultExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const ALIEN_DIR: &str = ".alien";
const PROJECT_FILE: &str = "project.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLink {
    pub workspace: String,
    pub project_id: String,
    pub project_name: String,
    pub root_directory: Option<String>,
}

impl ProjectLink {
    pub fn new(workspace: String, project_id: String, project_name: String) -> Self {
        Self {
            workspace,
            project_id,
            project_name,
            root_directory: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_root_directory(mut self, root_directory: Option<String>) -> Self {
        self.root_directory = root_directory;
        self
    }
}

#[derive(Debug)]
pub enum ProjectLinkStatus {
    Linked(ProjectLink),
    NotLinked,
    Error(String),
}

pub fn get_project_link_status<P: AsRef<Path>>(dir: P) -> ProjectLinkStatus {
    let project_file = dir.as_ref().join(ALIEN_DIR).join(PROJECT_FILE);

    if !project_file.exists() {
        return ProjectLinkStatus::NotLinked;
    }

    match fs::read_to_string(&project_file) {
        Ok(content) => match serde_json::from_str::<ProjectLink>(&content) {
            Ok(link) => ProjectLinkStatus::Linked(link),
            Err(error) => ProjectLinkStatus::Error(format!("Invalid project link file: {error}")),
        },
        Err(error) => ProjectLinkStatus::Error(format!("Failed to read project link file: {error}")),
    }
}

pub fn save_project_link<P: AsRef<Path>>(dir: P, link: &ProjectLink) -> Result<()> {
    let alien_dir = dir.as_ref().join(ALIEN_DIR);
    let project_file = alien_dir.join(PROJECT_FILE);

    fs::create_dir_all(&alien_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: alien_dir.display().to_string(),
            reason: "Failed to create .alien directory".to_string(),
        })?;

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
            reason: "Failed to write project link".to_string(),
        })?;

    add_to_gitignore(dir.as_ref())?;
    Ok(())
}

pub fn remove_project_link<P: AsRef<Path>>(dir: P) -> Result<()> {
    let project_file = dir.as_ref().join(ALIEN_DIR).join(PROJECT_FILE);
    if project_file.exists() {
        fs::remove_file(&project_file)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "remove".to_string(),
                file_path: project_file.display().to_string(),
                reason: "Failed to remove project link".to_string(),
            })?;
    }

    Ok(())
}

fn add_to_gitignore(dir: &Path) -> Result<()> {
    let gitignore_path = dir.join(".gitignore");
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

    if content.lines().any(|line| line.trim() == ".alien") {
        return Ok(());
    }

    let next = if content.ends_with('\n') {
        format!("{content}.alien\n")
    } else {
        format!("{content}\n.alien\n")
    };

    fs::write(&gitignore_path, next)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: gitignore_path.display().to_string(),
            reason: "Failed to update .gitignore".to_string(),
        })?;

    Ok(())
}

pub async fn choose_or_create_project(
    http: &AuthHttp,
    workspace: &str,
    suggested_name: Option<&str>,
    dir: &Path,
    allow_prompt: bool,
) -> Result<types::ProjectListItemResponse> {
    let client = http.sdk_client();
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
    if existing_projects.is_empty() {
        return create_new_project(client, workspace, suggested_name, dir, allow_prompt).await;
    }

    if !allow_prompt || !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Project selection requires a real terminal. Pass `--project <name>` to use an existing project or run `alien link` interactively."
                    .to_string(),
        }));
    }

    let mut choices: Vec<String> = existing_projects
        .iter()
        .map(|project| project.name.as_str().to_string())
        .collect();
    choices.push("Create new project".to_string());

    let selected = prompt_select("Link this directory to which project?", &choices)?;
    if selected == "Create new project" {
        create_new_project(client, workspace, suggested_name, dir, true).await
    } else {
        existing_projects
            .into_iter()
            .find(|project| project.name.as_str() == selected)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Selected project was not found".to_string(),
                })
            })
    }
}

pub async fn create_new_project(
    client: &alien_platform_api::Client,
    workspace: &str,
    suggested_name: Option<&str>,
    dir: &Path,
    allow_prompt: bool,
) -> Result<types::ProjectListItemResponse> {
    let project_name = match suggested_name {
        Some(name) if allow_prompt && can_prompt() => prompt_text("Project name", Some(name))?,
        Some(name) => name.to_string(),
        None if allow_prompt && can_prompt() => prompt_text("Project name", None)?,
        None => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message:
                    "Project creation needs a name. Pass `alien link --name <project-name>` or run `alien link` in a real terminal."
                        .to_string(),
            }))
        }
    };

    if project_name.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "project_name".to_string(),
            message: "Project name cannot be empty".to_string(),
        }));
    }

    let git_repository = match git_utils::detect_git_repository(dir) {
        Ok(Some(repo_info)) => Some(repo_info),
        Ok(None) => None,
        Err(error) => {
            println!("Warning: failed to detect git repository: {error}");
            None
        }
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
        .body(&types::CreateProjectBody {
            name: types::CreateProjectBodyName::try_from(project_name.clone())
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: "project_name".to_string(),
                    message: "Invalid project name format".to_string(),
                })?,
            git_repository,
            root_directory: None,
            packages_config: None,
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create project".to_string(),
            url: None,
        })?
        .into_inner();

    Ok(types::ProjectListItemResponse {
        id: types::ProjectListItemResponseId::try_from(response.id.as_str())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "project_id".to_string(),
                message: "Invalid project ID in response".to_string(),
            })?,
        name: types::ProjectListItemResponseName::try_from(response.name.as_str())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "project_name".to_string(),
                message: "Invalid project name in response".to_string(),
            })?,
        workspace_id: types::ProjectListItemResponseWorkspaceId::try_from(
            response.workspace_id.as_str(),
        )
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace_id".to_string(),
            message: "Invalid workspace ID in response".to_string(),
        })?,
        domain_id: response
            .domain_id
            .map(|domain_id| {
                types::ProjectListItemResponseDomainId::try_from(domain_id.as_str())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "domain_id".to_string(),
                        message: "Invalid domain ID in response".to_string(),
                    })
            })
            .transpose()?,
        created_at: response.created_at,
        root_directory: response
            .root_directory
            .map(|root_directory| {
                types::ProjectListItemResponseRootDirectory::try_from(root_directory.as_str())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "root_directory".to_string(),
                        message: "Invalid root directory in response".to_string(),
                    })
            })
            .transpose()?,
        git_repository: response
            .git_repository
            .map(|git_repository| -> Result<types::ProjectListItemResponseGitRepository> {
                Ok(types::ProjectListItemResponseGitRepository {
                    repo: types::ProjectListItemResponseGitRepositoryRepo::try_from(
                        git_repository.repo.as_str(),
                    )
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "git_repository_repo".to_string(),
                        message: "Invalid git repository in response".to_string(),
                    })?,
                    type_: match git_repository.type_ {
                        types::CreateProjectResponseGitRepositoryType::Github => {
                            types::ProjectListItemResponseGitRepositoryType::Github
                        }
                    },
                })
            })
            .transpose()?,
        deployment_page_background: None,
        packages_config: None,
        deployment_count: Some(0.0),
        latest_release: None.into(),
    })
}

pub fn suggest_project_name<P: AsRef<Path>>(dir: P) -> String {
    dir.as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("alien-project")
        .replace(' ', "-")
        .to_ascii_lowercase()
}

pub async fn ensure_project_linked<P: AsRef<Path>>(
    dir: P,
    http: &AuthHttp,
    workspace: &str,
    allow_prompt: bool,
) -> Result<ProjectLink> {
    match get_project_link_status(&dir) {
        ProjectLinkStatus::Linked(link) => validate_linked_project(http, workspace, link, allow_prompt).await,
        ProjectLinkStatus::NotLinked => {
            if !allow_prompt || !can_prompt() {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message:
                        "No project is linked to this directory. Run `alien link`, or pass `--project <name>`."
                            .to_string(),
                }));
            }

            let dir = dir.as_ref();
            if !prompt_confirm(&format!("Set up and link \"{}\"?", dir.display()), true)? {
                return Err(AlienError::new(ErrorData::UserCancelled));
            }

            let project = choose_or_create_project(
                http,
                workspace,
                Some(&suggest_project_name(dir)),
                dir,
                true,
            )
            .await?;

            let link = ProjectLink::new(
                workspace.to_string(),
                project.id.as_str().to_string(),
                project.name.as_str().to_string(),
            );
            save_project_link(dir, &link)?;
            Ok(link)
        }
        ProjectLinkStatus::Error(error) => Err(AlienError::new(ErrorData::ProjectLinkInvalid {
            message: error,
        })),
    }
}

async fn validate_linked_project(
    http: &AuthHttp,
    workspace: &str,
    link: ProjectLink,
    allow_prompt: bool,
) -> Result<ProjectLink> {
    let client = http.sdk_client();
    let workspace_param = types::GetProjectWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "Invalid workspace name".to_string(),
        })?;

    let project_id = types::ProjectIdOrNamePathParam::try_from(link.project_id.clone())
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "project".to_string(),
            message: "Invalid project link".to_string(),
        })?;

    match client
        .get_project()
        .workspace(&workspace_param)
        .id_or_name(&project_id)
        .send()
        .await
    {
        Ok(_) => Ok(link),
        Err(_) if allow_prompt && can_prompt() => Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "The linked project no longer exists. Run `alien link --force` to choose a new project."
                    .to_string(),
        })),
        Err(_) => Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "The linked project no longer exists. Run `alien link --force` or pass `--project <name>`."
                    .to_string(),
        })),
    }
}

pub async fn get_project_by_name(
    http: &AuthHttp,
    workspace: &str,
    project_name: &str,
) -> Result<ProjectLink> {
    let client = http.sdk_client();
    let workspace_param = types::ListProjectsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: format!("Invalid workspace name format: '{workspace}'"),
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

    let project = response
        .into_inner()
        .items
        .into_iter()
        .find(|project| project.name.as_str() == project_name || project.id.as_str() == project_name)
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidProjectName {
                project_name: project_name.to_string(),
                reason: format!("Project '{project_name}' not found in workspace '{workspace}'"),
            })
        })?;

    Ok(ProjectLink::new(
        workspace.to_string(),
        project.id.as_str().to_string(),
        project.name.as_str().to_string(),
    ))
}
