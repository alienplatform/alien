//! Sequence floors: the highest sequence an enforcer accepted, per key.
//!
//! The key is the statement kind, or the kind and a subject where statements
//! of one kind are independent of each other, such as one dynamic container
//! among many. Enforcers persist floors next to their own state.

use std::collections::BTreeMap;

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Floors by key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Floors(BTreeMap<String, u64>);

impl Floors {
    /// The floor for `key`, if one was ever recorded.
    pub fn get(&self, key: &str) -> Option<u64> {
        self.0.get(key).copied()
    }

    /// Refuse `sequence` unless it is above the floor for `key`.
    pub fn check(&self, key: &str, sequence: u64) -> Result<()> {
        match self.get(key) {
            Some(floor) if sequence <= floor => Err(AlienError::new(ErrorData::SequenceReplayed {
                kind: key.to_string(),
                sequence,
                floor,
            })),
            _ => Ok(()),
        }
    }

    /// Check `sequence` like [`Floors::check`], but let the statement that set
    /// the floor through again. An enforcer re-reads its current statement on
    /// every pass; only an older one is a replay.
    pub fn check_current(&self, key: &str, sequence: u64) -> Result<()> {
        match self.get(key) {
            Some(floor) if sequence < floor => Err(AlienError::new(ErrorData::SequenceReplayed {
                kind: key.to_string(),
                sequence,
                floor,
            })),
            _ => Ok(()),
        }
    }

    /// Raise the floor for `key` to `sequence`. Never lowers it.
    pub fn raise(&mut self, key: &str, sequence: u64) {
        let floor = self.0.entry(key.to_string()).or_insert(sequence);
        *floor = (*floor).max(sequence);
    }
}
