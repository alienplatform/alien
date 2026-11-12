//! What a management-side permission set must never grant: reading a customer's stored contents
//! (objects, items, secrets, messages, logs, build output) or reaching inside a sandbox session.
//!
//! Alien's own `provision`, `management` and `heartbeat` sets are held to these lists by the
//! sensitive-invariant tests. An extension's sets with those names go to the extension's
//! identity, whose credentials reach the extension's controller, which Alien has not reviewed.
//! [`extension_identity_refusals`] admits only what the extension catalogs list, each reviewed
//! for a third party on the scopes it names, and still checks these lists and more: nothing
//! that grants access or acts as another identity, and the reads Alien's own sets need but a
//! third party's do not.

use alien_core::permissions::{PermissionGrant, PermissionSet};

use crate::registry::{iam_action_pattern_matches, AZURE_SANDBOX_DATA_PLANE_ROLE};
use crate::SENSITIVE_MICROVM_ACTIONS;

/// AWS actions that read stored contents.
pub const AWS_SENSITIVE_CONTENT_ACTIONS: &[&str] = &[
    "s3:GetObject",
    "s3:GetObjectVersion",
    "dynamodb:BatchGetItem",
    "dynamodb:GetItem",
    "dynamodb:Query",
    "dynamodb:Scan",
    "ssm:GetParameter",
    "ssm:GetParameters",
    "ssm:GetParametersByPath",
    "secretsmanager:GetSecretValue",
    "sqs:ReceiveMessage",
    "codebuild:BatchGetBuilds",
    "logs:GetLogEvents",
    "logs:FilterLogEvents",
    "logs:StartQuery",
    "logs:GetQueryResults",
    "dynamodb:PartiQLSelect",
    "dynamodb:GetRecords",
    "ssm:GetParameterHistory",
    "kinesis:GetRecords",
    "kinesis:SubscribeToShard",
    // Each copies contents somewhere the caller chooses, or streams them to the caller.
    "dynamodb:ExportTableToPointInTime",
    "logs:StartLiveTail",
    "logs:CreateExportTask",
    "logs:PutSubscriptionFilter",
];

/// GCP permissions that read stored contents.
pub const GCP_SENSITIVE_CONTENT_PERMISSIONS: &[&str] = &[
    "storage.objects.get",
    "datastore.entities.get",
    "datastore.entities.list",
    "secretmanager.versions.access",
    "pubsub.subscriptions.consume",
    "artifactregistry.repositories.downloadArtifacts",
    "cloudbuild.builds.get",
    "cloudbuild.builds.list",
    "bigquery.tables.getData",
    "bigquery.tables.export",
    "logging.logEntries.list",
    "logging.privateLogEntries.list",
    "logging.views.access",
    "spanner.databases.read",
    "spanner.databases.select",
    "bigtable.tables.readRows",
    // Each copies contents somewhere the caller chooses.
    "cloudsql.instances.export",
    "datastore.databases.export",
    "logging.sinks.create",
    // Runs code and reads files inside a live sandbox session; belongs to sandbox/execute alone.
    "aiplatform.sandboxEnvironments.execute",
];

/// GCP predefined roles that carry a permission on [`GCP_SENSITIVE_CONTENT_PERMISSIONS`].
pub const GCP_SENSITIVE_CONTENT_ROLES: &[&str] = &[
    "roles/storage.admin",
    "roles/storage.objectViewer",
    "roles/storage.objectUser",
    "roles/storage.objectAdmin",
    "roles/datastore.viewer",
    "roles/datastore.user",
    "roles/datastore.owner",
    "roles/datastore.admin",
    "roles/secretmanager.secretAccessor",
    "roles/secretmanager.admin",
    "roles/pubsub.subscriber",
    "roles/pubsub.editor",
    "roles/pubsub.admin",
    "roles/artifactregistry.reader",
    "roles/artifactregistry.writer",
    "roles/artifactregistry.repoAdmin",
    "roles/artifactregistry.admin",
    "roles/cloudbuild.builds.viewer",
    "roles/cloudbuild.builds.editor",
    "roles/cloudbuild.admin",
];

/// Azure control-plane actions that hand over a key or secret.
pub const AZURE_SENSITIVE_CONTENT_ACTIONS: &[&str] = &[
    "Microsoft.Storage/storageAccounts/listKeys/action",
    // A SAS token reads whatever the account or service holds.
    "Microsoft.Storage/storageAccounts/ListAccountSas/action",
    "Microsoft.Storage/storageAccounts/listServiceSas/action",
    // The account API key, which is the whole inference surface. Azure spells it `listkeys` in
    // the role definition and `listKeys` in its docs, which is why every comparison is
    // case-insensitive.
    "Microsoft.CognitiveServices/accounts/listKeys/action",
    "Microsoft.App/containerApps/listSecrets/action",
    "Microsoft.App/managedEnvironments/listSecrets/action",
];

/// Azure data actions that read stored contents or reach inside a sandbox session.
pub const AZURE_SENSITIVE_CONTENT_DATA_ACTIONS: &[&str] = &[
    // Reaching inside a live sandbox session: running a command, or moving its files. Session
    // lifecycle is separate on Azure and is not listed here.
    "Microsoft.App/sandboxGroups/sandboxes/executeCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/executeShellCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/exec/stream/action",
    "Microsoft.App/sandboxGroups/sandboxes/files/read",
    "Microsoft.App/sandboxGroups/sandboxes/files/write",
    "Microsoft.App/sandboxGroups/sandboxes/files/delete",
    "Microsoft.App/sandboxGroups/sandboxes/downloadContentPackage/action",
    // Each turns a live session's filesystem into an object outside it: a disk image another
    // sandbox can boot, or a snapshot.
    "Microsoft.App/sandboxGroups/sandboxes/commit/action",
    "Microsoft.App/sandboxGroups/sandboxes/snapshot/action",
    "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
    "Microsoft.Storage/storageAccounts/tableServices/tables/entities/read",
    "Microsoft.Storage/storageAccounts/queueServices/queues/messages/read",
    "Microsoft.KeyVault/vaults/secrets/read",
    "Microsoft.KeyVault/vaults/secrets/getSecret/action",
    "Microsoft.ServiceBus/namespaces/queues/messages/receive/action",
    "Microsoft.ServiceBus/namespaces/topics/subscriptions/messages/receive/action",
    "Microsoft.ServiceBus/namespaces/queues/messages/peek/action",
    "Microsoft.ServiceBus/namespaces/topics/subscriptions/messages/peek/action",
    "Microsoft.ServiceBus/namespaces/messages/receive/action",
    "Microsoft.Storage/storageAccounts/queueServices/queues/messages/process/action",
    "Microsoft.Storage/storageAccounts/fileServices/fileshares/files/read",
    "Microsoft.EventHub/namespaces/messages/receive/action",
    "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/items/read",
];

/// Azure built-in roles that carry an action on the two lists above.
pub const AZURE_SENSITIVE_CONTENT_ROLES: &[&str] = &[
    "AcrPull",
    "AcrPush",
    // Carries `Microsoft.CognitiveServices/accounts/listkeys/action` and a
    // `Microsoft.CognitiveServices/*` dataAction, so it hands over the account key.
    "Cognitive Services User",
    // Carries the whole sandbox data plane, session contents included, so it belongs only to sets
    // meant to reach inside a session — `sandbox/execute` and `sandbox/remote-execute`. Named
    // from the shared constant so this list and the reach predicate cannot disagree.
    AZURE_SANDBOX_DATA_PLANE_ROLE,
    "Azure Service Bus Data Receiver",
    "Key Vault Secrets User",
    "Storage Blob Data Contributor",
    "Storage Blob Data Reader",
    "Storage Table Data Contributor",
    "Storage Table Data Reader",
];

/// AWS actions an extension's identity may not have, beyond [`AWS_SENSITIVE_CONTENT_ACTIONS`]:
/// reads Alien's own sets need (a function's code or settings, an image's layers, a topic's
/// messages), and actions that grant access, share a resource or run code as another identity.
/// Any action of a service whose name the pattern covers as `iam` but a `Get` or `List`, and of
/// one it covers as `sts` but `GetCallerIdentity`, is refused as well.
pub const AWS_EXTENSION_IDENTITY_REFUSED_ACTIONS: &[&str] = &[
    "lambda:GetFunction",
    "lambda:GetFunctionConfiguration",
    "ecr:BatchGetImage",
    "ecr:GetDownloadUrlForLayer",
    "sns:Subscribe",
    // A resource policy, ACL or share opens the resource to another principal or account.
    "s3:PutBucketPolicy",
    "s3:PutBucketAcl",
    "s3:PutObjectAcl",
    "kms:PutKeyPolicy",
    "kms:CreateGrant",
    "secretsmanager:PutResourcePolicy",
    "sqs:AddPermission",
    "sqs:SetQueueAttributes",
    "lambda:AddPermission",
    "ecr:SetRepositoryPolicy",
    "dynamodb:PutResourcePolicy",
    "ec2:ModifySnapshotAttribute",
    "rds:ModifyDBSnapshotAttribute",
    // Each runs code as the identity of the function, instance, task or build.
    "lambda:UpdateFunctionCode",
    "ssm:SendCommand",
    "ssm:StartSession",
    "ecs:ExecuteCommand",
    "codebuild:StartBuild",
    "iam:PutRolePolicy",
    "iam:AttachRolePolicy",
    "iam:PassRole",
    "iam:CreatePolicyVersion",
    "iam:UpdateAssumeRolePolicy",
    "iam:CreateAccessKey",
    "sts:AssumeRole",
];

/// GCP permissions an extension's identity may not have, beyond
/// [`GCP_SENSITIVE_CONTENT_PERMISSIONS`]: acting as a service account or minting its
/// credentials, and changing who has access. Any permission ending in `.setIamPolicy` is
/// refused as well.
pub const GCP_EXTENSION_IDENTITY_REFUSED_PERMISSIONS: &[&str] = &[
    "iam.serviceAccounts.actAs",
    "iam.serviceAccounts.getAccessToken",
    "iam.serviceAccounts.getOpenIdToken",
    "iam.serviceAccounts.implicitDelegation",
    "iam.serviceAccounts.signBlob",
    "iam.serviceAccounts.signJwt",
    "iam.serviceAccountKeys.create",
    "iam.roles.create",
    "iam.roles.update",
    "resourcemanager.projects.setIamPolicy",
    // A key that signs as a service account.
    "storage.hmacKeys.create",
    // Deployment Manager creates resources as its own service agent.
    "deploymentmanager.deployments.create",
    // Alien's own queue sets need it; it subscribes to a topic's messages.
    "pubsub.topics.attachSubscription",
];

/// Azure actions an extension's identity may not have, beyond [`AZURE_SENSITIVE_CONTENT_ACTIONS`]:
/// changing who has access, running code on a machine, and handing over or rewriting a key or
/// secret. Any action of a namespace the pattern covers as `Microsoft.Authorization` but a read
/// is refused as well.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_ACTIONS: &[&str] = &[
    "Microsoft.Authorization/roleAssignments/write",
    "Microsoft.Authorization/roleDefinitions/write",
    "Microsoft.KeyVault/vaults/accessPolicies/write",
    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
    "Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials/write",
    "Microsoft.Compute/virtualMachines/runCommand/action",
    "Microsoft.Compute/virtualMachines/extensions/write",
    // Rewrites the vault's access policies.
    "Microsoft.KeyVault/vaults/write",
    "Microsoft.Storage/storageAccounts/regeneratekey/action",
    "Microsoft.ServiceBus/namespaces/authorizationRules/listKeys/action",
    "Microsoft.ServiceBus/namespaces/queues/authorizationRules/listKeys/action",
    "Microsoft.ServiceBus/namespaces/topics/authorizationRules/listKeys/action",
    "Microsoft.EventHub/namespaces/authorizationRules/listKeys/action",
    "Microsoft.EventHub/namespaces/eventhubs/authorizationRules/listKeys/action",
    "Microsoft.DocumentDB/databaseAccounts/listKeys/action",
    "Microsoft.DocumentDB/databaseAccounts/listConnectionStrings/action",
    "Microsoft.Cache/redis/listKeys/action",
    "Microsoft.ContainerRegistry/registries/listCredentials/action",
    // An app's settings, which hold its connection strings and keys.
    "Microsoft.Web/sites/config/list/action",
];

/// A name an extension's identity may hold, and the binding values it was reviewed with: AWS
/// resource ARNs, or GCP and Azure scopes. A binding passes when a reviewed value covers it.
pub struct ReviewedGrant {
    pub name: &'static str,
    pub scopes: &'static [&'static str],
}

const ROUTE53_HOSTED_ZONES: &[&str] = &["arn:aws:route53:::hostedzone/*"];

/// The AWS actions an extension's identity may have. Each entry is reviewed for what a third
/// party could do with it on the scopes it names, and added when an extension needs it.
/// Route 53 names a zone by an id it assigns, so no ARN limits these to the stack's own zones:
/// the setup template the customer reviews shows the extension may change any hosted zone.
pub const AWS_EXTENSION_IDENTITY_CATALOG: &[ReviewedGrant] = &[
    // Neither takes a resource.
    ReviewedGrant {
        name: "route53:CreateHostedZone",
        scopes: &["*"],
    },
    ReviewedGrant {
        name: "route53:ListHostedZonesByName",
        scopes: &["*"],
    },
    ReviewedGrant {
        name: "route53:GetChange",
        scopes: &["arn:aws:route53:::change/*"],
    },
    ReviewedGrant {
        name: "route53:GetHostedZone",
        scopes: ROUTE53_HOSTED_ZONES,
    },
    ReviewedGrant {
        name: "route53:DeleteHostedZone",
        scopes: ROUTE53_HOSTED_ZONES,
    },
    ReviewedGrant {
        name: "route53:ChangeResourceRecordSets",
        scopes: ROUTE53_HOSTED_ZONES,
    },
    ReviewedGrant {
        name: "route53:ListResourceRecordSets",
        scopes: ROUTE53_HOSTED_ZONES,
    },
    ReviewedGrant {
        name: "route53:ChangeTagsForResource",
        scopes: ROUTE53_HOSTED_ZONES,
    },
    ReviewedGrant {
        name: "route53:ListTagsForResource",
        scopes: ROUTE53_HOSTED_ZONES,
    },
    // Associating a private zone with a VPC reads the VPC's id; it takes no resource.
    ReviewedGrant {
        name: "ec2:DescribeVpcs",
        scopes: &["*"],
    },
];

/// The GCP permissions an extension's identity may have. None is reviewed yet.
pub const GCP_EXTENSION_IDENTITY_CATALOG: &[ReviewedGrant] = &[];

/// The Azure actions an extension's identity may have. None is reviewed yet.
pub const AZURE_EXTENSION_IDENTITY_CATALOG: &[ReviewedGrant] = &[];

/// The catalog entry for `name`. AWS and Azure match ignoring case, as their policies do; GCP
/// matches exactly. A wildcard matches no entry.
fn reviewed<'c>(catalog: &'c [ReviewedGrant], name: &str) -> Option<&'c ReviewedGrant> {
    if name.contains(['*', '?']) {
        return None;
    }
    catalog
        .iter()
        .find(|grant| grant.name.eq_ignore_ascii_case(name))
}

/// Each grant of `permission_set` an extension's `provision`, `management` or `heartbeat` set
/// may not have, as "<cloud> <kind> <grant>". The set may name only what the extension catalogs
/// above list, by exact name and on a scope the entry was reviewed for. A wildcard, a predefined
/// role, a `NotResource`, a field the cloud does not read and any name not listed is refused.
/// The content, access and identity lists are checked too, so a catalog entry that slipped
/// through review is still refused.
pub fn extension_identity_refusals(permission_set: &PermissionSet) -> Vec<String> {
    let mut found = sensitive_content_grants(permission_set);
    let mut refuse = |label: String| {
        if !found.contains(&label) {
            found.push(label);
        }
    };
    // Every field is named, so a field added to the grant fails to compile here until it is
    // checked.
    let unused = |cloud: &str, grant: &PermissionGrant, refuse: &mut dyn FnMut(String)| {
        let PermissionGrant {
            actions,
            permissions,
            predefined_roles,
            residual_permissions,
            data_actions,
        } = grant;
        let fields: [(&str, &Option<Vec<String>>); 5] = [
            ("action", actions),
            ("permission", permissions),
            ("role", predefined_roles),
            ("residual permission", residual_permissions),
            ("data action", data_actions),
        ];
        let used: &[&str] = match cloud {
            "AWS" => &["action"],
            "GCP" => &["permission", "residual permission", "role"],
            _ => &["action", "data action", "role"],
        };
        for (kind, names) in fields {
            if !used.contains(&kind) {
                for name in names.iter().flatten() {
                    refuse(format!("{cloud} {kind} {name}"));
                }
            }
        }
    };
    // `label` is refused unless `catalog` lists `name` with a scope covering each of `scopes`.
    let check = |catalog: &[ReviewedGrant],
                 name: &str,
                 scopes: &[&str],
                 label: String,
                 refuse: &mut dyn FnMut(String)| {
        let Some(grant) = reviewed(catalog, name) else {
            refuse(label);
            return;
        };
        for scope in scopes {
            if !grant
                .scopes
                .iter()
                .any(|reviewed| glob_covers(reviewed, scope))
            {
                refuse(format!("{label} on {scope}"));
            }
        }
    };
    for entry in permission_set.platforms.aws.iter().flatten() {
        if !entry.effect.is_allow() {
            continue;
        }
        unused("AWS", &entry.grant, &mut refuse);
        let specs = [
            entry.binding.stack.as_ref(),
            entry.binding.resource.as_ref(),
        ];
        let resources: Vec<&str> = specs
            .iter()
            .flatten()
            .flat_map(|spec| spec.resources.iter().map(String::as_str))
            .collect();
        for spec in specs.iter().flatten() {
            for not_resource in &spec.not_resources {
                refuse(format!("AWS NotResource {not_resource}"));
            }
        }
        for pattern in entry.grant.actions.iter().flatten() {
            let lowered = pattern.to_ascii_lowercase();
            // A pattern that can match an `iam:` or `sts:` name, such as `*:PutUserPolicy`, is held
            // to their rules. A verb wildcard is read by what comes before it: `iam:Get*` reads,
            // `iam:Set*` may not.
            let verb = lowered.split_once(':').map_or("*", |(_, verb)| verb);
            let fixed = verb.split('*').next().unwrap_or_default();
            let escalates = (can_match_a_name_starting(&lowered, "iam:")
                && !(fixed.starts_with("get") || fixed.starts_with("list")))
                || (can_match_a_name_starting(&lowered, "sts:") && verb != "getcalleridentity");
            let listed = AWS_EXTENSION_IDENTITY_REFUSED_ACTIONS
                .iter()
                .any(|action| glob_covers(pattern, action));
            let label = format!("AWS action {pattern}");
            if escalates || listed {
                refuse(label);
            } else {
                check(
                    AWS_EXTENSION_IDENTITY_CATALOG,
                    pattern,
                    &resources,
                    label,
                    &mut refuse,
                );
            }
        }
    }
    for entry in permission_set.platforms.gcp.iter().flatten() {
        unused("GCP", &entry.grant, &mut refuse);
        for role in entry.grant.predefined_roles.iter().flatten() {
            refuse(format!("GCP role {role}"));
        }
        let scopes: Vec<&str> = [
            entry.binding.stack.as_ref(),
            entry.binding.resource.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|spec| spec.scope.as_str())
        .collect();
        // The generator grants residual permissions through a predefined role when one covers
        // them, so they are held to the same catalog.
        let named = [
            ("permission", &entry.grant.permissions),
            ("residual permission", &entry.grant.residual_permissions),
        ];
        for (kind, permissions) in named {
            for permission in permissions.iter().flatten() {
                let label = format!("GCP {kind} {permission}");
                let refused = permission.to_ascii_lowercase().ends_with(".setiampolicy")
                    || GCP_EXTENSION_IDENTITY_REFUSED_PERMISSIONS
                        .iter()
                        .any(|listed| glob_covers(permission, listed));
                if refused {
                    refuse(label);
                } else {
                    check(
                        GCP_EXTENSION_IDENTITY_CATALOG,
                        permission,
                        &scopes,
                        label,
                        &mut refuse,
                    );
                }
            }
        }
    }
    for entry in permission_set.platforms.azure.iter().flatten() {
        unused("Azure", &entry.grant, &mut refuse);
        for role in entry.grant.predefined_roles.iter().flatten() {
            refuse(format!("Azure role {role}"));
        }
        let scopes: Vec<&str> = [
            entry.binding.stack.as_ref(),
            entry.binding.resource.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|spec| spec.scope.as_str())
        .collect();
        for action in entry.grant.actions.iter().flatten() {
            let lowered = action.to_ascii_lowercase();
            let authorization = can_match_a_name_starting(&lowered, "microsoft.authorization/")
                && !lowered.ends_with("/read");
            let refused = authorization
                || AZURE_EXTENSION_IDENTITY_REFUSED_ACTIONS
                    .iter()
                    .any(|listed| glob_covers(action, listed));
            let label = format!("Azure action {action}");
            if refused {
                refuse(label);
            } else {
                check(
                    AZURE_EXTENSION_IDENTITY_CATALOG,
                    action,
                    &scopes,
                    label,
                    &mut refuse,
                );
            }
        }
        for data_action in entry.grant.data_actions.iter().flatten() {
            refuse(format!("Azure data action {data_action}"));
        }
    }
    found
}

/// Each grant of `permission_set` that reaches a customer's stored contents, as
/// "<cloud> <kind> <grant>". A wildcard counts when it covers a listed action, so `s3:Get*` or
/// `*` is reported like `s3:GetObject`.
pub fn sensitive_content_grants(permission_set: &PermissionSet) -> Vec<String> {
    let mut found = Vec::new();
    let aws_listed = || {
        AWS_SENSITIVE_CONTENT_ACTIONS
            .iter()
            .chain(SENSITIVE_MICROVM_ACTIONS)
    };
    for entry in permission_set.platforms.aws.iter().flatten() {
        if !entry.effect.is_allow() {
            continue;
        }
        for pattern in entry.grant.actions.iter().flatten() {
            let lowered = pattern.to_ascii_lowercase();
            if aws_listed().any(|action| {
                iam_action_pattern_matches(
                    lowered.as_bytes(),
                    action.to_ascii_lowercase().as_bytes(),
                )
            }) {
                found.push(format!("AWS action {pattern}"));
            }
        }
    }
    for entry in permission_set.platforms.gcp.iter().flatten() {
        for role in entry.grant.predefined_roles.iter().flatten() {
            if GCP_SENSITIVE_CONTENT_ROLES.contains(&role.as_str()) {
                found.push(format!("GCP role {role}"));
            }
        }
        let permissions = entry.grant.permissions.iter().flatten();
        let residual = entry.grant.residual_permissions.iter().flatten();
        for permission in permissions.chain(residual) {
            if GCP_SENSITIVE_CONTENT_PERMISSIONS
                .iter()
                .any(|listed| glob_covers(permission, listed))
            {
                found.push(format!("GCP permission {permission}"));
            }
        }
    }
    for entry in permission_set.platforms.azure.iter().flatten() {
        for role in entry.grant.predefined_roles.iter().flatten() {
            if AZURE_SENSITIVE_CONTENT_ROLES
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(role))
            {
                found.push(format!("Azure role {role}"));
            }
        }
        for action in entry.grant.actions.iter().flatten() {
            if AZURE_SENSITIVE_CONTENT_ACTIONS
                .iter()
                .any(|listed| glob_covers(action, listed))
            {
                found.push(format!("Azure action {action}"));
            }
        }
        for data_action in entry.grant.data_actions.iter().flatten() {
            if AZURE_SENSITIVE_CONTENT_DATA_ACTIONS
                .iter()
                .any(|listed| glob_covers(data_action, listed))
            {
                found.push(format!("Azure data action {data_action}"));
            }
        }
    }
    found
}

/// Whether `pattern`, lowercase with `*` and `?` wildcards, matches some name that starts with
/// `prefix`.
fn can_match_a_name_starting(pattern: &str, prefix: &str) -> bool {
    fn reaches(pattern: &[u8], prefix: &[u8]) -> bool {
        match (pattern.first(), prefix.first()) {
            (_, None) | (Some(b'*'), _) => true,
            (Some(b'?'), Some(_)) => reaches(&pattern[1..], &prefix[1..]),
            (Some(expected), Some(actual)) if expected == actual => {
                reaches(&pattern[1..], &prefix[1..])
            }
            _ => false,
        }
    }
    reaches(pattern.as_bytes(), prefix.as_bytes())
}

/// Whether `pattern`, with `*` wildcards, covers `name`, ignoring case.
fn glob_covers(pattern: &str, name: &str) -> bool {
    iam_action_pattern_matches(
        pattern.to_ascii_lowercase().as_bytes(),
        name.to_ascii_lowercase().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(platforms: serde_json::Value) -> PermissionSet {
        serde_json::from_value(serde_json::json!({
            "id": "acme/dns-zone/provision", "description": "d", "platforms": platforms
        }))
        .unwrap()
    }

    /// One entry granting `grant` on `cloud`, bound to the whole stack.
    fn grant(cloud: &str, grant: serde_json::Value) -> serde_json::Value {
        let binding = match cloud {
            "aws" => serde_json::json!({ "resources": ["*"] }),
            _ => serde_json::json!({ "scope": "${stackPrefix}" }),
        };
        serde_json::json!([{ "grant": grant, "binding": { "stack": binding } }])
    }

    #[test]
    fn a_grant_that_reads_contents_is_found_on_each_cloud_wildcards_included() {
        for (platforms, expected) in [
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["s3:getobject"] })) }),
                "AWS action s3:getobject",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["s3:Get*"] })) }),
                "AWS action s3:Get*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["*"] })) }),
                "AWS action *",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/storage.objectViewer"] })) }),
                "GCP role roles/storage.objectViewer",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "permissions": ["secretmanager.versions.access"] })) }),
                "GCP permission secretmanager.versions.access",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "dataActions": ["Microsoft.Storage/storageAccounts/blobServices/*"] })) }),
                "Azure data action Microsoft.Storage/storageAccounts/blobServices/*",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Storage/storageAccounts/listkeys/action"] })) }),
                "Azure action Microsoft.Storage/storageAccounts/listkeys/action",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["storage blob data reader"] })) }),
                "Azure role storage blob data reader",
            ),
        ] {
            assert_eq!(sensitive_content_grants(&set(platforms)), [expected]);
        }
    }

    #[test]
    fn an_extension_identity_gets_no_predefined_role_access_change_or_extra_read() {
        for (platforms, expected) in [
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/editor"] })) }),
                "GCP role roles/editor",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "permissions": ["iam.serviceAccounts.actAs"] })) }),
                "GCP permission iam.serviceAccounts.actAs",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "permissions": ["storage.buckets.setIamPolicy"] })) }),
                "GCP permission storage.buckets.setIamPolicy",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Key Vault Contributor"] })) }),
                "Azure role Key Vault Contributor",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/roleAssignments/write"] })) }),
                "Azure action Microsoft.Authorization/roleAssignments/write",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/*"] })) }),
                "Azure action Microsoft.Authorization/*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:PassRole"] })) }),
                "AWS action iam:PassRole",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:*"] })) }),
                "AWS action iam:*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:DeleteRolePermissionsBoundary"] })) }),
                "AWS action iam:DeleteRolePermissionsBoundary",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["sts:AssumeRole"] })) }),
                "AWS action sts:AssumeRole",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:Set*"] })) }),
                "AWS action iam:Set*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["sts:Get*"] })) }),
                "AWS action sts:Get*",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/locks/*"] })) }),
                "Azure action Microsoft.Authorization/locks/*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["ecr:BatchGetImage"] })) }),
                "AWS action ecr:BatchGetImage",
            ),
        ] {
            assert_eq!(extension_identity_refusals(&set(platforms)), [expected]);
        }
        // Reads the shared lists gained, for an extension and for Alien's own sets alike.
        for (platforms, expected) in [
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["dynamodb:PartiQLSelect"] })) }),
                "AWS action dynamodb:PartiQLSelect",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "dataActions": ["Microsoft.KeyVault/vaults/secrets/getSecret/action"] })) }),
                "Azure data action Microsoft.KeyVault/vaults/secrets/getSecret/action",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Storage/storageAccounts/ListAccountSas/action"] })) }),
                "Azure action Microsoft.Storage/storageAccounts/ListAccountSas/action",
            ),
        ] {
            assert_eq!(
                sensitive_content_grants(&set(platforms.clone())),
                [expected]
            );
            assert_eq!(extension_identity_refusals(&set(platforms)), [expected]);
        }
        // `*` is reported once, as a read.
        assert_eq!(
            extension_identity_refusals(&set(
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["*"] })) })
            )),
            ["AWS action *"]
        );
    }

    /// `(cloud, kind, grant)` reported by [`extension_identity_refusals`] as one refusal.
    fn refused_alone(cloud: &str, kind: &str, name: &str) -> Vec<String> {
        let field = match kind {
            "action" => "actions",
            "permission" => "permissions",
            _ => "dataActions",
        };
        let platforms =
            serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
        extension_identity_refusals(&set(platforms))
    }

    fn label(cloud: &str, kind: &str, name: &str) -> String {
        let cloud = match cloud {
            "aws" => "AWS",
            "gcp" => "GCP",
            _ => "Azure",
        };
        let kind = if kind == "data" { "data action" } else { kind };
        format!("{cloud} {kind} {name}")
    }

    #[test]
    fn an_extension_identity_shares_no_resource_with_another_principal() {
        for action in [
            "s3:PutBucketPolicy",
            "s3:PutBucketAcl",
            "s3:PutObjectAcl",
            "kms:PutKeyPolicy",
            "kms:CreateGrant",
            "secretsmanager:PutResourcePolicy",
            "sqs:AddPermission",
            "sqs:SetQueueAttributes",
            "lambda:AddPermission",
            "ecr:SetRepositoryPolicy",
            "dynamodb:PutResourcePolicy",
            "ec2:ModifySnapshotAttribute",
            "rds:ModifyDBSnapshotAttribute",
            "s3:PutBucket*",
        ] {
            assert_eq!(
                refused_alone("aws", "action", action),
                [label("aws", "action", action)]
            );
        }
    }

    #[test]
    fn an_extension_identity_runs_no_code_as_another_identity() {
        for (cloud, kind, name) in [
            ("aws", "action", "lambda:UpdateFunctionCode"),
            ("aws", "action", "ssm:SendCommand"),
            ("aws", "action", "ssm:StartSession"),
            ("aws", "action", "ecs:ExecuteCommand"),
            ("aws", "action", "codebuild:StartBuild"),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachines/runCommand/action",
            ),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachines/extensions/write",
            ),
            ("gcp", "permission", "storage.hmacKeys.create"),
            ("gcp", "permission", "deploymentmanager.deployments.create"),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
    }

    #[test]
    fn an_extension_identity_gets_no_azure_key_and_rewrites_no_vault() {
        for action in [
            "Microsoft.KeyVault/vaults/write",
            "Microsoft.Storage/storageAccounts/regenerateKey/action",
            "Microsoft.ServiceBus/namespaces/authorizationRules/listKeys/action",
            "Microsoft.ServiceBus/namespaces/queues/authorizationRules/listKeys/action",
            "Microsoft.EventHub/namespaces/authorizationRules/listKeys/action",
            "Microsoft.EventHub/namespaces/eventhubs/authorizationRules/listKeys/action",
            "Microsoft.DocumentDB/databaseAccounts/listKeys/action",
            "Microsoft.DocumentDB/databaseAccounts/listConnectionStrings/action",
            "Microsoft.Cache/redis/listKeys/action",
            "Microsoft.ContainerRegistry/registries/listCredentials/action",
            "Microsoft.Web/sites/config/list/action",
            "Microsoft.EventHub/*",
        ] {
            assert_eq!(
                refused_alone("azure", "action", action),
                [label("azure", "action", action)]
            );
            // Alien's own sets may hold them.
            let platforms = serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": [action] })) });
            assert!(
                sensitive_content_grants(&set(platforms)).is_empty(),
                "{action}"
            );
        }
    }

    #[test]
    fn content_reads_are_refused_for_every_management_set() {
        for (cloud, kind, name) in [
            ("aws", "action", "dynamodb:ExportTableToPointInTime"),
            ("aws", "action", "logs:StartLiveTail"),
            ("aws", "action", "logs:CreateExportTask"),
            ("aws", "action", "logs:PutSubscriptionFilter"),
            ("aws", "action", "kinesis:SubscribeToShard"),
            ("gcp", "permission", "spanner.databases.read"),
            ("gcp", "permission", "spanner.databases.select"),
            ("gcp", "permission", "bigtable.tables.readRows"),
            ("gcp", "permission", "cloudsql.instances.export"),
            ("gcp", "permission", "datastore.databases.export"),
            ("gcp", "permission", "logging.views.access"),
            ("gcp", "permission", "logging.sinks.create"),
            (
                "azure",
                "data",
                "Microsoft.ServiceBus/namespaces/messages/receive/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.Storage/storageAccounts/queueServices/queues/messages/process/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.Storage/storageAccounts/fileServices/fileshares/files/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.EventHub/namespaces/messages/receive/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/items/read",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
            let field = match kind {
                "action" => "actions",
                "permission" => "permissions",
                _ => "dataActions",
            };
            let platforms =
                serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
            assert_eq!(
                sensitive_content_grants(&set(platforms)),
                [label(cloud, kind, name)]
            );
        }
        // Reads Alien's own sets need are refused for an extension alone.
        for (cloud, kind, name) in [
            ("aws", "action", "sns:Subscribe"),
            ("aws", "action", "lambda:GetFunctionConfiguration"),
            ("gcp", "permission", "pubsub.topics.attachSubscription"),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
            let field = if kind == "action" {
                "actions"
            } else {
                "permissions"
            };
            let platforms =
                serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
            assert!(
                sensitive_content_grants(&set(platforms)).is_empty(),
                "{name}"
            );
        }
    }

    /// The service or namespace is a pattern too: one that covers `iam`, `sts` or
    /// `Microsoft.Authorization` is held to their rules.
    #[test]
    fn a_wildcard_service_is_held_to_the_access_rules() {
        for (cloud, name) in [
            ("aws", "*:PutUserPolicy"),
            ("aws", "i?m:PutRolePolicy"),
            ("aws", "*:AssumeRole"),
            ("aws", "s?s:TagSession"),
            ("aws", "*PutUserPolicy"),
            ("azure", "*/locks/write"),
            ("azure", "*/policyAssignments/write"),
            ("azure", "Microsoft.Auth*ments/write"),
        ] {
            assert_eq!(
                refused_alone(cloud, "action", name),
                [label(cloud, "action", name)]
            );
        }
    }

    #[test]
    fn a_grant_that_only_manages_is_not_found() {
        let platforms = serde_json::json!({
            "aws": grant("aws", serde_json::json!({ "actions": ["route53:CreateHostedZone", "s3:PutBucketTagging"] })),
            "gcp": grant("gcp", serde_json::json!({ "permissions": ["storage.buckets.create"] })),
            "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Storage/storageAccounts/write"] })),
        });
        assert!(sensitive_content_grants(&set(platforms)).is_empty());
        let zone = serde_json::json!({
            "aws": grant("aws", serde_json::json!({ "actions": ["route53:CreateHostedZone"] })),
        });
        assert!(extension_identity_refusals(&set(zone)).is_empty());
    }

    /// A name the extension catalog does not list is refused, however harmless it looks, and
    /// so is one Alien's own sets use: those were reviewed for Alien's identity, not a third
    /// party's.
    #[test]
    fn an_extension_identity_gets_only_listed_permissions() {
        for (cloud, kind, name) in [
            ("aws", "action", "gamelift:CreateFleet"),
            ("aws", "action", "route53:CreateTrafficPolicy"),
            ("gcp", "permission", "dns.managedZones.create"),
            ("azure", "action", "Microsoft.Network/dnszones/write"),
            // Each is in an Alien set, and lets its holder read data or run code as another
            // identity: a disk's download URL, a VM's startup script, a database's password.
            ("aws", "action", "s3:PutBucketTagging"),
            ("aws", "action", "rds:ModifyDBInstance"),
            ("aws", "action", "lambda:UpdateFunctionConfiguration"),
            ("gcp", "permission", "compute.instances.setMetadata"),
            ("gcp", "permission", "cloudbuild.builds.create"),
            ("gcp", "permission", "storage.buckets.create"),
            (
                "azure",
                "action",
                "Microsoft.Compute/disks/beginGetAccess/action",
            ),
            ("azure", "action", "Microsoft.Storage/storageAccounts/write"),
            // Alien's own sets use these data actions; an extension's identity gets none.
            (
                "azure",
                "data",
                "Microsoft.App/sandboxGroups/sandboxes/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.ServiceBus/namespaces/messages/send/action",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
    }

    /// A wildcard is refused even when every name it covers today is listed: the service can
    /// add an action the pattern then grants.
    #[test]
    fn a_wildcard_is_refused_even_over_listed_names() {
        for (cloud, kind, name) in [
            ("aws", "action", "route53:Change*"),
            ("aws", "action", "ec2:DescribeVpc?"),
            ("aws", "action", "iam:List*"),
            ("gcp", "permission", "storage.buckets.*"),
            ("azure", "action", "*/read"),
            ("azure", "action", "Microsoft.Authorization/*/read"),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
    }

    /// A field the cloud's generator does not read is refused rather than skipped.
    #[test]
    fn a_grant_field_the_cloud_does_not_use_is_refused() {
        for (cloud, field, name, expected) in [
            (
                "aws",
                "permissions",
                "s3:PutBucketTagging",
                "AWS permission s3:PutBucketTagging",
            ),
            (
                "aws",
                "residualPermissions",
                "s3:PutBucketTagging",
                "AWS residual permission s3:PutBucketTagging",
            ),
            (
                "aws",
                "dataActions",
                "s3:PutBucketTagging",
                "AWS data action s3:PutBucketTagging",
            ),
            (
                "aws",
                "predefinedRoles",
                "ReadOnlyAccess",
                "AWS role ReadOnlyAccess",
            ),
            (
                "gcp",
                "actions",
                "storage.buckets.create",
                "GCP action storage.buckets.create",
            ),
            (
                "gcp",
                "dataActions",
                "storage.buckets.create",
                "GCP data action storage.buckets.create",
            ),
            (
                "azure",
                "permissions",
                "Microsoft.Storage/storageAccounts/write",
                "Azure permission Microsoft.Storage/storageAccounts/write",
            ),
            (
                "azure",
                "residualPermissions",
                "Microsoft.Storage/storageAccounts/write",
                "Azure residual permission Microsoft.Storage/storageAccounts/write",
            ),
        ] {
            let platforms =
                serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
            assert_eq!(
                extension_identity_refusals(&set(platforms)),
                [expected],
                "{cloud} {field}"
            );
        }
    }

    /// The published example and the SDK's test guest, which the docs copy, are accepted.
    #[test]
    fn the_dns_zone_example_sets_are_accepted() {
        for source in [
            include_str!(
                "../../../examples/extensions/dns-zone/permissions/dns-zone/provision.jsonc"
            ),
            include_str!(
                "../../alien-extension-sdk/tests/guests/acme/permissions/dns-zone/provision.jsonc"
            ),
        ] {
            let set: PermissionSet = json5::from_str(source).unwrap();
            assert_eq!(
                extension_identity_refusals(&set),
                Vec::<String>::new(),
                "{}",
                set.id
            );
        }
    }

    /// Nothing the extension catalog lists is refused by the rules on the scopes it names, so
    /// the catalog never claims a grant the check then drops.
    #[test]
    fn the_extension_catalog_lists_only_names_the_rules_accept() {
        for reviewed in AWS_EXTENSION_IDENTITY_CATALOG {
            let platforms = serde_json::json!({ "aws": [{
                "grant": { "actions": [reviewed.name] },
                "binding": { "stack": { "resources": reviewed.scopes } },
            }] });
            assert!(
                extension_identity_refusals(&set(platforms)).is_empty(),
                "{}",
                reviewed.name
            );
        }
    }

    /// A listed name bound beyond the scope it was reviewed for, or with a `NotResource`,
    /// is refused.
    #[test]
    fn a_binding_beyond_the_reviewed_scope_is_refused() {
        let bound = |binding: serde_json::Value| {
            extension_identity_refusals(&set(serde_json::json!({ "aws": [{
                "grant": { "actions": ["route53:ChangeResourceRecordSets"] },
                "binding": binding,
            }] })))
        };
        assert_eq!(
            bound(serde_json::json!({ "stack": { "resources": ["*"] } })),
            ["AWS action route53:ChangeResourceRecordSets on *"]
        );
        assert_eq!(
            bound(serde_json::json!({
                "stack": { "resources": ["arn:aws:route53:::hostedzone/*"] },
                "resource": { "resources": ["arn:aws:route53:::trafficpolicy/*"] },
            })),
            ["AWS action route53:ChangeResourceRecordSets on arn:aws:route53:::trafficpolicy/*"]
        );
        assert_eq!(
            bound(serde_json::json!({ "stack": {
                "resources": ["*"],
                "notResources": ["arn:aws:route53:::hostedzone/Z1"],
            } })),
            [
                "AWS NotResource arn:aws:route53:::hostedzone/Z1",
                "AWS action route53:ChangeResourceRecordSets on *",
            ]
        );
        assert!(bound(serde_json::json!({
            "stack": { "resources": ["arn:aws:route53:::hostedzone/*"] },
        }))
        .is_empty());
    }
}
