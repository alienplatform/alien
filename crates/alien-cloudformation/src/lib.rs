//! CloudFormation template generation for Alien stacks.
//!
//! Owns the entire CloudFormation surface: the template IR ([`CfTemplate`] /
//! [`CfResource`] / [`CfExpression`]), the per-resource [`CfEmitter`] trait,
//! the [`CfRegistry`] that dispatches `(ResourceType, Platform)` pairs to
//! emitters, and the top-level [`generate_cloudformation_template`] orchestration.
//!
//! `alien-core` deliberately stays format-agnostic — only typed `ImportData`
//! payloads and wire types live there. Plugins extend the CFN surface by
//! constructing a `CfRegistry` and calling `register(resource_type, platform, emitter)`
//! on top of `CfRegistry::built_in()`.

mod built_ins;
mod emitter;
pub mod emitters;
mod generator;
mod registry;
mod template;
#[doc(hidden)]
pub mod test_utils;

pub use emitter::CfEmitter;
pub use generator::{
    generate_cloudformation_stack_policy, generate_cloudformation_template, to_yaml,
    CloudFormationOptions, RegistrationMode,
};
pub use registry::CfRegistry;
pub use template::{CfExpression, CfOutput, CfParameter, CfResource, CfTemplate};
