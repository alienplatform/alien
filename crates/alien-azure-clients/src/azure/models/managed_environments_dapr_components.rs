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
///Dapr Component.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dapr Component.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "Dapr Component resource specific properties",
///      "type": "object",
///      "properties": {
///        "componentType": {
///          "description": "Component type",
///          "type": "string"
///        },
///        "ignoreErrors": {
///          "description": "Boolean describing if the component errors are ignores",
///          "default": false,
///          "type": "boolean"
///        },
///        "initTimeout": {
///          "description": "Initialization timeout",
///          "type": "string"
///        },
///        "metadata": {
///          "description": "Component metadata",
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/DaprMetadata"
///          },
///          "x-ms-identifiers": [
///            "name"
///          ]
///        },
///        "scopes": {
///          "description": "Names of container apps that can use this Dapr component",
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "secretStoreComponent": {
///          "description": "Name of a Dapr component to retrieve component secrets from",
///          "type": "string"
///        },
///        "secrets": {
///          "description": "Collection of secrets used by a Dapr component",
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/Secret"
///          },
///          "x-ms-identifiers": [
///            "name"
///          ]
///        },
///        "version": {
///          "description": "Component version",
///          "type": "string"
///        }
///      },
///      "x-ms-client-flatten": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DaprComponent {
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
    pub properties: ::std::option::Option<DaprComponentProperties>,
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
impl ::std::convert::From<&DaprComponent> for DaprComponent {
    fn from(value: &DaprComponent) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DaprComponent {
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
///Dapr Component resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dapr Component resource specific properties",
///  "type": "object",
///  "properties": {
///    "componentType": {
///      "description": "Component type",
///      "type": "string"
///    },
///    "ignoreErrors": {
///      "description": "Boolean describing if the component errors are ignores",
///      "default": false,
///      "type": "boolean"
///    },
///    "initTimeout": {
///      "description": "Initialization timeout",
///      "type": "string"
///    },
///    "metadata": {
///      "description": "Component metadata",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DaprMetadata"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "scopes": {
///      "description": "Names of container apps that can use this Dapr component",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "secretStoreComponent": {
///      "description": "Name of a Dapr component to retrieve component secrets from",
///      "type": "string"
///    },
///    "secrets": {
///      "description": "Collection of secrets used by a Dapr component",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Secret"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "version": {
///      "description": "Component version",
///      "type": "string"
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DaprComponentProperties {
    ///Component type
    #[serde(
        rename = "componentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub component_type: ::std::option::Option<::std::string::String>,
    ///Boolean describing if the component errors are ignores
    #[serde(
        rename = "ignoreErrors",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ignore_errors: bool,
    ///Initialization timeout
    #[serde(
        rename = "initTimeout",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub init_timeout: ::std::option::Option<::std::string::String>,
    ///Component metadata
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::std::vec::Vec<DaprMetadata>,
    ///Names of container apps that can use this Dapr component
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scopes: ::std::vec::Vec<::std::string::String>,
    ///Name of a Dapr component to retrieve component secrets from
    #[serde(
        rename = "secretStoreComponent",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secret_store_component: ::std::option::Option<::std::string::String>,
    ///Collection of secrets used by a Dapr component
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secrets: ::std::vec::Vec<Secret>,
    ///Component version
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DaprComponentProperties> for DaprComponentProperties {
    fn from(value: &DaprComponentProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DaprComponentProperties {
    fn default() -> Self {
        Self {
            component_type: Default::default(),
            ignore_errors: Default::default(),
            init_timeout: Default::default(),
            metadata: Default::default(),
            scopes: Default::default(),
            secret_store_component: Default::default(),
            secrets: Default::default(),
            version: Default::default(),
        }
    }
}
///Dapr Components ARM resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dapr Components ARM resource.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "nextLink": {
///      "description": "Link to next page of resources.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "Collection of resources.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DaprComponent"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DaprComponentsCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<DaprComponent>,
}
impl ::std::convert::From<&DaprComponentsCollection> for DaprComponentsCollection {
    fn from(value: &DaprComponentsCollection) -> Self {
        value.clone()
    }
}
///Dapr component metadata.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dapr component metadata.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "Metadata property name.",
///      "type": "string"
///    },
///    "secretRef": {
///      "description": "Name of the Dapr Component secret from which to pull the metadata property value.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Metadata property value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DaprMetadata {
    ///Metadata property name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Name of the Dapr Component secret from which to pull the metadata property value.
    #[serde(
        rename = "secretRef",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secret_ref: ::std::option::Option<::std::string::String>,
    ///Metadata property value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DaprMetadata> for DaprMetadata {
    fn from(value: &DaprMetadata) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DaprMetadata {
    fn default() -> Self {
        Self {
            name: Default::default(),
            secret_ref: Default::default(),
            value: Default::default(),
        }
    }
}
///Dapr component Secret for ListSecrets Action
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dapr component Secret for ListSecrets Action",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "Secret Name.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "Secret Value.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-secret": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DaprSecret {
    ///Secret Name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Secret Value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DaprSecret> for DaprSecret {
    fn from(value: &DaprSecret) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DaprSecret {
    fn default() -> Self {
        Self {
            name: Default::default(),
            value: Default::default(),
        }
    }
}
///Dapr component Secrets Collection for ListSecrets Action.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dapr component Secrets Collection for ListSecrets Action.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "Collection of secrets used by a Dapr component",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DaprSecret"
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
pub struct DaprSecretsCollection {
    ///Collection of secrets used by a Dapr component
    pub value: ::std::vec::Vec<DaprSecret>,
}
impl ::std::convert::From<&DaprSecretsCollection> for DaprSecretsCollection {
    fn from(value: &DaprSecretsCollection) -> Self {
        value.clone()
    }
}
///App Service error response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "App Service error response.",
///  "type": "object",
///  "properties": {
///    "error": {
///      "description": "Error model.",
///      "readOnly": true,
///      "type": "object",
///      "properties": {
///        "code": {
///          "description": "Standardized string to programmatically identify the error.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "details": {
///          "description": "Details or the error",
///          "type": "array",
///          "items": {
///            "description": "Detailed errors.",
///            "readOnly": true,
///            "type": "object",
///            "properties": {
///              "code": {
///                "description": "Standardized string to programmatically identify the error.",
///                "readOnly": true,
///                "type": "string"
///              },
///              "message": {
///                "description": "Detailed error description and debugging information.",
///                "readOnly": true,
///                "type": "string"
///              },
///              "target": {
///                "description": "Detailed error description and debugging information.",
///                "readOnly": true,
///                "type": "string"
///              }
///            }
///          },
///          "x-ms-identifiers": [
///            "code"
///          ]
///        },
///        "innererror": {
///          "description": "More information to debug error.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "message": {
///          "description": "Detailed error description and debugging information.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "target": {
///          "description": "Detailed error description and debugging information.",
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
pub struct DefaultErrorResponse {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<DefaultErrorResponseError>,
}
impl ::std::convert::From<&DefaultErrorResponse> for DefaultErrorResponse {
    fn from(value: &DefaultErrorResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DefaultErrorResponse {
    fn default() -> Self {
        Self { error: Default::default() }
    }
}
///Error model.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Error model.",
///  "readOnly": true,
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "Standardized string to programmatically identify the error.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "details": {
///      "description": "Details or the error",
///      "type": "array",
///      "items": {
///        "description": "Detailed errors.",
///        "readOnly": true,
///        "type": "object",
///        "properties": {
///          "code": {
///            "description": "Standardized string to programmatically identify the error.",
///            "readOnly": true,
///            "type": "string"
///          },
///          "message": {
///            "description": "Detailed error description and debugging information.",
///            "readOnly": true,
///            "type": "string"
///          },
///          "target": {
///            "description": "Detailed error description and debugging information.",
///            "readOnly": true,
///            "type": "string"
///          }
///        }
///      },
///      "x-ms-identifiers": [
///        "code"
///      ]
///    },
///    "innererror": {
///      "description": "More information to debug error.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "message": {
///      "description": "Detailed error description and debugging information.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "target": {
///      "description": "Detailed error description and debugging information.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DefaultErrorResponseError {
    ///Standardized string to programmatically identify the error.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///Details or the error
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub details: ::std::vec::Vec<DefaultErrorResponseErrorDetailsItem>,
    ///More information to debug error.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub innererror: ::std::option::Option<::std::string::String>,
    ///Detailed error description and debugging information.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///Detailed error description and debugging information.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DefaultErrorResponseError> for DefaultErrorResponseError {
    fn from(value: &DefaultErrorResponseError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DefaultErrorResponseError {
    fn default() -> Self {
        Self {
            code: Default::default(),
            details: Default::default(),
            innererror: Default::default(),
            message: Default::default(),
            target: Default::default(),
        }
    }
}
///Detailed errors.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Detailed errors.",
///  "readOnly": true,
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "Standardized string to programmatically identify the error.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "message": {
///      "description": "Detailed error description and debugging information.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "target": {
///      "description": "Detailed error description and debugging information.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DefaultErrorResponseErrorDetailsItem {
    ///Standardized string to programmatically identify the error.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///Detailed error description and debugging information.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///Detailed error description and debugging information.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DefaultErrorResponseErrorDetailsItem>
for DefaultErrorResponseErrorDetailsItem {
    fn from(value: &DefaultErrorResponseErrorDetailsItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DefaultErrorResponseErrorDetailsItem {
    fn default() -> Self {
        Self {
            code: Default::default(),
            message: Default::default(),
            target: Default::default(),
        }
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
///Secret definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Secret definition.",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "description": "Resource ID of a managed identity to authenticate with Azure Key Vault, or System to use a system-assigned identity.",
///      "type": "string"
///    },
///    "keyVaultUrl": {
///      "description": "Azure Key Vault URL pointing to the secret referenced by the container app.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Secret Name.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Secret Value.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "update"
///      ],
///      "x-ms-secret": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Secret {
    ///Resource ID of a managed identity to authenticate with Azure Key Vault, or System to use a system-assigned identity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///Azure Key Vault URL pointing to the secret referenced by the container app.
    #[serde(
        rename = "keyVaultUrl",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_vault_url: ::std::option::Option<::std::string::String>,
    ///Secret Name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Secret Value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Secret> for Secret {
    fn from(value: &Secret) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Secret {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            key_vault_url: Default::default(),
            name: Default::default(),
            value: Default::default(),
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
