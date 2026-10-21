//! The Azure roles the runtime assigns after setup, which setup creates, and the condition that
//! limits the management identity to assigning exactly those.
//!
//! After setup the management identity creates Live resources and assigns roles on them: to the
//! application's identities (permission profiles and the remote-bindings identity) and to itself
//! (`management.extend` grants named for a resource). Azure has no "only the roles this stack
//! declares" permission, so setup builds one:
//!
//! - Setup creates every custom role those assignments use, under the ids the runtime derives,
//!   assignable at the subscription too. Management holds no `roleDefinitions/write`, so it can
//!   neither create a role nor change one setup made.
//! - Management's `roleAssignments/write` is assigned with a condition that admits only those
//!   roles: the management roles onto the management identity, the application roles onto any
//!   other principal.
//!
//! The condition cannot name the assignment scope, so a listed role can be assigned anywhere in
//! the scope management's role-assignment role is assigned at (the deployment resource group).

use crate::{
    error::Result,
    generators::{
        azure_runtime::{AzureRoleDefinition, AzureRoleDefinitionRef},
        AzureRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use alien_core::{
    permissions::{PermissionProfile, PermissionSetReference},
    PermissionSet, Stack,
};
use alien_error::Context;
use std::collections::BTreeSet;
use uuid::Uuid;

/// `Azure Service Bus Data Sender`: a worker's direct command sender, assigned by the runtime to
/// the management identity on the worker's command queue.
pub const AZURE_SERVICE_BUS_DATA_SENDER_ROLE_ID: &str = "69a216fc-b8fb-44d8-bc22-1f3c2cd27a39";

/// `Azure Service Bus Data Receiver`: assigned by the runtime to a worker's execution identity on
/// a storage-trigger queue.
pub const AZURE_SERVICE_BUS_DATA_RECEIVER_ROLE_ID: &str = "4f6d3b9b-027b-4f4c-9142-0e5a2a2247e0";

/// The profile name the runtime uses for the remote-bindings identity's roles.
pub const AZURE_REMOTE_BINDINGS_PROFILE: &str = "remote-bindings";

/// Longest condition a shard holds. Azure refuses a role-assignment condition over 8192
/// characters; the margin leaves room for a template expression in place of a principal id.
pub const AZURE_CONDITION_SHARD_LIMIT: usize = 7500;

const ROLE_ASSIGNMENT_WRITE: &str = "Microsoft.Authorization/roleAssignments/write";
const REQUEST: &str = "@Request[Microsoft.Authorization/roleAssignments";

/// Who a runtime-assigned role is for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AzureRuntimeRoleHolder {
    /// The management identity itself.
    Management,
    /// The identity of a permission profile, or the remote-bindings identity
    /// ([`AZURE_REMOTE_BINDINGS_PROFILE`]).
    Profile(String),
}

impl AzureRuntimeRoleHolder {
    /// The name tag the runtime gives the holder's custom roles: `[mgmt]` or `[<profile>]`.
    pub fn role_name_tag(&self) -> &str {
        match self {
            Self::Management => "mgmt",
            Self::Profile(profile) => profile,
        }
    }

    /// The seed of a custom role's id; [`azure_runtime_role_definition_uuid`] hashes it.
    ///
    /// `resource_prefix` may be a template (`${local.resource_prefix}`), so Terraform can derive
    /// the same id with `uuidv5("oid", seed)`.
    pub fn role_definition_seed(
        &self,
        resource_prefix: &str,
        permission_set_id: &str,
        custom_role_key: &str,
    ) -> String {
        let segment = azure_role_key_segment(custom_role_key);
        match self {
            Self::Management => format!(
                "deployment:azure:mgmt-res-role-def:{resource_prefix}:{permission_set_id}:{segment}"
            ),
            Self::Profile(profile) => format!(
                "deployment:azure:res-role-def:{resource_prefix}:{profile}:{permission_set_id}:{segment}"
            ),
        }
    }
}

/// The id the runtime uses for a holder's custom role.
pub fn azure_runtime_role_definition_uuid(
    holder: &AzureRuntimeRoleHolder,
    resource_prefix: &str,
    permission_set_id: &str,
    custom_role_key: &str,
) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        holder
            .role_definition_seed(resource_prefix, permission_set_id, custom_role_key)
            .as_bytes(),
    )
    .to_string()
}

/// The last segment of a custom role key, made safe for ids and Terraform addresses.
pub fn azure_role_key_segment(key: &str) -> String {
    key.rsplit(':')
        .next()
        .map(|segment| {
            segment
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| "custom".to_string())
}

/// A custom role the runtime assigns after setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AzureRuntimeCustomRole {
    /// Who the runtime assigns it to.
    pub holder: AzureRuntimeRoleHolder,
    /// The permission set it comes from.
    pub permission_set_id: String,
    /// The grant plan's key for it; part of the role's id.
    pub key: String,
    /// The role's actions. `name` is the generator's name, without prefix or holder tag.
    pub role_definition: AzureRoleDefinition,
}

impl AzureRuntimeCustomRole {
    /// The id the runtime uses for this role.
    pub fn uuid(&self, resource_prefix: &str) -> String {
        azure_runtime_role_definition_uuid(
            &self.holder,
            resource_prefix,
            &self.permission_set_id,
            &self.key,
        )
    }

    /// The name the runtime gives this role: `<prefix>-<name> [<holder>]`.
    pub fn role_name(&self, resource_prefix: &str) -> String {
        format!(
            "{resource_prefix}-{} [{}]",
            self.role_definition.name,
            self.holder.role_name_tag()
        )
    }
}

/// Every role the runtime assigns after setup for a stack.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AzureRuntimeRoles {
    /// Custom roles, which setup creates. Each holder/set/key appears once.
    pub custom_roles: Vec<AzureRuntimeCustomRole>,
    /// Built-in role ids the runtime assigns to the management identity.
    pub management_predefined_role_ids: BTreeSet<String>,
    /// Built-in role ids the runtime assigns to the application's identities.
    pub workload_predefined_role_ids: BTreeSet<String>,
}

impl AzureRuntimeRoles {
    /// The role ids the runtime may assign to the management identity.
    pub fn management_role_ids(&self, resource_prefix: &str) -> BTreeSet<String> {
        self.role_ids(resource_prefix, true)
    }

    /// The role ids the runtime may assign to the application's identities.
    pub fn workload_role_ids(&self, resource_prefix: &str) -> BTreeSet<String> {
        self.role_ids(resource_prefix, false)
    }

    fn role_ids(&self, resource_prefix: &str, management: bool) -> BTreeSet<String> {
        let predefined = if management {
            &self.management_predefined_role_ids
        } else {
            &self.workload_predefined_role_ids
        };
        self.custom_roles
            .iter()
            .filter(|role| (role.holder == AzureRuntimeRoleHolder::Management) == management)
            .map(|role| role.uuid(resource_prefix))
            .chain(predefined.iter().cloned())
            .collect()
    }

    /// Whether the runtime assigns any role at all after setup.
    pub fn is_empty(&self) -> bool {
        self.custom_roles.is_empty()
            && self.management_predefined_role_ids.is_empty()
            && self.workload_predefined_role_ids.is_empty()
    }
}

/// Every role the runtime may assign after setup, for every resource in the stack: the roles
/// the permission profiles and the remote-bindings identity get on a resource, the
/// resource-scoped `management` grants, and the built-ins Azure workers use.
///
/// This lists more than a given deployment uses (a Frozen resource's grants are assigned by setup
/// itself), so the runtime always finds its role already made.
pub fn azure_runtime_roles(stack: &Stack) -> Result<AzureRuntimeRoles> {
    let mut roles = AzureRuntimeRoles::default();
    let mut seen = BTreeSet::new();
    let generator = AzureRuntimePermissionsGenerator::new();
    let context = enumeration_context();

    let mut add = |holder: AzureRuntimeRoleHolder,
                   permission_set: &PermissionSet,
                   roles: &mut AzureRuntimeRoles|
     -> Result<()> {
        if !supports_azure_resource_binding(permission_set)
            || !seen.insert((holder.clone(), permission_set.id.clone()))
        {
            return Ok(());
        }
        let plan = generator
            .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
            .context(crate::error::ErrorData::GeneratorError {
                platform: "azure".to_string(),
                message: format!(
                    "Failed to plan the runtime roles of permission set '{}'",
                    permission_set.id
                ),
            })?;
        for binding in &plan.bindings {
            if let AzureRoleDefinitionRef::Predefined { role_definition_id } =
                &binding.role_definition
            {
                let id = role_definition_guid(role_definition_id);
                if holder == AzureRuntimeRoleHolder::Management {
                    roles.management_predefined_role_ids.insert(id);
                } else {
                    roles.workload_predefined_role_ids.insert(id);
                }
            }
        }
        roles
            .custom_roles
            .extend(plan.custom_roles.into_iter().map(|custom_role| {
                AzureRuntimeCustomRole {
                    holder: holder.clone(),
                    permission_set_id: permission_set.id.clone(),
                    key: custom_role.key,
                    role_definition: custom_role.role_definition,
                }
            }));
        Ok(())
    };

    for (resource_id, entry) in stack.resources() {
        let resource_type = entry.config.resource_type().to_string();
        for (profile_name, profile) in &stack.permissions.profiles {
            for reference in resource_scoped_refs(profile, resource_id, &resource_type) {
                if let Some(set) = resolve(reference) {
                    add(
                        AzureRuntimeRoleHolder::Profile(profile_name.clone()),
                        &set,
                        &mut roles,
                    )?;
                }
            }
        }
        if let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(entry) {
            if let Some(set) = get_permission_set(definition.permission_set) {
                add(
                    AzureRuntimeRoleHolder::Profile(AZURE_REMOTE_BINDINGS_PROFILE.to_string()),
                    set,
                    &mut roles,
                )?;
            }
        }
        if let Some(management) = stack.management().profile() {
            // `/provision` sets are bound at the resource group by setup.
            for reference in resource_scoped_refs(management, resource_id, &resource_type)
                .into_iter()
                .filter(|reference| !reference.id().ends_with("/provision"))
            {
                if let Some(set) = resolve(reference) {
                    add(AzureRuntimeRoleHolder::Management, &set, &mut roles)?;
                }
            }
        }
        if entry.config.resource_type() == alien_core::Worker::RESOURCE_TYPE {
            roles
                .management_predefined_role_ids
                .insert(AZURE_SERVICE_BUS_DATA_SENDER_ROLE_ID.to_string());
            roles
                .workload_predefined_role_ids
                .insert(AZURE_SERVICE_BUS_DATA_RECEIVER_ROLE_ID.to_string());
        }
    }
    Ok(roles)
}

/// The grants a profile names for one resource: those keyed by the resource, then the stack-wide
/// grants for its type. A worker's command transport is not a role grant.
pub fn resource_scoped_refs<'p>(
    profile: &'p PermissionProfile,
    resource_id: &str,
    resource_type: &str,
) -> Vec<&'p PermissionSetReference> {
    let type_prefix = format!("{resource_type}/");
    let transport = |reference: &&PermissionSetReference| {
        !(resource_type == "worker" && reference.id() == "worker/dispatch-command")
    };
    profile
        .0
        .get(resource_id)
        .into_iter()
        .flatten()
        .filter(transport)
        .chain(
            profile
                .0
                .get("*")
                .into_iter()
                .flatten()
                .filter(|reference| reference.id().starts_with(&type_prefix))
                .filter(transport),
        )
        .collect()
}

/// Whether a permission set grants anything on a single resource on Azure.
pub fn supports_azure_resource_binding(permission_set: &PermissionSet) -> bool {
    permission_set
        .platforms
        .azure
        .as_ref()
        .is_some_and(|permissions| {
            permissions
                .iter()
                .any(|permission| permission.binding.resource.is_some())
        })
}

fn resolve(reference: &PermissionSetReference) -> Option<PermissionSet> {
    reference.resolve(|name| get_permission_set(name).cloned())
}

/// The GUID at the end of a role definition id, lowercased.
pub fn role_definition_guid(role_definition_id: &str) -> String {
    role_definition_id
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(role_definition_id)
        .to_ascii_lowercase()
}

/// A context with every variable a resource-bound Azure grant may name. Only role keys, actions
/// and built-in ids are read from the plans it produces, and none of those depend on it.
fn enumeration_context() -> PermissionContext {
    PermissionContext::new()
        .with_subscription_id("00000000-0000-0000-0000-000000000000".to_string())
        .with_resource_group("resource-group".to_string())
        .with_stack_prefix("prefix".to_string())
        .with_managing_subscription_id("00000000-0000-0000-0000-000000000000".to_string())
        .with_managing_resource_group("resource-group".to_string())
        .with_resource_name("resource".to_string())
}

/// The conditions to assign management's role-assignment role with, one per role assignment.
///
/// Each admits a write only when it assigns a listed management role to `management_principal_id`,
/// or a listed application role to any other principal. Azure grants a write when any assignment
/// of the action admits it, so a list longer than one condition holds ([`AZURE_CONDITION_SHARD_LIMIT`])
/// is split across several assignments, each with its own role definition.
///
/// With no roles at all, a single condition that admits no write. `management_principal_id` may
/// be a template expression.
pub fn azure_role_assignment_allowlist_conditions(
    management_principal_id: &str,
    management_role_ids: &BTreeSet<String>,
    workload_role_ids: &BTreeSet<String>,
) -> Vec<String> {
    let deny_writes = format!("(!(ActionMatches{{'{ROLE_ASSIGNMENT_WRITE}'}}))");
    let clause = |ids: &[&String], management: bool| {
        let roles = ids.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", ");
        let principal = if management {
            format!("{REQUEST}:PrincipalId] ForAnyOfAnyValues:GuidEquals {{{management_principal_id}}}")
        } else {
            format!(
                "{REQUEST}:PrincipalId] ForAnyOfAllValues:GuidNotEquals {{{management_principal_id}}}"
            )
        };
        format!("({REQUEST}:RoleDefinitionId] ForAnyOfAnyValues:GuidEquals {{{roles}}} AND {principal})")
    };
    let render = |management: &[&String], workload: &[&String]| {
        let clauses: Vec<String> = [(management, true), (workload, false)]
            .into_iter()
            .filter(|(ids, _)| !ids.is_empty())
            .map(|(ids, is_management)| clause(ids, is_management))
            .collect();
        if clauses.is_empty() {
            format!("({deny_writes})")
        } else {
            format!("({deny_writes} OR {})", clauses.join(" OR "))
        }
    };

    // Fill each shard greedily, management roles first, so a short list stays in one condition.
    let all: Vec<(&String, bool)> = management_role_ids
        .iter()
        .map(|id| (id, true))
        .chain(workload_role_ids.iter().map(|id| (id, false)))
        .collect();
    let mut conditions = Vec::new();
    let mut management = Vec::new();
    let mut workload = Vec::new();
    for (id, is_management) in all {
        let (mut next_management, mut next_workload) = (management.clone(), workload.clone());
        if is_management {
            next_management.push(id);
        } else {
            next_workload.push(id);
        }
        if render(&next_management, &next_workload).len() > AZURE_CONDITION_SHARD_LIMIT
            && !(management.is_empty() && workload.is_empty())
        {
            conditions.push(render(&management, &workload));
            management.clear();
            workload.clear();
        }
        if is_management {
            management.push(id);
        } else {
            workload.push(id);
        }
    }
    if conditions.is_empty() || !(management.is_empty() && workload.is_empty()) {
        conditions.push(render(&management, &workload));
    }
    conditions
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::{ManagementPermissions, PermissionProfile},
        ResourceLifecycle, Vault, Worker, WorkerCode,
    };
    use indexmap::IndexMap;

    fn ids(prefix: &str, count: usize) -> BTreeSet<String> {
        (0..count)
            .map(|i| {
                Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{prefix}{i}").as_bytes()).to_string()
            })
            .collect()
    }

    fn profile(grants: &[(&str, &[&str])]) -> PermissionProfile {
        let mut map = IndexMap::new();
        for (resource, sets) in grants {
            map.insert(
                resource.to_string(),
                sets.iter()
                    .map(|set| PermissionSetReference::from_name(*set))
                    .collect(),
            );
        }
        PermissionProfile(map)
    }

    fn vault_stack(management: &[(&str, &[&str])]) -> Stack {
        let mut stack = Stack::new("app".to_string())
            .add(Vault::new("app-secrets".to_string()).build(), ResourceLifecycle::Live)
            .build();
        stack.permissions.profiles.insert(
            "execution".to_string(),
            profile(&[("app-secrets", &["vault/data-read"])]),
        );
        stack.permissions.management = ManagementPermissions::Extend(profile(management));
        stack
    }

    /// The fixture of the failing fresh install: a management grant named for a Live vault. Its
    /// role is listed, under the id the runtime assigns, and only for the management identity.
    #[test]
    fn a_management_grant_on_a_live_vault_is_a_setup_owned_management_role() {
        let stack = vault_stack(&[("app-secrets", &["vault/data-write"])]);
        let roles = azure_runtime_roles(&stack).unwrap();

        let write = roles
            .custom_roles
            .iter()
            .find(|role| role.holder == AzureRuntimeRoleHolder::Management)
            .expect("management role");
        assert_eq!(write.permission_set_id, "vault/data-write");
        assert!(write
            .role_definition
            .data_actions
            .iter()
            .any(|action| action == "Microsoft.KeyVault/vaults/secrets/setSecret/action"));
        assert!(!write
            .role_definition
            .data_actions
            .iter()
            .any(|action| action.contains("getSecret")));
        assert_eq!(
            write.uuid("p1"),
            Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!(
                    "deployment:azure:mgmt-res-role-def:p1:vault/data-write:{}",
                    azure_role_key_segment(&write.key)
                )
                .as_bytes()
            )
            .to_string()
        );
        assert_eq!(write.role_name("p1"), format!("p1-{} [mgmt]", write.role_definition.name));
        assert_eq!(roles.management_role_ids("p1"), BTreeSet::from([write.uuid("p1")]));

        let read = roles
            .custom_roles
            .iter()
            .find(|role| role.holder == AzureRuntimeRoleHolder::Profile("execution".to_string()))
            .expect("execution role");
        assert!(roles.workload_role_ids("p1").contains(&read.uuid("p1")));
        assert!(!roles.management_role_ids("p1").contains(&read.uuid("p1")));
    }

    /// Setup binds `/provision` sets at the resource group itself; the runtime never assigns them.
    #[test]
    fn provision_sets_are_not_runtime_roles() {
        let stack = vault_stack(&[("app-secrets", &["vault/provision"])]);
        let roles = azure_runtime_roles(&stack).unwrap();
        assert!(roles
            .custom_roles
            .iter()
            .all(|role| role.holder != AzureRuntimeRoleHolder::Management));
        assert!(roles.management_predefined_role_ids.is_empty());
    }

    /// A worker's command sender and storage-trigger receiver are built-ins the worker controller
    /// assigns, not permission-set grants, so they are listed by resource type.
    #[test]
    fn workers_list_the_service_bus_roles_their_controller_assigns() {
        let stack = Stack::new("app".to_string())
            .add(
                Worker::new("fn".to_string())
                    .code(WorkerCode::Image {
                        image: "img".to_string(),
                    })
                    .permissions("execution".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let roles = azure_runtime_roles(&stack).unwrap();
        assert!(roles
            .management_predefined_role_ids
            .contains(AZURE_SERVICE_BUS_DATA_SENDER_ROLE_ID));
        assert!(roles
            .workload_predefined_role_ids
            .contains(AZURE_SERVICE_BUS_DATA_RECEIVER_ROLE_ID));
    }

    #[test]
    fn the_condition_pairs_management_roles_with_management_and_the_rest_with_anyone_else() {
        let management = BTreeSet::from(["m1".to_string()]);
        let workload = BTreeSet::from(["w1".to_string(), "w2".to_string()]);
        let conditions = azure_role_assignment_allowlist_conditions("mp", &management, &workload);
        assert_eq!(
            conditions,
            vec![
                "((!(ActionMatches{'Microsoft.Authorization/roleAssignments/write'})) OR \
                 (@Request[Microsoft.Authorization/roleAssignments:RoleDefinitionId] ForAnyOfAnyValues:GuidEquals {m1} \
                 AND @Request[Microsoft.Authorization/roleAssignments:PrincipalId] ForAnyOfAnyValues:GuidEquals {mp}) OR \
                 (@Request[Microsoft.Authorization/roleAssignments:RoleDefinitionId] ForAnyOfAnyValues:GuidEquals {w1, w2} \
                 AND @Request[Microsoft.Authorization/roleAssignments:PrincipalId] ForAnyOfAllValues:GuidNotEquals {mp}))"
                    .to_string()
            ]
        );
    }

    #[test]
    fn no_runtime_roles_admit_no_role_assignment_write() {
        assert_eq!(
            azure_role_assignment_allowlist_conditions("mp", &BTreeSet::new(), &BTreeSet::new()),
            vec!["((!(ActionMatches{'Microsoft.Authorization/roleAssignments/write'})))".to_string()]
        );
    }

    /// A list longer than one condition holds is split, every shard under the limit, with every
    /// id in exactly one shard and each id still paired with its own principal clause.
    #[test]
    fn a_long_allowlist_is_split_into_shards_under_the_limit() {
        let management = ids("m", 30);
        let workload = ids("w", 250);
        let principal = "ead6f253-2dec-4841-b608-f2b9fc67dc86";
        let conditions =
            azure_role_assignment_allowlist_conditions(principal, &management, &workload);
        assert!(conditions.len() >= 2, "{} shards", conditions.len());
        for condition in &conditions {
            assert!(condition.len() <= AZURE_CONDITION_SHARD_LIMIT, "{}", condition.len());
        }
        for id in management.iter().chain(&workload) {
            assert_eq!(
                conditions.iter().filter(|c| c.contains(id.as_str())).count(),
                1,
                "{id}"
            );
        }
        let management_clause = |c: &str| {
            c.find("GuidEquals {ead6f253").map(|end| c[..end].to_string())
        };
        for id in &management {
            let shard = conditions.iter().find(|c| c.contains(id.as_str())).unwrap();
            let before = management_clause(shard).expect("management clause");
            assert!(before.contains(id.as_str()), "{id} outside the management clause");
        }
    }
}
