//! Background loops for the alien-operator

pub mod access_requests;
pub mod airgap;
pub mod commands;
pub mod debug_session;
pub mod deployment;
pub mod dynamic_containers;
pub mod kubernetes_heartbeats;
mod observed_release;
pub mod operations_exec;
pub mod otlp;
pub mod sync;
pub mod tunnel;
