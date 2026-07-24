use crate::{block::attr, emitter::TfFragment, expr};
use alien_core::{RemoteStackManagement, Stack};
use hcl::{
    expr::Expression,
    structure::{Block, Structure},
};
use indexmap::IndexMap;

pub(super) fn apply_resource_dependencies(
    stack: &Stack,
    per_resource: &mut IndexMap<String, TfFragment>,
) {
    let dependency_addresses: IndexMap<String, Vec<Expression>> = per_resource
        .iter()
        .map(|(resource_id, fragment)| {
            let addresses = fragment
                .resource_blocks
                .iter()
                .filter_map(resource_address)
                .collect();
            (resource_id.clone(), addresses)
        })
        .collect();
    // Remote Storage grants refer back to the management identity. For the
    // management -> storage bootstrap edge, wait for the physical storage
    // resources but not the grants that cannot exist until management does.
    let remote_storage_prerequisite_addresses: IndexMap<String, Vec<Expression>> = per_resource
        .iter()
        .filter(|(resource_id, _)| {
            stack
                .resources
                .get(*resource_id)
                .is_some_and(alien_core::ResourceEntry::is_remote_frozen_storage)
        })
        .map(|(resource_id, fragment)| {
            let addresses = fragment
                .resource_blocks
                .iter()
                .filter(|resource| !is_remote_storage_permission_support_resource(resource))
                .filter_map(resource_address)
                .collect();
            (resource_id.clone(), addresses)
        })
        .collect();

    for (resource_id, entry) in stack.resources() {
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };

        let mut depends_on = Vec::new();
        for dependency in &entry.dependencies {
            if dependency.id() == resource_id {
                continue;
            }
            let addresses = if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE
            {
                remote_storage_prerequisite_addresses
                    .get(dependency.id())
                    .or_else(|| dependency_addresses.get(dependency.id()))
            } else {
                dependency_addresses.get(dependency.id())
            };
            if let Some(addresses) = addresses {
                for address in addresses {
                    if !depends_on.contains(address) {
                        depends_on.push(address.clone());
                    }
                }
            }
        }

        if depends_on.is_empty() {
            continue;
        }

        for resource in &mut fragment.resource_blocks {
            if !resource_inherits_stack_resource_dependencies(resource) {
                continue;
            }
            upsert_depends_on(resource, &depends_on);
        }
    }
}

pub(super) fn apply_azure_resource_group_dependency(
    stack: &Stack,
    labels: &IndexMap<String, String>,
    per_resource: &mut IndexMap<String, TfFragment>,
) {
    let Some((resource_group_id, resource_group_label)) =
        stack.resources().find_map(|(resource_id, entry)| {
            if entry.config.resource_type().as_ref() != "azure_resource_group" {
                return None;
            }
            Some((resource_id.as_str(), labels.get(resource_id)?.as_str()))
        })
    else {
        return;
    };

    let dependency = expr::traversal(["azurerm_resource_group", resource_group_label]);
    for (resource_id, entry) in stack.resources() {
        if resource_id == resource_group_id
            || entry.config.resource_type().as_ref() == "service_activation"
        {
            continue;
        }
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };
        for resource in &mut fragment.resource_blocks {
            upsert_depends_on(resource, std::slice::from_ref(&dependency));
        }
    }
}

pub(super) fn resource_address(resource: &Block) -> Option<Expression> {
    if resource.identifier.as_str() != "resource" {
        return None;
    }
    let provider_type = resource.labels.first()?.as_str();
    let label = resource.labels.get(1)?.as_str();
    Some(expr::traversal([provider_type, label]))
}

fn resource_inherits_stack_resource_dependencies(resource: &Block) -> bool {
    !is_gcp_iam_support_resource(resource)
}

fn is_remote_storage_permission_support_resource(resource: &Block) -> bool {
    let Some(provider_type) = resource.labels.first().map(|label| label.as_str()) else {
        return false;
    };

    provider_type == "aws_iam_role_policy"
        || provider_type == "google_storage_bucket_iam_member"
        || provider_type == "azurerm_role_assignment"
}

fn is_gcp_iam_support_resource(resource: &Block) -> bool {
    if resource.identifier.as_str() != "resource" {
        return false;
    }

    let Some(provider_type) = resource.labels.first().map(|label| label.as_str()) else {
        return false;
    };

    provider_type == "google_project_iam_custom_role" || provider_type.ends_with("_iam_member")
}

fn upsert_depends_on(resource: &mut Block, depends_on: &[Expression]) {
    for structure in &mut resource.body.0 {
        if let Structure::Attribute(attribute) = structure {
            if attribute.key.as_str() == "depends_on" {
                if let Expression::Array(existing) = &mut attribute.expr {
                    for dependency in depends_on {
                        if !existing.contains(dependency) {
                            existing.push(dependency.clone());
                        }
                    }
                } else {
                    attribute.expr = Expression::Array(depends_on.to_vec());
                }
                return;
            }
        }
    }

    resource
        .body
        .0
        .push(attr("depends_on", Expression::Array(depends_on.to_vec())));
}
