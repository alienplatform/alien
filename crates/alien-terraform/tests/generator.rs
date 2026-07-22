//! End-to-end Terraform generator tests, organized by category.
//!
//! Mirrors the `executor_tests/` pattern from `alien-infra/src/core/`:
//! one module per concern, a shared `helpers` module for fixtures + the
//! multi-file snapshot helper. Each scenario produces ONE `.snap` file
//! containing every rendered file separated by `=== <path> ===` markers
//! so PR review can diff a complete module in one place.

mod generator {
    pub mod helpers;

    pub mod aws_compute_tests;
    pub mod aws_data_layer_tests;
    pub mod aws_full_stack_tests;
    pub mod aws_identity_tests;
    pub mod azure_compute_tests;
    pub mod azure_data_layer_tests;
    pub mod azure_full_stack_tests;
    pub mod azure_identity_tests;
    pub mod enabled_queue_tests;
    pub mod enabled_storage_tests;
    pub mod enabled_tests;
    pub mod gcp_compute_tests;
    pub mod gcp_data_layer_tests;
    pub mod gcp_full_stack_tests;
    pub mod gcp_identity_tests;
    pub mod k8s_overlay_tests;
    pub mod stack_input_tests;
}
