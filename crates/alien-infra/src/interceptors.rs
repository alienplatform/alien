//! A deployment's interceptors: the interceptor hooks of the extensions its stack lists, run in
//! each extension's hooks module on the calls Alien's controllers make.
//!
//! A client gets only the hooks for its own cloud, so a module is never asked about another
//! cloud's calls. Within that cloud, the module's own `matches` decides which calls it sees, and
//! a call it does not match reaches the cloud as Alien built it.

use crate::error::{ErrorData, Result};
use crate::LoadedExtensions;
use alien_client_core::interceptor::{
    CallInterceptor, Cloud, HookRequest, HookResponse, InterceptorChain,
};
use alien_core::hooks::{
    HookKind, InterceptedCloud, InterceptedRequest, InterceptedResponse, InterceptorInput,
    InterceptorOutput,
};
use alien_core::Stack;
use alien_error::AlienError;
use alien_hooks_runtime::{HookModule, Limits};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use std::sync::Arc;

/// The interceptors of one release of a deployment, in the order its stack lists extensions,
/// and within an extension in the order it registered them.
#[derive(Clone, Default)]
pub struct DeploymentInterceptors {
    hooks: Arc<[InterceptorHook]>,
}

/// One interceptor hook of one extension's module.
#[derive(Clone)]
struct InterceptorHook {
    extension: String,
    module: HookModule,
    hook: String,
    cloud: Cloud,
}

impl std::fmt::Debug for DeploymentInterceptors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.hooks
                    .iter()
                    .map(|hook| format!("{}/{} ({})", hook.extension, hook.hook, hook.cloud)),
            )
            .finish()
    }
}

impl DeploymentInterceptors {
    /// The interceptor hooks of `stack`'s extensions, from the modules in `extensions`. Fails
    /// when `extensions` lacks the hooks module of an extension the stack lists.
    pub fn for_stack(stack: &Stack, extensions: &LoadedExtensions) -> Result<Self> {
        extensions.check_loaded(stack)?;
        let mut hooks = Vec::new();
        let loaded = stack.extensions.iter().filter_map(|manifest| {
            extensions
                .hooks(&manifest.name)
                .map(|module| (manifest, module))
        });
        for (manifest, module) in loaded {
            if module.manifest.extension != manifest.name {
                return Err(not_loaded(
                    stack,
                    vec![manifest.name.clone()],
                    format!(
                        "the hooks module loaded for '{}' is extension '{}'",
                        manifest.name, module.manifest.extension
                    ),
                ));
            }
            for entry in &module.manifest.hooks {
                let Some(cloud) = intercepted_cloud(entry.kind) else {
                    continue;
                };
                hooks.push(InterceptorHook {
                    extension: manifest.name.clone(),
                    module: module.clone(),
                    hook: entry.name.clone(),
                    cloud,
                });
            }
        }
        Ok(Self {
            hooks: hooks.into(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// The chain for a client of `cloud`. `tenant_id` is the Azure tenant an Azure client's
    /// calls are made in, which Azure interceptors read.
    pub fn chain(&self, cloud: Cloud, tenant_id: Option<&str>) -> InterceptorChain {
        InterceptorChain::new(
            self.hooks
                .iter()
                .filter(|hook| hook.cloud == cloud)
                .map(|hook| {
                    Arc::new(ModuleInterceptor {
                        hook: hook.clone(),
                        tenant_id: tenant_id.map(str::to_string),
                    }) as Arc<dyn CallInterceptor>
                })
                .collect(),
        )
    }
}

/// `stack` refused because this process has not loaded `missing`, the named extensions.
pub(crate) fn not_loaded(
    stack: &Stack,
    missing: Vec<String>,
    message: String,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ExtensionsNotLoaded {
        stack_id: stack.id.clone(),
        extensions: missing,
        message,
    })
}

fn intercepted_cloud(kind: HookKind) -> Option<Cloud> {
    match kind {
        HookKind::AwsInterceptor => Some(Cloud::Aws),
        HookKind::GcpInterceptor => Some(Cloud::Gcp),
        HookKind::AzureInterceptor => Some(Cloud::Azure),
        HookKind::KubernetesInterceptor => Some(Cloud::Kubernetes),
        _ => None,
    }
}

/// One interceptor hook, as one client's chain runs it.
struct ModuleInterceptor {
    hook: InterceptorHook,
    tenant_id: Option<String>,
}

type ClientResult<T> = alien_client_core::Result<T>;

impl ModuleInterceptor {
    /// A failure of this interceptor. Its message never quotes a request or a response: they
    /// carry the customer's data.
    fn failed(&self, message: impl Into<String>) -> AlienError<alien_client_core::ErrorData> {
        AlienError::new(alien_client_core::ErrorData::InterceptorFailed {
            extension: self.hook.extension.clone(),
            message: message.into(),
        })
    }

    /// Runs the hook on `input`, which takes up to the hook's deadline.
    fn ask(&self, input: &InterceptorInput) -> ClientResult<InterceptorOutput> {
        let input = serde_json::to_vec(input)
            .map_err(|_| self.failed("the intercepted call could not be encoded"))?;
        let limits = Limits::for_kind(intercepted_kind(self.hook.cloud));
        let call = || self.hook.module.call(&self.hook.hook, &input, &limits);
        // The chain runs inside a client's async send and can't await. On a multi-thread runtime,
        // as in the manager, `block_in_place` hands this worker's other tasks to another thread
        // while the hook runs. A current-thread runtime has no other worker, and
        // `block_in_place` panics there, so the hook runs in place.
        let multi_thread = tokio::runtime::Handle::try_current().is_ok_and(|runtime| {
            runtime.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        });
        let output = if multi_thread {
            tokio::task::block_in_place(call)
        } else {
            call()
        }
        // The hook's own error text stays out: the hook saw the request and the response, and
        // its error is saved with the deployment and shown to the extension's developer.
        .map_err(|error| {
            // A hook stopped at its deadline may finish on another try; one that failed won't,
            // unless it said so. `ResponseHook::apply` makes a `modify_response` failure final
            // whatever this says, since the call was already sent.
            let mut failed = self.failed(format!(
                "its hook '{}' failed ({})",
                self.hook.hook, error.code
            ));
            failed.retryable = error.retryable
                || matches!(
                    error.error,
                    Some(alien_hooks_runtime::ErrorData::HookDeadlineExceeded { .. })
                );
            failed
        })?;
        serde_json::from_slice(&output).map_err(|_| {
            self.failed(format!(
                "its hook '{}' answered with something other than an interceptor's output",
                self.hook.hook
            ))
        })
    }

    fn wire_request(&self, request: &HookRequest) -> ClientResult<InterceptedRequest> {
        Ok(InterceptedRequest {
            cloud: match request.cloud {
                Cloud::Aws => InterceptedCloud::Aws,
                Cloud::Gcp => InterceptedCloud::Gcp,
                Cloud::Azure => InterceptedCloud::Azure,
                Cloud::Kubernetes => InterceptedCloud::Kubernetes,
            },
            service: request.service.clone(),
            method: request.method.to_string(),
            url: request.url.to_string(),
            headers: self.wire_headers(&request.headers)?,
            body: request.body.clone(),
            tenant_id: match request.cloud {
                Cloud::Azure => self.tenant_id.clone(),
                _ => None,
            },
        })
    }

    fn wire_headers(&self, headers: &HeaderMap) -> ClientResult<Vec<(String, String)>> {
        headers
            .iter()
            .map(|(name, value)| {
                let value = value.to_str().map_err(|_| {
                    self.failed(format!("the value of header '{name}' is not text"))
                })?;
                Ok((name.to_string(), value.to_string()))
            })
            .collect()
    }

    fn header_map(&self, headers: &[(String, String)]) -> ClientResult<HeaderMap> {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            let name = HeaderName::try_from(name.as_str())
                .map_err(|_| self.failed("it set a header with an invalid name"))?;
            let value = HeaderValue::try_from(value.as_str())
                .map_err(|_| self.failed(format!("it set header '{name}' to an invalid value")))?;
            map.append(name, value);
        }
        Ok(map)
    }
}

fn intercepted_kind(cloud: Cloud) -> HookKind {
    match cloud {
        Cloud::Aws => HookKind::AwsInterceptor,
        Cloud::Gcp => HookKind::GcpInterceptor,
        Cloud::Azure => HookKind::AzureInterceptor,
        Cloud::Kubernetes => HookKind::KubernetesInterceptor,
    }
}

impl CallInterceptor for ModuleInterceptor {
    fn extension(&self) -> &str {
        &self.hook.extension
    }

    fn matches(&self, request: &HookRequest) -> ClientResult<bool> {
        match self.ask(&InterceptorInput::Matches {
            request: self.wire_request(request)?,
        })? {
            InterceptorOutput::Matches(matched) => Ok(matched),
            _ => Err(self.failed("it answered `matches` with something other than a yes or no")),
        }
    }

    fn modify_request(&self, request: &mut HookRequest) -> ClientResult<()> {
        let InterceptorOutput::Request(changed) = self.ask(&InterceptorInput::ModifyRequest {
            request: self.wire_request(request)?,
        })?
        else {
            return Err(
                self.failed("it answered `modify_request` with something other than a request")
            );
        };
        request.cloud = match changed.cloud {
            InterceptedCloud::Aws => Cloud::Aws,
            InterceptedCloud::Gcp => Cloud::Gcp,
            InterceptedCloud::Azure => Cloud::Azure,
            InterceptedCloud::Kubernetes => Cloud::Kubernetes,
        };
        request.service = changed.service;
        request.method = Method::from_bytes(changed.method.as_bytes())
            .map_err(|_| self.failed("it set an invalid HTTP method"))?;
        request.url = Url::parse(&changed.url).map_err(|_| self.failed("it set an invalid URL"))?;
        request.headers = self.header_map(&changed.headers)?;
        request.body = changed.body;
        Ok(())
    }

    fn modify_response(
        &self,
        request: &HookRequest,
        response: &mut HookResponse,
    ) -> ClientResult<()> {
        let InterceptorOutput::Response(changed) = self.ask(&InterceptorInput::ModifyResponse {
            request: self.wire_request(request)?,
            response: InterceptedResponse {
                status: response.status,
                headers: self.wire_headers(&response.headers)?,
                body: response.body.clone(),
            },
        })?
        else {
            return Err(
                self.failed("it answered `modify_response` with something other than a response")
            );
        };
        response.status = changed.status;
        response.headers = self.header_map(&changed.headers)?;
        response.body = changed.body;
        Ok(())
    }
}
