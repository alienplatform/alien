//! The controller binary's side of the step protocol: `run_controllers`, which `alien dev` runs
//! an extension's `Platform::Local` controllers with, and what a handler adds to Alien's
//! `ResourceControllerContext`.

use crate::{AlienResource, ResourceTypeOf};
use alien_core::{AwsCredentials, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::step_protocol::host::{step_client_config, ControllerHost};
use alien_infra::{
    DefaultPlatformServiceProvider, ErrorData, ResourceController, ResourceControllerContext,
    Result,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::future::Future;
use std::process::ExitCode;
use std::sync::Arc;

/// The controllers a binary runs, which `run_controllers` passes to its closure.
pub struct Controllers {
    host: ControllerHost,
    problems: Vec<String>,
}

impl Controllers {
    /// Runs `controller` for resources of `resource_type` on `platform`. A new resource starts
    /// from `controller` as it is.
    pub fn register<R, C>(
        &mut self,
        resource_type: ResourceTypeOf<R>,
        platform: Platform,
        controller: C,
    ) where
        R: AlienResource,
        C: ResourceController + DeserializeOwned + 'static,
    {
        if let Err(problem) =
            self.host
                .register::<R, C>(resource_type.resource_type(), platform, controller)
        {
            self.problems.push(problem);
        }
    }
}

/// Builds the controllers `register` adds and answers steps for them, as the arguments say:
///
/// - none: one message per line on stdin, one reply per line on stdout, as `alien dev` starts
///   it;
/// - `--controllers`: prints the type and platforms of each controller, as JSON, for
///   `alien extensions build`.
pub fn run_controllers(register: impl FnOnce(&mut Controllers)) -> ExitCode {
    let mut controllers = Controllers {
        host: ControllerHost::new(
            step_client_config,
            Arc::new(DefaultPlatformServiceProvider::default()),
            None,
        ),
        problems: Vec::new(),
    };
    register(&mut controllers);
    if !controllers.problems.is_empty() {
        eprintln!("{}", controllers.problems.join("\n"));
        return ExitCode::FAILURE;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match serve(controllers.host, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn serve(host: ControllerHost, args: &[String]) -> std::result::Result<(), String> {
    match args.first().map(String::as_str) {
        None => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("the runtime did not start: {e}"))?;
            host.serve_stdio(&runtime)
                .map_err(|e| format!("stdin or stdout failed: {e}"))
        }
        Some("--controllers") => {
            let mut listed: BTreeMap<&str, Vec<Platform>> = BTreeMap::new();
            for (resource_type, platform) in host.registrations() {
                listed.entry(resource_type).or_default().push(platform);
            }
            for platforms in listed.values_mut() {
                platforms.sort_by_key(|platform| platform.to_string());
            }
            println!("{}", json!(listed));
            Ok(())
        }
        Some(other) => Err(format!(
            "unknown argument `{other}`: run with no arguments for stdin and stdout, or `--controllers` to list the controllers"
        )),
    }
}

/// What a controller's handler reads besides Alien's own context.
pub trait ControllerContextExt {
    /// An AWS SDK config for any `aws-sdk-*` client, built from the step alone: its credentials
    /// and region, the latest behavior version, standard retries, an HTTPS client and a Tokio
    /// sleep. Nothing comes from the environment or an AWS profile, so `AWS_ENDPOINT_URL`, FIPS,
    /// dual-stack and retry settings there don't apply.
    ///
    /// The HTTPS client trusts the system's root certificates, read by rustls-native-certs, which
    /// also reads `SSL_CERT_FILE` and `SSL_CERT_DIR`. It ignores HTTP proxies, so a controller
    /// behind a proxy builds its own client.
    fn aws_sdk_config(&self) -> impl Future<Output = Result<aws_types::SdkConfig>> + Send;

    /// The outputs of `id`, a resource this one depends on, as `T`. A controller reads a
    /// dependency's outputs, never its saved state.
    fn dependency_outputs<T: DeserializeOwned>(&self, id: &str) -> Result<T>;
}

impl ControllerContextExt for ResourceControllerContext<'_> {
    fn aws_sdk_config(&self) -> impl Future<Output = Result<aws_types::SdkConfig>> + Send {
        let config = self.get_aws_config().cloned();
        async move { sdk_config_of(config?) }
    }

    fn dependency_outputs<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        let own = self.desired_config.id();
        let not_found = || {
            AlienError::new(ErrorData::DependencyNotFound {
                resource_id: own.to_string(),
                dependency_id: id.to_string(),
            })
        };
        if id == own {
            return Err(not_found());
        }
        let outputs = self
            .state
            .resources
            .get(id)
            .ok_or_else(not_found)?
            .outputs
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: own.to_string(),
                    dependency_id: id.to_string(),
                })
            })?;
        let mut value = serde_json::to_value(outputs).into_alien_error().context(
            ErrorData::ResourceStateSerializationFailed {
                resource_id: id.to_string(),
                message: "the dependency's outputs did not serialize".to_string(),
            },
        )?;
        if let Value::Object(fields) = &mut value {
            fields.shift_remove("type");
        }
        serde_json::from_value(value)
            .into_alien_error()
            .context(ErrorData::ResourceConfigInvalid {
                message: format!("the outputs of '{id}' did not read as the type asked for"),
                resource_id: Some(own.to_string()),
            })
    }
}

fn sdk_config_of(config: alien_core::AwsClientConfig) -> Result<aws_types::SdkConfig> {
    use aws_smithy_http_client::tls;

    let AwsCredentials::AccessKeys {
        access_key_id,
        secret_access_key,
        session_token,
    } = config.credentials
    else {
        return Err(AlienError::new(ErrorData::ClientConfigInvalid {
            platform: Platform::Aws,
            message: "the step's AWS credentials are not access keys".to_string(),
        }));
    };
    let credentials = aws_credential_types::Credentials::new(
        access_key_id,
        secret_access_key,
        session_token,
        None,
        "alien-step",
    );
    let http_client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .build_https();
    Ok(aws_types::SdkConfig::builder()
        .behavior_version(
            aws_smithy_runtime_api::client::behavior_version::BehaviorVersion::latest(),
        )
        .region(aws_types::region::Region::new(config.region))
        .credentials_provider(
            aws_credential_types::provider::SharedCredentialsProvider::new(credentials),
        )
        .retry_config(aws_types::sdk_config::RetryConfig::standard())
        .http_client(http_client)
        .sleep_impl(aws_smithy_async::rt::sleep::TokioSleep::new())
        .time_source(aws_smithy_async::time::SystemTimeSource::new())
        .build())
}
