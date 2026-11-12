//! The Rust SDK for Alien extensions.
//!
//! An extension's hooks crate implements the traits here (template modifiers, emitters,
//! importers, links and interceptors), registers them in a `register` function, and wraps that
//! function in [`export!`]:
//!
//! ```ignore
//! use alien_extension_sdk::prelude::*;
//!
//! fn register(ext: &mut Extension) {
//!     ext.cloudformation_modifier(postgres_iam_auth::CloudFormation);
//! }
//!
//! alien_extension_sdk::export!("acme", register);
//! ```
//!
//! The crate builds to one `wasm32-wasip1` module, which Alien loads and calls one hook at a
//! time. `export!` writes the functions the module exports, so the same hooks also run natively,
//! in the crate's own tests.

mod cloudformation;
mod descriptor;
mod helm;
mod interceptor;
mod resource;
mod terraform;

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod abi;

#[cfg(all(feature = "testing", not(target_arch = "wasm32")))]
pub mod testing;

#[cfg(all(feature = "controller", not(target_arch = "wasm32")))]
mod controllers;
#[cfg(all(feature = "controller", not(target_arch = "wasm32")))]
pub use controllers::{run_controllers, ControllerContextExt, Controllers};

pub use alien_macros::alien_resource;
pub use cloudformation::{CfResourceExt, CfTemplateExt, CloudFormationModifier};
pub use helm::{HelmChart, HelmModifier};
pub use interceptor::{
    AwsCall, AwsInterceptor, AwsRequest, AwsResponse, AzureCall, AzureInterceptor, AzureRequest,
    AzureResponse, GcpCall, GcpInterceptor, GcpRequest, GcpResponse, JsonResponse, KubernetesCall,
    KubernetesInterceptor, KubernetesObject,
};
pub use resource::{
    downcast, resource_config, AlienResource, ImportContextExt, Link, ResourceRegistration,
    ResourceTypeOf,
};
pub use terraform::{TerraformModifier, TfBlock, TfModule};

use alien_core::hooks::{HookEntry, HookKind, HooksManifest};
use serde_json::{Map, Value};

/// What `#[alien_resource]`'s expansion names, so a types crate needs only this crate.
#[doc(hidden)]
pub mod __private {
    pub use alien_core;
    pub use alien_error;
    pub use serde_json;
}

/// What a hooks crate imports.
pub mod prelude {
    pub use crate::{
        alien_resource, downcast, resource_config, AlienResource, AwsCall, AwsInterceptor,
        AwsRequest, AwsResponse, AzureCall, AzureInterceptor, AzureRequest, AzureResponse,
        CfResourceExt, CfTemplateExt, CloudFormationModifier, Extension, GcpCall, GcpInterceptor,
        GcpRequest, GcpResponse, HelmChart, HelmModifier, ImportContextExt, KubernetesCall,
        KubernetesInterceptor, KubernetesObject, Link, ResourceTypeOf, TerraformModifier, TfBlock,
        TfModule,
    };
    pub use alien_cloudformation::emitters::aws::helpers::{
        cf_from_json, required_logical_id, stack_name,
    };
    pub use alien_cloudformation::{CfEmitter, CfExpression, CfResource, CfTemplate};
    pub use alien_core::import::{EmitContext, ImportContext, ResourceImporter};
    pub use alien_core::{
        ErrorData, Platform, ResourceDefinition, ResourceOutputs, ResourceOutputsDefinition,
        ResourceRef, ResourceStatus, ResourceType, Result, StackResourceState,
    };
    pub use alien_error::{AlienError, Context, IntoAlienError};
    pub use alien_helm::{HelmEmitter, HelmFragment, InfrastructureValue};
    pub use alien_terraform::block::{attr, block, nested, resource_block};
    pub use alien_terraform::emitters::gcp::helpers::{required_label, resource_prefix_template};
    pub use alien_terraform::{expr, TfEmitter, TfFragment};
    pub use hcl::expr::Expression;
    pub use indexmap::indexmap;
    pub use serde::{Deserialize, Serialize};
    pub use serde_json::{json, Value};
}

/// What a controller crate imports. Controllers return `alien_infra`'s errors, as Alien's own
/// controllers do.
///
/// `#[controller]` expands to `crate::core`, `crate::error` and `crate::Result`, so the
/// controller crate's root imports this prelude with `use alien_extension_sdk::controller_prelude::*;`.
#[cfg(all(feature = "controller", not(target_arch = "wasm32")))]
pub mod controller_prelude {
    pub use crate::{ControllerContextExt, Controllers};
    pub use alien_core::{Platform, ResourceDefinition, ResourceOutputs, ResourceStatus};
    pub use alien_error::{AlienError, Context, IntoAlienError};
    pub use alien_infra::controller_test::SingleControllerExecutor;
    pub use alien_infra::{ErrorData, ResourceController, ResourceControllerContext, Result};
    pub use alien_macros::{controller, flow_entry, handler, terminal_state};
    pub use serde::{Deserialize, Serialize};
    pub use std::time::Duration;
    // `#[controller]`'s expansion names these crates, so a controller crate gets them here. Its
    // serde derives need `serde` among the crate's own dependencies.
    #[doc(hidden)]
    pub use {alien_core, alien_error, async_trait, serde_json};

    #[doc(hidden)]
    pub mod core {
        pub use alien_infra::{
            ResourceController, ResourceControllerContext, ResourceControllerStepResult,
        };
    }

    #[doc(hidden)]
    pub mod error {
        pub use alien_infra::{ErrorData, Result};
    }
}

/// Declares the extension: its name, which prefixes every resource type it adds, and the
/// function that registers its hooks.
///
/// Built for `wasm32-wasip1`, it writes the functions Alien calls in the module. On any target it
/// writes `pub fn extension() -> Extension`, which runs the same hooks natively.
#[macro_export]
macro_rules! export {
    ($name:literal, $register:path) => {
        /// The extension this crate declares, with every hook its `register` function adds.
        pub fn extension() -> $crate::Extension {
            $crate::Extension::new($name, $register)
        }

        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn alien_hooks_abi_v1() {}

            #[unsafe(no_mangle)]
            pub extern "C" fn alien_alloc(len: i32) -> i32 {
                $crate::abi::alloc(len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn alien_hooks_manifest() -> i64 {
                $crate::abi::manifest(extension)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn alien_hooks_call(
                name_ptr: i32,
                name_len: i32,
                input_ptr: i32,
                input_len: i32,
            ) -> i64 {
                // SAFETY: the host wrote both buffers into memory `alien_alloc` returned.
                unsafe { $crate::abi::call(extension, name_ptr, name_len, input_ptr, input_len) }
            }
        };
    };
}

/// A hook as the module runs it: its input as JSON, and its output as JSON or its error message.
type Hook = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, alien_core::hooks::HookError>>;

/// An extension: the hooks its `register` function adds, in the order it adds them.
pub struct Extension {
    name: &'static str,
    hooks: Vec<(HookEntry, Hook)>,
    permission_sets: Vec<(alien_core::ResourceType, String)>,
    declarations: indexmap::IndexMap<alien_core::ResourceType, resource::ResourceDeclarations>,
    /// Registrations no host could use, reported when the manifest is read.
    problems: Vec<String>,
}

impl Extension {
    /// Runs `register` for the extension `name`.
    pub fn new(name: &'static str, register: impl FnOnce(&mut Extension)) -> Self {
        let mut extension = Self {
            name,
            hooks: Vec::new(),
            permission_sets: Vec::new(),
            declarations: indexmap::IndexMap::new(),
            problems: Vec::new(),
        };
        if let Some(problem) = alien_core::extension_name_problem(name) {
            extension
                .problems
                .push(format!("the extension name `{name}` {problem}"));
        }
        register(&mut extension);
        extension
    }

    /// The name `export!` gave the extension.
    pub fn name(&self) -> &str {
        self.name
    }

    /// The hooks the extension declares and its resource types' descriptors, as the module's
    /// manifest lists them. Each descriptor's `controller_platforms` is empty: the controller
    /// binary lists those (`run_controllers`).
    pub fn manifest(&self) -> Result<HooksManifest, alien_core::hooks::HookError> {
        let mut problems = self.problems.clone();
        let resource_types = descriptor::descriptors(self).unwrap_or_else(|found| {
            problems.extend(found);
            Default::default()
        });
        if !problems.is_empty() {
            return Err(problems.join("; ").into());
        }
        Ok(HooksManifest {
            extension: self.name.to_string(),
            hooks: self.hooks.iter().map(|(entry, _)| entry.clone()).collect(),
            resource_types,
        })
    }

    /// Runs the hook named `hook` on `input`: what the module does for one call.
    pub fn call(&self, hook: &str, input: &[u8]) -> Result<Vec<u8>, alien_core::hooks::HookError> {
        let (_, run) = self
            .hooks
            .iter()
            .find(|(entry, _)| entry.name == hook)
            .ok_or_else(|| format!("the extension `{}` has no hook named `{hook}`", self.name))?;
        run(input)
    }

    /// The permission sets each resource type declares, as `register` passed them, in JSONC.
    pub fn permission_sets(&self) -> impl Iterator<Item = (&alien_core::ResourceType, &str)> {
        self.permission_sets
            .iter()
            .map(|(resource_type, set)| (resource_type, set.as_str()))
    }

    /// Writes what `register` declared for `resource_type` into its descriptor: the cloud
    /// services it requires and whether it requires the stack's network. A type that declared
    /// neither leaves the descriptor as it is.
    pub fn describe(
        &self,
        resource_type: &alien_core::ResourceType,
        descriptor: &mut alien_core::ResourceTypeDescriptor,
    ) {
        if let Some(declarations) = self.declarations.get(resource_type) {
            declarations.write(descriptor);
        }
    }

    /// Registers a [`CloudFormationModifier`].
    pub fn cloudformation_modifier(&mut self, modifier: impl CloudFormationModifier + 'static) {
        self.add_numbered(HookKind::CloudformationModifier, move |input| {
            cloudformation::run(&modifier, input)
        });
    }

    /// Registers a [`TerraformModifier`].
    pub fn terraform_modifier(&mut self, modifier: impl TerraformModifier + 'static) {
        let extension = self.name;
        self.add_numbered(HookKind::TerraformModifier, move |input| {
            terraform::run(extension, &modifier, input)
        });
    }

    /// Registers a [`HelmModifier`].
    pub fn helm_modifier(&mut self, modifier: impl HelmModifier + 'static) {
        self.add_numbered(HookKind::HelmModifier, move |input| {
            helm::run(&modifier, input)
        });
    }

    /// Registers an [`AwsInterceptor`].
    pub fn aws_interceptor(&mut self, interceptor: impl AwsInterceptor + 'static) {
        self.add_numbered(HookKind::AwsInterceptor, move |input| {
            interceptor::run_aws(&interceptor, input)
        });
    }

    /// Registers a [`GcpInterceptor`].
    pub fn gcp_interceptor(&mut self, interceptor: impl GcpInterceptor + 'static) {
        self.add_numbered(HookKind::GcpInterceptor, move |input| {
            interceptor::run_gcp(&interceptor, input)
        });
    }

    /// Registers an [`AzureInterceptor`].
    pub fn azure_interceptor(&mut self, interceptor: impl AzureInterceptor + 'static) {
        self.add_numbered(HookKind::AzureInterceptor, move |input| {
            interceptor::run_azure(&interceptor, input)
        });
    }

    /// Registers a [`KubernetesInterceptor`].
    pub fn kubernetes_interceptor(&mut self, interceptor: impl KubernetesInterceptor + 'static) {
        self.add_numbered(HookKind::KubernetesInterceptor, move |input| {
            interceptor::run_kubernetes(&interceptor, input)
        });
    }

    /// Registers the custom resource type `T`. The builder it returns adds its permission sets,
    /// emitters, importers and link.
    pub fn resource<T: AlienResource>(&mut self) -> ResourceRegistration<'_, T> {
        let prefix = format!("{}/", self.name);
        if !T::RESOURCE_TYPE.resource_type().0.starts_with(&prefix) {
            self.problems.push(format!(
                "the resource type `{}` must start with `{prefix}`, the extension's name",
                T::RESOURCE_TYPE
            ));
        }
        self.declarations
            .entry(T::RESOURCE_TYPE.resource_type())
            .or_insert_with(resource::ResourceDeclarations::new::<T>);
        ResourceRegistration::new(self)
    }

    /// Adds a hook named after its kind and its place among the extension's hooks.
    fn add_numbered(
        &mut self,
        kind: HookKind,
        run: impl Fn(&[u8]) -> Result<Vec<u8>, alien_core::hooks::HookError> + 'static,
    ) {
        let name = format!("{}/{}", kind_name(kind), self.hooks.len());
        self.add(name, kind, Map::new(), Box::new(run));
    }

    fn add(&mut self, name: String, kind: HookKind, routing: Map<String, Value>, run: Hook) {
        if self.hooks.iter().any(|(entry, _)| entry.name == name) {
            self.problems.push(format!("`{name}` is registered twice"));
            return;
        }
        self.hooks.push((
            HookEntry {
                name,
                kind,
                routing,
            },
            run,
        ));
    }
}

/// A kind's name as the manifest writes it, such as `cloudformationModifier`.
fn kind_name(kind: HookKind) -> String {
    match serde_json::to_value(kind) {
        Ok(Value::String(name)) => name,
        _ => unreachable!("a hook kind serializes as its name"),
    }
}

/// Reads a hook's input, naming the input in the error a malformed one gives.
fn read_input<T: serde::de::DeserializeOwned>(input: &[u8], what: &str) -> Result<T, String> {
    serde_json::from_slice(input)
        .map_err(|error| format!("the {what} Alien sent did not read: {error}"))
}

/// Writes a hook's output.
fn write_output(output: &impl serde::Serialize) -> Result<Vec<u8>, alien_core::hooks::HookError> {
    serde_json::to_vec(output)
        .map_err(|error| format!("the hook's output did not serialize: {error}").into())
}
