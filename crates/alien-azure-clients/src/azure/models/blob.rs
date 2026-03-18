/// Error types.
pub mod error {
    /// Error from a `TryFrom` or `FromStr` implementation.
    pub struct ConversionError(::std::borrow::Cow<'static, str>);
    impl ::std::error::Error for ConversionError {}
    impl ::std::fmt::Display for ConversionError {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Display::fmt(&self.0, f)
        }
    }
    impl ::std::fmt::Debug for ConversionError {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Debug::fmt(&self.0, f)
        }
    }
    impl From<&'static str> for ConversionError {
        fn from(value: &'static str) -> Self {
            Self(value.into())
        }
    }
    impl From<String> for ConversionError {
        fn from(value: String) -> Self {
            Self(value.into())
        }
    }
}
///The resource model definition for an Azure Resource Manager resource with an etag.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Entity Resource",
///  "description": "The resource model definition for an Azure Resource Manager resource with an etag.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "Resource Etag.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-client-name": "AzureEntityResource"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AzureEntityResource {
    ///Resource Etag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&AzureEntityResource> for AzureEntityResource {
    fn from(value: &AzureEntityResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AzureEntityResource {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            name: Default::default(),
            type_: Default::default(),
        }
    }
}
///Properties of the blob container, including Id, resource name, resource type, Etag.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the blob container, including Id, resource name, resource type, Etag.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/AzureEntityResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ContainerProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobContainer {
    ///Resource Etag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<ContainerProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&BlobContainer> for BlobContainer {
    fn from(value: &BlobContainer) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobContainer {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///`BlobServiceItems`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "properties": {
///    "value": {
///      "description": "List of blob services returned.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/BlobServiceProperties"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobServiceItems {
    ///List of blob services returned.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<BlobServiceProperties>,
}
impl ::std::convert::From<&BlobServiceItems> for BlobServiceItems {
    fn from(value: &BlobServiceItems) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobServiceItems {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///The properties of a storage account’s Blob service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a storage account’s Blob service.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "The properties of a storage account’s Blob service.",
///      "properties": {
///        "automaticSnapshotPolicyEnabled": {
///          "description": "Deprecated in favor of isVersioningEnabled property.",
///          "type": "boolean"
///        },
///        "changeFeed": {
///          "$ref": "#/components/schemas/ChangeFeed"
///        },
///        "containerDeleteRetentionPolicy": {
///          "$ref": "#/components/schemas/DeleteRetentionPolicy"
///        },
///        "cors": {
///          "$ref": "#/components/schemas/CorsRules"
///        },
///        "defaultServiceVersion": {
///          "description": "DefaultServiceVersion indicates the default version to use for requests to the Blob service if an incoming request’s version is not specified. Possible values include version 2008-10-27 and all more recent versions.",
///          "type": "string"
///        },
///        "deleteRetentionPolicy": {
///          "$ref": "#/components/schemas/DeleteRetentionPolicy"
///        },
///        "isVersioningEnabled": {
///          "description": "Versioning is enabled if set to true.",
///          "type": "boolean"
///        },
///        "lastAccessTimeTrackingPolicy": {
///          "$ref": "#/components/schemas/LastAccessTimeTrackingPolicy"
///        },
///        "restorePolicy": {
///          "$ref": "#/components/schemas/RestorePolicyProperties"
///        }
///      },
///      "x-ms-client-flatten": true,
///      "x-ms-client-name": "BlobServiceProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobServiceProperties {
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<BlobServicePropertiesProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&BlobServiceProperties> for BlobServiceProperties {
    fn from(value: &BlobServiceProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobServiceProperties {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            sku: Default::default(),
            type_: Default::default(),
        }
    }
}
///The properties of a storage account’s Blob service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a storage account’s Blob service.",
///  "properties": {
///    "automaticSnapshotPolicyEnabled": {
///      "description": "Deprecated in favor of isVersioningEnabled property.",
///      "type": "boolean"
///    },
///    "changeFeed": {
///      "$ref": "#/components/schemas/ChangeFeed"
///    },
///    "containerDeleteRetentionPolicy": {
///      "$ref": "#/components/schemas/DeleteRetentionPolicy"
///    },
///    "cors": {
///      "$ref": "#/components/schemas/CorsRules"
///    },
///    "defaultServiceVersion": {
///      "description": "DefaultServiceVersion indicates the default version to use for requests to the Blob service if an incoming request’s version is not specified. Possible values include version 2008-10-27 and all more recent versions.",
///      "type": "string"
///    },
///    "deleteRetentionPolicy": {
///      "$ref": "#/components/schemas/DeleteRetentionPolicy"
///    },
///    "isVersioningEnabled": {
///      "description": "Versioning is enabled if set to true.",
///      "type": "boolean"
///    },
///    "lastAccessTimeTrackingPolicy": {
///      "$ref": "#/components/schemas/LastAccessTimeTrackingPolicy"
///    },
///    "restorePolicy": {
///      "$ref": "#/components/schemas/RestorePolicyProperties"
///    }
///  },
///  "x-ms-client-flatten": true,
///  "x-ms-client-name": "BlobServiceProperties"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobServicePropertiesProperties {
    ///Deprecated in favor of isVersioningEnabled property.
    #[serde(
        rename = "automaticSnapshotPolicyEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub automatic_snapshot_policy_enabled: ::std::option::Option<bool>,
    #[serde(
        rename = "changeFeed",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub change_feed: ::std::option::Option<ChangeFeed>,
    #[serde(
        rename = "containerDeleteRetentionPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub container_delete_retention_policy: ::std::option::Option<DeleteRetentionPolicy>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cors: ::std::option::Option<CorsRules>,
    ///DefaultServiceVersion indicates the default version to use for requests to the Blob service if an incoming request’s version is not specified. Possible values include version 2008-10-27 and all more recent versions.
    #[serde(
        rename = "defaultServiceVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_service_version: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "deleteRetentionPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delete_retention_policy: ::std::option::Option<DeleteRetentionPolicy>,
    ///Versioning is enabled if set to true.
    #[serde(
        rename = "isVersioningEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_versioning_enabled: ::std::option::Option<bool>,
    #[serde(
        rename = "lastAccessTimeTrackingPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_access_time_tracking_policy: ::std::option::Option<LastAccessTimeTrackingPolicy>,
    #[serde(
        rename = "restorePolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub restore_policy: ::std::option::Option<RestorePolicyProperties>,
}
impl ::std::convert::From<&BlobServicePropertiesProperties> for BlobServicePropertiesProperties {
    fn from(value: &BlobServicePropertiesProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobServicePropertiesProperties {
    fn default() -> Self {
        Self {
            automatic_snapshot_policy_enabled: Default::default(),
            change_feed: Default::default(),
            container_delete_retention_policy: Default::default(),
            cors: Default::default(),
            default_service_version: Default::default(),
            delete_retention_policy: Default::default(),
            is_versioning_enabled: Default::default(),
            last_access_time_tracking_policy: Default::default(),
            restore_policy: Default::default(),
        }
    }
}
///The blob service properties for change feed events.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The blob service properties for change feed events.",
///  "properties": {
///    "enabled": {
///      "description": "Indicates whether change feed event logging is enabled for the Blob service.",
///      "type": "boolean"
///    },
///    "retentionInDays": {
///      "description": "Indicates the duration of changeFeed retention in days. Minimum value is 1 day and maximum value is 146000 days (400 years). A null value indicates an infinite retention of the change feed.",
///      "type": "integer",
///      "format": "int32",
///      "maximum": 146000.0,
///      "minimum": 1.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ChangeFeed {
    ///Indicates whether change feed event logging is enabled for the Blob service.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///Indicates the duration of changeFeed retention in days. Minimum value is 1 day and maximum value is 146000 days (400 years). A null value indicates an infinite retention of the change feed.
    #[serde(
        rename = "retentionInDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub retention_in_days: ::std::option::Option<::std::num::NonZeroU32>,
}
impl ::std::convert::From<&ChangeFeed> for ChangeFeed {
    fn from(value: &ChangeFeed) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ChangeFeed {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
            retention_in_days: Default::default(),
        }
    }
}
///An error response from the Storage service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the Storage service.",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/CloudErrorBody"
///    }
///  },
///  "x-ms-external": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CloudError {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<CloudErrorBody>,
}
impl ::std::convert::From<&CloudError> for CloudError {
    fn from(value: &CloudError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CloudError {
    fn default() -> Self {
        Self {
            error: Default::default(),
        }
    }
}
///An error response from the Storage service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the Storage service.",
///  "properties": {
///    "code": {
///      "description": "An identifier for the error. Codes are invariant and are intended to be consumed programmatically.",
///      "type": "string"
///    },
///    "details": {
///      "description": "A list of additional details about the error.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CloudErrorBody"
///      }
///    },
///    "message": {
///      "description": "A message describing the error, intended to be suitable for display in a user interface.",
///      "type": "string"
///    },
///    "target": {
///      "description": "The target of the particular error. For example, the name of the property in error.",
///      "type": "string"
///    }
///  },
///  "x-ms-external": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CloudErrorBody {
    ///An identifier for the error. Codes are invariant and are intended to be consumed programmatically.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///A list of additional details about the error.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub details: ::std::vec::Vec<CloudErrorBody>,
    ///A message describing the error, intended to be suitable for display in a user interface.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///The target of the particular error. For example, the name of the property in error.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CloudErrorBody> for CloudErrorBody {
    fn from(value: &CloudErrorBody) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CloudErrorBody {
    fn default() -> Self {
        Self {
            code: Default::default(),
            details: Default::default(),
            message: Default::default(),
            target: Default::default(),
        }
    }
}
///The properties of a container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a container.",
///  "properties": {
///    "defaultEncryptionScope": {
///      "description": "Default the container to use specified encryption scope for all writes.",
///      "type": "string"
///    },
///    "deleted": {
///      "description": "Indicates whether the blob container was deleted.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "deletedTime": {
///      "description": "Blob container deletion time.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "denyEncryptionScopeOverride": {
///      "description": "Block override of encryption scope from the container default.",
///      "type": "boolean"
///    },
///    "enableNfsV3AllSquash": {
///      "description": "Enable NFSv3 all squash on blob container.",
///      "type": "boolean"
///    },
///    "enableNfsV3RootSquash": {
///      "description": "Enable NFSv3 root squash on blob container.",
///      "type": "boolean"
///    },
///    "hasImmutabilityPolicy": {
///      "description": "The hasImmutabilityPolicy public property is set to true by SRP if ImmutabilityPolicy has been created for this container. The hasImmutabilityPolicy public property is set to false by SRP if ImmutabilityPolicy has not been created for this container.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "hasLegalHold": {
///      "description": "The hasLegalHold public property is set to true by SRP if there are at least one existing tag. The hasLegalHold public property is set to false by SRP if all existing legal hold tags are cleared out. There can be a maximum of 1000 blob containers with hasLegalHold=true for a given account.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "immutabilityPolicy": {
///      "$ref": "#/components/schemas/ImmutabilityPolicyProperties"
///    },
///    "immutableStorageWithVersioning": {
///      "$ref": "#/components/schemas/ImmutableStorageWithVersioning"
///    },
///    "lastModifiedTime": {
///      "description": "Returns the date and time the container was last modified.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "leaseDuration": {
///      "description": "Specifies whether the lease on a container is of infinite or fixed duration, only when the container is leased.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Infinite",
///        "Fixed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LeaseDuration"
///      }
///    },
///    "leaseState": {
///      "description": "Lease state of the container.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Available",
///        "Leased",
///        "Expired",
///        "Breaking",
///        "Broken"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LeaseState"
///      }
///    },
///    "leaseStatus": {
///      "description": "The lease status of the container.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Locked",
///        "Unlocked"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LeaseStatus"
///      }
///    },
///    "legalHold": {
///      "$ref": "#/components/schemas/LegalHoldProperties"
///    },
///    "metadata": {
///      "description": "A name-value pair to associate with the container as metadata.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "publicAccess": {
///      "description": "Specifies whether data in the container may be accessed publicly and the level of access.",
///      "type": "string",
///      "enum": [
///        "Container",
///        "Blob",
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "PublicAccess"
///      }
///    },
///    "remainingRetentionDays": {
///      "description": "Remaining retention days for soft deleted blob container.",
///      "readOnly": true,
///      "type": "integer"
///    },
///    "version": {
///      "description": "The version of the deleted blob container.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerProperties {
    ///Default the container to use specified encryption scope for all writes.
    #[serde(
        rename = "defaultEncryptionScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_encryption_scope: ::std::option::Option<::std::string::String>,
    ///Indicates whether the blob container was deleted.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deleted: ::std::option::Option<bool>,
    ///Blob container deletion time.
    #[serde(
        rename = "deletedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deleted_time: ::std::option::Option<::std::string::String>,
    ///Block override of encryption scope from the container default.
    #[serde(
        rename = "denyEncryptionScopeOverride",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deny_encryption_scope_override: ::std::option::Option<bool>,
    ///Enable NFSv3 all squash on blob container.
    #[serde(
        rename = "enableNfsV3AllSquash",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_nfs_v3_all_squash: ::std::option::Option<bool>,
    ///Enable NFSv3 root squash on blob container.
    #[serde(
        rename = "enableNfsV3RootSquash",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_nfs_v3_root_squash: ::std::option::Option<bool>,
    ///The hasImmutabilityPolicy public property is set to true by SRP if ImmutabilityPolicy has been created for this container. The hasImmutabilityPolicy public property is set to false by SRP if ImmutabilityPolicy has not been created for this container.
    #[serde(
        rename = "hasImmutabilityPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_immutability_policy: ::std::option::Option<bool>,
    ///The hasLegalHold public property is set to true by SRP if there are at least one existing tag. The hasLegalHold public property is set to false by SRP if all existing legal hold tags are cleared out. There can be a maximum of 1000 blob containers with hasLegalHold=true for a given account.
    #[serde(
        rename = "hasLegalHold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_legal_hold: ::std::option::Option<bool>,
    #[serde(
        rename = "immutabilityPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutability_policy: ::std::option::Option<ImmutabilityPolicyProperties>,
    #[serde(
        rename = "immutableStorageWithVersioning",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutable_storage_with_versioning: ::std::option::Option<ImmutableStorageWithVersioning>,
    ///Returns the date and time the container was last modified.
    #[serde(
        rename = "lastModifiedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_time: ::std::option::Option<::std::string::String>,
    ///Specifies whether the lease on a container is of infinite or fixed duration, only when the container is leased.
    #[serde(
        rename = "leaseDuration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_duration: ::std::option::Option<ContainerPropertiesLeaseDuration>,
    ///Lease state of the container.
    #[serde(
        rename = "leaseState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_state: ::std::option::Option<ContainerPropertiesLeaseState>,
    ///The lease status of the container.
    #[serde(
        rename = "leaseStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_status: ::std::option::Option<ContainerPropertiesLeaseStatus>,
    #[serde(
        rename = "legalHold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub legal_hold: ::std::option::Option<LegalHoldProperties>,
    ///A name-value pair to associate with the container as metadata.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Specifies whether data in the container may be accessed publicly and the level of access.
    #[serde(
        rename = "publicAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_access: ::std::option::Option<ContainerPropertiesPublicAccess>,
    ///Remaining retention days for soft deleted blob container.
    #[serde(
        rename = "remainingRetentionDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub remaining_retention_days: ::std::option::Option<i64>,
    ///The version of the deleted blob container.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ContainerProperties> for ContainerProperties {
    fn from(value: &ContainerProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ContainerProperties {
    fn default() -> Self {
        Self {
            default_encryption_scope: Default::default(),
            deleted: Default::default(),
            deleted_time: Default::default(),
            deny_encryption_scope_override: Default::default(),
            enable_nfs_v3_all_squash: Default::default(),
            enable_nfs_v3_root_squash: Default::default(),
            has_immutability_policy: Default::default(),
            has_legal_hold: Default::default(),
            immutability_policy: Default::default(),
            immutable_storage_with_versioning: Default::default(),
            last_modified_time: Default::default(),
            lease_duration: Default::default(),
            lease_state: Default::default(),
            lease_status: Default::default(),
            legal_hold: Default::default(),
            metadata: Default::default(),
            public_access: Default::default(),
            remaining_retention_days: Default::default(),
            version: Default::default(),
        }
    }
}
///Specifies whether the lease on a container is of infinite or fixed duration, only when the container is leased.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies whether the lease on a container is of infinite or fixed duration, only when the container is leased.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Infinite",
///    "Fixed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LeaseDuration"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum ContainerPropertiesLeaseDuration {
    Infinite,
    Fixed,
}
impl ::std::convert::From<&Self> for ContainerPropertiesLeaseDuration {
    fn from(value: &ContainerPropertiesLeaseDuration) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerPropertiesLeaseDuration {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Infinite => f.write_str("Infinite"),
            Self::Fixed => f.write_str("Fixed"),
        }
    }
}
impl ::std::str::FromStr for ContainerPropertiesLeaseDuration {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "infinite" => Ok(Self::Infinite),
            "fixed" => Ok(Self::Fixed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerPropertiesLeaseDuration {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContainerPropertiesLeaseDuration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContainerPropertiesLeaseDuration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Lease state of the container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Lease state of the container.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Available",
///    "Leased",
///    "Expired",
///    "Breaking",
///    "Broken"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LeaseState"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum ContainerPropertiesLeaseState {
    Available,
    Leased,
    Expired,
    Breaking,
    Broken,
}
impl ::std::convert::From<&Self> for ContainerPropertiesLeaseState {
    fn from(value: &ContainerPropertiesLeaseState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerPropertiesLeaseState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Available => f.write_str("Available"),
            Self::Leased => f.write_str("Leased"),
            Self::Expired => f.write_str("Expired"),
            Self::Breaking => f.write_str("Breaking"),
            Self::Broken => f.write_str("Broken"),
        }
    }
}
impl ::std::str::FromStr for ContainerPropertiesLeaseState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "available" => Ok(Self::Available),
            "leased" => Ok(Self::Leased),
            "expired" => Ok(Self::Expired),
            "breaking" => Ok(Self::Breaking),
            "broken" => Ok(Self::Broken),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerPropertiesLeaseState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContainerPropertiesLeaseState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContainerPropertiesLeaseState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The lease status of the container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The lease status of the container.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Locked",
///    "Unlocked"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LeaseStatus"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum ContainerPropertiesLeaseStatus {
    Locked,
    Unlocked,
}
impl ::std::convert::From<&Self> for ContainerPropertiesLeaseStatus {
    fn from(value: &ContainerPropertiesLeaseStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerPropertiesLeaseStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Locked => f.write_str("Locked"),
            Self::Unlocked => f.write_str("Unlocked"),
        }
    }
}
impl ::std::str::FromStr for ContainerPropertiesLeaseStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "locked" => Ok(Self::Locked),
            "unlocked" => Ok(Self::Unlocked),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerPropertiesLeaseStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContainerPropertiesLeaseStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContainerPropertiesLeaseStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Specifies whether data in the container may be accessed publicly and the level of access.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies whether data in the container may be accessed publicly and the level of access.",
///  "type": "string",
///  "enum": [
///    "Container",
///    "Blob",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "PublicAccess"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum ContainerPropertiesPublicAccess {
    Container,
    Blob,
    None,
}
impl ::std::convert::From<&Self> for ContainerPropertiesPublicAccess {
    fn from(value: &ContainerPropertiesPublicAccess) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerPropertiesPublicAccess {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Container => f.write_str("Container"),
            Self::Blob => f.write_str("Blob"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for ContainerPropertiesPublicAccess {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "container" => Ok(Self::Container),
            "blob" => Ok(Self::Blob),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerPropertiesPublicAccess {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContainerPropertiesPublicAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContainerPropertiesPublicAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Specifies a CORS rule for the Blob service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies a CORS rule for the Blob service.",
///  "required": [
///    "allowedHeaders",
///    "allowedMethods",
///    "allowedOrigins",
///    "exposedHeaders",
///    "maxAgeInSeconds"
///  ],
///  "properties": {
///    "allowedHeaders": {
///      "description": "Required if CorsRule element is present. A list of headers allowed to be part of the cross-origin request.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "allowedMethods": {
///      "description": "Required if CorsRule element is present. A list of HTTP methods that are allowed to be executed by the origin.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "DELETE",
///          "GET",
///          "HEAD",
///          "MERGE",
///          "POST",
///          "OPTIONS",
///          "PUT",
///          "PATCH",
///          "CONNECT",
///          "TRACE"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "AllowedMethods"
///        }
///      }
///    },
///    "allowedOrigins": {
///      "description": "Required if CorsRule element is present. A list of origin domains that will be allowed via CORS, or \"*\" to allow all domains",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "exposedHeaders": {
///      "description": "Required if CorsRule element is present. A list of response headers to expose to CORS clients.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "maxAgeInSeconds": {
///      "description": "Required if CorsRule element is present. The number of seconds that the client/browser should cache a preflight response.",
///      "type": "integer"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CorsRule {
    ///Required if CorsRule element is present. A list of headers allowed to be part of the cross-origin request.
    #[serde(rename = "allowedHeaders")]
    pub allowed_headers: ::std::vec::Vec<::std::string::String>,
    ///Required if CorsRule element is present. A list of HTTP methods that are allowed to be executed by the origin.
    #[serde(rename = "allowedMethods")]
    pub allowed_methods: ::std::vec::Vec<CorsRuleAllowedMethodsItem>,
    ///Required if CorsRule element is present. A list of origin domains that will be allowed via CORS, or "*" to allow all domains
    #[serde(rename = "allowedOrigins")]
    pub allowed_origins: ::std::vec::Vec<::std::string::String>,
    ///Required if CorsRule element is present. A list of response headers to expose to CORS clients.
    #[serde(rename = "exposedHeaders")]
    pub exposed_headers: ::std::vec::Vec<::std::string::String>,
    ///Required if CorsRule element is present. The number of seconds that the client/browser should cache a preflight response.
    #[serde(rename = "maxAgeInSeconds")]
    pub max_age_in_seconds: i64,
}
impl ::std::convert::From<&CorsRule> for CorsRule {
    fn from(value: &CorsRule) -> Self {
        value.clone()
    }
}
///`CorsRuleAllowedMethodsItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "DELETE",
///    "GET",
///    "HEAD",
///    "MERGE",
///    "POST",
///    "OPTIONS",
///    "PUT",
///    "PATCH",
///    "CONNECT",
///    "TRACE"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AllowedMethods"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum CorsRuleAllowedMethodsItem {
    #[serde(rename = "DELETE")]
    Delete,
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "HEAD")]
    Head,
    #[serde(rename = "MERGE")]
    Merge,
    #[serde(rename = "POST")]
    Post,
    #[serde(rename = "OPTIONS")]
    Options,
    #[serde(rename = "PUT")]
    Put,
    #[serde(rename = "PATCH")]
    Patch,
    #[serde(rename = "CONNECT")]
    Connect,
    #[serde(rename = "TRACE")]
    Trace,
}
impl ::std::convert::From<&Self> for CorsRuleAllowedMethodsItem {
    fn from(value: &CorsRuleAllowedMethodsItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CorsRuleAllowedMethodsItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Delete => f.write_str("DELETE"),
            Self::Get => f.write_str("GET"),
            Self::Head => f.write_str("HEAD"),
            Self::Merge => f.write_str("MERGE"),
            Self::Post => f.write_str("POST"),
            Self::Options => f.write_str("OPTIONS"),
            Self::Put => f.write_str("PUT"),
            Self::Patch => f.write_str("PATCH"),
            Self::Connect => f.write_str("CONNECT"),
            Self::Trace => f.write_str("TRACE"),
        }
    }
}
impl ::std::str::FromStr for CorsRuleAllowedMethodsItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "delete" => Ok(Self::Delete),
            "get" => Ok(Self::Get),
            "head" => Ok(Self::Head),
            "merge" => Ok(Self::Merge),
            "post" => Ok(Self::Post),
            "options" => Ok(Self::Options),
            "put" => Ok(Self::Put),
            "patch" => Ok(Self::Patch),
            "connect" => Ok(Self::Connect),
            "trace" => Ok(Self::Trace),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CorsRuleAllowedMethodsItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CorsRuleAllowedMethodsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CorsRuleAllowedMethodsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Sets the CORS rules. You can include up to five CorsRule elements in the request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sets the CORS rules. You can include up to five CorsRule elements in the request. ",
///  "properties": {
///    "corsRules": {
///      "description": "The List of CORS rules. You can include up to five CorsRule elements in the request. ",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CorsRule"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CorsRules {
    ///The List of CORS rules. You can include up to five CorsRule elements in the request.
    #[serde(
        rename = "corsRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cors_rules: ::std::vec::Vec<CorsRule>,
}
impl ::std::convert::From<&CorsRules> for CorsRules {
    fn from(value: &CorsRules) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CorsRules {
    fn default() -> Self {
        Self {
            cors_rules: Default::default(),
        }
    }
}
///The service properties for soft delete.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The service properties for soft delete.",
///  "properties": {
///    "allowPermanentDelete": {
///      "description": "This property when set to true allows deletion of the soft deleted blob versions and snapshots. This property cannot be used blob restore policy. This property only applies to blob service and does not apply to containers or file share.",
///      "type": "boolean"
///    },
///    "days": {
///      "description": "Indicates the number of days that the deleted item should be retained. The minimum specified value can be 1 and the maximum value can be 365.",
///      "type": "integer",
///      "maximum": 365.0,
///      "minimum": 1.0
///    },
///    "enabled": {
///      "description": "Indicates whether DeleteRetentionPolicy is enabled.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeleteRetentionPolicy {
    ///This property when set to true allows deletion of the soft deleted blob versions and snapshots. This property cannot be used blob restore policy. This property only applies to blob service and does not apply to containers or file share.
    #[serde(
        rename = "allowPermanentDelete",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_permanent_delete: ::std::option::Option<bool>,
    ///Indicates the number of days that the deleted item should be retained. The minimum specified value can be 1 and the maximum value can be 365.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days: ::std::option::Option<::std::num::NonZeroU64>,
    ///Indicates whether DeleteRetentionPolicy is enabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<&DeleteRetentionPolicy> for DeleteRetentionPolicy {
    fn from(value: &DeleteRetentionPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeleteRetentionPolicy {
    fn default() -> Self {
        Self {
            allow_permanent_delete: Default::default(),
            days: Default::default(),
            enabled: Default::default(),
        }
    }
}
///The ImmutabilityPolicy property of a blob container, including Id, resource name, resource type, Etag.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The ImmutabilityPolicy property of a blob container, including Id, resource name, resource type, Etag.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/AzureEntityResource"
///    }
///  ],
///  "required": [
///    "properties"
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ImmutabilityPolicyProperty"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImmutabilityPolicy {
    ///Resource Etag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    pub properties: ImmutabilityPolicyProperty,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ImmutabilityPolicy> for ImmutabilityPolicy {
    fn from(value: &ImmutabilityPolicy) -> Self {
        value.clone()
    }
}
///The properties of an ImmutabilityPolicy of a blob container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of an ImmutabilityPolicy of a blob container.",
///  "properties": {
///    "etag": {
///      "description": "ImmutabilityPolicy Etag.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ImmutabilityPolicyProperty"
///    },
///    "updateHistory": {
///      "description": "The ImmutabilityPolicy update history of the blob container.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/UpdateHistoryProperty"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImmutabilityPolicyProperties {
    ///ImmutabilityPolicy Etag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<ImmutabilityPolicyProperty>,
    ///The ImmutabilityPolicy update history of the blob container.
    #[serde(
        rename = "updateHistory",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub update_history: ::std::vec::Vec<UpdateHistoryProperty>,
}
impl ::std::convert::From<&ImmutabilityPolicyProperties> for ImmutabilityPolicyProperties {
    fn from(value: &ImmutabilityPolicyProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ImmutabilityPolicyProperties {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            properties: Default::default(),
            update_history: Default::default(),
        }
    }
}
///The properties of an ImmutabilityPolicy of a blob container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of an ImmutabilityPolicy of a blob container.",
///  "properties": {
///    "allowProtectedAppendWrites": {
///      "description": "This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to an append blob while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API.",
///      "type": "boolean"
///    },
///    "allowProtectedAppendWritesAll": {
///      "description": "This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API. The 'allowProtectedAppendWrites' and 'allowProtectedAppendWritesAll' properties are mutually exclusive.",
///      "type": "boolean"
///    },
///    "immutabilityPeriodSinceCreationInDays": {
///      "description": "The immutability period for the blobs in the container since the policy creation, in days.",
///      "type": "integer"
///    },
///    "state": {
///      "description": "The ImmutabilityPolicy state of a blob container, possible values include: Locked and Unlocked.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Locked",
///        "Unlocked"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ImmutabilityPolicyState"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImmutabilityPolicyProperty {
    ///This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to an append blob while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API.
    #[serde(
        rename = "allowProtectedAppendWrites",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes: ::std::option::Option<bool>,
    ///This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API. The 'allowProtectedAppendWrites' and 'allowProtectedAppendWritesAll' properties are mutually exclusive.
    #[serde(
        rename = "allowProtectedAppendWritesAll",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes_all: ::std::option::Option<bool>,
    ///The immutability period for the blobs in the container since the policy creation, in days.
    #[serde(
        rename = "immutabilityPeriodSinceCreationInDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutability_period_since_creation_in_days: ::std::option::Option<i64>,
    ///The ImmutabilityPolicy state of a blob container, possible values include: Locked and Unlocked.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub state: ::std::option::Option<ImmutabilityPolicyPropertyState>,
}
impl ::std::convert::From<&ImmutabilityPolicyProperty> for ImmutabilityPolicyProperty {
    fn from(value: &ImmutabilityPolicyProperty) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ImmutabilityPolicyProperty {
    fn default() -> Self {
        Self {
            allow_protected_append_writes: Default::default(),
            allow_protected_append_writes_all: Default::default(),
            immutability_period_since_creation_in_days: Default::default(),
            state: Default::default(),
        }
    }
}
///The ImmutabilityPolicy state of a blob container, possible values include: Locked and Unlocked.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The ImmutabilityPolicy state of a blob container, possible values include: Locked and Unlocked.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Locked",
///    "Unlocked"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ImmutabilityPolicyState"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum ImmutabilityPolicyPropertyState {
    Locked,
    Unlocked,
}
impl ::std::convert::From<&Self> for ImmutabilityPolicyPropertyState {
    fn from(value: &ImmutabilityPolicyPropertyState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ImmutabilityPolicyPropertyState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Locked => f.write_str("Locked"),
            Self::Unlocked => f.write_str("Unlocked"),
        }
    }
}
impl ::std::str::FromStr for ImmutabilityPolicyPropertyState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "locked" => Ok(Self::Locked),
            "unlocked" => Ok(Self::Unlocked),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ImmutabilityPolicyPropertyState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ImmutabilityPolicyPropertyState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ImmutabilityPolicyPropertyState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Object level immutability properties of the container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Object level immutability properties of the container.",
///  "properties": {
///    "enabled": {
///      "description": "This is an immutable property, when set to true it enables object level immutability at the container level.",
///      "type": "boolean"
///    },
///    "migrationState": {
///      "description": "This property denotes the container level immutability to object level immutability migration state.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "InProgress",
///        "Completed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "MigrationState"
///      }
///    },
///    "timeStamp": {
///      "description": "Returns the date and time the object level immutability was enabled.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImmutableStorageWithVersioning {
    ///This is an immutable property, when set to true it enables object level immutability at the container level.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///This property denotes the container level immutability to object level immutability migration state.
    #[serde(
        rename = "migrationState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub migration_state: ::std::option::Option<ImmutableStorageWithVersioningMigrationState>,
    ///Returns the date and time the object level immutability was enabled.
    #[serde(
        rename = "timeStamp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub time_stamp: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ImmutableStorageWithVersioning> for ImmutableStorageWithVersioning {
    fn from(value: &ImmutableStorageWithVersioning) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ImmutableStorageWithVersioning {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
            migration_state: Default::default(),
            time_stamp: Default::default(),
        }
    }
}
///This property denotes the container level immutability to object level immutability migration state.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This property denotes the container level immutability to object level immutability migration state.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "InProgress",
///    "Completed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "MigrationState"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum ImmutableStorageWithVersioningMigrationState {
    InProgress,
    Completed,
}
impl ::std::convert::From<&Self> for ImmutableStorageWithVersioningMigrationState {
    fn from(value: &ImmutableStorageWithVersioningMigrationState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ImmutableStorageWithVersioningMigrationState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::InProgress => f.write_str("InProgress"),
            Self::Completed => f.write_str("Completed"),
        }
    }
}
impl ::std::str::FromStr for ImmutableStorageWithVersioningMigrationState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "inprogress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ImmutableStorageWithVersioningMigrationState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for ImmutableStorageWithVersioningMigrationState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for ImmutableStorageWithVersioningMigrationState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The blob service properties for Last access time based tracking policy.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The blob service properties for Last access time based tracking policy.",
///  "required": [
///    "enable"
///  ],
///  "properties": {
///    "blobType": {
///      "description": "An array of predefined supported blob types. Only blockBlob is the supported value. This field is currently read only",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "enable": {
///      "description": "When set to true last access time based tracking is enabled.",
///      "type": "boolean"
///    },
///    "name": {
///      "description": "Name of the policy. The valid value is AccessTimeTracking. This field is currently read only",
///      "type": "string",
///      "enum": [
///        "AccessTimeTracking"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "name"
///      }
///    },
///    "trackingGranularityInDays": {
///      "description": "The field specifies blob object tracking granularity in days, typically how often the blob object should be tracked.This field is currently read only with value as 1",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LastAccessTimeTrackingPolicy {
    ///An array of predefined supported blob types. Only blockBlob is the supported value. This field is currently read only
    #[serde(
        rename = "blobType",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob_type: ::std::vec::Vec<::std::string::String>,
    ///When set to true last access time based tracking is enabled.
    pub enable: bool,
    ///Name of the policy. The valid value is AccessTimeTracking. This field is currently read only
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<LastAccessTimeTrackingPolicyName>,
    ///The field specifies blob object tracking granularity in days, typically how often the blob object should be tracked.This field is currently read only with value as 1
    #[serde(
        rename = "trackingGranularityInDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tracking_granularity_in_days: ::std::option::Option<i32>,
}
impl ::std::convert::From<&LastAccessTimeTrackingPolicy> for LastAccessTimeTrackingPolicy {
    fn from(value: &LastAccessTimeTrackingPolicy) -> Self {
        value.clone()
    }
}
///Name of the policy. The valid value is AccessTimeTracking. This field is currently read only
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Name of the policy. The valid value is AccessTimeTracking. This field is currently read only",
///  "type": "string",
///  "enum": [
///    "AccessTimeTracking"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "name"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum LastAccessTimeTrackingPolicyName {
    AccessTimeTracking,
}
impl ::std::convert::From<&Self> for LastAccessTimeTrackingPolicyName {
    fn from(value: &LastAccessTimeTrackingPolicyName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for LastAccessTimeTrackingPolicyName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AccessTimeTracking => f.write_str("AccessTimeTracking"),
        }
    }
}
impl ::std::str::FromStr for LastAccessTimeTrackingPolicyName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "accesstimetracking" => Ok(Self::AccessTimeTracking),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LastAccessTimeTrackingPolicyName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LastAccessTimeTrackingPolicyName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LastAccessTimeTrackingPolicyName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Lease Container request schema.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Lease Container request schema.",
///  "required": [
///    "action"
///  ],
///  "properties": {
///    "action": {
///      "description": "Specifies the lease action. Can be one of the available actions.",
///      "type": "string",
///      "enum": [
///        "Acquire",
///        "Renew",
///        "Change",
///        "Release",
///        "Break"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LeaseContainerRequestAction"
///      }
///    },
///    "breakPeriod": {
///      "description": "Optional. For a break action, proposed duration the lease should continue before it is broken, in seconds, between 0 and 60.",
///      "type": "integer"
///    },
///    "leaseDuration": {
///      "description": "Required for acquire. Specifies the duration of the lease, in seconds, or negative one (-1) for a lease that never expires.",
///      "type": "integer"
///    },
///    "leaseId": {
///      "description": "Identifies the lease. Can be specified in any valid GUID string format.",
///      "type": "string"
///    },
///    "proposedLeaseId": {
///      "description": "Optional for acquire, required for change. Proposed lease ID, in a GUID string format.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LeaseContainerRequest {
    ///Specifies the lease action. Can be one of the available actions.
    pub action: LeaseContainerRequestAction,
    ///Optional. For a break action, proposed duration the lease should continue before it is broken, in seconds, between 0 and 60.
    #[serde(
        rename = "breakPeriod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub break_period: ::std::option::Option<i64>,
    ///Required for acquire. Specifies the duration of the lease, in seconds, or negative one (-1) for a lease that never expires.
    #[serde(
        rename = "leaseDuration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_duration: ::std::option::Option<i64>,
    ///Identifies the lease. Can be specified in any valid GUID string format.
    #[serde(
        rename = "leaseId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_id: ::std::option::Option<::std::string::String>,
    ///Optional for acquire, required for change. Proposed lease ID, in a GUID string format.
    #[serde(
        rename = "proposedLeaseId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub proposed_lease_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&LeaseContainerRequest> for LeaseContainerRequest {
    fn from(value: &LeaseContainerRequest) -> Self {
        value.clone()
    }
}
///Specifies the lease action. Can be one of the available actions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies the lease action. Can be one of the available actions.",
///  "type": "string",
///  "enum": [
///    "Acquire",
///    "Renew",
///    "Change",
///    "Release",
///    "Break"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LeaseContainerRequestAction"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum LeaseContainerRequestAction {
    Acquire,
    Renew,
    Change,
    Release,
    Break,
}
impl ::std::convert::From<&Self> for LeaseContainerRequestAction {
    fn from(value: &LeaseContainerRequestAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for LeaseContainerRequestAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Acquire => f.write_str("Acquire"),
            Self::Renew => f.write_str("Renew"),
            Self::Change => f.write_str("Change"),
            Self::Release => f.write_str("Release"),
            Self::Break => f.write_str("Break"),
        }
    }
}
impl ::std::str::FromStr for LeaseContainerRequestAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "acquire" => Ok(Self::Acquire),
            "renew" => Ok(Self::Renew),
            "change" => Ok(Self::Change),
            "release" => Ok(Self::Release),
            "break" => Ok(Self::Break),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LeaseContainerRequestAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LeaseContainerRequestAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LeaseContainerRequestAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Lease Container response schema.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Lease Container response schema.",
///  "properties": {
///    "leaseId": {
///      "description": "Returned unique lease ID that must be included with any request to delete the container, or to renew, change, or release the lease.",
///      "type": "string"
///    },
///    "leaseTimeSeconds": {
///      "description": "Approximate time remaining in the lease period, in seconds.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LeaseContainerResponse {
    ///Returned unique lease ID that must be included with any request to delete the container, or to renew, change, or release the lease.
    #[serde(
        rename = "leaseId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_id: ::std::option::Option<::std::string::String>,
    ///Approximate time remaining in the lease period, in seconds.
    #[serde(
        rename = "leaseTimeSeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lease_time_seconds: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&LeaseContainerResponse> for LeaseContainerResponse {
    fn from(value: &LeaseContainerResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LeaseContainerResponse {
    fn default() -> Self {
        Self {
            lease_id: Default::default(),
            lease_time_seconds: Default::default(),
        }
    }
}
///The LegalHold property of a blob container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The LegalHold property of a blob container.",
///  "required": [
///    "tags"
///  ],
///  "properties": {
///    "allowProtectedAppendWritesAll": {
///      "description": "When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining legal hold protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted.",
///      "type": "boolean"
///    },
///    "hasLegalHold": {
///      "description": "The hasLegalHold public property is set to true by SRP if there are at least one existing tag. The hasLegalHold public property is set to false by SRP if all existing legal hold tags are cleared out. There can be a maximum of 1000 blob containers with hasLegalHold=true for a given account.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "tags": {
///      "description": "Each tag should be 3 to 23 alphanumeric characters and is normalized to lower case at SRP.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "maxLength": 23,
///        "minLength": 3
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LegalHold {
    ///When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining legal hold protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted.
    #[serde(
        rename = "allowProtectedAppendWritesAll",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes_all: ::std::option::Option<bool>,
    ///The hasLegalHold public property is set to true by SRP if there are at least one existing tag. The hasLegalHold public property is set to false by SRP if all existing legal hold tags are cleared out. There can be a maximum of 1000 blob containers with hasLegalHold=true for a given account.
    #[serde(
        rename = "hasLegalHold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_legal_hold: ::std::option::Option<bool>,
    ///Each tag should be 3 to 23 alphanumeric characters and is normalized to lower case at SRP.
    pub tags: ::std::vec::Vec<LegalHoldTagsItem>,
}
impl ::std::convert::From<&LegalHold> for LegalHold {
    fn from(value: &LegalHold) -> Self {
        value.clone()
    }
}
///The LegalHold property of a blob container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The LegalHold property of a blob container.",
///  "properties": {
///    "hasLegalHold": {
///      "description": "The hasLegalHold public property is set to true by SRP if there are at least one existing tag. The hasLegalHold public property is set to false by SRP if all existing legal hold tags are cleared out. There can be a maximum of 1000 blob containers with hasLegalHold=true for a given account.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "protectedAppendWritesHistory": {
///      "$ref": "#/components/schemas/ProtectedAppendWritesHistory"
///    },
///    "tags": {
///      "description": "The list of LegalHold tags of a blob container.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TagProperty"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LegalHoldProperties {
    ///The hasLegalHold public property is set to true by SRP if there are at least one existing tag. The hasLegalHold public property is set to false by SRP if all existing legal hold tags are cleared out. There can be a maximum of 1000 blob containers with hasLegalHold=true for a given account.
    #[serde(
        rename = "hasLegalHold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_legal_hold: ::std::option::Option<bool>,
    #[serde(
        rename = "protectedAppendWritesHistory",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub protected_append_writes_history: ::std::option::Option<ProtectedAppendWritesHistory>,
    ///The list of LegalHold tags of a blob container.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::vec::Vec<TagProperty>,
}
impl ::std::convert::From<&LegalHoldProperties> for LegalHoldProperties {
    fn from(value: &LegalHoldProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LegalHoldProperties {
    fn default() -> Self {
        Self {
            has_legal_hold: Default::default(),
            protected_append_writes_history: Default::default(),
            tags: Default::default(),
        }
    }
}
///`LegalHoldTagsItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "maxLength": 23,
///  "minLength": 3
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct LegalHoldTagsItem(::std::string::String);
impl ::std::ops::Deref for LegalHoldTagsItem {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<LegalHoldTagsItem> for ::std::string::String {
    fn from(value: LegalHoldTagsItem) -> Self {
        value.0
    }
}
impl ::std::convert::From<&LegalHoldTagsItem> for LegalHoldTagsItem {
    fn from(value: &LegalHoldTagsItem) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for LegalHoldTagsItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 23usize {
            return Err("longer than 23 characters".into());
        }
        if value.chars().count() < 3usize {
            return Err("shorter than 3 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for LegalHoldTagsItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LegalHoldTagsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LegalHoldTagsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for LegalHoldTagsItem {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
///The blob container properties be listed out.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The blob container properties be listed out.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/AzureEntityResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ContainerProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListContainerItem {
    ///Resource Etag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<ContainerProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListContainerItem> for ListContainerItem {
    fn from(value: &ListContainerItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListContainerItem {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Response schema. Contains list of blobs returned, and if paging is requested or required, a URL to next page of containers.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Response schema. Contains list of blobs returned, and if paging is requested or required, a URL to next page of containers.",
///  "properties": {
///    "nextLink": {
///      "description": "Request URL that can be used to query next page of containers. Returned when total number of requested containers exceed maximum page size.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "List of blobs containers returned.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ListContainerItem"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListContainerItems {
    ///Request URL that can be used to query next page of containers. Returned when total number of requested containers exceed maximum page size.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///List of blobs containers returned.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ListContainerItem>,
}
impl ::std::convert::From<&ListContainerItems> for ListContainerItems {
    fn from(value: &ListContainerItems) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListContainerItems {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Protected append writes history setting for the blob container with Legal holds.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Protected append writes history setting for the blob container with Legal holds.",
///  "type": "object",
///  "properties": {
///    "allowProtectedAppendWritesAll": {
///      "description": "When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining legal hold protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted.",
///      "type": "boolean"
///    },
///    "timestamp": {
///      "description": "Returns the date and time the tag was added.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProtectedAppendWritesHistory {
    ///When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining legal hold protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted.
    #[serde(
        rename = "allowProtectedAppendWritesAll",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes_all: ::std::option::Option<bool>,
    ///Returns the date and time the tag was added.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timestamp: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ProtectedAppendWritesHistory> for ProtectedAppendWritesHistory {
    fn from(value: &ProtectedAppendWritesHistory) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProtectedAppendWritesHistory {
    fn default() -> Self {
        Self {
            allow_protected_append_writes_all: Default::default(),
            timestamp: Default::default(),
        }
    }
}
///Common fields that are returned in the response for all Azure Resource Manager resources
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Resource",
///  "description": "Common fields that are returned in the response for all Azure Resource Manager resources",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of the resource. E.g. \"Microsoft.Compute/virtualMachines\" or \"Microsoft.Storage/storageAccounts\"",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Resource {
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Resource> for Resource {
    fn from(value: &Resource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Resource {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            type_: Default::default(),
        }
    }
}
///The blob service properties for blob restore policy
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The blob service properties for blob restore policy",
///  "required": [
///    "enabled"
///  ],
///  "properties": {
///    "days": {
///      "description": "how long this blob can be restored. It should be great than zero and less than DeleteRetentionPolicy.days.",
///      "type": "integer",
///      "maximum": 365.0,
///      "minimum": 1.0
///    },
///    "enabled": {
///      "description": "Blob restore is enabled if set to true.",
///      "type": "boolean"
///    },
///    "lastEnabledTime": {
///      "description": "Deprecated in favor of minRestoreTime property.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "minRestoreTime": {
///      "description": "Returns the minimum date and time that the restore can be started.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RestorePolicyProperties {
    ///how long this blob can be restored. It should be great than zero and less than DeleteRetentionPolicy.days.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days: ::std::option::Option<::std::num::NonZeroU64>,
    ///Blob restore is enabled if set to true.
    pub enabled: bool,
    ///Deprecated in favor of minRestoreTime property.
    #[serde(
        rename = "lastEnabledTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_enabled_time: ::std::option::Option<::std::string::String>,
    ///Returns the minimum date and time that the restore can be started.
    #[serde(
        rename = "minRestoreTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub min_restore_time: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RestorePolicyProperties> for RestorePolicyProperties {
    fn from(value: &RestorePolicyProperties) -> Self {
        value.clone()
    }
}
///The SKU of the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU of the storage account.",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "name": {
///      "$ref": "#/components/schemas/SkuName"
///    },
///    "tier": {
///      "$ref": "#/components/schemas/Tier"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Sku {
    pub name: SkuName,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<Tier>,
}
impl ::std::convert::From<&Sku> for Sku {
    fn from(value: &Sku) -> Self {
        value.clone()
    }
}
///The SKU name. Required for account creation; optional for update. Note that in older versions, SKU name was called accountType.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU name. Required for account creation; optional for update. Note that in older versions, SKU name was called accountType.",
///  "type": "string",
///  "enum": [
///    "Standard_LRS",
///    "Standard_GRS",
///    "Standard_RAGRS",
///    "Standard_ZRS",
///    "Premium_LRS",
///    "Premium_ZRS",
///    "Standard_GZRS",
///    "Standard_RAGZRS",
///    "StandardV2_LRS",
///    "StandardV2_GRS",
///    "StandardV2_ZRS",
///    "StandardV2_GZRS",
///    "PremiumV2_LRS",
///    "PremiumV2_ZRS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SkuName"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum SkuName {
    #[serde(rename = "Standard_LRS")]
    StandardLrs,
    #[serde(rename = "Standard_GRS")]
    StandardGrs,
    #[serde(rename = "Standard_RAGRS")]
    StandardRagrs,
    #[serde(rename = "Standard_ZRS")]
    StandardZrs,
    #[serde(rename = "Premium_LRS")]
    PremiumLrs,
    #[serde(rename = "Premium_ZRS")]
    PremiumZrs,
    #[serde(rename = "Standard_GZRS")]
    StandardGzrs,
    #[serde(rename = "Standard_RAGZRS")]
    StandardRagzrs,
    #[serde(rename = "StandardV2_LRS")]
    StandardV2Lrs,
    #[serde(rename = "StandardV2_GRS")]
    StandardV2Grs,
    #[serde(rename = "StandardV2_ZRS")]
    StandardV2Zrs,
    #[serde(rename = "StandardV2_GZRS")]
    StandardV2Gzrs,
    #[serde(rename = "PremiumV2_LRS")]
    PremiumV2Lrs,
    #[serde(rename = "PremiumV2_ZRS")]
    PremiumV2Zrs,
}
impl ::std::convert::From<&Self> for SkuName {
    fn from(value: &SkuName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SkuName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::StandardLrs => f.write_str("Standard_LRS"),
            Self::StandardGrs => f.write_str("Standard_GRS"),
            Self::StandardRagrs => f.write_str("Standard_RAGRS"),
            Self::StandardZrs => f.write_str("Standard_ZRS"),
            Self::PremiumLrs => f.write_str("Premium_LRS"),
            Self::PremiumZrs => f.write_str("Premium_ZRS"),
            Self::StandardGzrs => f.write_str("Standard_GZRS"),
            Self::StandardRagzrs => f.write_str("Standard_RAGZRS"),
            Self::StandardV2Lrs => f.write_str("StandardV2_LRS"),
            Self::StandardV2Grs => f.write_str("StandardV2_GRS"),
            Self::StandardV2Zrs => f.write_str("StandardV2_ZRS"),
            Self::StandardV2Gzrs => f.write_str("StandardV2_GZRS"),
            Self::PremiumV2Lrs => f.write_str("PremiumV2_LRS"),
            Self::PremiumV2Zrs => f.write_str("PremiumV2_ZRS"),
        }
    }
}
impl ::std::str::FromStr for SkuName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard_lrs" => Ok(Self::StandardLrs),
            "standard_grs" => Ok(Self::StandardGrs),
            "standard_ragrs" => Ok(Self::StandardRagrs),
            "standard_zrs" => Ok(Self::StandardZrs),
            "premium_lrs" => Ok(Self::PremiumLrs),
            "premium_zrs" => Ok(Self::PremiumZrs),
            "standard_gzrs" => Ok(Self::StandardGzrs),
            "standard_ragzrs" => Ok(Self::StandardRagzrs),
            "standardv2_lrs" => Ok(Self::StandardV2Lrs),
            "standardv2_grs" => Ok(Self::StandardV2Grs),
            "standardv2_zrs" => Ok(Self::StandardV2Zrs),
            "standardv2_gzrs" => Ok(Self::StandardV2Gzrs),
            "premiumv2_lrs" => Ok(Self::PremiumV2Lrs),
            "premiumv2_zrs" => Ok(Self::PremiumV2Zrs),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SkuName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A tag of the LegalHold of a blob container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A tag of the LegalHold of a blob container.",
///  "properties": {
///    "objectIdentifier": {
///      "description": "Returns the Object ID of the user who added the tag.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tag": {
///      "description": "The tag value.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "Returns the Tenant ID that issued the token for the user who added the tag.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "timestamp": {
///      "description": "Returns the date and time the tag was added.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "upn": {
///      "description": "Returns the User Principal Name of the user who added the tag.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagProperty {
    ///Returns the Object ID of the user who added the tag.
    #[serde(
        rename = "objectIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub object_identifier: ::std::option::Option<::std::string::String>,
    ///The tag value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag: ::std::option::Option<::std::string::String>,
    ///Returns the Tenant ID that issued the token for the user who added the tag.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    ///Returns the date and time the tag was added.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timestamp: ::std::option::Option<::std::string::String>,
    ///Returns the User Principal Name of the user who added the tag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub upn: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TagProperty> for TagProperty {
    fn from(value: &TagProperty) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagProperty {
    fn default() -> Self {
        Self {
            object_identifier: Default::default(),
            tag: Default::default(),
            tenant_id: Default::default(),
            timestamp: Default::default(),
            upn: Default::default(),
        }
    }
}
///The SKU tier. This is based on the SKU name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU tier. This is based on the SKU name.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Standard",
///    "Premium"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "SkuTier"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum Tier {
    Standard,
    Premium,
}
impl ::std::convert::From<&Self> for Tier {
    fn from(value: &Tier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for Tier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
        }
    }
}
impl ::std::str::FromStr for Tier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "premium" => Ok(Self::Premium),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An update history of the ImmutabilityPolicy of a blob container.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An update history of the ImmutabilityPolicy of a blob container.",
///  "properties": {
///    "allowProtectedAppendWrites": {
///      "description": "This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to an append blob while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API.",
///      "type": "boolean"
///    },
///    "allowProtectedAppendWritesAll": {
///      "description": "This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API. The 'allowProtectedAppendWrites' and 'allowProtectedAppendWritesAll' properties are mutually exclusive.",
///      "type": "boolean"
///    },
///    "immutabilityPeriodSinceCreationInDays": {
///      "description": "The immutability period for the blobs in the container since the policy creation, in days.",
///      "readOnly": true,
///      "type": "integer"
///    },
///    "objectIdentifier": {
///      "description": "Returns the Object ID of the user who updated the ImmutabilityPolicy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "Returns the Tenant ID that issued the token for the user who updated the ImmutabilityPolicy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "timestamp": {
///      "description": "Returns the date and time the ImmutabilityPolicy was updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "update": {
///      "description": "The ImmutabilityPolicy update type of a blob container, possible values include: put, lock and extend.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "put",
///        "lock",
///        "extend"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ImmutabilityPolicyUpdateType"
///      }
///    },
///    "upn": {
///      "description": "Returns the User Principal Name of the user who updated the ImmutabilityPolicy.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UpdateHistoryProperty {
    ///This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to an append blob while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API.
    #[serde(
        rename = "allowProtectedAppendWrites",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes: ::std::option::Option<bool>,
    ///This property can only be changed for unlocked time-based retention policies. When enabled, new blocks can be written to both 'Append and Bock Blobs' while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted. This property cannot be changed with ExtendImmutabilityPolicy API. The 'allowProtectedAppendWrites' and 'allowProtectedAppendWritesAll' properties are mutually exclusive.
    #[serde(
        rename = "allowProtectedAppendWritesAll",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes_all: ::std::option::Option<bool>,
    ///The immutability period for the blobs in the container since the policy creation, in days.
    #[serde(
        rename = "immutabilityPeriodSinceCreationInDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutability_period_since_creation_in_days: ::std::option::Option<i64>,
    ///Returns the Object ID of the user who updated the ImmutabilityPolicy.
    #[serde(
        rename = "objectIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub object_identifier: ::std::option::Option<::std::string::String>,
    ///Returns the Tenant ID that issued the token for the user who updated the ImmutabilityPolicy.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    ///Returns the date and time the ImmutabilityPolicy was updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timestamp: ::std::option::Option<::std::string::String>,
    ///The ImmutabilityPolicy update type of a blob container, possible values include: put, lock and extend.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub update: ::std::option::Option<UpdateHistoryPropertyUpdate>,
    ///Returns the User Principal Name of the user who updated the ImmutabilityPolicy.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub upn: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&UpdateHistoryProperty> for UpdateHistoryProperty {
    fn from(value: &UpdateHistoryProperty) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UpdateHistoryProperty {
    fn default() -> Self {
        Self {
            allow_protected_append_writes: Default::default(),
            allow_protected_append_writes_all: Default::default(),
            immutability_period_since_creation_in_days: Default::default(),
            object_identifier: Default::default(),
            tenant_id: Default::default(),
            timestamp: Default::default(),
            update: Default::default(),
            upn: Default::default(),
        }
    }
}
///The ImmutabilityPolicy update type of a blob container, possible values include: put, lock and extend.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The ImmutabilityPolicy update type of a blob container, possible values include: put, lock and extend.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "put",
///    "lock",
///    "extend"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ImmutabilityPolicyUpdateType"
///  }
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum UpdateHistoryPropertyUpdate {
    #[serde(rename = "put")]
    Put,
    #[serde(rename = "lock")]
    Lock,
    #[serde(rename = "extend")]
    Extend,
}
impl ::std::convert::From<&Self> for UpdateHistoryPropertyUpdate {
    fn from(value: &UpdateHistoryPropertyUpdate) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for UpdateHistoryPropertyUpdate {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Put => f.write_str("put"),
            Self::Lock => f.write_str("lock"),
            Self::Extend => f.write_str("extend"),
        }
    }
}
impl ::std::str::FromStr for UpdateHistoryPropertyUpdate {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "put" => Ok(Self::Put),
            "lock" => Ok(Self::Lock),
            "extend" => Ok(Self::Extend),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for UpdateHistoryPropertyUpdate {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for UpdateHistoryPropertyUpdate {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for UpdateHistoryPropertyUpdate {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
