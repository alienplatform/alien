use alien_permissions::get_permission_set;

struct OperationCoverage<'a> {
    permission_set_id: &'a str,
    aws_actions: &'a [&'a str],
    gcp_permissions: &'a [&'a str],
    gcp_predefined_roles: &'a [&'a str],
    azure_actions: &'a [&'a str],
    azure_data_actions: &'a [&'a str],
    azure_predefined_roles: &'a [&'a str],
}

#[test]
fn critical_e2e_provider_operations_are_declared() {
    let cases = [
        OperationCoverage {
            // Ops the provision flow exercises against live clouds; the WHY for each grant lives
            // in permission-sets/postgres/provision.jsonc.
            permission_set_id: "postgres/provision",
            aws_actions: &[
                "ec2:CreateSecurityGroup",
                "ec2:CreateTags",
                "ec2:AuthorizeSecurityGroupIngress",
                "rds:CreateDBCluster",
                "rds:ModifyDBCluster",
                "rds:DescribeDBInstances",
                "secretsmanager:CreateSecret",
            ],
            gcp_permissions: &[
                "cloudsql.instances.create",
                "cloudsql.users.create",
                "cloudsql.users.update",
                "cloudsql.databases.create",
                "compute.addresses.create",
                "compute.addresses.createInternal",
                "compute.forwardingRules.pscCreate",
                "servicedirectory.namespaces.create",
                "secretmanager.versions.add",
            ],
            gcp_predefined_roles: &[],
            azure_actions: &[
                "Microsoft.DBforPostgreSQL/flexibleServers/write",
                "Microsoft.DBforPostgreSQL/flexibleServers/read",
                "Microsoft.Network/privateEndpoints/write",
                "Microsoft.Network/privateEndpoints/read",
                "Microsoft.Network/privateDnsZones/virtualNetworkLinks/delete",
                "Microsoft.Network/privateDnsZones/delete",
            ],
            azure_data_actions: &[],
            azure_predefined_roles: &[],
        },
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
            gcp_predefined_roles: &[],
            azure_actions: &[
                "Microsoft.App/containerApps/write",
                "Microsoft.App/containerApps/delete",
                "Microsoft.App/containerApps/read",
                "Microsoft.App/managedEnvironments/join/action",
                "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
            ],
            azure_data_actions: &[],
            azure_predefined_roles: &[],
        },
        OperationCoverage {
            permission_set_id: "worker/heartbeat",
            aws_actions: &["lambda:GetFunctionConfiguration", "lambda:ListTags"],
            gcp_permissions: &[],
            gcp_predefined_roles: &[
                "roles/run.viewer",
                "roles/pubsub.viewer",
                "roles/iam.serviceAccountViewer",
            ],
            azure_actions: &[],
            azure_data_actions: &[],
            azure_predefined_roles: &["Reader"],
        },
        OperationCoverage {
            permission_set_id: "daemon/provision",
            aws_actions: &[
                "elasticloadbalancing:CreateLoadBalancer",
                "elasticloadbalancing:CreateTargetGroup",
                "elasticloadbalancing:CreateListener",
                "elasticloadbalancing:ModifyListener",
                "elasticloadbalancing:DeleteLoadBalancer",
                "elasticloadbalancing:DeleteTargetGroup",
                "elasticloadbalancing:DeleteListener",
                "ec2:CreateSecurityGroup",
                "ec2:CreateTags",
                "ec2:AuthorizeSecurityGroupIngress",
                "ec2:RevokeSecurityGroupIngress",
                "ec2:DeleteSecurityGroup",
                "acm:ImportCertificate",
                "acm:AddTagsToCertificate",
                "acm:DeleteCertificate",
            ],
            gcp_permissions: &[
                "compute.sslCertificates.create",
                "compute.sslCertificates.delete",
                "compute.targetHttpsProxies.setSslCertificates",
                "compute.globalForwardingRules.create",
                "compute.globalForwardingRules.delete",
            ],
            gcp_predefined_roles: &[],
            azure_actions: &[
                "Microsoft.Network/applicationGateways/write",
                "Microsoft.Network/applicationGateways/delete",
                "Microsoft.ManagedIdentity/userAssignedIdentities/write",
                "Microsoft.Authorization/roleAssignments/write",
                "Microsoft.Network/publicIPAddresses/write",
            ],
            azure_data_actions: &[
                "Microsoft.KeyVault/vaults/certificates/import/action",
                "Microsoft.KeyVault/vaults/certificates/delete",
            ],
            azure_predefined_roles: &[],
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
            gcp_permissions: &[],
            gcp_predefined_roles: &[
                "roles/artifactregistry.reader",
                "roles/logging.logWriter",
                "roles/run.viewer",
            ],
            azure_actions: &[],
            azure_data_actions: &[],
            azure_predefined_roles: &["AcrPull", "Reader"],
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
            gcp_predefined_roles: &[],
            azure_actions: &[
                "Microsoft.Authorization/roleDefinitions/write",
                "Microsoft.Authorization/roleAssignments/write",
            ],
            azure_data_actions: &[],
            azure_predefined_roles: &["Managed Identity Contributor"],
        },
        OperationCoverage {
            permission_set_id: "kubernetes-cluster/heartbeat",
            aws_actions: &["eks:DescribeCluster"],
            gcp_permissions: &["container.clusters.get", "container.clusters.list"],
            gcp_predefined_roles: &[],
            azure_actions: &["Microsoft.ContainerService/managedClusters/read"],
            azure_data_actions: &[],
            azure_predefined_roles: &[],
        },
        OperationCoverage {
            permission_set_id: "storage/heartbeat",
            aws_actions: &["s3:GetBucketLocation", "s3:GetEncryptionConfiguration"],
            gcp_permissions: &["storage.buckets.get", "storage.hmacKeys.list"],
            gcp_predefined_roles: &[],
            azure_actions: &[],
            azure_data_actions: &[],
            azure_predefined_roles: &["Reader"],
        },
        OperationCoverage {
            permission_set_id: "storage/trigger-management",
            aws_actions: &["s3:GetBucketNotification", "s3:PutBucketNotification"],
            gcp_permissions: &["storage.buckets.get", "storage.buckets.update"],
            gcp_predefined_roles: &[],
            azure_actions: &[],
            azure_data_actions: &[],
            azure_predefined_roles: &[],
        },
        OperationCoverage {
            permission_set_id: "queue/heartbeat",
            aws_actions: &["sqs:GetQueueAttributes", "sqs:ListQueueTags"],
            gcp_permissions: &[],
            gcp_predefined_roles: &["roles/pubsub.viewer"],
            azure_actions: &[],
            azure_data_actions: &[],
            azure_predefined_roles: &["Reader"],
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
            gcp_predefined_roles: &[],
            azure_actions: &[
                "Microsoft.ContainerRegistry/registries/write",
                "Microsoft.ContainerRegistry/registries/read",
            ],
            azure_data_actions: &[],
            azure_predefined_roles: &[],
        },
        OperationCoverage {
            permission_set_id: "observe/observe",
            aws_actions: &[
                "tag:GetResources",
                "cloudwatch:GetMetricData",
                "cloudwatch:ListMetrics",
            ],
            gcp_permissions: &[
                "cloudasset.assets.searchAllResources",
                "monitoring.timeSeries.list",
                "monitoring.metricDescriptors.list",
            ],
            gcp_predefined_roles: &[],
            azure_actions: &[
                "Microsoft.ResourceGraph/resources/read",
                "Microsoft.Insights/metrics/read",
                "Microsoft.Insights/metricDefinitions/read",
            ],
            azure_data_actions: &[],
            azure_predefined_roles: &[],
        },
        OperationCoverage {
            permission_set_id: "vault/data-write",
            aws_actions: &["ssm:PutParameter", "ssm:DeleteParameter"],
            gcp_permissions: &["secretmanager.secrets.delete"],
            gcp_predefined_roles: &["roles/secretmanager.secretVersionAdder"],
            azure_actions: &[],
            azure_data_actions: &[
                "Microsoft.KeyVault/vaults/secrets/readMetadata/action",
                "Microsoft.KeyVault/vaults/secrets/setSecret/action",
                "Microsoft.KeyVault/vaults/secrets/delete",
            ],
            azure_predefined_roles: &[],
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
            .flat_map(|permission| {
                permission
                    .grant
                    .residual_permissions
                    .as_deref()
                    .or(permission.grant.permissions.as_deref())
                    .unwrap_or(&[])
            })
            .collect::<Vec<_>>();
        let gcp_predefined_roles = permission_set
            .platforms
            .gcp
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.predefined_roles.as_deref().unwrap_or(&[]))
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
        let azure_predefined_roles = permission_set
            .platforms
            .azure
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.predefined_roles.as_deref().unwrap_or(&[]))
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
        for role in case.gcp_predefined_roles {
            if !gcp_predefined_roles.iter().any(|existing| existing == role) {
                failures.push(format!(
                    "{} is missing GCP predefined role {}",
                    case.permission_set_id, role
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
        for role in case.azure_predefined_roles {
            if !azure_predefined_roles
                .iter()
                .any(|existing| existing == role)
            {
                failures.push(format!(
                    "{} is missing Azure predefined role {}",
                    case.permission_set_id, role
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

/// provision/heartbeat generate the connection password and only ever WRITE it (secret + DB
/// principal), so a secret-value READ grant would widen the management identity's data reach for
/// nothing. Pins that no such grant — action or predefined role — appears in either set.
#[test]
fn postgres_provision_and_heartbeat_grant_no_secret_value_read() {
    const FORBIDDEN_AWS: &[&str] = &[
        "secretsmanager:GetSecretValue",
        "secretsmanager:BatchGetSecretValue",
    ];
    const FORBIDDEN_GCP: &[&str] = &["secretmanager.versions.access", "secretmanager.secrets.get"];
    // Role names hide their payload, so secret-reading predefined roles must be pinned too — a
    // `Reader` → `Key Vault Secrets User` swap would otherwise sail through the action checks.
    // (`secrets.get` above is metadata-only; it rides along as defense-in-depth, unlike the Azure
    // matcher below which deliberately skips metadata actions.)
    const FORBIDDEN_GCP_ROLES: &[&str] = &[
        "roles/secretmanager.secretAccessor",
        "roles/secretmanager.admin",
    ];
    const FORBIDDEN_AZURE_ROLES: &[&str] = &[
        "key vault secrets user",
        "key vault secrets officer",
        "key vault administrator",
    ];

    for set_id in [
        "postgres/provision",
        "postgres/heartbeat",
        "postgres/management",
    ] {
        let permission_set =
            get_permission_set(set_id).unwrap_or_else(|| panic!("missing permission set {set_id}"));

        let aws_actions: Vec<&str> = permission_set
            .platforms
            .aws
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.actions.as_deref().unwrap_or(&[]))
            .map(String::as_str)
            .collect();
        for forbidden in FORBIDDEN_AWS {
            assert!(
                !aws_actions.contains(forbidden),
                "{set_id} must not grant AWS secret-value read '{forbidden}'"
            );
        }

        let gcp_permissions: Vec<&str> = permission_set
            .platforms
            .gcp
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| {
                // `permissions` and `residual_permissions` can co-exist; the guard must scan
                // both or a forbidden permission could hide in the unscanned list.
                permission
                    .grant
                    .permissions
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .chain(
                        permission
                            .grant
                            .residual_permissions
                            .as_deref()
                            .unwrap_or(&[]),
                    )
            })
            .map(String::as_str)
            .collect();
        for forbidden in FORBIDDEN_GCP {
            assert!(
                !gcp_permissions.contains(forbidden),
                "{set_id} must not grant GCP secret read '{forbidden}'"
            );
        }

        let gcp_roles: Vec<String> = permission_set
            .platforms
            .gcp
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.predefined_roles.as_deref().unwrap_or(&[]))
            .map(|role| role.to_ascii_lowercase())
            .collect();
        for forbidden in FORBIDDEN_GCP_ROLES {
            assert!(
                !gcp_roles
                    .iter()
                    .any(|role| role == &forbidden.to_ascii_lowercase()),
                "{set_id} must not grant the secret-reading GCP role '{forbidden}'"
            );
        }

        // These sets have no business touching Key Vault secret contents at all, so any
        // `/secrets/` data action is forbidden except the metadata-only reads — this also
        // catches wildcard forms a getSecret/backup denylist would miss.
        let has_azure_secret_read = permission_set
            .platforms
            .azure
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.data_actions.as_deref().unwrap_or(&[]))
            .any(|action| {
                let lower = action.to_ascii_lowercase();
                lower.contains("/secrets/") && !lower.contains("readmetadata")
            });
        assert!(
            !has_azure_secret_read,
            "{set_id} must not grant an Azure Key Vault secrets data action (beyond metadata)"
        );

        let azure_roles: Vec<String> = permission_set
            .platforms
            .azure
            .as_ref()
            .into_iter()
            .flatten()
            .flat_map(|permission| permission.grant.predefined_roles.as_deref().unwrap_or(&[]))
            .map(|role| role.to_ascii_lowercase())
            .collect();
        for forbidden in FORBIDDEN_AZURE_ROLES {
            assert!(
                !azure_roles.iter().any(|role| role == forbidden),
                "{set_id} must not grant the secret-reading Azure role '{forbidden}'"
            );
        }
    }
}
