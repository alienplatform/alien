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
///Role definition permissions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role definition permissions.",
///  "type": "object",
///  "properties": {
///    "actions": {
///      "description": "Allowed actions.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "dataActions": {
///      "description": "Allowed Data actions.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "notActions": {
///      "description": "Denied actions.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "notDataActions": {
///      "description": "Denied Data actions.",
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
pub struct Permission {
    ///Allowed actions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub actions: ::std::vec::Vec<::std::string::String>,
    ///Allowed Data actions.
    #[serde(
        rename = "dataActions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_actions: ::std::vec::Vec<::std::string::String>,
    ///Denied actions.
    #[serde(
        rename = "notActions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub not_actions: ::std::vec::Vec<::std::string::String>,
    ///Denied Data actions.
    #[serde(
        rename = "notDataActions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub not_data_actions: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&Permission> for Permission {
    fn from(value: &Permission) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Permission {
    fn default() -> Self {
        Self {
            actions: Default::default(),
            data_actions: Default::default(),
            not_actions: Default::default(),
            not_data_actions: Default::default(),
        }
    }
}
///Permissions information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Permissions information.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of permissions.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Permission"
///      },
///      "x-ms-identifiers": []
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PermissionGetResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of permissions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Permission>,
}
impl ::std::convert::From<&PermissionGetResult> for PermissionGetResult {
    fn from(value: &PermissionGetResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PermissionGetResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Role definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role definition.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The role definition ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The role definition name.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RoleDefinitionProperties"
///    },
///    "type": {
///      "description": "The role definition type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleDefinition {
    ///The role definition ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The role definition name.
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
    pub properties: ::std::option::Option<RoleDefinitionProperties>,
    ///The role definition type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RoleDefinition> for RoleDefinition {
    fn from(value: &RoleDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleDefinition {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Role Definitions filter
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role Definitions filter",
///  "type": "object",
///  "properties": {
///    "roleName": {
///      "description": "Returns role definition with the specific name.",
///      "type": "string"
///    },
///    "type": {
///      "description": "Returns role definition with the specific type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleDefinitionFilter {
    ///Returns role definition with the specific name.
    #[serde(
        rename = "roleName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub role_name: ::std::option::Option<::std::string::String>,
    ///Returns role definition with the specific type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RoleDefinitionFilter> for RoleDefinitionFilter {
    fn from(value: &RoleDefinitionFilter) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleDefinitionFilter {
    fn default() -> Self {
        Self {
            role_name: Default::default(),
            type_: Default::default(),
        }
    }
}
///Role definition list operation result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role definition list operation result.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Role definition list.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RoleDefinition"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoleDefinitionListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Role definition list.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<RoleDefinition>,
}
impl ::std::convert::From<&RoleDefinitionListResult> for RoleDefinitionListResult {
    fn from(value: &RoleDefinitionListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleDefinitionListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Role definition properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role definition properties.",
///  "type": "object",
///  "properties": {
///    "assignableScopes": {
///      "description": "Role definition assignable scopes.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
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
///    "description": {
///      "description": "The role definition description.",
///      "type": "string"
///    },
///    "permissions": {
///      "description": "Role definition permissions.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Permission"
///      },
///      "x-ms-identifiers": []
///    },
///    "roleName": {
///      "description": "The role name.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The role type.",
///      "type": "string",
///      "x-ms-client-name": "roleType"
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
pub struct RoleDefinitionProperties {
    ///Role definition assignable scopes.
    #[serde(
        rename = "assignableScopes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub assignable_scopes: ::std::vec::Vec<::std::string::String>,
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
    ///The role definition description.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Role definition permissions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub permissions: ::std::vec::Vec<Permission>,
    ///The role name.
    #[serde(
        rename = "roleName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub role_name: ::std::option::Option<::std::string::String>,
    ///The role type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
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
impl ::std::convert::From<&RoleDefinitionProperties> for RoleDefinitionProperties {
    fn from(value: &RoleDefinitionProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoleDefinitionProperties {
    fn default() -> Self {
        Self {
            assignable_scopes: Default::default(),
            created_by: Default::default(),
            created_on: Default::default(),
            description: Default::default(),
            permissions: Default::default(),
            role_name: Default::default(),
            type_: Default::default(),
            updated_by: Default::default(),
            updated_on: Default::default(),
        }
    }
}
