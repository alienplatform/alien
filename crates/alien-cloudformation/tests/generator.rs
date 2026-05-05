//! End-to-end CloudFormation generator tests, organized by category.
//!
//! Mirrors the `executor_tests/` pattern from `alien-infra/src/core/`:
//! one module per concern, a shared `helpers` module for fixtures plus
//! the `cfn-lint` wrapper. Each test renders a complete template (via
//! `CfRegistry::built_in()` or a sample emitter), runs `cfn-lint`, then
//! asserts against a YAML snapshot. Reviewers diff a complete template
//! per scenario, the way a security team would actually read it.

mod generator {
    pub mod helpers;

    pub mod aws_compute_tests;
    pub mod aws_data_layer_tests;
    pub mod aws_full_stack_tests;
    pub mod network_tests;
    pub mod output_chunking_tests;
    pub mod plugin_tests;
    pub mod registration_tests;
}
