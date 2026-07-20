//! AWS EC2 Client
//!
//! This module provides a client for interacting with AWS EC2 APIs, focused on
//! VPC networking operations including VPCs, Subnets, Internet Gateways, NAT Gateways,
//! Route Tables, Security Groups, and Elastic IPs.
//!
//! # Example
//!
//! ```rust,ignore
//! use alien_aws_clients::ec2::{Ec2Client, Ec2Api, CreateVpcRequest};
//! use reqwest::Client;
//!
//! let ec2_client = Ec2Client::new(Client::new(), aws_config);
//! let vpc = ec2_client.create_vpc(
//!     CreateVpcRequest::builder()
//!         .cidr_block("10.0.0.0/16".to_string())
//!         .build()
//! ).await?;
//! ```

mod api;
mod client;
mod types;

pub use api::*;
pub use client::*;
pub use types::*;
