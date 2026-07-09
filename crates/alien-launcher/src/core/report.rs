//! Failure reporting: maps launcher-local update outcomes onto the sync-wire
//! `OperatorUpdateReport` shape via on-disk failure records.
//!
//! The launcher has no network path to the manager, so the failure record in
//! the version store is the handoff — the operator translates the newest
//! record into its `SyncRequest.operator_update` on every sync.
//!
//! Implementation lands with the failure-records work.
