use alien_permissions::get_permission_set;

struct OperationCoverage<'a> {
    permission_set_id: &'a str,
    aws_actions: &'a [&'a str],
    gcp_permissions: &'a [&'a str],
    azure_actions: &'a [&'a str],
    azure_data_actions: &'a [&'a str],
}

#[test]
fn critical_e2e_provider_operations_are_declared() {
    let cases = [
        OperationCoverage {
            permission_set_id: "worker/provision",
            aws_actions: &[
                "lambda:CreateWorker",
                "iam:PassRole",
                "iam:PutRolePolicy",
                "apigateway:POST",
                "apigateway:TagResource",
                "events:PutRule",
                "events:TagResource",
            ],
            gcp_permissions: &[
                "resourcemanager.projects.get",
                "iam.serviceAccounts.actAs",
                "run.services.create",
                "run.services.get",
                "run.services.update",
                "run.services.delete",
            ],
            azure_actions: &[
                "Microsoft.App/containerApps/write",
                "Microsoft.App/containerApps/delete",
                "Microsoft.App/containerApps/read",
                "Microsoft.App/managedEnvironments/join/action",
                "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
            ],
            azure_data_actions: &[],
        },
        OperationCoverage {
            permission_set_id: "worker/heartbeat",
            aws_actions: &[
                "lambda:GetWorker",
                "lambda:GetWorkerConfiguration",
                "lambda:ListTags",
                "ecr:GetAuthorizationToken",
            ],
            gcp_permissions: &[
                "resourcemanager.projects.get",
                "run.services.get",
                "run.services.getIamPolicy",
            ],
            azure_actions: &["Microsoft.App/containerApps/read"],
            azure_data_actions: &[],
        },
        OperationCoverage {
            permission_set_id: "service-account/provision",
            aws_actions: &[
                "iam:CreateRole",
                "iam:PutRolePolicy",
                "iam:GetRole",
                "iam:TagRole",
            ],
            gcp_permissions: &[
                "iam.serviceAccounts.create",
                "iam.roles.create",
                "resourcemanager.projects.getIamPolicy",
                "resourcemanager.projects.setIamPolicy",
            ],
            azure_actions: &[
                "Microsoft.ManagedIdentity/userAssignedIdentities/write",
                "Microsoft.Authorization/roleDefinitions/write",
                "Microsoft.Authorization/roleAssignments/write",
            ],
            azure_data_actions: &[],
        },
        OperationCoverage {
            permission_set_id: "artifact-registry/provision",
            aws_actions: &[
                "ecr:CreateRepository",
                "ecr:GetRepositoryPolicy",
                "ecr:SetRepositoryPolicy",
            ],
            gcp_permissions: &[
                "artifactregistry.repositories.create",
                "artifactregistry.repositories.getIamPolicy",
                "artifactregistry.repositories.setIamPolicy",
            ],
            azure_actions: &[
                "Microsoft.ContainerRegistry/registries/write",
                "Microsoft.ContainerRegistry/registries/read",
            ],
            azure_data_actions: &[],
        },
        OperationCoverage {
            permission_set_id: "vault/data-write",
            aws_actions: &["ssm:PutParameter", "ssm:DeleteParameter"],
            gcp_permissions: &[
                "secretmanager.secrets.create",
                "secretmanager.versions.add",
                "secretmanager.secrets.delete",
            ],
            azure_actions: &[],
            azure_data_actions: &[
                "Microsoft.KeyVault/vaults/secrets/getSecret/action",
                "Microsoft.KeyVault/vaults/secrets/setSecret/action",
                "Microsoft.KeyVault/vaults/secrets/delete",
            ],
        },
    ];

    let mut failures = Vec::new();
    for case in cases {
        let permission_set = get_permission_set(case.permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {}", case.permission_set_id));
        let aws_actions = permission_set
            .platforms
            .aws
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.actions.as_deref().unwrap_or(&[]))
            .collect::<Vec<_>>();
        let gcp_permissions = permission_set
            .platforms
            .gcp
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.permissions.as_deref().unwrap_or(&[]))
            .collect::<Vec<_>>();
        let azure_actions = permission_set
            .platforms
            .azure
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.actions.as_deref().unwrap_or(&[]))
            .collect::<Vec<_>>();
        let azure_data_actions = permission_set
            .platforms
            .azure
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.data_actions.as_deref().unwrap_or(&[]))
            .collect::<Vec<_>>();

        for action in case.aws_actions {
            if !aws_actions.iter().any(|existing| existing == action) {
                failures.push(format!(
                    "{} is missing AWS action {}",
                    case.permission_set_id, action
                ));
            }
        }
        for permission in case.gcp_permissions {
            if !gcp_permissions
                .iter()
                .any(|existing| existing == permission)
            {
                failures.push(format!(
                    "{} is missing GCP permission {}",
                    case.permission_set_id, permission
                ));
            }
        }
        for action in case.azure_actions {
            if !azure_actions.iter().any(|existing| existing == action) {
                failures.push(format!(
                    "{} is missing Azure action {}",
                    case.permission_set_id, action
                ));
            }
        }
        for data_action in case.azure_data_actions {
            if !azure_data_actions
                .iter()
                .any(|existing| existing == data_action)
            {
                failures.push(format!(
                    "{} is missing Azure dataAction {}",
                    case.permission_set_id, data_action
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "critical provider operation coverage failed:\n{}",
        failures.join("\n")
    );
}
