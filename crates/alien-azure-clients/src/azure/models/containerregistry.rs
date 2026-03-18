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
///The activation properties of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The activation properties of the connected registry.",
///  "type": "object",
///  "properties": {
///    "status": {
///      "description": "The activation status of the connected registry.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Active",
///        "Inactive"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ActivationStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ActivationProperties {
    ///The activation status of the connected registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<ActivationPropertiesStatus>,
}
impl ::std::convert::From<&ActivationProperties> for ActivationProperties {
    fn from(value: &ActivationProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ActivationProperties {
    fn default() -> Self {
        Self {
            status: Default::default(),
        }
    }
}
///The activation status of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The activation status of the connected registry.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Active",
///    "Inactive"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ActivationStatus"
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
pub enum ActivationPropertiesStatus {
    Active,
    Inactive,
}
impl ::std::convert::From<&Self> for ActivationPropertiesStatus {
    fn from(value: &ActivationPropertiesStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ActivationPropertiesStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Active => f.write_str("Active"),
            Self::Inactive => f.write_str("Inactive"),
        }
    }
}
impl ::std::str::FromStr for ActivationPropertiesStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ActivationPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ActivationPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ActivationPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The Active Directory Object that will be used for authenticating the token of a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Active Directory Object that will be used for authenticating the token of a container registry.",
///  "type": "object",
///  "properties": {
///    "objectId": {
///      "description": "The user/group/application object ID for Active Directory Object that will be used for authenticating the token of a container registry.",
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "The tenant ID of user/group/application object Active Directory Object that will be used for authenticating the token of a container registry.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ActiveDirectoryObject {
    ///The user/group/application object ID for Active Directory Object that will be used for authenticating the token of a container registry.
    #[serde(
        rename = "objectId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub object_id: ::std::option::Option<::std::string::String>,
    ///The tenant ID of user/group/application object Active Directory Object that will be used for authenticating the token of a container registry.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ActiveDirectoryObject> for ActiveDirectoryObject {
    fn from(value: &ActiveDirectoryObject) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ActiveDirectoryObject {
    fn default() -> Self {
        Self {
            object_id: Default::default(),
            tenant_id: Default::default(),
        }
    }
}
///The agent that initiated the event. For most situations, this could be from the authorization context of the request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The agent that initiated the event. For most situations, this could be from the authorization context of the request.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "The subject or username associated with the request context that generated the event.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Actor {
    ///The subject or username associated with the request context that generated the event.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Actor> for Actor {
    fn from(value: &Actor) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Actor {
    fn default() -> Self {
        Self {
            name: Default::default(),
        }
    }
}
///Authentication credential stored for an upstream.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Authentication credential stored for an upstream.",
///  "type": "object",
///  "properties": {
///    "credentialHealth": {
///      "$ref": "#/components/schemas/CredentialHealth"
///    },
///    "name": {
///      "description": "The name of the credential.",
///      "type": "string",
///      "enum": [
///        "Credential1"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "CredentialName"
///      }
///    },
///    "passwordSecretIdentifier": {
///      "description": "KeyVault Secret URI for accessing the password.",
///      "type": "string"
///    },
///    "usernameSecretIdentifier": {
///      "description": "KeyVault Secret URI for accessing the username.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AuthCredential {
    #[serde(
        rename = "credentialHealth",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credential_health: ::std::option::Option<CredentialHealth>,
    ///The name of the credential.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<AuthCredentialName>,
    ///KeyVault Secret URI for accessing the password.
    #[serde(
        rename = "passwordSecretIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub password_secret_identifier: ::std::option::Option<::std::string::String>,
    ///KeyVault Secret URI for accessing the username.
    #[serde(
        rename = "usernameSecretIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub username_secret_identifier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&AuthCredential> for AuthCredential {
    fn from(value: &AuthCredential) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AuthCredential {
    fn default() -> Self {
        Self {
            credential_health: Default::default(),
            name: Default::default(),
            password_secret_identifier: Default::default(),
            username_secret_identifier: Default::default(),
        }
    }
}
///The name of the credential.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The name of the credential.",
///  "type": "string",
///  "enum": [
///    "Credential1"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CredentialName"
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
pub enum AuthCredentialName {
    Credential1,
}
impl ::std::convert::From<&Self> for AuthCredentialName {
    fn from(value: &AuthCredentialName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AuthCredentialName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Credential1 => f.write_str("Credential1"),
        }
    }
}
impl ::std::str::FromStr for AuthCredentialName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "credential1" => Ok(Self::Credential1),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AuthCredentialName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AuthCredentialName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AuthCredentialName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The policy for using ARM audience token for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The policy for using ARM audience token for a container registry.",
///  "type": "object",
///  "properties": {
///    "status": {
///      "description": "The value that indicates whether the policy is enabled or not.",
///      "default": "enabled",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AzureADAuthenticationAsArmPolicyStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AzureAdAuthenticationAsArmPolicy {
    ///The value that indicates whether the policy is enabled or not.
    #[serde(
        default = "defaults::azure_ad_authentication_as_arm_policy_status",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: AzureAdAuthenticationAsArmPolicyStatus,
}
impl ::std::convert::From<&AzureAdAuthenticationAsArmPolicy> for AzureAdAuthenticationAsArmPolicy {
    fn from(value: &AzureAdAuthenticationAsArmPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AzureAdAuthenticationAsArmPolicy {
    fn default() -> Self {
        Self {
            status: defaults::azure_ad_authentication_as_arm_policy_status(),
        }
    }
}
///The value that indicates whether the policy is enabled or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The value that indicates whether the policy is enabled or not.",
///  "default": "enabled",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AzureADAuthenticationAsArmPolicyStatus"
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
pub enum AzureAdAuthenticationAsArmPolicyStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for AzureAdAuthenticationAsArmPolicyStatus {
    fn from(value: &AzureAdAuthenticationAsArmPolicyStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AzureAdAuthenticationAsArmPolicyStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for AzureAdAuthenticationAsArmPolicyStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AzureAdAuthenticationAsArmPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AzureAdAuthenticationAsArmPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AzureAdAuthenticationAsArmPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for AzureAdAuthenticationAsArmPolicyStatus {
    fn default() -> Self {
        AzureAdAuthenticationAsArmPolicyStatus::Enabled
    }
}
///An object that represents a cache rule for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a cache rule for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/CacheRuleProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CacheRule {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<CacheRuleProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CacheRule> for CacheRule {
    fn from(value: &CacheRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CacheRule {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The properties of a cache rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a cache rule.",
///  "type": "object",
///  "properties": {
///    "creationDate": {
///      "description": "The creation date of the cache rule.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "credentialSetResourceId": {
///      "description": "The ARM resource ID of the credential store which is associated with the cache rule.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the resource.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "sourceRepository": {
///      "description": "Source repository pulled from upstream.",
///      "type": "string"
///    },
///    "targetRepository": {
///      "description": "Target repository specified in docker pull command.\r\nEg: docker pull myregistry.azurecr.io/{targetRepository}:{tag}",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CacheRuleProperties {
    ///The creation date of the cache rule.
    #[serde(
        rename = "creationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_date: ::std::option::Option<::std::string::String>,
    ///The ARM resource ID of the credential store which is associated with the cache rule.
    #[serde(
        rename = "credentialSetResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credential_set_resource_id: ::std::option::Option<::std::string::String>,
    ///Provisioning state of the resource.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<CacheRulePropertiesProvisioningState>,
    ///Source repository pulled from upstream.
    #[serde(
        rename = "sourceRepository",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_repository: ::std::option::Option<::std::string::String>,
    /**Target repository specified in docker pull command.
    Eg: docker pull myregistry.azurecr.io/{targetRepository}:{tag}*/
    #[serde(
        rename = "targetRepository",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_repository: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CacheRuleProperties> for CacheRuleProperties {
    fn from(value: &CacheRuleProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CacheRuleProperties {
    fn default() -> Self {
        Self {
            creation_date: Default::default(),
            credential_set_resource_id: Default::default(),
            provisioning_state: Default::default(),
            source_repository: Default::default(),
            target_repository: Default::default(),
        }
    }
}
///Provisioning state of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the resource.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum CacheRulePropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for CacheRulePropertiesProvisioningState {
    fn from(value: &CacheRulePropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CacheRulePropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for CacheRulePropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CacheRulePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CacheRulePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CacheRulePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for updating a cache rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a cache rule.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/CacheRuleUpdateProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CacheRuleUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<CacheRuleUpdateProperties>,
}
impl ::std::convert::From<&CacheRuleUpdateParameters> for CacheRuleUpdateParameters {
    fn from(value: &CacheRuleUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CacheRuleUpdateParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
        }
    }
}
///The parameters for updating cache rule properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating cache rule properties.",
///  "type": "object",
///  "properties": {
///    "credentialSetResourceId": {
///      "description": "The ARM resource ID of the credential store which is associated with the Cache rule.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CacheRuleUpdateProperties {
    ///The ARM resource ID of the credential store which is associated with the Cache rule.
    #[serde(
        rename = "credentialSetResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credential_set_resource_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CacheRuleUpdateProperties> for CacheRuleUpdateProperties {
    fn from(value: &CacheRuleUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CacheRuleUpdateProperties {
    fn default() -> Self {
        Self {
            credential_set_resource_id: Default::default(),
        }
    }
}
///The result of a request to list cache rules for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list cache rules for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "If provided, client must use NextLink URI to request next list of cache rules.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of cache rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CacheRule"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CacheRulesListResult {
    ///If provided, client must use NextLink URI to request next list of cache rules.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of cache rules.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<CacheRule>,
}
impl ::std::convert::From<&CacheRulesListResult> for CacheRulesListResult {
    fn from(value: &CacheRulesListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CacheRulesListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The configuration of service URI and custom headers for the webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The configuration of service URI and custom headers for the webhook.",
///  "type": "object",
///  "required": [
///    "serviceUri"
///  ],
///  "properties": {
///    "customHeaders": {
///      "description": "Custom headers that will be added to the webhook notifications.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "serviceUri": {
///      "description": "The service URI for the webhook to post notifications.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CallbackConfig {
    ///Custom headers that will be added to the webhook notifications.
    #[serde(
        rename = "customHeaders",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_headers: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The service URI for the webhook to post notifications.
    #[serde(rename = "serviceUri")]
    pub service_uri: ::std::string::String,
}
impl ::std::convert::From<&CallbackConfig> for CallbackConfig {
    fn from(value: &CallbackConfig) -> Self {
        value.clone()
    }
}
///An object that represents a connected registry for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a connected registry for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ConnectedRegistryProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ConnectedRegistry {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<ConnectedRegistryProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ConnectedRegistry> for ConnectedRegistry {
    fn from(value: &ConnectedRegistry) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ConnectedRegistry {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The result of a request to list connected registries for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list connected registries for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of connected registries.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of connected registries. Since this list may be incomplete, the nextLink field should be used to request the next list of connected registries.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ConnectedRegistry"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ConnectedRegistryListResult {
    ///The URI that can be used to request the next list of connected registries.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of connected registries. Since this list may be incomplete, the nextLink field should be used to request the next list of connected registries.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ConnectedRegistry>,
}
impl ::std::convert::From<&ConnectedRegistryListResult> for ConnectedRegistryListResult {
    fn from(value: &ConnectedRegistryListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ConnectedRegistryListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a connected registry.",
///  "type": "object",
///  "required": [
///    "mode",
///    "parent"
///  ],
///  "properties": {
///    "activation": {
///      "$ref": "#/components/schemas/ActivationProperties"
///    },
///    "clientTokenIds": {
///      "description": "The list of the ACR token resource IDs used to authenticate clients to the connected registry.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "connectionState": {
///      "description": "The current connection state of the connected registry.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Online",
///        "Offline",
///        "Syncing",
///        "Unhealthy"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ConnectionState"
///      }
///    },
///    "garbageCollection": {
///      "$ref": "#/components/schemas/GarbageCollectionProperties"
///    },
///    "lastActivityTime": {
///      "description": "The last activity time of the connected registry.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "logging": {
///      "$ref": "#/components/schemas/LoggingProperties"
///    },
///    "loginServer": {
///      "$ref": "#/components/schemas/LoginServerProperties"
///    },
///    "mode": {
///      "description": "The mode of the connected registry resource that indicates the permissions of the registry.",
///      "type": "string",
///      "enum": [
///        "ReadWrite",
///        "ReadOnly",
///        "Registry",
///        "Mirror"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ConnectedRegistryMode"
///      }
///    },
///    "notificationsList": {
///      "description": "The list of notifications subscription information for the connected registry.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "parent": {
///      "$ref": "#/components/schemas/ParentProperties"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the resource.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "statusDetails": {
///      "description": "The list of current statuses of the connected registry.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/StatusDetailProperties"
///      },
///      "x-ms-identifiers": [
///        "correlationId"
///      ]
///    },
///    "version": {
///      "description": "The current version of ACR runtime on the connected registry.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ConnectedRegistryProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub activation: ::std::option::Option<ActivationProperties>,
    ///The list of the ACR token resource IDs used to authenticate clients to the connected registry.
    #[serde(
        rename = "clientTokenIds",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_token_ids: ::std::vec::Vec<::std::string::String>,
    ///The current connection state of the connected registry.
    #[serde(
        rename = "connectionState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub connection_state: ::std::option::Option<ConnectedRegistryPropertiesConnectionState>,
    #[serde(
        rename = "garbageCollection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub garbage_collection: ::std::option::Option<GarbageCollectionProperties>,
    ///The last activity time of the connected registry.
    #[serde(
        rename = "lastActivityTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_activity_time: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub logging: ::std::option::Option<LoggingProperties>,
    #[serde(
        rename = "loginServer",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub login_server: ::std::option::Option<LoginServerProperties>,
    ///The mode of the connected registry resource that indicates the permissions of the registry.
    pub mode: ConnectedRegistryPropertiesMode,
    ///The list of notifications subscription information for the connected registry.
    #[serde(
        rename = "notificationsList",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub notifications_list: ::std::vec::Vec<::std::string::String>,
    pub parent: ParentProperties,
    ///Provisioning state of the resource.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ConnectedRegistryPropertiesProvisioningState>,
    ///The list of current statuses of the connected registry.
    #[serde(
        rename = "statusDetails",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status_details: ::std::vec::Vec<StatusDetailProperties>,
    ///The current version of ACR runtime on the connected registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ConnectedRegistryProperties> for ConnectedRegistryProperties {
    fn from(value: &ConnectedRegistryProperties) -> Self {
        value.clone()
    }
}
///The current connection state of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The current connection state of the connected registry.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Online",
///    "Offline",
///    "Syncing",
///    "Unhealthy"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ConnectionState"
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
pub enum ConnectedRegistryPropertiesConnectionState {
    Online,
    Offline,
    Syncing,
    Unhealthy,
}
impl ::std::convert::From<&Self> for ConnectedRegistryPropertiesConnectionState {
    fn from(value: &ConnectedRegistryPropertiesConnectionState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ConnectedRegistryPropertiesConnectionState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Online => f.write_str("Online"),
            Self::Offline => f.write_str("Offline"),
            Self::Syncing => f.write_str("Syncing"),
            Self::Unhealthy => f.write_str("Unhealthy"),
        }
    }
}
impl ::std::str::FromStr for ConnectedRegistryPropertiesConnectionState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "online" => Ok(Self::Online),
            "offline" => Ok(Self::Offline),
            "syncing" => Ok(Self::Syncing),
            "unhealthy" => Ok(Self::Unhealthy),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ConnectedRegistryPropertiesConnectionState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for ConnectedRegistryPropertiesConnectionState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ConnectedRegistryPropertiesConnectionState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The mode of the connected registry resource that indicates the permissions of the registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The mode of the connected registry resource that indicates the permissions of the registry.",
///  "type": "string",
///  "enum": [
///    "ReadWrite",
///    "ReadOnly",
///    "Registry",
///    "Mirror"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ConnectedRegistryMode"
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
pub enum ConnectedRegistryPropertiesMode {
    ReadWrite,
    ReadOnly,
    Registry,
    Mirror,
}
impl ::std::convert::From<&Self> for ConnectedRegistryPropertiesMode {
    fn from(value: &ConnectedRegistryPropertiesMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ConnectedRegistryPropertiesMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::ReadWrite => f.write_str("ReadWrite"),
            Self::ReadOnly => f.write_str("ReadOnly"),
            Self::Registry => f.write_str("Registry"),
            Self::Mirror => f.write_str("Mirror"),
        }
    }
}
impl ::std::str::FromStr for ConnectedRegistryPropertiesMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "readwrite" => Ok(Self::ReadWrite),
            "readonly" => Ok(Self::ReadOnly),
            "registry" => Ok(Self::Registry),
            "mirror" => Ok(Self::Mirror),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ConnectedRegistryPropertiesMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ConnectedRegistryPropertiesMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ConnectedRegistryPropertiesMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Provisioning state of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the resource.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum ConnectedRegistryPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for ConnectedRegistryPropertiesProvisioningState {
    fn from(value: &ConnectedRegistryPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ConnectedRegistryPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for ConnectedRegistryPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ConnectedRegistryPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for ConnectedRegistryPropertiesProvisioningState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for ConnectedRegistryPropertiesProvisioningState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for updating a connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a connected registry.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ConnectedRegistryUpdateProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ConnectedRegistryUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<ConnectedRegistryUpdateProperties>,
}
impl ::std::convert::From<&ConnectedRegistryUpdateParameters>
    for ConnectedRegistryUpdateParameters
{
    fn from(value: &ConnectedRegistryUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ConnectedRegistryUpdateParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
        }
    }
}
///The parameters for updating token properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating token properties.",
///  "type": "object",
///  "properties": {
///    "clientTokenIds": {
///      "description": "The list of the ACR token resource IDs used to authenticate clients to the connected registry.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "garbageCollection": {
///      "$ref": "#/components/schemas/GarbageCollectionProperties"
///    },
///    "logging": {
///      "$ref": "#/components/schemas/LoggingProperties"
///    },
///    "notificationsList": {
///      "description": "The list of notifications subscription information for the connected registry.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "syncProperties": {
///      "$ref": "#/components/schemas/SyncUpdateProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ConnectedRegistryUpdateProperties {
    ///The list of the ACR token resource IDs used to authenticate clients to the connected registry.
    #[serde(
        rename = "clientTokenIds",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_token_ids: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "garbageCollection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub garbage_collection: ::std::option::Option<GarbageCollectionProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub logging: ::std::option::Option<LoggingProperties>,
    ///The list of notifications subscription information for the connected registry.
    #[serde(
        rename = "notificationsList",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub notifications_list: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "syncProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sync_properties: ::std::option::Option<SyncUpdateProperties>,
}
impl ::std::convert::From<&ConnectedRegistryUpdateProperties>
    for ConnectedRegistryUpdateProperties
{
    fn from(value: &ConnectedRegistryUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ConnectedRegistryUpdateProperties {
    fn default() -> Self {
        Self {
            client_token_ids: Default::default(),
            garbage_collection: Default::default(),
            logging: Default::default(),
            notifications_list: Default::default(),
            sync_properties: Default::default(),
        }
    }
}
///The health of the auth credential.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The health of the auth credential.",
///  "type": "object",
///  "properties": {
///    "errorCode": {
///      "description": "Error code representing the health check error.",
///      "type": "string"
///    },
///    "errorMessage": {
///      "description": "Descriptive message representing the health check error.",
///      "type": "string"
///    },
///    "status": {
///      "description": "The health status of credential.",
///      "type": "string",
///      "enum": [
///        "Healthy",
///        "Unhealthy"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "CredentialHealthStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CredentialHealth {
    ///Error code representing the health check error.
    #[serde(
        rename = "errorCode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error_code: ::std::option::Option<::std::string::String>,
    ///Descriptive message representing the health check error.
    #[serde(
        rename = "errorMessage",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error_message: ::std::option::Option<::std::string::String>,
    ///The health status of credential.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<CredentialHealthStatus>,
}
impl ::std::convert::From<&CredentialHealth> for CredentialHealth {
    fn from(value: &CredentialHealth) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CredentialHealth {
    fn default() -> Self {
        Self {
            error_code: Default::default(),
            error_message: Default::default(),
            status: Default::default(),
        }
    }
}
///The health status of credential.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The health status of credential.",
///  "type": "string",
///  "enum": [
///    "Healthy",
///    "Unhealthy"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CredentialHealthStatus"
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
pub enum CredentialHealthStatus {
    Healthy,
    Unhealthy,
}
impl ::std::convert::From<&Self> for CredentialHealthStatus {
    fn from(value: &CredentialHealthStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CredentialHealthStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Healthy => f.write_str("Healthy"),
            Self::Unhealthy => f.write_str("Unhealthy"),
        }
    }
}
impl ::std::str::FromStr for CredentialHealthStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "healthy" => Ok(Self::Healthy),
            "unhealthy" => Ok(Self::Unhealthy),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CredentialHealthStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CredentialHealthStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CredentialHealthStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An object that represents a credential set resource for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a credential set resource for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/IdentityProperties"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/CredentialSetProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CredentialSet {
    ///The resource ID.
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
    pub identity: ::std::option::Option<IdentityProperties>,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<CredentialSetProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CredentialSet> for CredentialSet {
    fn from(value: &CredentialSet) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CredentialSet {
    fn default() -> Self {
        Self {
            id: Default::default(),
            identity: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The result of a request to list credential sets for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list credential sets for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of credential sets.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of credential sets. Since this list may be incomplete, the nextLink field should be used to request the next list of credential sets.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CredentialSet"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CredentialSetListResult {
    ///The URI that can be used to request the next list of credential sets.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of credential sets. Since this list may be incomplete, the nextLink field should be used to request the next list of credential sets.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<CredentialSet>,
}
impl ::std::convert::From<&CredentialSetListResult> for CredentialSetListResult {
    fn from(value: &CredentialSetListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CredentialSetListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a credential set resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a credential set resource.",
///  "type": "object",
///  "properties": {
///    "authCredentials": {
///      "description": "List of authentication credentials stored for an upstream.\r\nUsually consists of a primary and an optional secondary credential.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AuthCredential"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "creationDate": {
///      "description": "The creation date of credential store resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "loginServer": {
///      "description": "The credentials are stored for this upstream or login server.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the resource.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CredentialSetProperties {
    /**List of authentication credentials stored for an upstream.
    Usually consists of a primary and an optional secondary credential.*/
    #[serde(
        rename = "authCredentials",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auth_credentials: ::std::vec::Vec<AuthCredential>,
    ///The creation date of credential store resource.
    #[serde(
        rename = "creationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_date: ::std::option::Option<::std::string::String>,
    ///The credentials are stored for this upstream or login server.
    #[serde(
        rename = "loginServer",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub login_server: ::std::option::Option<::std::string::String>,
    ///Provisioning state of the resource.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<CredentialSetPropertiesProvisioningState>,
}
impl ::std::convert::From<&CredentialSetProperties> for CredentialSetProperties {
    fn from(value: &CredentialSetProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CredentialSetProperties {
    fn default() -> Self {
        Self {
            auth_credentials: Default::default(),
            creation_date: Default::default(),
            login_server: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///Provisioning state of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the resource.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum CredentialSetPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for CredentialSetPropertiesProvisioningState {
    fn from(value: &CredentialSetPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CredentialSetPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for CredentialSetPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CredentialSetPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CredentialSetPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CredentialSetPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for updating a credential set
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a credential set",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/IdentityProperties"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/CredentialSetUpdateProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CredentialSetUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<IdentityProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<CredentialSetUpdateProperties>,
}
impl ::std::convert::From<&CredentialSetUpdateParameters> for CredentialSetUpdateParameters {
    fn from(value: &CredentialSetUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CredentialSetUpdateParameters {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            properties: Default::default(),
        }
    }
}
///The parameters for updating credential set properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating credential set properties.",
///  "type": "object",
///  "properties": {
///    "authCredentials": {
///      "description": "List of authentication credentials stored for an upstream.\r\nUsually consists of a primary and an optional secondary credential.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AuthCredential"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CredentialSetUpdateProperties {
    /**List of authentication credentials stored for an upstream.
    Usually consists of a primary and an optional secondary credential.*/
    #[serde(
        rename = "authCredentials",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auth_credentials: ::std::vec::Vec<AuthCredential>,
}
impl ::std::convert::From<&CredentialSetUpdateProperties> for CredentialSetUpdateProperties {
    fn from(value: &CredentialSetUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CredentialSetUpdateProperties {
    fn default() -> Self {
        Self {
            auth_credentials: Default::default(),
        }
    }
}
///`EncryptionProperty`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "keyVaultProperties": {
///      "$ref": "#/components/schemas/KeyVaultProperties"
///    },
///    "status": {
///      "description": "Indicates whether or not the encryption is enabled for container registry.",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "EncryptionStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionProperty {
    #[serde(
        rename = "keyVaultProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_vault_properties: ::std::option::Option<KeyVaultProperties>,
    ///Indicates whether or not the encryption is enabled for container registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<EncryptionPropertyStatus>,
}
impl ::std::convert::From<&EncryptionProperty> for EncryptionProperty {
    fn from(value: &EncryptionProperty) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionProperty {
    fn default() -> Self {
        Self {
            key_vault_properties: Default::default(),
            status: Default::default(),
        }
    }
}
///Indicates whether or not the encryption is enabled for container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates whether or not the encryption is enabled for container registry.",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "EncryptionStatus"
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
pub enum EncryptionPropertyStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for EncryptionPropertyStatus {
    fn from(value: &EncryptionPropertyStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionPropertyStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for EncryptionPropertyStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionPropertyStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionPropertyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionPropertyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The resource management error additional info.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The resource management error additional info.",
///  "type": "object",
///  "properties": {
///    "info": {
///      "description": "The additional info.",
///      "readOnly": true,
///      "type": "object"
///    },
///    "type": {
///      "description": "The additional info type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ErrorAdditionalInfo {
    ///The additional info.
    #[serde(
        default,
        skip_serializing_if = "::serde_json::Map::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub info: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///The additional info type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ErrorAdditionalInfo> for ErrorAdditionalInfo {
    fn from(value: &ErrorAdditionalInfo) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorAdditionalInfo {
    fn default() -> Self {
        Self {
            info: Default::default(),
            type_: Default::default(),
        }
    }
}
///The error detail.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The error detail.",
///  "type": "object",
///  "properties": {
///    "additionalInfo": {
///      "description": "The error additional info.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ErrorAdditionalInfo"
///      },
///      "x-ms-identifiers": []
///    },
///    "code": {
///      "description": "The error code.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "details": {
///      "description": "The error details.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ErrorDetail"
///      },
///      "x-ms-identifiers": [
///        "message",
///        "target"
///      ]
///    },
///    "message": {
///      "description": "The error message.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "target": {
///      "description": "The error target.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ErrorDetail {
    ///The error additional info.
    #[serde(
        rename = "additionalInfo",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub additional_info: ::std::vec::Vec<ErrorAdditionalInfo>,
    ///The error code.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///The error details.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub details: ::std::vec::Vec<ErrorDetail>,
    ///The error message.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///The error target.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ErrorDetail> for ErrorDetail {
    fn from(value: &ErrorDetail) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorDetail {
    fn default() -> Self {
        Self {
            additional_info: Default::default(),
            code: Default::default(),
            details: Default::default(),
            message: Default::default(),
            target: Default::default(),
        }
    }
}
///Common error response for all Azure Resource Manager APIs to return error details for failed operations. (This also follows the OData error response format.).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Error response",
///  "description": "Common error response for all Azure Resource Manager APIs to return error details for failed operations. (This also follows the OData error response format.).",
///  "type": "object",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/ErrorDetail"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ErrorResponse {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<ErrorDetail>,
}
impl ::std::convert::From<&ErrorResponse> for ErrorResponse {
    fn from(value: &ErrorResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorResponse {
    fn default() -> Self {
        Self {
            error: Default::default(),
        }
    }
}
///The event for a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The event for a webhook.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/EventInfo"
///    }
///  ],
///  "properties": {
///    "eventRequestMessage": {
///      "$ref": "#/components/schemas/EventRequestMessage"
///    },
///    "eventResponseMessage": {
///      "$ref": "#/components/schemas/EventResponseMessage"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Event {
    #[serde(
        rename = "eventRequestMessage",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_request_message: ::std::option::Option<EventRequestMessage>,
    #[serde(
        rename = "eventResponseMessage",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_response_message: ::std::option::Option<EventResponseMessage>,
    ///The event ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Event> for Event {
    fn from(value: &Event) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Event {
    fn default() -> Self {
        Self {
            event_request_message: Default::default(),
            event_response_message: Default::default(),
            id: Default::default(),
        }
    }
}
///The content of the event request message.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The content of the event request message.",
///  "type": "object",
///  "properties": {
///    "action": {
///      "description": "The action that encompasses the provided event.",
///      "type": "string"
///    },
///    "actor": {
///      "$ref": "#/components/schemas/Actor"
///    },
///    "id": {
///      "description": "The event ID.",
///      "type": "string"
///    },
///    "request": {
///      "$ref": "#/components/schemas/Request"
///    },
///    "source": {
///      "$ref": "#/components/schemas/Source"
///    },
///    "target": {
///      "$ref": "#/components/schemas/Target"
///    },
///    "timestamp": {
///      "description": "The time at which the event occurred.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EventContent {
    ///The action that encompasses the provided event.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub actor: ::std::option::Option<Actor>,
    ///The event ID.
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
    pub request: ::std::option::Option<Request>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source: ::std::option::Option<Source>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<Target>,
    ///The time at which the event occurred.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timestamp: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EventContent> for EventContent {
    fn from(value: &EventContent) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EventContent {
    fn default() -> Self {
        Self {
            action: Default::default(),
            actor: Default::default(),
            id: Default::default(),
            request: Default::default(),
            source: Default::default(),
            target: Default::default(),
            timestamp: Default::default(),
        }
    }
}
///The basic information of an event.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The basic information of an event.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The event ID.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EventInfo {
    ///The event ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EventInfo> for EventInfo {
    fn from(value: &EventInfo) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EventInfo {
    fn default() -> Self {
        Self {
            id: Default::default(),
        }
    }
}
///The result of a request to list events for a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list events for a webhook.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of events.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of events. Since this list may be incomplete, the nextLink field should be used to request the next list of events.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Event"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EventListResult {
    ///The URI that can be used to request the next list of events.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of events. Since this list may be incomplete, the nextLink field should be used to request the next list of events.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Event>,
}
impl ::std::convert::From<&EventListResult> for EventListResult {
    fn from(value: &EventListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EventListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The event request message sent to the service URI.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The event request message sent to the service URI.",
///  "type": "object",
///  "properties": {
///    "content": {
///      "$ref": "#/components/schemas/EventContent"
///    },
///    "headers": {
///      "description": "The headers of the event request message.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "method": {
///      "description": "The HTTP method used to send the event request message.",
///      "type": "string"
///    },
///    "requestUri": {
///      "description": "The URI used to send the event request message.",
///      "type": "string"
///    },
///    "version": {
///      "description": "The HTTP message version.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EventRequestMessage {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content: ::std::option::Option<EventContent>,
    ///The headers of the event request message.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub headers: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The HTTP method used to send the event request message.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub method: ::std::option::Option<::std::string::String>,
    ///The URI used to send the event request message.
    #[serde(
        rename = "requestUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub request_uri: ::std::option::Option<::std::string::String>,
    ///The HTTP message version.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EventRequestMessage> for EventRequestMessage {
    fn from(value: &EventRequestMessage) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EventRequestMessage {
    fn default() -> Self {
        Self {
            content: Default::default(),
            headers: Default::default(),
            method: Default::default(),
            request_uri: Default::default(),
            version: Default::default(),
        }
    }
}
///The event response message received from the service URI.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The event response message received from the service URI.",
///  "type": "object",
///  "properties": {
///    "content": {
///      "description": "The content of the event response message.",
///      "type": "string"
///    },
///    "headers": {
///      "description": "The headers of the event response message.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "reasonPhrase": {
///      "description": "The reason phrase of the event response message.",
///      "type": "string"
///    },
///    "statusCode": {
///      "description": "The status code of the event response message.",
///      "type": "string"
///    },
///    "version": {
///      "description": "The HTTP message version.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EventResponseMessage {
    ///The content of the event response message.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content: ::std::option::Option<::std::string::String>,
    ///The headers of the event response message.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub headers: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The reason phrase of the event response message.
    #[serde(
        rename = "reasonPhrase",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reason_phrase: ::std::option::Option<::std::string::String>,
    ///The status code of the event response message.
    #[serde(
        rename = "statusCode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status_code: ::std::option::Option<::std::string::String>,
    ///The HTTP message version.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EventResponseMessage> for EventResponseMessage {
    fn from(value: &EventResponseMessage) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EventResponseMessage {
    fn default() -> Self {
        Self {
            content: Default::default(),
            headers: Default::default(),
            reason_phrase: Default::default(),
            status_code: Default::default(),
            version: Default::default(),
        }
    }
}
///The export policy for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The export policy for a container registry.",
///  "type": "object",
///  "properties": {
///    "status": {
///      "description": "The value that indicates whether the policy is enabled or not.",
///      "default": "enabled",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ExportPolicyStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ExportPolicy {
    ///The value that indicates whether the policy is enabled or not.
    #[serde(
        default = "defaults::export_policy_status",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ExportPolicyStatus,
}
impl ::std::convert::From<&ExportPolicy> for ExportPolicy {
    fn from(value: &ExportPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ExportPolicy {
    fn default() -> Self {
        Self {
            status: defaults::export_policy_status(),
        }
    }
}
///The value that indicates whether the policy is enabled or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The value that indicates whether the policy is enabled or not.",
///  "default": "enabled",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ExportPolicyStatus"
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
pub enum ExportPolicyStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for ExportPolicyStatus {
    fn from(value: &ExportPolicyStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ExportPolicyStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for ExportPolicyStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExportPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExportPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExportPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for ExportPolicyStatus {
    fn default() -> Self {
        ExportPolicyStatus::Enabled
    }
}
///The garbage collection properties of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The garbage collection properties of the connected registry.",
///  "type": "object",
///  "properties": {
///    "enabled": {
///      "description": "Indicates whether garbage collection is enabled for the connected registry.",
///      "type": "boolean"
///    },
///    "schedule": {
///      "description": "The cron expression indicating the schedule that the connected registry will run garbage collection.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GarbageCollectionProperties {
    ///Indicates whether garbage collection is enabled for the connected registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///The cron expression indicating the schedule that the connected registry will run garbage collection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub schedule: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&GarbageCollectionProperties> for GarbageCollectionProperties {
    fn from(value: &GarbageCollectionProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GarbageCollectionProperties {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
            schedule: Default::default(),
        }
    }
}
///The parameters used to generate credentials for a specified token or user of a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used to generate credentials for a specified token or user of a container registry.",
///  "type": "object",
///  "properties": {
///    "expiry": {
///      "description": "The expiry date of the generated credentials after which the credentials become invalid.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Specifies name of the password which should be regenerated if any -- password1 or password2.",
///      "type": "string",
///      "enum": [
///        "password1",
///        "password2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TokenPasswordName"
///      }
///    },
///    "tokenId": {
///      "description": "The resource ID of the token for which credentials have to be generated.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GenerateCredentialsParameters {
    ///The expiry date of the generated credentials after which the credentials become invalid.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expiry: ::std::option::Option<::std::string::String>,
    ///Specifies name of the password which should be regenerated if any -- password1 or password2.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<GenerateCredentialsParametersName>,
    ///The resource ID of the token for which credentials have to be generated.
    #[serde(
        rename = "tokenId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub token_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&GenerateCredentialsParameters> for GenerateCredentialsParameters {
    fn from(value: &GenerateCredentialsParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GenerateCredentialsParameters {
    fn default() -> Self {
        Self {
            expiry: Default::default(),
            name: Default::default(),
            token_id: Default::default(),
        }
    }
}
///Specifies name of the password which should be regenerated if any -- password1 or password2.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies name of the password which should be regenerated if any -- password1 or password2.",
///  "type": "string",
///  "enum": [
///    "password1",
///    "password2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TokenPasswordName"
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
pub enum GenerateCredentialsParametersName {
    #[serde(rename = "password1")]
    Password1,
    #[serde(rename = "password2")]
    Password2,
}
impl ::std::convert::From<&Self> for GenerateCredentialsParametersName {
    fn from(value: &GenerateCredentialsParametersName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for GenerateCredentialsParametersName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Password1 => f.write_str("password1"),
            Self::Password2 => f.write_str("password2"),
        }
    }
}
impl ::std::str::FromStr for GenerateCredentialsParametersName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "password1" => Ok(Self::Password1),
            "password2" => Ok(Self::Password2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for GenerateCredentialsParametersName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GenerateCredentialsParametersName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GenerateCredentialsParametersName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The response from the GenerateCredentials operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the GenerateCredentials operation.",
///  "type": "object",
///  "properties": {
///    "passwords": {
///      "description": "The list of passwords for a container registry.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TokenPassword"
///      },
///      "x-ms-identifiers": []
///    },
///    "username": {
///      "description": "The username for a container registry.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GenerateCredentialsResult {
    ///The list of passwords for a container registry.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub passwords: ::std::vec::Vec<TokenPassword>,
    ///The username for a container registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub username: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&GenerateCredentialsResult> for GenerateCredentialsResult {
    fn from(value: &GenerateCredentialsResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GenerateCredentialsResult {
    fn default() -> Self {
        Self {
            passwords: Default::default(),
            username: Default::default(),
        }
    }
}
///Managed identity for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Managed identity for the resource.",
///  "type": "object",
///  "properties": {
///    "principalId": {
///      "description": "The principal ID of resource identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "The tenant ID of resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The identity type.",
///      "type": "string",
///      "enum": [
///        "SystemAssigned",
///        "UserAssigned",
///        "SystemAssigned, UserAssigned",
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "ResourceIdentityType"
///      }
///    },
///    "userAssignedIdentities": {
///      "description": "The list of user identities associated with the resource. The user identity \r\ndictionary key references will be ARM resource ids in the form: \r\n'/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/\r\n    providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.",
///      "type": "object",
///      "additionalProperties": {
///        "$ref": "#/components/schemas/UserIdentityProperties"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IdentityProperties {
    ///The principal ID of resource identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
    ///The tenant ID of resource.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    ///The identity type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<IdentityPropertiesType>,
    /**The list of user identities associated with the resource. The user identity
    dictionary key references will be ARM resource ids in the form:
    '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/
        providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.*/
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities:
        ::std::collections::HashMap<::std::string::String, UserIdentityProperties>,
}
impl ::std::convert::From<&IdentityProperties> for IdentityProperties {
    fn from(value: &IdentityProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IdentityProperties {
    fn default() -> Self {
        Self {
            principal_id: Default::default(),
            tenant_id: Default::default(),
            type_: Default::default(),
            user_assigned_identities: Default::default(),
        }
    }
}
///The identity type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The identity type.",
///  "type": "string",
///  "enum": [
///    "SystemAssigned",
///    "UserAssigned",
///    "SystemAssigned, UserAssigned",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "ResourceIdentityType"
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
pub enum IdentityPropertiesType {
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned, UserAssigned")]
    SystemAssignedUserAssigned,
    None,
}
impl ::std::convert::From<&Self> for IdentityPropertiesType {
    fn from(value: &IdentityPropertiesType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IdentityPropertiesType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => f.write_str("SystemAssigned, UserAssigned"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for IdentityPropertiesType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "systemassigned" => Ok(Self::SystemAssigned),
            "userassigned" => Ok(Self::UserAssigned),
            "systemassigned, userassigned" => Ok(Self::SystemAssignedUserAssigned),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IdentityPropertiesType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IdentityPropertiesType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IdentityPropertiesType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`ImportImageParameters`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "source"
///  ],
///  "properties": {
///    "mode": {
///      "description": "When Force, any existing target tags will be overwritten. When NoForce, any existing target tags will fail the operation before any copying begins.",
///      "default": "NoForce",
///      "type": "string",
///      "enum": [
///        "NoForce",
///        "Force"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ImportMode"
///      }
///    },
///    "source": {
///      "$ref": "#/components/schemas/ImportSource"
///    },
///    "targetTags": {
///      "description": "List of strings of the form repo[:tag]. When tag is omitted the source will be used (or 'latest' if source tag is also omitted).",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "untaggedTargetRepositories": {
///      "description": "List of strings of repository names to do a manifest only copy. No tag will be created.",
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
pub struct ImportImageParameters {
    ///When Force, any existing target tags will be overwritten. When NoForce, any existing target tags will fail the operation before any copying begins.
    #[serde(
        default = "defaults::import_image_parameters_mode",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub mode: ImportImageParametersMode,
    pub source: ImportSource,
    ///List of strings of the form repo[:tag]. When tag is omitted the source will be used (or 'latest' if source tag is also omitted).
    #[serde(
        rename = "targetTags",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_tags: ::std::vec::Vec<::std::string::String>,
    ///List of strings of repository names to do a manifest only copy. No tag will be created.
    #[serde(
        rename = "untaggedTargetRepositories",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub untagged_target_repositories: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ImportImageParameters> for ImportImageParameters {
    fn from(value: &ImportImageParameters) -> Self {
        value.clone()
    }
}
///When Force, any existing target tags will be overwritten. When NoForce, any existing target tags will fail the operation before any copying begins.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "When Force, any existing target tags will be overwritten. When NoForce, any existing target tags will fail the operation before any copying begins.",
///  "default": "NoForce",
///  "type": "string",
///  "enum": [
///    "NoForce",
///    "Force"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ImportMode"
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
pub enum ImportImageParametersMode {
    NoForce,
    Force,
}
impl ::std::convert::From<&Self> for ImportImageParametersMode {
    fn from(value: &ImportImageParametersMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ImportImageParametersMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NoForce => f.write_str("NoForce"),
            Self::Force => f.write_str("Force"),
        }
    }
}
impl ::std::str::FromStr for ImportImageParametersMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "noforce" => Ok(Self::NoForce),
            "force" => Ok(Self::Force),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ImportImageParametersMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ImportImageParametersMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ImportImageParametersMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for ImportImageParametersMode {
    fn default() -> Self {
        ImportImageParametersMode::NoForce
    }
}
///`ImportSource`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "sourceImage"
///  ],
///  "properties": {
///    "credentials": {
///      "$ref": "#/components/schemas/ImportSourceCredentials"
///    },
///    "registryUri": {
///      "description": "The address of the source registry (e.g. 'mcr.microsoft.com').",
///      "type": "string"
///    },
///    "resourceId": {
///      "description": "The resource identifier of the source Azure Container Registry.",
///      "type": "string"
///    },
///    "sourceImage": {
///      "description": "Repository name of the source image.\r\nSpecify an image by repository ('hello-world'). This will use the 'latest' tag.\r\nSpecify an image by tag ('hello-world:latest').\r\nSpecify an image by sha256-based manifest digest ('hello-world@sha256:abc123').",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImportSource {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credentials: ::std::option::Option<ImportSourceCredentials>,
    ///The address of the source registry (e.g. 'mcr.microsoft.com').
    #[serde(
        rename = "registryUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registry_uri: ::std::option::Option<::std::string::String>,
    ///The resource identifier of the source Azure Container Registry.
    #[serde(
        rename = "resourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_id: ::std::option::Option<::std::string::String>,
    /**Repository name of the source image.
    Specify an image by repository ('hello-world'). This will use the 'latest' tag.
    Specify an image by tag ('hello-world:latest').
    Specify an image by sha256-based manifest digest ('hello-world@sha256:abc123').*/
    #[serde(rename = "sourceImage")]
    pub source_image: ::std::string::String,
}
impl ::std::convert::From<&ImportSource> for ImportSource {
    fn from(value: &ImportSource) -> Self {
        value.clone()
    }
}
///`ImportSourceCredentials`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "password"
///  ],
///  "properties": {
///    "password": {
///      "description": "The password used to authenticate with the source registry.",
///      "type": "string"
///    },
///    "username": {
///      "description": "The username to authenticate with the source registry.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImportSourceCredentials {
    ///The password used to authenticate with the source registry.
    pub password: ::std::string::String,
    ///The username to authenticate with the source registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub username: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ImportSourceCredentials> for ImportSourceCredentials {
    fn from(value: &ImportSourceCredentials) -> Self {
        value.clone()
    }
}
///IP rule with specific IP or IP range in CIDR format.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP rule with specific IP or IP range in CIDR format.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "action": {
///      "description": "The action of IP ACL rule.",
///      "default": "Allow",
///      "type": "string",
///      "enum": [
///        "Allow"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Action"
///      }
///    },
///    "value": {
///      "description": "Specifies the IP or IP range in CIDR format. Only IPV4 address is allowed.",
///      "type": "string",
///      "x-ms-client-name": "IPAddressOrRange"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpRule {
    ///The action of IP ACL rule.
    #[serde(
        default = "defaults::ip_rule_action",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action: IpRuleAction,
    ///Specifies the IP or IP range in CIDR format. Only IPV4 address is allowed.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&IpRule> for IpRule {
    fn from(value: &IpRule) -> Self {
        value.clone()
    }
}
///The action of IP ACL rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The action of IP ACL rule.",
///  "default": "Allow",
///  "type": "string",
///  "enum": [
///    "Allow"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Action"
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
pub enum IpRuleAction {
    Allow,
}
impl ::std::convert::From<&Self> for IpRuleAction {
    fn from(value: &IpRuleAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IpRuleAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
        }
    }
}
impl ::std::str::FromStr for IpRuleAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IpRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IpRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IpRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for IpRuleAction {
    fn default() -> Self {
        IpRuleAction::Allow
    }
}
///`KeyVaultProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "identity": {
///      "description": "The client id of the identity which will be used to access key vault.",
///      "type": "string"
///    },
///    "keyIdentifier": {
///      "description": "Key vault uri to access the encryption key.",
///      "type": "string"
///    },
///    "keyRotationEnabled": {
///      "description": "Auto key rotation status for a CMK enabled registry.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "lastKeyRotationTimestamp": {
///      "description": "Timestamp of the last successful key rotation.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "versionedKeyIdentifier": {
///      "description": "The fully qualified key identifier that includes the version of the key that is actually used for encryption.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyVaultProperties {
    ///The client id of the identity which will be used to access key vault.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///Key vault uri to access the encryption key.
    #[serde(
        rename = "keyIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_identifier: ::std::option::Option<::std::string::String>,
    ///Auto key rotation status for a CMK enabled registry.
    #[serde(
        rename = "keyRotationEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_rotation_enabled: ::std::option::Option<bool>,
    ///Timestamp of the last successful key rotation.
    #[serde(
        rename = "lastKeyRotationTimestamp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_key_rotation_timestamp: ::std::option::Option<::std::string::String>,
    ///The fully qualified key identifier that includes the version of the key that is actually used for encryption.
    #[serde(
        rename = "versionedKeyIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub versioned_key_identifier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&KeyVaultProperties> for KeyVaultProperties {
    fn from(value: &KeyVaultProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KeyVaultProperties {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            key_identifier: Default::default(),
            key_rotation_enabled: Default::default(),
            last_key_rotation_timestamp: Default::default(),
            versioned_key_identifier: Default::default(),
        }
    }
}
///The logging properties of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The logging properties of the connected registry.",
///  "type": "object",
///  "properties": {
///    "auditLogStatus": {
///      "description": "Indicates whether audit logs are enabled on the connected registry.",
///      "default": "Disabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AuditLogStatus"
///      }
///    },
///    "logLevel": {
///      "description": "The verbosity of logs persisted on the connected registry.",
///      "default": "Information",
///      "type": "string",
///      "enum": [
///        "Debug",
///        "Information",
///        "Warning",
///        "Error",
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LogLevel"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LoggingProperties {
    ///Indicates whether audit logs are enabled on the connected registry.
    #[serde(
        rename = "auditLogStatus",
        default = "defaults::logging_properties_audit_log_status",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub audit_log_status: LoggingPropertiesAuditLogStatus,
    ///The verbosity of logs persisted on the connected registry.
    #[serde(
        rename = "logLevel",
        default = "defaults::logging_properties_log_level",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub log_level: LoggingPropertiesLogLevel,
}
impl ::std::convert::From<&LoggingProperties> for LoggingProperties {
    fn from(value: &LoggingProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LoggingProperties {
    fn default() -> Self {
        Self {
            audit_log_status: defaults::logging_properties_audit_log_status(),
            log_level: defaults::logging_properties_log_level(),
        }
    }
}
///Indicates whether audit logs are enabled on the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates whether audit logs are enabled on the connected registry.",
///  "default": "Disabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AuditLogStatus"
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
pub enum LoggingPropertiesAuditLogStatus {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for LoggingPropertiesAuditLogStatus {
    fn from(value: &LoggingPropertiesAuditLogStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for LoggingPropertiesAuditLogStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for LoggingPropertiesAuditLogStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LoggingPropertiesAuditLogStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LoggingPropertiesAuditLogStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LoggingPropertiesAuditLogStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for LoggingPropertiesAuditLogStatus {
    fn default() -> Self {
        LoggingPropertiesAuditLogStatus::Disabled
    }
}
///The verbosity of logs persisted on the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The verbosity of logs persisted on the connected registry.",
///  "default": "Information",
///  "type": "string",
///  "enum": [
///    "Debug",
///    "Information",
///    "Warning",
///    "Error",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LogLevel"
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
pub enum LoggingPropertiesLogLevel {
    Debug,
    Information,
    Warning,
    Error,
    None,
}
impl ::std::convert::From<&Self> for LoggingPropertiesLogLevel {
    fn from(value: &LoggingPropertiesLogLevel) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for LoggingPropertiesLogLevel {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Debug => f.write_str("Debug"),
            Self::Information => f.write_str("Information"),
            Self::Warning => f.write_str("Warning"),
            Self::Error => f.write_str("Error"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for LoggingPropertiesLogLevel {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "debug" => Ok(Self::Debug),
            "information" => Ok(Self::Information),
            "warning" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LoggingPropertiesLogLevel {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for LoggingPropertiesLogLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for LoggingPropertiesLogLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for LoggingPropertiesLogLevel {
    fn default() -> Self {
        LoggingPropertiesLogLevel::Information
    }
}
///The login server properties of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The login server properties of the connected registry.",
///  "type": "object",
///  "properties": {
///    "host": {
///      "description": "The host of the connected registry. Can be FQDN or IP.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tls": {
///      "$ref": "#/components/schemas/TlsProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LoginServerProperties {
    ///The host of the connected registry. Can be FQDN or IP.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub host: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tls: ::std::option::Option<TlsProperties>,
}
impl ::std::convert::From<&LoginServerProperties> for LoginServerProperties {
    fn from(value: &LoginServerProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LoginServerProperties {
    fn default() -> Self {
        Self {
            host: Default::default(),
            tls: Default::default(),
        }
    }
}
///The network rule set for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The network rule set for a container registry.",
///  "type": "object",
///  "required": [
///    "defaultAction"
///  ],
///  "properties": {
///    "defaultAction": {
///      "description": "The default action of allow or deny when no other rules match.",
///      "default": "Allow",
///      "type": "string",
///      "enum": [
///        "Allow",
///        "Deny"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DefaultAction"
///      }
///    },
///    "ipRules": {
///      "description": "The IP ACL rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IPRule"
///      },
///      "x-ms-identifiers": []
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkRuleSet {
    ///The default action of allow or deny when no other rules match.
    #[serde(
        rename = "defaultAction",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_action: NetworkRuleSetDefaultAction,
    ///The IP ACL rules.
    #[serde(
        rename = "ipRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_rules: ::std::vec::Vec<IpRule>,
}
impl ::std::convert::From<&NetworkRuleSet> for NetworkRuleSet {
    fn from(value: &NetworkRuleSet) -> Self {
        value.clone()
    }
}
///The default action of allow or deny when no other rules match.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The default action of allow or deny when no other rules match.",
///  "default": "Allow",
///  "type": "string",
///  "enum": [
///    "Allow",
///    "Deny"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DefaultAction"
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
impl ::std::default::Default for NetworkRuleSetDefaultAction {
    fn default() -> Self {
        NetworkRuleSetDefaultAction::Allow
    }
}
///The definition of a container registry operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The definition of a container registry operation.",
///  "type": "object",
///  "properties": {
///    "display": {
///      "$ref": "#/components/schemas/OperationDisplayDefinition"
///    },
///    "isDataAction": {
///      "description": "This property indicates if the operation is an action or a data action\r\nref: https://docs.microsoft.com/en-us/azure/role-based-access-control/role-definitions#management-and-data-operations",
///      "type": "boolean"
///    },
///    "name": {
///      "description": "Operation name: {provider}/{resource}/{operation}.",
///      "type": "string"
///    },
///    "origin": {
///      "description": "The origin information of the container registry operation.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/OperationPropertiesDefinition"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationDefinition {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display: ::std::option::Option<OperationDisplayDefinition>,
    /**This property indicates if the operation is an action or a data action
    ref: https://docs.microsoft.com/en-us/azure/role-based-access-control/role-definitions#management-and-data-operations*/
    #[serde(
        rename = "isDataAction",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_data_action: ::std::option::Option<bool>,
    ///Operation name: {provider}/{resource}/{operation}.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The origin information of the container registry operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub origin: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<OperationPropertiesDefinition>,
}
impl ::std::convert::From<&OperationDefinition> for OperationDefinition {
    fn from(value: &OperationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationDefinition {
    fn default() -> Self {
        Self {
            display: Default::default(),
            is_data_action: Default::default(),
            name: Default::default(),
            origin: Default::default(),
            properties: Default::default(),
        }
    }
}
///The display information for a container registry operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The display information for a container registry operation.",
///  "type": "object",
///  "properties": {
///    "description": {
///      "description": "The description for the operation.",
///      "type": "string"
///    },
///    "operation": {
///      "description": "The operation that users can perform.",
///      "type": "string"
///    },
///    "provider": {
///      "description": "The resource provider name: Microsoft.ContainerRegistry.",
///      "type": "string"
///    },
///    "resource": {
///      "description": "The resource on which the operation is performed.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationDisplayDefinition {
    ///The description for the operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The operation that users can perform.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<::std::string::String>,
    ///The resource provider name: Microsoft.ContainerRegistry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
    ///The resource on which the operation is performed.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&OperationDisplayDefinition> for OperationDisplayDefinition {
    fn from(value: &OperationDisplayDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationDisplayDefinition {
    fn default() -> Self {
        Self {
            description: Default::default(),
            operation: Default::default(),
            provider: Default::default(),
            resource: Default::default(),
        }
    }
}
///The result of a request to list container registry operations.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list container registry operations.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of container registry operations.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of container registry operations. Since this list may be incomplete, the nextLink field should be used to request the next list of operations.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/OperationDefinition"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationListResult {
    ///The URI that can be used to request the next list of container registry operations.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of container registry operations. Since this list may be incomplete, the nextLink field should be used to request the next list of operations.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<OperationDefinition>,
}
impl ::std::convert::From<&OperationListResult> for OperationListResult {
    fn from(value: &OperationListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The definition of Azure Monitoring log.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The definition of Azure Monitoring log.",
///  "type": "object",
///  "properties": {
///    "blobDuration": {
///      "description": "Log blob duration.",
///      "type": "string"
///    },
///    "displayName": {
///      "description": "Log display name.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Log name.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationLogSpecificationDefinition {
    ///Log blob duration.
    #[serde(
        rename = "blobDuration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob_duration: ::std::option::Option<::std::string::String>,
    ///Log display name.
    #[serde(
        rename = "displayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_name: ::std::option::Option<::std::string::String>,
    ///Log name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&OperationLogSpecificationDefinition>
    for OperationLogSpecificationDefinition
{
    fn from(value: &OperationLogSpecificationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationLogSpecificationDefinition {
    fn default() -> Self {
        Self {
            blob_duration: Default::default(),
            display_name: Default::default(),
            name: Default::default(),
        }
    }
}
///The definition of Azure Monitoring metric.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The definition of Azure Monitoring metric.",
///  "type": "object",
///  "properties": {
///    "aggregationType": {
///      "description": "Metric aggregation type.",
///      "type": "string"
///    },
///    "displayDescription": {
///      "description": "Metric description.",
///      "type": "string"
///    },
///    "displayName": {
///      "description": "Metric display name.",
///      "type": "string"
///    },
///    "internalMetricName": {
///      "description": "Internal metric name.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Metric name.",
///      "type": "string"
///    },
///    "unit": {
///      "description": "Metric unit.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationMetricSpecificationDefinition {
    ///Metric aggregation type.
    #[serde(
        rename = "aggregationType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub aggregation_type: ::std::option::Option<::std::string::String>,
    ///Metric description.
    #[serde(
        rename = "displayDescription",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_description: ::std::option::Option<::std::string::String>,
    ///Metric display name.
    #[serde(
        rename = "displayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_name: ::std::option::Option<::std::string::String>,
    ///Internal metric name.
    #[serde(
        rename = "internalMetricName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub internal_metric_name: ::std::option::Option<::std::string::String>,
    ///Metric name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Metric unit.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub unit: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&OperationMetricSpecificationDefinition>
    for OperationMetricSpecificationDefinition
{
    fn from(value: &OperationMetricSpecificationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationMetricSpecificationDefinition {
    fn default() -> Self {
        Self {
            aggregation_type: Default::default(),
            display_description: Default::default(),
            display_name: Default::default(),
            internal_metric_name: Default::default(),
            name: Default::default(),
            unit: Default::default(),
        }
    }
}
///The definition of Azure Monitoring properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The definition of Azure Monitoring properties.",
///  "type": "object",
///  "properties": {
///    "serviceSpecification": {
///      "$ref": "#/components/schemas/OperationServiceSpecificationDefinition"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationPropertiesDefinition {
    #[serde(
        rename = "serviceSpecification",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_specification: ::std::option::Option<OperationServiceSpecificationDefinition>,
}
impl ::std::convert::From<&OperationPropertiesDefinition> for OperationPropertiesDefinition {
    fn from(value: &OperationPropertiesDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationPropertiesDefinition {
    fn default() -> Self {
        Self {
            service_specification: Default::default(),
        }
    }
}
///The definition of Azure Monitoring list.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The definition of Azure Monitoring list.",
///  "type": "object",
///  "properties": {
///    "logSpecifications": {
///      "description": "A list of Azure Monitoring log definitions.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/OperationLogSpecificationDefinition"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "metricSpecifications": {
///      "description": "A list of Azure Monitoring metrics definition.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/OperationMetricSpecificationDefinition"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationServiceSpecificationDefinition {
    ///A list of Azure Monitoring log definitions.
    #[serde(
        rename = "logSpecifications",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub log_specifications: ::std::vec::Vec<OperationLogSpecificationDefinition>,
    ///A list of Azure Monitoring metrics definition.
    #[serde(
        rename = "metricSpecifications",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metric_specifications: ::std::vec::Vec<OperationMetricSpecificationDefinition>,
}
impl ::std::convert::From<&OperationServiceSpecificationDefinition>
    for OperationServiceSpecificationDefinition
{
    fn from(value: &OperationServiceSpecificationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationServiceSpecificationDefinition {
    fn default() -> Self {
        Self {
            log_specifications: Default::default(),
            metric_specifications: Default::default(),
        }
    }
}
///The properties of a package type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a package type.",
///  "type": "object",
///  "properties": {
///    "endpoint": {
///      "description": "The endpoint of the package type.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the package type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PackageType {
    ///The endpoint of the package type.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub endpoint: ::std::option::Option<::std::string::String>,
    ///The name of the package type.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PackageType> for PackageType {
    fn from(value: &PackageType) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PackageType {
    fn default() -> Self {
        Self {
            endpoint: Default::default(),
            name: Default::default(),
        }
    }
}
///The properties of the connected registry parent.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of the connected registry parent.",
///  "type": "object",
///  "required": [
///    "syncProperties"
///  ],
///  "properties": {
///    "id": {
///      "description": "The resource ID of the parent to which the connected registry will be associated.",
///      "type": "string"
///    },
///    "syncProperties": {
///      "$ref": "#/components/schemas/SyncProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ParentProperties {
    ///The resource ID of the parent to which the connected registry will be associated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    #[serde(rename = "syncProperties")]
    pub sync_properties: SyncProperties,
}
impl ::std::convert::From<&ParentProperties> for ParentProperties {
    fn from(value: &ParentProperties) -> Self {
        value.clone()
    }
}
///The policies for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The policies for a container registry.",
///  "type": "object",
///  "properties": {
///    "azureADAuthenticationAsArmPolicy": {
///      "$ref": "#/components/schemas/AzureADAuthenticationAsArmPolicy"
///    },
///    "exportPolicy": {
///      "$ref": "#/components/schemas/ExportPolicy"
///    },
///    "quarantinePolicy": {
///      "$ref": "#/components/schemas/QuarantinePolicy"
///    },
///    "retentionPolicy": {
///      "$ref": "#/components/schemas/RetentionPolicy"
///    },
///    "trustPolicy": {
///      "$ref": "#/components/schemas/TrustPolicy"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Policies {
    #[serde(
        rename = "azureADAuthenticationAsArmPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub azure_ad_authentication_as_arm_policy:
        ::std::option::Option<AzureAdAuthenticationAsArmPolicy>,
    #[serde(
        rename = "exportPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub export_policy: ::std::option::Option<ExportPolicy>,
    #[serde(
        rename = "quarantinePolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub quarantine_policy: ::std::option::Option<QuarantinePolicy>,
    #[serde(
        rename = "retentionPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub retention_policy: ::std::option::Option<RetentionPolicy>,
    #[serde(
        rename = "trustPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub trust_policy: ::std::option::Option<TrustPolicy>,
}
impl ::std::convert::From<&Policies> for Policies {
    fn from(value: &Policies) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Policies {
    fn default() -> Self {
        Self {
            azure_ad_authentication_as_arm_policy: Default::default(),
            export_policy: Default::default(),
            quarantine_policy: Default::default(),
            retention_policy: Default::default(),
            trust_policy: Default::default(),
        }
    }
}
///The Private Endpoint resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Private Endpoint resource.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "This is private endpoint resource created with Microsoft.Network resource provider.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpoint {
    ///This is private endpoint resource created with Microsoft.Network resource provider.
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
///An object that represents a private endpoint connection for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a private endpoint connection for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnection {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
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
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
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
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The result of a request to list private endpoint connections for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list private endpoint connections for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of private endpoint connections.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of private endpoint connections. Since this list may be incomplete, the nextLink field should be used to request the next list of private endpoint connections.",
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
    ///The URI that can be used to request the next list of private endpoint connections.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of private endpoint connections. Since this list may be incomplete, the nextLink field should be used to request the next list of private endpoint connections.
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
///The properties of a private endpoint connection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a private endpoint connection.",
///  "type": "object",
///  "properties": {
///    "privateEndpoint": {
///      "$ref": "#/components/schemas/PrivateEndpoint"
///    },
///    "privateLinkServiceConnectionState": {
///      "$ref": "#/components/schemas/PrivateLinkServiceConnectionState"
///    },
///    "provisioningState": {
///      "description": "The provisioning state of private endpoint connection resource.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
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
    ///The provisioning state of private endpoint connection resource.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state:
        ::std::option::Option<PrivateEndpointConnectionPropertiesProvisioningState>,
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
///The provisioning state of private endpoint connection resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provisioning state of private endpoint connection resource.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum PrivateEndpointConnectionPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for PrivateEndpointConnectionPropertiesProvisioningState {
    fn from(value: &PrivateEndpointConnectionPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateEndpointConnectionPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointConnectionPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateEndpointConnectionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PrivateEndpointConnectionPropertiesProvisioningState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for PrivateEndpointConnectionPropertiesProvisioningState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A resource that supports private link capabilities.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A resource that supports private link capabilities.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The resource ID.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkResourceProperties"
///    },
///    "type": {
///      "description": "The resource type is private link resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResource {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
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
    ///The resource type is private link resource.
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
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///The result of a request to list private link resources for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list private link resources for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of private link resources.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of private link resources. Since this list may be incomplete, the nextLink field should be used to request the next list of private link resources.",
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
    ///The URI that can be used to request the next list of private link resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of private link resources. Since this list may be incomplete, the nextLink field should be used to request the next list of private link resources.
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
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a private link resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a private link resource.",
///  "type": "object",
///  "properties": {
///    "groupId": {
///      "description": "The private link resource group id.",
///      "type": "string"
///    },
///    "requiredMembers": {
///      "description": "The private link resource required member names.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "requiredZoneNames": {
///      "description": "The private link resource Private link DNS zone name.",
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
    ///The private link resource group id.
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<::std::string::String>,
    ///The private link resource required member names.
    #[serde(
        rename = "requiredMembers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_members: ::std::vec::Vec<::std::string::String>,
    ///The private link resource Private link DNS zone name.
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
///The state of a private link service connection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The state of a private link service connection.",
///  "type": "object",
///  "properties": {
///    "actionsRequired": {
///      "description": "A message indicating if changes on the service provider require any updates on the consumer.",
///      "type": "string",
///      "enum": [
///        "None",
///        "Recreate"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ActionsRequired"
///      }
///    },
///    "description": {
///      "description": "The description for connection status. For example if connection is rejected it can indicate reason for rejection.",
///      "type": "string"
///    },
///    "status": {
///      "description": "The private link service connection status.",
///      "type": "string",
///      "enum": [
///        "Approved",
///        "Pending",
///        "Rejected",
///        "Disconnected"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ConnectionStatus"
///      }
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
    ///The description for connection status. For example if connection is rejected it can indicate reason for rejection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The private link service connection status.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<PrivateLinkServiceConnectionStateStatus>,
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
///    "None",
///    "Recreate"
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
    Recreate,
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
            Self::Recreate => f.write_str("Recreate"),
        }
    }
}
impl ::std::str::FromStr for PrivateLinkServiceConnectionStateActionsRequired {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "recreate" => Ok(Self::Recreate),
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
///The private link service connection status.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The private link service connection status.",
///  "type": "string",
///  "enum": [
///    "Approved",
///    "Pending",
///    "Rejected",
///    "Disconnected"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ConnectionStatus"
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
pub enum PrivateLinkServiceConnectionStateStatus {
    Approved,
    Pending,
    Rejected,
    Disconnected,
}
impl ::std::convert::From<&Self> for PrivateLinkServiceConnectionStateStatus {
    fn from(value: &PrivateLinkServiceConnectionStateStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateLinkServiceConnectionStateStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Approved => f.write_str("Approved"),
            Self::Pending => f.write_str("Pending"),
            Self::Rejected => f.write_str("Rejected"),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}
impl ::std::str::FromStr for PrivateLinkServiceConnectionStateStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "approved" => Ok(Self::Approved),
            "pending" => Ok(Self::Pending),
            "rejected" => Ok(Self::Rejected),
            "disconnected" => Ok(Self::Disconnected),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateLinkServiceConnectionStateStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PrivateLinkServiceConnectionStateStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateLinkServiceConnectionStateStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The resource model definition for a ARM proxy resource. It will have everything other than required location and tags.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The resource model definition for a ARM proxy resource. It will have everything other than required location and tags.",
///  "properties": {
///    "id": {
///      "description": "The resource ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/SystemData"
///    },
///    "type": {
///      "description": "The type of the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProxyResource {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ProxyResource> for ProxyResource {
    fn from(value: &ProxyResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProxyResource {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The quarantine policy for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The quarantine policy for a container registry.",
///  "type": "object",
///  "properties": {
///    "status": {
///      "description": "The value that indicates whether the policy is enabled or not.",
///      "default": "disabled",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PolicyStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct QuarantinePolicy {
    ///The value that indicates whether the policy is enabled or not.
    #[serde(
        default = "defaults::quarantine_policy_status",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: QuarantinePolicyStatus,
}
impl ::std::convert::From<&QuarantinePolicy> for QuarantinePolicy {
    fn from(value: &QuarantinePolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for QuarantinePolicy {
    fn default() -> Self {
        Self {
            status: defaults::quarantine_policy_status(),
        }
    }
}
///The value that indicates whether the policy is enabled or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The value that indicates whether the policy is enabled or not.",
///  "default": "disabled",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PolicyStatus"
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
pub enum QuarantinePolicyStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for QuarantinePolicyStatus {
    fn from(value: &QuarantinePolicyStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for QuarantinePolicyStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for QuarantinePolicyStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for QuarantinePolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for QuarantinePolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for QuarantinePolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for QuarantinePolicyStatus {
    fn default() -> Self {
        QuarantinePolicyStatus::Disabled
    }
}
///The parameters used to regenerate the login credential.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used to regenerate the login credential.",
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "name": {
///      "description": "Specifies name of the password which should be regenerated -- password or password2.",
///      "type": "string",
///      "enum": [
///        "password",
///        "password2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "PasswordName"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegenerateCredentialParameters {
    ///Specifies name of the password which should be regenerated -- password or password2.
    pub name: RegenerateCredentialParametersName,
}
impl ::std::convert::From<&RegenerateCredentialParameters> for RegenerateCredentialParameters {
    fn from(value: &RegenerateCredentialParameters) -> Self {
        value.clone()
    }
}
///Specifies name of the password which should be regenerated -- password or password2.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies name of the password which should be regenerated -- password or password2.",
///  "type": "string",
///  "enum": [
///    "password",
///    "password2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "PasswordName"
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
pub enum RegenerateCredentialParametersName {
    #[serde(rename = "password")]
    Password,
    #[serde(rename = "password2")]
    Password2,
}
impl ::std::convert::From<&Self> for RegenerateCredentialParametersName {
    fn from(value: &RegenerateCredentialParametersName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegenerateCredentialParametersName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Password => f.write_str("password"),
            Self::Password2 => f.write_str("password2"),
        }
    }
}
impl ::std::str::FromStr for RegenerateCredentialParametersName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "password" => Ok(Self::Password),
            "password2" => Ok(Self::Password2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegenerateCredentialParametersName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegenerateCredentialParametersName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegenerateCredentialParametersName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An object that represents a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "required": [
///    "sku"
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/IdentityProperties"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RegistryProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Registry {
    ///The resource ID.
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
    pub identity: ::std::option::Option<IdentityProperties>,
    ///The location of the resource. This cannot be changed after the resource is created.
    pub location: ::std::string::String,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<RegistryProperties>,
    pub sku: Sku,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The tags of the resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Registry> for Registry {
    fn from(value: &Registry) -> Self {
        value.clone()
    }
}
///The response from the ListCredentials operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the ListCredentials operation.",
///  "type": "object",
///  "properties": {
///    "passwords": {
///      "description": "The list of passwords for a container registry.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RegistryPassword"
///      },
///      "x-ms-identifiers": []
///    },
///    "username": {
///      "description": "The username for a container registry.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryListCredentialsResult {
    ///The list of passwords for a container registry.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub passwords: ::std::vec::Vec<RegistryPassword>,
    ///The username for a container registry.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub username: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistryListCredentialsResult> for RegistryListCredentialsResult {
    fn from(value: &RegistryListCredentialsResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryListCredentialsResult {
    fn default() -> Self {
        Self {
            passwords: Default::default(),
            username: Default::default(),
        }
    }
}
///The result of a request to list container registries.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list container registries.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of container registries.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of container registries. Since this list may be incomplete, the nextLink field should be used to request the next list of container registries.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Registry"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryListResult {
    ///The URI that can be used to request the next list of container registries.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of container registries. Since this list may be incomplete, the nextLink field should be used to request the next list of container registries.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Registry>,
}
impl ::std::convert::From<&RegistryListResult> for RegistryListResult {
    fn from(value: &RegistryListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///A request to check whether a container registry name is available.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A request to check whether a container registry name is available.",
///  "type": "object",
///  "required": [
///    "name",
///    "type"
///  ],
///  "properties": {
///    "name": {
///      "description": "The name of the container registry.",
///      "type": "string",
///      "maxLength": 50,
///      "minLength": 5,
///      "pattern": "^[a-zA-Z0-9]*$"
///    },
///    "type": {
///      "description": "The resource type of the container registry. This field must be set to 'Microsoft.ContainerRegistry/registries'.",
///      "type": "string",
///      "enum": [
///        "Microsoft.ContainerRegistry/registries"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "ContainerRegistryResourceType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryNameCheckRequest {
    ///The name of the container registry.
    pub name: RegistryNameCheckRequestName,
    ///The resource type of the container registry. This field must be set to 'Microsoft.ContainerRegistry/registries'.
    #[serde(rename = "type")]
    pub type_: RegistryNameCheckRequestType,
}
impl ::std::convert::From<&RegistryNameCheckRequest> for RegistryNameCheckRequest {
    fn from(value: &RegistryNameCheckRequest) -> Self {
        value.clone()
    }
}
///The name of the container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The name of the container registry.",
///  "type": "string",
///  "maxLength": 50,
///  "minLength": 5,
///  "pattern": "^[a-zA-Z0-9]*$"
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct RegistryNameCheckRequestName(::std::string::String);
impl ::std::ops::Deref for RegistryNameCheckRequestName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<RegistryNameCheckRequestName> for ::std::string::String {
    fn from(value: RegistryNameCheckRequestName) -> Self {
        value.0
    }
}
impl ::std::convert::From<&RegistryNameCheckRequestName> for RegistryNameCheckRequestName {
    fn from(value: &RegistryNameCheckRequestName) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for RegistryNameCheckRequestName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 50usize {
            return Err("longer than 50 characters".into());
        }
        if value.chars().count() < 5usize {
            return Err("shorter than 5 characters".into());
        }
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^[a-zA-Z0-9]*$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[a-zA-Z0-9]*$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for RegistryNameCheckRequestName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryNameCheckRequestName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryNameCheckRequestName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for RegistryNameCheckRequestName {
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
///The resource type of the container registry. This field must be set to 'Microsoft.ContainerRegistry/registries'.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The resource type of the container registry. This field must be set to 'Microsoft.ContainerRegistry/registries'.",
///  "type": "string",
///  "enum": [
///    "Microsoft.ContainerRegistry/registries"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "ContainerRegistryResourceType"
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
pub enum RegistryNameCheckRequestType {
    #[serde(rename = "Microsoft.ContainerRegistry/registries")]
    MicrosoftContainerRegistryRegistries,
}
impl ::std::convert::From<&Self> for RegistryNameCheckRequestType {
    fn from(value: &RegistryNameCheckRequestType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryNameCheckRequestType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftContainerRegistryRegistries => {
                f.write_str("Microsoft.ContainerRegistry/registries")
            }
        }
    }
}
impl ::std::str::FromStr for RegistryNameCheckRequestType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoft.containerregistry/registries" => {
                Ok(Self::MicrosoftContainerRegistryRegistries)
            }
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryNameCheckRequestType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryNameCheckRequestType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryNameCheckRequestType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The result of a request to check the availability of a container registry name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to check the availability of a container registry name.",
///  "type": "object",
///  "properties": {
///    "message": {
///      "description": "If any, the error message that provides more detail for the reason that the name is not available.",
///      "type": "string"
///    },
///    "nameAvailable": {
///      "description": "The value that indicates whether the name is available.",
///      "type": "boolean"
///    },
///    "reason": {
///      "description": "If any, the reason that the name is not available.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryNameStatus {
    ///If any, the error message that provides more detail for the reason that the name is not available.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///The value that indicates whether the name is available.
    #[serde(
        rename = "nameAvailable",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name_available: ::std::option::Option<bool>,
    ///If any, the reason that the name is not available.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reason: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistryNameStatus> for RegistryNameStatus {
    fn from(value: &RegistryNameStatus) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryNameStatus {
    fn default() -> Self {
        Self {
            message: Default::default(),
            name_available: Default::default(),
            reason: Default::default(),
        }
    }
}
///The login password for the container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The login password for the container registry.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "The password name.",
///      "type": "string",
///      "enum": [
///        "password",
///        "password2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "PasswordName"
///      }
///    },
///    "value": {
///      "description": "The password value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryPassword {
    ///The password name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<RegistryPasswordName>,
    ///The password value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistryPassword> for RegistryPassword {
    fn from(value: &RegistryPassword) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryPassword {
    fn default() -> Self {
        Self {
            name: Default::default(),
            value: Default::default(),
        }
    }
}
///The password name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The password name.",
///  "type": "string",
///  "enum": [
///    "password",
///    "password2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "PasswordName"
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
pub enum RegistryPasswordName {
    #[serde(rename = "password")]
    Password,
    #[serde(rename = "password2")]
    Password2,
}
impl ::std::convert::From<&Self> for RegistryPasswordName {
    fn from(value: &RegistryPasswordName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPasswordName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Password => f.write_str("password"),
            Self::Password2 => f.write_str("password2"),
        }
    }
}
impl ::std::str::FromStr for RegistryPasswordName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "password" => Ok(Self::Password),
            "password2" => Ok(Self::Password2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPasswordName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryPasswordName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryPasswordName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The properties of a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a container registry.",
///  "type": "object",
///  "properties": {
///    "adminUserEnabled": {
///      "description": "The value that indicates whether the admin user is enabled.",
///      "default": false,
///      "type": "boolean"
///    },
///    "anonymousPullEnabled": {
///      "description": "Enables registry-wide pull from unauthenticated clients.",
///      "default": false,
///      "type": "boolean"
///    },
///    "creationDate": {
///      "description": "The creation date of the container registry in ISO8601 format.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "dataEndpointEnabled": {
///      "description": "Enable a single data endpoint per region for serving data.",
///      "type": "boolean"
///    },
///    "dataEndpointHostNames": {
///      "description": "List of host names that will serve data when dataEndpointEnabled is true.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/EncryptionProperty"
///    },
///    "loginServer": {
///      "description": "The URL that can be used to log into the container registry.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "networkRuleBypassOptions": {
///      "description": "Whether to allow trusted Azure services to access a network restricted registry.",
///      "default": "AzureServices",
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
///    "networkRuleSet": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "policies": {
///      "$ref": "#/components/schemas/Policies"
///    },
///    "privateEndpointConnections": {
///      "description": "List of private endpoint connections for a container registry.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    },
///    "provisioningState": {
///      "description": "The provisioning state of the container registry at the time the operation was called.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "publicNetworkAccess": {
///      "description": "Whether or not public network access is allowed for the container registry.",
///      "default": "Enabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PublicNetworkAccess"
///      }
///    },
///    "status": {
///      "$ref": "#/components/schemas/Status"
///    },
///    "zoneRedundancy": {
///      "description": "Whether or not zone redundancy is enabled for this container registry",
///      "default": "Disabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ZoneRedundancy"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryProperties {
    ///The value that indicates whether the admin user is enabled.
    #[serde(
        rename = "adminUserEnabled",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub admin_user_enabled: bool,
    ///Enables registry-wide pull from unauthenticated clients.
    #[serde(
        rename = "anonymousPullEnabled",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub anonymous_pull_enabled: bool,
    ///The creation date of the container registry in ISO8601 format.
    #[serde(
        rename = "creationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_date: ::std::option::Option<::std::string::String>,
    ///Enable a single data endpoint per region for serving data.
    #[serde(
        rename = "dataEndpointEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_endpoint_enabled: ::std::option::Option<bool>,
    ///List of host names that will serve data when dataEndpointEnabled is true.
    #[serde(
        rename = "dataEndpointHostNames",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_endpoint_host_names: ::std::vec::Vec<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<EncryptionProperty>,
    ///The URL that can be used to log into the container registry.
    #[serde(
        rename = "loginServer",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub login_server: ::std::option::Option<::std::string::String>,
    ///Whether to allow trusted Azure services to access a network restricted registry.
    #[serde(
        rename = "networkRuleBypassOptions",
        default = "defaults::registry_properties_network_rule_bypass_options",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_rule_bypass_options: RegistryPropertiesNetworkRuleBypassOptions,
    #[serde(
        rename = "networkRuleSet",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_rule_set: ::std::option::Option<NetworkRuleSet>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policies: ::std::option::Option<Policies>,
    ///List of private endpoint connections for a container registry.
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnection>,
    ///The provisioning state of the container registry at the time the operation was called.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<RegistryPropertiesProvisioningState>,
    ///Whether or not public network access is allowed for the container registry.
    #[serde(
        rename = "publicNetworkAccess",
        default = "defaults::registry_properties_public_network_access",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: RegistryPropertiesPublicNetworkAccess,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<Status>,
    ///Whether or not zone redundancy is enabled for this container registry
    #[serde(
        rename = "zoneRedundancy",
        default = "defaults::registry_properties_zone_redundancy",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zone_redundancy: RegistryPropertiesZoneRedundancy,
}
impl ::std::convert::From<&RegistryProperties> for RegistryProperties {
    fn from(value: &RegistryProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryProperties {
    fn default() -> Self {
        Self {
            admin_user_enabled: Default::default(),
            anonymous_pull_enabled: Default::default(),
            creation_date: Default::default(),
            data_endpoint_enabled: Default::default(),
            data_endpoint_host_names: Default::default(),
            encryption: Default::default(),
            login_server: Default::default(),
            network_rule_bypass_options: defaults::registry_properties_network_rule_bypass_options(
            ),
            network_rule_set: Default::default(),
            policies: Default::default(),
            private_endpoint_connections: Default::default(),
            provisioning_state: Default::default(),
            public_network_access: defaults::registry_properties_public_network_access(),
            status: Default::default(),
            zone_redundancy: defaults::registry_properties_zone_redundancy(),
        }
    }
}
///Whether to allow trusted Azure services to access a network restricted registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether to allow trusted Azure services to access a network restricted registry.",
///  "default": "AzureServices",
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
pub enum RegistryPropertiesNetworkRuleBypassOptions {
    AzureServices,
    None,
}
impl ::std::convert::From<&Self> for RegistryPropertiesNetworkRuleBypassOptions {
    fn from(value: &RegistryPropertiesNetworkRuleBypassOptions) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPropertiesNetworkRuleBypassOptions {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AzureServices => f.write_str("AzureServices"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for RegistryPropertiesNetworkRuleBypassOptions {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "azureservices" => Ok(Self::AzureServices),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPropertiesNetworkRuleBypassOptions {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for RegistryPropertiesNetworkRuleBypassOptions
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryPropertiesNetworkRuleBypassOptions {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for RegistryPropertiesNetworkRuleBypassOptions {
    fn default() -> Self {
        RegistryPropertiesNetworkRuleBypassOptions::AzureServices
    }
}
///The provisioning state of the container registry at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provisioning state of the container registry at the time the operation was called.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum RegistryPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for RegistryPropertiesProvisioningState {
    fn from(value: &RegistryPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for RegistryPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Whether or not public network access is allowed for the container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether or not public network access is allowed for the container registry.",
///  "default": "Enabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicNetworkAccess"
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
pub enum RegistryPropertiesPublicNetworkAccess {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for RegistryPropertiesPublicNetworkAccess {
    fn from(value: &RegistryPropertiesPublicNetworkAccess) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPropertiesPublicNetworkAccess {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for RegistryPropertiesPublicNetworkAccess {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPropertiesPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryPropertiesPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryPropertiesPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for RegistryPropertiesPublicNetworkAccess {
    fn default() -> Self {
        RegistryPropertiesPublicNetworkAccess::Enabled
    }
}
///The parameters for updating the properties of a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating the properties of a container registry.",
///  "type": "object",
///  "properties": {
///    "adminUserEnabled": {
///      "description": "The value that indicates whether the admin user is enabled.",
///      "type": "boolean"
///    },
///    "anonymousPullEnabled": {
///      "description": "Enables registry-wide pull from unauthenticated clients.",
///      "type": "boolean"
///    },
///    "dataEndpointEnabled": {
///      "description": "Enable a single data endpoint per region for serving data.",
///      "type": "boolean"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/EncryptionProperty"
///    },
///    "networkRuleBypassOptions": {
///      "description": "Whether to allow trusted Azure services to access a network restricted registry.",
///      "default": "AzureServices",
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
///    "networkRuleSet": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "policies": {
///      "$ref": "#/components/schemas/Policies"
///    },
///    "publicNetworkAccess": {
///      "description": "Whether or not public network access is allowed for the container registry.",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PublicNetworkAccess"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryPropertiesUpdateParameters {
    ///The value that indicates whether the admin user is enabled.
    #[serde(
        rename = "adminUserEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub admin_user_enabled: ::std::option::Option<bool>,
    ///Enables registry-wide pull from unauthenticated clients.
    #[serde(
        rename = "anonymousPullEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub anonymous_pull_enabled: ::std::option::Option<bool>,
    ///Enable a single data endpoint per region for serving data.
    #[serde(
        rename = "dataEndpointEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_endpoint_enabled: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<EncryptionProperty>,
    ///Whether to allow trusted Azure services to access a network restricted registry.
    #[serde(
        rename = "networkRuleBypassOptions",
        default = "defaults::registry_properties_update_parameters_network_rule_bypass_options",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_rule_bypass_options: RegistryPropertiesUpdateParametersNetworkRuleBypassOptions,
    #[serde(
        rename = "networkRuleSet",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_rule_set: ::std::option::Option<NetworkRuleSet>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policies: ::std::option::Option<Policies>,
    ///Whether or not public network access is allowed for the container registry.
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access:
        ::std::option::Option<RegistryPropertiesUpdateParametersPublicNetworkAccess>,
}
impl ::std::convert::From<&RegistryPropertiesUpdateParameters>
    for RegistryPropertiesUpdateParameters
{
    fn from(value: &RegistryPropertiesUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryPropertiesUpdateParameters {
    fn default() -> Self {
        Self {
            admin_user_enabled: Default::default(),
            anonymous_pull_enabled: Default::default(),
            data_endpoint_enabled: Default::default(),
            encryption: Default::default(),
            network_rule_bypass_options:
                defaults::registry_properties_update_parameters_network_rule_bypass_options(),
            network_rule_set: Default::default(),
            policies: Default::default(),
            public_network_access: Default::default(),
        }
    }
}
///Whether to allow trusted Azure services to access a network restricted registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether to allow trusted Azure services to access a network restricted registry.",
///  "default": "AzureServices",
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
pub enum RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
    AzureServices,
    None,
}
impl ::std::convert::From<&Self> for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
    fn from(value: &RegistryPropertiesUpdateParametersNetworkRuleBypassOptions) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AzureServices => f.write_str("AzureServices"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "azureservices" => Ok(Self::AzureServices),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
    fn default() -> Self {
        RegistryPropertiesUpdateParametersNetworkRuleBypassOptions::AzureServices
    }
}
///Whether or not public network access is allowed for the container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether or not public network access is allowed for the container registry.",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicNetworkAccess"
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
pub enum RegistryPropertiesUpdateParametersPublicNetworkAccess {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for RegistryPropertiesUpdateParametersPublicNetworkAccess {
    fn from(value: &RegistryPropertiesUpdateParametersPublicNetworkAccess) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPropertiesUpdateParametersPublicNetworkAccess {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for RegistryPropertiesUpdateParametersPublicNetworkAccess {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPropertiesUpdateParametersPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for RegistryPropertiesUpdateParametersPublicNetworkAccess
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for RegistryPropertiesUpdateParametersPublicNetworkAccess
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Whether or not zone redundancy is enabled for this container registry
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether or not zone redundancy is enabled for this container registry",
///  "default": "Disabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ZoneRedundancy"
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
pub enum RegistryPropertiesZoneRedundancy {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for RegistryPropertiesZoneRedundancy {
    fn from(value: &RegistryPropertiesZoneRedundancy) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryPropertiesZoneRedundancy {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for RegistryPropertiesZoneRedundancy {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryPropertiesZoneRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryPropertiesZoneRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryPropertiesZoneRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for RegistryPropertiesZoneRedundancy {
    fn default() -> Self {
        RegistryPropertiesZoneRedundancy::Disabled
    }
}
///The parameters for updating a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a container registry.",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/IdentityProperties"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RegistryPropertiesUpdateParameters"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    },
///    "tags": {
///      "description": "The tags for the container registry.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<IdentityProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<RegistryPropertiesUpdateParameters>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
    ///The tags for the container registry.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&RegistryUpdateParameters> for RegistryUpdateParameters {
    fn from(value: &RegistryUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryUpdateParameters {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
        }
    }
}
///The quota usage for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The quota usage for a container registry.",
///  "type": "object",
///  "properties": {
///    "currentValue": {
///      "description": "The current value of the usage.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "limit": {
///      "description": "The limit of the usage.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "name": {
///      "description": "The name of the usage.",
///      "type": "string"
///    },
///    "unit": {
///      "description": "The unit of measurement.",
///      "type": "string",
///      "enum": [
///        "Count",
///        "Bytes"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "RegistryUsageUnit"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryUsage {
    ///The current value of the usage.
    #[serde(
        rename = "currentValue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub current_value: ::std::option::Option<i64>,
    ///The limit of the usage.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub limit: ::std::option::Option<i64>,
    ///The name of the usage.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The unit of measurement.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub unit: ::std::option::Option<RegistryUsageUnit>,
}
impl ::std::convert::From<&RegistryUsage> for RegistryUsage {
    fn from(value: &RegistryUsage) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryUsage {
    fn default() -> Self {
        Self {
            current_value: Default::default(),
            limit: Default::default(),
            name: Default::default(),
            unit: Default::default(),
        }
    }
}
///The result of a request to get container registry quota usages.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to get container registry quota usages.",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "The list of container registry quota usages.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RegistryUsage"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryUsageListResult {
    ///The list of container registry quota usages.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<RegistryUsage>,
}
impl ::std::convert::From<&RegistryUsageListResult> for RegistryUsageListResult {
    fn from(value: &RegistryUsageListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryUsageListResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///The unit of measurement.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The unit of measurement.",
///  "type": "string",
///  "enum": [
///    "Count",
///    "Bytes"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "RegistryUsageUnit"
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
pub enum RegistryUsageUnit {
    Count,
    Bytes,
}
impl ::std::convert::From<&Self> for RegistryUsageUnit {
    fn from(value: &RegistryUsageUnit) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistryUsageUnit {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Count => f.write_str("Count"),
            Self::Bytes => f.write_str("Bytes"),
        }
    }
}
impl ::std::str::FromStr for RegistryUsageUnit {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "count" => Ok(Self::Count),
            "bytes" => Ok(Self::Bytes),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RegistryUsageUnit {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RegistryUsageUnit {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RegistryUsageUnit {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An object that represents a replication for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a replication for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ReplicationProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Replication {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The location of the resource. This cannot be changed after the resource is created.
    pub location: ::std::string::String,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<ReplicationProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The tags of the resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Replication> for Replication {
    fn from(value: &Replication) -> Self {
        value.clone()
    }
}
///The result of a request to list replications for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list replications for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of replications.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of replications. Since this list may be incomplete, the nextLink field should be used to request the next list of replications.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Replication"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ReplicationListResult {
    ///The URI that can be used to request the next list of replications.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of replications. Since this list may be incomplete, the nextLink field should be used to request the next list of replications.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Replication>,
}
impl ::std::convert::From<&ReplicationListResult> for ReplicationListResult {
    fn from(value: &ReplicationListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ReplicationListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a replication.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a replication.",
///  "type": "object",
///  "properties": {
///    "provisioningState": {
///      "description": "The provisioning state of the replication at the time the operation was called.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "regionEndpointEnabled": {
///      "description": "Specifies whether the replication's regional endpoint is enabled. Requests will not be routed to a replication whose regional endpoint is disabled, however its data will continue to be synced with other replications.",
///      "default": true,
///      "type": "boolean"
///    },
///    "status": {
///      "$ref": "#/components/schemas/Status"
///    },
///    "zoneRedundancy": {
///      "description": "Whether or not zone redundancy is enabled for this container registry replication",
///      "default": "Disabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ZoneRedundancy"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ReplicationProperties {
    ///The provisioning state of the replication at the time the operation was called.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ReplicationPropertiesProvisioningState>,
    ///Specifies whether the replication's regional endpoint is enabled. Requests will not be routed to a replication whose regional endpoint is disabled, however its data will continue to be synced with other replications.
    #[serde(
        rename = "regionEndpointEnabled",
        default = "defaults::default_bool::<true>",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub region_endpoint_enabled: bool,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<Status>,
    ///Whether or not zone redundancy is enabled for this container registry replication
    #[serde(
        rename = "zoneRedundancy",
        default = "defaults::replication_properties_zone_redundancy",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zone_redundancy: ReplicationPropertiesZoneRedundancy,
}
impl ::std::convert::From<&ReplicationProperties> for ReplicationProperties {
    fn from(value: &ReplicationProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ReplicationProperties {
    fn default() -> Self {
        Self {
            provisioning_state: Default::default(),
            region_endpoint_enabled: defaults::default_bool::<true>(),
            status: Default::default(),
            zone_redundancy: defaults::replication_properties_zone_redundancy(),
        }
    }
}
///The provisioning state of the replication at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provisioning state of the replication at the time the operation was called.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum ReplicationPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for ReplicationPropertiesProvisioningState {
    fn from(value: &ReplicationPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ReplicationPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for ReplicationPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ReplicationPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ReplicationPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ReplicationPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Whether or not zone redundancy is enabled for this container registry replication
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether or not zone redundancy is enabled for this container registry replication",
///  "default": "Disabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ZoneRedundancy"
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
pub enum ReplicationPropertiesZoneRedundancy {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for ReplicationPropertiesZoneRedundancy {
    fn from(value: &ReplicationPropertiesZoneRedundancy) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ReplicationPropertiesZoneRedundancy {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for ReplicationPropertiesZoneRedundancy {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ReplicationPropertiesZoneRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ReplicationPropertiesZoneRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ReplicationPropertiesZoneRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for ReplicationPropertiesZoneRedundancy {
    fn default() -> Self {
        ReplicationPropertiesZoneRedundancy::Disabled
    }
}
///The parameters for updating a replication.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a replication.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ReplicationUpdateParametersProperties"
///    },
///    "tags": {
///      "description": "The tags for the replication.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ReplicationUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<ReplicationUpdateParametersProperties>,
    ///The tags for the replication.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&ReplicationUpdateParameters> for ReplicationUpdateParameters {
    fn from(value: &ReplicationUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ReplicationUpdateParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
            tags: Default::default(),
        }
    }
}
///`ReplicationUpdateParametersProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "regionEndpointEnabled": {
///      "description": "Specifies whether the replication's regional endpoint is enabled. Requests will not be routed to a replication whose regional endpoint is disabled, however its data will continue to be synced with other replications.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ReplicationUpdateParametersProperties {
    ///Specifies whether the replication's regional endpoint is enabled. Requests will not be routed to a replication whose regional endpoint is disabled, however its data will continue to be synced with other replications.
    #[serde(
        rename = "regionEndpointEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub region_endpoint_enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ReplicationUpdateParametersProperties>
    for ReplicationUpdateParametersProperties
{
    fn from(value: &ReplicationUpdateParametersProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ReplicationUpdateParametersProperties {
    fn default() -> Self {
        Self {
            region_endpoint_enabled: Default::default(),
        }
    }
}
///The request that generated the event.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The request that generated the event.",
///  "type": "object",
///  "properties": {
///    "addr": {
///      "description": "The IP or hostname and possibly port of the client connection that initiated the event. This is the RemoteAddr from the standard http request.",
///      "type": "string"
///    },
///    "host": {
///      "description": "The externally accessible hostname of the registry instance, as specified by the http host header on incoming requests.",
///      "type": "string"
///    },
///    "id": {
///      "description": "The ID of the request that initiated the event.",
///      "type": "string"
///    },
///    "method": {
///      "description": "The request method that generated the event.",
///      "type": "string"
///    },
///    "useragent": {
///      "description": "The user agent header of the request.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Request {
    ///The IP or hostname and possibly port of the client connection that initiated the event. This is the RemoteAddr from the standard http request.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub addr: ::std::option::Option<::std::string::String>,
    ///The externally accessible hostname of the registry instance, as specified by the http host header on incoming requests.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub host: ::std::option::Option<::std::string::String>,
    ///The ID of the request that initiated the event.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The request method that generated the event.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub method: ::std::option::Option<::std::string::String>,
    ///The user agent header of the request.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub useragent: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Request> for Request {
    fn from(value: &Request) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Request {
    fn default() -> Self {
        Self {
            addr: Default::default(),
            host: Default::default(),
            id: Default::default(),
            method: Default::default(),
            useragent: Default::default(),
        }
    }
}
///An Azure resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An Azure resource.",
///  "required": [
///    "location"
///  ],
///  "properties": {
///    "id": {
///      "description": "The resource ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "The location of the resource. This cannot be changed after the resource is created.",
///      "type": "string",
///      "x-ms-mutability": [
///        "read",
///        "create"
///      ]
///    },
///    "name": {
///      "description": "The name of the resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/SystemData"
///    },
///    "tags": {
///      "description": "The tags of the resource.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "The type of the resource.",
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
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The location of the resource. This cannot be changed after the resource is created.
    pub location: ::std::string::String,
    ///The name of the resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The tags of the resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource.
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
///The retention policy for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The retention policy for a container registry.",
///  "type": "object",
///  "properties": {
///    "days": {
///      "description": "The number of days to retain an untagged manifest after which it gets purged.",
///      "default": 7,
///      "type": "integer",
///      "format": "int32"
///    },
///    "lastUpdatedTime": {
///      "description": "The timestamp when the policy was last updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "status": {
///      "description": "The value that indicates whether the policy is enabled or not.",
///      "default": "disabled",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PolicyStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RetentionPolicy {
    ///The number of days to retain an untagged manifest after which it gets purged.
    #[serde(
        default = "defaults::default_u64::<i32, 7>",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days: i32,
    ///The timestamp when the policy was last updated.
    #[serde(
        rename = "lastUpdatedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_updated_time: ::std::option::Option<::std::string::String>,
    ///The value that indicates whether the policy is enabled or not.
    #[serde(
        default = "defaults::retention_policy_status",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: RetentionPolicyStatus,
}
impl ::std::convert::From<&RetentionPolicy> for RetentionPolicy {
    fn from(value: &RetentionPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            days: defaults::default_u64::<i32, 7>(),
            last_updated_time: Default::default(),
            status: defaults::retention_policy_status(),
        }
    }
}
///The value that indicates whether the policy is enabled or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The value that indicates whether the policy is enabled or not.",
///  "default": "disabled",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PolicyStatus"
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
pub enum RetentionPolicyStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for RetentionPolicyStatus {
    fn from(value: &RetentionPolicyStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RetentionPolicyStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for RetentionPolicyStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RetentionPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RetentionPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RetentionPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for RetentionPolicyStatus {
    fn default() -> Self {
        RetentionPolicyStatus::Disabled
    }
}
///An object that represents a scope map for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a scope map for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ScopeMapProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScopeMap {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<ScopeMapProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ScopeMap> for ScopeMap {
    fn from(value: &ScopeMap) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ScopeMap {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The result of a request to list scope maps for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list scope maps for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of scope maps.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of scope maps. Since this list may be incomplete, the nextLink field should be used to request the next list of scope maps.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScopeMap"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScopeMapListResult {
    ///The URI that can be used to request the next list of scope maps.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of scope maps. Since this list may be incomplete, the nextLink field should be used to request the next list of scope maps.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ScopeMap>,
}
impl ::std::convert::From<&ScopeMapListResult> for ScopeMapListResult {
    fn from(value: &ScopeMapListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ScopeMapListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a scope map.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a scope map.",
///  "type": "object",
///  "required": [
///    "actions"
///  ],
///  "properties": {
///    "actions": {
///      "description": "The list of scoped permissions for registry artifacts.\r\nE.g. repositories/repository-name/content/read,\r\nrepositories/repository-name/metadata/write",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "creationDate": {
///      "description": "The creation date of scope map.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "description": {
///      "description": "The user friendly description of the scope map.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the resource.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "type": {
///      "description": "The type of the scope map. E.g. BuildIn scope map.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScopeMapProperties {
    /**The list of scoped permissions for registry artifacts.
    E.g. repositories/repository-name/content/read,
    repositories/repository-name/metadata/write*/
    pub actions: ::std::vec::Vec<::std::string::String>,
    ///The creation date of scope map.
    #[serde(
        rename = "creationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_date: ::std::option::Option<::std::string::String>,
    ///The user friendly description of the scope map.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Provisioning state of the resource.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ScopeMapPropertiesProvisioningState>,
    ///The type of the scope map. E.g. BuildIn scope map.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ScopeMapProperties> for ScopeMapProperties {
    fn from(value: &ScopeMapProperties) -> Self {
        value.clone()
    }
}
///Provisioning state of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the resource.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum ScopeMapPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for ScopeMapPropertiesProvisioningState {
    fn from(value: &ScopeMapPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ScopeMapPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for ScopeMapPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ScopeMapPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ScopeMapPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ScopeMapPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The update parameters for scope map properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The update parameters for scope map properties.",
///  "type": "object",
///  "properties": {
///    "actions": {
///      "description": "The list of scope permissions for registry artifacts.\r\nE.g. repositories/repository-name/pull, \r\nrepositories/repository-name/delete",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "description": {
///      "description": "The user friendly description of the scope map.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScopeMapPropertiesUpdateParameters {
    /**The list of scope permissions for registry artifacts.
    E.g. repositories/repository-name/pull,
    repositories/repository-name/delete*/
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub actions: ::std::vec::Vec<::std::string::String>,
    ///The user friendly description of the scope map.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ScopeMapPropertiesUpdateParameters>
    for ScopeMapPropertiesUpdateParameters
{
    fn from(value: &ScopeMapPropertiesUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ScopeMapPropertiesUpdateParameters {
    fn default() -> Self {
        Self {
            actions: Default::default(),
            description: Default::default(),
        }
    }
}
///The properties for updating the scope map.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties for updating the scope map.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ScopeMapPropertiesUpdateParameters"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScopeMapUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<ScopeMapPropertiesUpdateParameters>,
}
impl ::std::convert::From<&ScopeMapUpdateParameters> for ScopeMapUpdateParameters {
    fn from(value: &ScopeMapUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ScopeMapUpdateParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
        }
    }
}
///The SKU of a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU of a container registry.",
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "name": {
///      "description": "The SKU name of the container registry. Required for registry creation.",
///      "type": "string",
///      "enum": [
///        "Classic",
///        "Basic",
///        "Standard",
///        "Premium"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SkuName"
///      }
///    },
///    "tier": {
///      "description": "The SKU tier based on the SKU name.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Classic",
///        "Basic",
///        "Standard",
///        "Premium"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SkuTier"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Sku {
    ///The SKU name of the container registry. Required for registry creation.
    pub name: SkuName,
    ///The SKU tier based on the SKU name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<SkuTier>,
}
impl ::std::convert::From<&Sku> for Sku {
    fn from(value: &Sku) -> Self {
        value.clone()
    }
}
///The SKU name of the container registry. Required for registry creation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU name of the container registry. Required for registry creation.",
///  "type": "string",
///  "enum": [
///    "Classic",
///    "Basic",
///    "Standard",
///    "Premium"
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
    Classic,
    Basic,
    Standard,
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
            Self::Classic => f.write_str("Classic"),
            Self::Basic => f.write_str("Basic"),
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
        }
    }
}
impl ::std::str::FromStr for SkuName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "classic" => Ok(Self::Classic),
            "basic" => Ok(Self::Basic),
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
///The SKU tier based on the SKU name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU tier based on the SKU name.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Classic",
///    "Basic",
///    "Standard",
///    "Premium"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
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
pub enum SkuTier {
    Classic,
    Basic,
    Standard,
    Premium,
}
impl ::std::convert::From<&Self> for SkuTier {
    fn from(value: &SkuTier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SkuTier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Classic => f.write_str("Classic"),
            Self::Basic => f.write_str("Basic"),
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
        }
    }
}
impl ::std::str::FromStr for SkuTier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "classic" => Ok(Self::Classic),
            "basic" => Ok(Self::Basic),
            "standard" => Ok(Self::Standard),
            "premium" => Ok(Self::Premium),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SkuTier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The registry node that generated the event. Put differently, while the actor initiates the event, the source generates it.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The registry node that generated the event. Put differently, while the actor initiates the event, the source generates it.",
///  "type": "object",
///  "properties": {
///    "addr": {
///      "description": "The IP or hostname and the port of the registry node that generated the event. Generally, this will be resolved by os.Hostname() along with the running port.",
///      "type": "string"
///    },
///    "instanceID": {
///      "description": "The running instance of an application. Changes after each restart.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Source {
    ///The IP or hostname and the port of the registry node that generated the event. Generally, this will be resolved by os.Hostname() along with the running port.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub addr: ::std::option::Option<::std::string::String>,
    ///The running instance of an application. Changes after each restart.
    #[serde(
        rename = "instanceID",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub instance_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Source> for Source {
    fn from(value: &Source) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Source {
    fn default() -> Self {
        Self {
            addr: Default::default(),
            instance_id: Default::default(),
        }
    }
}
///The status of an Azure resource at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of an Azure resource at the time the operation was called.",
///  "type": "object",
///  "properties": {
///    "displayStatus": {
///      "description": "The short label for the status.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "message": {
///      "description": "The detailed message for the status, including alerts and error messages.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "timestamp": {
///      "description": "The timestamp when the status was changed to the current value.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Status {
    ///The short label for the status.
    #[serde(
        rename = "displayStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_status: ::std::option::Option<::std::string::String>,
    ///The detailed message for the status, including alerts and error messages.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///The timestamp when the status was changed to the current value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timestamp: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Status> for Status {
    fn from(value: &Status) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Status {
    fn default() -> Self {
        Self {
            display_status: Default::default(),
            message: Default::default(),
            timestamp: Default::default(),
        }
    }
}
///The status detail properties of the connected registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status detail properties of the connected registry.",
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "The code of the status.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "correlationId": {
///      "description": "The correlation ID of the status.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "description": {
///      "description": "The description of the status.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "timestamp": {
///      "description": "The timestamp of the status.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The component of the connected registry corresponding to the status.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StatusDetailProperties {
    ///The code of the status.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///The correlation ID of the status.
    #[serde(
        rename = "correlationId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub correlation_id: ::std::option::Option<::std::string::String>,
    ///The description of the status.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The timestamp of the status.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timestamp: ::std::option::Option<::std::string::String>,
    ///The component of the connected registry corresponding to the status.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&StatusDetailProperties> for StatusDetailProperties {
    fn from(value: &StatusDetailProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StatusDetailProperties {
    fn default() -> Self {
        Self {
            code: Default::default(),
            correlation_id: Default::default(),
            description: Default::default(),
            timestamp: Default::default(),
            type_: Default::default(),
        }
    }
}
///The properties of a storage account for a container registry. Only applicable to Classic SKU.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a storage account for a container registry. Only applicable to Classic SKU.",
///  "type": "object",
///  "required": [
///    "id"
///  ],
///  "properties": {
///    "id": {
///      "description": "The resource ID of the storage account.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountProperties {
    ///The resource ID of the storage account.
    pub id: ::std::string::String,
}
impl ::std::convert::From<&StorageAccountProperties> for StorageAccountProperties {
    fn from(value: &StorageAccountProperties) -> Self {
        value.clone()
    }
}
///The sync properties of the connected registry with its parent.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The sync properties of the connected registry with its parent.",
///  "type": "object",
///  "required": [
///    "messageTtl",
///    "tokenId"
///  ],
///  "properties": {
///    "gatewayEndpoint": {
///      "description": "The gateway endpoint used by the connected registry to communicate with its parent.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "lastSyncTime": {
///      "description": "The last time a sync occurred between the connected registry and its parent.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "messageTtl": {
///      "description": "The period of time for which a message is available to sync before it is expired. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.",
///      "type": "string",
///      "format": "duration"
///    },
///    "schedule": {
///      "description": "The cron expression indicating the schedule that the connected registry will sync with its parent.",
///      "type": "string"
///    },
///    "syncWindow": {
///      "description": "The time window during which sync is enabled for each schedule occurrence. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.",
///      "type": "string",
///      "format": "duration"
///    },
///    "tokenId": {
///      "description": "The resource ID of the ACR token used to authenticate the connected registry to its parent during sync.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SyncProperties {
    ///The gateway endpoint used by the connected registry to communicate with its parent.
    #[serde(
        rename = "gatewayEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub gateway_endpoint: ::std::option::Option<::std::string::String>,
    ///The last time a sync occurred between the connected registry and its parent.
    #[serde(
        rename = "lastSyncTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_sync_time: ::std::option::Option<::std::string::String>,
    ///The period of time for which a message is available to sync before it is expired. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.
    #[serde(rename = "messageTtl")]
    pub message_ttl: ::std::string::String,
    ///The cron expression indicating the schedule that the connected registry will sync with its parent.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub schedule: ::std::option::Option<::std::string::String>,
    ///The time window during which sync is enabled for each schedule occurrence. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.
    #[serde(
        rename = "syncWindow",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sync_window: ::std::option::Option<::std::string::String>,
    ///The resource ID of the ACR token used to authenticate the connected registry to its parent during sync.
    #[serde(rename = "tokenId")]
    pub token_id: ::std::string::String,
}
impl ::std::convert::From<&SyncProperties> for SyncProperties {
    fn from(value: &SyncProperties) -> Self {
        value.clone()
    }
}
///The parameters for updating the sync properties of the connected registry with its parent.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating the sync properties of the connected registry with its parent.",
///  "type": "object",
///  "properties": {
///    "messageTtl": {
///      "description": "The period of time for which a message is available to sync before it is expired. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.",
///      "type": "string",
///      "format": "duration"
///    },
///    "schedule": {
///      "description": "The cron expression indicating the schedule that the connected registry will sync with its parent.",
///      "type": "string"
///    },
///    "syncWindow": {
///      "description": "The time window during which sync is enabled for each schedule occurrence. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.",
///      "type": "string",
///      "format": "duration"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SyncUpdateProperties {
    ///The period of time for which a message is available to sync before it is expired. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.
    #[serde(
        rename = "messageTtl",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message_ttl: ::std::option::Option<::std::string::String>,
    ///The cron expression indicating the schedule that the connected registry will sync with its parent.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub schedule: ::std::option::Option<::std::string::String>,
    ///The time window during which sync is enabled for each schedule occurrence. Specify the duration using the format P[n]Y[n]M[n]DT[n]H[n]M[n]S as per ISO8601.
    #[serde(
        rename = "syncWindow",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sync_window: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SyncUpdateProperties> for SyncUpdateProperties {
    fn from(value: &SyncUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SyncUpdateProperties {
    fn default() -> Self {
        Self {
            message_ttl: Default::default(),
            schedule: Default::default(),
            sync_window: Default::default(),
        }
    }
}
///Metadata pertaining to creation and last modification of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Metadata pertaining to creation and last modification of the resource.",
///  "readOnly": true,
///  "type": "object",
///  "properties": {
///    "createdAt": {
///      "description": "The timestamp of resource creation (UTC).",
///      "type": "string"
///    },
///    "createdBy": {
///      "description": "The identity that created the resource.",
///      "type": "string"
///    },
///    "createdByType": {
///      "description": "The type of identity that created the resource.",
///      "type": "string",
///      "enum": [
///        "User",
///        "Application",
///        "ManagedIdentity",
///        "Key"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "createdByType"
///      }
///    },
///    "lastModifiedAt": {
///      "description": "The timestamp of resource modification (UTC).",
///      "type": "string"
///    },
///    "lastModifiedBy": {
///      "description": "The identity that last modified the resource.",
///      "type": "string"
///    },
///    "lastModifiedByType": {
///      "description": "The type of identity that last modified the resource.",
///      "type": "string",
///      "enum": [
///        "User",
///        "Application",
///        "ManagedIdentity",
///        "Key"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "lastModifiedByType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SystemData {
    ///The timestamp of resource creation (UTC).
    #[serde(
        rename = "createdAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_at: ::std::option::Option<::std::string::String>,
    ///The identity that created the resource.
    #[serde(
        rename = "createdBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_by: ::std::option::Option<::std::string::String>,
    ///The type of identity that created the resource.
    #[serde(
        rename = "createdByType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_by_type: ::std::option::Option<SystemDataCreatedByType>,
    ///The timestamp of resource modification (UTC).
    #[serde(
        rename = "lastModifiedAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_at: ::std::option::Option<::std::string::String>,
    ///The identity that last modified the resource.
    #[serde(
        rename = "lastModifiedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_by: ::std::option::Option<::std::string::String>,
    ///The type of identity that last modified the resource.
    #[serde(
        rename = "lastModifiedByType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_by_type: ::std::option::Option<SystemDataLastModifiedByType>,
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
///The type of identity that created the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of identity that created the resource.",
///  "type": "string",
///  "enum": [
///    "User",
///    "Application",
///    "ManagedIdentity",
///    "Key"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "createdByType"
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
pub enum SystemDataCreatedByType {
    User,
    Application,
    ManagedIdentity,
    Key,
}
impl ::std::convert::From<&Self> for SystemDataCreatedByType {
    fn from(value: &SystemDataCreatedByType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SystemDataCreatedByType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::User => f.write_str("User"),
            Self::Application => f.write_str("Application"),
            Self::ManagedIdentity => f.write_str("ManagedIdentity"),
            Self::Key => f.write_str("Key"),
        }
    }
}
impl ::std::str::FromStr for SystemDataCreatedByType {
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
impl ::std::convert::TryFrom<&str> for SystemDataCreatedByType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SystemDataCreatedByType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SystemDataCreatedByType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The type of identity that last modified the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of identity that last modified the resource.",
///  "type": "string",
///  "enum": [
///    "User",
///    "Application",
///    "ManagedIdentity",
///    "Key"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "lastModifiedByType"
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
pub enum SystemDataLastModifiedByType {
    User,
    Application,
    ManagedIdentity,
    Key,
}
impl ::std::convert::From<&Self> for SystemDataLastModifiedByType {
    fn from(value: &SystemDataLastModifiedByType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SystemDataLastModifiedByType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::User => f.write_str("User"),
            Self::Application => f.write_str("Application"),
            Self::ManagedIdentity => f.write_str("ManagedIdentity"),
            Self::Key => f.write_str("Key"),
        }
    }
}
impl ::std::str::FromStr for SystemDataLastModifiedByType {
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
impl ::std::convert::TryFrom<&str> for SystemDataLastModifiedByType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SystemDataLastModifiedByType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SystemDataLastModifiedByType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The target of the event.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The target of the event.",
///  "type": "object",
///  "properties": {
///    "digest": {
///      "description": "The digest of the content, as defined by the Registry V2 HTTP API Specification.",
///      "type": "string"
///    },
///    "length": {
///      "description": "The number of bytes of the content. Same as Size field.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "mediaType": {
///      "description": "The MIME type of the referenced object.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the artifact.",
///      "type": "string"
///    },
///    "repository": {
///      "description": "The repository name.",
///      "type": "string"
///    },
///    "size": {
///      "description": "The number of bytes of the content. Same as Length field.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "tag": {
///      "description": "The tag name.",
///      "type": "string"
///    },
///    "url": {
///      "description": "The direct URL to the content.",
///      "type": "string"
///    },
///    "version": {
///      "description": "The version of the artifact.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Target {
    ///The digest of the content, as defined by the Registry V2 HTTP API Specification.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub digest: ::std::option::Option<::std::string::String>,
    ///The number of bytes of the content. Same as Size field.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub length: ::std::option::Option<i64>,
    ///The MIME type of the referenced object.
    #[serde(
        rename = "mediaType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub media_type: ::std::option::Option<::std::string::String>,
    ///The name of the artifact.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The repository name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub repository: ::std::option::Option<::std::string::String>,
    ///The number of bytes of the content. Same as Length field.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub size: ::std::option::Option<i64>,
    ///The tag name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag: ::std::option::Option<::std::string::String>,
    ///The direct URL to the content.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub url: ::std::option::Option<::std::string::String>,
    ///The version of the artifact.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Target> for Target {
    fn from(value: &Target) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Target {
    fn default() -> Self {
        Self {
            digest: Default::default(),
            length: Default::default(),
            media_type: Default::default(),
            name: Default::default(),
            repository: Default::default(),
            size: Default::default(),
            tag: Default::default(),
            url: Default::default(),
            version: Default::default(),
        }
    }
}
///The TLS certificate properties of the connected registry login server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The TLS certificate properties of the connected registry login server.",
///  "type": "object",
///  "properties": {
///    "location": {
///      "description": "Indicates the location of the certificates.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of certificate location.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "LocalDirectory"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "CertificateType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TlsCertificateProperties {
    ///Indicates the location of the certificates.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///The type of certificate location.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<TlsCertificatePropertiesType>,
}
impl ::std::convert::From<&TlsCertificateProperties> for TlsCertificateProperties {
    fn from(value: &TlsCertificateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TlsCertificateProperties {
    fn default() -> Self {
        Self {
            location: Default::default(),
            type_: Default::default(),
        }
    }
}
///The type of certificate location.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of certificate location.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "LocalDirectory"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CertificateType"
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
pub enum TlsCertificatePropertiesType {
    LocalDirectory,
}
impl ::std::convert::From<&Self> for TlsCertificatePropertiesType {
    fn from(value: &TlsCertificatePropertiesType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TlsCertificatePropertiesType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::LocalDirectory => f.write_str("LocalDirectory"),
        }
    }
}
impl ::std::str::FromStr for TlsCertificatePropertiesType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "localdirectory" => Ok(Self::LocalDirectory),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TlsCertificatePropertiesType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TlsCertificatePropertiesType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TlsCertificatePropertiesType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The TLS properties of the connected registry login server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The TLS properties of the connected registry login server.",
///  "type": "object",
///  "properties": {
///    "certificate": {
///      "$ref": "#/components/schemas/TlsCertificateProperties"
///    },
///    "status": {
///      "description": "Indicates whether HTTPS is enabled for the login server.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TlsStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TlsProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificate: ::std::option::Option<TlsCertificateProperties>,
    ///Indicates whether HTTPS is enabled for the login server.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<TlsPropertiesStatus>,
}
impl ::std::convert::From<&TlsProperties> for TlsProperties {
    fn from(value: &TlsProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TlsProperties {
    fn default() -> Self {
        Self {
            certificate: Default::default(),
            status: Default::default(),
        }
    }
}
///Indicates whether HTTPS is enabled for the login server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates whether HTTPS is enabled for the login server.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TlsStatus"
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
pub enum TlsPropertiesStatus {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for TlsPropertiesStatus {
    fn from(value: &TlsPropertiesStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TlsPropertiesStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for TlsPropertiesStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TlsPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TlsPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TlsPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An object that represents a token for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a token for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/TokenProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Token {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<TokenProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Token> for Token {
    fn from(value: &Token) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Token {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The properties of a certificate used for authenticating a token.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a certificate used for authenticating a token.",
///  "type": "object",
///  "properties": {
///    "encodedPemCertificate": {
///      "description": "Base 64 encoded string of the public certificate1 in PEM format that will be used for authenticating the token.",
///      "type": "string"
///    },
///    "expiry": {
///      "description": "The expiry datetime of the certificate.",
///      "type": "string"
///    },
///    "name": {
///      "type": "string",
///      "enum": [
///        "certificate1",
///        "certificate2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TokenCertificateName"
///      }
///    },
///    "thumbprint": {
///      "description": "The thumbprint of the certificate.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenCertificate {
    ///Base 64 encoded string of the public certificate1 in PEM format that will be used for authenticating the token.
    #[serde(
        rename = "encodedPemCertificate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encoded_pem_certificate: ::std::option::Option<::std::string::String>,
    ///The expiry datetime of the certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expiry: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<TokenCertificateName>,
    ///The thumbprint of the certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub thumbprint: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TokenCertificate> for TokenCertificate {
    fn from(value: &TokenCertificate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenCertificate {
    fn default() -> Self {
        Self {
            encoded_pem_certificate: Default::default(),
            expiry: Default::default(),
            name: Default::default(),
            thumbprint: Default::default(),
        }
    }
}
///`TokenCertificateName`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "certificate1",
///    "certificate2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TokenCertificateName"
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
pub enum TokenCertificateName {
    #[serde(rename = "certificate1")]
    Certificate1,
    #[serde(rename = "certificate2")]
    Certificate2,
}
impl ::std::convert::From<&Self> for TokenCertificateName {
    fn from(value: &TokenCertificateName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TokenCertificateName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Certificate1 => f.write_str("certificate1"),
            Self::Certificate2 => f.write_str("certificate2"),
        }
    }
}
impl ::std::str::FromStr for TokenCertificateName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "certificate1" => Ok(Self::Certificate1),
            "certificate2" => Ok(Self::Certificate2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TokenCertificateName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TokenCertificateName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TokenCertificateName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The properties of the credentials that can be used for authenticating the token.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of the credentials that can be used for authenticating the token.",
///  "type": "object",
///  "properties": {
///    "certificates": {
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TokenCertificate"
///      },
///      "x-ms-identifiers": [
///        "thumbprint"
///      ]
///    },
///    "passwords": {
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TokenPassword"
///      },
///      "x-ms-identifiers": []
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenCredentialsProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificates: ::std::vec::Vec<TokenCertificate>,
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub passwords: ::std::vec::Vec<TokenPassword>,
}
impl ::std::convert::From<&TokenCredentialsProperties> for TokenCredentialsProperties {
    fn from(value: &TokenCredentialsProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenCredentialsProperties {
    fn default() -> Self {
        Self {
            certificates: Default::default(),
            passwords: Default::default(),
        }
    }
}
///The result of a request to list tokens for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list tokens for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of tokens.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of tokens. Since this list may be incomplete, the nextLink field should be used to request the next list of tokens.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Token"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenListResult {
    ///The URI that can be used to request the next list of tokens.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of tokens. Since this list may be incomplete, the nextLink field should be used to request the next list of tokens.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Token>,
}
impl ::std::convert::From<&TokenListResult> for TokenListResult {
    fn from(value: &TokenListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The password that will be used for authenticating the token of a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The password that will be used for authenticating the token of a container registry.",
///  "type": "object",
///  "properties": {
///    "creationTime": {
///      "description": "The creation datetime of the password.",
///      "type": "string"
///    },
///    "expiry": {
///      "description": "The expiry datetime of the password.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The password name \"password1\" or \"password2\"",
///      "type": "string",
///      "enum": [
///        "password1",
///        "password2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TokenPasswordName"
///      }
///    },
///    "value": {
///      "description": "The password value.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenPassword {
    ///The creation datetime of the password.
    #[serde(
        rename = "creationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_time: ::std::option::Option<::std::string::String>,
    ///The expiry datetime of the password.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expiry: ::std::option::Option<::std::string::String>,
    ///The password name "password1" or "password2"
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<TokenPasswordName>,
    ///The password value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TokenPassword> for TokenPassword {
    fn from(value: &TokenPassword) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenPassword {
    fn default() -> Self {
        Self {
            creation_time: Default::default(),
            expiry: Default::default(),
            name: Default::default(),
            value: Default::default(),
        }
    }
}
///The password name "password1" or "password2"
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The password name \"password1\" or \"password2\"",
///  "type": "string",
///  "enum": [
///    "password1",
///    "password2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TokenPasswordName"
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
pub enum TokenPasswordName {
    #[serde(rename = "password1")]
    Password1,
    #[serde(rename = "password2")]
    Password2,
}
impl ::std::convert::From<&Self> for TokenPasswordName {
    fn from(value: &TokenPasswordName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TokenPasswordName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Password1 => f.write_str("password1"),
            Self::Password2 => f.write_str("password2"),
        }
    }
}
impl ::std::str::FromStr for TokenPasswordName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "password1" => Ok(Self::Password1),
            "password2" => Ok(Self::Password2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TokenPasswordName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TokenPasswordName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TokenPasswordName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The properties of a token.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a token.",
///  "type": "object",
///  "properties": {
///    "creationDate": {
///      "description": "The creation date of scope map.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "credentials": {
///      "$ref": "#/components/schemas/TokenCredentialsProperties"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the resource.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "scopeMapId": {
///      "description": "The resource ID of the scope map to which the token will be associated with.",
///      "type": "string"
///    },
///    "status": {
///      "description": "The status of the token example enabled or disabled.",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TokenStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenProperties {
    ///The creation date of scope map.
    #[serde(
        rename = "creationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_date: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credentials: ::std::option::Option<TokenCredentialsProperties>,
    ///Provisioning state of the resource.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<TokenPropertiesProvisioningState>,
    ///The resource ID of the scope map to which the token will be associated with.
    #[serde(
        rename = "scopeMapId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scope_map_id: ::std::option::Option<::std::string::String>,
    ///The status of the token example enabled or disabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<TokenPropertiesStatus>,
}
impl ::std::convert::From<&TokenProperties> for TokenProperties {
    fn from(value: &TokenProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenProperties {
    fn default() -> Self {
        Self {
            creation_date: Default::default(),
            credentials: Default::default(),
            provisioning_state: Default::default(),
            scope_map_id: Default::default(),
            status: Default::default(),
        }
    }
}
///Provisioning state of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the resource.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum TokenPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for TokenPropertiesProvisioningState {
    fn from(value: &TokenPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TokenPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for TokenPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TokenPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TokenPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TokenPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The status of the token example enabled or disabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of the token example enabled or disabled.",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TokenStatus"
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
pub enum TokenPropertiesStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for TokenPropertiesStatus {
    fn from(value: &TokenPropertiesStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TokenPropertiesStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for TokenPropertiesStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TokenPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TokenPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TokenPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for updating a token.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a token.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/TokenUpdateProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<TokenUpdateProperties>,
}
impl ::std::convert::From<&TokenUpdateParameters> for TokenUpdateParameters {
    fn from(value: &TokenUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenUpdateParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
        }
    }
}
///The parameters for updating token properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating token properties.",
///  "type": "object",
///  "properties": {
///    "credentials": {
///      "$ref": "#/components/schemas/TokenCredentialsProperties"
///    },
///    "scopeMapId": {
///      "description": "The resource ID of the scope map to which the token will be associated with.",
///      "type": "string"
///    },
///    "status": {
///      "description": "The status of the token example enabled or disabled.",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TokenStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TokenUpdateProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credentials: ::std::option::Option<TokenCredentialsProperties>,
    ///The resource ID of the scope map to which the token will be associated with.
    #[serde(
        rename = "scopeMapId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scope_map_id: ::std::option::Option<::std::string::String>,
    ///The status of the token example enabled or disabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<TokenUpdatePropertiesStatus>,
}
impl ::std::convert::From<&TokenUpdateProperties> for TokenUpdateProperties {
    fn from(value: &TokenUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TokenUpdateProperties {
    fn default() -> Self {
        Self {
            credentials: Default::default(),
            scope_map_id: Default::default(),
            status: Default::default(),
        }
    }
}
///The status of the token example enabled or disabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of the token example enabled or disabled.",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TokenStatus"
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
pub enum TokenUpdatePropertiesStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for TokenUpdatePropertiesStatus {
    fn from(value: &TokenUpdatePropertiesStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TokenUpdatePropertiesStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for TokenUpdatePropertiesStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TokenUpdatePropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TokenUpdatePropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TokenUpdatePropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The content trust policy for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The content trust policy for a container registry.",
///  "type": "object",
///  "properties": {
///    "status": {
///      "description": "The value that indicates whether the policy is enabled or not.",
///      "default": "disabled",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PolicyStatus"
///      }
///    },
///    "type": {
///      "description": "The type of trust policy.",
///      "default": "Notary",
///      "type": "string",
///      "enum": [
///        "Notary"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TrustPolicyType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TrustPolicy {
    ///The value that indicates whether the policy is enabled or not.
    #[serde(
        default = "defaults::trust_policy_status",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: TrustPolicyStatus,
    ///The type of trust policy.
    #[serde(
        rename = "type",
        default = "defaults::trust_policy_type",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: TrustPolicyType,
}
impl ::std::convert::From<&TrustPolicy> for TrustPolicy {
    fn from(value: &TrustPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TrustPolicy {
    fn default() -> Self {
        Self {
            status: defaults::trust_policy_status(),
            type_: defaults::trust_policy_type(),
        }
    }
}
///The value that indicates whether the policy is enabled or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The value that indicates whether the policy is enabled or not.",
///  "default": "disabled",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PolicyStatus"
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
pub enum TrustPolicyStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for TrustPolicyStatus {
    fn from(value: &TrustPolicyStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TrustPolicyStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for TrustPolicyStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TrustPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TrustPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TrustPolicyStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for TrustPolicyStatus {
    fn default() -> Self {
        TrustPolicyStatus::Disabled
    }
}
///The type of trust policy.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of trust policy.",
///  "default": "Notary",
///  "type": "string",
///  "enum": [
///    "Notary"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TrustPolicyType"
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
pub enum TrustPolicyType {
    Notary,
}
impl ::std::convert::From<&Self> for TrustPolicyType {
    fn from(value: &TrustPolicyType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TrustPolicyType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Notary => f.write_str("Notary"),
        }
    }
}
impl ::std::str::FromStr for TrustPolicyType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notary" => Ok(Self::Notary),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TrustPolicyType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TrustPolicyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TrustPolicyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for TrustPolicyType {
    fn default() -> Self {
        TrustPolicyType::Notary
    }
}
///`UserIdentityProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "clientId": {
///      "description": "The client id of user assigned identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "principalId": {
///      "description": "The principal id of user assigned identity.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserIdentityProperties {
    ///The client id of user assigned identity.
    #[serde(
        rename = "clientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_id: ::std::option::Option<::std::string::String>,
    ///The principal id of user assigned identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&UserIdentityProperties> for UserIdentityProperties {
    fn from(value: &UserIdentityProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UserIdentityProperties {
    fn default() -> Self {
        Self {
            client_id: Default::default(),
            principal_id: Default::default(),
        }
    }
}
///An object that represents a webhook for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that represents a webhook for a container registry.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/WebhookProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Webhook {
    ///The resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The location of the resource. This cannot be changed after the resource is created.
    pub location: ::std::string::String,
    ///The name of the resource.
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
    pub properties: ::std::option::Option<WebhookProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The tags of the resource.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Webhook> for Webhook {
    fn from(value: &Webhook) -> Self {
        value.clone()
    }
}
///The parameters for creating a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for creating a webhook.",
///  "type": "object",
///  "required": [
///    "location"
///  ],
///  "properties": {
///    "location": {
///      "description": "The location of the webhook. This cannot be changed after the resource is created.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/WebhookPropertiesCreateParameters"
///    },
///    "tags": {
///      "description": "The tags for the webhook.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WebhookCreateParameters {
    ///The location of the webhook. This cannot be changed after the resource is created.
    pub location: ::std::string::String,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<WebhookPropertiesCreateParameters>,
    ///The tags for the webhook.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&WebhookCreateParameters> for WebhookCreateParameters {
    fn from(value: &WebhookCreateParameters) -> Self {
        value.clone()
    }
}
///The result of a request to list webhooks for a container registry.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The result of a request to list webhooks for a container registry.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URI that can be used to request the next list of webhooks.",
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of webhooks. Since this list may be incomplete, the nextLink field should be used to request the next list of webhooks.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Webhook"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WebhookListResult {
    ///The URI that can be used to request the next list of webhooks.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of webhooks. Since this list may be incomplete, the nextLink field should be used to request the next list of webhooks.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Webhook>,
}
impl ::std::convert::From<&WebhookListResult> for WebhookListResult {
    fn from(value: &WebhookListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for WebhookListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a webhook.",
///  "type": "object",
///  "required": [
///    "actions"
///  ],
///  "properties": {
///    "actions": {
///      "description": "The list of actions that trigger the webhook to post notifications.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "push",
///          "delete",
///          "quarantine",
///          "chart_push",
///          "chart_delete"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "WebhookAction"
///        }
///      }
///    },
///    "provisioningState": {
///      "description": "The provisioning state of the webhook at the time the operation was called.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Failed",
///        "Canceled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "scope": {
///      "description": "The scope of repositories where the event can be triggered. For example, 'foo:*' means events for all tags under repository 'foo'. 'foo:bar' means events for 'foo:bar' only. 'foo' is equivalent to 'foo:latest'. Empty means all events.",
///      "type": "string"
///    },
///    "status": {
///      "description": "The status of the webhook at the time the operation was called.",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "WebhookStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WebhookProperties {
    ///The list of actions that trigger the webhook to post notifications.
    pub actions: ::std::vec::Vec<WebhookPropertiesActionsItem>,
    ///The provisioning state of the webhook at the time the operation was called.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<WebhookPropertiesProvisioningState>,
    ///The scope of repositories where the event can be triggered. For example, 'foo:*' means events for all tags under repository 'foo'. 'foo:bar' means events for 'foo:bar' only. 'foo' is equivalent to 'foo:latest'. Empty means all events.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scope: ::std::option::Option<::std::string::String>,
    ///The status of the webhook at the time the operation was called.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<WebhookPropertiesStatus>,
}
impl ::std::convert::From<&WebhookProperties> for WebhookProperties {
    fn from(value: &WebhookProperties) -> Self {
        value.clone()
    }
}
///`WebhookPropertiesActionsItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "push",
///    "delete",
///    "quarantine",
///    "chart_push",
///    "chart_delete"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "WebhookAction"
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
pub enum WebhookPropertiesActionsItem {
    #[serde(rename = "push")]
    Push,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "quarantine")]
    Quarantine,
    #[serde(rename = "chart_push")]
    ChartPush,
    #[serde(rename = "chart_delete")]
    ChartDelete,
}
impl ::std::convert::From<&Self> for WebhookPropertiesActionsItem {
    fn from(value: &WebhookPropertiesActionsItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesActionsItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Push => f.write_str("push"),
            Self::Delete => f.write_str("delete"),
            Self::Quarantine => f.write_str("quarantine"),
            Self::ChartPush => f.write_str("chart_push"),
            Self::ChartDelete => f.write_str("chart_delete"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesActionsItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "push" => Ok(Self::Push),
            "delete" => Ok(Self::Delete),
            "quarantine" => Ok(Self::Quarantine),
            "chart_push" => Ok(Self::ChartPush),
            "chart_delete" => Ok(Self::ChartDelete),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesActionsItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for WebhookPropertiesActionsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for WebhookPropertiesActionsItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for creating the properties of a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for creating the properties of a webhook.",
///  "type": "object",
///  "required": [
///    "actions",
///    "serviceUri"
///  ],
///  "properties": {
///    "actions": {
///      "description": "The list of actions that trigger the webhook to post notifications.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "push",
///          "delete",
///          "quarantine",
///          "chart_push",
///          "chart_delete"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "WebhookAction"
///        }
///      }
///    },
///    "customHeaders": {
///      "description": "Custom headers that will be added to the webhook notifications.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      },
///      "x-ms-secret": true
///    },
///    "scope": {
///      "description": "The scope of repositories where the event can be triggered. For example, 'foo:*' means events for all tags under repository 'foo'. 'foo:bar' means events for 'foo:bar' only. 'foo' is equivalent to 'foo:latest'. Empty means all events.",
///      "type": "string"
///    },
///    "serviceUri": {
///      "description": "The service URI for the webhook to post notifications.",
///      "type": "string",
///      "x-ms-secret": true
///    },
///    "status": {
///      "description": "The status of the webhook at the time the operation was called.",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "WebhookStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WebhookPropertiesCreateParameters {
    ///The list of actions that trigger the webhook to post notifications.
    pub actions: ::std::vec::Vec<WebhookPropertiesCreateParametersActionsItem>,
    ///Custom headers that will be added to the webhook notifications.
    #[serde(
        rename = "customHeaders",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_headers: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The scope of repositories where the event can be triggered. For example, 'foo:*' means events for all tags under repository 'foo'. 'foo:bar' means events for 'foo:bar' only. 'foo' is equivalent to 'foo:latest'. Empty means all events.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scope: ::std::option::Option<::std::string::String>,
    ///The service URI for the webhook to post notifications.
    #[serde(rename = "serviceUri")]
    pub service_uri: ::std::string::String,
    ///The status of the webhook at the time the operation was called.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<WebhookPropertiesCreateParametersStatus>,
}
impl ::std::convert::From<&WebhookPropertiesCreateParameters>
    for WebhookPropertiesCreateParameters
{
    fn from(value: &WebhookPropertiesCreateParameters) -> Self {
        value.clone()
    }
}
///`WebhookPropertiesCreateParametersActionsItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "push",
///    "delete",
///    "quarantine",
///    "chart_push",
///    "chart_delete"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "WebhookAction"
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
pub enum WebhookPropertiesCreateParametersActionsItem {
    #[serde(rename = "push")]
    Push,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "quarantine")]
    Quarantine,
    #[serde(rename = "chart_push")]
    ChartPush,
    #[serde(rename = "chart_delete")]
    ChartDelete,
}
impl ::std::convert::From<&Self> for WebhookPropertiesCreateParametersActionsItem {
    fn from(value: &WebhookPropertiesCreateParametersActionsItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesCreateParametersActionsItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Push => f.write_str("push"),
            Self::Delete => f.write_str("delete"),
            Self::Quarantine => f.write_str("quarantine"),
            Self::ChartPush => f.write_str("chart_push"),
            Self::ChartDelete => f.write_str("chart_delete"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesCreateParametersActionsItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "push" => Ok(Self::Push),
            "delete" => Ok(Self::Delete),
            "quarantine" => Ok(Self::Quarantine),
            "chart_push" => Ok(Self::ChartPush),
            "chart_delete" => Ok(Self::ChartDelete),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesCreateParametersActionsItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for WebhookPropertiesCreateParametersActionsItem
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for WebhookPropertiesCreateParametersActionsItem
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The status of the webhook at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of the webhook at the time the operation was called.",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "WebhookStatus"
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
pub enum WebhookPropertiesCreateParametersStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for WebhookPropertiesCreateParametersStatus {
    fn from(value: &WebhookPropertiesCreateParametersStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesCreateParametersStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesCreateParametersStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesCreateParametersStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for WebhookPropertiesCreateParametersStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for WebhookPropertiesCreateParametersStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The provisioning state of the webhook at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provisioning state of the webhook at the time the operation was called.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Failed",
///    "Canceled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisioningState"
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
pub enum WebhookPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Failed,
    Canceled,
}
impl ::std::convert::From<&Self> for WebhookPropertiesProvisioningState {
    fn from(value: &WebhookPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for WebhookPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for WebhookPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The status of the webhook at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of the webhook at the time the operation was called.",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "WebhookStatus"
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
pub enum WebhookPropertiesStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for WebhookPropertiesStatus {
    fn from(value: &WebhookPropertiesStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for WebhookPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for WebhookPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for updating the properties of a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating the properties of a webhook.",
///  "type": "object",
///  "properties": {
///    "actions": {
///      "description": "The list of actions that trigger the webhook to post notifications.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "enum": [
///          "push",
///          "delete",
///          "quarantine",
///          "chart_push",
///          "chart_delete"
///        ],
///        "x-ms-enum": {
///          "modelAsString": true,
///          "name": "WebhookAction"
///        }
///      }
///    },
///    "customHeaders": {
///      "description": "Custom headers that will be added to the webhook notifications.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      },
///      "x-ms-secret": true
///    },
///    "scope": {
///      "description": "The scope of repositories where the event can be triggered. For example, 'foo:*' means events for all tags under repository 'foo'. 'foo:bar' means events for 'foo:bar' only. 'foo' is equivalent to 'foo:latest'. Empty means all events.",
///      "type": "string"
///    },
///    "serviceUri": {
///      "description": "The service URI for the webhook to post notifications.",
///      "type": "string",
///      "x-ms-secret": true
///    },
///    "status": {
///      "description": "The status of the webhook at the time the operation was called.",
///      "type": "string",
///      "enum": [
///        "enabled",
///        "disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "WebhookStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WebhookPropertiesUpdateParameters {
    ///The list of actions that trigger the webhook to post notifications.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub actions: ::std::vec::Vec<WebhookPropertiesUpdateParametersActionsItem>,
    ///Custom headers that will be added to the webhook notifications.
    #[serde(
        rename = "customHeaders",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_headers: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The scope of repositories where the event can be triggered. For example, 'foo:*' means events for all tags under repository 'foo'. 'foo:bar' means events for 'foo:bar' only. 'foo' is equivalent to 'foo:latest'. Empty means all events.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scope: ::std::option::Option<::std::string::String>,
    ///The service URI for the webhook to post notifications.
    #[serde(
        rename = "serviceUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_uri: ::std::option::Option<::std::string::String>,
    ///The status of the webhook at the time the operation was called.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<WebhookPropertiesUpdateParametersStatus>,
}
impl ::std::convert::From<&WebhookPropertiesUpdateParameters>
    for WebhookPropertiesUpdateParameters
{
    fn from(value: &WebhookPropertiesUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for WebhookPropertiesUpdateParameters {
    fn default() -> Self {
        Self {
            actions: Default::default(),
            custom_headers: Default::default(),
            scope: Default::default(),
            service_uri: Default::default(),
            status: Default::default(),
        }
    }
}
///`WebhookPropertiesUpdateParametersActionsItem`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "string",
///  "enum": [
///    "push",
///    "delete",
///    "quarantine",
///    "chart_push",
///    "chart_delete"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "WebhookAction"
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
pub enum WebhookPropertiesUpdateParametersActionsItem {
    #[serde(rename = "push")]
    Push,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "quarantine")]
    Quarantine,
    #[serde(rename = "chart_push")]
    ChartPush,
    #[serde(rename = "chart_delete")]
    ChartDelete,
}
impl ::std::convert::From<&Self> for WebhookPropertiesUpdateParametersActionsItem {
    fn from(value: &WebhookPropertiesUpdateParametersActionsItem) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesUpdateParametersActionsItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Push => f.write_str("push"),
            Self::Delete => f.write_str("delete"),
            Self::Quarantine => f.write_str("quarantine"),
            Self::ChartPush => f.write_str("chart_push"),
            Self::ChartDelete => f.write_str("chart_delete"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesUpdateParametersActionsItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "push" => Ok(Self::Push),
            "delete" => Ok(Self::Delete),
            "quarantine" => Ok(Self::Quarantine),
            "chart_push" => Ok(Self::ChartPush),
            "chart_delete" => Ok(Self::ChartDelete),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesUpdateParametersActionsItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for WebhookPropertiesUpdateParametersActionsItem
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for WebhookPropertiesUpdateParametersActionsItem
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The status of the webhook at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of the webhook at the time the operation was called.",
///  "type": "string",
///  "enum": [
///    "enabled",
///    "disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "WebhookStatus"
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
pub enum WebhookPropertiesUpdateParametersStatus {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl ::std::convert::From<&Self> for WebhookPropertiesUpdateParametersStatus {
    fn from(value: &WebhookPropertiesUpdateParametersStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for WebhookPropertiesUpdateParametersStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("enabled"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
impl ::std::str::FromStr for WebhookPropertiesUpdateParametersStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for WebhookPropertiesUpdateParametersStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for WebhookPropertiesUpdateParametersStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for WebhookPropertiesUpdateParametersStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters for updating a webhook.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters for updating a webhook.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/WebhookPropertiesUpdateParameters"
///    },
///    "tags": {
///      "description": "The tags for the webhook.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WebhookUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<WebhookPropertiesUpdateParameters>,
    ///The tags for the webhook.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&WebhookUpdateParameters> for WebhookUpdateParameters {
    fn from(value: &WebhookUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for WebhookUpdateParameters {
    fn default() -> Self {
        Self {
            properties: Default::default(),
            tags: Default::default(),
        }
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
    pub(super) fn azure_ad_authentication_as_arm_policy_status(
    ) -> super::AzureAdAuthenticationAsArmPolicyStatus {
        super::AzureAdAuthenticationAsArmPolicyStatus::Enabled
    }
    pub(super) fn export_policy_status() -> super::ExportPolicyStatus {
        super::ExportPolicyStatus::Enabled
    }
    pub(super) fn import_image_parameters_mode() -> super::ImportImageParametersMode {
        super::ImportImageParametersMode::NoForce
    }
    pub(super) fn ip_rule_action() -> super::IpRuleAction {
        super::IpRuleAction::Allow
    }
    pub(super) fn logging_properties_audit_log_status() -> super::LoggingPropertiesAuditLogStatus {
        super::LoggingPropertiesAuditLogStatus::Disabled
    }
    pub(super) fn logging_properties_log_level() -> super::LoggingPropertiesLogLevel {
        super::LoggingPropertiesLogLevel::Information
    }
    pub(super) fn quarantine_policy_status() -> super::QuarantinePolicyStatus {
        super::QuarantinePolicyStatus::Disabled
    }
    pub(super) fn registry_properties_network_rule_bypass_options(
    ) -> super::RegistryPropertiesNetworkRuleBypassOptions {
        super::RegistryPropertiesNetworkRuleBypassOptions::AzureServices
    }
    pub(super) fn registry_properties_public_network_access(
    ) -> super::RegistryPropertiesPublicNetworkAccess {
        super::RegistryPropertiesPublicNetworkAccess::Enabled
    }
    pub(super) fn registry_properties_zone_redundancy() -> super::RegistryPropertiesZoneRedundancy {
        super::RegistryPropertiesZoneRedundancy::Disabled
    }
    pub(super) fn registry_properties_update_parameters_network_rule_bypass_options(
    ) -> super::RegistryPropertiesUpdateParametersNetworkRuleBypassOptions {
        super::RegistryPropertiesUpdateParametersNetworkRuleBypassOptions::AzureServices
    }
    pub(super) fn replication_properties_zone_redundancy(
    ) -> super::ReplicationPropertiesZoneRedundancy {
        super::ReplicationPropertiesZoneRedundancy::Disabled
    }
    pub(super) fn retention_policy_status() -> super::RetentionPolicyStatus {
        super::RetentionPolicyStatus::Disabled
    }
    pub(super) fn trust_policy_status() -> super::TrustPolicyStatus {
        super::TrustPolicyStatus::Disabled
    }
    pub(super) fn trust_policy_type() -> super::TrustPolicyType {
        super::TrustPolicyType::Notary
    }
}
