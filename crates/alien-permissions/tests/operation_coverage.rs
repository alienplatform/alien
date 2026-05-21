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
                "lambda:CreateFunction",
                "ec2:DescribeSecurityGroups",
                "ec2:DescribeNetworkInterfaces",
                "ec2:DescribeSubnets",
                "ec2:DescribeVpcs",
                "acm:ImportCertificate",
                "acm:AddTagsToCertificate",
                "iam:PassRole",
                "iam:PutRolePolicy",
                "apigateway:POST",
                "apigateway:PUT",
                "apigateway:TagResource",
                "events:PutRule",
                "events:TagResource",
            ],
            gcp_permissions: &[
                "run.services.create",
                "run.services.update",
                "iam.serviceAccounts.actAs",
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
                "lambda:GetFunction",
                "lambda:GetFunctionConfiguration",
                "lambda:ListTags",
                "ecr:GetAuthorizationToken",
            ],
            gcp_permissions: &["run.services.get"],
            azure_actions: &["Microsoft.App/containerApps/read"],
            azure_data_actions: &[],
        },
        OperationCoverage {
            permission_set_id: "worker/execute",
            aws_actions: &[
                "logs:CreateLogGroup",
                "logs:CreateLogStream",
                "logs:PutLogEvents",
                "ec2:CreateNetworkInterface",
                "ec2:DescribeNetworkInterfaces",
                "ec2:DeleteNetworkInterface",
            ],
            gcp_permissions: &["run.services.get", "logging.logEntries.create"],
            azure_actions: &["Microsoft.ContainerRegistry/registries/read"],
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
            gcp_permissions: &["iam.serviceAccounts.create", "iam.roles.create"],
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
                "iam:CreateRole",
                "iam:PutRolePolicy",
                "iam:ListAttachedRolePolicies",
                "iam:DeleteRole",
            ],
            gcp_permissions: &["artifactregistry.repositories.create"],
            azure_actions: &[
                "Microsoft.ContainerRegistry/registries/write",
                "Microsoft.ContainerRegistry/registries/read",
            ],
            azure_data_actions: &[],
        },
        OperationCoverage {
            permission_set_id: "vault/data-write",
            aws_actions: &["ssm:PutParameter", "ssm:DeleteParameter"],
            gcp_permissions: &["secretmanager.versions.add", "secretmanager.secrets.delete"],
            azure_actions: &[],
            azure_data_actions: &[
                "Microsoft.KeyVault/vaults/secrets/readMetadata/action",
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
