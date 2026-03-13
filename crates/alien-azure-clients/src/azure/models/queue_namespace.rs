/// Error types.
pub mod error {
    /// Error from a `TryFrom` or `FromStr` implementation.
    pub struct ConversionError(::std::borrow::Cow<'static, str>);
    impl ::std::error::Error for ConversionError {}
    impl ::std::fmt::Display for ConversionError {
        fn fmt(
            &self,
            f: &mut ::std::fmt::Formatter<'_>,
        ) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Display::fmt(&self.0, f)
        }
    }
    impl ::std::fmt::Debug for ConversionError {
        fn fmt(
            &self,
            f: &mut ::std::fmt::Formatter<'_>,
        ) -> Result<(), ::std::fmt::Error> {
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
///ConnectionState information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "ConnectionState information.",
///  "type": "object",
///  "properties": {
///    "description": {
///      "description": "Description of the connection state.",
///      "type": "string"
///    },
///    "status": {
///      "description": "Status of the connection.",
///      "type": "string",
///      "enum": [
///        "Pending",
///        "Approved",
///        "Rejected",
///        "Disconnected"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PrivateLinkConnectionStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ConnectionState {
    ///Description of the connection state.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Status of the connection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<ConnectionStateStatus>,
}
impl ::std::convert::From<&ConnectionState> for ConnectionState {
    fn from(value: &ConnectionState) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ConnectionState {
    fn default() -> Self {
        Self {
            description: Default::default(),
            status: Default::default(),
        }
    }
}
///Status of the connection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Status of the connection.",
///  "type": "string",
///  "enum": [
///    "Pending",
///    "Approved",
///    "Rejected",
///    "Disconnected"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateLinkConnectionStatus"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum ConnectionStateStatus {
    Pending,
    Approved,
    Rejected,
    Disconnected,
}
impl ::std::convert::From<&Self> for ConnectionStateStatus {
    fn from(value: &ConnectionStateStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ConnectionStateStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Pending => f.write_str("Pending"),
            Self::Approved => f.write_str("Approved"),
            Self::Rejected => f.write_str("Rejected"),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}
impl ::std::str::FromStr for ConnectionStateStatus {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "disconnected" => Ok(Self::Disconnected),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ConnectionStateStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ConnectionStateStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ConnectionStateStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Properties to configure Encryption
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties to configure Encryption",
///  "type": "object",
///  "properties": {
///    "keySource": {
///      "description": "Enumerates the possible value of keySource for Encryption",
///      "default": "Microsoft.KeyVault",
///      "type": "string",
///      "enum": [
///        "Microsoft.KeyVault"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "keySource"
///      }
///    },
///    "keyVaultProperties": {
///      "description": "Properties of KeyVault",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/KeyVaultProperties"
///      },
///      "x-ms-client-name": "KeyVaultProperties"
///    },
///    "requireInfrastructureEncryption": {
///      "description": "Enable Infrastructure Encryption (Double Encryption)",
///      "type": "boolean"
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Encryption {
    ///Enumerates the possible value of keySource for Encryption
    #[serde(
        rename = "keySource",
        default = "defaults::encryption_key_source",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_source: EncryptionKeySource,
    ///Properties of KeyVault
    #[serde(
        rename = "keyVaultProperties",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_vault_properties: ::std::vec::Vec<KeyVaultProperties>,
    ///Enable Infrastructure Encryption (Double Encryption)
    #[serde(
        rename = "requireInfrastructureEncryption",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub require_infrastructure_encryption: ::std::option::Option<bool>,
}
impl ::std::convert::From<&Encryption> for Encryption {
    fn from(value: &Encryption) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Encryption {
    fn default() -> Self {
        Self {
            key_source: defaults::encryption_key_source(),
            key_vault_properties: Default::default(),
            require_infrastructure_encryption: Default::default(),
        }
    }
}
///Enumerates the possible value of keySource for Encryption
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Enumerates the possible value of keySource for Encryption",
///  "default": "Microsoft.KeyVault",
///  "type": "string",
///  "enum": [
///    "Microsoft.KeyVault"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "keySource"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum EncryptionKeySource {
    #[serde(rename = "Microsoft.KeyVault")]
    MicrosoftKeyVault,
}
impl ::std::convert::From<&Self> for EncryptionKeySource {
    fn from(value: &EncryptionKeySource) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionKeySource {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftKeyVault => f.write_str("Microsoft.KeyVault"),
        }
    }
}
impl ::std::str::FromStr for EncryptionKeySource {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoft.keyvault" => Ok(Self::MicrosoftKeyVault),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionKeySource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionKeySource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionKeySource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for EncryptionKeySource {
    fn default() -> Self {
        EncryptionKeySource::MicrosoftKeyVault
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
///The resource management error response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The resource management error response.",
///  "type": "object",
///  "properties": {
///    "error": {
///      "description": "The error object.",
///      "type": "object",
///      "properties": {
///        "additionalInfo": {
///          "description": "The error additional info.",
///          "readOnly": true,
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/ErrorAdditionalInfo"
///          }
///        },
///        "code": {
///          "description": "The error code.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "details": {
///          "description": "The error details.",
///          "readOnly": true,
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/ErrorResponse"
///          }
///        },
///        "message": {
///          "description": "The error message.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "target": {
///          "description": "The error target.",
///          "readOnly": true,
///          "type": "string"
///        }
///      }
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
    pub error: ::std::option::Option<ErrorResponseError>,
}
impl ::std::convert::From<&ErrorResponse> for ErrorResponse {
    fn from(value: &ErrorResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorResponse {
    fn default() -> Self {
        Self { error: Default::default() }
    }
}
///The error object.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The error object.",
///  "type": "object",
///  "properties": {
///    "additionalInfo": {
///      "description": "The error additional info.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ErrorAdditionalInfo"
///      }
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
///        "$ref": "#/components/schemas/ErrorResponse"
///      }
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
pub struct ErrorResponseError {
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
    pub details: ::std::vec::Vec<ErrorResponse>,
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
impl ::std::convert::From<&ErrorResponseError> for ErrorResponseError {
    fn from(value: &ErrorResponseError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorResponseError {
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
///Properties to configure User Assigned Identities for Bring your Own Keys
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties to configure User Assigned Identities for Bring your Own Keys",
///  "type": "object",
///  "properties": {
///    "principalId": {
///      "description": "ObjectId from the KeyVault",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "TenantId from the KeyVault",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "Type of managed service identity.",
///      "type": "string",
///      "enum": [
///        "SystemAssigned",
///        "UserAssigned",
///        "SystemAssigned, UserAssigned",
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "ManagedServiceIdentityType"
///      }
///    },
///    "userAssignedIdentities": {
///      "description": "Properties for User Assigned Identities",
///      "type": "object",
///      "additionalProperties": {
///        "$ref": "#/components/schemas/UserAssignedIdentity"
///      }
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Identity {
    ///ObjectId from the KeyVault
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
    ///TenantId from the KeyVault
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    ///Type of managed service identity.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<IdentityType>,
    ///Properties for User Assigned Identities
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities: ::std::collections::HashMap<
        ::std::string::String,
        UserAssignedIdentity,
    >,
}
impl ::std::convert::From<&Identity> for Identity {
    fn from(value: &Identity) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Identity {
    fn default() -> Self {
        Self {
            principal_id: Default::default(),
            tenant_id: Default::default(),
            type_: Default::default(),
            user_assigned_identities: Default::default(),
        }
    }
}
///Type of managed service identity.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Type of managed service identity.",
///  "type": "string",
///  "enum": [
///    "SystemAssigned",
///    "UserAssigned",
///    "SystemAssigned, UserAssigned",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "ManagedServiceIdentityType"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum IdentityType {
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned, UserAssigned")]
    SystemAssignedUserAssigned,
    None,
}
impl ::std::convert::From<&Self> for IdentityType {
    fn from(value: &IdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => {
                f.write_str("SystemAssigned, UserAssigned")
            }
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for IdentityType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "systemassigned" => Ok(Self::SystemAssigned),
            "userassigned" => Ok(Self::UserAssigned),
            "systemassigned, userassigned" => Ok(Self::SystemAssignedUserAssigned),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Properties to configure keyVault Properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties to configure keyVault Properties",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/userAssignedIdentityProperties"
///    },
///    "keyName": {
///      "description": "Name of the Key from KeyVault",
///      "type": "string"
///    },
///    "keyVaultUri": {
///      "description": "Uri of KeyVault",
///      "type": "string"
///    },
///    "keyVersion": {
///      "description": "Version of KeyVault",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyVaultProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<UserAssignedIdentityProperties>,
    ///Name of the Key from KeyVault
    #[serde(
        rename = "keyName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_name: ::std::option::Option<::std::string::String>,
    ///Uri of KeyVault
    #[serde(
        rename = "keyVaultUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_vault_uri: ::std::option::Option<::std::string::String>,
    ///Version of KeyVault
    #[serde(
        rename = "keyVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_version: ::std::option::Option<::std::string::String>,
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
            key_name: Default::default(),
            key_vault_uri: Default::default(),
            key_version: Default::default(),
        }
    }
}
///PrivateEndpoint information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "PrivateEndpoint information.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The ARM identifier for Private Endpoint.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpoint {
    ///The ARM identifier for Private Endpoint.
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
        Self { id: Default::default() }
    }
}
///Properties of the PrivateEndpointConnection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the PrivateEndpointConnection.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnection {
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The geo-location where the resource lives
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
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
    pub properties: ::std::option::Option<PrivateEndpointConnectionProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource. E.g. "Microsoft.EventHub/Namespaces" or "Microsoft.EventHub/Namespaces/EventHubs"
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
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///Result of the list of all private endpoint connections operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Result of the list of all private endpoint connections operation.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "A link for the next page of private endpoint connection resources.",
///      "type": "string"
///    },
///    "value": {
///      "description": "A collection of private endpoint connection resources.",
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
    ///A link for the next page of private endpoint connection resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A collection of private endpoint connection resources.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PrivateEndpointConnection>,
}
impl ::std::convert::From<&PrivateEndpointConnectionListResult>
for PrivateEndpointConnectionListResult {
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
///      "$ref": "#/components/schemas/ConnectionState"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the Private Endpoint Connection.",
///      "type": "string",
///      "enum": [
///        "Creating",
///        "Updating",
///        "Deleting",
///        "Succeeded",
///        "Canceled",
///        "Failed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "EndPointProvisioningState"
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
    pub private_link_service_connection_state: ::std::option::Option<ConnectionState>,
    ///Provisioning state of the Private Endpoint Connection.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        PrivateEndpointConnectionPropertiesProvisioningState,
    >,
}
impl ::std::convert::From<&PrivateEndpointConnectionProperties>
for PrivateEndpointConnectionProperties {
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
///Provisioning state of the Private Endpoint Connection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the Private Endpoint Connection.",
///  "type": "string",
///  "enum": [
///    "Creating",
///    "Updating",
///    "Deleting",
///    "Succeeded",
///    "Canceled",
///    "Failed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "EndPointProvisioningState"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum PrivateEndpointConnectionPropertiesProvisioningState {
    Creating,
    Updating,
    Deleting,
    Succeeded,
    Canceled,
    Failed,
}
impl ::std::convert::From<&Self>
for PrivateEndpointConnectionPropertiesProvisioningState {
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
            Self::Canceled => f.write_str("Canceled"),
            Self::Failed => f.write_str("Failed"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointConnectionPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "succeeded" => Ok(Self::Succeeded),
            "canceled" => Ok(Self::Canceled),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
for PrivateEndpointConnectionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for PrivateEndpointConnectionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for PrivateEndpointConnectionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Information of the private link resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Information of the private link resource.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Fully qualified identifier of the resource.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the resource",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkResourceProperties"
///    },
///    "type": {
///      "description": "Type of the resource",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResource {
    ///Fully qualified identifier of the resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Name of the resource
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
    ///Type of the resource
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
///Properties of PrivateLinkResource
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of PrivateLinkResource",
///  "type": "object",
///  "properties": {
///    "groupId": {
///      "type": "string"
///    },
///    "requiredMembers": {
///      "description": "Required Members",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "requiredZoneNames": {
///      "description": "Required Zone Names",
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
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<::std::string::String>,
    ///Required Members
    #[serde(
        rename = "requiredMembers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_members: ::std::vec::Vec<::std::string::String>,
    ///Required Zone Names
    #[serde(
        rename = "requiredZoneNames",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_zone_names: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkResourceProperties>
for PrivateLinkResourceProperties {
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
///Result of the List private link resources operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Result of the List private link resources operation.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "A link for the next page of private link resources.",
///      "type": "string"
///    },
///    "value": {
///      "description": "A collection of private link resources",
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
pub struct PrivateLinkResourcesListResult {
    ///A link for the next page of private link resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A collection of private link resources
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PrivateLinkResource>,
}
impl ::std::convert::From<&PrivateLinkResourcesListResult>
for PrivateLinkResourcesListResult {
    fn from(value: &PrivateLinkResourcesListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResourcesListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
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
///    "location": {
///      "description": "The geo-location where the resource lives",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of the resource. E.g. \"Microsoft.EventHub/Namespaces\" or \"Microsoft.EventHub/Namespaces/EventHubs\"",
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
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The geo-location where the resource lives
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///The name of the resource
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The type of the resource. E.g. "Microsoft.EventHub/Namespaces" or "Microsoft.EventHub/Namespaces/EventHubs"
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
            location: Default::default(),
            name: Default::default(),
            type_: Default::default(),
        }
    }
}
///The Resource definition for other than namespace.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Resource definition for other than namespace.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Resource Id",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "Resource name",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "Resource type",
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
    ///Resource Id
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Resource name
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Resource type
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
///The Resource definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Resource definition.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "location": {
///      "description": "Resource location",
///      "type": "string"
///    },
///    "tags": {
///      "description": "Resource tags",
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
pub struct ResourceNamespacePatch {
    ///Resource Id
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Resource location
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Resource name
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceNamespacePatch> for ResourceNamespacePatch {
    fn from(value: &ResourceNamespacePatch) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceNamespacePatch {
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
///Description of a namespace resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Description of a namespace resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/Identity"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/SBNamespaceProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/SBSku"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbNamespace {
    ///Resource Id
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
    pub identity: ::std::option::Option<Identity>,
    ///The Geo-location where the resource lives
    pub location: ::std::string::String,
    ///Resource name
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
    pub properties: ::std::option::Option<SbNamespaceProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<SbSku>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SbNamespace> for SbNamespace {
    fn from(value: &SbNamespace) -> Self {
        value.clone()
    }
}
///The response of the List Namespace operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response of the List Namespace operation.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "Link to the next set of results. Not empty if Value contains incomplete list of Namespaces.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Result of the List Namespace operation.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SBNamespace"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbNamespaceListResult {
    ///Link to the next set of results. Not empty if Value contains incomplete list of Namespaces.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Result of the List Namespace operation.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<SbNamespace>,
}
impl ::std::convert::From<&SbNamespaceListResult> for SbNamespaceListResult {
    fn from(value: &SbNamespaceListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbNamespaceListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Properties of the namespace.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the namespace.",
///  "type": "object",
///  "properties": {
///    "alternateName": {
///      "description": "Alternate name for namespace",
///      "type": "string"
///    },
///    "createdAt": {
///      "description": "The time the namespace was created",
///      "readOnly": true,
///      "type": "string"
///    },
///    "disableLocalAuth": {
///      "description": "This property disables SAS authentication for the Service Bus namespace.",
///      "type": "boolean"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "metricId": {
///      "description": "Identifier for Azure Insights metrics",
///      "readOnly": true,
///      "type": "string"
///    },
///    "minimumTlsVersion": {
///      "description": "The minimum TLS version for the cluster to support, e.g. '1.2'",
///      "type": "string",
///      "enum": [
///        "1.0",
///        "1.1",
///        "1.2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TlsVersion"
///      }
///    },
///    "premiumMessagingPartitions": {
///      "description": "The number of partitions of a Service Bus namespace. This property is only applicable to Premium SKU namespaces. The default value is 1 and possible values are 1, 2 and 4",
///      "type": "integer",
///      "format": "int32"
///    },
///    "privateEndpointConnections": {
///      "description": "List of private endpoint connections.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the namespace.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "publicNetworkAccess": {
///      "description": "This determines if traffic is allowed over public network. By default it is enabled.",
///      "default": "Enabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled",
///        "SecuredByPerimeter"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PublicNetworkAccess"
///      }
///    },
///    "serviceBusEndpoint": {
///      "description": "Endpoint you can use to perform Service Bus operations.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "status": {
///      "description": "Status of the namespace.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "updatedAt": {
///      "description": "The time the namespace was updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "zoneRedundant": {
///      "description": "This property reflects if zone redundancy has been enabled for namespaces in regions that support availability zones.",
///      "type": "boolean",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbNamespaceProperties {
    ///Alternate name for namespace
    #[serde(
        rename = "alternateName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub alternate_name: ::std::option::Option<::std::string::String>,
    ///The time the namespace was created
    #[serde(
        rename = "createdAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_at: ::std::option::Option<::std::string::String>,
    ///This property disables SAS authentication for the Service Bus namespace.
    #[serde(
        rename = "disableLocalAuth",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disable_local_auth: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    ///Identifier for Azure Insights metrics
    #[serde(
        rename = "metricId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metric_id: ::std::option::Option<::std::string::String>,
    ///The minimum TLS version for the cluster to support, e.g. '1.2'
    #[serde(
        rename = "minimumTlsVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub minimum_tls_version: ::std::option::Option<
        SbNamespacePropertiesMinimumTlsVersion,
    >,
    ///The number of partitions of a Service Bus namespace. This property is only applicable to Premium SKU namespaces. The default value is 1 and possible values are 1, 2 and 4
    #[serde(
        rename = "premiumMessagingPartitions",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub premium_messaging_partitions: ::std::option::Option<i32>,
    ///List of private endpoint connections.
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnection>,
    ///Provisioning state of the namespace.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    ///This determines if traffic is allowed over public network. By default it is enabled.
    #[serde(
        rename = "publicNetworkAccess",
        default = "defaults::sb_namespace_properties_public_network_access",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: SbNamespacePropertiesPublicNetworkAccess,
    ///Endpoint you can use to perform Service Bus operations.
    #[serde(
        rename = "serviceBusEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_bus_endpoint: ::std::option::Option<::std::string::String>,
    ///Status of the namespace.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<::std::string::String>,
    ///The time the namespace was updated.
    #[serde(
        rename = "updatedAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated_at: ::std::option::Option<::std::string::String>,
    ///This property reflects if zone redundancy has been enabled for namespaces in regions that support availability zones.
    #[serde(
        rename = "zoneRedundant",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zone_redundant: ::std::option::Option<bool>,
}
impl ::std::convert::From<&SbNamespaceProperties> for SbNamespaceProperties {
    fn from(value: &SbNamespaceProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbNamespaceProperties {
    fn default() -> Self {
        Self {
            alternate_name: Default::default(),
            created_at: Default::default(),
            disable_local_auth: Default::default(),
            encryption: Default::default(),
            metric_id: Default::default(),
            minimum_tls_version: Default::default(),
            premium_messaging_partitions: Default::default(),
            private_endpoint_connections: Default::default(),
            provisioning_state: Default::default(),
            public_network_access: defaults::sb_namespace_properties_public_network_access(),
            service_bus_endpoint: Default::default(),
            status: Default::default(),
            updated_at: Default::default(),
            zone_redundant: Default::default(),
        }
    }
}
///The minimum TLS version for the cluster to support, e.g. '1.2'
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The minimum TLS version for the cluster to support, e.g. '1.2'",
///  "type": "string",
///  "enum": [
///    "1.0",
///    "1.1",
///    "1.2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TlsVersion"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum SbNamespacePropertiesMinimumTlsVersion {
    #[serde(rename = "1.0")]
    X10,
    #[serde(rename = "1.1")]
    X11,
    #[serde(rename = "1.2")]
    X12,
}
impl ::std::convert::From<&Self> for SbNamespacePropertiesMinimumTlsVersion {
    fn from(value: &SbNamespacePropertiesMinimumTlsVersion) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SbNamespacePropertiesMinimumTlsVersion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::X10 => f.write_str("1.0"),
            Self::X11 => f.write_str("1.1"),
            Self::X12 => f.write_str("1.2"),
        }
    }
}
impl ::std::str::FromStr for SbNamespacePropertiesMinimumTlsVersion {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "1.0" => Ok(Self::X10),
            "1.1" => Ok(Self::X11),
            "1.2" => Ok(Self::X12),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SbNamespacePropertiesMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for SbNamespacePropertiesMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for SbNamespacePropertiesMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///This determines if traffic is allowed over public network. By default it is enabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This determines if traffic is allowed over public network. By default it is enabled.",
///  "default": "Enabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled",
///    "SecuredByPerimeter"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum SbNamespacePropertiesPublicNetworkAccess {
    Enabled,
    Disabled,
    SecuredByPerimeter,
}
impl ::std::convert::From<&Self> for SbNamespacePropertiesPublicNetworkAccess {
    fn from(value: &SbNamespacePropertiesPublicNetworkAccess) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SbNamespacePropertiesPublicNetworkAccess {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
            Self::SecuredByPerimeter => f.write_str("SecuredByPerimeter"),
        }
    }
}
impl ::std::str::FromStr for SbNamespacePropertiesPublicNetworkAccess {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            "securedbyperimeter" => Ok(Self::SecuredByPerimeter),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SbNamespacePropertiesPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for SbNamespacePropertiesPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for SbNamespacePropertiesPublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for SbNamespacePropertiesPublicNetworkAccess {
    fn default() -> Self {
        SbNamespacePropertiesPublicNetworkAccess::Enabled
    }
}
///Description of a namespace resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Description of a namespace resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ResourceNamespacePatch"
///    }
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/Identity"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/SBNamespaceUpdateProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/SBSku"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbNamespaceUpdateParameters {
    ///Resource Id
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
    pub identity: ::std::option::Option<Identity>,
    ///Resource location
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Resource name
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
    pub properties: ::std::option::Option<SbNamespaceUpdateProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<SbSku>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SbNamespaceUpdateParameters> for SbNamespaceUpdateParameters {
    fn from(value: &SbNamespaceUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbNamespaceUpdateParameters {
    fn default() -> Self {
        Self {
            id: Default::default(),
            identity: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Properties of the namespace.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the namespace.",
///  "type": "object",
///  "properties": {
///    "alternateName": {
///      "description": "Alternate name for namespace",
///      "type": "string"
///    },
///    "createdAt": {
///      "description": "The time the namespace was created",
///      "readOnly": true,
///      "type": "string"
///    },
///    "disableLocalAuth": {
///      "description": "This property disables SAS authentication for the Service Bus namespace.",
///      "type": "boolean"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "metricId": {
///      "description": "Identifier for Azure Insights metrics",
///      "readOnly": true,
///      "type": "string"
///    },
///    "privateEndpointConnections": {
///      "description": "List of private endpoint connections.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the namespace.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "serviceBusEndpoint": {
///      "description": "Endpoint you can use to perform Service Bus operations.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "status": {
///      "description": "Status of the namespace.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "updatedAt": {
///      "description": "The time the namespace was updated.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbNamespaceUpdateProperties {
    ///Alternate name for namespace
    #[serde(
        rename = "alternateName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub alternate_name: ::std::option::Option<::std::string::String>,
    ///The time the namespace was created
    #[serde(
        rename = "createdAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_at: ::std::option::Option<::std::string::String>,
    ///This property disables SAS authentication for the Service Bus namespace.
    #[serde(
        rename = "disableLocalAuth",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disable_local_auth: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    ///Identifier for Azure Insights metrics
    #[serde(
        rename = "metricId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metric_id: ::std::option::Option<::std::string::String>,
    ///List of private endpoint connections.
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnection>,
    ///Provisioning state of the namespace.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    ///Endpoint you can use to perform Service Bus operations.
    #[serde(
        rename = "serviceBusEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_bus_endpoint: ::std::option::Option<::std::string::String>,
    ///Status of the namespace.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<::std::string::String>,
    ///The time the namespace was updated.
    #[serde(
        rename = "updatedAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated_at: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SbNamespaceUpdateProperties> for SbNamespaceUpdateProperties {
    fn from(value: &SbNamespaceUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbNamespaceUpdateProperties {
    fn default() -> Self {
        Self {
            alternate_name: Default::default(),
            created_at: Default::default(),
            disable_local_auth: Default::default(),
            encryption: Default::default(),
            metric_id: Default::default(),
            private_endpoint_connections: Default::default(),
            provisioning_state: Default::default(),
            service_bus_endpoint: Default::default(),
            status: Default::default(),
            updated_at: Default::default(),
        }
    }
}
///SKU of the namespace.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU of the namespace.",
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "capacity": {
///      "description": "Messaging units for your service bus premium namespace. Valid capacities are {1, 2, 4, 8, 16} multiples of your properties.premiumMessagingPartitions setting. For example, If properties.premiumMessagingPartitions is 1 then possible capacity values are 1, 2, 4, 8, and 16. If properties.premiumMessagingPartitions is 4 then possible capacity values are 4, 8, 16, 32 and 64",
///      "type": "integer",
///      "format": "int32"
///    },
///    "name": {
///      "description": "Name of this SKU.",
///      "type": "string",
///      "enum": [
///        "Basic",
///        "Standard",
///        "Premium"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "SkuName"
///      }
///    },
///    "tier": {
///      "description": "The billing tier of this particular SKU.",
///      "type": "string",
///      "enum": [
///        "Basic",
///        "Standard",
///        "Premium"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "SkuTier"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbSku {
    ///Messaging units for your service bus premium namespace. Valid capacities are {1, 2, 4, 8, 16} multiples of your properties.premiumMessagingPartitions setting. For example, If properties.premiumMessagingPartitions is 1 then possible capacity values are 1, 2, 4, 8, and 16. If properties.premiumMessagingPartitions is 4 then possible capacity values are 4, 8, 16, 32 and 64
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub capacity: ::std::option::Option<i32>,
    ///Name of this SKU.
    pub name: SbSkuName,
    ///The billing tier of this particular SKU.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<SbSkuTier>,
}
impl ::std::convert::From<&SbSku> for SbSku {
    fn from(value: &SbSku) -> Self {
        value.clone()
    }
}
///Name of this SKU.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Name of this SKU.",
///  "type": "string",
///  "enum": [
///    "Basic",
///    "Standard",
///    "Premium"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum SbSkuName {
    Basic,
    Standard,
    Premium,
}
impl ::std::convert::From<&Self> for SbSkuName {
    fn from(value: &SbSkuName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SbSkuName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Basic => f.write_str("Basic"),
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
        }
    }
}
impl ::std::str::FromStr for SbSkuName {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "basic" => Ok(Self::Basic),
            "standard" => Ok(Self::Standard),
            "premium" => Ok(Self::Premium),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SbSkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SbSkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SbSkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The billing tier of this particular SKU.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The billing tier of this particular SKU.",
///  "type": "string",
///  "enum": [
///    "Basic",
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum SbSkuTier {
    Basic,
    Standard,
    Premium,
}
impl ::std::convert::From<&Self> for SbSkuTier {
    fn from(value: &SbSkuTier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SbSkuTier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Basic => f.write_str("Basic"),
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
        }
    }
}
impl ::std::str::FromStr for SbSkuTier {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "basic" => Ok(Self::Basic),
            "standard" => Ok(Self::Standard),
            "premium" => Ok(Self::Premium),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SbSkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SbSkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SbSkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///      "description": "The type of identity that last modified the resource.",
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
///        "name": "createdByType"
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
    ///The type of identity that last modified the resource.
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
    PartialOrd
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
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    PartialOrd
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
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///The Resource definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Resource definition.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "required": [
///    "location"
///  ],
///  "properties": {
///    "location": {
///      "description": "The Geo-location where the resource lives",
///      "type": "string",
///      "x-ms-mutability": [
///        "read",
///        "create"
///      ]
///    },
///    "tags": {
///      "description": "Resource tags",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      },
///      "x-ms-mutability": [
///        "read",
///        "create",
///        "update"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TrackedResource {
    ///Resource Id
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The Geo-location where the resource lives
    pub location: ::std::string::String,
    ///Resource name
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TrackedResource> for TrackedResource {
    fn from(value: &TrackedResource) -> Self {
        value.clone()
    }
}
///Recognized Dictionary value.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Recognized Dictionary value.",
///  "type": "object",
///  "properties": {
///    "clientId": {
///      "description": "Client Id of user assigned identity",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-client-name": "ClientId"
///    },
///    "principalId": {
///      "description": "Principal Id of user assigned identity",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-client-name": "PrincipalId"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserAssignedIdentity {
    ///Client Id of user assigned identity
    #[serde(
        rename = "clientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_id: ::std::option::Option<::std::string::String>,
    ///Principal Id of user assigned identity
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&UserAssignedIdentity> for UserAssignedIdentity {
    fn from(value: &UserAssignedIdentity) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UserAssignedIdentity {
    fn default() -> Self {
        Self {
            client_id: Default::default(),
            principal_id: Default::default(),
        }
    }
}
///`UserAssignedIdentityProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "userAssignedIdentity": {
///      "description": "ARM ID of user Identity selected for encryption",
///      "type": "string"
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserAssignedIdentityProperties {
    ///ARM ID of user Identity selected for encryption
    #[serde(
        rename = "userAssignedIdentity",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identity: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&UserAssignedIdentityProperties>
for UserAssignedIdentityProperties {
    fn from(value: &UserAssignedIdentityProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UserAssignedIdentityProperties {
    fn default() -> Self {
        Self {
            user_assigned_identity: Default::default(),
        }
    }
}
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn encryption_key_source() -> super::EncryptionKeySource {
        super::EncryptionKeySource::MicrosoftKeyVault
    }
    pub(super) fn sb_namespace_properties_public_network_access() -> super::SbNamespacePropertiesPublicNetworkAccess {
        super::SbNamespacePropertiesPublicNetworkAccess::Enabled
    }
}
