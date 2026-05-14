//! Per-resource CloudFormation emitters, organized by cloud.
//!
//! One sub-module per cloud (currently AWS only — CloudFormation has no
//! GCP / Azure analogue). Each cloud module has one file per resource so
//! reviewers find "what does the `data` storage actually become" by
//! opening `emitters/aws/storage.rs` instead of grepping a 2k-line file.
//!
//! Plugins layer on additional `(resource_type, platform)` emitters by
//! constructing a [`crate::CfRegistry`] and calling `register(...)` on
//! top of [`crate::CfRegistry::built_in`].

pub mod aws;
