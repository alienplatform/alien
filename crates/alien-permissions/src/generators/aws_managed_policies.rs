//! Packs IAM statements into customer managed policies for one role.
//!
//! AWS limits a managed policy to 6,144 characters, not counting whitespace, and
//! limits how many managed policies one role can have attached (20 by default).
//! Every path that splits a management grant across managed policies uses
//! [`pack_managed_policy_statements`], so they all fit the same number of
//! policies into a role.

use alien_error::{AlienErrorData, Result};
use serde::Serialize;

use super::aws_runtime::AwsIamStatement;

/// Largest policy document, in bytes of compact JSON, that a packed policy may
/// reach. AWS allows 6,144 characters; the rest is headroom for values that
/// grow when they're resolved, such as template placeholders.
pub const MAX_MANAGED_POLICY_BYTES: usize = 5_500;

/// Most numbered management policies Alien attaches to one role. AWS's default
/// quota is 20 managed policies per role; this keeps 2 for policies the
/// account owner attaches.
pub const MAX_ATTACHED_MANAGEMENT_POLICIES: usize = 18;

/// Merges statements that differ only in their actions (see
/// [`merge_statement_actions`]), then packs them into as few policies as fit,
/// first-fit decreasing: largest statement first (ties keep their order), each
/// into the first policy it fits. A greedy split in the original order leaves
/// room unused in most policies, which can push a role over the per-role quota
/// of attached policies. Each policy lists its statements in their original
/// order, and the same statements always give the same policies.
///
/// `policy_size` measures a policy document holding the given statements. A
/// statement that is larger than `max_policy_bytes` on its own gets a policy of
/// its own; callers check each policy's size and reject it.
pub fn pack_managed_policy_statements<E>(
    statements: Vec<AwsIamStatement>,
    max_policy_bytes: usize,
    policy_size: impl Fn(&[AwsIamStatement]) -> Result<usize, E>,
) -> Result<Vec<Vec<AwsIamStatement>>, E>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    let statements = merge_statement_actions(statements, max_policy_bytes, &policy_size)?;
    let mut sized = Vec::with_capacity(statements.len());
    for (index, statement) in statements.into_iter().enumerate() {
        let size = policy_size(std::slice::from_ref(&statement))?;
        sized.push((size, index, statement));
    }
    sized.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut chunks: Vec<Vec<(usize, AwsIamStatement)>> = Vec::new();
    for (size, index, statement) in sized {
        let mut placed = false;
        if size <= max_policy_bytes {
            for chunk in chunks.iter_mut() {
                let mut candidate: Vec<AwsIamStatement> = chunk
                    .iter()
                    .map(|(_, statement)| statement.clone())
                    .collect();
                candidate.push(statement.clone());
                if policy_size(&candidate)? <= max_policy_bytes {
                    chunk.push((index, statement.clone()));
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            chunks.push(vec![(index, statement)]);
        }
    }
    Ok(chunks
        .into_iter()
        .map(|mut chunk| {
            chunk.sort_by_key(|(index, _)| *index);
            chunk.into_iter().map(|(_, statement)| statement).collect()
        })
        .collect())
}

/// Merges statements that differ only in their actions: same effect, same
/// resources and same conditions. A statement allows or denies each of its
/// actions on each resource under its conditions independently, so one
/// statement with the union of the actions means exactly what the separate
/// statements meant, and the shared resources and conditions are written once.
/// The merged statement keeps the first statement's Sid and position, and is
/// never larger than a policy holds. Statements with `NotResource` stay as they
/// are.
pub fn merge_statement_actions<E>(
    statements: Vec<AwsIamStatement>,
    max_policy_bytes: usize,
    policy_size: impl Fn(&[AwsIamStatement]) -> Result<usize, E>,
) -> Result<Vec<AwsIamStatement>, E>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    let mut merged: Vec<AwsIamStatement> = Vec::with_capacity(statements.len());
    for statement in statements {
        let mut placed = false;
        if statement.not_resource.is_empty() {
            for existing in merged.iter_mut().filter(|existing| {
                existing.not_resource.is_empty()
                    && existing.effect == statement.effect
                    && existing.resource == statement.resource
                    && existing.condition == statement.condition
            }) {
                let mut candidate = existing.clone();
                for action in &statement.action {
                    if !candidate.action.contains(action) {
                        candidate.action.push(action.clone());
                    }
                }
                // A merge never makes a statement too large for a policy of its own.
                if policy_size(std::slice::from_ref(&candidate))? <= max_policy_bytes {
                    *existing = candidate;
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            merged.push(statement);
        }
    }
    Ok(merged)
}
