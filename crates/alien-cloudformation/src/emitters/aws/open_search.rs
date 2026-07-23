//! AWS OpenSearch — next-generation OpenSearch Serverless collection.
//!
//! Emits a collection group pinned to `Generation: NEXTGEN` (compute and
//! storage decoupled, scale-to-zero), a collection inside it encrypted with
//! an AWS-owned key, a public network security policy, and — when service
//! accounts are granted `experimental/aws-opensearch/data-access` — a
//! data-access policy plus the matching `aoss:APIAccessAll` IAM policies.
//!
//! The collection endpoint is public but every request must be SigV4-signed
//! (service name `aoss`, not `es`) and pass both IAM and the data-access
//! policy, so "public network policy" does not mean anonymous access.
//!
//! Physical names: the collection, collection group, and both policies share
//! the name `{resource-id}-{stack-suffix}`. AOSS names must match
//! `^[a-z][a-z0-9-]{2,31}$`, so the resource id is validated here
//! (lowercase, max 23 chars) before anything is emitted.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, service_account_role_id,
            stack_id_short_suffix, tags, uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, AwsOpenSearch, AwsOpenSearchCollectionType, ErrorData, PermissionProfile,
    PermissionSetReference, Result, ServiceAccount,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

/// Permission-set id prefix for this resource type.
const PERMISSION_SET_PREFIX: &str = "experimental/aws-opensearch/";

/// AOSS collection (and policy) names must match `^[a-z][a-z0-9-]{2,31}$`.
/// The emitted name is `{id}-{8-char stack suffix}`, so the id itself is
/// limited to 23 characters.
const MAX_ID_LENGTH: usize = 23;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsOpenSearchEmitter;

impl CfEmitter for AwsOpenSearchEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let search = resource_config::<AwsOpenSearch>(ctx, AwsOpenSearch::RESOURCE_TYPE)?;
        validate_id(search.id())?;
        let logical_id = required_logical_id(ctx)?;
        let name = collection_name(search.id());

        let group_id = format!("{logical_id}Group");
        let mut group = CfResource::new(
            group_id.clone(),
            "AWS::OpenSearchServerless::CollectionGroup".to_string(),
        );
        group.properties.insert("Name".to_string(), name.clone());
        // Next-gen groups have replication built in: the AOSS API rejects
        // `StandbyReplicas: DISABLED` when `Generation` is `NEXTGEN`, so the
        // field is fixed to ENABLED and not exposed on the resource.
        group
            .properties
            .insert("StandbyReplicas".to_string(), CfExpression::from("ENABLED"));
        group
            .properties
            .insert("Generation".to_string(), CfExpression::from("NEXTGEN"));
        group.properties.insert("Tags".to_string(), tags(ctx));
        // The collection is retained on stack deletion (durable search
        // state), so the group that contains it must be retained too.
        group.deletion_policy = Some("Retain".to_string());
        group.update_replace_policy = Some("Retain".to_string());

        let mut network = CfResource::new(
            format!("{logical_id}NetworkPolicy"),
            "AWS::OpenSearchServerless::SecurityPolicy".to_string(),
        );
        network.properties.insert("Name".to_string(), name.clone());
        network
            .properties
            .insert("Type".to_string(), CfExpression::from("network"));
        network
            .properties
            .insert("Policy".to_string(), network_policy(&name));

        let mut collection = CfResource::new(
            logical_id.to_string(),
            "AWS::OpenSearchServerless::Collection".to_string(),
        );
        collection
            .properties
            .insert("Name".to_string(), name.clone());
        collection.properties.insert(
            "Type".to_string(),
            CfExpression::from(match search.collection_type {
                AwsOpenSearchCollectionType::Search => "SEARCH",
                AwsOpenSearchCollectionType::VectorSearch => "VECTORSEARCH",
            }),
        );
        collection
            .properties
            .insert("CollectionGroupName".to_string(), name.clone());
        collection.properties.insert(
            "EncryptionConfig".to_string(),
            CfExpression::object([("AWSOwnedKey", CfExpression::from(true))]),
        );
        collection.properties.insert("Tags".to_string(), tags(ctx));
        collection.depends_on.push(group_id);
        collection.deletion_policy = Some("Retain".to_string());
        collection.update_replace_policy = Some("Retain".to_string());

        let mut resources = vec![group, network, collection];

        let owners = permission_owners(ctx);
        if let Some(access_policy) = data_access_policy(ctx, search, &name, &owners) {
            resources.push(access_policy);
        }
        resources.extend(api_access_iam_policies(search, logical_id, &owners)?);

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let search = resource_config::<AwsOpenSearch>(ctx, AwsOpenSearch::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("collectionName", collection_name(search.id())),
            ("collectionId", CfExpression::ref_(logical_id)),
            ("collectionArn", CfExpression::get_att(logical_id, "Arn")),
            // Next-gen collections expose no DashboardEndpoint attribute
            // (verified live: GetAtt fails and batch-get-collection omits the
            // field for NEXTGEN-group collections), so only the collection
            // endpoint is exported.
            (
                "endpoint",
                CfExpression::get_att(logical_id, "CollectionEndpoint"),
            ),
        ]))
    }

    /// Runtime binding payload. `service` is the SigV4 signing service name:
    /// OpenSearch Serverless requests are signed for `aoss`, not `es`.
    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let search = resource_config::<AwsOpenSearch>(ctx, AwsOpenSearch::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("aoss")),
            (
                "endpoint",
                CfExpression::get_att(logical_id, "CollectionEndpoint"),
            ),
            ("collectionName", collection_name(search.id())),
        ])))
    }
}

/// Reject ids that cannot become a valid AOSS collection name. Checked here
/// (not just left to CloudFormation) so the failure is a clear generation
/// error instead of a mid-deploy rollback.
fn validate_id(id: &str) -> Result<()> {
    let valid_chars = id
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    let starts_lower = id.chars().next().is_some_and(|ch| ch.is_ascii_lowercase());
    if id.len() < 3 || id.len() > MAX_ID_LENGTH || !valid_chars || !starts_lower {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "AwsOpenSearch id '{id}' is invalid: OpenSearch Serverless collection names \
                 require the id to start with a lowercase letter, contain only lowercase \
                 letters, digits, and hyphens, and be 3-{MAX_ID_LENGTH} characters long"
            ),
        }));
    }
    Ok(())
}

/// Physical collection / group / policy name: `{id}-{stack-suffix}`.
fn collection_name(id: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Join",
        CfExpression::list([
            CfExpression::from("-"),
            CfExpression::list([CfExpression::from(id), stack_id_short_suffix()]),
        ]),
    )])
}

// AOSS policy documents as structs rather than `serde_json::json!` maps: the
// macro's key order follows serde_json's `preserve_order` feature, which
// feature-unification can toggle between builds, changing the emitted
// template text. Struct fields always serialize in declaration order.
// PascalCase names are the AOSS policy document schema.
#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct AossPolicyRule {
    resource_type: &'static str,
    resource: [&'static str; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    permission: Option<&'static [&'static str]>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct AossNetworkPolicy {
    rules: [AossPolicyRule; 2],
    allow_from_public: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct AossDataAccessPolicy {
    description: String,
    rules: [AossPolicyRule; 2],
    principal: Vec<String>,
}

fn policy_json<T: serde::Serialize>(policy: &[&T; 1]) -> String {
    serde_json::to_string(policy).expect("static AOSS policy document serializes")
}

/// Public network policy for the collection and its dashboard. Requests are
/// still gated by IAM (`aoss:APIAccessAll`) plus the data-access policy.
fn network_policy(name: &CfExpression) -> CfExpression {
    let policy = AossNetworkPolicy {
        rules: [
            AossPolicyRule {
                resource_type: "collection",
                resource: ["collection/${Name}"],
                permission: None,
            },
            AossPolicyRule {
                resource_type: "dashboard",
                resource: ["collection/${Name}"],
                permission: None,
            },
        ],
        allow_from_public: true,
    };
    CfExpression::sub_with(policy_json(&[&policy]), [("Name", name.clone())])
}

/// Data-access policy granting collection- and index-level permissions to the
/// service-account roles whose permission profile references
/// `experimental/aws-opensearch/data-access` for this resource. Skipped when
/// no role holds a grant — AOSS rejects policies without principals, and a
/// collection nobody can read doesn't need one.
fn data_access_policy(
    ctx: &EmitContext<'_>,
    search: &AwsOpenSearch,
    name: &CfExpression,
    owners: &[(String, Vec<PermissionSetReference>)],
) -> Option<CfResource> {
    let principal_roles: Vec<&String> = owners
        .iter()
        .filter(|(_role_id, refs)| {
            refs.iter()
                .any(|reference| reference.id() == "experimental/aws-opensearch/data-access")
        })
        .map(|(role_id, _refs)| role_id)
        .collect();
    if principal_roles.is_empty() {
        return None;
    }

    let mut variables: Vec<(String, CfExpression)> = vec![("Name".to_string(), name.clone())];
    let mut principals = Vec::new();
    for (index, role_id) in principal_roles.iter().enumerate() {
        variables.push((
            format!("Principal{index}"),
            CfExpression::get_att((*role_id).clone(), "Arn"),
        ));
        principals.push(format!("${{Principal{index}}}"));
    }

    let policy = AossDataAccessPolicy {
        description: format!("Alien-managed data access for '{}'", search.id()),
        rules: [
            AossPolicyRule {
                resource_type: "collection",
                resource: ["collection/${Name}"],
                permission: Some(&[
                    "aoss:CreateCollectionItems",
                    "aoss:DeleteCollectionItems",
                    "aoss:UpdateCollectionItems",
                    "aoss:DescribeCollectionItems",
                ]),
            },
            AossPolicyRule {
                resource_type: "index",
                resource: ["index/${Name}/*"],
                permission: Some(&[
                    "aoss:CreateIndex",
                    "aoss:DeleteIndex",
                    "aoss:UpdateIndex",
                    "aoss:DescribeIndex",
                    "aoss:ReadDocument",
                    "aoss:WriteDocument",
                ]),
            },
        ],
        principal: principals,
    };

    let logical_id = ctx.name_for(ctx.resource_id)?;
    let mut access = CfResource::new(
        format!("{logical_id}DataAccessPolicy"),
        "AWS::OpenSearchServerless::AccessPolicy".to_string(),
    );
    access.properties.insert("Name".to_string(), name.clone());
    access
        .properties
        .insert("Type".to_string(), CfExpression::from("data"));
    access.properties.insert(
        "Policy".to_string(),
        CfExpression::sub_with(policy_json(&[&policy]), variables),
    );
    Some(access)
}

/// IAM policies attaching the `experimental/aws-opensearch/*` permission sets
/// (in practice `data-access`, i.e. `aoss:APIAccessAll`) to the owning
/// service-account roles, scoped to this collection's ARN. AOSS enforces both
/// layers: IAM must allow `aoss:APIAccessAll` on the collection AND the
/// data-access policy must list the caller as principal.
fn api_access_iam_policies(
    search: &AwsOpenSearch,
    logical_id: &str,
    owners: &[(String, Vec<PermissionSetReference>)],
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", search.id()));

    for (owner_index, (role_id, permission_refs)) in owners.iter().enumerate() {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with(PERMISSION_SET_PREFIX) {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation OpenSearch IAM policy for '{}'",
                        search.id()
                    ),
                })?;
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy did not serialize to a JSON object".to_string(),
                }));
            };
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                continue;
            };
            // The permission set can only scope to `collection/*` (AOSS
            // collection ARNs use the server-assigned collection id, unknown
            // until apply time); pin every statement to this collection's ARN.
            let policy_statements = policy_statements
                .into_iter()
                .map(|statement| pin_statement_to_collection(statement, logical_id))
                .collect::<Vec<_>>();

            let policy_id =
                format!("{logical_id}{role_id}OpenSearchPermission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-opensearch-{owner_index}-{permission_index}",
                    search.id()
                )),
            );
            policy_resource.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list(uniquify_iam_statement_sids(policy_statements)),
                    ),
                ]),
            );
            policy_resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(role_id)]),
            );
            policy_resource.depends_on.push(logical_id.to_string());
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

fn pin_statement_to_collection(statement: CfExpression, logical_id: &str) -> CfExpression {
    let CfExpression::Object(mut statement_object) = statement else {
        return statement;
    };
    statement_object.insert(
        "Resource".to_string(),
        CfExpression::get_att(logical_id, "Arn"),
    );
    CfExpression::Object(statement_object)
}

/// Service-account roles whose permission profile references an
/// `experimental/aws-opensearch/*` permission set for this resource (either
/// directly by resource id or through a `*` wildcard grant).
fn permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if service_account_for_id(ctx, &service_account_id).is_some() {
            if let Some(role_id) = service_account_role_id(ctx, profile_name) {
                owners.push((role_id, refs));
            }
        }
    }
    owners
}

fn resource_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with(PERMISSION_SET_PREFIX))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a ServiceAccount> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()
}
