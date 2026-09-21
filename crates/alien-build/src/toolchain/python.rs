use super::docker::DockerToolchain;
use super::{Toolchain, ToolchainContext, ToolchainOutput, WorkloadKind};
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::warn;

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

#[derive(Debug)]
struct PythonBuildInputs {
    root: PathBuf,
    dockerfile: PathBuf,
    sdk_context: Option<PathBuf>,
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

    async fn prepare_build_inputs(
        &self,
        context: &ToolchainContext,
        wheel: Option<&Path>,
    ) -> Result<PythonBuildInputs> {
        let root = DockerToolchain::absolute_path(&context.build_dir.join(format!(
            ".python-{}",
            context.build_target.runtime_platform_id()
        )))?;
        if root.exists() {
            fs::remove_dir_all(&root).await.into_alien_error().context(
                ErrorData::ImageBuildFailed {
                    resource_name: "python-toolchain".to_string(),
                    reason: "Failed to clear stale Python build inputs".to_string(),
                    build_output: None,
                },
            )?;
        }
        fs::create_dir_all(&root).await.into_alien_error().context(
            ErrorData::ImageBuildFailed {
                resource_name: "python-toolchain".to_string(),
                reason: "Failed to create Python build input directory".to_string(),
                build_output: None,
            },
        )?;

        let dockerfile = root.join("Dockerfile");
        fs::write(&dockerfile, self.dockerfile(wheel.is_some())?)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "python-toolchain".to_string(),
                reason: "Failed to write generated Python Dockerfile".to_string(),
                build_output: None,
            })?;

        let sdk_context = if let Some(wheel) = wheel {
            let directory = root.join("sdk");
            fs::create_dir_all(&directory)
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
            fs::copy(wheel, directory.join(filename))
                .await
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: "python-toolchain".to_string(),
                    reason: "Failed to stage Python SDK wheel".to_string(),
                    build_output: None,
                })?;
            Some(directory)
        } else {
            None
        };

        Ok(PythonBuildInputs {
            root,
            dockerfile,
            sdk_context,
        })
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

        let wheel = sdk_wheel_path()?;
        let inputs = self.prepare_build_inputs(context, wheel.as_deref()).await?;
        let contexts = inputs
            .sdk_context
            .as_deref()
            .map(|path| vec![("alien_python_sdk", path)])
            .unwrap_or_default();

        let result = DockerToolchain {
            dockerfile: Some(inputs.dockerfile.to_string_lossy().into_owned()),
            build_args: None,
            target: None,
        }
        .build_with_contexts(context, &contexts)
        .await;

        if let Err(error) = fs::remove_dir_all(&inputs.root).await {
            if result.is_ok() {
                return Err(error
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        resource_name: "python-toolchain".to_string(),
                        reason: "Failed to remove Python build inputs".to_string(),
                        build_output: None,
                    }));
            }
            warn!(
                path = %inputs.root.display(),
                %error,
                "Failed to clean up Python build inputs after build failure"
            );
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
    use alien_core::BinaryTarget;
    use tempfile::tempdir;

    fn context(build_dir: PathBuf, target: BinaryTarget) -> ToolchainContext {
        ToolchainContext {
            src_dir: PathBuf::from("nested/source"),
            build_dir,
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: target,
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            workload: WorkloadKind::Container,
        }
    }

    fn toolchain() -> PythonToolchain {
        PythonToolchain {
            python_version: None,
            package: None,
            command: vec!["python".to_string(), "app.py".to_string()],
        }
    }

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

    #[tokio::test]
    async fn generated_inputs_are_absolute_when_build_directory_is_relative() {
        let build_root = tempfile::Builder::new()
            .prefix("python-relative-build-")
            .tempdir_in(".")
            .expect("relative temp directory");
        let relative_build_dir = build_root
            .path()
            .strip_prefix(std::env::current_dir().expect("current directory"))
            .unwrap_or(build_root.path())
            .to_path_buf();
        let inputs = toolchain()
            .prepare_build_inputs(&context(relative_build_dir, BinaryTarget::LinuxX64), None)
            .await
            .expect("build inputs should be prepared");

        assert!(inputs.root.is_absolute());
        assert!(inputs.dockerfile.is_absolute());
        assert!(inputs.dockerfile.is_file());
    }

    #[tokio::test]
    async fn parallel_targets_keep_their_staged_sdk_inputs_isolated() {
        let build_root = tempdir().expect("build root");
        let wheel_root = tempdir().expect("wheel root");
        let wheel = wheel_root.path().join("alienplatform-test.whl");
        fs::write(&wheel, b"wheel-bytes")
            .await
            .expect("write wheel fixture");
        let x64 = context(build_root.path().to_path_buf(), BinaryTarget::LinuxX64);
        let arm64 = context(build_root.path().to_path_buf(), BinaryTarget::LinuxArm64);
        let toolchain = toolchain();

        let (x64_inputs, arm64_inputs) = tokio::join!(
            toolchain.prepare_build_inputs(&x64, Some(&wheel)),
            toolchain.prepare_build_inputs(&arm64, Some(&wheel)),
        );
        let x64_inputs = x64_inputs.expect("x64 inputs");
        let arm64_inputs = arm64_inputs.expect("arm64 inputs");
        assert_ne!(x64_inputs.root, arm64_inputs.root);

        fs::remove_dir_all(&x64_inputs.root)
            .await
            .expect("remove x64 inputs");
        let arm64_wheel = arm64_inputs
            .sdk_context
            .as_ref()
            .expect("arm64 SDK context")
            .join("alienplatform-test.whl");
        assert_eq!(
            fs::read(arm64_wheel).await.expect("read arm64 wheel"),
            b"wheel-bytes"
        );
        assert!(arm64_inputs.dockerfile.is_file());
    }
}
