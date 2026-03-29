//! Alien Manager API
//!
//! Auto-generated from OpenAPI spec using Progenitor.
//! Provides a type-safe Rust client for the alien-manager API.
//!
//! ## Usage
//!
//! ```ignore
//! use alien_manager_api::Client;
//!
//! let client = Client::new("http://localhost:8080");
//!
//! // Create a deployment
//! let response = client
//!     .create_deployment()
//!     .body(&CreateDeploymentRequest {
//!         name: "my-deployment".into(),
//!         platform: Platform::Aws,
//!         ..Default::default()
//!     })
//!     .send()
//!     .await?;
//! ```

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));
