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
///Backend address of an application gateway.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Backend address of an application gateway.",
///  "properties": {
///    "fqdn": {
///      "description": "Fully qualified domain name (FQDN).",
///      "type": "string"
///    },
///    "ipAddress": {
///      "description": "IP address.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationGatewayBackendAddress {
    ///Fully qualified domain name (FQDN).
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fqdn: ::std::option::Option<::std::string::String>,
    ///IP address.
    #[serde(
        rename = "ipAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_address: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApplicationGatewayBackendAddress> for ApplicationGatewayBackendAddress {
    fn from(value: &ApplicationGatewayBackendAddress) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationGatewayBackendAddress {
    fn default() -> Self {
        Self {
            fqdn: Default::default(),
            ip_address: Default::default(),
        }
    }
}
///Backend Address Pool of an application gateway.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Backend Address Pool of an application gateway.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the backend address pool that is unique within an Application Gateway.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ApplicationGatewayBackendAddressPoolPropertiesFormat"
///    },
///    "type": {
///      "description": "Type of the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationGatewayBackendAddressPool {
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
    ///Name of the backend address pool that is unique within an Application Gateway.
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
    pub properties: ::std::option::Option<ApplicationGatewayBackendAddressPoolPropertiesFormat>,
    ///Type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApplicationGatewayBackendAddressPool>
    for ApplicationGatewayBackendAddressPool
{
    fn from(value: &ApplicationGatewayBackendAddressPool) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationGatewayBackendAddressPool {
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
///Properties of Backend Address Pool of an application gateway.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of Backend Address Pool of an application gateway.",
///  "properties": {
///    "backendAddresses": {
///      "description": "Backend addresses.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationGatewayBackendAddress"
///      }
///    },
///    "backendIPConfigurations": {
///      "description": "Collection of references to IPs defined in network interfaces.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterfaceIPConfiguration"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationGatewayBackendAddressPoolPropertiesFormat {
    ///Backend addresses.
    #[serde(
        rename = "backendAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_addresses: ::std::vec::Vec<ApplicationGatewayBackendAddress>,
    ///Collection of references to IPs defined in network interfaces.
    #[serde(
        rename = "backendIPConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_ip_configurations: ::std::vec::Vec<NetworkInterfaceIpConfiguration>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
}
impl ::std::convert::From<&ApplicationGatewayBackendAddressPoolPropertiesFormat>
    for ApplicationGatewayBackendAddressPoolPropertiesFormat
{
    fn from(value: &ApplicationGatewayBackendAddressPoolPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationGatewayBackendAddressPoolPropertiesFormat {
    fn default() -> Self {
        Self {
            backend_addresses: Default::default(),
            backend_ip_configurations: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///IP configuration of an application gateway. Currently 1 public and 1 private IP configuration is allowed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP configuration of an application gateway. Currently 1 public and 1 private IP configuration is allowed.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the IP configuration that is unique within an Application Gateway.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ApplicationGatewayIPConfigurationPropertiesFormat"
///    },
///    "type": {
///      "description": "Type of the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationGatewayIpConfiguration {
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
    ///Name of the IP configuration that is unique within an Application Gateway.
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
    pub properties: ::std::option::Option<ApplicationGatewayIpConfigurationPropertiesFormat>,
    ///Type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApplicationGatewayIpConfiguration>
    for ApplicationGatewayIpConfiguration
{
    fn from(value: &ApplicationGatewayIpConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationGatewayIpConfiguration {
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
///Properties of IP configuration of an application gateway.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of IP configuration of an application gateway.",
///  "properties": {
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationGatewayIpConfigurationPropertiesFormat {
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<SubResource>,
}
impl ::std::convert::From<&ApplicationGatewayIpConfigurationPropertiesFormat>
    for ApplicationGatewayIpConfigurationPropertiesFormat
{
    fn from(value: &ApplicationGatewayIpConfigurationPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationGatewayIpConfigurationPropertiesFormat {
    fn default() -> Self {
        Self {
            provisioning_state: Default::default(),
            subnet: Default::default(),
        }
    }
}
///An application security group in a resource group.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An application security group in a resource group.",
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
///      "$ref": "#/components/schemas/ApplicationSecurityGroupPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationSecurityGroup {
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
    pub properties: ::std::option::Option<ApplicationSecurityGroupPropertiesFormat>,
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
impl ::std::convert::From<&ApplicationSecurityGroup> for ApplicationSecurityGroup {
    fn from(value: &ApplicationSecurityGroup) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationSecurityGroup {
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
///Application security group properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Application security group properties.",
///  "properties": {
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the application security group resource. It uniquely identifies a resource, even if the user changes its name or migrate the resource across subscriptions or resource groups.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApplicationSecurityGroupPropertiesFormat {
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The resource GUID property of the application security group resource. It uniquely identifies a resource, even if the user changes its name or migrate the resource across subscriptions or resource groups.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApplicationSecurityGroupPropertiesFormat>
    for ApplicationSecurityGroupPropertiesFormat
{
    fn from(value: &ApplicationSecurityGroupPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApplicationSecurityGroupPropertiesFormat {
    fn default() -> Self {
        Self {
            provisioning_state: Default::default(),
            resource_guid: Default::default(),
        }
    }
}
///Pool of backend IP addresses.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Pool of backend IP addresses.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within the set of backend address pools used by the load balancer. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/BackendAddressPoolPropertiesFormat"
///    },
///    "type": {
///      "description": "Type of the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BackendAddressPool {
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
    ///The name of the resource that is unique within the set of backend address pools used by the load balancer. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<BackendAddressPoolPropertiesFormat>,
    ///Type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&BackendAddressPool> for BackendAddressPool {
    fn from(value: &BackendAddressPool) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BackendAddressPool {
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
///Properties of the backend address pool.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the backend address pool.",
///  "properties": {
///    "backendIPConfigurations": {
///      "description": "An array of references to IP addresses defined in network interfaces.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterfaceIPConfiguration"
///      }
///    },
///    "drainPeriodInSeconds": {
///      "description": "Amount of seconds Load Balancer waits for before sending RESET to client and backend address.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "inboundNatRules": {
///      "description": "An array of references to inbound NAT rules that use this backend address pool.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "loadBalancerBackendAddresses": {
///      "description": "An array of backend addresses.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/LoadBalancerBackendAddress"
///      }
///    },
///    "loadBalancingRules": {
///      "description": "An array of references to load balancing rules that use this backend address pool.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "location": {
///      "description": "The location of the backend address pool.",
///      "type": "string"
///    },
///    "outboundRule": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "outboundRules": {
///      "description": "An array of references to outbound rules that use this backend address pool.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "syncMode": {
///      "description": "Backend address synchronous mode for the backend pool",
///      "type": "string",
///      "enum": [
///        "Automatic",
///        "Manual"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SyncMode"
///      }
///    },
///    "tunnelInterfaces": {
///      "description": "An array of gateway load balancer tunnel interfaces.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/GatewayLoadBalancerTunnelInterface"
///      }
///    },
///    "virtualNetwork": {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BackendAddressPoolPropertiesFormat {
    ///An array of references to IP addresses defined in network interfaces.
    #[serde(
        rename = "backendIPConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_ip_configurations: ::std::vec::Vec<NetworkInterfaceIpConfiguration>,
    ///Amount of seconds Load Balancer waits for before sending RESET to client and backend address.
    #[serde(
        rename = "drainPeriodInSeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub drain_period_in_seconds: ::std::option::Option<i32>,
    ///An array of references to inbound NAT rules that use this backend address pool.
    #[serde(
        rename = "inboundNatRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub inbound_nat_rules: ::std::vec::Vec<SubResource>,
    ///An array of backend addresses.
    #[serde(
        rename = "loadBalancerBackendAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancer_backend_addresses: ::std::vec::Vec<LoadBalancerBackendAddress>,
    ///An array of references to load balancing rules that use this backend address pool.
    #[serde(
        rename = "loadBalancingRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancing_rules: ::std::vec::Vec<SubResource>,
    ///The location of the backend address pool.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "outboundRule",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub outbound_rule: ::std::option::Option<SubResource>,
    ///An array of references to outbound rules that use this backend address pool.
    #[serde(
        rename = "outboundRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub outbound_rules: ::std::vec::Vec<SubResource>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///Backend address synchronous mode for the backend pool
    #[serde(
        rename = "syncMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sync_mode: ::std::option::Option<BackendAddressPoolPropertiesFormatSyncMode>,
    ///An array of gateway load balancer tunnel interfaces.
    #[serde(
        rename = "tunnelInterfaces",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tunnel_interfaces: ::std::vec::Vec<GatewayLoadBalancerTunnelInterface>,
    #[serde(
        rename = "virtualNetwork",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_network: ::std::option::Option<SubResource>,
}
impl ::std::convert::From<&BackendAddressPoolPropertiesFormat>
    for BackendAddressPoolPropertiesFormat
{
    fn from(value: &BackendAddressPoolPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BackendAddressPoolPropertiesFormat {
    fn default() -> Self {
        Self {
            backend_ip_configurations: Default::default(),
            drain_period_in_seconds: Default::default(),
            inbound_nat_rules: Default::default(),
            load_balancer_backend_addresses: Default::default(),
            load_balancing_rules: Default::default(),
            location: Default::default(),
            outbound_rule: Default::default(),
            outbound_rules: Default::default(),
            provisioning_state: Default::default(),
            sync_mode: Default::default(),
            tunnel_interfaces: Default::default(),
            virtual_network: Default::default(),
        }
    }
}
///Backend address synchronous mode for the backend pool
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Backend address synchronous mode for the backend pool",
///  "type": "string",
///  "enum": [
///    "Automatic",
///    "Manual"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SyncMode"
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
pub enum BackendAddressPoolPropertiesFormatSyncMode {
    Automatic,
    Manual,
}
impl ::std::convert::From<&Self> for BackendAddressPoolPropertiesFormatSyncMode {
    fn from(value: &BackendAddressPoolPropertiesFormatSyncMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for BackendAddressPoolPropertiesFormatSyncMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Automatic => f.write_str("Automatic"),
            Self::Manual => f.write_str("Manual"),
        }
    }
}
impl ::std::str::FromStr for BackendAddressPoolPropertiesFormatSyncMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "automatic" => Ok(Self::Automatic),
            "manual" => Ok(Self::Manual),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BackendAddressPoolPropertiesFormatSyncMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for BackendAddressPoolPropertiesFormatSyncMode
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BackendAddressPoolPropertiesFormatSyncMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
        Self {
            error: Default::default(),
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
///Contains custom Dns resolution configuration from customer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Contains custom Dns resolution configuration from customer.",
///  "properties": {
///    "fqdn": {
///      "description": "Fqdn that resolves to private endpoint ip address.",
///      "type": "string"
///    },
///    "ipAddresses": {
///      "description": "A list of private ip addresses of the private endpoint.",
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
pub struct CustomDnsConfigPropertiesFormat {
    ///Fqdn that resolves to private endpoint ip address.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fqdn: ::std::option::Option<::std::string::String>,
    ///A list of private ip addresses of the private endpoint.
    #[serde(
        rename = "ipAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_addresses: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&CustomDnsConfigPropertiesFormat> for CustomDnsConfigPropertiesFormat {
    fn from(value: &CustomDnsConfigPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CustomDnsConfigPropertiesFormat {
    fn default() -> Self {
        Self {
            fqdn: Default::default(),
            ip_addresses: Default::default(),
        }
    }
}
///Contains the DDoS protection settings of the public IP.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Contains the DDoS protection settings of the public IP.",
///  "properties": {
///    "ddosProtectionPlan": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "protectionMode": {
///      "description": "The DDoS protection mode of the public IP",
///      "type": "string",
///      "enum": [
///        "VirtualNetworkInherited",
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DdosSettingsProtectionMode"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DdosSettings {
    #[serde(
        rename = "ddosProtectionPlan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ddos_protection_plan: ::std::option::Option<SubResource>,
    ///The DDoS protection mode of the public IP
    #[serde(
        rename = "protectionMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub protection_mode: ::std::option::Option<DdosSettingsProtectionMode>,
}
impl ::std::convert::From<&DdosSettings> for DdosSettings {
    fn from(value: &DdosSettings) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DdosSettings {
    fn default() -> Self {
        Self {
            ddos_protection_plan: Default::default(),
            protection_mode: Default::default(),
        }
    }
}
///The DDoS protection mode of the public IP
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The DDoS protection mode of the public IP",
///  "type": "string",
///  "enum": [
///    "VirtualNetworkInherited",
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DdosSettingsProtectionMode"
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
pub enum DdosSettingsProtectionMode {
    VirtualNetworkInherited,
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for DdosSettingsProtectionMode {
    fn from(value: &DdosSettingsProtectionMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DdosSettingsProtectionMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::VirtualNetworkInherited => f.write_str("VirtualNetworkInherited"),
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for DdosSettingsProtectionMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "virtualnetworkinherited" => Ok(Self::VirtualNetworkInherited),
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DdosSettingsProtectionMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DdosSettingsProtectionMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DdosSettingsProtectionMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Details the service to which the subnet is delegated.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Details the service to which the subnet is delegated.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a subnet. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ServiceDelegationPropertiesFormat"
///    },
///    "type": {
///      "description": "Resource type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Delegation {
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
    ///The name of the resource that is unique within a subnet. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<ServiceDelegationPropertiesFormat>,
    ///Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Delegation> for Delegation {
    fn from(value: &Delegation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Delegation {
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
///The request for DisassociateCloudServicePublicIpOperation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The request for DisassociateCloudServicePublicIpOperation.",
///  "type": "object",
///  "required": [
///    "publicIpArmId"
///  ],
///  "properties": {
///    "publicIpArmId": {
///      "description": "ARM ID of the Standalone Public IP to associate. This is of the form : /subscriptions/{subscriptionId}/resourcegroups/{resourceGroupName}/providers/Microsoft.Network/publicIPAddresses/{publicIpAddressName}",
///      "type": "string",
///      "format": "arm-id",
///      "x-ms-arm-id-details": {
///        "allowedResources": [
///          {
///            "type": "Microsoft.Network/publicIPAddresses"
///          }
///        ]
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DisassociateCloudServicePublicIpRequest {
    ///ARM ID of the Standalone Public IP to associate. This is of the form : /subscriptions/{subscriptionId}/resourcegroups/{resourceGroupName}/providers/Microsoft.Network/publicIPAddresses/{publicIpAddressName}
    #[serde(rename = "publicIpArmId")]
    pub public_ip_arm_id: ::std::string::String,
}
impl ::std::convert::From<&DisassociateCloudServicePublicIpRequest>
    for DisassociateCloudServicePublicIpRequest
{
    fn from(value: &DisassociateCloudServicePublicIpRequest) -> Self {
        value.clone()
    }
}
///ExtendedLocation complex type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "ExtendedLocation complex type.",
///  "properties": {
///    "name": {
///      "description": "The name of the extended location.",
///      "type": "string"
///    },
///    "type": {
///      "$ref": "#/components/schemas/ExtendedLocationType"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ExtendedLocation {
    ///The name of the extended location.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<ExtendedLocationType>,
}
impl ::std::convert::From<&ExtendedLocation> for ExtendedLocation {
    fn from(value: &ExtendedLocation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ExtendedLocation {
    fn default() -> Self {
        Self {
            name: Default::default(),
            type_: Default::default(),
        }
    }
}
///The supported ExtendedLocation types. Currently only EdgeZone is supported in Microsoft.Network resources.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The supported ExtendedLocation types. Currently only EdgeZone is supported in Microsoft.Network resources.",
///  "type": "string",
///  "enum": [
///    "EdgeZone"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ExtendedLocationTypes"
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
pub enum ExtendedLocationType {
    EdgeZone,
}
impl ::std::convert::From<&Self> for ExtendedLocationType {
    fn from(value: &ExtendedLocationType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ExtendedLocationType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EdgeZone => f.write_str("EdgeZone"),
        }
    }
}
impl ::std::str::FromStr for ExtendedLocationType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "edgezone" => Ok(Self::EdgeZone),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExtendedLocationType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExtendedLocationType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExtendedLocationType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A flow log resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A flow log resource.",
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
///    "identity": {
///      "$ref": "#/components/schemas/ManagedServiceIdentity"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/FlowLogPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct FlowLog {
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
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<ManagedServiceIdentity>,
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
    pub properties: ::std::option::Option<FlowLogPropertiesFormat>,
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
impl ::std::convert::From<&FlowLog> for FlowLog {
    fn from(value: &FlowLog) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for FlowLog {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            identity: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Parameters that define the flow log format.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters that define the flow log format.",
///  "properties": {
///    "type": {
///      "description": "The file type of flow log.",
///      "type": "string",
///      "enum": [
///        "JSON"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "FlowLogFormatType"
///      }
///    },
///    "version": {
///      "description": "The version (revision) of the flow log.",
///      "default": 0,
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct FlowLogFormatParameters {
    ///The file type of flow log.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<FlowLogFormatParametersType>,
    ///The version (revision) of the flow log.
    #[serde(
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: i32,
}
impl ::std::convert::From<&FlowLogFormatParameters> for FlowLogFormatParameters {
    fn from(value: &FlowLogFormatParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for FlowLogFormatParameters {
    fn default() -> Self {
        Self {
            type_: Default::default(),
            version: Default::default(),
        }
    }
}
///The file type of flow log.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The file type of flow log.",
///  "type": "string",
///  "enum": [
///    "JSON"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "FlowLogFormatType"
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
pub enum FlowLogFormatParametersType {
    #[serde(rename = "JSON")]
    Json,
}
impl ::std::convert::From<&Self> for FlowLogFormatParametersType {
    fn from(value: &FlowLogFormatParametersType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for FlowLogFormatParametersType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Json => f.write_str("JSON"),
        }
    }
}
impl ::std::str::FromStr for FlowLogFormatParametersType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for FlowLogFormatParametersType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for FlowLogFormatParametersType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for FlowLogFormatParametersType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Parameters that define the configuration of flow log.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters that define the configuration of flow log.",
///  "required": [
///    "storageId",
///    "targetResourceId"
///  ],
///  "properties": {
///    "enabled": {
///      "description": "Flag to enable/disable flow logging.",
///      "type": "boolean"
///    },
///    "enabledFilteringCriteria": {
///      "description": "Optional field to filter network traffic logs based on SrcIP, SrcPort, DstIP, DstPort, Protocol, Encryption, Direction and Action. If not specified, all network traffic will be logged.",
///      "type": "string"
///    },
///    "flowAnalyticsConfiguration": {
///      "$ref": "#/components/schemas/TrafficAnalyticsProperties"
///    },
///    "format": {
///      "$ref": "#/components/schemas/FlowLogFormatParameters"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "recordTypes": {
///      "description": "Optional field to filter network traffic logs based on flow states. Value of this field could be any comma separated combination string of letters B,C,E or D. B represents Begin, when a flow is created. C represents Continue for an ongoing flow generated at every five-minute interval. E represents End, when a flow is terminated. D represents Deny, when a flow is denied. If not specified, all network traffic will be logged.",
///      "type": "string"
///    },
///    "retentionPolicy": {
///      "$ref": "#/components/schemas/RetentionPolicyParameters"
///    },
///    "storageId": {
///      "description": "ID of the storage account which is used to store the flow log.",
///      "type": "string"
///    },
///    "targetResourceGuid": {
///      "description": "Guid of network security group to which flow log will be applied.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "targetResourceId": {
///      "description": "ID of network security group to which flow log will be applied.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct FlowLogPropertiesFormat {
    ///Flag to enable/disable flow logging.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///Optional field to filter network traffic logs based on SrcIP, SrcPort, DstIP, DstPort, Protocol, Encryption, Direction and Action. If not specified, all network traffic will be logged.
    #[serde(
        rename = "enabledFilteringCriteria",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_filtering_criteria: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "flowAnalyticsConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub flow_analytics_configuration: ::std::option::Option<TrafficAnalyticsProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub format: ::std::option::Option<FlowLogFormatParameters>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///Optional field to filter network traffic logs based on flow states. Value of this field could be any comma separated combination string of letters B,C,E or D. B represents Begin, when a flow is created. C represents Continue for an ongoing flow generated at every five-minute interval. E represents End, when a flow is terminated. D represents Deny, when a flow is denied. If not specified, all network traffic will be logged.
    #[serde(
        rename = "recordTypes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub record_types: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "retentionPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub retention_policy: ::std::option::Option<RetentionPolicyParameters>,
    ///ID of the storage account which is used to store the flow log.
    #[serde(rename = "storageId")]
    pub storage_id: ::std::string::String,
    ///Guid of network security group to which flow log will be applied.
    #[serde(
        rename = "targetResourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_resource_guid: ::std::option::Option<::std::string::String>,
    ///ID of network security group to which flow log will be applied.
    #[serde(rename = "targetResourceId")]
    pub target_resource_id: ::std::string::String,
}
impl ::std::convert::From<&FlowLogPropertiesFormat> for FlowLogPropertiesFormat {
    fn from(value: &FlowLogPropertiesFormat) -> Self {
        value.clone()
    }
}
///Frontend IP address of the load balancer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Frontend IP address of the load balancer.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within the set of frontend IP configurations used by the load balancer. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/FrontendIPConfigurationPropertiesFormat"
///    },
///    "type": {
///      "description": "Type of the resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "zones": {
///      "description": "A list of availability zones denoting the IP allocated for the resource needs to come from.",
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
pub struct FrontendIpConfiguration {
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
    ///The name of the resource that is unique within the set of frontend IP configurations used by the load balancer. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<FrontendIpConfigurationPropertiesFormat>,
    ///Type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
    ///A list of availability zones denoting the IP allocated for the resource needs to come from.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zones: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&FrontendIpConfiguration> for FrontendIpConfiguration {
    fn from(value: &FrontendIpConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for FrontendIpConfiguration {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
            zones: Default::default(),
        }
    }
}
///Properties of Frontend IP Configuration of the load balancer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of Frontend IP Configuration of the load balancer.",
///  "properties": {
///    "gatewayLoadBalancer": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "inboundNatPools": {
///      "description": "An array of references to inbound pools that use this frontend IP.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "inboundNatRules": {
///      "description": "An array of references to inbound rules that use this frontend IP.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "loadBalancingRules": {
///      "description": "An array of references to load balancing rules that use this frontend IP.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "outboundRules": {
///      "description": "An array of references to outbound rules that use this frontend IP.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "privateIPAddress": {
///      "description": "The private IP address of the IP configuration.",
///      "type": "string"
///    },
///    "privateIPAddressVersion": {
///      "$ref": "#/components/schemas/IPVersion"
///    },
///    "privateIPAllocationMethod": {
///      "$ref": "#/components/schemas/IPAllocationMethod"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "publicIPAddress": {
///      "$ref": "#/components/schemas/PublicIPAddress"
///    },
///    "publicIPPrefix": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/Subnet"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct FrontendIpConfigurationPropertiesFormat {
    #[serde(
        rename = "gatewayLoadBalancer",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub gateway_load_balancer: ::std::option::Option<SubResource>,
    ///An array of references to inbound pools that use this frontend IP.
    #[serde(
        rename = "inboundNatPools",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub inbound_nat_pools: ::std::vec::Vec<SubResource>,
    ///An array of references to inbound rules that use this frontend IP.
    #[serde(
        rename = "inboundNatRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub inbound_nat_rules: ::std::vec::Vec<SubResource>,
    ///An array of references to load balancing rules that use this frontend IP.
    #[serde(
        rename = "loadBalancingRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancing_rules: ::std::vec::Vec<SubResource>,
    ///An array of references to outbound rules that use this frontend IP.
    #[serde(
        rename = "outboundRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub outbound_rules: ::std::vec::Vec<SubResource>,
    ///The private IP address of the IP configuration.
    #[serde(
        rename = "privateIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "privateIPAddressVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address_version: ::std::option::Option<IpVersion>,
    #[serde(
        rename = "privateIPAllocationMethod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_allocation_method: ::std::option::Option<IpAllocationMethod>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        rename = "publicIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_address: ::std::option::Option<PublicIpAddress>,
    #[serde(
        rename = "publicIPPrefix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_prefix: ::std::option::Option<SubResource>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<Subnet>,
}
impl ::std::convert::From<&FrontendIpConfigurationPropertiesFormat>
    for FrontendIpConfigurationPropertiesFormat
{
    fn from(value: &FrontendIpConfigurationPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for FrontendIpConfigurationPropertiesFormat {
    fn default() -> Self {
        Self {
            gateway_load_balancer: Default::default(),
            inbound_nat_pools: Default::default(),
            inbound_nat_rules: Default::default(),
            load_balancing_rules: Default::default(),
            outbound_rules: Default::default(),
            private_ip_address: Default::default(),
            private_ip_address_version: Default::default(),
            private_ip_allocation_method: Default::default(),
            provisioning_state: Default::default(),
            public_ip_address: Default::default(),
            public_ip_prefix: Default::default(),
            subnet: Default::default(),
        }
    }
}
///Gateway load balancer tunnel interface of a load balancer backend address pool.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gateway load balancer tunnel interface of a load balancer backend address pool.",
///  "properties": {
///    "identifier": {
///      "description": "Identifier of gateway load balancer tunnel interface.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "port": {
///      "description": "Port of gateway load balancer tunnel interface.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "protocol": {
///      "description": "Protocol of gateway load balancer tunnel interface.",
///      "type": "string",
///      "enum": [
///        "None",
///        "Native",
///        "VXLAN"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "GatewayLoadBalancerTunnelProtocol"
///      }
///    },
///    "type": {
///      "description": "Traffic type of gateway load balancer tunnel interface.",
///      "type": "string",
///      "enum": [
///        "None",
///        "Internal",
///        "External"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "GatewayLoadBalancerTunnelInterfaceType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GatewayLoadBalancerTunnelInterface {
    ///Identifier of gateway load balancer tunnel interface.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identifier: ::std::option::Option<i32>,
    ///Port of gateway load balancer tunnel interface.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub port: ::std::option::Option<i32>,
    ///Protocol of gateway load balancer tunnel interface.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub protocol: ::std::option::Option<GatewayLoadBalancerTunnelInterfaceProtocol>,
    ///Traffic type of gateway load balancer tunnel interface.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<GatewayLoadBalancerTunnelInterfaceType>,
}
impl ::std::convert::From<&GatewayLoadBalancerTunnelInterface>
    for GatewayLoadBalancerTunnelInterface
{
    fn from(value: &GatewayLoadBalancerTunnelInterface) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GatewayLoadBalancerTunnelInterface {
    fn default() -> Self {
        Self {
            identifier: Default::default(),
            port: Default::default(),
            protocol: Default::default(),
            type_: Default::default(),
        }
    }
}
///Protocol of gateway load balancer tunnel interface.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Protocol of gateway load balancer tunnel interface.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Native",
///    "VXLAN"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "GatewayLoadBalancerTunnelProtocol"
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
pub enum GatewayLoadBalancerTunnelInterfaceProtocol {
    None,
    Native,
    #[serde(rename = "VXLAN")]
    Vxlan,
}
impl ::std::convert::From<&Self> for GatewayLoadBalancerTunnelInterfaceProtocol {
    fn from(value: &GatewayLoadBalancerTunnelInterfaceProtocol) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for GatewayLoadBalancerTunnelInterfaceProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Native => f.write_str("Native"),
            Self::Vxlan => f.write_str("VXLAN"),
        }
    }
}
impl ::std::str::FromStr for GatewayLoadBalancerTunnelInterfaceProtocol {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "native" => Ok(Self::Native),
            "vxlan" => Ok(Self::Vxlan),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for GatewayLoadBalancerTunnelInterfaceProtocol {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for GatewayLoadBalancerTunnelInterfaceProtocol
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GatewayLoadBalancerTunnelInterfaceProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Traffic type of gateway load balancer tunnel interface.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Traffic type of gateway load balancer tunnel interface.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Internal",
///    "External"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "GatewayLoadBalancerTunnelInterfaceType"
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
pub enum GatewayLoadBalancerTunnelInterfaceType {
    None,
    Internal,
    External,
}
impl ::std::convert::From<&Self> for GatewayLoadBalancerTunnelInterfaceType {
    fn from(value: &GatewayLoadBalancerTunnelInterfaceType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for GatewayLoadBalancerTunnelInterfaceType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Internal => f.write_str("Internal"),
            Self::External => f.write_str("External"),
        }
    }
}
impl ::std::str::FromStr for GatewayLoadBalancerTunnelInterfaceType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "internal" => Ok(Self::Internal),
            "external" => Ok(Self::External),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for GatewayLoadBalancerTunnelInterfaceType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GatewayLoadBalancerTunnelInterfaceType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GatewayLoadBalancerTunnelInterfaceType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Inbound NAT rule of the load balancer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Inbound NAT rule of the load balancer.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within the set of inbound NAT rules used by the load balancer. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/InboundNatRulePropertiesFormat"
///    },
///    "type": {
///      "description": "Type of the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct InboundNatRule {
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
    ///The name of the resource that is unique within the set of inbound NAT rules used by the load balancer. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<InboundNatRulePropertiesFormat>,
    ///Type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&InboundNatRule> for InboundNatRule {
    fn from(value: &InboundNatRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for InboundNatRule {
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
///Properties of the inbound NAT rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the inbound NAT rule.",
///  "properties": {
///    "backendAddressPool": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "backendIPConfiguration": {
///      "$ref": "#/components/schemas/NetworkInterfaceIPConfiguration"
///    },
///    "backendPort": {
///      "description": "The port used for the internal endpoint. Acceptable values range from 1 to 65535.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "enableFloatingIP": {
///      "description": "Configures a virtual machine's endpoint for the floating IP capability required to configure a SQL AlwaysOn Availability Group. This setting is required when using the SQL AlwaysOn Availability Groups in SQL server. This setting can't be changed after you create the endpoint.",
///      "type": "boolean"
///    },
///    "enableTcpReset": {
///      "description": "Receive bidirectional TCP Reset on TCP flow idle timeout or unexpected connection termination. This element is only used when the protocol is set to TCP.",
///      "type": "boolean"
///    },
///    "frontendIPConfiguration": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "frontendPort": {
///      "description": "The port for the external endpoint. Port numbers for each rule must be unique within the Load Balancer. Acceptable values range from 1 to 65534.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "frontendPortRangeEnd": {
///      "description": "The port range end for the external endpoint. This property is used together with BackendAddressPool and FrontendPortRangeStart. Individual inbound NAT rule port mappings will be created for each backend address from BackendAddressPool. Acceptable values range from 1 to 65534.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "frontendPortRangeStart": {
///      "description": "The port range start for the external endpoint. This property is used together with BackendAddressPool and FrontendPortRangeEnd. Individual inbound NAT rule port mappings will be created for each backend address from BackendAddressPool. Acceptable values range from 1 to 65534.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "idleTimeoutInMinutes": {
///      "description": "The timeout for the TCP idle connection. The value can be set between 4 and 30 minutes. The default value is 4 minutes. This element is only used when the protocol is set to TCP.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "protocol": {
///      "$ref": "#/components/schemas/TransportProtocol"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct InboundNatRulePropertiesFormat {
    #[serde(
        rename = "backendAddressPool",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_address_pool: ::std::option::Option<SubResource>,
    #[serde(
        rename = "backendIPConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_ip_configuration: ::std::option::Option<NetworkInterfaceIpConfiguration>,
    ///The port used for the internal endpoint. Acceptable values range from 1 to 65535.
    #[serde(
        rename = "backendPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_port: ::std::option::Option<i32>,
    ///Configures a virtual machine's endpoint for the floating IP capability required to configure a SQL AlwaysOn Availability Group. This setting is required when using the SQL AlwaysOn Availability Groups in SQL server. This setting can't be changed after you create the endpoint.
    #[serde(
        rename = "enableFloatingIP",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_floating_ip: ::std::option::Option<bool>,
    ///Receive bidirectional TCP Reset on TCP flow idle timeout or unexpected connection termination. This element is only used when the protocol is set to TCP.
    #[serde(
        rename = "enableTcpReset",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_tcp_reset: ::std::option::Option<bool>,
    #[serde(
        rename = "frontendIPConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub frontend_ip_configuration: ::std::option::Option<SubResource>,
    ///The port for the external endpoint. Port numbers for each rule must be unique within the Load Balancer. Acceptable values range from 1 to 65534.
    #[serde(
        rename = "frontendPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub frontend_port: ::std::option::Option<i32>,
    ///The port range end for the external endpoint. This property is used together with BackendAddressPool and FrontendPortRangeStart. Individual inbound NAT rule port mappings will be created for each backend address from BackendAddressPool. Acceptable values range from 1 to 65534.
    #[serde(
        rename = "frontendPortRangeEnd",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub frontend_port_range_end: ::std::option::Option<i32>,
    ///The port range start for the external endpoint. This property is used together with BackendAddressPool and FrontendPortRangeEnd. Individual inbound NAT rule port mappings will be created for each backend address from BackendAddressPool. Acceptable values range from 1 to 65534.
    #[serde(
        rename = "frontendPortRangeStart",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub frontend_port_range_start: ::std::option::Option<i32>,
    ///The timeout for the TCP idle connection. The value can be set between 4 and 30 minutes. The default value is 4 minutes. This element is only used when the protocol is set to TCP.
    #[serde(
        rename = "idleTimeoutInMinutes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub idle_timeout_in_minutes: ::std::option::Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub protocol: ::std::option::Option<TransportProtocol>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
}
impl ::std::convert::From<&InboundNatRulePropertiesFormat> for InboundNatRulePropertiesFormat {
    fn from(value: &InboundNatRulePropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for InboundNatRulePropertiesFormat {
    fn default() -> Self {
        Self {
            backend_address_pool: Default::default(),
            backend_ip_configuration: Default::default(),
            backend_port: Default::default(),
            enable_floating_ip: Default::default(),
            enable_tcp_reset: Default::default(),
            frontend_ip_configuration: Default::default(),
            frontend_port: Default::default(),
            frontend_port_range_end: Default::default(),
            frontend_port_range_start: Default::default(),
            idle_timeout_in_minutes: Default::default(),
            protocol: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///IP address allocation method.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP address allocation method.",
///  "type": "string",
///  "enum": [
///    "Static",
///    "Dynamic"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IPAllocationMethod"
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
pub enum IpAllocationMethod {
    Static,
    Dynamic,
}
impl ::std::convert::From<&Self> for IpAllocationMethod {
    fn from(value: &IpAllocationMethod) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IpAllocationMethod {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Static => f.write_str("Static"),
            Self::Dynamic => f.write_str("Dynamic"),
        }
    }
}
impl ::std::str::FromStr for IpAllocationMethod {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "static" => Ok(Self::Static),
            "dynamic" => Ok(Self::Dynamic),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IpAllocationMethod {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IpAllocationMethod {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IpAllocationMethod {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///IP configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP configuration.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/IPConfigurationPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpConfiguration {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<IpConfigurationPropertiesFormat>,
}
impl ::std::convert::From<&IpConfiguration> for IpConfiguration {
    fn from(value: &IpConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpConfiguration {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
        }
    }
}
///IP configuration profile child resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP configuration profile child resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/IPConfigurationProfilePropertiesFormat"
///    },
///    "type": {
///      "description": "Sub Resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpConfigurationProfile {
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
    ///The name of the resource. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<IpConfigurationProfilePropertiesFormat>,
    ///Sub Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IpConfigurationProfile> for IpConfigurationProfile {
    fn from(value: &IpConfigurationProfile) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpConfigurationProfile {
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
///IP configuration profile properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP configuration profile properties.",
///  "properties": {
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/Subnet"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpConfigurationProfilePropertiesFormat {
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<Subnet>,
}
impl ::std::convert::From<&IpConfigurationProfilePropertiesFormat>
    for IpConfigurationProfilePropertiesFormat
{
    fn from(value: &IpConfigurationProfilePropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpConfigurationProfilePropertiesFormat {
    fn default() -> Self {
        Self {
            provisioning_state: Default::default(),
            subnet: Default::default(),
        }
    }
}
///Properties of IP configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of IP configuration.",
///  "properties": {
///    "privateIPAddress": {
///      "description": "The private IP address of the IP configuration.",
///      "type": "string"
///    },
///    "privateIPAllocationMethod": {
///      "$ref": "#/components/schemas/IPAllocationMethod"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "publicIPAddress": {
///      "$ref": "#/components/schemas/PublicIPAddress"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/Subnet"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpConfigurationPropertiesFormat {
    ///The private IP address of the IP configuration.
    #[serde(
        rename = "privateIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "privateIPAllocationMethod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_allocation_method: ::std::option::Option<IpAllocationMethod>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        rename = "publicIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_address: ::std::option::Option<PublicIpAddress>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<Subnet>,
}
impl ::std::convert::From<&IpConfigurationPropertiesFormat> for IpConfigurationPropertiesFormat {
    fn from(value: &IpConfigurationPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpConfigurationPropertiesFormat {
    fn default() -> Self {
        Self {
            private_ip_address: Default::default(),
            private_ip_allocation_method: Default::default(),
            provisioning_state: Default::default(),
            public_ip_address: Default::default(),
            subnet: Default::default(),
        }
    }
}
///Contains the IpTag associated with the object.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Contains the IpTag associated with the object.",
///  "properties": {
///    "ipTagType": {
///      "description": "The IP tag type. Example: FirstPartyUsage.",
///      "type": "string"
///    },
///    "tag": {
///      "description": "The value of the IP tag associated with the public IP. Example: SQL.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpTag {
    ///The IP tag type. Example: FirstPartyUsage.
    #[serde(
        rename = "ipTagType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_tag_type: ::std::option::Option<::std::string::String>,
    ///The value of the IP tag associated with the public IP. Example: SQL.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IpTag> for IpTag {
    fn from(value: &IpTag) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpTag {
    fn default() -> Self {
        Self {
            ip_tag_type: Default::default(),
            tag: Default::default(),
        }
    }
}
///IP address version.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP address version.",
///  "type": "string",
///  "enum": [
///    "IPv4",
///    "IPv6"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IPVersion"
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
pub enum IpVersion {
    IPv4,
    IPv6,
}
impl ::std::convert::From<&Self> for IpVersion {
    fn from(value: &IpVersion) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IpVersion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::IPv4 => f.write_str("IPv4"),
            Self::IPv6 => f.write_str("IPv6"),
        }
    }
}
impl ::std::str::FromStr for IpVersion {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "ipv4" => Ok(Self::IPv4),
            "ipv6" => Ok(Self::IPv6),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IpVersion {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IpVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IpVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///IpamPool prefix allocation reference.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IpamPool prefix allocation reference.",
///  "type": "object",
///  "properties": {
///    "allocatedAddressPrefixes": {
///      "description": "List of assigned IP address prefixes in the IpamPool of the associated resource.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "numberOfIpAddresses": {
///      "description": "Number of IP addresses to allocate.",
///      "type": "string"
///    },
///    "pool": {
///      "type": "object",
///      "properties": {
///        "id": {
///          "description": "Resource id of the associated Azure IpamPool resource.",
///          "type": "string",
///          "format": "arm-id"
///        }
///      },
///      "x-ms-client-flatten": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpamPoolPrefixAllocation {
    ///List of assigned IP address prefixes in the IpamPool of the associated resource.
    #[serde(
        rename = "allocatedAddressPrefixes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allocated_address_prefixes: ::std::vec::Vec<::std::string::String>,
    ///Number of IP addresses to allocate.
    #[serde(
        rename = "numberOfIpAddresses",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub number_of_ip_addresses: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub pool: ::std::option::Option<IpamPoolPrefixAllocationPool>,
}
impl ::std::convert::From<&IpamPoolPrefixAllocation> for IpamPoolPrefixAllocation {
    fn from(value: &IpamPoolPrefixAllocation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpamPoolPrefixAllocation {
    fn default() -> Self {
        Self {
            allocated_address_prefixes: Default::default(),
            number_of_ip_addresses: Default::default(),
            pool: Default::default(),
        }
    }
}
///`IpamPoolPrefixAllocationPool`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Resource id of the associated Azure IpamPool resource.",
///      "type": "string",
///      "format": "arm-id"
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpamPoolPrefixAllocationPool {
    ///Resource id of the associated Azure IpamPool resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IpamPoolPrefixAllocationPool> for IpamPoolPrefixAllocationPool {
    fn from(value: &IpamPoolPrefixAllocationPool) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IpamPoolPrefixAllocationPool {
    fn default() -> Self {
        Self {
            id: Default::default(),
        }
    }
}
///Load balancer backend addresses.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Load balancer backend addresses.",
///  "properties": {
///    "name": {
///      "description": "Name of the backend address.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/LoadBalancerBackendAddressPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LoadBalancerBackendAddress {
    ///Name of the backend address.
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
    pub properties: ::std::option::Option<LoadBalancerBackendAddressPropertiesFormat>,
}
impl ::std::convert::From<&LoadBalancerBackendAddress> for LoadBalancerBackendAddress {
    fn from(value: &LoadBalancerBackendAddress) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LoadBalancerBackendAddress {
    fn default() -> Self {
        Self {
            name: Default::default(),
            properties: Default::default(),
        }
    }
}
///Properties of the load balancer backend addresses.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the load balancer backend addresses.",
///  "properties": {
///    "adminState": {
///      "description": "A list of administrative states which once set can override health probe so that Load Balancer will always forward new connections to backend, or deny new connections and reset existing connections.",
///      "type": "string",
///      "enum": [
///        "None",
///        "Up",
///        "Down"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LoadBalancerBackendAddressAdminState"
///      }
///    },
///    "inboundNatRulesPortMapping": {
///      "description": "Collection of inbound NAT rule port mappings.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NatRulePortMapping"
///      }
///    },
///    "ipAddress": {
///      "description": "IP Address belonging to the referenced virtual network.",
///      "type": "string",
///      "x-ms-azure-resource": false
///    },
///    "loadBalancerFrontendIPConfiguration": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "networkInterfaceIPConfiguration": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "virtualNetwork": {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LoadBalancerBackendAddressPropertiesFormat {
    ///A list of administrative states which once set can override health probe so that Load Balancer will always forward new connections to backend, or deny new connections and reset existing connections.
    #[serde(
        rename = "adminState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub admin_state: ::std::option::Option<LoadBalancerBackendAddressPropertiesFormatAdminState>,
    ///Collection of inbound NAT rule port mappings.
    #[serde(
        rename = "inboundNatRulesPortMapping",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub inbound_nat_rules_port_mapping: ::std::vec::Vec<NatRulePortMapping>,
    ///IP Address belonging to the referenced virtual network.
    #[serde(
        rename = "ipAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_address: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "loadBalancerFrontendIPConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancer_frontend_ip_configuration: ::std::option::Option<SubResource>,
    #[serde(
        rename = "networkInterfaceIPConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_interface_ip_configuration: ::std::option::Option<SubResource>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<SubResource>,
    #[serde(
        rename = "virtualNetwork",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_network: ::std::option::Option<SubResource>,
}
impl ::std::convert::From<&LoadBalancerBackendAddressPropertiesFormat>
    for LoadBalancerBackendAddressPropertiesFormat
{
    fn from(value: &LoadBalancerBackendAddressPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LoadBalancerBackendAddressPropertiesFormat {
    fn default() -> Self {
        Self {
            admin_state: Default::default(),
            inbound_nat_rules_port_mapping: Default::default(),
            ip_address: Default::default(),
            load_balancer_frontend_ip_configuration: Default::default(),
            network_interface_ip_configuration: Default::default(),
            subnet: Default::default(),
            virtual_network: Default::default(),
        }
    }
}
///A list of administrative states which once set can override health probe so that Load Balancer will always forward new connections to backend, or deny new connections and reset existing connections.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A list of administrative states which once set can override health probe so that Load Balancer will always forward new connections to backend, or deny new connections and reset existing connections.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Up",
///    "Down"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LoadBalancerBackendAddressAdminState"
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
pub enum LoadBalancerBackendAddressPropertiesFormatAdminState {
    None,
    Up,
    Down,
}
impl ::std::convert::From<&Self> for LoadBalancerBackendAddressPropertiesFormatAdminState {
    fn from(value: &LoadBalancerBackendAddressPropertiesFormatAdminState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for LoadBalancerBackendAddressPropertiesFormatAdminState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Up => f.write_str("Up"),
            Self::Down => f.write_str("Down"),
        }
    }
}
impl ::std::str::FromStr for LoadBalancerBackendAddressPropertiesFormatAdminState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for LoadBalancerBackendAddressPropertiesFormatAdminState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for LoadBalancerBackendAddressPropertiesFormatAdminState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for LoadBalancerBackendAddressPropertiesFormatAdminState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Identity for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Identity for the resource.",
///  "properties": {
///    "principalId": {
///      "description": "The principal id of the system assigned identity. This property will only be provided for a system assigned identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "The tenant id of the system assigned identity. This property will only be provided for a system assigned identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of identity used for the resource. The type 'SystemAssigned, UserAssigned' includes both an implicitly created identity and a set of user assigned identities. The type 'None' will remove any identities from the virtual machine.",
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
///      "description": "The list of user identities associated with resource. The user identity dictionary key references will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "object",
///        "properties": {
///          "clientId": {
///            "description": "The client id of user assigned identity.",
///            "readOnly": true,
///            "type": "string"
///          },
///          "principalId": {
///            "description": "The principal id of user assigned identity.",
///            "readOnly": true,
///            "type": "string"
///          }
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedServiceIdentity {
    ///The principal id of the system assigned identity. This property will only be provided for a system assigned identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
    ///The tenant id of the system assigned identity. This property will only be provided for a system assigned identity.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    ///The type of identity used for the resource. The type 'SystemAssigned, UserAssigned' includes both an implicitly created identity and a set of user assigned identities. The type 'None' will remove any identities from the virtual machine.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<ManagedServiceIdentityType>,
    ///The list of user identities associated with resource. The user identity dictionary key references will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities: ::std::collections::HashMap<
        ::std::string::String,
        ManagedServiceIdentityUserAssignedIdentitiesValue,
    >,
}
impl ::std::convert::From<&ManagedServiceIdentity> for ManagedServiceIdentity {
    fn from(value: &ManagedServiceIdentity) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedServiceIdentity {
    fn default() -> Self {
        Self {
            principal_id: Default::default(),
            tenant_id: Default::default(),
            type_: Default::default(),
            user_assigned_identities: Default::default(),
        }
    }
}
///The type of identity used for the resource. The type 'SystemAssigned, UserAssigned' includes both an implicitly created identity and a set of user assigned identities. The type 'None' will remove any identities from the virtual machine.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of identity used for the resource. The type 'SystemAssigned, UserAssigned' includes both an implicitly created identity and a set of user assigned identities. The type 'None' will remove any identities from the virtual machine.",
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
pub enum ManagedServiceIdentityType {
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned, UserAssigned")]
    SystemAssignedUserAssigned,
    None,
}
impl ::std::convert::From<&Self> for ManagedServiceIdentityType {
    fn from(value: &ManagedServiceIdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ManagedServiceIdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => f.write_str("SystemAssigned, UserAssigned"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for ManagedServiceIdentityType {
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
impl ::std::convert::TryFrom<&str> for ManagedServiceIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ManagedServiceIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ManagedServiceIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`ManagedServiceIdentityUserAssignedIdentitiesValue`
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
pub struct ManagedServiceIdentityUserAssignedIdentitiesValue {
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
impl ::std::convert::From<&ManagedServiceIdentityUserAssignedIdentitiesValue>
    for ManagedServiceIdentityUserAssignedIdentitiesValue
{
    fn from(value: &ManagedServiceIdentityUserAssignedIdentitiesValue) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedServiceIdentityUserAssignedIdentitiesValue {
    fn default() -> Self {
        Self {
            client_id: Default::default(),
            principal_id: Default::default(),
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
        Self {
            name: Default::default(),
        }
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
    PartialOrd,
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
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "standardv2" => Ok(Self::StandardV2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NatGatewaySkuName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Individual port mappings for inbound NAT rule created for backend pool.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Individual port mappings for inbound NAT rule created for backend pool.",
///  "type": "object",
///  "properties": {
///    "backendPort": {
///      "description": "Backend port.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "frontendPort": {
///      "description": "Frontend port.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "inboundNatRuleName": {
///      "description": "Name of inbound NAT rule.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NatRulePortMapping {
    ///Backend port.
    #[serde(
        rename = "backendPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub backend_port: ::std::option::Option<i32>,
    ///Frontend port.
    #[serde(
        rename = "frontendPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub frontend_port: ::std::option::Option<i32>,
    ///Name of inbound NAT rule.
    #[serde(
        rename = "inboundNatRuleName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub inbound_nat_rule_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&NatRulePortMapping> for NatRulePortMapping {
    fn from(value: &NatRulePortMapping) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NatRulePortMapping {
    fn default() -> Self {
        Self {
            backend_port: Default::default(),
            frontend_port: Default::default(),
            inbound_nat_rule_name: Default::default(),
        }
    }
}
///A network interface in a resource group.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A network interface in a resource group.",
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
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/NetworkInterfacePropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterface {
    ///A unique read-only string that changes whenever the resource is updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
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
    pub properties: ::std::option::Option<NetworkInterfacePropertiesFormat>,
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
impl ::std::convert::From<&NetworkInterface> for NetworkInterface {
    fn from(value: &NetworkInterface) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterface {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            extended_location: Default::default(),
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///DNS settings of a network interface.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "DNS settings of a network interface.",
///  "properties": {
///    "appliedDnsServers": {
///      "description": "If the VM that uses this NIC is part of an Availability Set, then this list will have the union of all DNS servers from all NICs that are part of the Availability Set. This property is what is configured on each of those VMs.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "dnsServers": {
///      "description": "List of DNS servers IP addresses. Use 'AzureProvidedDNS' to switch to azure provided DNS resolution. 'AzureProvidedDNS' value cannot be combined with other IPs, it must be the only value in dnsServers collection.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "internalDnsNameLabel": {
///      "description": "Relative DNS name for this NIC used for internal communications between VMs in the same virtual network.",
///      "type": "string"
///    },
///    "internalDomainNameSuffix": {
///      "description": "Even if internalDnsNameLabel is not specified, a DNS entry is created for the primary NIC of the VM. This DNS name can be constructed by concatenating the VM name with the value of internalDomainNameSuffix.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "internalFqdn": {
///      "description": "Fully qualified DNS name supporting internal communications between VMs in the same virtual network.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfaceDnsSettings {
    ///If the VM that uses this NIC is part of an Availability Set, then this list will have the union of all DNS servers from all NICs that are part of the Availability Set. This property is what is configured on each of those VMs.
    #[serde(
        rename = "appliedDnsServers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub applied_dns_servers: ::std::vec::Vec<::std::string::String>,
    ///List of DNS servers IP addresses. Use 'AzureProvidedDNS' to switch to azure provided DNS resolution. 'AzureProvidedDNS' value cannot be combined with other IPs, it must be the only value in dnsServers collection.
    #[serde(
        rename = "dnsServers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_servers: ::std::vec::Vec<::std::string::String>,
    ///Relative DNS name for this NIC used for internal communications between VMs in the same virtual network.
    #[serde(
        rename = "internalDnsNameLabel",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub internal_dns_name_label: ::std::option::Option<::std::string::String>,
    ///Even if internalDnsNameLabel is not specified, a DNS entry is created for the primary NIC of the VM. This DNS name can be constructed by concatenating the VM name with the value of internalDomainNameSuffix.
    #[serde(
        rename = "internalDomainNameSuffix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub internal_domain_name_suffix: ::std::option::Option<::std::string::String>,
    ///Fully qualified DNS name supporting internal communications between VMs in the same virtual network.
    #[serde(
        rename = "internalFqdn",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub internal_fqdn: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&NetworkInterfaceDnsSettings> for NetworkInterfaceDnsSettings {
    fn from(value: &NetworkInterfaceDnsSettings) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfaceDnsSettings {
    fn default() -> Self {
        Self {
            applied_dns_servers: Default::default(),
            dns_servers: Default::default(),
            internal_dns_name_label: Default::default(),
            internal_domain_name_suffix: Default::default(),
            internal_fqdn: Default::default(),
        }
    }
}
///IPConfiguration in a network interface.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IPConfiguration in a network interface.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/NetworkInterfaceIPConfigurationPropertiesFormat"
///    },
///    "type": {
///      "description": "Resource type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfaceIpConfiguration {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<NetworkInterfaceIpConfigurationPropertiesFormat>,
    ///Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&NetworkInterfaceIpConfiguration> for NetworkInterfaceIpConfiguration {
    fn from(value: &NetworkInterfaceIpConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfaceIpConfiguration {
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
///PrivateLinkConnection properties for the network interface.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "PrivateLinkConnection properties for the network interface.",
///  "properties": {
///    "fqdns": {
///      "description": "List of FQDNs for current private link connection.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "groupId": {
///      "description": "The group ID for current private link connection.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "requiredMemberName": {
///      "description": "The required member name for current private link connection.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfaceIpConfigurationPrivateLinkConnectionProperties {
    ///List of FQDNs for current private link connection.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fqdns: ::std::vec::Vec<::std::string::String>,
    ///The group ID for current private link connection.
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<::std::string::String>,
    ///The required member name for current private link connection.
    #[serde(
        rename = "requiredMemberName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_member_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&NetworkInterfaceIpConfigurationPrivateLinkConnectionProperties>
    for NetworkInterfaceIpConfigurationPrivateLinkConnectionProperties
{
    fn from(value: &NetworkInterfaceIpConfigurationPrivateLinkConnectionProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfaceIpConfigurationPrivateLinkConnectionProperties {
    fn default() -> Self {
        Self {
            fqdns: Default::default(),
            group_id: Default::default(),
            required_member_name: Default::default(),
        }
    }
}
///Properties of IP configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of IP configuration.",
///  "properties": {
///    "applicationGatewayBackendAddressPools": {
///      "description": "The reference to ApplicationGatewayBackendAddressPool resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationGatewayBackendAddressPool"
///      }
///    },
///    "applicationSecurityGroups": {
///      "description": "Application security groups in which the IP configuration is included.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationSecurityGroup"
///      }
///    },
///    "gatewayLoadBalancer": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "loadBalancerBackendAddressPools": {
///      "description": "The reference to LoadBalancerBackendAddressPool resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/BackendAddressPool"
///      }
///    },
///    "loadBalancerInboundNatRules": {
///      "description": "A list of references of LoadBalancerInboundNatRules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/InboundNatRule"
///      }
///    },
///    "primary": {
///      "description": "Whether this is a primary customer address on the network interface.",
///      "type": "boolean"
///    },
///    "privateIPAddress": {
///      "description": "Private IP address of the IP configuration. It can be a single IP address or a CIDR block in the format <address>/<prefix-length>.",
///      "type": "string"
///    },
///    "privateIPAddressPrefixLength": {
///      "description": "The private IP address prefix length. If specified and the allocation method is dynamic, the service will allocate a CIDR block instead of a single IP address.",
///      "type": [
///        "integer",
///        "null"
///      ],
///      "format": "int32",
///      "maximum": 128.0,
///      "minimum": 1.0
///    },
///    "privateIPAddressVersion": {
///      "$ref": "#/components/schemas/IPVersion"
///    },
///    "privateIPAllocationMethod": {
///      "$ref": "#/components/schemas/IPAllocationMethod"
///    },
///    "privateLinkConnectionProperties": {
///      "$ref": "#/components/schemas/NetworkInterfaceIPConfigurationPrivateLinkConnectionProperties"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "publicIPAddress": {
///      "$ref": "#/components/schemas/PublicIPAddress"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/Subnet"
///    },
///    "virtualNetworkTaps": {
///      "description": "The reference to Virtual Network Taps.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/VirtualNetworkTap"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfaceIpConfigurationPropertiesFormat {
    ///The reference to ApplicationGatewayBackendAddressPool resource.
    #[serde(
        rename = "applicationGatewayBackendAddressPools",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub application_gateway_backend_address_pools:
        ::std::vec::Vec<ApplicationGatewayBackendAddressPool>,
    ///Application security groups in which the IP configuration is included.
    #[serde(
        rename = "applicationSecurityGroups",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub application_security_groups: ::std::vec::Vec<ApplicationSecurityGroup>,
    #[serde(
        rename = "gatewayLoadBalancer",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub gateway_load_balancer: ::std::option::Option<SubResource>,
    ///The reference to LoadBalancerBackendAddressPool resource.
    #[serde(
        rename = "loadBalancerBackendAddressPools",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancer_backend_address_pools: ::std::vec::Vec<BackendAddressPool>,
    ///A list of references of LoadBalancerInboundNatRules.
    #[serde(
        rename = "loadBalancerInboundNatRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancer_inbound_nat_rules: ::std::vec::Vec<InboundNatRule>,
    ///Whether this is a primary customer address on the network interface.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub primary: ::std::option::Option<bool>,
    ///Private IP address of the IP configuration. It can be a single IP address or a CIDR block in the format <address>/<prefix-length>.
    #[serde(
        rename = "privateIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address: ::std::option::Option<::std::string::String>,
    ///The private IP address prefix length. If specified and the allocation method is dynamic, the service will allocate a CIDR block instead of a single IP address.
    #[serde(
        rename = "privateIPAddressPrefixLength",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address_prefix_length: ::std::option::Option<::std::num::NonZeroU32>,
    #[serde(
        rename = "privateIPAddressVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address_version: ::std::option::Option<IpVersion>,
    #[serde(
        rename = "privateIPAllocationMethod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_allocation_method: ::std::option::Option<IpAllocationMethod>,
    #[serde(
        rename = "privateLinkConnectionProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_connection_properties:
        ::std::option::Option<NetworkInterfaceIpConfigurationPrivateLinkConnectionProperties>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        rename = "publicIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_address: ::std::option::Option<PublicIpAddress>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<Subnet>,
    ///The reference to Virtual Network Taps.
    #[serde(
        rename = "virtualNetworkTaps",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_network_taps: ::std::vec::Vec<VirtualNetworkTap>,
}
impl ::std::convert::From<&NetworkInterfaceIpConfigurationPropertiesFormat>
    for NetworkInterfaceIpConfigurationPropertiesFormat
{
    fn from(value: &NetworkInterfaceIpConfigurationPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfaceIpConfigurationPropertiesFormat {
    fn default() -> Self {
        Self {
            application_gateway_backend_address_pools: Default::default(),
            application_security_groups: Default::default(),
            gateway_load_balancer: Default::default(),
            load_balancer_backend_address_pools: Default::default(),
            load_balancer_inbound_nat_rules: Default::default(),
            primary: Default::default(),
            private_ip_address: Default::default(),
            private_ip_address_prefix_length: Default::default(),
            private_ip_address_version: Default::default(),
            private_ip_allocation_method: Default::default(),
            private_link_connection_properties: Default::default(),
            provisioning_state: Default::default(),
            public_ip_address: Default::default(),
            subnet: Default::default(),
            virtual_network_taps: Default::default(),
        }
    }
}
///NetworkInterface properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "NetworkInterface properties.",
///  "properties": {
///    "auxiliaryMode": {
///      "description": "Auxiliary mode of Network Interface resource.",
///      "type": "string",
///      "enum": [
///        "None",
///        "MaxConnections",
///        "Floating",
///        "AcceleratedConnections"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NetworkInterfaceAuxiliaryMode"
///      }
///    },
///    "auxiliarySku": {
///      "description": "Auxiliary sku of Network Interface resource.",
///      "type": "string",
///      "enum": [
///        "None",
///        "A1",
///        "A2",
///        "A4",
///        "A8"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NetworkInterfaceAuxiliarySku"
///      }
///    },
///    "defaultOutboundConnectivityEnabled": {
///      "description": "Whether default outbound connectivity for nic was configured or not.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "disableTcpStateTracking": {
///      "description": "Indicates whether to disable tcp state tracking.",
///      "type": "boolean"
///    },
///    "dnsSettings": {
///      "$ref": "#/components/schemas/NetworkInterfaceDnsSettings"
///    },
///    "dscpConfiguration": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "enableAcceleratedNetworking": {
///      "description": "If the network interface is configured for accelerated networking. Not applicable to VM sizes which require accelerated networking.",
///      "type": "boolean"
///    },
///    "enableIPForwarding": {
///      "description": "Indicates whether IP forwarding is enabled on this network interface.",
///      "type": "boolean"
///    },
///    "hostedWorkloads": {
///      "description": "A list of references to linked BareMetal resources.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "ipConfigurations": {
///      "description": "A list of IPConfigurations of the network interface.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterfaceIPConfiguration"
///      }
///    },
///    "macAddress": {
///      "description": "The MAC address of the network interface.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "migrationPhase": {
///      "description": "Migration phase of Network Interface resource.",
///      "type": "string",
///      "enum": [
///        "None",
///        "Prepare",
///        "Commit",
///        "Abort",
///        "Committed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NetworkInterfaceMigrationPhase"
///      }
///    },
///    "networkSecurityGroup": {
///      "$ref": "#/components/schemas/NetworkSecurityGroup"
///    },
///    "nicType": {
///      "description": "Type of Network Interface resource.",
///      "type": "string",
///      "enum": [
///        "Standard",
///        "Elastic"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "NetworkInterfaceNicType"
///      }
///    },
///    "primary": {
///      "description": "Whether this is a primary network interface on a virtual machine.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "privateEndpoint": {
///      "$ref": "#/components/schemas/PrivateEndpoint"
///    },
///    "privateLinkService": {
///      "$ref": "#/components/schemas/PrivateLinkService"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the network interface resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tapConfigurations": {
///      "description": "A list of TapConfigurations of the network interface.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterfaceTapConfiguration"
///      }
///    },
///    "virtualMachine": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "vnetEncryptionSupported": {
///      "description": "Whether the virtual machine this nic is attached to supports encryption.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "workloadType": {
///      "description": "WorkloadType of the NetworkInterface for BareMetal resources",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfacePropertiesFormat {
    ///Auxiliary mode of Network Interface resource.
    #[serde(
        rename = "auxiliaryMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auxiliary_mode: ::std::option::Option<NetworkInterfacePropertiesFormatAuxiliaryMode>,
    ///Auxiliary sku of Network Interface resource.
    #[serde(
        rename = "auxiliarySku",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auxiliary_sku: ::std::option::Option<NetworkInterfacePropertiesFormatAuxiliarySku>,
    ///Whether default outbound connectivity for nic was configured or not.
    #[serde(
        rename = "defaultOutboundConnectivityEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_outbound_connectivity_enabled: ::std::option::Option<bool>,
    ///Indicates whether to disable tcp state tracking.
    #[serde(
        rename = "disableTcpStateTracking",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disable_tcp_state_tracking: ::std::option::Option<bool>,
    #[serde(
        rename = "dnsSettings",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_settings: ::std::option::Option<NetworkInterfaceDnsSettings>,
    #[serde(
        rename = "dscpConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dscp_configuration: ::std::option::Option<SubResource>,
    ///If the network interface is configured for accelerated networking. Not applicable to VM sizes which require accelerated networking.
    #[serde(
        rename = "enableAcceleratedNetworking",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_accelerated_networking: ::std::option::Option<bool>,
    ///Indicates whether IP forwarding is enabled on this network interface.
    #[serde(
        rename = "enableIPForwarding",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_ip_forwarding: ::std::option::Option<bool>,
    ///A list of references to linked BareMetal resources.
    #[serde(
        rename = "hostedWorkloads",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub hosted_workloads: ::std::vec::Vec<::std::string::String>,
    ///A list of IPConfigurations of the network interface.
    #[serde(
        rename = "ipConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_configurations: ::std::vec::Vec<NetworkInterfaceIpConfiguration>,
    ///The MAC address of the network interface.
    #[serde(
        rename = "macAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub mac_address: ::std::option::Option<::std::string::String>,
    ///Migration phase of Network Interface resource.
    #[serde(
        rename = "migrationPhase",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub migration_phase: ::std::option::Option<NetworkInterfacePropertiesFormatMigrationPhase>,
    #[serde(
        rename = "networkSecurityGroup",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_security_group: ::std::option::Option<NetworkSecurityGroup>,
    ///Type of Network Interface resource.
    #[serde(
        rename = "nicType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub nic_type: ::std::option::Option<NetworkInterfacePropertiesFormatNicType>,
    ///Whether this is a primary network interface on a virtual machine.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub primary: ::std::option::Option<bool>,
    #[serde(
        rename = "privateEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint: ::std::option::Option<PrivateEndpoint>,
    #[serde(
        rename = "privateLinkService",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_service: ::std::option::Option<PrivateLinkService>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The resource GUID property of the network interface resource.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
    ///A list of TapConfigurations of the network interface.
    #[serde(
        rename = "tapConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tap_configurations: ::std::vec::Vec<NetworkInterfaceTapConfiguration>,
    #[serde(
        rename = "virtualMachine",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_machine: ::std::option::Option<SubResource>,
    ///Whether the virtual machine this nic is attached to supports encryption.
    #[serde(
        rename = "vnetEncryptionSupported",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub vnet_encryption_supported: ::std::option::Option<bool>,
    ///WorkloadType of the NetworkInterface for BareMetal resources
    #[serde(
        rename = "workloadType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workload_type: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&NetworkInterfacePropertiesFormat> for NetworkInterfacePropertiesFormat {
    fn from(value: &NetworkInterfacePropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfacePropertiesFormat {
    fn default() -> Self {
        Self {
            auxiliary_mode: Default::default(),
            auxiliary_sku: Default::default(),
            default_outbound_connectivity_enabled: Default::default(),
            disable_tcp_state_tracking: Default::default(),
            dns_settings: Default::default(),
            dscp_configuration: Default::default(),
            enable_accelerated_networking: Default::default(),
            enable_ip_forwarding: Default::default(),
            hosted_workloads: Default::default(),
            ip_configurations: Default::default(),
            mac_address: Default::default(),
            migration_phase: Default::default(),
            network_security_group: Default::default(),
            nic_type: Default::default(),
            primary: Default::default(),
            private_endpoint: Default::default(),
            private_link_service: Default::default(),
            provisioning_state: Default::default(),
            resource_guid: Default::default(),
            tap_configurations: Default::default(),
            virtual_machine: Default::default(),
            vnet_encryption_supported: Default::default(),
            workload_type: Default::default(),
        }
    }
}
///Auxiliary mode of Network Interface resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Auxiliary mode of Network Interface resource.",
///  "type": "string",
///  "enum": [
///    "None",
///    "MaxConnections",
///    "Floating",
///    "AcceleratedConnections"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkInterfaceAuxiliaryMode"
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
pub enum NetworkInterfacePropertiesFormatAuxiliaryMode {
    None,
    MaxConnections,
    Floating,
    AcceleratedConnections,
}
impl ::std::convert::From<&Self> for NetworkInterfacePropertiesFormatAuxiliaryMode {
    fn from(value: &NetworkInterfacePropertiesFormatAuxiliaryMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkInterfacePropertiesFormatAuxiliaryMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::MaxConnections => f.write_str("MaxConnections"),
            Self::Floating => f.write_str("Floating"),
            Self::AcceleratedConnections => f.write_str("AcceleratedConnections"),
        }
    }
}
impl ::std::str::FromStr for NetworkInterfacePropertiesFormatAuxiliaryMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "maxconnections" => Ok(Self::MaxConnections),
            "floating" => Ok(Self::Floating),
            "acceleratedconnections" => Ok(Self::AcceleratedConnections),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkInterfacePropertiesFormatAuxiliaryMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for NetworkInterfacePropertiesFormatAuxiliaryMode
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for NetworkInterfacePropertiesFormatAuxiliaryMode
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Auxiliary sku of Network Interface resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Auxiliary sku of Network Interface resource.",
///  "type": "string",
///  "enum": [
///    "None",
///    "A1",
///    "A2",
///    "A4",
///    "A8"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkInterfaceAuxiliarySku"
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
pub enum NetworkInterfacePropertiesFormatAuxiliarySku {
    None,
    A1,
    A2,
    A4,
    A8,
}
impl ::std::convert::From<&Self> for NetworkInterfacePropertiesFormatAuxiliarySku {
    fn from(value: &NetworkInterfacePropertiesFormatAuxiliarySku) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkInterfacePropertiesFormatAuxiliarySku {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::A1 => f.write_str("A1"),
            Self::A2 => f.write_str("A2"),
            Self::A4 => f.write_str("A4"),
            Self::A8 => f.write_str("A8"),
        }
    }
}
impl ::std::str::FromStr for NetworkInterfacePropertiesFormatAuxiliarySku {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "a1" => Ok(Self::A1),
            "a2" => Ok(Self::A2),
            "a4" => Ok(Self::A4),
            "a8" => Ok(Self::A8),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkInterfacePropertiesFormatAuxiliarySku {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for NetworkInterfacePropertiesFormatAuxiliarySku
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for NetworkInterfacePropertiesFormatAuxiliarySku
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Migration phase of Network Interface resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Migration phase of Network Interface resource.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Prepare",
///    "Commit",
///    "Abort",
///    "Committed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkInterfaceMigrationPhase"
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
pub enum NetworkInterfacePropertiesFormatMigrationPhase {
    None,
    Prepare,
    Commit,
    Abort,
    Committed,
}
impl ::std::convert::From<&Self> for NetworkInterfacePropertiesFormatMigrationPhase {
    fn from(value: &NetworkInterfacePropertiesFormatMigrationPhase) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkInterfacePropertiesFormatMigrationPhase {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Prepare => f.write_str("Prepare"),
            Self::Commit => f.write_str("Commit"),
            Self::Abort => f.write_str("Abort"),
            Self::Committed => f.write_str("Committed"),
        }
    }
}
impl ::std::str::FromStr for NetworkInterfacePropertiesFormatMigrationPhase {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "prepare" => Ok(Self::Prepare),
            "commit" => Ok(Self::Commit),
            "abort" => Ok(Self::Abort),
            "committed" => Ok(Self::Committed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkInterfacePropertiesFormatMigrationPhase {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for NetworkInterfacePropertiesFormatMigrationPhase
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for NetworkInterfacePropertiesFormatMigrationPhase
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Type of Network Interface resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Type of Network Interface resource.",
///  "type": "string",
///  "enum": [
///    "Standard",
///    "Elastic"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkInterfaceNicType"
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
pub enum NetworkInterfacePropertiesFormatNicType {
    Standard,
    Elastic,
}
impl ::std::convert::From<&Self> for NetworkInterfacePropertiesFormatNicType {
    fn from(value: &NetworkInterfacePropertiesFormatNicType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkInterfacePropertiesFormatNicType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::Elastic => f.write_str("Elastic"),
        }
    }
}
impl ::std::str::FromStr for NetworkInterfacePropertiesFormatNicType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "elastic" => Ok(Self::Elastic),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkInterfacePropertiesFormatNicType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkInterfacePropertiesFormatNicType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkInterfacePropertiesFormatNicType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Tap configuration in a Network Interface.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tap configuration in a Network Interface.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/NetworkInterfaceTapConfigurationPropertiesFormat"
///    },
///    "type": {
///      "description": "Sub Resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfaceTapConfiguration {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<NetworkInterfaceTapConfigurationPropertiesFormat>,
    ///Sub Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&NetworkInterfaceTapConfiguration> for NetworkInterfaceTapConfiguration {
    fn from(value: &NetworkInterfaceTapConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfaceTapConfiguration {
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
///Properties of Virtual Network Tap configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of Virtual Network Tap configuration.",
///  "properties": {
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "virtualNetworkTap": {
///      "$ref": "#/components/schemas/VirtualNetworkTap"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkInterfaceTapConfigurationPropertiesFormat {
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        rename = "virtualNetworkTap",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_network_tap: ::std::option::Option<VirtualNetworkTap>,
}
impl ::std::convert::From<&NetworkInterfaceTapConfigurationPropertiesFormat>
    for NetworkInterfaceTapConfigurationPropertiesFormat
{
    fn from(value: &NetworkInterfaceTapConfigurationPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkInterfaceTapConfigurationPropertiesFormat {
    fn default() -> Self {
        Self {
            provisioning_state: Default::default(),
            virtual_network_tap: Default::default(),
        }
    }
}
///NetworkSecurityGroup resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "NetworkSecurityGroup resource.",
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
///      "$ref": "#/components/schemas/NetworkSecurityGroupPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkSecurityGroup {
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
    pub properties: ::std::option::Option<NetworkSecurityGroupPropertiesFormat>,
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
impl ::std::convert::From<&NetworkSecurityGroup> for NetworkSecurityGroup {
    fn from(value: &NetworkSecurityGroup) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkSecurityGroup {
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
///Network Security Group resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Network Security Group resource.",
///  "properties": {
///    "defaultSecurityRules": {
///      "description": "The default security rules of network security group.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SecurityRule"
///      }
///    },
///    "flowLogs": {
///      "description": "A collection of references to flow log resources.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/FlowLog"
///      }
///    },
///    "flushConnection": {
///      "description": "When enabled, flows created from Network Security Group connections will be re-evaluated when rules are updates. Initial enablement will trigger re-evaluation.",
///      "type": "boolean"
///    },
///    "networkInterfaces": {
///      "description": "A collection of references to network interfaces.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterface"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the network security group resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "securityRules": {
///      "description": "A collection of security rules of the network security group.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SecurityRule"
///      }
///    },
///    "subnets": {
///      "description": "A collection of references to subnets.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Subnet"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkSecurityGroupPropertiesFormat {
    ///The default security rules of network security group.
    #[serde(
        rename = "defaultSecurityRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_security_rules: ::std::vec::Vec<SecurityRule>,
    ///A collection of references to flow log resources.
    #[serde(
        rename = "flowLogs",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub flow_logs: ::std::vec::Vec<FlowLog>,
    ///When enabled, flows created from Network Security Group connections will be re-evaluated when rules are updates. Initial enablement will trigger re-evaluation.
    #[serde(
        rename = "flushConnection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub flush_connection: ::std::option::Option<bool>,
    ///A collection of references to network interfaces.
    #[serde(
        rename = "networkInterfaces",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_interfaces: ::std::vec::Vec<NetworkInterface>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The resource GUID property of the network security group resource.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
    ///A collection of security rules of the network security group.
    #[serde(
        rename = "securityRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_rules: ::std::vec::Vec<SecurityRule>,
    ///A collection of references to subnets.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnets: ::std::vec::Vec<Subnet>,
}
impl ::std::convert::From<&NetworkSecurityGroupPropertiesFormat>
    for NetworkSecurityGroupPropertiesFormat
{
    fn from(value: &NetworkSecurityGroupPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for NetworkSecurityGroupPropertiesFormat {
    fn default() -> Self {
        Self {
            default_security_rules: Default::default(),
            flow_logs: Default::default(),
            flush_connection: Default::default(),
            network_interfaces: Default::default(),
            provisioning_state: Default::default(),
            resource_guid: Default::default(),
            security_rules: Default::default(),
            subnets: Default::default(),
        }
    }
}
///Private endpoint resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Private endpoint resource.",
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
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpoint {
    ///A unique read-only string that changes whenever the resource is updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
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
    pub properties: ::std::option::Option<PrivateEndpointProperties>,
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
impl ::std::convert::From<&PrivateEndpoint> for PrivateEndpoint {
    fn from(value: &PrivateEndpoint) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpoint {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            extended_location: Default::default(),
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///PrivateEndpointConnection resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "PrivateEndpointConnection resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProperties"
///    },
///    "type": {
///      "description": "The resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnection {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    ///The resource type.
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
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Properties of the PrivateEndpointConnectProperties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the PrivateEndpointConnectProperties.",
///  "properties": {
///    "linkIdentifier": {
///      "description": "The consumer link id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "privateEndpoint": {
///      "$ref": "#/components/schemas/PrivateEndpoint"
///    },
///    "privateEndpointLocation": {
///      "description": "The location of the private endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "privateLinkServiceConnectionState": {
///      "$ref": "#/components/schemas/PrivateLinkServiceConnectionState"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnectionProperties {
    ///The consumer link id.
    #[serde(
        rename = "linkIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub link_identifier: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "privateEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint: ::std::option::Option<PrivateEndpoint>,
    ///The location of the private endpoint.
    #[serde(
        rename = "privateEndpointLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_location: ::std::option::Option<::std::string::String>,
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
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
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
            link_identifier: Default::default(),
            private_endpoint: Default::default(),
            private_endpoint_location: Default::default(),
            private_link_service_connection_state: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///An IP Configuration of the private endpoint.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An IP Configuration of the private endpoint.",
///  "type": "object",
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointIPConfigurationProperties"
///    },
///    "type": {
///      "description": "The resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointIpConfiguration {
    ///A unique read-only string that changes whenever the resource is updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    ///The name of the resource that is unique within a resource group.
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
    pub properties: ::std::option::Option<PrivateEndpointIpConfigurationProperties>,
    ///The resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateEndpointIpConfiguration> for PrivateEndpointIpConfiguration {
    fn from(value: &PrivateEndpointIpConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointIpConfiguration {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Properties of an IP Configuration of the private endpoint.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of an IP Configuration of the private endpoint.",
///  "type": "object",
///  "properties": {
///    "groupId": {
///      "description": "The ID of a group obtained from the remote resource that this private endpoint should connect to.",
///      "type": "string"
///    },
///    "memberName": {
///      "description": "The member name of a group obtained from the remote resource that this private endpoint should connect to.",
///      "type": "string"
///    },
///    "privateIPAddress": {
///      "description": "A private ip address obtained from the private endpoint's subnet.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointIpConfigurationProperties {
    ///The ID of a group obtained from the remote resource that this private endpoint should connect to.
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<::std::string::String>,
    ///The member name of a group obtained from the remote resource that this private endpoint should connect to.
    #[serde(
        rename = "memberName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub member_name: ::std::option::Option<::std::string::String>,
    ///A private ip address obtained from the private endpoint's subnet.
    #[serde(
        rename = "privateIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateEndpointIpConfigurationProperties>
    for PrivateEndpointIpConfigurationProperties
{
    fn from(value: &PrivateEndpointIpConfigurationProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointIpConfigurationProperties {
    fn default() -> Self {
        Self {
            group_id: Default::default(),
            member_name: Default::default(),
            private_ip_address: Default::default(),
        }
    }
}
///Properties of the private endpoint.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the private endpoint.",
///  "properties": {
///    "applicationSecurityGroups": {
///      "description": "Application security groups in which the private endpoint IP configuration is included.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationSecurityGroup"
///      }
///    },
///    "customDnsConfigs": {
///      "description": "An array of custom dns configurations.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CustomDnsConfigPropertiesFormat"
///      }
///    },
///    "customNetworkInterfaceName": {
///      "description": "The custom name of the network interface attached to the private endpoint.",
///      "type": "string"
///    },
///    "ipConfigurations": {
///      "description": "A list of IP configurations of the private endpoint. This will be used to map to the First Party Service's endpoints.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointIPConfiguration"
///      }
///    },
///    "ipVersionType": {
///      "description": "Specifies the IP version type for the private IPs of the private endpoint. If not defined, this defaults to IPv4.",
///      "default": "IPv4",
///      "type": "string",
///      "enum": [
///        "IPv4",
///        "IPv6",
///        "DualStack"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PrivateEndpointIPVersionType",
///        "values": [
///          {
///            "description": "Indicates that the Private IPs of the private endpoint will be IPv4 only.",
///            "name": "IPv4",
///            "value": "IPv4"
///          },
///          {
///            "description": "Indicates that the Private IPs of the private endpoint will be IPv6 only.",
///            "name": "IPv6",
///            "value": "IPv6"
///          },
///          {
///            "description": "Indicates that the Private IPs of the private endpoint can be both IPv4 and IPv6.",
///            "name": "DualStack",
///            "value": "DualStack"
///          }
///        ]
///      }
///    },
///    "manualPrivateLinkServiceConnections": {
///      "description": "A grouping of information about the connection to the remote resource. Used when the network admin does not have access to approve connections to the remote resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateLinkServiceConnection"
///      }
///    },
///    "networkInterfaces": {
///      "description": "An array of references to the network interfaces created for this private endpoint.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterface"
///      }
///    },
///    "privateLinkServiceConnections": {
///      "description": "A grouping of information about the connection to the remote resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateLinkServiceConnection"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/Subnet"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointProperties {
    ///Application security groups in which the private endpoint IP configuration is included.
    #[serde(
        rename = "applicationSecurityGroups",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub application_security_groups: ::std::vec::Vec<ApplicationSecurityGroup>,
    ///An array of custom dns configurations.
    #[serde(
        rename = "customDnsConfigs",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_dns_configs: ::std::vec::Vec<CustomDnsConfigPropertiesFormat>,
    ///The custom name of the network interface attached to the private endpoint.
    #[serde(
        rename = "customNetworkInterfaceName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_network_interface_name: ::std::option::Option<::std::string::String>,
    ///A list of IP configurations of the private endpoint. This will be used to map to the First Party Service's endpoints.
    #[serde(
        rename = "ipConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_configurations: ::std::vec::Vec<PrivateEndpointIpConfiguration>,
    ///Specifies the IP version type for the private IPs of the private endpoint. If not defined, this defaults to IPv4.
    #[serde(
        rename = "ipVersionType",
        default = "defaults::private_endpoint_properties_ip_version_type",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_version_type: PrivateEndpointPropertiesIpVersionType,
    ///A grouping of information about the connection to the remote resource. Used when the network admin does not have access to approve connections to the remote resource.
    #[serde(
        rename = "manualPrivateLinkServiceConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub manual_private_link_service_connections: ::std::vec::Vec<PrivateLinkServiceConnection>,
    ///An array of references to the network interfaces created for this private endpoint.
    #[serde(
        rename = "networkInterfaces",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_interfaces: ::std::vec::Vec<NetworkInterface>,
    ///A grouping of information about the connection to the remote resource.
    #[serde(
        rename = "privateLinkServiceConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_service_connections: ::std::vec::Vec<PrivateLinkServiceConnection>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<Subnet>,
}
impl ::std::convert::From<&PrivateEndpointProperties> for PrivateEndpointProperties {
    fn from(value: &PrivateEndpointProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointProperties {
    fn default() -> Self {
        Self {
            application_security_groups: Default::default(),
            custom_dns_configs: Default::default(),
            custom_network_interface_name: Default::default(),
            ip_configurations: Default::default(),
            ip_version_type: defaults::private_endpoint_properties_ip_version_type(),
            manual_private_link_service_connections: Default::default(),
            network_interfaces: Default::default(),
            private_link_service_connections: Default::default(),
            provisioning_state: Default::default(),
            subnet: Default::default(),
        }
    }
}
///Specifies the IP version type for the private IPs of the private endpoint. If not defined, this defaults to IPv4.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies the IP version type for the private IPs of the private endpoint. If not defined, this defaults to IPv4.",
///  "default": "IPv4",
///  "type": "string",
///  "enum": [
///    "IPv4",
///    "IPv6",
///    "DualStack"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateEndpointIPVersionType",
///    "values": [
///      {
///        "description": "Indicates that the Private IPs of the private endpoint will be IPv4 only.",
///        "name": "IPv4",
///        "value": "IPv4"
///      },
///      {
///        "description": "Indicates that the Private IPs of the private endpoint will be IPv6 only.",
///        "name": "IPv6",
///        "value": "IPv6"
///      },
///      {
///        "description": "Indicates that the Private IPs of the private endpoint can be both IPv4 and IPv6.",
///        "name": "DualStack",
///        "value": "DualStack"
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
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum PrivateEndpointPropertiesIpVersionType {
    IPv4,
    IPv6,
    DualStack,
}
impl ::std::convert::From<&Self> for PrivateEndpointPropertiesIpVersionType {
    fn from(value: &PrivateEndpointPropertiesIpVersionType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateEndpointPropertiesIpVersionType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::IPv4 => f.write_str("IPv4"),
            Self::IPv6 => f.write_str("IPv6"),
            Self::DualStack => f.write_str("DualStack"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointPropertiesIpVersionType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "ipv4" => Ok(Self::IPv4),
            "ipv6" => Ok(Self::IPv6),
            "dualstack" => Ok(Self::DualStack),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateEndpointPropertiesIpVersionType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PrivateEndpointPropertiesIpVersionType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateEndpointPropertiesIpVersionType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for PrivateEndpointPropertiesIpVersionType {
    fn default() -> Self {
        PrivateEndpointPropertiesIpVersionType::IPv4
    }
}
///Private link service resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Private link service resource.",
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
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkServiceProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkService {
    ///A unique read-only string that changes whenever the resource is updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
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
    pub properties: ::std::option::Option<PrivateLinkServiceProperties>,
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
impl ::std::convert::From<&PrivateLinkService> for PrivateLinkService {
    fn from(value: &PrivateLinkService) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkService {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            extended_location: Default::default(),
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///PrivateLinkServiceConnection resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "PrivateLinkServiceConnection resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkServiceConnectionProperties"
///    },
///    "type": {
///      "description": "The resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkServiceConnection {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<PrivateLinkServiceConnectionProperties>,
    ///The resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkServiceConnection> for PrivateLinkServiceConnection {
    fn from(value: &PrivateLinkServiceConnection) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkServiceConnection {
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
///Properties of the PrivateLinkServiceConnection.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the PrivateLinkServiceConnection.",
///  "properties": {
///    "groupIds": {
///      "description": "The ID(s) of the group(s) obtained from the remote resource that this private endpoint should connect to.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "privateLinkServiceConnectionState": {
///      "$ref": "#/components/schemas/PrivateLinkServiceConnectionState"
///    },
///    "privateLinkServiceId": {
///      "description": "The resource id of private link service.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "requestMessage": {
///      "description": "A message passed to the owner of the remote resource with this connection request. Restricted to 140 chars.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkServiceConnectionProperties {
    ///The ID(s) of the group(s) obtained from the remote resource that this private endpoint should connect to.
    #[serde(
        rename = "groupIds",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_ids: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "privateLinkServiceConnectionState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_service_connection_state:
        ::std::option::Option<PrivateLinkServiceConnectionState>,
    ///The resource id of private link service.
    #[serde(
        rename = "privateLinkServiceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_service_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///A message passed to the owner of the remote resource with this connection request. Restricted to 140 chars.
    #[serde(
        rename = "requestMessage",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub request_message: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkServiceConnectionProperties>
    for PrivateLinkServiceConnectionProperties
{
    fn from(value: &PrivateLinkServiceConnectionProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkServiceConnectionProperties {
    fn default() -> Self {
        Self {
            group_ids: Default::default(),
            private_link_service_connection_state: Default::default(),
            private_link_service_id: Default::default(),
            provisioning_state: Default::default(),
            request_message: Default::default(),
        }
    }
}
///A collection of information about the state of the connection between service consumer and provider.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A collection of information about the state of the connection between service consumer and provider.",
///  "properties": {
///    "actionsRequired": {
///      "description": "A message indicating if changes on the service provider require any updates on the consumer.",
///      "type": "string"
///    },
///    "description": {
///      "description": "The reason for approval/rejection of the connection.",
///      "type": "string"
///    },
///    "status": {
///      "description": "Indicates whether the connection has been Approved/Rejected/Removed by the owner of the service.",
///      "type": "string"
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
    pub actions_required: ::std::option::Option<::std::string::String>,
    ///The reason for approval/rejection of the connection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Indicates whether the connection has been Approved/Rejected/Removed by the owner of the service.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<::std::string::String>,
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
///The private link service ip configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The private link service ip configuration.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of private link service ip configuration.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkServiceIpConfigurationProperties"
///    },
///    "type": {
///      "description": "The resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkServiceIpConfiguration {
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
    ///The name of private link service ip configuration.
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
    pub properties: ::std::option::Option<PrivateLinkServiceIpConfigurationProperties>,
    ///The resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkServiceIpConfiguration>
    for PrivateLinkServiceIpConfiguration
{
    fn from(value: &PrivateLinkServiceIpConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkServiceIpConfiguration {
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
///Properties of private link service IP configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of private link service IP configuration.",
///  "properties": {
///    "primary": {
///      "description": "Whether the ip configuration is primary or not.",
///      "type": "boolean"
///    },
///    "privateIPAddress": {
///      "description": "The private IP address of the IP configuration.",
///      "type": "string"
///    },
///    "privateIPAddressVersion": {
///      "$ref": "#/components/schemas/IPVersion"
///    },
///    "privateIPAllocationMethod": {
///      "$ref": "#/components/schemas/IPAllocationMethod"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "subnet": {
///      "$ref": "#/components/schemas/Subnet"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkServiceIpConfigurationProperties {
    ///Whether the ip configuration is primary or not.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub primary: ::std::option::Option<bool>,
    ///The private IP address of the IP configuration.
    #[serde(
        rename = "privateIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "privateIPAddressVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_address_version: ::std::option::Option<IpVersion>,
    #[serde(
        rename = "privateIPAllocationMethod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_ip_allocation_method: ::std::option::Option<IpAllocationMethod>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnet: ::std::option::Option<Subnet>,
}
impl ::std::convert::From<&PrivateLinkServiceIpConfigurationProperties>
    for PrivateLinkServiceIpConfigurationProperties
{
    fn from(value: &PrivateLinkServiceIpConfigurationProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkServiceIpConfigurationProperties {
    fn default() -> Self {
        Self {
            primary: Default::default(),
            private_ip_address: Default::default(),
            private_ip_address_version: Default::default(),
            private_ip_allocation_method: Default::default(),
            provisioning_state: Default::default(),
            subnet: Default::default(),
        }
    }
}
///Properties of the private link service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the private link service.",
///  "properties": {
///    "accessMode": {
///      "description": "The access mode of the private link service.",
///      "type": "string",
///      "enum": [
///        "Default",
///        "Restricted"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AccessMode",
///        "values": [
///          {
///            "description": "Allows unrestricted access to the private link service.",
///            "name": "Default",
///            "value": "Default"
///          },
///          {
///            "description": "Limits access to subscriptions which are inside visibility list only.",
///            "name": "Restricted",
///            "value": "Restricted"
///          }
///        ]
///      }
///    },
///    "alias": {
///      "description": "The alias of the private link service.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "autoApproval": {
///      "description": "The auto-approval list of the private link service.",
///      "allOf": [
///        {
///          "$ref": "#/components/schemas/ResourceSet"
///        }
///      ]
///    },
///    "destinationIPAddress": {
///      "description": "The destination IP address of the private link service.",
///      "type": "string"
///    },
///    "enableProxyProtocol": {
///      "description": "Whether the private link service is enabled for proxy protocol or not.",
///      "type": "boolean"
///    },
///    "fqdns": {
///      "description": "The list of Fqdn.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "ipConfigurations": {
///      "description": "An array of private link service IP configurations.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateLinkServiceIpConfiguration"
///      }
///    },
///    "loadBalancerFrontendIpConfigurations": {
///      "description": "An array of references to the load balancer IP configurations.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/FrontendIPConfiguration"
///      }
///    },
///    "networkInterfaces": {
///      "description": "An array of references to the network interfaces created for this private link service.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterface"
///      }
///    },
///    "privateEndpointConnections": {
///      "description": "An array of list about connections to the private endpoint.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "visibility": {
///      "description": "The visibility list of the private link service.",
///      "allOf": [
///        {
///          "$ref": "#/components/schemas/ResourceSet"
///        }
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkServiceProperties {
    ///The access mode of the private link service.
    #[serde(
        rename = "accessMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_mode: ::std::option::Option<PrivateLinkServicePropertiesAccessMode>,
    ///The alias of the private link service.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub alias: ::std::option::Option<::std::string::String>,
    ///The auto-approval list of the private link service.
    #[serde(
        rename = "autoApproval",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auto_approval: ::std::option::Option<ResourceSet>,
    ///The destination IP address of the private link service.
    #[serde(
        rename = "destinationIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_ip_address: ::std::option::Option<::std::string::String>,
    ///Whether the private link service is enabled for proxy protocol or not.
    #[serde(
        rename = "enableProxyProtocol",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_proxy_protocol: ::std::option::Option<bool>,
    ///The list of Fqdn.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fqdns: ::std::vec::Vec<::std::string::String>,
    ///An array of private link service IP configurations.
    #[serde(
        rename = "ipConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_configurations: ::std::vec::Vec<PrivateLinkServiceIpConfiguration>,
    ///An array of references to the load balancer IP configurations.
    #[serde(
        rename = "loadBalancerFrontendIpConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub load_balancer_frontend_ip_configurations: ::std::vec::Vec<FrontendIpConfiguration>,
    ///An array of references to the network interfaces created for this private link service.
    #[serde(
        rename = "networkInterfaces",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_interfaces: ::std::vec::Vec<NetworkInterface>,
    ///An array of list about connections to the private endpoint.
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnection>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The visibility list of the private link service.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub visibility: ::std::option::Option<ResourceSet>,
}
impl ::std::convert::From<&PrivateLinkServiceProperties> for PrivateLinkServiceProperties {
    fn from(value: &PrivateLinkServiceProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkServiceProperties {
    fn default() -> Self {
        Self {
            access_mode: Default::default(),
            alias: Default::default(),
            auto_approval: Default::default(),
            destination_ip_address: Default::default(),
            enable_proxy_protocol: Default::default(),
            fqdns: Default::default(),
            ip_configurations: Default::default(),
            load_balancer_frontend_ip_configurations: Default::default(),
            network_interfaces: Default::default(),
            private_endpoint_connections: Default::default(),
            provisioning_state: Default::default(),
            visibility: Default::default(),
        }
    }
}
///The access mode of the private link service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The access mode of the private link service.",
///  "type": "string",
///  "enum": [
///    "Default",
///    "Restricted"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AccessMode",
///    "values": [
///      {
///        "description": "Allows unrestricted access to the private link service.",
///        "name": "Default",
///        "value": "Default"
///      },
///      {
///        "description": "Limits access to subscriptions which are inside visibility list only.",
///        "name": "Restricted",
///        "value": "Restricted"
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
    PartialOrd,
)]
#[serde(try_from = "String")]
pub enum PrivateLinkServicePropertiesAccessMode {
    Default,
    Restricted,
}
impl ::std::convert::From<&Self> for PrivateLinkServicePropertiesAccessMode {
    fn from(value: &PrivateLinkServicePropertiesAccessMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateLinkServicePropertiesAccessMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Default => f.write_str("Default"),
            Self::Restricted => f.write_str("Restricted"),
        }
    }
}
impl ::std::str::FromStr for PrivateLinkServicePropertiesAccessMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "restricted" => Ok(Self::Restricted),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateLinkServicePropertiesAccessMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PrivateLinkServicePropertiesAccessMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateLinkServicePropertiesAccessMode {
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
    PartialOrd,
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
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Public IP address resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Public IP address resource.",
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
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PublicIPAddressPropertiesFormat"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/PublicIPAddressSku"
///    },
///    "zones": {
///      "description": "A list of availability zones denoting the IP allocated for the resource needs to come from.",
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
pub struct PublicIpAddress {
    ///A unique read-only string that changes whenever the resource is updated.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub etag: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
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
    pub properties: ::std::option::Option<::std::boxed::Box<PublicIpAddressPropertiesFormat>>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<PublicIpAddressSku>,
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
    ///A list of availability zones denoting the IP allocated for the resource needs to come from.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zones: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&PublicIpAddress> for PublicIpAddress {
    fn from(value: &PublicIpAddress) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PublicIpAddress {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            extended_location: Default::default(),
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
///Contains FQDN of the DNS record associated with the public IP address.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Contains FQDN of the DNS record associated with the public IP address.",
///  "properties": {
///    "domainNameLabel": {
///      "description": "The domain name label. The concatenation of the domain name label and the regionalized DNS zone make up the fully qualified domain name associated with the public IP address. If a domain name label is specified, an A DNS record is created for the public IP in the Microsoft Azure DNS system.",
///      "type": "string"
///    },
///    "domainNameLabelScope": {
///      "description": "The domain name label scope. If a domain name label and a domain name label scope are specified, an A DNS record is created for the public IP in the Microsoft Azure DNS system with a hashed value includes in FQDN.",
///      "type": "string",
///      "enum": [
///        "TenantReuse",
///        "SubscriptionReuse",
///        "ResourceGroupReuse",
///        "NoReuse"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "PublicIpAddressDnsSettingsDomainNameLabelScope"
///      }
///    },
///    "fqdn": {
///      "description": "The Fully Qualified Domain Name of the A DNS record associated with the public IP. This is the concatenation of the domainNameLabel and the regionalized DNS zone.",
///      "type": "string"
///    },
///    "reverseFqdn": {
///      "description": "The reverse FQDN. A user-visible, fully qualified domain name that resolves to this public IP address. If the reverseFqdn is specified, then a PTR DNS record is created pointing from the IP address in the in-addr.arpa domain to the reverse FQDN.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PublicIpAddressDnsSettings {
    ///The domain name label. The concatenation of the domain name label and the regionalized DNS zone make up the fully qualified domain name associated with the public IP address. If a domain name label is specified, an A DNS record is created for the public IP in the Microsoft Azure DNS system.
    #[serde(
        rename = "domainNameLabel",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub domain_name_label: ::std::option::Option<::std::string::String>,
    ///The domain name label scope. If a domain name label and a domain name label scope are specified, an A DNS record is created for the public IP in the Microsoft Azure DNS system with a hashed value includes in FQDN.
    #[serde(
        rename = "domainNameLabelScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub domain_name_label_scope:
        ::std::option::Option<PublicIpAddressDnsSettingsDomainNameLabelScope>,
    ///The Fully Qualified Domain Name of the A DNS record associated with the public IP. This is the concatenation of the domainNameLabel and the regionalized DNS zone.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fqdn: ::std::option::Option<::std::string::String>,
    ///The reverse FQDN. A user-visible, fully qualified domain name that resolves to this public IP address. If the reverseFqdn is specified, then a PTR DNS record is created pointing from the IP address in the in-addr.arpa domain to the reverse FQDN.
    #[serde(
        rename = "reverseFqdn",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reverse_fqdn: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PublicIpAddressDnsSettings> for PublicIpAddressDnsSettings {
    fn from(value: &PublicIpAddressDnsSettings) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PublicIpAddressDnsSettings {
    fn default() -> Self {
        Self {
            domain_name_label: Default::default(),
            domain_name_label_scope: Default::default(),
            fqdn: Default::default(),
            reverse_fqdn: Default::default(),
        }
    }
}
///The domain name label scope. If a domain name label and a domain name label scope are specified, an A DNS record is created for the public IP in the Microsoft Azure DNS system with a hashed value includes in FQDN.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The domain name label scope. If a domain name label and a domain name label scope are specified, an A DNS record is created for the public IP in the Microsoft Azure DNS system with a hashed value includes in FQDN.",
///  "type": "string",
///  "enum": [
///    "TenantReuse",
///    "SubscriptionReuse",
///    "ResourceGroupReuse",
///    "NoReuse"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "PublicIpAddressDnsSettingsDomainNameLabelScope"
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
pub enum PublicIpAddressDnsSettingsDomainNameLabelScope {
    TenantReuse,
    SubscriptionReuse,
    ResourceGroupReuse,
    NoReuse,
}
impl ::std::convert::From<&Self> for PublicIpAddressDnsSettingsDomainNameLabelScope {
    fn from(value: &PublicIpAddressDnsSettingsDomainNameLabelScope) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicIpAddressDnsSettingsDomainNameLabelScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::TenantReuse => f.write_str("TenantReuse"),
            Self::SubscriptionReuse => f.write_str("SubscriptionReuse"),
            Self::ResourceGroupReuse => f.write_str("ResourceGroupReuse"),
            Self::NoReuse => f.write_str("NoReuse"),
        }
    }
}
impl ::std::str::FromStr for PublicIpAddressDnsSettingsDomainNameLabelScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "tenantreuse" => Ok(Self::TenantReuse),
            "subscriptionreuse" => Ok(Self::SubscriptionReuse),
            "resourcegroupreuse" => Ok(Self::ResourceGroupReuse),
            "noreuse" => Ok(Self::NoReuse),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicIpAddressDnsSettingsDomainNameLabelScope {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PublicIpAddressDnsSettingsDomainNameLabelScope
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for PublicIpAddressDnsSettingsDomainNameLabelScope
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Response for ListPublicIpAddresses API service call.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Response for ListPublicIpAddresses API service call.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of results.",
///      "type": "string"
///    },
///    "value": {
///      "description": "A list of public IP addresses that exists in a resource group.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PublicIPAddress"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PublicIpAddressListResult {
    ///The URL to get the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A list of public IP addresses that exists in a resource group.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PublicIpAddress>,
}
impl ::std::convert::From<&PublicIpAddressListResult> for PublicIpAddressListResult {
    fn from(value: &PublicIpAddressListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PublicIpAddressListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Public IP address properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Public IP address properties.",
///  "properties": {
///    "ddosSettings": {
///      "$ref": "#/components/schemas/DdosSettings"
///    },
///    "deleteOption": {
///      "description": "Specify what happens to the public IP address when the VM using it is deleted",
///      "type": "string",
///      "enum": [
///        "Delete",
///        "Detach"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DeleteOptions"
///      }
///    },
///    "dnsSettings": {
///      "$ref": "#/components/schemas/PublicIPAddressDnsSettings"
///    },
///    "idleTimeoutInMinutes": {
///      "description": "The idle timeout of the public IP address.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "ipAddress": {
///      "description": "The IP address associated with the public IP address resource.",
///      "type": "string"
///    },
///    "ipConfiguration": {
///      "$ref": "#/components/schemas/IPConfiguration"
///    },
///    "ipTags": {
///      "description": "The list of tags associated with the public IP address.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IpTag"
///      }
///    },
///    "linkedPublicIPAddress": {
///      "$ref": "#/components/schemas/PublicIPAddress"
///    },
///    "migrationPhase": {
///      "description": "Migration phase of Public IP Address.",
///      "type": "string",
///      "enum": [
///        "None",
///        "Prepare",
///        "Commit",
///        "Abort",
///        "Committed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PublicIPAddressMigrationPhase"
///      }
///    },
///    "natGateway": {
///      "$ref": "#/components/schemas/NatGateway"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "publicIPAddressVersion": {
///      "$ref": "#/components/schemas/IPVersion"
///    },
///    "publicIPAllocationMethod": {
///      "$ref": "#/components/schemas/IPAllocationMethod"
///    },
///    "publicIPPrefix": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the public IP address resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "servicePublicIPAddress": {
///      "$ref": "#/components/schemas/PublicIPAddress"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PublicIpAddressPropertiesFormat {
    #[serde(
        rename = "ddosSettings",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ddos_settings: ::std::option::Option<DdosSettings>,
    ///Specify what happens to the public IP address when the VM using it is deleted
    #[serde(
        rename = "deleteOption",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delete_option: ::std::option::Option<PublicIpAddressPropertiesFormatDeleteOption>,
    #[serde(
        rename = "dnsSettings",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_settings: ::std::option::Option<PublicIpAddressDnsSettings>,
    ///The idle timeout of the public IP address.
    #[serde(
        rename = "idleTimeoutInMinutes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub idle_timeout_in_minutes: ::std::option::Option<i32>,
    ///The IP address associated with the public IP address resource.
    #[serde(
        rename = "ipAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_address: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "ipConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_configuration: ::std::option::Option<IpConfiguration>,
    ///The list of tags associated with the public IP address.
    #[serde(
        rename = "ipTags",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_tags: ::std::vec::Vec<IpTag>,
    #[serde(
        rename = "linkedPublicIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub linked_public_ip_address: ::std::option::Option<PublicIpAddress>,
    ///Migration phase of Public IP Address.
    #[serde(
        rename = "migrationPhase",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub migration_phase: ::std::option::Option<PublicIpAddressPropertiesFormatMigrationPhase>,
    #[serde(
        rename = "natGateway",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub nat_gateway: ::std::option::Option<NatGateway>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    #[serde(
        rename = "publicIPAddressVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_address_version: ::std::option::Option<IpVersion>,
    #[serde(
        rename = "publicIPAllocationMethod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_allocation_method: ::std::option::Option<IpAllocationMethod>,
    #[serde(
        rename = "publicIPPrefix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_prefix: ::std::option::Option<SubResource>,
    ///The resource GUID property of the public IP address resource.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "servicePublicIPAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_public_ip_address: ::std::option::Option<PublicIpAddress>,
}
impl ::std::convert::From<&PublicIpAddressPropertiesFormat> for PublicIpAddressPropertiesFormat {
    fn from(value: &PublicIpAddressPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PublicIpAddressPropertiesFormat {
    fn default() -> Self {
        Self {
            ddos_settings: Default::default(),
            delete_option: Default::default(),
            dns_settings: Default::default(),
            idle_timeout_in_minutes: Default::default(),
            ip_address: Default::default(),
            ip_configuration: Default::default(),
            ip_tags: Default::default(),
            linked_public_ip_address: Default::default(),
            migration_phase: Default::default(),
            nat_gateway: Default::default(),
            provisioning_state: Default::default(),
            public_ip_address_version: Default::default(),
            public_ip_allocation_method: Default::default(),
            public_ip_prefix: Default::default(),
            resource_guid: Default::default(),
            service_public_ip_address: Default::default(),
        }
    }
}
///Specify what happens to the public IP address when the VM using it is deleted
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specify what happens to the public IP address when the VM using it is deleted",
///  "type": "string",
///  "enum": [
///    "Delete",
///    "Detach"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DeleteOptions"
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
pub enum PublicIpAddressPropertiesFormatDeleteOption {
    Delete,
    Detach,
}
impl ::std::convert::From<&Self> for PublicIpAddressPropertiesFormatDeleteOption {
    fn from(value: &PublicIpAddressPropertiesFormatDeleteOption) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicIpAddressPropertiesFormatDeleteOption {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Delete => f.write_str("Delete"),
            Self::Detach => f.write_str("Detach"),
        }
    }
}
impl ::std::str::FromStr for PublicIpAddressPropertiesFormatDeleteOption {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "delete" => Ok(Self::Delete),
            "detach" => Ok(Self::Detach),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicIpAddressPropertiesFormatDeleteOption {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PublicIpAddressPropertiesFormatDeleteOption
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for PublicIpAddressPropertiesFormatDeleteOption
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Migration phase of Public IP Address.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Migration phase of Public IP Address.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Prepare",
///    "Commit",
///    "Abort",
///    "Committed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicIPAddressMigrationPhase"
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
pub enum PublicIpAddressPropertiesFormatMigrationPhase {
    None,
    Prepare,
    Commit,
    Abort,
    Committed,
}
impl ::std::convert::From<&Self> for PublicIpAddressPropertiesFormatMigrationPhase {
    fn from(value: &PublicIpAddressPropertiesFormatMigrationPhase) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicIpAddressPropertiesFormatMigrationPhase {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Prepare => f.write_str("Prepare"),
            Self::Commit => f.write_str("Commit"),
            Self::Abort => f.write_str("Abort"),
            Self::Committed => f.write_str("Committed"),
        }
    }
}
impl ::std::str::FromStr for PublicIpAddressPropertiesFormatMigrationPhase {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "prepare" => Ok(Self::Prepare),
            "commit" => Ok(Self::Commit),
            "abort" => Ok(Self::Abort),
            "committed" => Ok(Self::Committed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicIpAddressPropertiesFormatMigrationPhase {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PublicIpAddressPropertiesFormatMigrationPhase
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for PublicIpAddressPropertiesFormatMigrationPhase
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///SKU of a public IP address.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU of a public IP address.",
///  "properties": {
///    "name": {
///      "description": "Name of a public IP address SKU.",
///      "type": "string",
///      "enum": [
///        "Basic",
///        "Standard",
///        "StandardV2"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PublicIPAddressSkuName"
///      }
///    },
///    "tier": {
///      "description": "Tier of a public IP address SKU.",
///      "type": "string",
///      "enum": [
///        "Regional",
///        "Global"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "PublicIPAddressSkuTier"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PublicIpAddressSku {
    ///Name of a public IP address SKU.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<PublicIpAddressSkuName>,
    ///Tier of a public IP address SKU.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<PublicIpAddressSkuTier>,
}
impl ::std::convert::From<&PublicIpAddressSku> for PublicIpAddressSku {
    fn from(value: &PublicIpAddressSku) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PublicIpAddressSku {
    fn default() -> Self {
        Self {
            name: Default::default(),
            tier: Default::default(),
        }
    }
}
///Name of a public IP address SKU.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Name of a public IP address SKU.",
///  "type": "string",
///  "enum": [
///    "Basic",
///    "Standard",
///    "StandardV2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicIPAddressSkuName"
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
pub enum PublicIpAddressSkuName {
    Basic,
    Standard,
    StandardV2,
}
impl ::std::convert::From<&Self> for PublicIpAddressSkuName {
    fn from(value: &PublicIpAddressSkuName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicIpAddressSkuName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Basic => f.write_str("Basic"),
            Self::Standard => f.write_str("Standard"),
            Self::StandardV2 => f.write_str("StandardV2"),
        }
    }
}
impl ::std::str::FromStr for PublicIpAddressSkuName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "basic" => Ok(Self::Basic),
            "standard" => Ok(Self::Standard),
            "standardv2" => Ok(Self::StandardV2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicIpAddressSkuName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PublicIpAddressSkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PublicIpAddressSkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Tier of a public IP address SKU.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tier of a public IP address SKU.",
///  "type": "string",
///  "enum": [
///    "Regional",
///    "Global"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicIPAddressSkuTier"
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
pub enum PublicIpAddressSkuTier {
    Regional,
    Global,
}
impl ::std::convert::From<&Self> for PublicIpAddressSkuTier {
    fn from(value: &PublicIpAddressSkuTier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicIpAddressSkuTier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Regional => f.write_str("Regional"),
            Self::Global => f.write_str("Global"),
        }
    }
}
impl ::std::str::FromStr for PublicIpAddressSkuTier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "regional" => Ok(Self::Regional),
            "global" => Ok(Self::Global),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicIpAddressSkuTier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PublicIpAddressSkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PublicIpAddressSkuTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Response for GetPublicIpAddressDdosProtectionStatusOperation API service call.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Response for GetPublicIpAddressDdosProtectionStatusOperation API service call.",
///  "type": "object",
///  "properties": {
///    "ddosProtectionPlanId": {
///      "description": " DDoS protection plan Resource Id of a if IP address is protected through a plan.",
///      "type": "string"
///    },
///    "isWorkloadProtected": {
///      "description": "Value indicating whether the IP address is DDoS workload protected or not.",
///      "type": "string",
///      "enum": [
///        "False",
///        "True"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "IsWorkloadProtected"
///      }
///    },
///    "publicIpAddress": {
///      "description": "IP Address of the Public IP Resource",
///      "type": "string"
///    },
///    "publicIpAddressId": {
///      "description": "Public IP ARM resource ID",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PublicIpDdosProtectionStatusResult {
    /// DDoS protection plan Resource Id of a if IP address is protected through a plan.
    #[serde(
        rename = "ddosProtectionPlanId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ddos_protection_plan_id: ::std::option::Option<::std::string::String>,
    ///Value indicating whether the IP address is DDoS workload protected or not.
    #[serde(
        rename = "isWorkloadProtected",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_workload_protected:
        ::std::option::Option<PublicIpDdosProtectionStatusResultIsWorkloadProtected>,
    ///IP Address of the Public IP Resource
    #[serde(
        rename = "publicIpAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_address: ::std::option::Option<::std::string::String>,
    ///Public IP ARM resource ID
    #[serde(
        rename = "publicIpAddressId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_ip_address_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PublicIpDdosProtectionStatusResult>
    for PublicIpDdosProtectionStatusResult
{
    fn from(value: &PublicIpDdosProtectionStatusResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PublicIpDdosProtectionStatusResult {
    fn default() -> Self {
        Self {
            ddos_protection_plan_id: Default::default(),
            is_workload_protected: Default::default(),
            public_ip_address: Default::default(),
            public_ip_address_id: Default::default(),
        }
    }
}
///Value indicating whether the IP address is DDoS workload protected or not.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Value indicating whether the IP address is DDoS workload protected or not.",
///  "type": "string",
///  "enum": [
///    "False",
///    "True"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IsWorkloadProtected"
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
pub enum PublicIpDdosProtectionStatusResultIsWorkloadProtected {
    False,
    True,
}
impl ::std::convert::From<&Self> for PublicIpDdosProtectionStatusResultIsWorkloadProtected {
    fn from(value: &PublicIpDdosProtectionStatusResultIsWorkloadProtected) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicIpDdosProtectionStatusResultIsWorkloadProtected {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::False => f.write_str("False"),
            Self::True => f.write_str("True"),
        }
    }
}
impl ::std::str::FromStr for PublicIpDdosProtectionStatusResultIsWorkloadProtected {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "false" => Ok(Self::False),
            "true" => Ok(Self::True),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicIpDdosProtectionStatusResultIsWorkloadProtected {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PublicIpDdosProtectionStatusResultIsWorkloadProtected
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for PublicIpDdosProtectionStatusResultIsWorkloadProtected
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The request for ReserveCloudServicePublicIpAddressOperation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The request for ReserveCloudServicePublicIpAddressOperation.",
///  "type": "object",
///  "required": [
///    "isRollback"
///  ],
///  "properties": {
///    "isRollback": {
///      "description": "When true, reverts from Static to Dynamic allocation (undo reservation).",
///      "type": "string",
///      "enum": [
///        "true",
///        "false"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "IsRollback"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ReserveCloudServicePublicIpAddressRequest {
    ///When true, reverts from Static to Dynamic allocation (undo reservation).
    #[serde(rename = "isRollback")]
    pub is_rollback: ReserveCloudServicePublicIpAddressRequestIsRollback,
}
impl ::std::convert::From<&ReserveCloudServicePublicIpAddressRequest>
    for ReserveCloudServicePublicIpAddressRequest
{
    fn from(value: &ReserveCloudServicePublicIpAddressRequest) -> Self {
        value.clone()
    }
}
///When true, reverts from Static to Dynamic allocation (undo reservation).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "When true, reverts from Static to Dynamic allocation (undo reservation).",
///  "type": "string",
///  "enum": [
///    "true",
///    "false"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IsRollback"
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
pub enum ReserveCloudServicePublicIpAddressRequestIsRollback {
    #[serde(rename = "true")]
    True,
    #[serde(rename = "false")]
    False,
}
impl ::std::convert::From<&Self> for ReserveCloudServicePublicIpAddressRequestIsRollback {
    fn from(value: &ReserveCloudServicePublicIpAddressRequestIsRollback) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ReserveCloudServicePublicIpAddressRequestIsRollback {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::True => f.write_str("true"),
            Self::False => f.write_str("false"),
        }
    }
}
impl ::std::str::FromStr for ReserveCloudServicePublicIpAddressRequestIsRollback {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "true" => Ok(Self::True),
            "false" => Ok(Self::False),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ReserveCloudServicePublicIpAddressRequestIsRollback {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for ReserveCloudServicePublicIpAddressRequestIsRollback
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for ReserveCloudServicePublicIpAddressRequestIsRollback
{
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
///ResourceNavigationLink resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "ResourceNavigationLink resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "id": {
///      "description": "Resource navigation link identifier.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ResourceNavigationLinkFormat"
///    },
///    "type": {
///      "description": "Resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceNavigationLink {
    ///A unique read-only string that changes whenever the resource is updated.
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
    pub id: ::std::option::Option<::std::string::String>,
    ///Name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<ResourceNavigationLinkFormat>,
    ///Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceNavigationLink> for ResourceNavigationLink {
    fn from(value: &ResourceNavigationLink) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceNavigationLink {
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
///Properties of ResourceNavigationLink.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of ResourceNavigationLink.",
///  "properties": {
///    "link": {
///      "description": "Link to the external resource.",
///      "type": "string"
///    },
///    "linkedResourceType": {
///      "description": "Resource type of the linked resource.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceNavigationLinkFormat {
    ///Link to the external resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub link: ::std::option::Option<::std::string::String>,
    ///Resource type of the linked resource.
    #[serde(
        rename = "linkedResourceType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub linked_resource_type: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
}
impl ::std::convert::From<&ResourceNavigationLinkFormat> for ResourceNavigationLinkFormat {
    fn from(value: &ResourceNavigationLinkFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceNavigationLinkFormat {
    fn default() -> Self {
        Self {
            link: Default::default(),
            linked_resource_type: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///The base resource set for visibility and auto-approval.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The base resource set for visibility and auto-approval.",
///  "properties": {
///    "subscriptions": {
///      "description": "The list of subscriptions.",
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
pub struct ResourceSet {
    ///The list of subscriptions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subscriptions: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ResourceSet> for ResourceSet {
    fn from(value: &ResourceSet) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceSet {
    fn default() -> Self {
        Self {
            subscriptions: Default::default(),
        }
    }
}
///Parameters that define the retention policy for flow log.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters that define the retention policy for flow log.",
///  "properties": {
///    "days": {
///      "description": "Number of days to retain flow log records.",
///      "default": 0,
///      "type": "integer",
///      "format": "int32"
///    },
///    "enabled": {
///      "description": "Flag to enable/disable retention.",
///      "default": false,
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RetentionPolicyParameters {
    ///Number of days to retain flow log records.
    #[serde(
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days: i32,
    ///Flag to enable/disable retention.
    #[serde(
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: bool,
}
impl ::std::convert::From<&RetentionPolicyParameters> for RetentionPolicyParameters {
    fn from(value: &RetentionPolicyParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RetentionPolicyParameters {
    fn default() -> Self {
        Self {
            days: Default::default(),
            enabled: Default::default(),
        }
    }
}
///Route resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Route resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RoutePropertiesFormat"
///    },
///    "type": {
///      "description": "The type of the resource.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Route {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<RoutePropertiesFormat>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Route> for Route {
    fn from(value: &Route) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Route {
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
///The type of Azure hop the packet should be sent to.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of Azure hop the packet should be sent to.",
///  "type": "string",
///  "enum": [
///    "VirtualNetworkGateway",
///    "VnetLocal",
///    "Internet",
///    "VirtualAppliance",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "RouteNextHopType"
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
pub enum RouteNextHopType {
    VirtualNetworkGateway,
    VnetLocal,
    Internet,
    VirtualAppliance,
    None,
}
impl ::std::convert::From<&Self> for RouteNextHopType {
    fn from(value: &RouteNextHopType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RouteNextHopType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::VirtualNetworkGateway => f.write_str("VirtualNetworkGateway"),
            Self::VnetLocal => f.write_str("VnetLocal"),
            Self::Internet => f.write_str("Internet"),
            Self::VirtualAppliance => f.write_str("VirtualAppliance"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for RouteNextHopType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "virtualnetworkgateway" => Ok(Self::VirtualNetworkGateway),
            "vnetlocal" => Ok(Self::VnetLocal),
            "internet" => Ok(Self::Internet),
            "virtualappliance" => Ok(Self::VirtualAppliance),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RouteNextHopType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RouteNextHopType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RouteNextHopType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Route resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Route resource.",
///  "required": [
///    "nextHopType"
///  ],
///  "properties": {
///    "addressPrefix": {
///      "description": "The destination CIDR to which the route applies.",
///      "type": "string"
///    },
///    "hasBgpOverride": {
///      "description": "A value indicating whether this route overrides overlapping BGP routes regardless of LPM.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "nextHopIpAddress": {
///      "description": "The IP address packets should be forwarded to. Next hop values are only allowed in routes where the next hop type is VirtualAppliance.",
///      "type": "string"
///    },
///    "nextHopType": {
///      "$ref": "#/components/schemas/RouteNextHopType"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoutePropertiesFormat {
    ///The destination CIDR to which the route applies.
    #[serde(
        rename = "addressPrefix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub address_prefix: ::std::option::Option<::std::string::String>,
    ///A value indicating whether this route overrides overlapping BGP routes regardless of LPM.
    #[serde(
        rename = "hasBgpOverride",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_bgp_override: ::std::option::Option<bool>,
    ///The IP address packets should be forwarded to. Next hop values are only allowed in routes where the next hop type is VirtualAppliance.
    #[serde(
        rename = "nextHopIpAddress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_hop_ip_address: ::std::option::Option<::std::string::String>,
    #[serde(rename = "nextHopType")]
    pub next_hop_type: RouteNextHopType,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
}
impl ::std::convert::From<&RoutePropertiesFormat> for RoutePropertiesFormat {
    fn from(value: &RoutePropertiesFormat) -> Self {
        value.clone()
    }
}
///Route table resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Route table resource.",
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
///      "$ref": "#/components/schemas/RouteTablePropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RouteTable {
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
    pub properties: ::std::option::Option<RouteTablePropertiesFormat>,
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
impl ::std::convert::From<&RouteTable> for RouteTable {
    fn from(value: &RouteTable) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RouteTable {
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
///Route Table resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Route Table resource.",
///  "properties": {
///    "disableBgpRoutePropagation": {
///      "description": "Whether to disable the routes learned by BGP on that route table. True means disable.",
///      "type": "boolean"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the route table.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "routes": {
///      "description": "Collection of routes contained within a route table.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Route"
///      }
///    },
///    "subnets": {
///      "description": "A collection of references to subnets.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Subnet"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RouteTablePropertiesFormat {
    ///Whether to disable the routes learned by BGP on that route table. True means disable.
    #[serde(
        rename = "disableBgpRoutePropagation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disable_bgp_route_propagation: ::std::option::Option<bool>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The resource GUID property of the route table.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
    ///Collection of routes contained within a route table.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub routes: ::std::vec::Vec<Route>,
    ///A collection of references to subnets.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnets: ::std::vec::Vec<Subnet>,
}
impl ::std::convert::From<&RouteTablePropertiesFormat> for RouteTablePropertiesFormat {
    fn from(value: &RouteTablePropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RouteTablePropertiesFormat {
    fn default() -> Self {
        Self {
            disable_bgp_route_propagation: Default::default(),
            provisioning_state: Default::default(),
            resource_guid: Default::default(),
            routes: Default::default(),
            subnets: Default::default(),
        }
    }
}
///Network security rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Network security rule.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/SecurityRulePropertiesFormat"
///    },
///    "type": {
///      "description": "The type of the resource.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecurityRule {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<SecurityRulePropertiesFormat>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SecurityRule> for SecurityRule {
    fn from(value: &SecurityRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecurityRule {
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
///Whether network traffic is allowed or denied.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Whether network traffic is allowed or denied.",
///  "type": "string",
///  "enum": [
///    "Allow",
///    "Deny"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SecurityRuleAccess"
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
pub enum SecurityRuleAccess {
    Allow,
    Deny,
}
impl ::std::convert::From<&Self> for SecurityRuleAccess {
    fn from(value: &SecurityRuleAccess) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SecurityRuleAccess {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
            Self::Deny => f.write_str("Deny"),
        }
    }
}
impl ::std::str::FromStr for SecurityRuleAccess {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SecurityRuleAccess {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SecurityRuleAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SecurityRuleAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The direction of the rule. The direction specifies if rule will be evaluated on incoming or outgoing traffic.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The direction of the rule. The direction specifies if rule will be evaluated on incoming or outgoing traffic.",
///  "type": "string",
///  "enum": [
///    "Inbound",
///    "Outbound"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SecurityRuleDirection"
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
pub enum SecurityRuleDirection {
    Inbound,
    Outbound,
}
impl ::std::convert::From<&Self> for SecurityRuleDirection {
    fn from(value: &SecurityRuleDirection) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SecurityRuleDirection {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Inbound => f.write_str("Inbound"),
            Self::Outbound => f.write_str("Outbound"),
        }
    }
}
impl ::std::str::FromStr for SecurityRuleDirection {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "inbound" => Ok(Self::Inbound),
            "outbound" => Ok(Self::Outbound),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SecurityRuleDirection {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SecurityRuleDirection {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SecurityRuleDirection {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Security rule resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Security rule resource.",
///  "required": [
///    "access",
///    "direction",
///    "priority",
///    "protocol"
///  ],
///  "properties": {
///    "access": {
///      "$ref": "#/components/schemas/SecurityRuleAccess"
///    },
///    "description": {
///      "description": "A description for this rule. Restricted to 140 chars.",
///      "type": "string"
///    },
///    "destinationAddressPrefix": {
///      "description": "The destination address prefix. CIDR or destination IP range. Asterisk '*' can also be used to match all source IPs. Default tags such as 'VirtualNetwork', 'AzureLoadBalancer' and 'Internet' can also be used.",
///      "type": "string"
///    },
///    "destinationAddressPrefixes": {
///      "description": "The destination address prefixes. CIDR or destination IP ranges.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "destinationApplicationSecurityGroups": {
///      "description": "The application security group specified as destination.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationSecurityGroup"
///      }
///    },
///    "destinationPortRange": {
///      "description": "The destination port or range. Integer or range between 0 and 65535. Asterisk '*' can also be used to match all ports.",
///      "type": "string"
///    },
///    "destinationPortRanges": {
///      "description": "The destination port ranges.",
///      "type": "array",
///      "items": {
///        "description": "The destination port.",
///        "type": "string"
///      }
///    },
///    "direction": {
///      "$ref": "#/components/schemas/SecurityRuleDirection"
///    },
///    "priority": {
///      "description": "The priority of the rule. The value can be between 100 and 4096. The priority number must be unique for each rule in the collection. The lower the priority number, the higher the priority of the rule.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "protocol": {
///      "description": "Network protocol this rule applies to.",
///      "type": "string",
///      "enum": [
///        "Tcp",
///        "Udp",
///        "Icmp",
///        "Esp",
///        "*",
///        "Ah"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SecurityRuleProtocol"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "sourceAddressPrefix": {
///      "description": "The CIDR or source IP range. Asterisk '*' can also be used to match all source IPs. Default tags such as 'VirtualNetwork', 'AzureLoadBalancer' and 'Internet' can also be used. If this is an ingress rule, specifies where network traffic originates from.",
///      "type": "string"
///    },
///    "sourceAddressPrefixes": {
///      "description": "The CIDR or source IP ranges.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "sourceApplicationSecurityGroups": {
///      "description": "The application security group specified as source.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationSecurityGroup"
///      }
///    },
///    "sourcePortRange": {
///      "description": "The source port or range. Integer or range between 0 and 65535. Asterisk '*' can also be used to match all ports.",
///      "type": "string"
///    },
///    "sourcePortRanges": {
///      "description": "The source port ranges.",
///      "type": "array",
///      "items": {
///        "description": "The source port.",
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecurityRulePropertiesFormat {
    pub access: SecurityRuleAccess,
    ///A description for this rule. Restricted to 140 chars.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The destination address prefix. CIDR or destination IP range. Asterisk '*' can also be used to match all source IPs. Default tags such as 'VirtualNetwork', 'AzureLoadBalancer' and 'Internet' can also be used.
    #[serde(
        rename = "destinationAddressPrefix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_address_prefix: ::std::option::Option<::std::string::String>,
    ///The destination address prefixes. CIDR or destination IP ranges.
    #[serde(
        rename = "destinationAddressPrefixes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_address_prefixes: ::std::vec::Vec<::std::string::String>,
    ///The application security group specified as destination.
    #[serde(
        rename = "destinationApplicationSecurityGroups",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_application_security_groups: ::std::vec::Vec<ApplicationSecurityGroup>,
    ///The destination port or range. Integer or range between 0 and 65535. Asterisk '*' can also be used to match all ports.
    #[serde(
        rename = "destinationPortRange",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_port_range: ::std::option::Option<::std::string::String>,
    ///The destination port ranges.
    #[serde(
        rename = "destinationPortRanges",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_port_ranges: ::std::vec::Vec<::std::string::String>,
    pub direction: SecurityRuleDirection,
    ///The priority of the rule. The value can be between 100 and 4096. The priority number must be unique for each rule in the collection. The lower the priority number, the higher the priority of the rule.
    pub priority: i32,
    ///Network protocol this rule applies to.
    pub protocol: SecurityRulePropertiesFormatProtocol,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The CIDR or source IP range. Asterisk '*' can also be used to match all source IPs. Default tags such as 'VirtualNetwork', 'AzureLoadBalancer' and 'Internet' can also be used. If this is an ingress rule, specifies where network traffic originates from.
    #[serde(
        rename = "sourceAddressPrefix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_address_prefix: ::std::option::Option<::std::string::String>,
    ///The CIDR or source IP ranges.
    #[serde(
        rename = "sourceAddressPrefixes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_address_prefixes: ::std::vec::Vec<::std::string::String>,
    ///The application security group specified as source.
    #[serde(
        rename = "sourceApplicationSecurityGroups",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_application_security_groups: ::std::vec::Vec<ApplicationSecurityGroup>,
    ///The source port or range. Integer or range between 0 and 65535. Asterisk '*' can also be used to match all ports.
    #[serde(
        rename = "sourcePortRange",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_port_range: ::std::option::Option<::std::string::String>,
    ///The source port ranges.
    #[serde(
        rename = "sourcePortRanges",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_port_ranges: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&SecurityRulePropertiesFormat> for SecurityRulePropertiesFormat {
    fn from(value: &SecurityRulePropertiesFormat) -> Self {
        value.clone()
    }
}
///Network protocol this rule applies to.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Network protocol this rule applies to.",
///  "type": "string",
///  "enum": [
///    "Tcp",
///    "Udp",
///    "Icmp",
///    "Esp",
///    "*",
///    "Ah"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SecurityRuleProtocol"
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
pub enum SecurityRulePropertiesFormatProtocol {
    Tcp,
    Udp,
    Icmp,
    Esp,
    #[serde(rename = "*")]
    X,
    Ah,
}
impl ::std::convert::From<&Self> for SecurityRulePropertiesFormatProtocol {
    fn from(value: &SecurityRulePropertiesFormatProtocol) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SecurityRulePropertiesFormatProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Tcp => f.write_str("Tcp"),
            Self::Udp => f.write_str("Udp"),
            Self::Icmp => f.write_str("Icmp"),
            Self::Esp => f.write_str("Esp"),
            Self::X => f.write_str("*"),
            Self::Ah => f.write_str("Ah"),
        }
    }
}
impl ::std::str::FromStr for SecurityRulePropertiesFormatProtocol {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            "icmp" => Ok(Self::Icmp),
            "esp" => Ok(Self::Esp),
            "*" => Ok(Self::X),
            "ah" => Ok(Self::Ah),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SecurityRulePropertiesFormatProtocol {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SecurityRulePropertiesFormatProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SecurityRulePropertiesFormatProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///ServiceAssociationLink resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "ServiceAssociationLink resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "Name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ServiceAssociationLinkPropertiesFormat"
///    },
///    "type": {
///      "description": "Resource type.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceAssociationLink {
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
    ///Name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<ServiceAssociationLinkPropertiesFormat>,
    ///Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ServiceAssociationLink> for ServiceAssociationLink {
    fn from(value: &ServiceAssociationLink) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceAssociationLink {
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
///Properties of ServiceAssociationLink.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of ServiceAssociationLink.",
///  "properties": {
///    "allowDelete": {
///      "description": "If true, the resource can be deleted.",
///      "type": "boolean"
///    },
///    "link": {
///      "description": "Link to the external resource.",
///      "type": "string"
///    },
///    "linkedResourceType": {
///      "description": "Resource type of the linked resource.",
///      "type": "string"
///    },
///    "locations": {
///      "description": "A list of locations.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceAssociationLinkPropertiesFormat {
    ///If true, the resource can be deleted.
    #[serde(
        rename = "allowDelete",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_delete: ::std::option::Option<bool>,
    ///Link to the external resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub link: ::std::option::Option<::std::string::String>,
    ///Resource type of the linked resource.
    #[serde(
        rename = "linkedResourceType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub linked_resource_type: ::std::option::Option<::std::string::String>,
    ///A list of locations.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub locations: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
}
impl ::std::convert::From<&ServiceAssociationLinkPropertiesFormat>
    for ServiceAssociationLinkPropertiesFormat
{
    fn from(value: &ServiceAssociationLinkPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceAssociationLinkPropertiesFormat {
    fn default() -> Self {
        Self {
            allow_delete: Default::default(),
            link: Default::default(),
            linked_resource_type: Default::default(),
            locations: Default::default(),
            provisioning_state: Default::default(),
        }
    }
}
///Properties of a service delegation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of a service delegation.",
///  "properties": {
///    "actions": {
///      "description": "The actions permitted to the service upon delegation.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "serviceName": {
///      "description": "The name of the service to whom the subnet should be delegated (e.g. Microsoft.Sql/servers).",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceDelegationPropertiesFormat {
    ///The actions permitted to the service upon delegation.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub actions: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The name of the service to whom the subnet should be delegated (e.g. Microsoft.Sql/servers).
    #[serde(
        rename = "serviceName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ServiceDelegationPropertiesFormat>
    for ServiceDelegationPropertiesFormat
{
    fn from(value: &ServiceDelegationPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceDelegationPropertiesFormat {
    fn default() -> Self {
        Self {
            actions: Default::default(),
            provisioning_state: Default::default(),
            service_name: Default::default(),
        }
    }
}
///Service End point policy resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Service End point policy resource.",
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
///    "kind": {
///      "description": "Kind of service endpoint policy. This is metadata used for the Azure portal experience.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ServiceEndpointPolicyPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceEndpointPolicy {
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
    ///Kind of service endpoint policy. This is metadata used for the Azure portal experience.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<::std::string::String>,
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
    pub properties: ::std::option::Option<ServiceEndpointPolicyPropertiesFormat>,
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
impl ::std::convert::From<&ServiceEndpointPolicy> for ServiceEndpointPolicy {
    fn from(value: &ServiceEndpointPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceEndpointPolicy {
    fn default() -> Self {
        Self {
            etag: Default::default(),
            id: Default::default(),
            kind: Default::default(),
            location: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Service Endpoint policy definitions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Service Endpoint policy definitions.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ServiceEndpointPolicyDefinitionPropertiesFormat"
///    },
///    "type": {
///      "description": "The type of the resource.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceEndpointPolicyDefinition {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<ServiceEndpointPolicyDefinitionPropertiesFormat>,
    ///The type of the resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ServiceEndpointPolicyDefinition> for ServiceEndpointPolicyDefinition {
    fn from(value: &ServiceEndpointPolicyDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceEndpointPolicyDefinition {
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
///Service Endpoint policy definition resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Service Endpoint policy definition resource.",
///  "properties": {
///    "description": {
///      "description": "A description for this rule. Restricted to 140 chars.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "service": {
///      "description": "Service endpoint name.",
///      "type": "string"
///    },
///    "serviceResources": {
///      "description": "A list of service resources.",
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
pub struct ServiceEndpointPolicyDefinitionPropertiesFormat {
    ///A description for this rule. Restricted to 140 chars.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///Service endpoint name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service: ::std::option::Option<::std::string::String>,
    ///A list of service resources.
    #[serde(
        rename = "serviceResources",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_resources: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ServiceEndpointPolicyDefinitionPropertiesFormat>
    for ServiceEndpointPolicyDefinitionPropertiesFormat
{
    fn from(value: &ServiceEndpointPolicyDefinitionPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceEndpointPolicyDefinitionPropertiesFormat {
    fn default() -> Self {
        Self {
            description: Default::default(),
            provisioning_state: Default::default(),
            service: Default::default(),
            service_resources: Default::default(),
        }
    }
}
///Service Endpoint Policy resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Service Endpoint Policy resource.",
///  "properties": {
///    "contextualServiceEndpointPolicies": {
///      "description": "A collection of contextual service endpoint policy.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the service endpoint policy resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "serviceAlias": {
///      "description": "The alias indicating if the policy belongs to a service",
///      "type": "string"
///    },
///    "serviceEndpointPolicyDefinitions": {
///      "description": "A collection of service endpoint policy definitions of the service endpoint policy.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ServiceEndpointPolicyDefinition"
///      }
///    },
///    "subnets": {
///      "description": "A collection of references to subnets.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Subnet"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceEndpointPolicyPropertiesFormat {
    ///A collection of contextual service endpoint policy.
    #[serde(
        rename = "contextualServiceEndpointPolicies",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub contextual_service_endpoint_policies: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The resource GUID property of the service endpoint policy resource.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
    ///The alias indicating if the policy belongs to a service
    #[serde(
        rename = "serviceAlias",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_alias: ::std::option::Option<::std::string::String>,
    ///A collection of service endpoint policy definitions of the service endpoint policy.
    #[serde(
        rename = "serviceEndpointPolicyDefinitions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_endpoint_policy_definitions: ::std::vec::Vec<ServiceEndpointPolicyDefinition>,
    ///A collection of references to subnets.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subnets: ::std::vec::Vec<Subnet>,
}
impl ::std::convert::From<&ServiceEndpointPolicyPropertiesFormat>
    for ServiceEndpointPolicyPropertiesFormat
{
    fn from(value: &ServiceEndpointPolicyPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceEndpointPolicyPropertiesFormat {
    fn default() -> Self {
        Self {
            contextual_service_endpoint_policies: Default::default(),
            provisioning_state: Default::default(),
            resource_guid: Default::default(),
            service_alias: Default::default(),
            service_endpoint_policy_definitions: Default::default(),
            subnets: Default::default(),
        }
    }
}
///The service endpoint properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The service endpoint properties.",
///  "properties": {
///    "locations": {
///      "description": "A list of locations.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "networkIdentifier": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "service": {
///      "description": "The type of the endpoint service.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceEndpointPropertiesFormat {
    ///A list of locations.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub locations: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "networkIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_identifier: ::std::option::Option<SubResource>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The type of the endpoint service.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ServiceEndpointPropertiesFormat> for ServiceEndpointPropertiesFormat {
    fn from(value: &ServiceEndpointPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceEndpointPropertiesFormat {
    fn default() -> Self {
        Self {
            locations: Default::default(),
            network_identifier: Default::default(),
            provisioning_state: Default::default(),
            service: Default::default(),
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
        Self {
            id: Default::default(),
        }
    }
}
///Subnet in a virtual network resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Subnet in a virtual network resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/SubResource"
///    }
///  ],
///  "properties": {
///    "etag": {
///      "description": "A unique read-only string that changes whenever the resource is updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource that is unique within a resource group. This name can be used to access the resource.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/SubnetPropertiesFormat"
///    },
///    "type": {
///      "description": "Resource type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Subnet {
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
    ///The name of the resource that is unique within a resource group. This name can be used to access the resource.
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
    pub properties: ::std::option::Option<SubnetPropertiesFormat>,
    ///Resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Subnet> for Subnet {
    fn from(value: &Subnet) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Subnet {
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
///Properties of the subnet.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the subnet.",
///  "properties": {
///    "addressPrefix": {
///      "description": "The address prefix for the subnet.",
///      "type": "string"
///    },
///    "addressPrefixes": {
///      "description": "List of address prefixes for the subnet.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "applicationGatewayIPConfigurations": {
///      "description": "Application gateway IP configurations of virtual network resource.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApplicationGatewayIPConfiguration"
///      }
///    },
///    "defaultOutboundAccess": {
///      "description": "Set this property to false to disable default outbound connectivity for all VMs in the subnet.",
///      "type": "boolean"
///    },
///    "delegations": {
///      "description": "An array of references to the delegations on the subnet.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Delegation"
///      }
///    },
///    "ipAllocations": {
///      "description": "Array of IpAllocation which reference this subnet.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SubResource"
///      }
///    },
///    "ipConfigurationProfiles": {
///      "description": "Array of IP configuration profiles which reference this subnet.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IPConfigurationProfile"
///      }
///    },
///    "ipConfigurations": {
///      "description": "An array of references to the network interface IP configurations using subnet.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IPConfiguration"
///      }
///    },
///    "ipamPoolPrefixAllocations": {
///      "description": "A list of IPAM Pools for allocating IP address prefixes.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IpamPoolPrefixAllocation"
///      }
///    },
///    "natGateway": {
///      "$ref": "#/components/schemas/SubResource"
///    },
///    "networkSecurityGroup": {
///      "$ref": "#/components/schemas/NetworkSecurityGroup"
///    },
///    "privateEndpointNetworkPolicies": {
///      "description": "Enable or Disable apply network policies on private end point in the subnet.",
///      "default": "Disabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled",
///        "NetworkSecurityGroupEnabled",
///        "RouteTableEnabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "VirtualNetworkPrivateEndpointNetworkPolicies"
///      }
///    },
///    "privateEndpoints": {
///      "description": "An array of references to private endpoints.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpoint"
///      }
///    },
///    "privateLinkServiceNetworkPolicies": {
///      "description": "Enable or Disable apply network policies on private link service in the subnet.",
///      "default": "Enabled",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "VirtualNetworkPrivateLinkServiceNetworkPolicies"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "purpose": {
///      "description": "A read-only string identifying the intention of use for this subnet based on delegations and other user-defined properties.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "resourceNavigationLinks": {
///      "description": "An array of references to the external resources using subnet.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ResourceNavigationLink"
///      }
///    },
///    "routeTable": {
///      "$ref": "#/components/schemas/RouteTable"
///    },
///    "serviceAssociationLinks": {
///      "description": "An array of references to services injecting into this subnet.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ServiceAssociationLink"
///      }
///    },
///    "serviceEndpointPolicies": {
///      "description": "An array of service endpoint policies.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ServiceEndpointPolicy"
///      }
///    },
///    "serviceEndpoints": {
///      "description": "An array of service endpoints.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ServiceEndpointPropertiesFormat"
///      }
///    },
///    "sharingScope": {
///      "description": "Set this property to Tenant to allow sharing subnet with other subscriptions in your AAD tenant. This property can only be set if defaultOutboundAccess is set to false, both properties can only be set if subnet is empty.",
///      "type": "string",
///      "enum": [
///        "Tenant",
///        "DelegatedServices"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SharingScope"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SubnetPropertiesFormat {
    ///The address prefix for the subnet.
    #[serde(
        rename = "addressPrefix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub address_prefix: ::std::option::Option<::std::string::String>,
    ///List of address prefixes for the subnet.
    #[serde(
        rename = "addressPrefixes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub address_prefixes: ::std::vec::Vec<::std::string::String>,
    ///Application gateway IP configurations of virtual network resource.
    #[serde(
        rename = "applicationGatewayIPConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub application_gateway_ip_configurations: ::std::vec::Vec<ApplicationGatewayIpConfiguration>,
    ///Set this property to false to disable default outbound connectivity for all VMs in the subnet.
    #[serde(
        rename = "defaultOutboundAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_outbound_access: ::std::option::Option<bool>,
    ///An array of references to the delegations on the subnet.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delegations: ::std::vec::Vec<Delegation>,
    ///Array of IpAllocation which reference this subnet.
    #[serde(
        rename = "ipAllocations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_allocations: ::std::vec::Vec<SubResource>,
    ///Array of IP configuration profiles which reference this subnet.
    #[serde(
        rename = "ipConfigurationProfiles",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_configuration_profiles: ::std::vec::Vec<IpConfigurationProfile>,
    ///An array of references to the network interface IP configurations using subnet.
    #[serde(
        rename = "ipConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_configurations: ::std::vec::Vec<IpConfiguration>,
    ///A list of IPAM Pools for allocating IP address prefixes.
    #[serde(
        rename = "ipamPoolPrefixAllocations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ipam_pool_prefix_allocations: ::std::vec::Vec<IpamPoolPrefixAllocation>,
    #[serde(
        rename = "natGateway",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub nat_gateway: ::std::option::Option<SubResource>,
    #[serde(
        rename = "networkSecurityGroup",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_security_group: ::std::option::Option<NetworkSecurityGroup>,
    ///Enable or Disable apply network policies on private end point in the subnet.
    #[serde(
        rename = "privateEndpointNetworkPolicies",
        default = "defaults::subnet_properties_format_private_endpoint_network_policies",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_network_policies: SubnetPropertiesFormatPrivateEndpointNetworkPolicies,
    ///An array of references to private endpoints.
    #[serde(
        rename = "privateEndpoints",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoints: ::std::vec::Vec<PrivateEndpoint>,
    ///Enable or Disable apply network policies on private link service in the subnet.
    #[serde(
        rename = "privateLinkServiceNetworkPolicies",
        default = "defaults::subnet_properties_format_private_link_service_network_policies",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_link_service_network_policies:
        SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///A read-only string identifying the intention of use for this subnet based on delegations and other user-defined properties.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub purpose: ::std::option::Option<::std::string::String>,
    ///An array of references to the external resources using subnet.
    #[serde(
        rename = "resourceNavigationLinks",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_navigation_links: ::std::vec::Vec<ResourceNavigationLink>,
    #[serde(
        rename = "routeTable",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub route_table: ::std::option::Option<RouteTable>,
    ///An array of references to services injecting into this subnet.
    #[serde(
        rename = "serviceAssociationLinks",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_association_links: ::std::vec::Vec<ServiceAssociationLink>,
    ///An array of service endpoint policies.
    #[serde(
        rename = "serviceEndpointPolicies",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_endpoint_policies: ::std::vec::Vec<ServiceEndpointPolicy>,
    ///An array of service endpoints.
    #[serde(
        rename = "serviceEndpoints",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_endpoints: ::std::vec::Vec<ServiceEndpointPropertiesFormat>,
    ///Set this property to Tenant to allow sharing subnet with other subscriptions in your AAD tenant. This property can only be set if defaultOutboundAccess is set to false, both properties can only be set if subnet is empty.
    #[serde(
        rename = "sharingScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sharing_scope: ::std::option::Option<SubnetPropertiesFormatSharingScope>,
}
impl ::std::convert::From<&SubnetPropertiesFormat> for SubnetPropertiesFormat {
    fn from(value: &SubnetPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SubnetPropertiesFormat {
    fn default() -> Self {
        Self {
            address_prefix: Default::default(),
            address_prefixes: Default::default(),
            application_gateway_ip_configurations: Default::default(),
            default_outbound_access: Default::default(),
            delegations: Default::default(),
            ip_allocations: Default::default(),
            ip_configuration_profiles: Default::default(),
            ip_configurations: Default::default(),
            ipam_pool_prefix_allocations: Default::default(),
            nat_gateway: Default::default(),
            network_security_group: Default::default(),
            private_endpoint_network_policies:
                defaults::subnet_properties_format_private_endpoint_network_policies(),
            private_endpoints: Default::default(),
            private_link_service_network_policies:
                defaults::subnet_properties_format_private_link_service_network_policies(),
            provisioning_state: Default::default(),
            purpose: Default::default(),
            resource_navigation_links: Default::default(),
            route_table: Default::default(),
            service_association_links: Default::default(),
            service_endpoint_policies: Default::default(),
            service_endpoints: Default::default(),
            sharing_scope: Default::default(),
        }
    }
}
///Enable or Disable apply network policies on private end point in the subnet.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Enable or Disable apply network policies on private end point in the subnet.",
///  "default": "Disabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled",
///    "NetworkSecurityGroupEnabled",
///    "RouteTableEnabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "VirtualNetworkPrivateEndpointNetworkPolicies"
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
pub enum SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
    Enabled,
    Disabled,
    NetworkSecurityGroupEnabled,
    RouteTableEnabled,
}
impl ::std::convert::From<&Self> for SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
    fn from(value: &SubnetPropertiesFormatPrivateEndpointNetworkPolicies) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
            Self::NetworkSecurityGroupEnabled => f.write_str("NetworkSecurityGroupEnabled"),
            Self::RouteTableEnabled => f.write_str("RouteTableEnabled"),
        }
    }
}
impl ::std::str::FromStr for SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            "networksecuritygroupenabled" => Ok(Self::NetworkSecurityGroupEnabled),
            "routetableenabled" => Ok(Self::RouteTableEnabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for SubnetPropertiesFormatPrivateEndpointNetworkPolicies
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for SubnetPropertiesFormatPrivateEndpointNetworkPolicies
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
    fn default() -> Self {
        SubnetPropertiesFormatPrivateEndpointNetworkPolicies::Disabled
    }
}
///Enable or Disable apply network policies on private link service in the subnet.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Enable or Disable apply network policies on private link service in the subnet.",
///  "default": "Enabled",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "VirtualNetworkPrivateLinkServiceNetworkPolicies"
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
pub enum SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
    fn from(value: &SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
    fn default() -> Self {
        SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies::Enabled
    }
}
///Set this property to Tenant to allow sharing subnet with other subscriptions in your AAD tenant. This property can only be set if defaultOutboundAccess is set to false, both properties can only be set if subnet is empty.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Set this property to Tenant to allow sharing subnet with other subscriptions in your AAD tenant. This property can only be set if defaultOutboundAccess is set to false, both properties can only be set if subnet is empty.",
///  "type": "string",
///  "enum": [
///    "Tenant",
///    "DelegatedServices"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SharingScope"
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
pub enum SubnetPropertiesFormatSharingScope {
    Tenant,
    DelegatedServices,
}
impl ::std::convert::From<&Self> for SubnetPropertiesFormatSharingScope {
    fn from(value: &SubnetPropertiesFormatSharingScope) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SubnetPropertiesFormatSharingScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Tenant => f.write_str("Tenant"),
            Self::DelegatedServices => f.write_str("DelegatedServices"),
        }
    }
}
impl ::std::str::FromStr for SubnetPropertiesFormatSharingScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "tenant" => Ok(Self::Tenant),
            "delegatedservices" => Ok(Self::DelegatedServices),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SubnetPropertiesFormatSharingScope {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SubnetPropertiesFormatSharingScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SubnetPropertiesFormatSharingScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
        Self {
            tags: Default::default(),
        }
    }
}
///Parameters that define the configuration of traffic analytics.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters that define the configuration of traffic analytics.",
///  "properties": {
///    "enabled": {
///      "description": "Flag to enable/disable traffic analytics.",
///      "type": "boolean"
///    },
///    "trafficAnalyticsInterval": {
///      "description": "The interval in minutes which would decide how frequently TA service should do flow analytics.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "workspaceId": {
///      "description": "The resource guid of the attached workspace.",
///      "type": "string"
///    },
///    "workspaceRegion": {
///      "description": "The location of the attached workspace.",
///      "type": "string"
///    },
///    "workspaceResourceId": {
///      "description": "Resource Id of the attached workspace.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TrafficAnalyticsConfigurationProperties {
    ///Flag to enable/disable traffic analytics.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///The interval in minutes which would decide how frequently TA service should do flow analytics.
    #[serde(
        rename = "trafficAnalyticsInterval",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub traffic_analytics_interval: ::std::option::Option<i32>,
    ///The resource guid of the attached workspace.
    #[serde(
        rename = "workspaceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workspace_id: ::std::option::Option<::std::string::String>,
    ///The location of the attached workspace.
    #[serde(
        rename = "workspaceRegion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workspace_region: ::std::option::Option<::std::string::String>,
    ///Resource Id of the attached workspace.
    #[serde(
        rename = "workspaceResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workspace_resource_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TrafficAnalyticsConfigurationProperties>
    for TrafficAnalyticsConfigurationProperties
{
    fn from(value: &TrafficAnalyticsConfigurationProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TrafficAnalyticsConfigurationProperties {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
            traffic_analytics_interval: Default::default(),
            workspace_id: Default::default(),
            workspace_region: Default::default(),
            workspace_resource_id: Default::default(),
        }
    }
}
///Parameters that define the configuration of traffic analytics.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters that define the configuration of traffic analytics.",
///  "properties": {
///    "networkWatcherFlowAnalyticsConfiguration": {
///      "$ref": "#/components/schemas/TrafficAnalyticsConfigurationProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TrafficAnalyticsProperties {
    #[serde(
        rename = "networkWatcherFlowAnalyticsConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_watcher_flow_analytics_configuration:
        ::std::option::Option<TrafficAnalyticsConfigurationProperties>,
}
impl ::std::convert::From<&TrafficAnalyticsProperties> for TrafficAnalyticsProperties {
    fn from(value: &TrafficAnalyticsProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TrafficAnalyticsProperties {
    fn default() -> Self {
        Self {
            network_watcher_flow_analytics_configuration: Default::default(),
        }
    }
}
///The transport protocol for the endpoint.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The transport protocol for the endpoint.",
///  "type": "string",
///  "enum": [
///    "Udp",
///    "Tcp",
///    "All",
///    "Quic"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TransportProtocol"
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
pub enum TransportProtocol {
    Udp,
    Tcp,
    All,
    Quic,
}
impl ::std::convert::From<&Self> for TransportProtocol {
    fn from(value: &TransportProtocol) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TransportProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Udp => f.write_str("Udp"),
            Self::Tcp => f.write_str("Tcp"),
            Self::All => f.write_str("All"),
            Self::Quic => f.write_str("Quic"),
        }
    }
}
impl ::std::str::FromStr for TransportProtocol {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "udp" => Ok(Self::Udp),
            "tcp" => Ok(Self::Tcp),
            "all" => Ok(Self::All),
            "quic" => Ok(Self::Quic),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TransportProtocol {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TransportProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TransportProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Virtual Network Tap resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Virtual Network Tap resource.",
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
///      "$ref": "#/components/schemas/VirtualNetworkTapPropertiesFormat"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VirtualNetworkTap {
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
    pub properties: ::std::option::Option<VirtualNetworkTapPropertiesFormat>,
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
impl ::std::convert::From<&VirtualNetworkTap> for VirtualNetworkTap {
    fn from(value: &VirtualNetworkTap) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VirtualNetworkTap {
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
///Virtual Network Tap properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Virtual Network Tap properties.",
///  "properties": {
///    "destinationLoadBalancerFrontEndIPConfiguration": {
///      "$ref": "#/components/schemas/FrontendIPConfiguration"
///    },
///    "destinationNetworkInterfaceIPConfiguration": {
///      "$ref": "#/components/schemas/NetworkInterfaceIPConfiguration"
///    },
///    "destinationPort": {
///      "description": "The VXLAN destination port that will receive the tapped traffic.",
///      "type": "integer"
///    },
///    "networkInterfaceTapConfigurations": {
///      "description": "Specifies the list of resource IDs for the network interface IP configuration that needs to be tapped.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/NetworkInterfaceTapConfiguration"
///      }
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/ProvisioningState"
///    },
///    "resourceGuid": {
///      "description": "The resource GUID property of the virtual network tap resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VirtualNetworkTapPropertiesFormat {
    #[serde(
        rename = "destinationLoadBalancerFrontEndIPConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_load_balancer_front_end_ip_configuration:
        ::std::option::Option<FrontendIpConfiguration>,
    #[serde(
        rename = "destinationNetworkInterfaceIPConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_network_interface_ip_configuration:
        ::std::option::Option<NetworkInterfaceIpConfiguration>,
    ///The VXLAN destination port that will receive the tapped traffic.
    #[serde(
        rename = "destinationPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination_port: ::std::option::Option<i64>,
    ///Specifies the list of resource IDs for the network interface IP configuration that needs to be tapped.
    #[serde(
        rename = "networkInterfaceTapConfigurations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_interface_tap_configurations: ::std::vec::Vec<NetworkInterfaceTapConfiguration>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<ProvisioningState>,
    ///The resource GUID property of the virtual network tap resource.
    #[serde(
        rename = "resourceGuid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_guid: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&VirtualNetworkTapPropertiesFormat>
    for VirtualNetworkTapPropertiesFormat
{
    fn from(value: &VirtualNetworkTapPropertiesFormat) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VirtualNetworkTapPropertiesFormat {
    fn default() -> Self {
        Self {
            destination_load_balancer_front_end_ip_configuration: Default::default(),
            destination_network_interface_ip_configuration: Default::default(),
            destination_port: Default::default(),
            network_interface_tap_configurations: Default::default(),
            provisioning_state: Default::default(),
            resource_guid: Default::default(),
        }
    }
}
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn private_endpoint_properties_ip_version_type(
    ) -> super::PrivateEndpointPropertiesIpVersionType {
        super::PrivateEndpointPropertiesIpVersionType::IPv4
    }
    pub(super) fn subnet_properties_format_private_endpoint_network_policies(
    ) -> super::SubnetPropertiesFormatPrivateEndpointNetworkPolicies {
        super::SubnetPropertiesFormatPrivateEndpointNetworkPolicies::Disabled
    }
    pub(super) fn subnet_properties_format_private_link_service_network_policies(
    ) -> super::SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies {
        super::SubnetPropertiesFormatPrivateLinkServiceNetworkPolicies::Enabled
    }
}
