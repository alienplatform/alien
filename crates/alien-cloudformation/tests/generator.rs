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

    pub mod aws_ai_tests;
    pub mod aws_compute_tests;
    pub mod aws_data_layer_tests;
    pub mod aws_email_tests;
    pub mod aws_full_stack_tests;
    pub mod aws_open_search_tests;
    pub mod aws_sandbox_gateway_tests;
    pub mod aws_sandbox_tests;
    pub mod boundary_tests;
    pub mod enabled_queue_tests;
    pub mod enabled_storage_tests;
    pub mod enabled_tests;
    pub mod gating_matrix_tests;
    pub mod kubernetes_cluster_tests;
    pub mod management_external_id_tests;
    pub mod management_policy_tests;
    pub mod management_trust_tests;
    pub mod network_inputs_tests;
    pub mod network_tests;
    pub mod output_chunking_tests;
    pub mod parameter_update_tests;
    pub mod plugin_tests;
    pub mod registration_delete_order_tests;
    pub mod registration_tests;
    pub mod resource_metadata_tests;
    pub mod stack_input_tests;
    pub mod volume_key_dependency_tests;
}
