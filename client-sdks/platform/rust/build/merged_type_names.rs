//! Keeps the type names of the complete client stable when repeated schemas are merged.
//!
//! `merge_repeated_schemas` replaces repeated inline schemas with one shared component, so the
//! generator no longer emits the per-use types it emitted before (for example
//! `CreateCommandRequestDeploymentId`). Code written against those names would stop compiling
//! when another crate in the same build turns on `full-api`, which would make the feature
//! remove names. The build script also generates two throwaway clients whose schema
//! descriptions end with their position in the merged document
//! (`openapi_filter::merge_repeated_schemas_with_positions`), and declares every type name the
//! merge removed as an alias of the merged type at the same position.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::openapi_filter::POSITION_MARKER;

/// Adds `pub type <Old> = <Merged>;` to `types` of `merged` for every type of `unmerged` that
/// `merged` no longer has, and the same to `types::builder` for builder types. `unmerged` and
/// `positioned` are the clients generated from the marked copies. Returns the number of
/// aliases.
///
/// Fails when a removed type has no merged type at its position: that would silently drop a
/// public type from the complete client.
pub fn add_aliases(
    unmerged: &syn::File,
    positioned: &syn::File,
    merged: &mut syn::File,
) -> Result<usize, String> {
    let before = types_module(unmerged)?;
    let after_names: BTreeSet<String> = type_docs(types_module(merged)?)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let after_builders = builder_names(types_module(merged)?);
    let before_builders = builder_names(before);

    let mut by_position = HashMap::<String, String>::new();
    for (name, docs) in type_docs(types_module(positioned)?) {
        if let Some(position) = own_position(&docs) {
            by_position.entry(position).or_insert(name);
        }
    }

    let mut aliases = BTreeMap::<String, String>::new();
    let mut unmatched = Vec::new();
    for (name, docs) in type_docs(before) {
        if after_names.contains(&name) {
            continue;
        }
        let target = own_position(&docs).and_then(|position| by_position.get(&position));
        match target {
            Some(target) if after_names.contains(target) => {
                aliases.insert(name, target.clone());
            }
            _ => unmatched.push(name),
        }
    }
    if !unmatched.is_empty() {
        return Err(format!(
            "merging repeated schemas removed {} types with no merged type at their position: {unmatched:?}",
            unmatched.len()
        ));
    }

    let types = types_module_mut(merged)?;
    for (old, new) in &aliases {
        types.push(alias_item(old, new)?);
    }
    let builder_aliases: Vec<_> = aliases
        .iter()
        .filter(|(old, new)| before_builders.contains(*old) && after_builders.contains(*new))
        .collect();
    if !builder_aliases.is_empty() {
        let builder = types
            .iter_mut()
            .find_map(|item| match item {
                syn::Item::Mod(module) if module.ident == "builder" => {
                    module.content.as_mut().map(|(_, items)| items)
                }
                _ => None,
            })
            .ok_or("the merged client has no `types::builder` module")?;
        for (old, new) in builder_aliases {
            builder.push(alias_item(old, new)?);
        }
    }
    Ok(aliases.len())
}

/// The first position in a type's docs. Its own description comes first; later positions
/// belong to nested schemas quoted in its JSON schema section.
fn own_position(docs: &str) -> Option<String> {
    let start = docs.find(POSITION_MARKER)? + POSITION_MARKER.len();
    let position = docs[start..].lines().next()?.trim();
    (!position.is_empty()).then(|| position.to_string())
}

fn alias_item(old: &str, new: &str) -> Result<syn::Item, String> {
    let source = format!(
        "#[doc = \"Same type as [`{new}`]: the schemas behind both names are identical.\"]\npub type {old} = {new};"
    );
    syn::parse_str(&source).map_err(|error| format!("alias `{old}`: {error}"))
}

/// Every struct, enum and type alias of `types`, with its doc text.
fn type_docs(items: &[syn::Item]) -> Vec<(String, String)> {
    items
        .iter()
        .filter_map(|item| {
            let (ident, attrs) = match item {
                syn::Item::Struct(item) => (&item.ident, &item.attrs),
                syn::Item::Enum(item) => (&item.ident, &item.attrs),
                syn::Item::Type(item) => (&item.ident, &item.attrs),
                _ => return None,
            };
            Some((ident.to_string(), doc_text(attrs)))
        })
        .collect()
}

fn doc_text(attrs: &[syn::Attribute]) -> String {
    let mut text = String::new();
    for attr in attrs {
        if let syn::Meta::NameValue(meta) = &attr.meta {
            if meta.path.is_ident("doc") {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) = &meta.value
                {
                    text.push_str(&value.value());
                    text.push('\n');
                }
            }
        }
    }
    text
}

fn builder_names(types: &[syn::Item]) -> BTreeSet<String> {
    types
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "builder" => {
                module.content.as_ref().map(|(_, items)| items)
            }
            _ => None,
        })
        .flatten()
        .filter_map(|item| match item {
            syn::Item::Struct(item) => Some(item.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn types_module(file: &syn::File) -> Result<&[syn::Item], String> {
    file.items
        .iter()
        .find_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "types" => {
                module.content.as_ref().map(|(_, items)| items.as_slice())
            }
            _ => None,
        })
        .ok_or_else(|| "the generated client has no `types` module".to_string())
}

fn types_module_mut(file: &mut syn::File) -> Result<&mut Vec<syn::Item>, String> {
    file.items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "types" => {
                module.content.as_mut().map(|(_, items)| items)
            }
            _ => None,
        })
        .ok_or_else(|| "the generated client has no `types` module".to_string())
}
