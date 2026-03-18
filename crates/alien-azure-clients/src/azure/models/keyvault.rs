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
///An identity that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An identity that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID.",
///  "type": "object",
///  "required": [
///    "objectId",
///    "permissions",
///    "tenantId"
///  ],
///  "properties": {
///    "applicationId": {
///      "description": " Application ID of the client making request on behalf of a principal",
///      "type": "string",
///      "format": "uuid"
///    },
///    "objectId": {
///      "description": "The object ID of a user, service principal or security group in the Azure Active Directory tenant for the vault. The object ID must be unique for the list of access policies.",
///      "type": "string"
///    },
///    "permissions": {
///      "$ref": "#/components/schemas/Permissions"
///    },
///    "tenantId": {
///      "description": "The Azure Active Directory tenant ID that should be used for authenticating requests to the key vault.",
///      "type": "string",
///      "format": "uuid"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AccessPolicyEntry {
    /// Application ID of the client making request on behalf of a principal
    #[serde(
        rename = "applicationId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub application_id: ::std::option::Option<::uuid::Uuid>,
    ///The object ID of a user, service principal or security group in the Azure Active Directory tenant for the vault. The object ID must be unique for the list of access policies.
    #[serde(rename = "objectId")]
    pub object_id: ::std::string::String,
    pub permissions: Permissions,
    ///The Azure Active Directory tenant ID that should be used for authenticating requests to the key vault.
    #[serde(rename = "tenantId")]
    pub tenant_id: ::uuid::Uuid,
}
impl ::std::convert::From<&AccessPolicyEntry> for AccessPolicyEntry {
    fn from(value: &AccessPolicyEntry) -> Self {
        value.clone()
    }
}
///The CheckNameAvailability operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The CheckNameAvailability operation response.",
///  "type": "object",
///  "properties": {
///    "message": {
///      "description": "An error message explaining the Reason value in more detail.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "nameAvailable": {
///      "description": "A boolean value that indicates whether the name is available for you to use. If true, the name is available. If false, the name has already been taken or is invalid and cannot be used.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "reason": {
///      "description": "The reason that a vault name could not be used. The Reason element is only returned if NameAvailable is false.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "AccountNameInvalid",
///        "AlreadyExists"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "Reason"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CheckNameAvailabilityResult {
    ///An error message explaining the Reason value in more detail.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///A boolean value that indicates whether the name is available for you to use. If true, the name is available. If false, the name has already been taken or is invalid and cannot be used.
    #[serde(
        rename = "nameAvailable",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name_available: ::std::option::Option<bool>,
    ///The reason that a vault name could not be used. The Reason element is only returned if NameAvailable is false.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reason: ::std::option::Option<CheckNameAvailabilityResultReason>,
}
impl ::std::convert::From<&CheckNameAvailabilityResult> for CheckNameAvailabilityResult {
    fn from(value: &CheckNameAvailabilityResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CheckNameAvailabilityResult {
    fn default() -> Self {
        Self {
            message: Default::default(),
            name_available: Default::default(),
            reason: Default::default(),
        }
    }
}
///The reason that a vault name could not be used. The Reason element is only returned if NameAvailable is false.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The reason that a vault name could not be used. The Reason element is only returned if NameAvailable is false.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "AccountNameInvalid",
///    "AlreadyExists"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "Reason"
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
pub enum CheckNameAvailabilityResultReason {
    AccountNameInvalid,
    AlreadyExists,
}
impl ::std::convert::From<&Self> for CheckNameAvailabilityResultReason {
    fn from(value: &CheckNameAvailabilityResultReason) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CheckNameAvailabilityResultReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AccountNameInvalid => f.write_str("AccountNameInvalid"),
            Self::AlreadyExists => f.write_str("AlreadyExists"),
        }
    }
}
impl ::std::str::FromStr for CheckNameAvailabilityResultReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "accountnameinvalid" => Ok(Self::AccountNameInvalid),
            "alreadyexists" => Ok(Self::AlreadyExists),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CheckNameAvailabilityResultReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CheckNameAvailabilityResultReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CheckNameAvailabilityResultReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An error response from Key Vault resource provider
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from Key Vault resource provider",
///  "type": "object",
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
///An error response from Key Vault resource provider
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from Key Vault resource provider",
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "Error code. This is a mnemonic that can be consumed programmatically.",
///      "type": "string"
///    },
///    "message": {
///      "description": "User friendly error message. The message is typically localized and may vary with service version.",
///      "type": "string"
///    }
///  },
///  "x-ms-external": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CloudErrorBody {
    ///Error code. This is a mnemonic that can be consumed programmatically.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///User friendly error message. The message is typically localized and may vary with service version.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
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
            message: Default::default(),
        }
    }
}
///Deleted vault information with extended details.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Deleted vault information with extended details.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The resource ID for the deleted key vault.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the key vault.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/DeletedVaultProperties"
///    },
///    "type": {
///      "description": "The resource type of the key vault.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedVault {
    ///The resource ID for the deleted key vault.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the key vault.
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
    pub properties: ::std::option::Option<DeletedVaultProperties>,
    ///The resource type of the key vault.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedVault> for DeletedVault {
    fn from(value: &DeletedVault) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedVault {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///List of vaults
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of vaults",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of deleted vaults.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of deleted vaults.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DeletedVault"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedVaultListResult {
    ///The URL to get the next set of deleted vaults.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of deleted vaults.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<DeletedVault>,
}
impl ::std::convert::From<&DeletedVaultListResult> for DeletedVaultListResult {
    fn from(value: &DeletedVaultListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedVaultListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Properties of the deleted vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the deleted vault.",
///  "type": "object",
///  "properties": {
///    "deletionDate": {
///      "description": "The deleted date.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "The location of the original vault.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "purgeProtectionEnabled": {
///      "description": "Purge protection status of the original vault.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "scheduledPurgeDate": {
///      "description": "The scheduled purged date.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tags": {
///      "description": "Tags of the original vault.",
///      "readOnly": true,
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "vaultId": {
///      "description": "The resource id of the original vault.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedVaultProperties {
    ///The deleted date.
    #[serde(
        rename = "deletionDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deletion_date: ::std::option::Option<::std::string::String>,
    ///The location of the original vault.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Purge protection status of the original vault.
    #[serde(
        rename = "purgeProtectionEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub purge_protection_enabled: ::std::option::Option<bool>,
    ///The scheduled purged date.
    #[serde(
        rename = "scheduledPurgeDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scheduled_purge_date: ::std::option::Option<::std::string::String>,
    ///Tags of the original vault.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The resource id of the original vault.
    #[serde(
        rename = "vaultId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub vault_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedVaultProperties> for DeletedVaultProperties {
    fn from(value: &DeletedVaultProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedVaultProperties {
    fn default() -> Self {
        Self {
            deletion_date: Default::default(),
            location: Default::default(),
            purge_protection_enabled: Default::default(),
            scheduled_purge_date: Default::default(),
            tags: Default::default(),
            vault_id: Default::default(),
        }
    }
}
///The type of identity.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of identity.",
///  "type": "string",
///  "enum": [
///    "User",
///    "Application",
///    "ManagedIdentity",
///    "Key"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "identityType"
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
pub enum IdentityType {
    User,
    Application,
    ManagedIdentity,
    Key,
}
impl ::std::convert::From<&Self> for IdentityType {
    fn from(value: &IdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::User => f.write_str("User"),
            Self::Application => f.write_str("Application"),
            Self::ManagedIdentity => f.write_str("ManagedIdentity"),
            Self::Key => f.write_str("Key"),
        }
    }
}
impl ::std::str::FromStr for IdentityType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "user" => Ok(Self::User),
            "application" => Ok(Self::Application),
            "managedidentity" => Ok(Self::ManagedIdentity),
            "key" => Ok(Self::Key),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A rule governing the accessibility of a vault from a specific ip address or ip range.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A rule governing the accessibility of a vault from a specific ip address or ip range.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "An IPv4 address range in CIDR notation, such as '124.56.78.91' (simple IP address) or '124.56.78.0/24' (all addresses that start with 124.56.78).",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpRule {
    ///An IPv4 address range in CIDR notation, such as '124.56.78.91' (simple IP address) or '124.56.78.0/24' (all addresses that start with 124.56.78).
    pub value: ::std::string::String,
}
impl ::std::convert::From<&IpRule> for IpRule {
    fn from(value: &IpRule) -> Self {
        value.clone()
    }
}
///A set of rules governing the network accessibility of a vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A set of rules governing the network accessibility of a vault.",
///  "type": "object",
///  "properties": {
///    "bypass": {
///      "description": "Tells what traffic can bypass network rules. This can be 'AzureServices' or 'None'.  If not specified the default is 'AzureServices'.",
///      "type": "string",
///      "enum": [
///        "AzureServices",
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NetworkRuleBypassOptions"
///      }
///    },
///    "defaultAction": {
///      "description": "The default action when no rule from ipRules and from virtualNetworkRules match. This is only used after the bypass property has been evaluated.",
///      "type": "string",
///      "enum": [
///        "Allow",
///        "Deny"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NetworkRuleAction"
///      }
///    },
///    "ipRules": {
///      "description": "The list of IP address rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IPRule"
///      }
///    },
///    "virtualNetworkRules": {
///      "description": "The list of virtual network rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/VirtualNetworkRule"
///      },
///      "x-ms-identifiers": [
///        "id"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkRuleSet {
    ///Tells what traffic can bypass network rules. This can be 'AzureServices' or 'None'.  If not specified the default is 'AzureServices'.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub bypass: ::std::option::Option<NetworkRuleSetBypass>,
    ///The default action when no rule from ipRules and from virtualNetworkRules match. This is only used after the bypass property has been evaluated.
    #[serde(
        rename = "defaultAction",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_action: ::std::option::Option<NetworkRuleSetDefaultAction>,
    ///The list of IP address rules.
    #[serde(
        rename = "ipRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_rules: ::std::vec::Vec<IpRule>,
    ///The list of virtual network rules.
    #[serde(
        rename = "virtualNetworkRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_network_rules: ::std::vec::Vec<VirtualNetworkRule>,
}
impl ::std::convert::From<&NetworkRuleSet> for NetworkRuleSet {
    fn from(value: &NetworkRuleSet) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkRuleSet {
    fn default() -> Self {
        Self {
            bypass: Default::default(),
            default_action: Default::default(),
            ip_rules: Default::default(),
            virtual_network_rules: Default::default(),
        }
    }
}
///Tells what traffic can bypass network rules. This can be 'AzureServices' or 'None'.  If not specified the default is 'AzureServices'.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tells what traffic can bypass network rules. This can be 'AzureServices' or 'None'.  If not specified the default is 'AzureServices'.",
///  "type": "string",
///  "enum": [
///    "AzureServices",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkRuleBypassOptions"
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
pub enum NetworkRuleSetBypass {
    AzureServices,
    None,
}
impl ::std::convert::From<&Self> for NetworkRuleSetBypass {
    fn from(value: &NetworkRuleSetBypass) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkRuleSetBypass {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AzureServices => f.write_str("AzureServices"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for NetworkRuleSetBypass {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "azureservices" => Ok(Self::AzureServices),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkRuleSetBypass {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkRuleSetBypass {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkRuleSetBypass {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The default action when no rule from ipRules and from virtualNetworkRules match. This is only used after the bypass property has been evaluated.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The default action when no rule from ipRules and from virtualNetworkRules match. This is only used after the bypass property has been evaluated.",
///  "type": "string",
///  "enum": [
///    "Allow",
///    "Deny"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkRuleAction"
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
pub enum NetworkRuleSetDefaultAction {
    Allow,
    Deny,
}
impl ::std::convert::From<&Self> for NetworkRuleSetDefaultAction {
    fn from(value: &NetworkRuleSetDefaultAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkRuleSetDefaultAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
            Self::Deny => f.write_str("Deny"),
        }
    }
}
impl ::std::str::FromStr for NetworkRuleSetDefaultAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkRuleSetDefaultAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkRuleSetDefaultAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkRuleSetDefaultAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Permissions the identity has for keys, secrets, certificates and storage.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Permissions the identity has for keys, secrets, certificates and storage.",
///  "type": "object",
///  "properties": {
///    "certificates": {
///      "description": "Permissions to certificates",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "all",
///          "get",
///          "list",
///          "delete",
///          "create",
///          "import",
///          "update",
///          "managecontacts",
///          "getissuers",
///          "listissuers",
///          "setissuers",
///          "deleteissuers",
///          "manageissuers",
///          "recover",
///          "purge",
///          "backup",
///          "restore"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "CertificatePermissions"
///        }
///      }
///    },
///    "keys": {
///      "description": "Permissions to keys",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "all",
///          "encrypt",
///          "decrypt",
///          "wrapKey",
///          "unwrapKey",
///          "sign",
///          "verify",
///          "get",
///          "list",
///          "create",
///          "update",
///          "import",
///          "delete",
///          "backup",
///          "restore",
///          "recover",
///          "purge",
///          "release",
///          "rotate",
///          "getrotationpolicy",
///          "setrotationpolicy"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "KeyPermissions"
///        }
///      }
///    },
///    "secrets": {
///      "description": "Permissions to secrets",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "all",
///          "get",
///          "list",
///          "set",
///          "delete",
///          "backup",
///          "restore",
///          "recover",
///          "purge"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "SecretPermissions"
///        }
///      }
///    },
///    "storage": {
///      "description": "Permissions to storage accounts",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "all",
///          "get",
///          "list",
///          "delete",
///          "set",
///          "update",
///          "regeneratekey",
///          "recover",
///          "purge",
///          "backup",
///          "restore",
///          "setsas",
///          "listsas",
///          "getsas",
///          "deletesas"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "StoragePermissions"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Permissions {
    ///Permissions to certificates
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificates: ::std::vec::Vec<PermissionsCertificatesItem>,
    ///Permissions to keys
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keys: ::std::vec::Vec<PermissionsKeysItem>,
    ///Permissions to secrets
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secrets: ::std::vec::Vec<PermissionsSecretsItem>,
    ///Permissions to storage accounts
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub storage: ::std::vec::Vec<PermissionsStorageItem>,
}
impl ::std::convert::From<&Permissions> for Permissions {
    fn from(value: &Permissions) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Permissions {
    fn default() -> Self {
        Self {
            certificates: Default::default(),
            keys: Default::default(),
            secrets: Default::default(),
            storage: Default::default(),
        }
    }
}
///`PermissionsCertificatesItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "all",
///    "get",
///    "list",
///    "delete",
///    "create",
///    "import",
///    "update",
///    "managecontacts",
///    "getissuers",
///    "listissuers",
///    "setissuers",
///    "deleteissuers",
///    "manageissuers",
///    "recover",
///    "purge",
///    "backup",
///    "restore"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CertificatePermissions"
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
pub enum PermissionsCertificatesItem {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "list")]
    List,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "create")]
    Create,
    #[serde(rename = "import")]
    Import,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "managecontacts")]
    Managecontacts,
    #[serde(rename = "getissuers")]
    Getissuers,
    #[serde(rename = "listissuers")]
    Listissuers,
    #[serde(rename = "setissuers")]
    Setissuers,
    #[serde(rename = "deleteissuers")]
    Deleteissuers,
    #[serde(rename = "manageissuers")]
    Manageissuers,
    #[serde(rename = "recover")]
    Recover,
    #[serde(rename = "purge")]
    Purge,
    #[serde(rename = "backup")]
    Backup,
    #[serde(rename = "restore")]
    Restore,
}
impl ::std::convert::From<&Self> for PermissionsCertificatesItem {
    fn from(value: &PermissionsCertificatesItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PermissionsCertificatesItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::All => f.write_str("all"),
            Self::Get => f.write_str("get"),
            Self::List => f.write_str("list"),
            Self::Delete => f.write_str("delete"),
            Self::Create => f.write_str("create"),
            Self::Import => f.write_str("import"),
            Self::Update => f.write_str("update"),
            Self::Managecontacts => f.write_str("managecontacts"),
            Self::Getissuers => f.write_str("getissuers"),
            Self::Listissuers => f.write_str("listissuers"),
            Self::Setissuers => f.write_str("setissuers"),
            Self::Deleteissuers => f.write_str("deleteissuers"),
            Self::Manageissuers => f.write_str("manageissuers"),
            Self::Recover => f.write_str("recover"),
            Self::Purge => f.write_str("purge"),
            Self::Backup => f.write_str("backup"),
            Self::Restore => f.write_str("restore"),
        }
    }
}
impl ::std::str::FromStr for PermissionsCertificatesItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "get" => Ok(Self::Get),
            "list" => Ok(Self::List),
            "delete" => Ok(Self::Delete),
            "create" => Ok(Self::Create),
            "import" => Ok(Self::Import),
            "update" => Ok(Self::Update),
            "managecontacts" => Ok(Self::Managecontacts),
            "getissuers" => Ok(Self::Getissuers),
            "listissuers" => Ok(Self::Listissuers),
            "setissuers" => Ok(Self::Setissuers),
            "deleteissuers" => Ok(Self::Deleteissuers),
            "manageissuers" => Ok(Self::Manageissuers),
            "recover" => Ok(Self::Recover),
            "purge" => Ok(Self::Purge),
            "backup" => Ok(Self::Backup),
            "restore" => Ok(Self::Restore),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PermissionsCertificatesItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PermissionsCertificatesItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PermissionsCertificatesItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`PermissionsKeysItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "all",
///    "encrypt",
///    "decrypt",
///    "wrapKey",
///    "unwrapKey",
///    "sign",
///    "verify",
///    "get",
///    "list",
///    "create",
///    "update",
///    "import",
///    "delete",
///    "backup",
///    "restore",
///    "recover",
///    "purge",
///    "release",
///    "rotate",
///    "getrotationpolicy",
///    "setrotationpolicy"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "KeyPermissions"
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
pub enum PermissionsKeysItem {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "encrypt")]
    Encrypt,
    #[serde(rename = "decrypt")]
    Decrypt,
    #[serde(rename = "wrapKey")]
    WrapKey,
    #[serde(rename = "unwrapKey")]
    UnwrapKey,
    #[serde(rename = "sign")]
    Sign,
    #[serde(rename = "verify")]
    Verify,
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "list")]
    List,
    #[serde(rename = "create")]
    Create,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "import")]
    Import,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "backup")]
    Backup,
    #[serde(rename = "restore")]
    Restore,
    #[serde(rename = "recover")]
    Recover,
    #[serde(rename = "purge")]
    Purge,
    #[serde(rename = "release")]
    Release,
    #[serde(rename = "rotate")]
    Rotate,
    #[serde(rename = "getrotationpolicy")]
    Getrotationpolicy,
    #[serde(rename = "setrotationpolicy")]
    Setrotationpolicy,
}
impl ::std::convert::From<&Self> for PermissionsKeysItem {
    fn from(value: &PermissionsKeysItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PermissionsKeysItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::All => f.write_str("all"),
            Self::Encrypt => f.write_str("encrypt"),
            Self::Decrypt => f.write_str("decrypt"),
            Self::WrapKey => f.write_str("wrapKey"),
            Self::UnwrapKey => f.write_str("unwrapKey"),
            Self::Sign => f.write_str("sign"),
            Self::Verify => f.write_str("verify"),
            Self::Get => f.write_str("get"),
            Self::List => f.write_str("list"),
            Self::Create => f.write_str("create"),
            Self::Update => f.write_str("update"),
            Self::Import => f.write_str("import"),
            Self::Delete => f.write_str("delete"),
            Self::Backup => f.write_str("backup"),
            Self::Restore => f.write_str("restore"),
            Self::Recover => f.write_str("recover"),
            Self::Purge => f.write_str("purge"),
            Self::Release => f.write_str("release"),
            Self::Rotate => f.write_str("rotate"),
            Self::Getrotationpolicy => f.write_str("getrotationpolicy"),
            Self::Setrotationpolicy => f.write_str("setrotationpolicy"),
        }
    }
}
impl ::std::str::FromStr for PermissionsKeysItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "encrypt" => Ok(Self::Encrypt),
            "decrypt" => Ok(Self::Decrypt),
            "wrapkey" => Ok(Self::WrapKey),
            "unwrapkey" => Ok(Self::UnwrapKey),
            "sign" => Ok(Self::Sign),
            "verify" => Ok(Self::Verify),
            "get" => Ok(Self::Get),
            "list" => Ok(Self::List),
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "import" => Ok(Self::Import),
            "delete" => Ok(Self::Delete),
            "backup" => Ok(Self::Backup),
            "restore" => Ok(Self::Restore),
            "recover" => Ok(Self::Recover),
            "purge" => Ok(Self::Purge),
            "release" => Ok(Self::Release),
            "rotate" => Ok(Self::Rotate),
            "getrotationpolicy" => Ok(Self::Getrotationpolicy),
            "setrotationpolicy" => Ok(Self::Setrotationpolicy),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PermissionsKeysItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PermissionsKeysItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PermissionsKeysItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`PermissionsSecretsItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "all",
///    "get",
///    "list",
///    "set",
///    "delete",
///    "backup",
///    "restore",
///    "recover",
///    "purge"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SecretPermissions"
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
pub enum PermissionsSecretsItem {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "list")]
    List,
    #[serde(rename = "set")]
    Set,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "backup")]
    Backup,
    #[serde(rename = "restore")]
    Restore,
    #[serde(rename = "recover")]
    Recover,
    #[serde(rename = "purge")]
    Purge,
}
impl ::std::convert::From<&Self> for PermissionsSecretsItem {
    fn from(value: &PermissionsSecretsItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PermissionsSecretsItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::All => f.write_str("all"),
            Self::Get => f.write_str("get"),
            Self::List => f.write_str("list"),
            Self::Set => f.write_str("set"),
            Self::Delete => f.write_str("delete"),
            Self::Backup => f.write_str("backup"),
            Self::Restore => f.write_str("restore"),
            Self::Recover => f.write_str("recover"),
            Self::Purge => f.write_str("purge"),
        }
    }
}
impl ::std::str::FromStr for PermissionsSecretsItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "get" => Ok(Self::Get),
            "list" => Ok(Self::List),
            "set" => Ok(Self::Set),
            "delete" => Ok(Self::Delete),
            "backup" => Ok(Self::Backup),
            "restore" => Ok(Self::Restore),
            "recover" => Ok(Self::Recover),
            "purge" => Ok(Self::Purge),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PermissionsSecretsItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PermissionsSecretsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PermissionsSecretsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`PermissionsStorageItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "all",
///    "get",
///    "list",
///    "delete",
///    "set",
///    "update",
///    "regeneratekey",
///    "recover",
///    "purge",
///    "backup",
///    "restore",
///    "setsas",
///    "listsas",
///    "getsas",
///    "deletesas"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "StoragePermissions"
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
pub enum PermissionsStorageItem {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "list")]
    List,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "set")]
    Set,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "regeneratekey")]
    Regeneratekey,
    #[serde(rename = "recover")]
    Recover,
    #[serde(rename = "purge")]
    Purge,
    #[serde(rename = "backup")]
    Backup,
    #[serde(rename = "restore")]
    Restore,
    #[serde(rename = "setsas")]
    Setsas,
    #[serde(rename = "listsas")]
    Listsas,
    #[serde(rename = "getsas")]
    Getsas,
    #[serde(rename = "deletesas")]
    Deletesas,
}
impl ::std::convert::From<&Self> for PermissionsStorageItem {
    fn from(value: &PermissionsStorageItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PermissionsStorageItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::All => f.write_str("all"),
            Self::Get => f.write_str("get"),
            Self::List => f.write_str("list"),
            Self::Delete => f.write_str("delete"),
            Self::Set => f.write_str("set"),
            Self::Update => f.write_str("update"),
            Self::Regeneratekey => f.write_str("regeneratekey"),
            Self::Recover => f.write_str("recover"),
            Self::Purge => f.write_str("purge"),
            Self::Backup => f.write_str("backup"),
            Self::Restore => f.write_str("restore"),
            Self::Setsas => f.write_str("setsas"),
            Self::Listsas => f.write_str("listsas"),
            Self::Getsas => f.write_str("getsas"),
            Self::Deletesas => f.write_str("deletesas"),
        }
    }
}
impl ::std::str::FromStr for PermissionsStorageItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "get" => Ok(Self::Get),
            "list" => Ok(Self::List),
            "delete" => Ok(Self::Delete),
            "set" => Ok(Self::Set),
            "update" => Ok(Self::Update),
            "regeneratekey" => Ok(Self::Regeneratekey),
            "recover" => Ok(Self::Recover),
            "purge" => Ok(Self::Purge),
            "backup" => Ok(Self::Backup),
            "restore" => Ok(Self::Restore),
            "setsas" => Ok(Self::Setsas),
            "listsas" => Ok(Self::Listsas),
            "getsas" => Ok(Self::Getsas),
            "deletesas" => Ok(Self::Deletesas),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PermissionsStorageItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PermissionsStorageItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PermissionsStorageItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Private endpoint object properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Private endpoint object properties.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Full identifier of the private endpoint resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpoint {
    ///Full identifier of the private endpoint resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateEndpoint> for PrivateEndpoint {
    fn from(value: &PrivateEndpoint) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpoint {
    fn default() -> Self {
        Self {
            id: Default::default(),
        }
    }
}
///Private endpoint connection resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Private endpoint connection resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "Modified whenever there is a change in the state of private endpoint connection.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProperties"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnection {
    ///Modified whenever there is a change in the state of private endpoint connection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Fully qualified identifier of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Azure location of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Name of the key vault resource.
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
    pub properties: ::std::option::Option<PrivateEndpointConnectionProperties>,
    ///Tags assigned to the key vault resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type of the key vault resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateEndpointConnection> for PrivateEndpointConnection {
    fn from(value: &PrivateEndpointConnection) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointConnection {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Private endpoint connection item.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Private endpoint connection item.",
///  "type": "object",
///  "properties": {
///    "etag": {
///      "description": "Modified whenever there is a change in the state of private endpoint connection.",
///      "type": "string"
///    },
///    "id": {
///      "description": "Id of private endpoint connection.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnectionItem {
    ///Modified whenever there is a change in the state of private endpoint connection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Id of private endpoint connection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<PrivateEndpointConnectionProperties>,
}
impl ::std::convert::From<&PrivateEndpointConnectionItem> for PrivateEndpointConnectionItem {
    fn from(value: &PrivateEndpointConnectionItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointConnectionItem {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            properties: Default::default(),
        }
    }
}
///List of private endpoint connections.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of private endpoint connections.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of private endpoint connections.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of private endpoint connections.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnectionListResult {
    ///The URL to get the next set of private endpoint connections.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of private endpoint connections.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PrivateEndpointConnection>,
}
impl ::std::convert::From<&PrivateEndpointConnectionListResult>
    for PrivateEndpointConnectionListResult
{
    fn from(value: &PrivateEndpointConnectionListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointConnectionListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Properties of the private endpoint connection resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the private endpoint connection resource.",
///  "type": "object",
///  "properties": {
///    "privateEndpoint": {
///      "$ref": "#/components/schemas/PrivateEndpoint"
///    },
///    "privateLinkServiceConnectionState": {
///      "$ref": "#/components/schemas/PrivateLinkServiceConnectionState"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnectionProperties {
    #[serde(
        rename = "privateEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint: ::std::option::Option<PrivateEndpoint>,
    #[serde(
        rename = "privateLinkServiceConnectionState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_service_connection_state:
        ::std::option::Option<PrivateLinkServiceConnectionState>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<PrivateEndpointConnectionProvisioningState>,
}
impl ::std::convert::From<&PrivateEndpointConnectionProperties>
    for PrivateEndpointConnectionProperties
{
    fn from(value: &PrivateEndpointConnectionProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointConnectionProperties {
    fn default() -> Self {
        Self {
            private_endpoint: Default::default(),
            private_link_service_connection_state: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///The current provisioning state.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The current provisioning state.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Failed",
///    "Disconnected"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateEndpointConnectionProvisioningState"
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
pub enum PrivateEndpointConnectionProvisioningState {
    Succeeded,
    Creating,
    Updating,
    Deleting,
    Failed,
    Disconnected,
}
impl ::std::convert::From<&Self> for PrivateEndpointConnectionProvisioningState {
    fn from(value: &PrivateEndpointConnectionProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateEndpointConnectionProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Failed => f.write_str("Failed"),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointConnectionProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "failed" => Ok(Self::Failed),
            "disconnected" => Ok(Self::Disconnected),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateEndpointConnectionProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PrivateEndpointConnectionProvisioningState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateEndpointConnectionProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The private endpoint connection status.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The private endpoint connection status.",
///  "type": "string",
///  "enum": [
///    "Pending",
///    "Approved",
///    "Rejected",
///    "Disconnected"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateEndpointServiceConnectionStatus"
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
pub enum PrivateEndpointServiceConnectionStatus {
    Pending,
    Approved,
    Rejected,
    Disconnected,
}
impl ::std::convert::From<&Self> for PrivateEndpointServiceConnectionStatus {
    fn from(value: &PrivateEndpointServiceConnectionStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateEndpointServiceConnectionStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Pending => f.write_str("Pending"),
            Self::Approved => f.write_str("Approved"),
            Self::Rejected => f.write_str("Rejected"),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointServiceConnectionStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "disconnected" => Ok(Self::Disconnected),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateEndpointServiceConnectionStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PrivateEndpointServiceConnectionStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateEndpointServiceConnectionStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A private link resource
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A private link resource",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkResourceProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResource {
    ///Fully qualified identifier of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Azure location of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Name of the key vault resource.
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
    pub properties: ::std::option::Option<PrivateLinkResourceProperties>,
    ///Tags assigned to the key vault resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type of the key vault resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkResource> for PrivateLinkResource {
    fn from(value: &PrivateLinkResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResource {
    fn default() -> Self {
        Self {
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///A list of private link resources
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A list of private link resources",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "Array of private link resources",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateLinkResource"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResourceListResult {
    ///Array of private link resources
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PrivateLinkResource>,
}
impl ::std::convert::From<&PrivateLinkResourceListResult> for PrivateLinkResourceListResult {
    fn from(value: &PrivateLinkResourceListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResourceListResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///Properties of a private link resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of a private link resource.",
///  "type": "object",
///  "properties": {
///    "groupId": {
///      "description": "Group identifier of private link resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "requiredMembers": {
///      "description": "Required member names of private link resource.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "requiredZoneNames": {
///      "description": "Required DNS zone names of the the private link resource.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResourceProperties {
    ///Group identifier of private link resource.
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<::std::string::String>,
    ///Required member names of private link resource.
    #[serde(
        rename = "requiredMembers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_members: ::std::vec::Vec<::std::string::String>,
    ///Required DNS zone names of the the private link resource.
    #[serde(
        rename = "requiredZoneNames",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_zone_names: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkResourceProperties> for PrivateLinkResourceProperties {
    fn from(value: &PrivateLinkResourceProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResourceProperties {
    fn default() -> Self {
        Self {
            group_id: Default::default(),
            required_members: Default::default(),
            required_zone_names: Default::default(),
        }
    }
}
///An object that represents the approval state of the private link connection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents the approval state of the private link connection.",
///  "type": "object",
///  "properties": {
///    "actionsRequired": {
///      "description": "A message indicating if changes on the service provider require any updates on the consumer.",
///      "type": "string",
///      "enum": [
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ActionsRequired"
///      }
///    },
///    "description": {
///      "description": "The reason for approval or rejection.",
///      "type": "string"
///    },
///    "status": {
///      "$ref": "#/components/schemas/PrivateEndpointServiceConnectionStatus"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkServiceConnectionState {
    ///A message indicating if changes on the service provider require any updates on the consumer.
    #[serde(
        rename = "actionsRequired",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub actions_required: ::std::option::Option<PrivateLinkServiceConnectionStateActionsRequired>,
    ///The reason for approval or rejection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<PrivateEndpointServiceConnectionStatus>,
}
impl ::std::convert::From<&PrivateLinkServiceConnectionState>
    for PrivateLinkServiceConnectionState
{
    fn from(value: &PrivateLinkServiceConnectionState) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkServiceConnectionState {
    fn default() -> Self {
        Self {
            actions_required: Default::default(),
            description: Default::default(),
            status: Default::default(),
        }
    }
}
///A message indicating if changes on the service provider require any updates on the consumer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A message indicating if changes on the service provider require any updates on the consumer.",
///  "type": "string",
///  "enum": [
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ActionsRequired"
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
pub enum PrivateLinkServiceConnectionStateActionsRequired {
    None,
}
impl ::std::convert::From<&Self> for PrivateLinkServiceConnectionStateActionsRequired {
    fn from(value: &PrivateLinkServiceConnectionStateActionsRequired) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateLinkServiceConnectionStateActionsRequired {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for PrivateLinkServiceConnectionStateActionsRequired {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateLinkServiceConnectionStateActionsRequired {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PrivateLinkServiceConnectionStateActionsRequired
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for PrivateLinkServiceConnectionStateActionsRequired
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Key Vault resource
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Key Vault resource",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Fully qualified identifier of the key vault resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "Azure location of the key vault resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the key vault resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tags": {
///      "description": "Tags assigned to the key vault resource.",
///      "readOnly": true,
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "Resource type of the key vault resource.",
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
    ///Fully qualified identifier of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Azure location of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Name of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Tags assigned to the key vault resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type of the key vault resource.
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
            location: Default::default(),
            name: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///List of vault resources.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of vault resources.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of vault resources.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of vault resources.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Resource"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceListResult {
    ///The URL to get the next set of vault resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of vault resources.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Resource>,
}
impl ::std::convert::From<&ResourceListResult> for ResourceListResult {
    fn from(value: &ResourceListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///SKU details
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU details",
///  "type": "object",
///  "required": [
///    "family",
///    "name"
///  ],
///  "properties": {
///    "family": {
///      "description": "SKU family name",
///      "type": "string",
///      "enum": [
///        "A"
///      ],
///      "x-ms-client-default": "A",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SkuFamily"
///      }
///    },
///    "name": {
///      "description": "SKU name to specify whether the key vault is a standard vault or a premium vault.",
///      "type": "string",
///      "enum": [
///        "standard",
///        "premium"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "SkuName"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Sku {
    ///SKU family name
    pub family: SkuFamily,
    ///SKU name to specify whether the key vault is a standard vault or a premium vault.
    pub name: SkuName,
}
impl ::std::convert::From<&Sku> for Sku {
    fn from(value: &Sku) -> Self {
        value.clone()
    }
}
///SKU family name
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU family name",
///  "type": "string",
///  "enum": [
///    "A"
///  ],
///  "x-ms-client-default": "A",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SkuFamily"
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
pub enum SkuFamily {
    A,
}
impl ::std::convert::From<&Self> for SkuFamily {
    fn from(value: &SkuFamily) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SkuFamily {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::A => f.write_str("A"),
        }
    }
}
impl ::std::str::FromStr for SkuFamily {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "a" => Ok(Self::A),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SkuFamily {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SkuFamily {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SkuFamily {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///SKU name to specify whether the key vault is a standard vault or a premium vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU name to specify whether the key vault is a standard vault or a premium vault.",
///  "type": "string",
///  "enum": [
///    "standard",
///    "premium"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
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
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "premium")]
    Premium,
}
impl ::std::convert::From<&Self> for SkuName {
    fn from(value: &SkuName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SkuName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("standard"),
            Self::Premium => f.write_str("premium"),
        }
    }
}
impl ::std::str::FromStr for SkuName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "premium" => Ok(Self::Premium),
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
///Metadata pertaining to creation and last modification of the key vault resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Metadata pertaining to creation and last modification of the key vault resource.",
///  "readOnly": true,
///  "type": "object",
///  "properties": {
///    "createdAt": {
///      "description": "The timestamp of the key vault resource creation (UTC).",
///      "type": "string"
///    },
///    "createdBy": {
///      "description": "The identity that created the key vault resource.",
///      "type": "string"
///    },
///    "createdByType": {
///      "$ref": "#/components/schemas/IdentityType"
///    },
///    "lastModifiedAt": {
///      "description": "The timestamp of the key vault resource last modification (UTC).",
///      "type": "string"
///    },
///    "lastModifiedBy": {
///      "description": "The identity that last modified the key vault resource.",
///      "type": "string"
///    },
///    "lastModifiedByType": {
///      "$ref": "#/components/schemas/IdentityType"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SystemData {
    ///The timestamp of the key vault resource creation (UTC).
    #[serde(
        rename = "createdAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_at: ::std::option::Option<::std::string::String>,
    ///The identity that created the key vault resource.
    #[serde(
        rename = "createdBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_by: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "createdByType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_by_type: ::std::option::Option<IdentityType>,
    ///The timestamp of the key vault resource last modification (UTC).
    #[serde(
        rename = "lastModifiedAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_at: ::std::option::Option<::std::string::String>,
    ///The identity that last modified the key vault resource.
    #[serde(
        rename = "lastModifiedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_by: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "lastModifiedByType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_by_type: ::std::option::Option<IdentityType>,
}
impl ::std::convert::From<&SystemData> for SystemData {
    fn from(value: &SystemData) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SystemData {
    fn default() -> Self {
        Self {
            created_at: Default::default(),
            created_by: Default::default(),
            created_by_type: Default::default(),
            last_modified_at: Default::default(),
            last_modified_by: Default::default(),
            last_modified_by_type: Default::default(),
        }
    }
}
///Resource information with extended details.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource information with extended details.",
///  "type": "object",
///  "required": [
///    "properties"
///  ],
///  "properties": {
///    "id": {
///      "description": "Fully qualified identifier of the key vault resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "Azure location of the key vault resource.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the key vault resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/VaultProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/SystemData"
///    },
///    "tags": {
///      "description": "Tags assigned to the key vault resource.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "Resource type of the key vault resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Vault {
    ///Fully qualified identifier of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Azure location of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Name of the key vault resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    pub properties: VaultProperties,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///Tags assigned to the key vault resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type of the key vault resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Vault> for Vault {
    fn from(value: &Vault) -> Self {
        value.clone()
    }
}
///Parameters for updating the access policy in a vault
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters for updating the access policy in a vault",
///  "type": "object",
///  "required": [
///    "properties"
///  ],
///  "properties": {
///    "id": {
///      "description": "The resource id of the access policy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "The resource type of the access policy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The resource name of the access policy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/VaultAccessPolicyProperties"
///    },
///    "type": {
///      "description": "The resource name of the access policy.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultAccessPolicyParameters {
    ///The resource id of the access policy.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The resource type of the access policy.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///The resource name of the access policy.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    pub properties: VaultAccessPolicyProperties,
    ///The resource name of the access policy.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&VaultAccessPolicyParameters> for VaultAccessPolicyParameters {
    fn from(value: &VaultAccessPolicyParameters) -> Self {
        value.clone()
    }
}
///Properties of the vault access policy
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the vault access policy",
///  "type": "object",
///  "required": [
///    "accessPolicies"
///  ],
///  "properties": {
///    "accessPolicies": {
///      "description": "An array of 0 to 16 identities that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AccessPolicyEntry"
///      },
///      "x-ms-identifiers": [
///        "tenantId",
///        "objectId",
///        "permissions"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultAccessPolicyProperties {
    ///An array of 0 to 16 identities that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID.
    #[serde(rename = "accessPolicies")]
    pub access_policies: ::std::vec::Vec<AccessPolicyEntry>,
}
impl ::std::convert::From<&VaultAccessPolicyProperties> for VaultAccessPolicyProperties {
    fn from(value: &VaultAccessPolicyProperties) -> Self {
        value.clone()
    }
}
///The parameters used to check the availability of the vault name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used to check the availability of the vault name.",
///  "type": "object",
///  "required": [
///    "name",
///    "type"
///  ],
///  "properties": {
///    "name": {
///      "description": "The vault name.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of resource, Microsoft.KeyVault/vaults",
///      "type": "string",
///      "enum": [
///        "Microsoft.KeyVault/vaults"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "Type"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultCheckNameAvailabilityParameters {
    ///The vault name.
    pub name: ::std::string::String,
    ///The type of resource, Microsoft.KeyVault/vaults
    #[serde(rename = "type")]
    pub type_: VaultCheckNameAvailabilityParametersType,
}
impl ::std::convert::From<&VaultCheckNameAvailabilityParameters>
    for VaultCheckNameAvailabilityParameters
{
    fn from(value: &VaultCheckNameAvailabilityParameters) -> Self {
        value.clone()
    }
}
///The type of resource, Microsoft.KeyVault/vaults
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of resource, Microsoft.KeyVault/vaults",
///  "type": "string",
///  "enum": [
///    "Microsoft.KeyVault/vaults"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "Type"
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
pub enum VaultCheckNameAvailabilityParametersType {
    #[serde(rename = "Microsoft.KeyVault/vaults")]
    MicrosoftKeyVaultVaults,
}
impl ::std::convert::From<&Self> for VaultCheckNameAvailabilityParametersType {
    fn from(value: &VaultCheckNameAvailabilityParametersType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VaultCheckNameAvailabilityParametersType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftKeyVaultVaults => f.write_str("Microsoft.KeyVault/vaults"),
        }
    }
}
impl ::std::str::FromStr for VaultCheckNameAvailabilityParametersType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoft.keyvault/vaults" => Ok(Self::MicrosoftKeyVaultVaults),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VaultCheckNameAvailabilityParametersType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VaultCheckNameAvailabilityParametersType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VaultCheckNameAvailabilityParametersType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Parameters for creating or updating a vault
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters for creating or updating a vault",
///  "type": "object",
///  "required": [
///    "location",
///    "properties"
///  ],
///  "properties": {
///    "location": {
///      "description": "The supported Azure location where the key vault should be created.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/VaultProperties"
///    },
///    "tags": {
///      "description": "The tags that will be assigned to the key vault.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultCreateOrUpdateParameters {
    ///The supported Azure location where the key vault should be created.
    pub location: ::std::string::String,
    pub properties: VaultProperties,
    ///The tags that will be assigned to the key vault.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&VaultCreateOrUpdateParameters> for VaultCreateOrUpdateParameters {
    fn from(value: &VaultCreateOrUpdateParameters) -> Self {
        value.clone()
    }
}
///List of vaults
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of vaults",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of vaults.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of vaults.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Vault"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultListResult {
    ///The URL to get the next set of vaults.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of vaults.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Vault>,
}
impl ::std::convert::From<&VaultListResult> for VaultListResult {
    fn from(value: &VaultListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VaultListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Parameters for creating or updating a vault
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters for creating or updating a vault",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/VaultPatchProperties"
///    },
///    "tags": {
///      "description": "The tags that will be assigned to the key vault. ",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultPatchParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<VaultPatchProperties>,
    ///The tags that will be assigned to the key vault.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&VaultPatchParameters> for VaultPatchParameters {
    fn from(value: &VaultPatchParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VaultPatchParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
            tags: Default::default(),
        }
    }
}
///Properties of the vault
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the vault",
///  "type": "object",
///  "properties": {
///    "accessPolicies": {
///      "description": "An array of 0 to 16 identities that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AccessPolicyEntry"
///      },
///      "x-ms-identifiers": [
///        "tenantId",
///        "objectId",
///        "permissions"
///      ]
///    },
///    "createMode": {
///      "description": "The vault's create mode to indicate whether the vault need to be recovered or not.",
///      "type": "string",
///      "enum": [
///        "recover",
///        "default"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "CreateMode"
///      }
///    },
///    "enablePurgeProtection": {
///      "description": "Property specifying whether protection against purge is enabled for this vault. Setting this property to true activates protection against purge for this vault and its content - only the Key Vault service may initiate a hard, irrecoverable deletion. The setting is effective only if soft delete is also enabled. Enabling this functionality is irreversible - that is, the property does not accept false as its value.",
///      "type": "boolean"
///    },
///    "enableRbacAuthorization": {
///      "description": "Property that controls how data actions are authorized. When true, the key vault will use Role Based Access Control (RBAC) for authorization of data actions, and the access policies specified in vault properties will be  ignored. When false, the key vault will use the access policies specified in vault properties, and any policy stored on Azure Resource Manager will be ignored. If null or not specified, the value of this property will not change.",
///      "type": "boolean"
///    },
///    "enableSoftDelete": {
///      "description": "Property to specify whether the 'soft delete' functionality is enabled for this key vault. Once set to true, it cannot be reverted to false.",
///      "type": "boolean"
///    },
///    "enabledForDeployment": {
///      "description": "Property to specify whether Azure Virtual Machines are permitted to retrieve certificates stored as secrets from the key vault.",
///      "type": "boolean"
///    },
///    "enabledForDiskEncryption": {
///      "description": "Property to specify whether Azure Disk Encryption is permitted to retrieve secrets from the vault and unwrap keys.",
///      "type": "boolean"
///    },
///    "enabledForTemplateDeployment": {
///      "description": "Property to specify whether Azure Resource Manager is permitted to retrieve secrets from the key vault.",
///      "type": "boolean"
///    },
///    "networkAcls": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "publicNetworkAccess": {
///      "description": "Property to specify whether the vault will accept traffic from public internet. If set to 'disabled' all traffic except private endpoint traffic and that that originates from trusted services will be blocked. This will override the set firewall rules, meaning that even if the firewall rules are present we will not honor the rules.",
///      "type": "string"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    },
///    "softDeleteRetentionInDays": {
///      "description": "softDelete data retention days. It accepts >=7 and <=90.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "tenantId": {
///      "description": "The Azure Active Directory tenant ID that should be used for authenticating requests to the key vault.",
///      "type": "string",
///      "format": "uuid"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultPatchProperties {
    ///An array of 0 to 16 identities that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID.
    #[serde(
        rename = "accessPolicies",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_policies: ::std::vec::Vec<AccessPolicyEntry>,
    ///The vault's create mode to indicate whether the vault need to be recovered or not.
    #[serde(
        rename = "createMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub create_mode: ::std::option::Option<VaultPatchPropertiesCreateMode>,
    ///Property specifying whether protection against purge is enabled for this vault. Setting this property to true activates protection against purge for this vault and its content - only the Key Vault service may initiate a hard, irrecoverable deletion. The setting is effective only if soft delete is also enabled. Enabling this functionality is irreversible - that is, the property does not accept false as its value.
    #[serde(
        rename = "enablePurgeProtection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_purge_protection: ::std::option::Option<bool>,
    ///Property that controls how data actions are authorized. When true, the key vault will use Role Based Access Control (RBAC) for authorization of data actions, and the access policies specified in vault properties will be  ignored. When false, the key vault will use the access policies specified in vault properties, and any policy stored on Azure Resource Manager will be ignored. If null or not specified, the value of this property will not change.
    #[serde(
        rename = "enableRbacAuthorization",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_rbac_authorization: ::std::option::Option<bool>,
    ///Property to specify whether the 'soft delete' functionality is enabled for this key vault. Once set to true, it cannot be reverted to false.
    #[serde(
        rename = "enableSoftDelete",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_soft_delete: ::std::option::Option<bool>,
    ///Property to specify whether Azure Virtual Machines are permitted to retrieve certificates stored as secrets from the key vault.
    #[serde(
        rename = "enabledForDeployment",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_for_deployment: ::std::option::Option<bool>,
    ///Property to specify whether Azure Disk Encryption is permitted to retrieve secrets from the vault and unwrap keys.
    #[serde(
        rename = "enabledForDiskEncryption",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_for_disk_encryption: ::std::option::Option<bool>,
    ///Property to specify whether Azure Resource Manager is permitted to retrieve secrets from the key vault.
    #[serde(
        rename = "enabledForTemplateDeployment",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_for_template_deployment: ::std::option::Option<bool>,
    #[serde(
        rename = "networkAcls",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_acls: ::std::option::Option<NetworkRuleSet>,
    ///Property to specify whether the vault will accept traffic from public internet. If set to 'disabled' all traffic except private endpoint traffic and that that originates from trusted services will be blocked. This will override the set firewall rules, meaning that even if the firewall rules are present we will not honor the rules.
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
    ///softDelete data retention days. It accepts >=7 and <=90.
    #[serde(
        rename = "softDeleteRetentionInDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub soft_delete_retention_in_days: ::std::option::Option<i32>,
    ///The Azure Active Directory tenant ID that should be used for authenticating requests to the key vault.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::uuid::Uuid>,
}
impl ::std::convert::From<&VaultPatchProperties> for VaultPatchProperties {
    fn from(value: &VaultPatchProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VaultPatchProperties {
    fn default() -> Self {
        Self {
            access_policies: Default::default(),
            create_mode: Default::default(),
            enable_purge_protection: Default::default(),
            enable_rbac_authorization: Default::default(),
            enable_soft_delete: Default::default(),
            enabled_for_deployment: Default::default(),
            enabled_for_disk_encryption: Default::default(),
            enabled_for_template_deployment: Default::default(),
            network_acls: Default::default(),
            public_network_access: Default::default(),
            sku: Default::default(),
            soft_delete_retention_in_days: Default::default(),
            tenant_id: Default::default(),
        }
    }
}
///The vault's create mode to indicate whether the vault need to be recovered or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The vault's create mode to indicate whether the vault need to be recovered or not.",
///  "type": "string",
///  "enum": [
///    "recover",
///    "default"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "CreateMode"
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
pub enum VaultPatchPropertiesCreateMode {
    #[serde(rename = "recover")]
    Recover,
    #[serde(rename = "default")]
    Default,
}
impl ::std::convert::From<&Self> for VaultPatchPropertiesCreateMode {
    fn from(value: &VaultPatchPropertiesCreateMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VaultPatchPropertiesCreateMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Recover => f.write_str("recover"),
            Self::Default => f.write_str("default"),
        }
    }
}
impl ::std::str::FromStr for VaultPatchPropertiesCreateMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "recover" => Ok(Self::Recover),
            "default" => Ok(Self::Default),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VaultPatchPropertiesCreateMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VaultPatchPropertiesCreateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VaultPatchPropertiesCreateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Properties of the vault
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the vault",
///  "type": "object",
///  "required": [
///    "sku",
///    "tenantId"
///  ],
///  "properties": {
///    "accessPolicies": {
///      "description": "An array of 0 to 1024 identities that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID. When `createMode` is set to `recover`, access policies are not required. Otherwise, access policies are required.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AccessPolicyEntry"
///      },
///      "x-ms-identifiers": [
///        "tenantId",
///        "objectId",
///        "permissions"
///      ]
///    },
///    "createMode": {
///      "description": "The vault's create mode to indicate whether the vault need to be recovered or not.",
///      "type": "string",
///      "enum": [
///        "recover",
///        "default"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "CreateMode"
///      },
///      "x-ms-mutability": [
///        "create",
///        "update"
///      ]
///    },
///    "enablePurgeProtection": {
///      "description": "Property specifying whether protection against purge is enabled for this vault. Setting this property to true activates protection against purge for this vault and its content - only the Key Vault service may initiate a hard, irrecoverable deletion. The setting is effective only if soft delete is also enabled. Enabling this functionality is irreversible - that is, the property does not accept false as its value.",
///      "type": "boolean"
///    },
///    "enableRbacAuthorization": {
///      "description": "Property that controls how data actions are authorized. When true, the key vault will use Role Based Access Control (RBAC) for authorization of data actions, and the access policies specified in vault properties will be  ignored. When false, the key vault will use the access policies specified in vault properties, and any policy stored on Azure Resource Manager will be ignored. If null or not specified, the vault is created with the default value of false. Note that management actions are always authorized with RBAC.",
///      "default": false,
///      "type": "boolean"
///    },
///    "enableSoftDelete": {
///      "description": "Property to specify whether the 'soft delete' functionality is enabled for this key vault. If it's not set to any value(true or false) when creating new key vault, it will be set to true by default. Once set to true, it cannot be reverted to false.",
///      "default": true,
///      "type": "boolean"
///    },
///    "enabledForDeployment": {
///      "description": "Property to specify whether Azure Virtual Machines are permitted to retrieve certificates stored as secrets from the key vault.",
///      "default": false,
///      "type": "boolean"
///    },
///    "enabledForDiskEncryption": {
///      "description": "Property to specify whether Azure Disk Encryption is permitted to retrieve secrets from the vault and unwrap keys.",
///      "default": false,
///      "type": "boolean"
///    },
///    "enabledForTemplateDeployment": {
///      "description": "Property to specify whether Azure Resource Manager is permitted to retrieve secrets from the key vault.",
///      "default": false,
///      "type": "boolean"
///    },
///    "hsmPoolResourceId": {
///      "description": "The resource id of HSM Pool.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "networkAcls": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "privateEndpointConnections": {
///      "description": "List of private endpoint connections associated with the key vault.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnectionItem"
///      }
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the vault.",
///      "type": "string",
///      "enum": [
///        "Succeeded",
///        "RegisteringDns"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "VaultProvisioningState"
///      }
///    },
///    "publicNetworkAccess": {
///      "description": "Property to specify whether the vault will accept traffic from public internet. If set to 'disabled' all traffic except private endpoint traffic and that that originates from trusted services will be blocked. This will override the set firewall rules, meaning that even if the firewall rules are present we will not honor the rules.",
///      "default": "enabled",
///      "type": "string"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    },
///    "softDeleteRetentionInDays": {
///      "description": "softDelete data retention days. It accepts >=7 and <=90.",
///      "default": 90,
///      "type": "integer",
///      "format": "int32"
///    },
///    "tenantId": {
///      "description": "The Azure Active Directory tenant ID that should be used for authenticating requests to the key vault.",
///      "type": "string",
///      "format": "uuid"
///    },
///    "vaultUri": {
///      "description": "The URI of the vault for performing operations on keys and secrets.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VaultProperties {
    ///An array of 0 to 1024 identities that have access to the key vault. All identities in the array must use the same tenant ID as the key vault's tenant ID. When `createMode` is set to `recover`, access policies are not required. Otherwise, access policies are required.
    #[serde(
        rename = "accessPolicies",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_policies: ::std::vec::Vec<AccessPolicyEntry>,
    ///The vault's create mode to indicate whether the vault need to be recovered or not.
    #[serde(
        rename = "createMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub create_mode: ::std::option::Option<VaultPropertiesCreateMode>,
    ///Property specifying whether protection against purge is enabled for this vault. Setting this property to true activates protection against purge for this vault and its content - only the Key Vault service may initiate a hard, irrecoverable deletion. The setting is effective only if soft delete is also enabled. Enabling this functionality is irreversible - that is, the property does not accept false as its value.
    #[serde(
        rename = "enablePurgeProtection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_purge_protection: ::std::option::Option<bool>,
    ///Property that controls how data actions are authorized. When true, the key vault will use Role Based Access Control (RBAC) for authorization of data actions, and the access policies specified in vault properties will be  ignored. When false, the key vault will use the access policies specified in vault properties, and any policy stored on Azure Resource Manager will be ignored. If null or not specified, the vault is created with the default value of false. Note that management actions are always authorized with RBAC.
    #[serde(
        rename = "enableRbacAuthorization",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_rbac_authorization: bool,
    ///Property to specify whether the 'soft delete' functionality is enabled for this key vault. If it's not set to any value(true or false) when creating new key vault, it will be set to true by default. Once set to true, it cannot be reverted to false.
    #[serde(
        rename = "enableSoftDelete",
        default = "defaults::default_bool::<true>",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_soft_delete: bool,
    ///Property to specify whether Azure Virtual Machines are permitted to retrieve certificates stored as secrets from the key vault.
    #[serde(
        rename = "enabledForDeployment",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_for_deployment: bool,
    ///Property to specify whether Azure Disk Encryption is permitted to retrieve secrets from the vault and unwrap keys.
    #[serde(
        rename = "enabledForDiskEncryption",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_for_disk_encryption: bool,
    ///Property to specify whether Azure Resource Manager is permitted to retrieve secrets from the key vault.
    #[serde(
        rename = "enabledForTemplateDeployment",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_for_template_deployment: bool,
    ///The resource id of HSM Pool.
    #[serde(
        rename = "hsmPoolResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub hsm_pool_resource_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "networkAcls",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_acls: ::std::option::Option<NetworkRuleSet>,
    ///List of private endpoint connections associated with the key vault.
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnectionItem>,
    ///Provisioning state of the vault.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<VaultPropertiesProvisioningState>,
    ///Property to specify whether the vault will accept traffic from public internet. If set to 'disabled' all traffic except private endpoint traffic and that that originates from trusted services will be blocked. This will override the set firewall rules, meaning that even if the firewall rules are present we will not honor the rules.
    #[serde(
        rename = "publicNetworkAccess",
        default = "defaults::vault_properties_public_network_access",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::string::String,
    pub sku: Sku,
    ///softDelete data retention days. It accepts >=7 and <=90.
    #[serde(
        rename = "softDeleteRetentionInDays",
        default = "defaults::default_u64::<i32, 90>",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub soft_delete_retention_in_days: i32,
    ///The Azure Active Directory tenant ID that should be used for authenticating requests to the key vault.
    #[serde(rename = "tenantId")]
    pub tenant_id: ::uuid::Uuid,
    ///The URI of the vault for performing operations on keys and secrets.
    #[serde(
        rename = "vaultUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub vault_uri: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&VaultProperties> for VaultProperties {
    fn from(value: &VaultProperties) -> Self {
        value.clone()
    }
}
///The vault's create mode to indicate whether the vault need to be recovered or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The vault's create mode to indicate whether the vault need to be recovered or not.",
///  "type": "string",
///  "enum": [
///    "recover",
///    "default"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "CreateMode"
///  },
///  "x-ms-mutability": [
///    "create",
///    "update"
///  ]
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
pub enum VaultPropertiesCreateMode {
    #[serde(rename = "recover")]
    Recover,
    #[serde(rename = "default")]
    Default,
}
impl ::std::convert::From<&Self> for VaultPropertiesCreateMode {
    fn from(value: &VaultPropertiesCreateMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VaultPropertiesCreateMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Recover => f.write_str("recover"),
            Self::Default => f.write_str("default"),
        }
    }
}
impl ::std::str::FromStr for VaultPropertiesCreateMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "recover" => Ok(Self::Recover),
            "default" => Ok(Self::Default),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VaultPropertiesCreateMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VaultPropertiesCreateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VaultPropertiesCreateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Provisioning state of the vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the vault.",
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "RegisteringDns"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "VaultProvisioningState"
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
pub enum VaultPropertiesProvisioningState {
    Succeeded,
    RegisteringDns,
}
impl ::std::convert::From<&Self> for VaultPropertiesProvisioningState {
    fn from(value: &VaultPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VaultPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::RegisteringDns => f.write_str("RegisteringDns"),
        }
    }
}
impl ::std::str::FromStr for VaultPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "registeringdns" => Ok(Self::RegisteringDns),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VaultPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VaultPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VaultPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A rule governing the accessibility of a vault from a specific virtual network.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A rule governing the accessibility of a vault from a specific virtual network.",
///  "type": "object",
///  "required": [
///    "id"
///  ],
///  "properties": {
///    "id": {
///      "description": "Full resource id of a vnet subnet, such as '/subscriptions/subid/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/test-vnet/subnets/subnet1'.",
///      "type": "string"
///    },
///    "ignoreMissingVnetServiceEndpoint": {
///      "description": "Property to specify whether NRP will ignore the check if parent subnet has serviceEndpoints configured.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VirtualNetworkRule {
    ///Full resource id of a vnet subnet, such as '/subscriptions/subid/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/test-vnet/subnets/subnet1'.
    pub id: ::std::string::String,
    ///Property to specify whether NRP will ignore the check if parent subnet has serviceEndpoints configured.
    #[serde(
        rename = "ignoreMissingVnetServiceEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ignore_missing_vnet_service_endpoint: ::std::option::Option<bool>,
}
impl ::std::convert::From<&VirtualNetworkRule> for VirtualNetworkRule {
    fn from(value: &VirtualNetworkRule) -> Self {
        value.clone()
    }
}
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn default_bool<const V: bool>() -> bool {
        V
    }
    pub(super) fn default_u64<T, const V: u64>() -> T
    where
        T: ::std::convert::TryFrom<u64>,
        <T as ::std::convert::TryFrom<u64>>::Error: ::std::fmt::Debug,
    {
        T::try_from(V).unwrap()
    }
    pub(super) fn vault_properties_public_network_access() -> ::std::string::String {
        "enabled".to_string()
    }
}
