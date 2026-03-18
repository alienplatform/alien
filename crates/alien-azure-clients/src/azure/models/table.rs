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
///Response schema. Contains list of tables returned
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Response schema. Contains list of tables returned",
///  "properties": {
///    "nextLink": {
///      "description": "Request URL that can be used to query next page of tables",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "List of tables returned.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Table"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListTableResource {
    ///Request URL that can be used to query next page of tables
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///List of tables returned.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Table>,
}
impl ::std::convert::From<&ListTableResource> for ListTableResource {
    fn from(value: &ListTableResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListTableResource {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///`ListTableServices`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "properties": {
///    "value": {
///      "description": "List of table services returned.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TableServiceProperties"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListTableServices {
    ///List of table services returned.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<TableServiceProperties>,
}
impl ::std::convert::From<&ListTableServices> for ListTableServices {
    fn from(value: &ListTableServices) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListTableServices {
    fn default() -> Self {
        Self {
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
///Properties of the table, including Id, resource name, resource type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the table, including Id, resource name, resource type.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/TableProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Table {
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
    pub properties: ::std::option::Option<TableProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Table> for Table {
    fn from(value: &Table) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Table {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Table Access Policy Properties Object.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Table Access Policy Properties Object.",
///  "type": "object",
///  "required": [
///    "permission"
///  ],
///  "properties": {
///    "expiryTime": {
///      "description": "Expiry time of the access policy",
///      "type": "string"
///    },
///    "permission": {
///      "description": "Required. List of abbreviated permissions. Supported permission values include 'r','a','u','d'",
///      "type": "string"
///    },
///    "startTime": {
///      "description": "Start time of the access policy",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TableAccessPolicy {
    ///Expiry time of the access policy
    #[serde(
        rename = "expiryTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expiry_time: ::std::option::Option<::std::string::String>,
    ///Required. List of abbreviated permissions. Supported permission values include 'r','a','u','d'
    pub permission: ::std::string::String,
    ///Start time of the access policy
    #[serde(
        rename = "startTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub start_time: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TableAccessPolicy> for TableAccessPolicy {
    fn from(value: &TableAccessPolicy) -> Self {
        value.clone()
    }
}
///`TableProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "properties": {
///    "signedIdentifiers": {
///      "description": "List of stored access policies specified on the table.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TableSignedIdentifier"
///      }
///    },
///    "tableName": {
///      "description": "Table name under the specified account",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TableProperties {
    ///List of stored access policies specified on the table.
    #[serde(
        rename = "signedIdentifiers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_identifiers: ::std::vec::Vec<TableSignedIdentifier>,
    ///Table name under the specified account
    #[serde(
        rename = "tableName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub table_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TableProperties> for TableProperties {
    fn from(value: &TableProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TableProperties {
    fn default() -> Self {
        Self {
            signed_identifiers: Default::default(),
            table_name: Default::default(),
        }
    }
}
///The properties of a storage account’s Table service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a storage account’s Table service.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "The properties of a storage account’s Table service.",
///      "properties": {
///        "cors": {
///          "$ref": "#/components/schemas/CorsRules"
///        }
///      },
///      "x-ms-client-flatten": true,
///      "x-ms-client-name": "TableServiceProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TableServiceProperties {
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
    pub properties: ::std::option::Option<TableServicePropertiesProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TableServiceProperties> for TableServiceProperties {
    fn from(value: &TableServiceProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TableServiceProperties {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///The properties of a storage account’s Table service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a storage account’s Table service.",
///  "properties": {
///    "cors": {
///      "$ref": "#/components/schemas/CorsRules"
///    }
///  },
///  "x-ms-client-flatten": true,
///  "x-ms-client-name": "TableServiceProperties"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TableServicePropertiesProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cors: ::std::option::Option<CorsRules>,
}
impl ::std::convert::From<&TableServicePropertiesProperties> for TableServicePropertiesProperties {
    fn from(value: &TableServicePropertiesProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TableServicePropertiesProperties {
    fn default() -> Self {
        Self {
            cors: Default::default(),
        }
    }
}
///Object to set Table Access Policy.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Object to set Table Access Policy.",
///  "type": "object",
///  "required": [
///    "id"
///  ],
///  "properties": {
///    "accessPolicy": {
///      "$ref": "#/components/schemas/TableAccessPolicy"
///    },
///    "id": {
///      "description": "unique-64-character-value of the stored access policy.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TableSignedIdentifier {
    #[serde(
        rename = "accessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_policy: ::std::option::Option<TableAccessPolicy>,
    ///unique-64-character-value of the stored access policy.
    pub id: ::std::string::String,
}
impl ::std::convert::From<&TableSignedIdentifier> for TableSignedIdentifier {
    fn from(value: &TableSignedIdentifier) -> Self {
        value.clone()
    }
}
