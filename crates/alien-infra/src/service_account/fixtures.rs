use alien_core::permissions::{
    AwsBindingSpec, AwsPlatformPermission, AzureBindingSpec, AzurePlatformPermission,
    BindingConfiguration, GcpBindingSpec, GcpPlatformPermission, PermissionGrant,
    PermissionProfile, PermissionSet, PermissionSetReference, PlatformPermissions,
};
use alien_core::ServiceAccount;
use indexmap::IndexMap;

/// Creates a basic ServiceAccount for testing with minimal permissions
pub fn basic_service_account() -> ServiceAccount {
    ServiceAccount::new("test-sa".to_string())
        .stack_permission_set(storage_read_permission_set())
        .build()
}

/// Creates a ServiceAccount with multiple stack-level permissions
pub fn service_account_with_multiple_stack_permissions() -> ServiceAccount {
    ServiceAccount::new("test-sa-with-multiple".to_string())
        .stack_permission_set(storage_read_permission_set())
        .stack_permission_set(storage_write_permission_set())
        .build()
}

/// Creates a ServiceAccount with multiple permission sets
pub fn service_account_with_multiple_permissions() -> ServiceAccount {
    ServiceAccount::new("test-sa-multi".to_string())
        .stack_permission_set(storage_read_permission_set())
        .stack_permission_set(function_execute_permission_set())
        .build()
}

/// Creates a ServiceAccount from a permission profile (tests the conversion)
pub fn service_account_from_profile() -> ServiceAccount {
    let mut permission_profile = PermissionProfile::new();

    permission_profile.0.insert(
        "*".to_string(),
        vec![PermissionSetReference::from_name("storage/data-read")],
    );

    // Note: Resource-scoped permissions like "logs-storage" are handled by
    // individual resource controllers, not by ServiceAccount controllers

    // Mock permission set resolver
    let resolver = |permission_set_id: &str| -> Option<PermissionSet> {
        match permission_set_id {
            "storage/data-read" => Some(storage_read_permission_set()),
            _ => None,
        }
    };

    ServiceAccount::from_permission_profile(
        "test-sa-from-profile".to_string(),
        &permission_profile,
        resolver,
    )
    .unwrap()
}

/// Creates a storage read permission set for testing
pub fn storage_read_permission_set() -> PermissionSet {
    PermissionSet {
        id: "storage/data-read".to_string(),
        description: "Allows reading data from storage buckets and containers".to_string(),
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
                        condition: Some(alien_core::permissions::GcpCondition {
                            title: "Stack-prefixed only".to_string(),
                            expression: "resource.name.startsWith('projects/_/buckets/${stackPrefix}-')".to_string(),
                        }),
                    }),
                    resource: Some(GcpBindingSpec {
                        scope: "projects/_/buckets/${resourceName}".to_string(),
                        condition: None,
                    }),
                },
            }]),
            azure: Some(vec![AzurePlatformPermission {
                grant: PermissionGrant {
                    actions: None,
                    permissions: None,
                    data_actions: Some(vec![
                        "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read".to_string(),
                    ]),
                },
                binding: BindingConfiguration {
                    stack: Some(AzureBindingSpec {
                        scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}".to_string(),
                    }),
                    resource: Some(AzureBindingSpec {
                        scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.Storage/storageAccounts/${storageAccountName}".to_string(),
                    }),
                },
            }]),
        },
    }
}

/// Creates a storage write permission set for testing
pub fn storage_write_permission_set() -> PermissionSet {
    PermissionSet {
        id: "storage/data-write".to_string(),
        description: "Allows writing data to storage buckets and containers".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![AwsPlatformPermission {
                grant: PermissionGrant {
                    actions: Some(vec![
                        "s3:PutObject".to_string(),
                        "s3:PutObjectAcl".to_string(),
                        "s3:DeleteObject".to_string(),
                    ]),
                    permissions: None,
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${stackPrefix}-*/*".to_string(),
                        ],
                        condition: None,
                    }),
                    resource: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:s3:::${resourceName}/*".to_string(),
                        ],
                        condition: None,
                    }),
                },
            }]),
            gcp: Some(vec![GcpPlatformPermission {
                grant: PermissionGrant {
                    actions: None,
                    permissions: Some(vec![
                        "storage.objects.create".to_string(),
                        "storage.objects.delete".to_string(),
                        "storage.objects.update".to_string(),
                    ]),
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(GcpBindingSpec {
                        scope: "projects/${projectName}".to_string(),
                        condition: Some(alien_core::permissions::GcpCondition {
                            title: "Stack-prefixed only".to_string(),
                            expression: "resource.name.startsWith('projects/_/buckets/${stackPrefix}-')".to_string(),
                        }),
                    }),
                    resource: Some(GcpBindingSpec {
                        scope: "projects/_/buckets/${resourceName}".to_string(),
                        condition: None,
                    }),
                },
            }]),
            azure: Some(vec![AzurePlatformPermission {
                grant: PermissionGrant {
                    actions: None,
                    permissions: None,
                    data_actions: Some(vec![
                        "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/write".to_string(),
                        "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/delete".to_string(),
                    ]),
                },
                binding: BindingConfiguration {
                    stack: Some(AzureBindingSpec {
                        scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}".to_string(),
                    }),
                    resource: Some(AzureBindingSpec {
                        scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.Storage/storageAccounts/${storageAccountName}".to_string(),
                    }),
                },
            }]),
        },
    }
}

/// Creates a function execute permission set for testing
pub fn function_execute_permission_set() -> PermissionSet {
    PermissionSet {
        id: "function/execute".to_string(),
        description: "Allows executing functions".to_string(),
        platforms: PlatformPermissions {
            aws: Some(vec![AwsPlatformPermission {
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
                            "arn:aws:lambda:*:*:function:${stackPrefix}-*".to_string(),
                        ],
                        condition: None,
                    }),
                    resource: Some(AwsBindingSpec {
                        resources: vec![
                            "arn:aws:lambda:*:*:function:${resourceName}".to_string(),
                        ],
                        condition: None,
                    }),
                },
            }]),
            gcp: Some(vec![GcpPlatformPermission {
                grant: PermissionGrant {
                    actions: None,
                    permissions: Some(vec![
                        "cloudfunctions.functions.invoke".to_string(),
                    ]),
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(GcpBindingSpec {
                        scope: "projects/${projectName}".to_string(),
                        condition: Some(alien_core::permissions::GcpCondition {
                            title: "Stack-prefixed only".to_string(),
                            expression: "resource.name.startsWith('projects/${projectName}/locations/*/functions/${stackPrefix}-')".to_string(),
                        }),
                    }),
                    resource: Some(GcpBindingSpec {
                        scope: "projects/${projectName}/locations/*/functions/${resourceName}".to_string(),
                        condition: None,
                    }),
                },
            }]),
            azure: Some(vec![AzurePlatformPermission {
                grant: PermissionGrant {
                    actions: Some(vec![
                        "Microsoft.Web/sites/functions/action".to_string(),
                    ]),
                    permissions: None,
                    data_actions: None,
                },
                binding: BindingConfiguration {
                    stack: Some(AzureBindingSpec {
                        scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}".to_string(),
                    }),
                    resource: Some(AzureBindingSpec {
                        scope: "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.Web/sites/${functionAppName}".to_string(),
                    }),
                },
            }]),
        },
    }
}
