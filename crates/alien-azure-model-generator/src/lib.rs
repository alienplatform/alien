use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

pub mod schema_filter;

pub const MODEL_ROOTS_JSON: &str = include_str!("../model_roots.json");
pub const MODEL_SHARDS_JSON: &str = include_str!("../model_shards.json");

const MODEL_SPECS: [(&str, &str); 25] = [
    ("ContainerApps.json", "container_apps.rs"),
    ("ManagedEnvironments.json", "managed_environments.rs"),
    ("Jobs.json", "jobs.rs"),
    (
        "ManagedEnvironmentsDaprComponents.json",
        "managed_environments_dapr_components.rs",
    ),
    (
        "authorization-RoleDefinitionsCalls.json",
        "authorization_role_definitions.rs",
    ),
    (
        "authorization-RoleAssignmentsCalls.json",
        "authorization_role_assignments.rs",
    ),
    ("ManagedIdentity.json", "managed_identity.rs"),
    ("blob.json", "blob.rs"),
    ("table.json", "table.rs"),
    ("storage.json", "storage.rs"),
    ("resources.json", "resources.rs"),
    ("containerregistry.json", "containerregistry.rs"),
    ("keyvault.json", "keyvault.rs"),
    ("secrets.json", "secrets.rs"),
    ("certificates.json", "certificates.rs"),
    ("Queue.json", "queue.rs"),
    ("namespace-preview.json", "queue_namespace.rs"),
    ("virtualNetwork.json", "virtual_network.rs"),
    ("natGateway.json", "nat_gateway.rs"),
    ("publicIpAddress.json", "public_ip_address.rs"),
    ("networkSecurityGroup.json", "network_security_group.rs"),
    ("loadBalancer.json", "load_balancer.rs"),
    ("ComputeRP.json", "compute_rp.rs"),
    ("DiskRP.json", "disk_rp.rs"),
    ("managedClusters.json", "managed_clusters.rs"),
];

/// Return the specification filenames assigned to a line-balanced model shard.
pub fn model_specs(shard: usize) -> Vec<String> {
    let shards = parse_model_shards(MODEL_SHARDS_JSON)
        .unwrap_or_else(|message| panic!("invalid Azure model shard manifest: {message}"));
    shards
        .get(&shard.to_string())
        .unwrap_or_else(|| panic!("missing Azure model shard {shard}"))
        .clone()
}

/// Find the line-balanced shard that owns an Azure specification.
pub fn model_shard_for_spec(spec_name: &str) -> Option<usize> {
    let shards = parse_model_shards(MODEL_SHARDS_JSON)
        .unwrap_or_else(|message| panic!("invalid Azure model shard manifest: {message}"));
    shards.into_iter().find_map(|(shard, specs)| {
        specs
            .iter()
            .any(|candidate| candidate == spec_name)
            .then(|| shard.parse().expect("validated Azure model shard key"))
    })
}

fn parse_model_shards(json: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    let shards: BTreeMap<String, Vec<String>> =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    let known_specs = MODEL_SPECS
        .iter()
        .map(|(spec_name, _)| *spec_name)
        .collect::<BTreeSet<_>>();
    let mut assigned_specs = BTreeSet::new();

    for (shard, specs) in &shards {
        shard
            .parse::<usize>()
            .map_err(|_| format!("shard key {shard:?} is not a positive integer"))?
            .checked_sub(1)
            .ok_or_else(|| "shard keys must start at 1".to_string())?;
        for spec_name in specs {
            if !assigned_specs.insert(spec_name.as_str()) {
                return Err(format!(
                    "specification {spec_name:?} is assigned to multiple shards"
                ));
            }
        }
    }

    if assigned_specs != known_specs {
        let missing = known_specs.difference(&assigned_specs).collect::<Vec<_>>();
        let unknown = assigned_specs.difference(&known_specs).collect::<Vec<_>>();
        return Err(format!(
            "manifest and generator mappings differ; missing {missing:?}, unknown {unknown:?}"
        ));
    }

    Ok(shards)
}

fn validate_selected_specs(selected_specs: &[String], openapi_dir: &Path) {
    let selected = selected_specs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        selected.len(),
        selected_specs.len(),
        "Azure model shard contains duplicate specifications"
    );

    let checked_in = std::fs::read_dir(openapi_dir)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", openapi_dir.display()))
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("json"))
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect::<BTreeSet<_>>();
    let checked_in = checked_in
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        checked_in, selected,
        "checked-in Azure specifications do not exactly match their model shard"
    );
}

/// Generate a selected subset of Azure model modules into Cargo's output directory.
pub fn generate_azure_models(
    selected_specs: &[String],
    openapi_dir: &std::path::Path,
    model_roots_json: &str,
    full_models: bool,
    max_filtered_lines: usize,
) -> usize {
    parse_model_shards(MODEL_SHARDS_JSON)
        .unwrap_or_else(|message| panic!("invalid Azure model shard manifest: {message}"));
    validate_selected_specs(selected_specs, openapi_dir);
    let model_roots: BTreeMap<String, Vec<String>> =
        serde_json::from_str(model_roots_json).unwrap();
    let mut total_generated_lines = 0;

    for (spec_name, output_file) in MODEL_SPECS
        .iter()
        .filter(|(spec_name, _)| selected_specs.iter().any(|selected| selected == *spec_name))
    {
        let source_path = openapi_dir.join(*spec_name);
        println!("cargo:rerun-if-changed={}", source_path.display());
        let file = std::fs::File::open(source_path).unwrap();
        let mut spec_json: serde_json::Value = serde_json::from_reader(file).unwrap();
        remove_zero_min_length(&mut spec_json);
        if !full_models {
            let roots = model_roots
                .get(*spec_name)
                .unwrap_or_else(|| panic!("missing model roots for {spec_name}"));
            schema_filter::retain_reachable_schemas(&mut spec_json, roots).unwrap();
        }
        let mut spec: openapiv3::OpenAPI = serde_json::from_value(spec_json).unwrap();
        spec.paths = Default::default();

        let mut generator = progenitor::Generator::default();

        let tokens = generator.generate_tokens(&spec).unwrap();
        let ast: syn::File = syn::parse2(tokens).unwrap();

        // Find the types module and extract only its content
        let types_module = ast
            .items
            .iter()
            .find_map(|item| {
                if let syn::Item::Mod(module) = item {
                    if module.ident == "types" && matches!(module.vis, syn::Visibility::Public(_)) {
                        return Some(module);
                    }
                }
                None
            })
            .expect("Could not find pub mod types in generated code");

        // Create a new file with only the types module content
        let mut types_content = if let Some((_, items)) = &types_module.content {
            items.clone()
        } else {
            panic!("Types module has no content");
        };

        // Add deserialize_with for fields with serde(default)
        for item in types_content.iter_mut() {
            if let syn::Item::Struct(struct_item) = item {
                if let syn::Fields::Named(ref mut fields) = struct_item.fields {
                    for field in fields.named.iter_mut() {
                        // Check if field has serde attribute with default
                        let mut has_serde_default = false;
                        let mut serde_attr_index = None;

                        for (i, attr) in field.attrs.iter().enumerate() {
                            if attr.path().is_ident("serde") {
                                serde_attr_index = Some(i);

                                // Check if the serde attribute contains "default"
                                let attr_tokens = attr
                                    .meta
                                    .require_list()
                                    .map(|list| list.tokens.to_string())
                                    .unwrap_or_default();
                                if attr_tokens.contains("default")
                                    && !attr_tokens.contains("deserialize_with")
                                    && !attr_tokens.contains("default =")
                                {
                                    has_serde_default = true;
                                    break;
                                }
                            }
                        }

                        // If we found serde(default) without deserialize_with, modify the attribute
                        if has_serde_default {
                            if let Some(attr_index) = serde_attr_index {
                                let attr = &mut field.attrs[attr_index];
                                if let syn::Meta::List(ref mut meta_list) = attr.meta {
                                    let existing_tokens = meta_list.tokens.to_string();
                                    let new_tokens = if existing_tokens.trim().is_empty() {
                                        "default, deserialize_with = \"serde_aux::field_attributes::deserialize_default_from_null\"".to_string()
                                    } else {
                                        format!("{}, deserialize_with = \"serde_aux::field_attributes::deserialize_default_from_null\"", existing_tokens)
                                    };
                                    meta_list.tokens = new_tokens.parse().unwrap();
                                }
                            }
                        }
                    }
                }
            }
        }

        // Collect enum names first
        let enum_names: HashSet<String> = types_content
            .iter()
            .filter_map(|item| {
                if let syn::Item::Enum(enum_item) = item {
                    Some(enum_item.ident.to_string())
                } else {
                    None
                }
            })
            .collect();

        // Add serde(try_from = "String") to enums and modify FromStr to be case-insensitive
        for item in types_content.iter_mut() {
            if let syn::Item::Enum(enum_item) = item {
                // Add serde(try_from = "String") attribute to the enum
                let try_from_attr: syn::Attribute = syn::parse_quote! {
                    #[serde(try_from = "String")]
                };
                enum_item.attrs.push(try_from_attr);
            } else if let syn::Item::Impl(impl_item) = item {
                // Modify FromStr implementation to be case-insensitive for enums only
                if let Some((_, trait_path, _)) = &impl_item.trait_ {
                    if trait_path
                        .segments
                        .last()
                        .map(|s| &s.ident)
                        .map(|i| i.to_string())
                        == Some("FromStr".to_string())
                    {
                        // Check if this impl is for an enum type
                        let is_enum_impl = if let syn::Type::Path(type_path) = &*impl_item.self_ty {
                            let type_name =
                                type_path.path.segments.last().map(|s| s.ident.to_string());
                            type_name.map_or(false, |name| enum_names.contains(&name))
                        } else {
                            false
                        };

                        if !is_enum_impl {
                            continue;
                        }
                        // Find the from_str method and modify it
                        for impl_item_inner in impl_item.items.iter_mut() {
                            if let syn::ImplItem::Fn(method) = impl_item_inner {
                                if method.sig.ident == "from_str" {
                                    // Replace the method body to use case-insensitive matching
                                    let new_body: syn::Expr =
                                        if let syn::Stmt::Expr(syn::Expr::Match(match_expr), _) =
                                            &method.block.stmts[0]
                                        {
                                            // Extract the match arms and modify them
                                            let arms = &match_expr.arms;
                                            let mut new_arms: Vec<syn::Arm> = Vec::new();

                                            for arm in arms {
                                                if let syn::Pat::Lit(pat_lit) = &arm.pat {
                                                    if let syn::Lit::Str(lit_str) = &pat_lit.lit {
                                                        // Convert pattern to lowercase to match the lowercased input
                                                        let value =
                                                            lit_str.value().to_ascii_lowercase();
                                                        let body = &arm.body;
                                                        let new_arm: syn::Arm = syn::parse_quote! {
                                                            #value => #body,
                                                        };
                                                        new_arms.push(new_arm);
                                                    }
                                                } else if let syn::Pat::Wild(_) = &arm.pat {
                                                    // Keep the wildcard arm as-is
                                                    new_arms.push(arm.clone());
                                                }
                                            }

                                            syn::parse_quote! {
                                                match value.to_ascii_lowercase().as_str() {
                                                    #(#new_arms)*
                                                }
                                            }
                                        } else {
                                            // Fallback in case the structure is different
                                            syn::parse_quote! {
                                                match value.to_ascii_lowercase().as_str() {
                                                    _ => Err("invalid value".into()),
                                                }
                                            }
                                        };

                                    method.block = syn::parse_quote! {
                                        { #new_body }
                                    };
                                }
                            }
                        }
                    }
                }
            }
        }

        // Create a new syn::File with just the types content
        let new_file = syn::File {
            shebang: None,
            attrs: Vec::new(),
            items: types_content,
        };

        let content = prettyplease::unparse(&new_file);
        total_generated_lines += content.lines().count();

        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
        let out_file = std::path::Path::new(&out_dir).join(*output_file);

        std::fs::write(out_file, content).unwrap();
    }

    if !full_models {
        assert!(
            total_generated_lines <= max_filtered_lines,
            "filtered Azure models expanded to {total_generated_lines} lines; update model roots intentionally or investigate newly reachable schemas"
        );
    }
    total_generated_lines
}

fn remove_zero_min_length(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            if object.get("minLength").and_then(|value| value.as_u64()) == Some(0) {
                object.remove("minLength");
            }
            for child in object.values_mut() {
                remove_zero_min_length(child);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                remove_zero_min_length(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_model_shards, MODEL_SHARDS_JSON};

    #[test]
    fn checked_in_manifest_has_exactly_one_owner_for_every_generator_spec() {
        parse_model_shards(MODEL_SHARDS_JSON).unwrap();
    }

    #[test]
    fn rejects_duplicate_spec_ownership() {
        let manifest = MODEL_SHARDS_JSON.replace(
            "\"publicIpAddress.json\"",
            "\"ComputeRP.json\", \"publicIpAddress.json\"",
        );

        assert!(parse_model_shards(&manifest)
            .unwrap_err()
            .contains("assigned to multiple shards"));
    }

    #[test]
    fn rejects_missing_generator_spec() {
        let manifest = MODEL_SHARDS_JSON.replace("    \"publicIpAddress.json\",\n", "");

        assert!(parse_model_shards(&manifest)
            .unwrap_err()
            .contains("manifest and generator mappings differ"));
    }
}
