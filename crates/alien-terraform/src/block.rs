//! Tiny `hcl-rs` block-construction helpers used by emitters.
//!
//! Wraps `hcl::Block` / `hcl::Body` / `hcl::structure::Structure` in
//! function calls that read like the rendered HCL, so emitters look like
//! the resource declarations they produce instead of like AST builders.

use hcl::{
    expr::Expression,
    structure::{Attribute, Block, BlockLabel, Body, Structure},
    Identifier,
};

/// Build a `resource "type" "label" { ... }` block from an iterator of
/// `Structure`s (use [`attr`] / [`nested`] to construct them).
pub fn resource_block(
    provider_type: &str,
    label: &str,
    body: impl IntoIterator<Item = Structure>,
) -> Block {
    Block {
        identifier: Identifier::sanitized("resource"),
        labels: vec![
            BlockLabel::String(provider_type.to_string()),
            BlockLabel::String(label.to_string()),
        ],
        body: Body::from(body.into_iter().collect::<Vec<_>>()),
    }
}

/// Build a `data "type" "label" { ... }` block.
pub fn data_block(
    provider_type: &str,
    label: &str,
    body: impl IntoIterator<Item = Structure>,
) -> Block {
    Block {
        identifier: Identifier::sanitized("data"),
        labels: vec![
            BlockLabel::String(provider_type.to_string()),
            BlockLabel::String(label.to_string()),
        ],
        body: Body::from(body.into_iter().collect::<Vec<_>>()),
    }
}

/// Build a nested unlabeled block (e.g. `rule { ... }` inside a resource).
pub fn block(name: &str, body: impl IntoIterator<Item = Structure>) -> Block {
    Block {
        identifier: Identifier::sanitized(name),
        labels: vec![],
        body: Body::from(body.into_iter().collect::<Vec<_>>()),
    }
}

/// `name = expression` attribute.
pub fn attr(name: &str, value: Expression) -> Structure {
    Structure::Attribute(Attribute::new(Identifier::sanitized(name), value))
}

/// Wrap a nested `Block` as a `Structure` so it can be appended to a body.
pub fn nested(block: Block) -> Structure {
    Structure::Block(block)
}
