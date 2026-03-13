use alien_core::{
    AwsBindingSpec, AwsPlatformPermission, AzureBindingSpec, AzurePlatformPermission,
    BindingConfiguration, GcpBindingSpec, GcpCondition, GcpPlatformPermission, PermissionGrant,
    PermissionSet, PlatformPermissions,
};
use alien_permissions::PermissionContext;
use indexmap::IndexMap;

/// Test helper to create a permission context with common variables
pub fn create_test_context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("my-stack")
        .with_resource_name("my-stack-payments-data")
        .with_project_name("my-project")
        .with_region("us-central1")
        .with_subscription_id("00000000-0000-0000-0000-000000000000")
        .with_resource_group("rg-observability-prod")
        .with_storage_account_name("stcxpaymentsprod")
        .with_service_account_name("my-sa")
        .with_principal_id("11111111-2222-3333-4444-555555555555")
        .with_external_id("my-external-id")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
}

/// Test helper to create AWS storage data read permission set
pub fn create_aws_storage_data_read_permission_set() -> PermissionSet {
    PermissionSet {
        id: "storage/data-read".to_string(),
        description: "Allows reading data from storage resources".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![AwsPlatformPermission {
                grant: PermissionGrant {
                    actions: Some(vec![
                        "s3:GetObject".to_string(),
                        "s3:GetObjectVersion".to_string(),
                        "s3:ListBucket".to_string(),
                    ]),
                    permissions: None,
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${stackPrefix}-*".to_string(),
                            "arn:aws:s3:::${stackPrefix}-*/*".to_string(),
                        ],
                        condition: None,
                    }),
                    resource: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${resourceName}".to_string(),
                            "arn:aws:s3:::${resourceName}/*".to_string(),
                        ],
                        condition: None,
                    }),
                },
            }]),
            gcp: None,
            azure: None,
        },
    }
}

/// Test helper to create AWS storage data read permission set with conditions
#[allow(dead_code)]
pub fn create_aws_storage_data_read_permission_set_with_condition() -> PermissionSet {
    let mut condition = IndexMap::new();
    let mut string_equals = IndexMap::new();
    string_equals.insert("sts:ExternalId".to_string(), "${externalId}".to_string());
    condition.insert("StringEquals".to_string(), string_equals);

    PermissionSet {
        id: "storage/data-read".to_string(),
        description: "Allows reading data from storage resources".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![AwsPlatformPermission {
                grant: PermissionGrant {
                    actions: Some(vec![
                        "s3:GetObject".to_string(),
                        "s3:GetObjectVersion".to_string(),
                        "s3:ListBucket".to_string(),
                    ]),
                    permissions: None,
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${stackPrefix}-*".to_string(),
                            "arn:aws:s3:::${stackPrefix}-*/*".to_string(),
                        ],
                        condition: Some(condition),
                    }),
                    resource: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${resourceName}".to_string(),
                            "arn:aws:s3:::${resourceName}/*".to_string(),
                        ],
                        condition: None,
                    }),
                },
            }]),
            gcp: None,
            azure: None,
        },
    }
}

/// Test helper to create GCP storage data read permission set
#[allow(dead_code)]
pub fn create_gcp_storage_data_read_permission_set() -> PermissionSet {
    PermissionSet {
        id: "storage/data-read".to_string(),
        description: "Allows reading data from storage resources".to_string(),
        platforms: PlatformPermissions {
            aws: None,
            gcp: Some(vec![GcpPlatformPermission {
                grant: PermissionGrant {
                    actions: None,
                    permissions: Some(vec![
                        "storage.objects.get".to_string(),
                        "storage.objects.list".to_string(),
                        "storage.buckets.get".to_string(),
                    ]),
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(GcpBindingSpec {
                        scope: "projects/${projectName}".to_string(),
                        condition: Some(GcpCondition {
                            title: "Stack-prefixed only".to_string(),
                            expression:
                                "resource.name.startsWith('projects/_/buckets/${stackPrefix}-')"
                                    .to_string(),
                        }),
                    }),
                    resource: Some(GcpBindingSpec {
                        scope: "projects/_/buckets/${resourceName}".to_string(),
                        condition: None,
                    }),
                },
            }]),
            azure: None,
        },
    }
}

/// Test helper to create Azure storage data read permission set
#[allow(dead_code)]
pub fn create_azure_storage_data_read_permission_set() -> PermissionSet {
    PermissionSet {
        id: "storage/data-read".to_string(),
        description: "Allows reading data from storage resources".to_string(),
        platforms: PlatformPermissions {
            aws: None,
            gcp: None,
            azure: Some(vec![
                AzurePlatformPermission {
                    grant: PermissionGrant {
                        actions: Some(vec!["Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read".to_string()]),
                        permissions: None,
                        data_actions: Some(vec!["Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read".to_string()]),
                    },
                    binding: BindingConfiguration {
                        stack: Some(AzureBindingSpec {
                            scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}".to_string(),
                        }),
                        resource: Some(AzureBindingSpec {
                            scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.Storage/storageAccounts/${storageAccountName}".to_string(),
                        }),
                    },
                }
            ]),
        },
    }
}

/// Test helper to create a permission set that's missing a platform for testing error cases
#[allow(dead_code)]
pub fn create_permission_set_missing_actions() -> PermissionSet {
    PermissionSet {
        id: "test/policy".to_string(),
        description: "Test permission set with missing actions".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![AwsPlatformPermission {
                grant: PermissionGrant {
                    actions: None, // Missing actions for AWS
                    permissions: None,
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(AwsBindingSpec {
                        resources: vec!["arn:aws:s3:::test-bucket".to_string()],
                        condition: None,
                    }),
                    resource: None,
                },
            }]),
            gcp: None,
            azure: None,
        },
    }
}

/// Test helper to create a permission set missing GCP permissions
#[allow(dead_code)]
pub fn create_permission_set_missing_gcp_permissions() -> PermissionSet {
    PermissionSet {
        id: "test/role".to_string(),
        description: "Test permission set with missing GCP permissions".to_string(),
        platforms: PlatformPermissions {
            aws: None,
            gcp: Some(vec![GcpPlatformPermission {
                grant: PermissionGrant {
                    actions: None,
                    permissions: None, // Missing permissions for GCP
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(GcpBindingSpec {
                        scope: "projects/test-project".to_string(),
                        condition: None,
                    }),
                    resource: None,
                },
            }]),
            azure: None,
        },
    }
}

/// Test helper to create a context with missing variables for testing error cases
#[allow(dead_code)]
pub fn create_empty_context() -> PermissionContext {
    PermissionContext::new()
}

/// Test helper to create CloudFormation context with CloudFormation-specific variables
#[allow(dead_code)]
pub fn create_cloudformation_context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("my-stack")
        .with_resource_name("PaymentsDataBucket") // CloudFormation logical ID
        .with_aws_account_id("123456789012") // Managing account ID
        .with_aws_region("us-east-1")
        .with_external_id("my-external-id")
}

/// Test helper to create AWS permission set with CloudFormation-specific resource references
#[allow(dead_code)]
pub fn create_aws_cloudformation_permission_set() -> PermissionSet {
    PermissionSet {
        id: "storage/data-read".to_string(),
        description: "Allows reading data from storage resources".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![AwsPlatformPermission {
                grant: PermissionGrant {
                    actions: Some(vec![
                        "s3:GetObject".to_string(),
                        "s3:GetObjectVersion".to_string(),
                        "s3:ListBucket".to_string(),
                    ]),
                    permissions: None,
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${AWS::StackName}-*".to_string(),
                            "arn:aws:s3:::${AWS::StackName}-*/*".to_string(),
                        ],
                        condition: None,
                    }),
                    resource: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${resourceName}".to_string(),
                            "arn:aws:s3:::${resourceName}/*".to_string(),
                        ],
                        condition: None,
                    }),
                },
            }]),
            gcp: None,
            azure: None,
        },
    }
}

/// Test helper to create AWS permission set with cross-service permissions (e.g., ECR access)
#[allow(dead_code)]
pub fn create_aws_lambda_permission_set() -> PermissionSet {
    let mut condition = IndexMap::new();
    let mut string_equals = IndexMap::new();
    string_equals.insert("sts:ExternalId".to_string(), "${externalId}".to_string());
    condition.insert("StringEquals".to_string(), string_equals);

    PermissionSet {
        id: "function/execute".to_string(),
        description: "Allows executing Lambda functions and pulling container images".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![
                AwsPlatformPermission {
                    grant: PermissionGrant {
                        actions: Some(vec![
                            "lambda:InvokeFunction".to_string(),
                        ]),
                        permissions: None,
                        data_actions: None,
                    },
                    binding: BindingConfiguration {
                        stack: Some(AwsBindingSpec {
                            resources: vec![
                                "arn:aws:lambda:${AWS::Region}:${AWS::AccountId}:function:${AWS::StackName}-*".to_string(),
                            ],
                            condition: None,
                        }),
                        resource: Some(AwsBindingSpec {
                            resources: vec![
                                "arn:aws:lambda:${AWS::Region}:${AWS::AccountId}:function:${AWS::StackName}-${resourceName}".to_string(),
                            ],
                            condition: None,
                        }),
                    },
                },
                // Cross-service permission for ECR
                AwsPlatformPermission {
                    grant: PermissionGrant {
                        actions: Some(vec![
                            "ecr:BatchGetImage".to_string(),
                            "ecr:GetDownloadUrlForLayer".to_string(),
                        ]),
                        permissions: None,
                        data_actions: None,
                    },
                    binding: BindingConfiguration {
                        stack: Some(AwsBindingSpec {
                            resources: vec![
                                "arn:aws:ecr:*:${ManagingAccountId}:repository/*".to_string(),
                            ],
                            condition: Some(condition.clone()),
                        }),
                        resource: None,
                    },
                }
            ]),
            gcp: None,
            azure: None,
        },
    }
}
