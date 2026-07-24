//! AWS Elastic Load Balancing v2 (ELBv2) Client
//!
//! This module provides a client for interacting with AWS ELBv2 APIs, including
//! Application Load Balancers, Target Groups, and Listeners.
//!
//! # Example
//!
//! ```rust,ignore
//! use alien_aws_clients::elbv2::{Elbv2Client, Elbv2Api, CreateLoadBalancerRequest};
//! use reqwest::Client;
//!
//! let elb_client = Elbv2Client::new(Client::new(), aws_config);
//! elb_client.create_load_balancer(
//!     CreateLoadBalancerRequest::builder()
//!         .name("my-alb".to_string())
//!         .subnets(vec!["subnet-12345".to_string()])
//!         .build()
//! ).await?;
//! ```

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use async_trait::async_trait;
use bon::Builder;
use form_urlencoded;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// ELBv2 Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Elbv2ErrorResponse {
    pub error: Elbv2ErrorWrapper,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Elbv2ErrorWrapper {
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
}

mod api_impl;
mod types;

pub use api_impl::{Elbv2Api, Elbv2Client};
pub use types::*;

#[cfg(test)]
mod wire_tests;
