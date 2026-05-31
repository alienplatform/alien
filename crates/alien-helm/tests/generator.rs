//! End-to-end Helm generator tests, organized by category.
//!
//! Each scenario produces a multi-file snapshot covering every rendered
//! file separated by `=== <path> ===` markers, runs `helm lint`, and
//! `helm template` + `kubeconform` against both the manager-fetch path
//! (default values.yaml) and the external-bindings initialize path
//! (examples/onprem.yaml).

mod generator {
    #[allow(dead_code)]
    #[path = "../../src/test_utils.rs"]
    pub mod test_utils;

    pub mod helpers;

    pub mod boot_paths_tests;
    pub mod manager_only_tests;
    pub mod plugin_tests;
    pub mod resource_layer_tests;
}
