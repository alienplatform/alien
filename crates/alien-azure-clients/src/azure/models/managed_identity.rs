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
///Universally Unique Identifier
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Universally Unique Identifier",
///  "type": "string",
///  "format": "uuid"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct AzureCoreUuid(pub ::uuid::Uuid);
impl ::std::ops::Deref for AzureCoreUuid {
    type Target = ::uuid::Uuid;
    fn deref(&self) -> &::uuid::Uuid {
        &self.0
    }
}
impl ::std::convert::From<AzureCoreUuid> for ::uuid::Uuid {
    fn from(value: AzureCoreUuid) -> Self {
        value.0
    }
}
impl ::std::convert::From<&AzureCoreUuid> for AzureCoreUuid {
    fn from(value: &AzureCoreUuid) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::uuid::Uuid> for AzureCoreUuid {
    fn from(value: ::uuid::Uuid) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for AzureCoreUuid {
    type Err = <::uuid::Uuid as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for AzureCoreUuid {
    type Error = <::uuid::Uuid as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&String> for AzureCoreUuid {
    type Error = <::uuid::Uuid as ::std::str::FromStr>::Err;
    fn try_from(value: &String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for AzureCoreUuid {
    type Error = <::uuid::Uuid as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for AzureCoreUuid {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
///An error response from the ManagedServiceIdentity service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the ManagedServiceIdentity service.",
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
        Self { error: Default::default() }
    }
}
///An error response from the ManagedServiceIdentity service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the ManagedServiceIdentity service.",
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "An identifier for the error.",
///      "type": "string"
///    },
///    "details": {
///      "description": "A list of additional details about the error.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CloudErrorBody"
///      },
///      "x-ms-identifiers": [
///        "code"
///      ]
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
    ///An identifier for the error.
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
///Describes a federated identity credential.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes a federated identity credential.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/FederatedIdentityCredentialProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct FederatedIdentityCredential {
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
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
    pub properties: ::std::option::Option<FederatedIdentityCredentialProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&FederatedIdentityCredential> for FederatedIdentityCredential {
    fn from(value: &FederatedIdentityCredential) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for FederatedIdentityCredential {
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
///The properties associated with a federated identity credential.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Federated identity credential properties.",
///  "description": "The properties associated with a federated identity credential.",
///  "type": "object",
///  "required": [
///    "audiences",
///    "issuer",
///    "subject"
///  ],
///  "properties": {
///    "audiences": {
///      "description": "The list of audiences that can appear in the issued token.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "issuer": {
///      "description": "The URL of the issuer to be trusted.",
///      "type": "string",
///      "format": "uri"
///    },
///    "subject": {
///      "description": "The identifier of the external identity.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct FederatedIdentityCredentialProperties {
    ///The list of audiences that can appear in the issued token.
    pub audiences: ::std::vec::Vec<::std::string::String>,
    ///The URL of the issuer to be trusted.
    pub issuer: ::std::string::String,
    ///The identifier of the external identity.
    pub subject: ::std::string::String,
}
impl ::std::convert::From<&FederatedIdentityCredentialProperties>
for FederatedIdentityCredentialProperties {
    fn from(value: &FederatedIdentityCredentialProperties) -> Self {
        value.clone()
    }
}
///Values returned by the List operation for federated identity credentials.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Values returned by the List operation for federated identity credentials.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "nextLink": {
///      "description": "The link to the next page of items",
///      "type": "string",
///      "format": "uri"
///    },
///    "value": {
///      "description": "The FederatedIdentityCredential items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/FederatedIdentityCredential"
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
pub struct FederatedIdentityCredentialsListResult {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The FederatedIdentityCredential items on this page
    pub value: ::std::vec::Vec<FederatedIdentityCredential>,
}
impl ::std::convert::From<&FederatedIdentityCredentialsListResult>
for FederatedIdentityCredentialsListResult {
    fn from(value: &FederatedIdentityCredentialsListResult) -> Self {
        value.clone()
    }
}
///Describes an identity resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes an identity resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/UserAssignedIdentityProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Identity {
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The geo-location where the resource lives
    pub location: ::std::string::String,
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
    pub properties: ::std::option::Option<UserAssignedIdentityProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///Resource tags.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Identity> for Identity {
    fn from(value: &Identity) -> Self {
        value.clone()
    }
}
///Describes an identity resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes an identity resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "location": {
///      "description": "The geo-location where the resource lives",
///      "type": "string",
///      "x-ms-mutability": [
///        "read",
///        "create"
///      ]
///    },
///    "properties": {
///      "$ref": "#/components/schemas/UserAssignedIdentityProperties"
///    },
///    "tags": {
///      "description": "Resource tags",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      },
///      "x-ms-mutability": [
///        "read",
///        "update",
///        "create"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IdentityUpdate {
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
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
    pub properties: ::std::option::Option<UserAssignedIdentityProperties>,
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
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IdentityUpdate> for IdentityUpdate {
    fn from(value: &IdentityUpdate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IdentityUpdate {
    fn default() -> Self {
        Self {
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Enum to configure regional restrictions on identity assignment, as necessary.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Enum to configure regional restrictions on identity assignment, as necessary.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Regional"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IsolationScope",
///    "values": [
///      {
///        "name": "None",
///        "value": "None"
///      },
///      {
///        "name": "Regional",
///        "value": "Regional"
///      }
///    ]
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
pub enum IsolationScope {
    None,
    Regional,
}
impl ::std::convert::From<&Self> for IsolationScope {
    fn from(value: &IsolationScope) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IsolationScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Regional => f.write_str("Regional"),
        }
    }
}
impl ::std::str::FromStr for IsolationScope {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "regional" => Ok(Self::Regional),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IsolationScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IsolationScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IsolationScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Details of a REST API operation, returned from the Resource Provider Operations API
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Microsoft.ManagedIdentity Operation.",
///  "description": "Details of a REST API operation, returned from the Resource Provider Operations API",
///  "type": "object",
///  "properties": {
///    "display": {
///      "$ref": "#/components/schemas/OperationDisplay"
///    },
///    "name": {
///      "title": "Operation Name.",
///      "description": "The name of the operation, as per Resource-Based Access Control (RBAC). Examples: \"Microsoft.Compute/virtualMachines/write\", \"Microsoft.Compute/virtualMachines/capture/action\"",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Operation {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display: ::std::option::Option<OperationDisplay>,
    ///The name of the operation, as per Resource-Based Access Control (RBAC). Examples: "Microsoft.Compute/virtualMachines/write", "Microsoft.Compute/virtualMachines/capture/action"
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Operation> for Operation {
    fn from(value: &Operation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Operation {
    fn default() -> Self {
        Self {
            display: Default::default(),
            name: Default::default(),
        }
    }
}
///Localized display information for and operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Operation Display.",
///  "description": "Localized display information for and operation.",
///  "type": "object",
///  "properties": {
///    "description": {
///      "title": "Operation description",
///      "description": "The short, localized friendly description of the operation; suitable for tool tips and detailed views.",
///      "type": "string"
///    },
///    "operation": {
///      "title": "Operation Type.",
///      "description": "The concise, localized friendly name for the operation; suitable for dropdowns. E.g. \"Create or Update Virtual Machine\", \"Restart Virtual Machine\".",
///      "type": "string"
///    },
///    "provider": {
///      "title": "Resource Provider Name.",
///      "description": "The localized friendly form of the resource provider name, e.g. \"Microsoft Monitoring Insights\" or \"Microsoft Compute\".",
///      "type": "string"
///    },
///    "resource": {
///      "title": "Resource Type.",
///      "description": "The localized friendly name of the resource type related to this operation. E.g. \"Virtual Machines\" or \"Job Schedule Collections\".",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationDisplay {
    ///The short, localized friendly description of the operation; suitable for tool tips and detailed views.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The concise, localized friendly name for the operation; suitable for dropdowns. E.g. "Create or Update Virtual Machine", "Restart Virtual Machine".
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<::std::string::String>,
    ///The localized friendly form of the resource provider name, e.g. "Microsoft Monitoring Insights" or "Microsoft Compute".
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
    ///The localized friendly name of the resource type related to this operation. E.g. "Virtual Machines" or "Job Schedule Collections".
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&OperationDisplay> for OperationDisplay {
    fn from(value: &OperationDisplay) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationDisplay {
    fn default() -> Self {
        Self {
            description: Default::default(),
            operation: Default::default(),
            provider: Default::default(),
            resource: Default::default(),
        }
    }
}
///A list of REST API operations supported by an Azure Resource Provider. It contains an URL link to get the next set of results.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Operations List.",
///  "description": "A list of REST API operations supported by an Azure Resource Provider. It contains an URL link to get the next set of results.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "nextLink": {
///      "title": "Next Link",
///      "description": "The link to the next page of items",
///      "type": "string",
///      "format": "uri"
///    },
///    "value": {
///      "title": "Operations List.",
///      "description": "The Operation items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Operation"
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
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The Operation items on this page
    pub value: ::std::vec::Vec<Operation>,
}
impl ::std::convert::From<&OperationListResult> for OperationListResult {
    fn from(value: &OperationListResult) -> Self {
        value.clone()
    }
}
///The resource model definition for a Azure Resource Manager proxy resource. It will not have tags and a location
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Proxy Resource",
///  "description": "The resource model definition for a Azure Resource Manager proxy resource. It will not have tags and a location",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct ProxyResource(pub Resource);
impl ::std::ops::Deref for ProxyResource {
    type Target = Resource;
    fn deref(&self) -> &Resource {
        &self.0
    }
}
impl ::std::convert::From<ProxyResource> for Resource {
    fn from(value: ProxyResource) -> Self {
        value.0
    }
}
impl ::std::convert::From<&ProxyResource> for ProxyResource {
    fn from(value: &ProxyResource) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<Resource> for ProxyResource {
    fn from(value: Resource) -> Self {
        Self(value)
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
///      "description": "Fully qualified resource ID for the resource. E.g. \"/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}\"",
///      "readOnly": true,
///      "type": "string",
///      "format": "arm-id"
///    },
///    "name": {
///      "description": "The name of the resource",
///      "readOnly": true,
///      "type": "string"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
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
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
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
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
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
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///Describes a system assigned identity resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes a system assigned identity resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "required": [
///    "location"
///  ],
///  "properties": {
///    "location": {
///      "type": "string",
///      "x-ms-mutability": [
///        "read",
///        "create"
///      ]
///    },
///    "properties": {
///      "$ref": "#/components/schemas/SystemAssignedIdentityProperties"
///    },
///    "tags": {
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      },
///      "x-ms-mutability": [
///        "read",
///        "update",
///        "create"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SystemAssignedIdentity {
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    pub location: ::std::string::String,
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
    pub properties: ::std::option::Option<SystemAssignedIdentityProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SystemAssignedIdentity> for SystemAssignedIdentity {
    fn from(value: &SystemAssignedIdentity) -> Self {
        value.clone()
    }
}
///The properties associated with the system assigned identity.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "System Assigned Identity properties.",
///  "description": "The properties associated with the system assigned identity.",
///  "type": "object",
///  "properties": {
///    "clientId": {
///      "$ref": "#/components/schemas/Azure.Core.uuid"
///    },
///    "clientSecretUrl": {
///      "description": "The ManagedServiceIdentity DataPlane URL that can be queried to obtain the identity credentials.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "principalId": {
///      "$ref": "#/components/schemas/Azure.Core.uuid"
///    },
///    "tenantId": {
///      "$ref": "#/components/schemas/Azure.Core.uuid"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SystemAssignedIdentityProperties {
    #[serde(
        rename = "clientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_id: ::std::option::Option<AzureCoreUuid>,
    ///The ManagedServiceIdentity DataPlane URL that can be queried to obtain the identity credentials.
    #[serde(
        rename = "clientSecretUrl",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_secret_url: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<AzureCoreUuid>,
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<AzureCoreUuid>,
}
impl ::std::convert::From<&SystemAssignedIdentityProperties>
for SystemAssignedIdentityProperties {
    fn from(value: &SystemAssignedIdentityProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SystemAssignedIdentityProperties {
    fn default() -> Self {
        Self {
            client_id: Default::default(),
            client_secret_url: Default::default(),
            principal_id: Default::default(),
            tenant_id: Default::default(),
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
///      "description": "The timestamp of resource last modification (UTC)",
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
    ///The timestamp of resource last modification (UTC)
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
///The resource model definition for an Azure Resource Manager tracked top level resource which has 'tags' and a 'location'
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Tracked Resource",
///  "description": "The resource model definition for an Azure Resource Manager tracked top level resource which has 'tags' and a 'location'",
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
///      "description": "The geo-location where the resource lives",
///      "type": "string",
///      "x-ms-mutability": [
///        "read",
///        "create"
///      ]
///    },
///    "tags": {
///      "description": "Resource tags.",
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
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The geo-location where the resource lives
    pub location: ::std::string::String,
    ///The name of the resource
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
    ///Resource tags.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
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
///Values returned by the List operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Values returned by the List operation.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "nextLink": {
///      "description": "The link to the next page of items",
///      "type": "string",
///      "format": "uri"
///    },
///    "value": {
///      "description": "The Identity items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Identity"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserAssignedIdentitiesListResult {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The Identity items on this page
    pub value: ::std::vec::Vec<Identity>,
}
impl ::std::convert::From<&UserAssignedIdentitiesListResult>
for UserAssignedIdentitiesListResult {
    fn from(value: &UserAssignedIdentitiesListResult) -> Self {
        value.clone()
    }
}
///The properties associated with the user assigned identity.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "User Assigned Identity properties.",
///  "description": "The properties associated with the user assigned identity.",
///  "type": "object",
///  "properties": {
///    "clientId": {
///      "$ref": "#/components/schemas/Azure.Core.uuid"
///    },
///    "isolationScope": {
///      "$ref": "#/components/schemas/IsolationScope"
///    },
///    "principalId": {
///      "$ref": "#/components/schemas/Azure.Core.uuid"
///    },
///    "tenantId": {
///      "$ref": "#/components/schemas/Azure.Core.uuid"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserAssignedIdentityProperties {
    #[serde(
        rename = "clientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_id: ::std::option::Option<AzureCoreUuid>,
    #[serde(
        rename = "isolationScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub isolation_scope: ::std::option::Option<IsolationScope>,
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<AzureCoreUuid>,
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<AzureCoreUuid>,
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
            client_id: Default::default(),
            isolation_scope: Default::default(),
            principal_id: Default::default(),
            tenant_id: Default::default(),
        }
    }
}
