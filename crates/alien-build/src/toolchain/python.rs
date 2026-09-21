use super::docker::DockerToolchain;
use super::{Toolchain, ToolchainContext, ToolchainOutput, WorkloadKind};
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

const SDK_WHEEL_ENV: &str = "ALIEN_PYTHON_SDK_WHEEL";
const DEFAULT_PYTHON_VERSION: &str = "3.12";
const UV_VERSION: &str = "0.8.11";

pub(crate) fn build_recipe_cache_key() -> &'static [u8] {
    b"python-toolchain-v1:python-slim-bookworm:uv-0.8.11"
}

#[derive(Debug, Clone)]
pub struct PythonToolchain {
    pub python_version: Option<String>,
    pub package: Option<String>,
    pub command: Vec<String>,
}

pub(crate) fn sdk_wheel_path() -> Result<Option<PathBuf>> {
    let Some(value) = std::env::var_os(SDK_WHEEL_ENV) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("whl") {
        return Err(AlienError::new(ErrorData::InvalidResourceConfig {
            resource_id: "python-toolchain".to_string(),
            reason: format!("{SDK_WHEEL_ENV} must point to a readable .whl file"),
        }));
    }
    Ok(Some(path))
}

impl PythonToolchain {
    fn version(&self) -> &str {
        self.python_version
            .as_deref()
            .unwrap_or(DEFAULT_PYTHON_VERSION)
    }

    fn dockerfile(&self, inject_sdk: bool) -> Result<String> {
        let command = serde_json::to_string(&self.command)
            .into_alien_error()
            .context(ErrorData::InvalidResourceConfig {
                resource_id: "python-toolchain".to_string(),
                reason: "Python command could not be encoded".to_string(),
            })?;
        let package = self
            .package
            .as_ref()
            .map(|name| format!(" --package {}", shell_escape(name)))
            .unwrap_or_default();
        let sdk_copy = if inject_sdk {
            "COPY --from=alien_python_sdk / /tmp/alien-python-sdk/\n"
        } else {
            ""
        };
        let sdk_install = if inject_sdk {
            "RUN uv pip install --python /app/.venv/bin/python --reinstall /tmp/alien-python-sdk/*.whl && rm -rf /tmp/alien-python-sdk\n"
        } else {
            ""
        };

        Ok(format!(
            "# syntax=docker/dockerfile:1\nFROM python:{version}-slim-bookworm\nRUN python -m pip install --no-cache-dir uv=={UV_VERSION}\nWORKDIR /app\nENV UV_LINK_MODE=copy UV_COMPILE_BYTECODE=1 VIRTUAL_ENV=/app/.venv PATH=/app/.venv/bin:$PATH\nCOPY . /app\nRUN uv sync --frozen --no-dev{package}\n{sdk_copy}{sdk_install}CMD {command}\n",
            version = self.version(),
        ))
    }
}

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[async_trait]
impl Toolchain for PythonToolchain {
    fn validate_source(&self, src_dir: &Path, resource_name: &str) -> Result<()> {
        if !src_dir.join("pyproject.toml").is_file() || !src_dir.join("uv.lock").is_file() {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: resource_name.to_string(),
                reason: "Python source requires pyproject.toml and uv.lock".to_string(),
            }));
        }
        if self.command.is_empty() || self.command.iter().any(String::is_empty) {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: resource_name.to_string(),
                reason: "Python toolchain command must contain non-empty argv values".to_string(),
            }));
        }
        if !matches!(self.version(), "3.10" | "3.11" | "3.12" | "3.13") {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: resource_name.to_string(),
                reason: "Python version must be one of 3.10, 3.11, 3.12, or 3.13".to_string(),
            }));
        }
        Ok(())
    }

    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
        if context.workload != WorkloadKind::Container {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: "python-toolchain".to_string(),
                reason: "Python source builds currently support Container workloads".to_string(),
            }));
        }

        fs::create_dir_all(&context.build_dir)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "python-toolchain".to_string(),
                reason: "Failed to create Python build directory".to_string(),
                build_output: None,
            })?;
        let wheel = sdk_wheel_path()?;
        let dockerfile_path = context.build_dir.join("Python.Dockerfile");
        fs::write(&dockerfile_path, self.dockerfile(wheel.is_some())?)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "python-toolchain".to_string(),
                reason: "Failed to write generated Python Dockerfile".to_string(),
                build_output: None,
            })?;

        let sdk_context_dir = context.build_dir.join("python-sdk");
        let mut contexts = Vec::new();
        if let Some(wheel) = wheel {
            if sdk_context_dir.exists() {
                fs::remove_dir_all(&sdk_context_dir)
                    .await
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        resource_name: "python-toolchain".to_string(),
                        reason: "Failed to clear stale Python SDK build context".to_string(),
                        build_output: None,
                    })?;
            }
            fs::create_dir_all(&sdk_context_dir)
                .await
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: "python-toolchain".to_string(),
                    reason: "Failed to create Python SDK build context".to_string(),
                    build_output: None,
                })?;
            let filename = wheel.file_name().ok_or_else(|| {
                AlienError::new(ErrorData::InvalidResourceConfig {
                    resource_id: "python-toolchain".to_string(),
                    reason: format!("{SDK_WHEEL_ENV} must include a wheel filename"),
                })
            })?;
            fs::copy(&wheel, sdk_context_dir.join(filename))
                .await
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: "python-toolchain".to_string(),
                    reason: "Failed to stage Python SDK wheel".to_string(),
                    build_output: None,
                })?;
            contexts.push(("alien_python_sdk", sdk_context_dir.as_path()));
        }

        let result = DockerToolchain {
            dockerfile: Some(dockerfile_path.to_string_lossy().into_owned()),
            build_args: None,
            target: None,
        }
        .build_with_contexts(context, &contexts)
        .await;

        if sdk_context_dir.exists() {
            fs::remove_dir_all(&sdk_context_dir)
                .await
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: "python-toolchain".to_string(),
                    reason: "Failed to remove staged Python SDK wheel".to_string(),
                    build_output: None,
                })?;
        }
        result
    }

    fn dev_command(&self, _src_dir: &Path) -> Vec<String> {
        let mut command = vec!["uv".to_string(), "run".to_string()];
        if let Some(package) = &self.package {
            command.extend(["--package".to_string(), package.clone()]);
        }
        command.extend(self.command.clone());
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_locked_project_and_explicit_command() {
        let dir = tempdir().expect("temp directory");
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname='example'\n",
        )
        .expect("write pyproject");
        std::fs::write(dir.path().join("uv.lock"), "version = 1\n").expect("write lock");
        let toolchain = PythonToolchain {
            python_version: None,
            package: None,
            command: vec![
                "python".to_string(),
                "-m".to_string(),
                "example".to_string(),
            ],
        };

        toolchain
            .validate_source(dir.path(), "example")
            .expect("locked project should be valid");
        let dockerfile = toolchain.dockerfile(true).expect("render Dockerfile");
        assert!(dockerfile.contains("uv sync --frozen --no-dev"));
        assert!(dockerfile.contains("COPY --from=alien_python_sdk / /tmp/alien-python-sdk/"));
        assert!(dockerfile.contains("CMD [\"python\",\"-m\",\"example\"]"));
    }

    #[test]
    fn rejects_unlocked_or_unsupported_projects() {
        let dir = tempdir().expect("temp directory");
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname='example'\n",
        )
        .expect("write pyproject");
        let mut toolchain = PythonToolchain {
            python_version: None,
            package: None,
            command: vec!["python".to_string(), "app.py".to_string()],
        };
        assert!(toolchain.validate_source(dir.path(), "example").is_err());

        std::fs::write(dir.path().join("uv.lock"), "version = 1\n").expect("write lock");
        toolchain.python_version = Some("3.9".to_string());
        assert!(toolchain.validate_source(dir.path(), "example").is_err());
    }
}
