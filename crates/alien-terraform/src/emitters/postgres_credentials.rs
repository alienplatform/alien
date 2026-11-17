//! A persisted secret version is the authority across interrupted setup applies.
//! Reopening that exact version ephemerally avoids generating a different database
//! password when the secret write succeeded but the database operation failed.

use crate::{
    block::{attr, block, ephemeral_block},
    expr, TfFragment,
};
use hcl::{Block, Expression};

pub(super) fn password_revision(fragment: &mut TfFragment, label: &str) -> Expression {
    let name = format!("{label}_password_revision");
    fragment.variable_blocks.push(
        Block::builder("variable")
            .add_label(name.clone())
            .add_attribute(("type", expr::raw("number")))
            .add_attribute(("default", 1))
            .add_attribute(("nullable", false))
            .add_attribute(("description", "Increment to rotate this database password. Keep the revision unchanged when retrying an interrupted apply."))
            .add_block(block("validation", [
                attr("condition", expr::raw(format!("var.{name} >= 1 && var.{name} == floor(var.{name})"))),
                attr("error_message", "Password revision must be a positive integer.".into()),
            ]))
            .build(),
    );
    fragment.ephemeral_blocks.push(ephemeral_block(
        "random_password",
        label,
        [
            attr("length", 60.into()),
            attr("special", false.into()),
            // Azure requires three character categories; do not rely on chance.
            attr("min_lower", 1.into()),
            attr("min_upper", 1.into()),
            attr("min_numeric", 1.into()),
        ],
    ));
    expr::traversal(["var", &name])
}

/// Provider version counters are signed integers; use a stable, non-secret
/// 48-bit token so a recreated secret also updates the database on the next apply.
pub(super) fn database_revision(version: &str) -> Expression {
    expr::raw(format!("parseint(substr(sha256({version}), 0, 12), 16)"))
}

pub(super) fn generated_password(label: &str) -> Expression {
    expr::traversal(["ephemeral", "random_password", label, "result"])
}
