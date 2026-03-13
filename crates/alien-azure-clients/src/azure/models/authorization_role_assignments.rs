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
        Self { error: Default::default() }
    }
}
///Role Assignments
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role Assignments",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The role assignment ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The role assignment name.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RoleAssignmentProperties"
///    },
///    "type": {
///      "description": "The role assignment type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleAssignment {
    ///The role assignment ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The role assignment name.
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
    pub properties: ::std::option::Option<RoleAssignmentProperties>,
    ///The role assignment type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RoleAssignment> for RoleAssignment {
    fn from(value: &RoleAssignment) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleAssignment {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Role assignment create parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role assignment create parameters.",
///  "type": "object",
///  "required": [
///    "properties"
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/RoleAssignmentProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleAssignmentCreateParameters {
    pub properties: RoleAssignmentProperties,
}
impl ::std::convert::From<&RoleAssignmentCreateParameters>
for RoleAssignmentCreateParameters {
    fn from(value: &RoleAssignmentCreateParameters) -> Self {
        value.clone()
    }
}
///Role Assignments filter
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role Assignments filter",
///  "type": "object",
///  "properties": {
///    "principalId": {
///      "description": "Returns role assignment of the specific principal.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleAssignmentFilter {
    ///Returns role assignment of the specific principal.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RoleAssignmentFilter> for RoleAssignmentFilter {
    fn from(value: &RoleAssignmentFilter) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleAssignmentFilter {
    fn default() -> Self {
        Self {
            principal_id: Default::default(),
        }
    }
}
///Role assignment list operation result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role assignment list operation result.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The skipToken to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "Role assignment list.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RoleAssignment"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleAssignmentListResult {
    ///The skipToken to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Role assignment list.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<RoleAssignment>,
}
impl ::std::convert::From<&RoleAssignmentListResult> for RoleAssignmentListResult {
    fn from(value: &RoleAssignmentListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleAssignmentListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Role assignment properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role assignment properties.",
///  "type": "object",
///  "required": [
///    "principalId",
///    "roleDefinitionId"
///  ],
///  "properties": {
///    "condition": {
///      "description": "The conditions on the role assignment. This limits the resources it can be assigned to. e.g.: @Resource[Microsoft.Storage/storageAccounts/blobServices/containers:ContainerName] StringEqualsIgnoreCase 'foo_storage_container'",
///      "type": "string"
///    },
///    "conditionVersion": {
///      "description": "Version of the condition. Currently the only accepted value is '2.0'",
///      "type": "string"
///    },
///    "createdBy": {
///      "description": "Id of the user who created the assignment",
///      "readOnly": true,
///      "type": "string"
///    },
///    "createdOn": {
///      "description": "Time it was created",
///      "readOnly": true,
///      "type": "string"
///    },
///    "delegatedManagedIdentityResourceId": {
///      "description": "Id of the delegated managed identity resource",
///      "type": "string"
///    },
///    "description": {
///      "description": "Description of role assignment",
///      "type": "string"
///    },
///    "principalId": {
///      "description": "The principal ID.",
///      "type": "string"
///    },
///    "principalType": {
///      "description": "The principal type of the assigned principal ID.",
///      "default": "User",
///      "type": "string",
///      "enum": [
///        "User",
///        "Group",
///        "ServicePrincipal",
///        "ForeignGroup",
///        "Device"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PrincipalType"
///      }
///    },
///    "roleDefinitionId": {
///      "description": "The role definition ID.",
///      "type": "string"
///    },
///    "scope": {
///      "description": "The role assignment scope.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "updatedBy": {
///      "description": "Id of the user who updated the assignment",
///      "readOnly": true,
///      "type": "string"
///    },
///    "updatedOn": {
///      "description": "Time it was updated",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleAssignmentProperties {
    ///The conditions on the role assignment. This limits the resources it can be assigned to. e.g.: @Resource[Microsoft.Storage/storageAccounts/blobServices/containers:ContainerName] StringEqualsIgnoreCase 'foo_storage_container'
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub condition: ::std::option::Option<::std::string::String>,
    ///Version of the condition. Currently the only accepted value is '2.0'
    #[serde(
        rename = "conditionVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub condition_version: ::std::option::Option<::std::string::String>,
    ///Id of the user who created the assignment
    #[serde(
        rename = "createdBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_by: ::std::option::Option<::std::string::String>,
    ///Time it was created
    #[serde(
        rename = "createdOn",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_on: ::std::option::Option<::std::string::String>,
    ///Id of the delegated managed identity resource
    #[serde(
        rename = "delegatedManagedIdentityResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delegated_managed_identity_resource_id: ::std::option::Option<
        ::std::string::String,
    >,
    ///Description of role assignment
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The principal ID.
    #[serde(rename = "principalId")]
    pub principal_id: ::std::string::String,
    ///The principal type of the assigned principal ID.
    #[serde(
        rename = "principalType",
        default = "defaults::role_assignment_properties_principal_type",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_type: RoleAssignmentPropertiesPrincipalType,
    ///The role definition ID.
    #[serde(rename = "roleDefinitionId")]
    pub role_definition_id: ::std::string::String,
    ///The role assignment scope.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scope: ::std::option::Option<::std::string::String>,
    ///Id of the user who updated the assignment
    #[serde(
        rename = "updatedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated_by: ::std::option::Option<::std::string::String>,
    ///Time it was updated
    #[serde(
        rename = "updatedOn",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated_on: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RoleAssignmentProperties> for RoleAssignmentProperties {
    fn from(value: &RoleAssignmentProperties) -> Self {
        value.clone()
    }
}
///The principal type of the assigned principal ID.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The principal type of the assigned principal ID.",
///  "default": "User",
///  "type": "string",
///  "enum": [
///    "User",
///    "Group",
///    "ServicePrincipal",
///    "ForeignGroup",
///    "Device"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrincipalType"
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
pub enum RoleAssignmentPropertiesPrincipalType {
    User,
    Group,
    ServicePrincipal,
    ForeignGroup,
    Device,
}
impl ::std::convert::From<&Self> for RoleAssignmentPropertiesPrincipalType {
    fn from(value: &RoleAssignmentPropertiesPrincipalType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RoleAssignmentPropertiesPrincipalType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::User => f.write_str("User"),
            Self::Group => f.write_str("Group"),
            Self::ServicePrincipal => f.write_str("ServicePrincipal"),
            Self::ForeignGroup => f.write_str("ForeignGroup"),
            Self::Device => f.write_str("Device"),
        }
    }
}
impl ::std::str::FromStr for RoleAssignmentPropertiesPrincipalType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "user" => Ok(Self::User),
            "group" => Ok(Self::Group),
            "serviceprincipal" => Ok(Self::ServicePrincipal),
            "foreigngroup" => Ok(Self::ForeignGroup),
            "device" => Ok(Self::Device),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RoleAssignmentPropertiesPrincipalType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for RoleAssignmentPropertiesPrincipalType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for RoleAssignmentPropertiesPrincipalType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for RoleAssignmentPropertiesPrincipalType {
    fn default() -> Self {
        RoleAssignmentPropertiesPrincipalType::User
    }
}
///Validation response
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Validation response",
///  "type": "object",
///  "properties": {
///    "errorInfo": {
///      "$ref": "#/components/schemas/ValidationResponseErrorInfo"
///    },
///    "isValid": {
///      "description": "Whether or not validation succeeded",
///      "readOnly": true,
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ValidationResponse {
    #[serde(
        rename = "errorInfo",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error_info: ::std::option::Option<ValidationResponseErrorInfo>,
    ///Whether or not validation succeeded
    #[serde(
        rename = "isValid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_valid: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ValidationResponse> for ValidationResponse {
    fn from(value: &ValidationResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ValidationResponse {
    fn default() -> Self {
        Self {
            error_info: Default::default(),
            is_valid: Default::default(),
        }
    }
}
///Failed validation result details
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Failed validation result details",
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "Error code indicating why validation failed",
///      "readOnly": true,
///      "type": "string"
///    },
///    "message": {
///      "description": "Message indicating why validation failed",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ValidationResponseErrorInfo {
    ///Error code indicating why validation failed
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///Message indicating why validation failed
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ValidationResponseErrorInfo> for ValidationResponseErrorInfo {
    fn from(value: &ValidationResponseErrorInfo) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ValidationResponseErrorInfo {
    fn default() -> Self {
        Self {
            code: Default::default(),
            message: Default::default(),
        }
    }
}
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn role_assignment_properties_principal_type() -> super::RoleAssignmentPropertiesPrincipalType {
        super::RoleAssignmentPropertiesPrincipalType::User
    }
}
