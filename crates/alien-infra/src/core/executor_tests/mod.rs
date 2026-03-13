//! Executor tests organized by functionality.
//!
//! These tests cover all aspects of the StackExecutor:
//! - Parallel execution of independent resources
//! - Dependency ordering and resolution
//! - Lifecycle filtering (Frozen, Live, LiveOnSetup)
//! - Resource deletion flows
//! - Resource updates and config changes
//! - Plan calculation (creates, updates, deletes)
//! - Stress tests for large dependency graphs
//! - Failure handling and retry scenarios
//! - Edge cases and special state handling

mod helpers;

// Core functionality tests
mod deletion_tests;
mod dependency_tests;
mod lifecycle_tests;
mod parallel_tests;
mod plan_tests;
mod update_tests;

// Advanced tests
mod edge_cases_tests;
mod failure_tests;
mod stress_tests;
