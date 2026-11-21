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
/// Fails when a removed type has no merged type at its position or more than one, when a
/// removed builder has no merged builder, or when a kept name is no longer a type at its
/// position: each would silently drop or change a public type of the complete client.
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

    let mut by_position = HashMap::<String, Vec<String>>::new();
    for (name, position) in positions(types_module(positioned)?) {
        by_position.entry(position).or_default().push(name);
    }
    let before_positions = positions(before);

    let mut aliases = BTreeMap::<String, String>::new();
    let mut unmatched = Vec::new();
    let mut ambiguous = Vec::new();
    let mut moved = Vec::new();
    for (name, _) in type_docs(before) {
        let targets = before_positions
            .get(&name)
            .and_then(|position| by_position.get(position));
        if after_names.contains(&name) {
            // A kept name must still be a type at its own position, or code using it would
            // silently get a different type.
            if let Some(targets) = targets {
                if !targets.contains(&name) {
                    moved.push(format!("{name} -> {targets:?}"));
                }
            }
            continue;
        }
        match targets.map(Vec::as_slice) {
            Some([target]) if after_names.contains(target) => {
                aliases.insert(name, target.clone());
            }
            // The generator copies a schema into every type that flattens it, so one position
            // can hold several types. An alias there could point at the wrong one.
            Some(targets @ [_, _, ..]) => ambiguous.push(format!("{name} -> {targets:?}")),
            _ => unmatched.push(name),
        }
    }
    if !unmatched.is_empty() {
        return Err(format!(
            "merging repeated schemas removed {} types with no merged type at their position: {unmatched:?}",
            unmatched.len()
        ));
    }
    if !ambiguous.is_empty() {
        return Err(format!(
            "merging repeated schemas removed {} types whose position holds more than one merged type: {ambiguous:?}",
            ambiguous.len()
        ));
    }
    if !moved.is_empty() {
        return Err(format!(
            "merging repeated schemas gave {} kept type names a different type: {moved:?}",
            moved.len()
        ));
    }

    let types = types_module_mut(merged)?;
    for (old, new) in &aliases {
        types.push(alias_item(old, new)?);
    }
    let builder_aliases: Vec<_> = aliases
        .iter()
        .filter(|(old, _)| before_builders.contains(*old))
        .collect();
    let missing_builders: Vec<_> = builder_aliases
        .iter()
        .filter(|(_, new)| !after_builders.contains(*new))
        .map(|(old, new)| format!("{old} -> {new}"))
        .collect();
    if !missing_builders.is_empty() {
        return Err(format!(
            "merging repeated schemas removed {} builder types whose merged type has no builder: {missing_builders:?}",
            missing_builders.len()
        ));
    }
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

/// The position of every type that has one, keyed by type name. A nullable object schema
/// becomes two types with the same description, `<Name>(Option<<Name>Inner>)` and
/// `<Name>Inner`; the inner one gets the position with an `/inner` suffix so each has its own.
fn positions(items: &[syn::Item]) -> HashMap<String, String> {
    let own: HashMap<String, String> = type_docs(items)
        .into_iter()
        .filter_map(|(name, docs)| own_position(&docs).map(|position| (name, position)))
        .collect();
    own.iter()
        .map(|(name, position)| {
            let outer = name.strip_suffix("Inner").and_then(|outer| own.get(outer));
            let position = match outer {
                Some(outer_position) if outer_position == position => format!("{position}/inner"),
                _ => position.clone(),
            };
            (name.clone(), position)
        })
        .collect()
}

/// The position in a type's own description. The generator quotes the type's JSON schema
/// after it in a `<details>` section, and nested schemas there carry their own positions, so
/// only the text before that section is read.
fn own_position(docs: &str) -> Option<String> {
    let own = docs.split("<details>").next().unwrap_or_default();
    let start = own.find(POSITION_MARKER)? + POSITION_MARKER.len();
    let position = own[start..].lines().next()?.trim();
    (!position.is_empty()).then(|| position.to_string())
}

fn alias_item(old: &str, new: &str) -> Result<syn::Item, String> {
    let source = format!(
        "#[doc = \"Same type as [`{new}`]: the schemas behind both names are structurally identical (their descriptions can differ).\"]\n#[doc(hidden)]\npub type {old} = {new};"
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
