use std::collections::HashSet;

fn main() {
    generate_azure_models();
}

fn generate_azure_models() {
    println!("cargo:rerun-if-changed=build.rs");

    let specs = [
        (
            "./openapi/ContainerApps.json",
            "src/azure/models/container_apps.rs",
        ),
        (
            "./openapi/ManagedEnvironments.json",
            "src/azure/models/managed_environments.rs",
        ),
        ("./openapi/Jobs.json", "src/azure/models/jobs.rs"),
        (
            "./openapi/ManagedEnvironmentsDaprComponents.json",
            "src/azure/models/managed_environments_dapr_components.rs",
        ),
        (
            "./openapi/authorization-RoleDefinitionsCalls.json",
            "src/azure/models/authorization_role_definitions.rs",
        ),
        (
            "./openapi/authorization-RoleAssignmentsCalls.json",
            "src/azure/models/authorization_role_assignments.rs",
        ),
        (
            "./openapi/ManagedIdentity.json",
            "src/azure/models/managed_identity.rs",
        ),
        ("./openapi/blob.json", "src/azure/models/blob.rs"),
        ("./openapi/table.json", "src/azure/models/table.rs"),
        ("./openapi/storage.json", "src/azure/models/storage.rs"),
        ("./openapi/resources.json", "src/azure/models/resources.rs"),
        (
            "./openapi/containerregistry.json",
            "src/azure/models/containerregistry.rs",
        ),
        ("./openapi/keyvault.json", "src/azure/models/keyvault.rs"),
        ("./openapi/secrets.json", "src/azure/models/secrets.rs"),
        (
            "./openapi/certificates.json",
            "src/azure/models/certificates.rs",
        ),
        ("./openapi/Queue.json", "src/azure/models/queue.rs"),
        (
            "./openapi/namespace-preview.json",
            "src/azure/models/queue_namespace.rs",
        ),
        (
            "./openapi/virtualNetwork.json",
            "src/azure/models/virtual_network.rs",
        ),
        (
            "./openapi/natGateway.json",
            "src/azure/models/nat_gateway.rs",
        ),
        (
            "./openapi/publicIpAddress.json",
            "src/azure/models/public_ip_address.rs",
        ),
        (
            "./openapi/networkSecurityGroup.json",
            "src/azure/models/network_security_group.rs",
        ),
        (
            "./openapi/loadBalancer.json",
            "src/azure/models/load_balancer.rs",
        ),
        ("./openapi/ComputeRP.json", "src/azure/models/compute_rp.rs"),
        ("./openapi/DiskRP.json", "src/azure/models/disk_rp.rs"),
    ];

    for (src, output_file) in specs.iter() {
        println!("cargo:rerun-if-changed={}", src);
        let file = std::fs::File::open(src).unwrap();
        let mut spec: openapiv3::OpenAPI = serde_json::from_reader(file).unwrap();
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
                    if module.ident == "types"
                        && module.vis == syn::Visibility::Public(Default::default())
                    {
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

        // Add bon::Builder to all struct derives
        // for item in types_content.iter_mut() {
        //     if let syn::Item::Struct(struct_item) = item {
        //         // Only add bon::Builder to structs with named fields
        //         if matches!(struct_item.fields, syn::Fields::Named(_)) {
        //             // Find the derive attribute and add bon::Builder to it
        //             for attr in struct_item.attrs.iter_mut() {
        //                 if attr.path().is_ident("derive") {
        //                     if let syn::Meta::List(ref mut meta_list) = attr.meta {
        //                         // Convert the existing tokens to string, add bon::Builder, and reparse
        //                         let existing_derives = meta_list.tokens.to_string();
        //                         let new_derives = if existing_derives.is_empty() {
        //                             "bon::Builder".to_string()
        //                         } else {
        //                             format!("{}, bon::Builder", existing_derives)
        //                         };

        //                         // Parse the new derive list back into tokens
        //                         meta_list.tokens = new_derives.parse().unwrap();
        //                     }
        //                     break;
        //                 }
        //             }
        //         }
        //     }
        // }

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

        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
        let file_name = std::path::Path::new(output_file)
            .file_name()
            .expect("invalid output_file");
        let out_file = std::path::Path::new(&out_dir).join(file_name);

        std::fs::write(out_file, content).unwrap();
    }
}
