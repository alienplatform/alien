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
///An error response from the service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the service.",
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
///An error response from the service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the service.",
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
///Nat Gateway resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Nat Gateway resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/NatGatewayPropertiesFormat"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/NatGatewaySku"
///    },
///    "zones": {
///      "description": "A list of availability zones denoting the zone in which Nat Gateway should be deployed.",
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
pub struct NatGateway {
    ///A unique read-only string that changes whenever the resource is updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///Resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Resource location.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Resource name.
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
    pub properties: ::std::option::Option<NatGatewayPropertiesFormat>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<NatGatewaySku>,
    ///Resource tags.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
    ///A list of availability zones denoting the zone in which Nat Gateway should be deployed.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zones: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&NatGateway> for NatGateway {
    fn from(value: &NatGateway) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NatGateway {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
            zones: Default::default(),
        }
    }
}
///Response for ListNatGateways API service call.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Response for ListNatGateways API service call.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of results.",
///      "type": "string"
///    },
///    "value": {
///      "description": "A list of Nat Gateways that exists in a resource group.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NatGateway"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NatGatewayListResult {
    ///The URL to get the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A list of Nat Gateways that exists in a resource group.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<NatGateway>,
}
impl ::std::convert::From<&NatGatewayListResult> for NatGatewayListResult {
    fn from(value: &NatGatewayListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NatGatewayListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Nat Gateway properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Nat Gateway properties.",
///  "properties": {
///    "idleTimeoutInMinutes": {
///      "description": "The idle timeout of the nat gateway.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "publicIpAddresses": {
///      "description": "An array of public ip addresses V4 associated with the nat gateway resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "publicIpAddressesV6": {
///      "description": "An array of public ip addresses V6 associated with the nat gateway resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "publicIpPrefixes": {
///      "description": "An array of public ip prefixes V4 associated with the nat gateway resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "publicIpPrefixesV6": {
///      "description": "An array of public ip prefixes V6 associated with the nat gateway resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the NAT gateway resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "sourceVirtualNetwork": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "subnets": {
///      "description": "An array of references to the subnets using this nat gateway resource.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NatGatewayPropertiesFormat {
    ///The idle timeout of the nat gateway.
    #[serde(
        rename = "idleTimeoutInMinutes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub idle_timeout_in_minutes: ::std::option::Option<i32>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///An array of public ip addresses V4 associated with the nat gateway resource.
    #[serde(
        rename = "publicIpAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_addresses: ::std::vec::Vec<SubResource>,
    ///An array of public ip addresses V6 associated with the nat gateway resource.
    #[serde(
        rename = "publicIpAddressesV6",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_addresses_v6: ::std::vec::Vec<SubResource>,
    ///An array of public ip prefixes V4 associated with the nat gateway resource.
    #[serde(
        rename = "publicIpPrefixes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_prefixes: ::std::vec::Vec<SubResource>,
    ///An array of public ip prefixes V6 associated with the nat gateway resource.
    #[serde(
        rename = "publicIpPrefixesV6",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_prefixes_v6: ::std::vec::Vec<SubResource>,
    ///The resource GUID property of the NAT gateway resource.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "sourceVirtualNetwork",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_virtual_network: ::std::option::Option<SubResource>,
    ///An array of references to the subnets using this nat gateway resource.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnets: ::std::vec::Vec<SubResource>,
}
impl ::std::convert::From<&NatGatewayPropertiesFormat> for NatGatewayPropertiesFormat {
    fn from(value: &NatGatewayPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NatGatewayPropertiesFormat {
    fn default() -> Self {
        Self {
            idle_timeout_in_minutes: Default::default(),
            provisioning_state: Default::default(),
            public_ip_addresses: Default::default(),
            public_ip_addresses_v6: Default::default(),
            public_ip_prefixes: Default::default(),
            public_ip_prefixes_v6: Default::default(),
            resource_guid: Default::default(),
            source_virtual_network: Default::default(),
            subnets: Default::default(),
        }
    }
}
///SKU of nat gateway.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU of nat gateway.",
///  "properties": {
///    "name": {
///      "description": "Name of Nat Gateway SKU.",
///      "type": "string",
///      "enum": [
///        "Standard",
///        "StandardV2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NatGatewaySkuName"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NatGatewaySku {
    ///Name of Nat Gateway SKU.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<NatGatewaySkuName>,
}
impl ::std::convert::From<&NatGatewaySku> for NatGatewaySku {
    fn from(value: &NatGatewaySku) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NatGatewaySku {
    fn default() -> Self {
        Self { name: Default::default() }
    }
}
///Name of Nat Gateway SKU.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Name of Nat Gateway SKU.",
///  "type": "string",
///  "enum": [
///    "Standard",
///    "StandardV2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NatGatewaySkuName"
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
pub enum NatGatewaySkuName {
    Standard,
    StandardV2,
}
impl ::std::convert::From<&Self> for NatGatewaySkuName {
    fn from(value: &NatGatewaySkuName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NatGatewaySkuName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::StandardV2 => f.write_str("StandardV2"),
        }
    }
}
impl ::std::str::FromStr for NatGatewaySkuName {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "standardv2" => Ok(Self::StandardV2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NatGatewaySkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NatGatewaySkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NatGatewaySkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///    "Updating",
///    "Deleting",
///    "Failed"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum ProvisioningState {
    Succeeded,
    Updating,
    Deleting,
    Failed,
}
impl ::std::convert::From<&Self> for ProvisioningState {
    fn from(value: &ProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Updating => f.write_str("Updating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Failed => f.write_str("Failed"),
        }
    }
}
impl ::std::str::FromStr for ProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "updating" => Ok(Self::Updating),
            "deleting" => Ok(Self::Deleting),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Common resource representation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Common resource representation.",
///  "properties": {
///    "id": {
///      "description": "Resource ID.",
///      "type": "string"
///    },
///    "location": {
///      "description": "Resource location.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Resource name.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tags": {
///      "description": "Resource tags.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "Resource type.",
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
    ///Resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Resource location.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Resource name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Resource tags.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type.
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
///Reference to another subresource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Reference to another subresource.",
///  "properties": {
///    "id": {
///      "description": "Resource ID.",
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SubResource {
    ///Resource ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SubResource> for SubResource {
    fn from(value: &SubResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SubResource {
    fn default() -> Self {
        Self { id: Default::default() }
    }
}
///Tags object for patch operations.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tags object for patch operations.",
///  "properties": {
///    "tags": {
///      "description": "Resource tags.",
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
pub struct TagsObject {
    ///Resource tags.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&TagsObject> for TagsObject {
    fn from(value: &TagsObject) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagsObject {
    fn default() -> Self {
        Self { tags: Default::default() }
    }
}
