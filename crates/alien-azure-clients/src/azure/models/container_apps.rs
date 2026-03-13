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
///Container App base container definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App base container definition.",
///  "type": "object",
///  "properties": {
///    "args": {
///      "description": "Container start command arguments.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "command": {
///      "description": "Container start command.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "env": {
///      "description": "Container environment variables.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EnvironmentVar"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "image": {
///      "description": "Container image tag.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Custom container name.",
///      "type": "string"
///    },
///    "resources": {
///      "$ref": "#/components/schemas/ContainerResources"
///    },
///    "volumeMounts": {
///      "description": "Container volume mounts.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/VolumeMount"
///      },
///      "x-ms-identifiers": [
///        "volumeName"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BaseContainer {
    ///Container start command arguments.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub args: ::std::vec::Vec<::std::string::String>,
    ///Container start command.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub command: ::std::vec::Vec<::std::string::String>,
    ///Container environment variables.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub env: ::std::vec::Vec<EnvironmentVar>,
    ///Container image tag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub image: ::std::option::Option<::std::string::String>,
    ///Custom container name.
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
    pub resources: ::std::option::Option<ContainerResources>,
    ///Container volume mounts.
    #[serde(
        rename = "volumeMounts",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub volume_mounts: ::std::vec::Vec<VolumeMount>,
}
impl ::std::convert::From<&BaseContainer> for BaseContainer {
    fn from(value: &BaseContainer) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BaseContainer {
    fn default() -> Self {
        Self {
            args: Default::default(),
            command: Default::default(),
            env: Default::default(),
            image: Default::default(),
            name: Default::default(),
            resources: Default::default(),
            volume_mounts: Default::default(),
        }
    }
}
///Non versioned Container App configuration properties that define the mutable settings of a Container app
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Non versioned Container App configuration properties that define the mutable settings of a Container app",
///  "type": "object",
///  "properties": {
///    "activeRevisionsMode": {
///      "description": "ActiveRevisionsMode controls how active revisions are handled for the Container app:\n<list><item>Multiple: multiple revisions can be active.</item><item>Single: Only one revision can be active at a time. Revision weights can not be used in this mode. If no value if provided, this is the default.</item></list>",
///      "default": "Single",
///      "type": "string",
///      "enum": [
///        "Multiple",
///        "Single"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ActiveRevisionsMode"
///      }
///    },
///    "dapr": {
///      "$ref": "#/components/schemas/Dapr"
///    },
///    "identitySettings": {
///      "description": "Optional settings for Managed Identities that are assigned to the Container App. If a Managed Identity is not specified here, default settings will be used.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IdentitySettings"
///      },
///      "x-ms-identifiers": [
///        "identity"
///      ]
///    },
///    "ingress": {
///      "$ref": "#/components/schemas/Ingress"
///    },
///    "maxInactiveRevisions": {
///      "description": "Optional. Max inactive revisions a Container App can have.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "registries": {
///      "description": "Collection of private container registry credentials for containers used by the Container app",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RegistryCredentials"
///      },
///      "x-ms-identifiers": [
///        "server"
///      ]
///    },
///    "runtime": {
///      "$ref": "#/components/schemas/Runtime"
///    },
///    "secrets": {
///      "description": "Collection of secrets used by a Container app",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Secret"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "service": {
///      "$ref": "#/components/schemas/Service"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Configuration {
    /**ActiveRevisionsMode controls how active revisions are handled for the Container app:
<list><item>Multiple: multiple revisions can be active.</item><item>Single: Only one revision can be active at a time. Revision weights can not be used in this mode. If no value if provided, this is the default.</item></list>*/
    #[serde(
        rename = "activeRevisionsMode",
        default = "defaults::configuration_active_revisions_mode",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub active_revisions_mode: ConfigurationActiveRevisionsMode,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dapr: ::std::option::Option<Dapr>,
    ///Optional settings for Managed Identities that are assigned to the Container App. If a Managed Identity is not specified here, default settings will be used.
    #[serde(
        rename = "identitySettings",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity_settings: ::std::vec::Vec<IdentitySettings>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ingress: ::std::option::Option<Ingress>,
    ///Optional. Max inactive revisions a Container App can have.
    #[serde(
        rename = "maxInactiveRevisions",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_inactive_revisions: ::std::option::Option<i32>,
    ///Collection of private container registry credentials for containers used by the Container app
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registries: ::std::vec::Vec<RegistryCredentials>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub runtime: ::std::option::Option<Runtime>,
    ///Collection of secrets used by a Container app
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secrets: ::std::vec::Vec<Secret>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service: ::std::option::Option<Service>,
}
impl ::std::convert::From<&Configuration> for Configuration {
    fn from(value: &Configuration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Configuration {
    fn default() -> Self {
        Self {
            active_revisions_mode: defaults::configuration_active_revisions_mode(),
            dapr: Default::default(),
            identity_settings: Default::default(),
            ingress: Default::default(),
            max_inactive_revisions: Default::default(),
            registries: Default::default(),
            runtime: Default::default(),
            secrets: Default::default(),
            service: Default::default(),
        }
    }
}
/**ActiveRevisionsMode controls how active revisions are handled for the Container app:
<list><item>Multiple: multiple revisions can be active.</item><item>Single: Only one revision can be active at a time. Revision weights can not be used in this mode. If no value if provided, this is the default.</item></list>*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "ActiveRevisionsMode controls how active revisions are handled for the Container app:\n<list><item>Multiple: multiple revisions can be active.</item><item>Single: Only one revision can be active at a time. Revision weights can not be used in this mode. If no value if provided, this is the default.</item></list>",
///  "default": "Single",
///  "type": "string",
///  "enum": [
///    "Multiple",
///    "Single"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ActiveRevisionsMode"
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
pub enum ConfigurationActiveRevisionsMode {
    Multiple,
    Single,
}
impl ::std::convert::From<&Self> for ConfigurationActiveRevisionsMode {
    fn from(value: &ConfigurationActiveRevisionsMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ConfigurationActiveRevisionsMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Multiple => f.write_str("Multiple"),
            Self::Single => f.write_str("Single"),
        }
    }
}
impl ::std::str::FromStr for ConfigurationActiveRevisionsMode {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "multiple" => Ok(Self::Multiple),
            "single" => Ok(Self::Single),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ConfigurationActiveRevisionsMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for ConfigurationActiveRevisionsMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for ConfigurationActiveRevisionsMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for ConfigurationActiveRevisionsMode {
    fn default() -> Self {
        ConfigurationActiveRevisionsMode::Single
    }
}
///Container App container definition
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container definition",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/BaseContainer"
///    }
///  ],
///  "properties": {
///    "probes": {
///      "description": "List of probes for the container.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ContainerAppProbe"
///      },
///      "x-ms-identifiers": [
///        "type"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Container {
    ///Container start command arguments.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub args: ::std::vec::Vec<::std::string::String>,
    ///Container start command.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub command: ::std::vec::Vec<::std::string::String>,
    ///Container environment variables.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub env: ::std::vec::Vec<EnvironmentVar>,
    ///Container image tag.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub image: ::std::option::Option<::std::string::String>,
    ///Custom container name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///List of probes for the container.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub probes: ::std::vec::Vec<ContainerAppProbe>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resources: ::std::option::Option<ContainerResources>,
    ///Container volume mounts.
    #[serde(
        rename = "volumeMounts",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub volume_mounts: ::std::vec::Vec<VolumeMount>,
}
impl ::std::convert::From<&Container> for Container {
    fn from(value: &Container) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Container {
    fn default() -> Self {
        Self {
            args: Default::default(),
            command: Default::default(),
            env: Default::default(),
            image: Default::default(),
            name: Default::default(),
            probes: Default::default(),
            resources: Default::default(),
            volume_mounts: Default::default(),
        }
    }
}
///Container App.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "identity": {
///      "$ref": "#/components/schemas/ManagedServiceIdentity"
///    },
///    "managedBy": {
///      "description": "The fully qualified resource ID of the resource that manages this resource. Indicates if this resource is managed by another Azure resource. If this is present, complete mode deployment will not delete the resource if it is removed from the template since it is managed by another resource.",
///      "type": "string",
///      "x-ms-mutability": [
///        "read",
///        "create",
///        "update"
///      ]
///    },
///    "properties": {
///      "description": "ContainerApp resource specific properties",
///      "type": "object",
///      "properties": {
///        "configuration": {
///          "$ref": "#/components/schemas/Configuration"
///        },
///        "customDomainVerificationId": {
///          "description": "Id used to verify domain name ownership",
///          "readOnly": true,
///          "type": "string"
///        },
///        "environmentId": {
///          "description": "Resource ID of environment.",
///          "type": "string",
///          "x-ms-mutability": [
///            "create",
///            "read"
///          ]
///        },
///        "eventStreamEndpoint": {
///          "description": "The endpoint of the eventstream of the container app.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "latestReadyRevisionName": {
///          "description": "Name of the latest ready revision of the Container App.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "latestRevisionFqdn": {
///          "description": "Fully Qualified Domain Name of the latest revision of the Container App.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "latestRevisionName": {
///          "description": "Name of the latest revision of the Container App.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "managedEnvironmentId": {
///          "description": "Deprecated. Resource ID of the Container App's environment.",
///          "type": "string",
///          "x-ms-mutability": [
///            "create",
///            "read"
///          ]
///        },
///        "outboundIpAddresses": {
///          "description": "Outbound IP Addresses for container app.",
///          "readOnly": true,
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "provisioningState": {
///          "description": "Provisioning state of the Container App.",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "InProgress",
///            "Succeeded",
///            "Failed",
///            "Canceled",
///            "Deleting"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "ContainerAppProvisioningState"
///          }
///        },
///        "runningStatus": {
///          "description": "Running status of the Container App.",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "Progressing",
///            "Running",
///            "Stopped",
///            "Suspended",
///            "Ready"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "ContainerAppRunningStatus",
///            "values": [
///              {
///                "description": "Container App is transitioning between Stopped and Running states.",
///                "value": "Progressing"
///              },
///              {
///                "description": "Container App is in Running state.",
///                "value": "Running"
///              },
///              {
///                "description": "Container App is in Stopped state.",
///                "value": "Stopped"
///              },
///              {
///                "description": "Container App Job is in Suspended state.",
///                "value": "Suspended"
///              },
///              {
///                "description": "Container App Job is in Ready state.",
///                "value": "Ready"
///              }
///            ]
///          }
///        },
///        "template": {
///          "$ref": "#/components/schemas/Template"
///        },
///        "workloadProfileName": {
///          "$ref": "#/components/schemas/WorkloadProfileName"
///        }
///      },
///      "x-ms-client-flatten": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerApp {
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Fully qualified resource ID for the resource. E.g. "/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}"
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
    ///The geo-location where the resource lives
    pub location: ::std::string::String,
    ///The fully qualified resource ID of the resource that manages this resource. Indicates if this resource is managed by another Azure resource. If this is present, complete mode deployment will not delete the resource if it is removed from the template since it is managed by another resource.
    #[serde(
        rename = "managedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by: ::std::option::Option<::std::string::String>,
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
    pub properties: ::std::option::Option<ContainerAppProperties>,
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
impl ::std::convert::From<&ContainerApp> for ContainerApp {
    fn from(value: &ContainerApp) -> Self {
        value.clone()
    }
}
///Container App Auth Token.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Auth Token.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "Container App auth token resource specific properties",
///      "type": "object",
///      "properties": {
///        "expires": {
///          "description": "Token expiration date.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "token": {
///          "description": "Auth token value.",
///          "readOnly": true,
///          "type": "string",
///          "x-ms-secret": true
///        }
///      },
///      "x-ms-client-flatten": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppAuthToken {
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
    pub properties: ::std::option::Option<ContainerAppAuthTokenProperties>,
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
impl ::std::convert::From<&ContainerAppAuthToken> for ContainerAppAuthToken {
    fn from(value: &ContainerAppAuthToken) -> Self {
        value.clone()
    }
}
///Container App auth token resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App auth token resource specific properties",
///  "type": "object",
///  "properties": {
///    "expires": {
///      "description": "Token expiration date.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "token": {
///      "description": "Auth token value.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-secret": true
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppAuthTokenProperties {
    ///Token expiration date.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expires: ::std::option::Option<::std::string::String>,
    ///Auth token value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub token: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ContainerAppAuthTokenProperties>
for ContainerAppAuthTokenProperties {
    fn from(value: &ContainerAppAuthTokenProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ContainerAppAuthTokenProperties {
    fn default() -> Self {
        Self {
            expires: Default::default(),
            token: Default::default(),
        }
    }
}
///Container App collection ARM resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App collection ARM resource.",
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
///        "$ref": "#/components/schemas/ContainerApp"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<ContainerApp>,
}
impl ::std::convert::From<&ContainerAppCollection> for ContainerAppCollection {
    fn from(value: &ContainerAppCollection) -> Self {
        value.clone()
    }
}
///Probe describes a health check to be performed against a container to determine whether it is alive or ready to receive traffic.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Probe describes a health check to be performed against a container to determine whether it is alive or ready to receive traffic.",
///  "type": "object",
///  "properties": {
///    "failureThreshold": {
///      "description": "Minimum consecutive failures for the probe to be considered failed after having succeeded. Defaults to 3. Minimum value is 1. Maximum value is 10.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "httpGet": {
///      "description": "HTTPGet specifies the http request to perform.",
///      "type": "object",
///      "required": [
///        "port"
///      ],
///      "properties": {
///        "host": {
///          "description": "Host name to connect to, defaults to the pod IP. You probably want to set \"Host\" in httpHeaders instead.",
///          "type": "string"
///        },
///        "httpHeaders": {
///          "description": "Custom headers to set in the request. HTTP allows repeated headers.",
///          "type": "array",
///          "items": {
///            "description": "HTTPHeader describes a custom header to be used in HTTP probes",
///            "type": "object",
///            "required": [
///              "name",
///              "value"
///            ],
///            "properties": {
///              "name": {
///                "description": "The header field name",
///                "type": "string"
///              },
///              "value": {
///                "description": "The header field value",
///                "type": "string"
///              }
///            }
///          },
///          "x-ms-identifiers": [
///            "name"
///          ]
///        },
///        "path": {
///          "description": "Path to access on the HTTP server.",
///          "type": "string"
///        },
///        "port": {
///          "description": "Name or number of the port to access on the container. Number must be in the range 1 to 65535. Name must be an IANA_SVC_NAME.",
///          "type": "integer",
///          "format": "int32"
///        },
///        "scheme": {
///          "description": "Scheme to use for connecting to the host. Defaults to HTTP.",
///          "type": "string",
///          "enum": [
///            "HTTP",
///            "HTTPS"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "Scheme"
///          }
///        }
///      }
///    },
///    "initialDelaySeconds": {
///      "description": "Number of seconds after the container has started before liveness probes are initiated. Minimum value is 1. Maximum value is 60.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "periodSeconds": {
///      "description": "How often (in seconds) to perform the probe. Default to 10 seconds. Minimum value is 1. Maximum value is 240.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "successThreshold": {
///      "description": "Minimum consecutive successes for the probe to be considered successful after having failed. Defaults to 1. Must be 1 for liveness and startup. Minimum value is 1. Maximum value is 10.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "tcpSocket": {
///      "description": "TCPSocket specifies an action involving a TCP port. TCP hooks not yet supported.",
///      "type": "object",
///      "required": [
///        "port"
///      ],
///      "properties": {
///        "host": {
///          "description": "Optional: Host name to connect to, defaults to the pod IP.",
///          "type": "string"
///        },
///        "port": {
///          "description": "Number or name of the port to access on the container. Number must be in the range 1 to 65535. Name must be an IANA_SVC_NAME.",
///          "type": "integer",
///          "format": "int32"
///        }
///      }
///    },
///    "terminationGracePeriodSeconds": {
///      "description": "Optional duration in seconds the pod needs to terminate gracefully upon probe failure. The grace period is the duration in seconds after the processes running in the pod are sent a termination signal and the time when the processes are forcibly halted with a kill signal. Set this value longer than the expected cleanup time for your process. If this value is nil, the pod's terminationGracePeriodSeconds will be used. Otherwise, this value overrides the value provided by the pod spec. Value must be non-negative integer. The value zero indicates stop immediately via the kill signal (no opportunity to shut down). This is an alpha field and requires enabling ProbeTerminationGracePeriod feature gate. Maximum value is 3600 seconds (1 hour)",
///      "type": "integer",
///      "format": "int64"
///    },
///    "timeoutSeconds": {
///      "description": "Number of seconds after which the probe times out. Defaults to 1 second. Minimum value is 1. Maximum value is 240.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "type": {
///      "description": "The type of probe.",
///      "type": "string",
///      "enum": [
///        "Liveness",
///        "Readiness",
///        "Startup"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Type"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppProbe {
    ///Minimum consecutive failures for the probe to be considered failed after having succeeded. Defaults to 3. Minimum value is 1. Maximum value is 10.
    #[serde(
        rename = "failureThreshold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub failure_threshold: ::std::option::Option<i32>,
    #[serde(
        rename = "httpGet",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub http_get: ::std::option::Option<ContainerAppProbeHttpGet>,
    ///Number of seconds after the container has started before liveness probes are initiated. Minimum value is 1. Maximum value is 60.
    #[serde(
        rename = "initialDelaySeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub initial_delay_seconds: ::std::option::Option<i32>,
    ///How often (in seconds) to perform the probe. Default to 10 seconds. Minimum value is 1. Maximum value is 240.
    #[serde(
        rename = "periodSeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub period_seconds: ::std::option::Option<i32>,
    ///Minimum consecutive successes for the probe to be considered successful after having failed. Defaults to 1. Must be 1 for liveness and startup. Minimum value is 1. Maximum value is 10.
    #[serde(
        rename = "successThreshold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub success_threshold: ::std::option::Option<i32>,
    #[serde(
        rename = "tcpSocket",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tcp_socket: ::std::option::Option<ContainerAppProbeTcpSocket>,
    ///Optional duration in seconds the pod needs to terminate gracefully upon probe failure. The grace period is the duration in seconds after the processes running in the pod are sent a termination signal and the time when the processes are forcibly halted with a kill signal. Set this value longer than the expected cleanup time for your process. If this value is nil, the pod's terminationGracePeriodSeconds will be used. Otherwise, this value overrides the value provided by the pod spec. Value must be non-negative integer. The value zero indicates stop immediately via the kill signal (no opportunity to shut down). This is an alpha field and requires enabling ProbeTerminationGracePeriod feature gate. Maximum value is 3600 seconds (1 hour)
    #[serde(
        rename = "terminationGracePeriodSeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub termination_grace_period_seconds: ::std::option::Option<i64>,
    ///Number of seconds after which the probe times out. Defaults to 1 second. Minimum value is 1. Maximum value is 240.
    #[serde(
        rename = "timeoutSeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub timeout_seconds: ::std::option::Option<i32>,
    ///The type of probe.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<ContainerAppProbeType>,
}
impl ::std::convert::From<&ContainerAppProbe> for ContainerAppProbe {
    fn from(value: &ContainerAppProbe) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ContainerAppProbe {
    fn default() -> Self {
        Self {
            failure_threshold: Default::default(),
            http_get: Default::default(),
            initial_delay_seconds: Default::default(),
            period_seconds: Default::default(),
            success_threshold: Default::default(),
            tcp_socket: Default::default(),
            termination_grace_period_seconds: Default::default(),
            timeout_seconds: Default::default(),
            type_: Default::default(),
        }
    }
}
///HTTPGet specifies the http request to perform.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "HTTPGet specifies the http request to perform.",
///  "type": "object",
///  "required": [
///    "port"
///  ],
///  "properties": {
///    "host": {
///      "description": "Host name to connect to, defaults to the pod IP. You probably want to set \"Host\" in httpHeaders instead.",
///      "type": "string"
///    },
///    "httpHeaders": {
///      "description": "Custom headers to set in the request. HTTP allows repeated headers.",
///      "type": "array",
///      "items": {
///        "description": "HTTPHeader describes a custom header to be used in HTTP probes",
///        "type": "object",
///        "required": [
///          "name",
///          "value"
///        ],
///        "properties": {
///          "name": {
///            "description": "The header field name",
///            "type": "string"
///          },
///          "value": {
///            "description": "The header field value",
///            "type": "string"
///          }
///        }
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "path": {
///      "description": "Path to access on the HTTP server.",
///      "type": "string"
///    },
///    "port": {
///      "description": "Name or number of the port to access on the container. Number must be in the range 1 to 65535. Name must be an IANA_SVC_NAME.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "scheme": {
///      "description": "Scheme to use for connecting to the host. Defaults to HTTP.",
///      "type": "string",
///      "enum": [
///        "HTTP",
///        "HTTPS"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Scheme"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppProbeHttpGet {
    ///Host name to connect to, defaults to the pod IP. You probably want to set "Host" in httpHeaders instead.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub host: ::std::option::Option<::std::string::String>,
    ///Custom headers to set in the request. HTTP allows repeated headers.
    #[serde(
        rename = "httpHeaders",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub http_headers: ::std::vec::Vec<ContainerAppProbeHttpGetHttpHeadersItem>,
    ///Path to access on the HTTP server.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub path: ::std::option::Option<::std::string::String>,
    ///Name or number of the port to access on the container. Number must be in the range 1 to 65535. Name must be an IANA_SVC_NAME.
    pub port: i32,
    ///Scheme to use for connecting to the host. Defaults to HTTP.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scheme: ::std::option::Option<ContainerAppProbeHttpGetScheme>,
}
impl ::std::convert::From<&ContainerAppProbeHttpGet> for ContainerAppProbeHttpGet {
    fn from(value: &ContainerAppProbeHttpGet) -> Self {
        value.clone()
    }
}
///HTTPHeader describes a custom header to be used in HTTP probes
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "HTTPHeader describes a custom header to be used in HTTP probes",
///  "type": "object",
///  "required": [
///    "name",
///    "value"
///  ],
///  "properties": {
///    "name": {
///      "description": "The header field name",
///      "type": "string"
///    },
///    "value": {
///      "description": "The header field value",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppProbeHttpGetHttpHeadersItem {
    ///The header field name
    pub name: ::std::string::String,
    ///The header field value
    pub value: ::std::string::String,
}
impl ::std::convert::From<&ContainerAppProbeHttpGetHttpHeadersItem>
for ContainerAppProbeHttpGetHttpHeadersItem {
    fn from(value: &ContainerAppProbeHttpGetHttpHeadersItem) -> Self {
        value.clone()
    }
}
///Scheme to use for connecting to the host. Defaults to HTTP.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Scheme to use for connecting to the host. Defaults to HTTP.",
///  "type": "string",
///  "enum": [
///    "HTTP",
///    "HTTPS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Scheme"
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
pub enum ContainerAppProbeHttpGetScheme {
    #[serde(rename = "HTTP")]
    Http,
    #[serde(rename = "HTTPS")]
    Https,
}
impl ::std::convert::From<&Self> for ContainerAppProbeHttpGetScheme {
    fn from(value: &ContainerAppProbeHttpGetScheme) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerAppProbeHttpGetScheme {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Http => f.write_str("HTTP"),
            Self::Https => f.write_str("HTTPS"),
        }
    }
}
impl ::std::str::FromStr for ContainerAppProbeHttpGetScheme {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerAppProbeHttpGetScheme {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContainerAppProbeHttpGetScheme {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContainerAppProbeHttpGetScheme {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///TCPSocket specifies an action involving a TCP port. TCP hooks not yet supported.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "TCPSocket specifies an action involving a TCP port. TCP hooks not yet supported.",
///  "type": "object",
///  "required": [
///    "port"
///  ],
///  "properties": {
///    "host": {
///      "description": "Optional: Host name to connect to, defaults to the pod IP.",
///      "type": "string"
///    },
///    "port": {
///      "description": "Number or name of the port to access on the container. Number must be in the range 1 to 65535. Name must be an IANA_SVC_NAME.",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppProbeTcpSocket {
    ///Optional: Host name to connect to, defaults to the pod IP.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub host: ::std::option::Option<::std::string::String>,
    ///Number or name of the port to access on the container. Number must be in the range 1 to 65535. Name must be an IANA_SVC_NAME.
    pub port: i32,
}
impl ::std::convert::From<&ContainerAppProbeTcpSocket> for ContainerAppProbeTcpSocket {
    fn from(value: &ContainerAppProbeTcpSocket) -> Self {
        value.clone()
    }
}
///The type of probe.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of probe.",
///  "type": "string",
///  "enum": [
///    "Liveness",
///    "Readiness",
///    "Startup"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum ContainerAppProbeType {
    Liveness,
    Readiness,
    Startup,
}
impl ::std::convert::From<&Self> for ContainerAppProbeType {
    fn from(value: &ContainerAppProbeType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerAppProbeType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Liveness => f.write_str("Liveness"),
            Self::Readiness => f.write_str("Readiness"),
            Self::Startup => f.write_str("Startup"),
        }
    }
}
impl ::std::str::FromStr for ContainerAppProbeType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "liveness" => Ok(Self::Liveness),
            "readiness" => Ok(Self::Readiness),
            "startup" => Ok(Self::Startup),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerAppProbeType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ContainerAppProbeType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ContainerAppProbeType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///ContainerApp resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "ContainerApp resource specific properties",
///  "type": "object",
///  "properties": {
///    "configuration": {
///      "$ref": "#/components/schemas/Configuration"
///    },
///    "customDomainVerificationId": {
///      "description": "Id used to verify domain name ownership",
///      "readOnly": true,
///      "type": "string"
///    },
///    "environmentId": {
///      "description": "Resource ID of environment.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "eventStreamEndpoint": {
///      "description": "The endpoint of the eventstream of the container app.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "latestReadyRevisionName": {
///      "description": "Name of the latest ready revision of the Container App.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "latestRevisionFqdn": {
///      "description": "Fully Qualified Domain Name of the latest revision of the Container App.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "latestRevisionName": {
///      "description": "Name of the latest revision of the Container App.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "managedEnvironmentId": {
///      "description": "Deprecated. Resource ID of the Container App's environment.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "outboundIpAddresses": {
///      "description": "Outbound IP Addresses for container app.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the Container App.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "InProgress",
///        "Succeeded",
///        "Failed",
///        "Canceled",
///        "Deleting"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ContainerAppProvisioningState"
///      }
///    },
///    "runningStatus": {
///      "description": "Running status of the Container App.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Progressing",
///        "Running",
///        "Stopped",
///        "Suspended",
///        "Ready"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ContainerAppRunningStatus",
///        "values": [
///          {
///            "description": "Container App is transitioning between Stopped and Running states.",
///            "value": "Progressing"
///          },
///          {
///            "description": "Container App is in Running state.",
///            "value": "Running"
///          },
///          {
///            "description": "Container App is in Stopped state.",
///            "value": "Stopped"
///          },
///          {
///            "description": "Container App Job is in Suspended state.",
///            "value": "Suspended"
///          },
///          {
///            "description": "Container App Job is in Ready state.",
///            "value": "Ready"
///          }
///        ]
///      }
///    },
///    "template": {
///      "$ref": "#/components/schemas/Template"
///    },
///    "workloadProfileName": {
///      "$ref": "#/components/schemas/WorkloadProfileName"
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub configuration: ::std::option::Option<Configuration>,
    ///Id used to verify domain name ownership
    #[serde(
        rename = "customDomainVerificationId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain_verification_id: ::std::option::Option<::std::string::String>,
    ///Resource ID of environment.
    #[serde(
        rename = "environmentId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub environment_id: ::std::option::Option<::std::string::String>,
    ///The endpoint of the eventstream of the container app.
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_stream_endpoint: ::std::option::Option<::std::string::String>,
    ///Name of the latest ready revision of the Container App.
    #[serde(
        rename = "latestReadyRevisionName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub latest_ready_revision_name: ::std::option::Option<::std::string::String>,
    ///Fully Qualified Domain Name of the latest revision of the Container App.
    #[serde(
        rename = "latestRevisionFqdn",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub latest_revision_fqdn: ::std::option::Option<::std::string::String>,
    ///Name of the latest revision of the Container App.
    #[serde(
        rename = "latestRevisionName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub latest_revision_name: ::std::option::Option<::std::string::String>,
    ///Deprecated. Resource ID of the Container App's environment.
    #[serde(
        rename = "managedEnvironmentId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_environment_id: ::std::option::Option<::std::string::String>,
    ///Outbound IP Addresses for container app.
    #[serde(
        rename = "outboundIpAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub outbound_ip_addresses: ::std::vec::Vec<::std::string::String>,
    ///Provisioning state of the Container App.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        ContainerAppPropertiesProvisioningState,
    >,
    ///Running status of the Container App.
    #[serde(
        rename = "runningStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub running_status: ::std::option::Option<ContainerAppPropertiesRunningStatus>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub template: ::std::option::Option<Template>,
    #[serde(
        rename = "workloadProfileName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workload_profile_name: ::std::option::Option<WorkloadProfileName>,
}
impl ::std::convert::From<&ContainerAppProperties> for ContainerAppProperties {
    fn from(value: &ContainerAppProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ContainerAppProperties {
    fn default() -> Self {
        Self {
            configuration: Default::default(),
            custom_domain_verification_id: Default::default(),
            environment_id: Default::default(),
            event_stream_endpoint: Default::default(),
            latest_ready_revision_name: Default::default(),
            latest_revision_fqdn: Default::default(),
            latest_revision_name: Default::default(),
            managed_environment_id: Default::default(),
            outbound_ip_addresses: Default::default(),
            provisioning_state: Default::default(),
            running_status: Default::default(),
            template: Default::default(),
            workload_profile_name: Default::default(),
        }
    }
}
///Provisioning state of the Container App.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the Container App.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "InProgress",
///    "Succeeded",
///    "Failed",
///    "Canceled",
///    "Deleting"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ContainerAppProvisioningState"
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
pub enum ContainerAppPropertiesProvisioningState {
    InProgress,
    Succeeded,
    Failed,
    Canceled,
    Deleting,
}
impl ::std::convert::From<&Self> for ContainerAppPropertiesProvisioningState {
    fn from(value: &ContainerAppPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerAppPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::InProgress => f.write_str("InProgress"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
            Self::Deleting => f.write_str("Deleting"),
        }
    }
}
impl ::std::str::FromStr for ContainerAppPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "inprogress" => Ok(Self::InProgress),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            "deleting" => Ok(Self::Deleting),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerAppPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for ContainerAppPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for ContainerAppPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Running status of the Container App.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Running status of the Container App.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Progressing",
///    "Running",
///    "Stopped",
///    "Suspended",
///    "Ready"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ContainerAppRunningStatus",
///    "values": [
///      {
///        "description": "Container App is transitioning between Stopped and Running states.",
///        "value": "Progressing"
///      },
///      {
///        "description": "Container App is in Running state.",
///        "value": "Running"
///      },
///      {
///        "description": "Container App is in Stopped state.",
///        "value": "Stopped"
///      },
///      {
///        "description": "Container App Job is in Suspended state.",
///        "value": "Suspended"
///      },
///      {
///        "description": "Container App Job is in Ready state.",
///        "value": "Ready"
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
pub enum ContainerAppPropertiesRunningStatus {
    Progressing,
    Running,
    Stopped,
    Suspended,
    Ready,
}
impl ::std::convert::From<&Self> for ContainerAppPropertiesRunningStatus {
    fn from(value: &ContainerAppPropertiesRunningStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ContainerAppPropertiesRunningStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Progressing => f.write_str("Progressing"),
            Self::Running => f.write_str("Running"),
            Self::Stopped => f.write_str("Stopped"),
            Self::Suspended => f.write_str("Suspended"),
            Self::Ready => f.write_str("Ready"),
        }
    }
}
impl ::std::str::FromStr for ContainerAppPropertiesRunningStatus {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "progressing" => Ok(Self::Progressing),
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            "suspended" => Ok(Self::Suspended),
            "ready" => Ok(Self::Ready),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerAppPropertiesRunningStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for ContainerAppPropertiesRunningStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for ContainerAppPropertiesRunningStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Container App Secret.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Secret.",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "description": "Resource ID of a managed identity to authenticate with Azure Key Vault, or System to use a system-assigned identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "keyVaultUrl": {
///      "description": "Azure Key Vault URL pointing to the secret referenced by the container app.",
///      "readOnly": true,
///      "type": "string"
///    },
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
pub struct ContainerAppSecret {
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
impl ::std::convert::From<&ContainerAppSecret> for ContainerAppSecret {
    fn from(value: &ContainerAppSecret) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ContainerAppSecret {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            key_vault_url: Default::default(),
            name: Default::default(),
            value: Default::default(),
        }
    }
}
///Container App container resource requirements.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container resource requirements.",
///  "type": "object",
///  "properties": {
///    "cpu": {
///      "description": "Required CPU in cores, e.g. 0.5",
///      "type": "number",
///      "format": "double"
///    },
///    "ephemeralStorage": {
///      "description": "Ephemeral Storage, e.g. \"1Gi\"",
///      "readOnly": true,
///      "type": "string"
///    },
///    "memory": {
///      "description": "Required memory, e.g. \"250Mb\"",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerResources {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cpu: ::std::option::Option<f64>,
    ///Ephemeral Storage, e.g. "1Gi"
    #[serde(
        rename = "ephemeralStorage",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ephemeral_storage: ::std::option::Option<::std::string::String>,
    ///Required memory, e.g. "250Mb"
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub memory: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ContainerResources> for ContainerResources {
    fn from(value: &ContainerResources) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ContainerResources {
    fn default() -> Self {
        Self {
            cpu: Default::default(),
            ephemeral_storage: Default::default(),
            memory: Default::default(),
        }
    }
}
///Cross-Origin-Resource-Sharing policy
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Cross-Origin-Resource-Sharing policy",
///  "type": "object",
///  "required": [
///    "allowedOrigins"
///  ],
///  "properties": {
///    "allowCredentials": {
///      "description": "Specifies whether the resource allows credentials",
///      "type": "boolean"
///    },
///    "allowedHeaders": {
///      "description": "Specifies the content for the access-control-allow-headers header",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "allowedMethods": {
///      "description": "Specifies the content for the access-control-allow-methods header",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "allowedOrigins": {
///      "description": "Specifies the content for the access-control-allow-origins header",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "exposeHeaders": {
///      "description": "Specifies the content for the access-control-expose-headers header ",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "maxAge": {
///      "description": "Specifies the content for the access-control-max-age header",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CorsPolicy {
    ///Specifies whether the resource allows credentials
    #[serde(
        rename = "allowCredentials",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_credentials: ::std::option::Option<bool>,
    ///Specifies the content for the access-control-allow-headers header
    #[serde(
        rename = "allowedHeaders",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allowed_headers: ::std::vec::Vec<::std::string::String>,
    ///Specifies the content for the access-control-allow-methods header
    #[serde(
        rename = "allowedMethods",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allowed_methods: ::std::vec::Vec<::std::string::String>,
    ///Specifies the content for the access-control-allow-origins header
    #[serde(rename = "allowedOrigins")]
    pub allowed_origins: ::std::vec::Vec<::std::string::String>,
    ///Specifies the content for the access-control-expose-headers header
    #[serde(
        rename = "exposeHeaders",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expose_headers: ::std::vec::Vec<::std::string::String>,
    ///Specifies the content for the access-control-max-age header
    #[serde(
        rename = "maxAge",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_age: ::std::option::Option<i32>,
}
impl ::std::convert::From<&CorsPolicy> for CorsPolicy {
    fn from(value: &CorsPolicy) -> Self {
        value.clone()
    }
}
///Custom Domain of a Container App
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Custom Domain of a Container App",
///  "type": "object",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "bindingType": {
///      "description": "Custom Domain binding type.",
///      "type": "string",
///      "enum": [
///        "Disabled",
///        "SniEnabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "bindingType"
///      }
///    },
///    "certificateId": {
///      "description": "Resource Id of the Certificate to be bound to this hostname. Must exist in the Managed Environment.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Hostname.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CustomDomain {
    ///Custom Domain binding type.
    #[serde(
        rename = "bindingType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub binding_type: ::std::option::Option<CustomDomainBindingType>,
    ///Resource Id of the Certificate to be bound to this hostname. Must exist in the Managed Environment.
    #[serde(
        rename = "certificateId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificate_id: ::std::option::Option<::std::string::String>,
    ///Hostname.
    pub name: ::std::string::String,
}
impl ::std::convert::From<&CustomDomain> for CustomDomain {
    fn from(value: &CustomDomain) -> Self {
        value.clone()
    }
}
///Custom Domain binding type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Custom Domain binding type.",
///  "type": "string",
///  "enum": [
///    "Disabled",
///    "SniEnabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "bindingType"
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
pub enum CustomDomainBindingType {
    Disabled,
    SniEnabled,
}
impl ::std::convert::From<&Self> for CustomDomainBindingType {
    fn from(value: &CustomDomainBindingType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CustomDomainBindingType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Disabled => f.write_str("Disabled"),
            Self::SniEnabled => f.write_str("SniEnabled"),
        }
    }
}
impl ::std::str::FromStr for CustomDomainBindingType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "snienabled" => Ok(Self::SniEnabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CustomDomainBindingType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CustomDomainBindingType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CustomDomainBindingType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Custom domain analysis.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Custom domain analysis.",
///  "type": "object",
///  "properties": {
///    "aRecords": {
///      "description": "A records visible for this hostname.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "alternateCNameRecords": {
///      "description": "Alternate CName records visible for this hostname.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "alternateTxtRecords": {
///      "description": "Alternate TXT records visible for this hostname.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "cNameRecords": {
///      "description": "CName records visible for this hostname.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "conflictWithEnvironmentCustomDomain": {
///      "description": "<code>true</code> if there is a conflict on the Container App's managed environment level custom domain; otherwise, <code>false</code>.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "conflictingContainerAppResourceId": {
///      "description": "Name of the conflicting Container App on the Managed Environment if it's within the same subscription.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "customDomainVerificationFailureInfo": {
///      "description": "Raw failure information if DNS verification fails.",
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
///    },
///    "customDomainVerificationTest": {
///      "description": "DNS verification test result.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Passed",
///        "Failed",
///        "Skipped"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "DnsVerificationTestResult"
///      }
///    },
///    "hasConflictOnManagedEnvironment": {
///      "description": "<code>true</code> if there is a conflict on the Container App's managed environment; otherwise, <code>false</code>.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "hostName": {
///      "description": "Host name that was analyzed",
///      "readOnly": true,
///      "type": "string"
///    },
///    "isHostnameAlreadyVerified": {
///      "description": "<code>true</code> if hostname is already verified; otherwise, <code>false</code>.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "txtRecords": {
///      "description": "TXT records visible for this hostname.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CustomHostnameAnalysisResult {
    ///A records visible for this hostname.
    #[serde(
        rename = "aRecords",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub a_records: ::std::vec::Vec<::std::string::String>,
    ///Alternate CName records visible for this hostname.
    #[serde(
        rename = "alternateCNameRecords",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub alternate_c_name_records: ::std::vec::Vec<::std::string::String>,
    ///Alternate TXT records visible for this hostname.
    #[serde(
        rename = "alternateTxtRecords",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub alternate_txt_records: ::std::vec::Vec<::std::string::String>,
    ///CName records visible for this hostname.
    #[serde(
        rename = "cNameRecords",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub c_name_records: ::std::vec::Vec<::std::string::String>,
    ///<code>true</code> if there is a conflict on the Container App's managed environment level custom domain; otherwise, <code>false</code>.
    #[serde(
        rename = "conflictWithEnvironmentCustomDomain",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub conflict_with_environment_custom_domain: ::std::option::Option<bool>,
    ///Name of the conflicting Container App on the Managed Environment if it's within the same subscription.
    #[serde(
        rename = "conflictingContainerAppResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub conflicting_container_app_resource_id: ::std::option::Option<
        ::std::string::String,
    >,
    #[serde(
        rename = "customDomainVerificationFailureInfo",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain_verification_failure_info: ::std::option::Option<
        CustomHostnameAnalysisResultCustomDomainVerificationFailureInfo,
    >,
    ///DNS verification test result.
    #[serde(
        rename = "customDomainVerificationTest",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain_verification_test: ::std::option::Option<
        CustomHostnameAnalysisResultCustomDomainVerificationTest,
    >,
    ///<code>true</code> if there is a conflict on the Container App's managed environment; otherwise, <code>false</code>.
    #[serde(
        rename = "hasConflictOnManagedEnvironment",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_conflict_on_managed_environment: ::std::option::Option<bool>,
    ///Host name that was analyzed
    #[serde(
        rename = "hostName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub host_name: ::std::option::Option<::std::string::String>,
    ///<code>true</code> if hostname is already verified; otherwise, <code>false</code>.
    #[serde(
        rename = "isHostnameAlreadyVerified",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_hostname_already_verified: ::std::option::Option<bool>,
    ///TXT records visible for this hostname.
    #[serde(
        rename = "txtRecords",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub txt_records: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&CustomHostnameAnalysisResult>
for CustomHostnameAnalysisResult {
    fn from(value: &CustomHostnameAnalysisResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CustomHostnameAnalysisResult {
    fn default() -> Self {
        Self {
            a_records: Default::default(),
            alternate_c_name_records: Default::default(),
            alternate_txt_records: Default::default(),
            c_name_records: Default::default(),
            conflict_with_environment_custom_domain: Default::default(),
            conflicting_container_app_resource_id: Default::default(),
            custom_domain_verification_failure_info: Default::default(),
            custom_domain_verification_test: Default::default(),
            has_conflict_on_managed_environment: Default::default(),
            host_name: Default::default(),
            is_hostname_already_verified: Default::default(),
            txt_records: Default::default(),
        }
    }
}
///Raw failure information if DNS verification fails.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Raw failure information if DNS verification fails.",
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
pub struct CustomHostnameAnalysisResultCustomDomainVerificationFailureInfo {
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
    pub details: ::std::vec::Vec<
        CustomHostnameAnalysisResultCustomDomainVerificationFailureInfoDetailsItem,
    >,
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
impl ::std::convert::From<
    &CustomHostnameAnalysisResultCustomDomainVerificationFailureInfo,
> for CustomHostnameAnalysisResultCustomDomainVerificationFailureInfo {
    fn from(
        value: &CustomHostnameAnalysisResultCustomDomainVerificationFailureInfo,
    ) -> Self {
        value.clone()
    }
}
impl ::std::default::Default
for CustomHostnameAnalysisResultCustomDomainVerificationFailureInfo {
    fn default() -> Self {
        Self {
            code: Default::default(),
            details: Default::default(),
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
pub struct CustomHostnameAnalysisResultCustomDomainVerificationFailureInfoDetailsItem {
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
impl ::std::convert::From<
    &CustomHostnameAnalysisResultCustomDomainVerificationFailureInfoDetailsItem,
> for CustomHostnameAnalysisResultCustomDomainVerificationFailureInfoDetailsItem {
    fn from(
        value: &CustomHostnameAnalysisResultCustomDomainVerificationFailureInfoDetailsItem,
    ) -> Self {
        value.clone()
    }
}
impl ::std::default::Default
for CustomHostnameAnalysisResultCustomDomainVerificationFailureInfoDetailsItem {
    fn default() -> Self {
        Self {
            code: Default::default(),
            message: Default::default(),
            target: Default::default(),
        }
    }
}
///DNS verification test result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "DNS verification test result.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Passed",
///    "Failed",
///    "Skipped"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "DnsVerificationTestResult"
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
pub enum CustomHostnameAnalysisResultCustomDomainVerificationTest {
    Passed,
    Failed,
    Skipped,
}
impl ::std::convert::From<&Self>
for CustomHostnameAnalysisResultCustomDomainVerificationTest {
    fn from(value: &CustomHostnameAnalysisResultCustomDomainVerificationTest) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CustomHostnameAnalysisResultCustomDomainVerificationTest {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Passed => f.write_str("Passed"),
            Self::Failed => f.write_str("Failed"),
            Self::Skipped => f.write_str("Skipped"),
        }
    }
}
impl ::std::str::FromStr for CustomHostnameAnalysisResultCustomDomainVerificationTest {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
for CustomHostnameAnalysisResultCustomDomainVerificationTest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for CustomHostnameAnalysisResultCustomDomainVerificationTest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for CustomHostnameAnalysisResultCustomDomainVerificationTest {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Container App container Custom scaling rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container Custom scaling rule.",
///  "type": "object",
///  "properties": {
///    "auth": {
///      "description": "Authentication secrets for the custom scale rule.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScaleRuleAuth"
///      },
///      "x-ms-identifiers": [
///        "triggerParameter"
///      ]
///    },
///    "identity": {
///      "description": "The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.",
///      "type": "string"
///    },
///    "metadata": {
///      "description": "Metadata properties to describe custom scale rule.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "Type of the custom scale rule\neg: azure-servicebus, redis etc.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CustomScaleRule {
    ///Authentication secrets for the custom scale rule.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auth: ::std::vec::Vec<ScaleRuleAuth>,
    ///The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///Metadata properties to describe custom scale rule.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::std::collections::HashMap<
        ::std::string::String,
        ::std::string::String,
    >,
    /**Type of the custom scale rule
eg: azure-servicebus, redis etc.*/
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CustomScaleRule> for CustomScaleRule {
    fn from(value: &CustomScaleRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CustomScaleRule {
    fn default() -> Self {
        Self {
            auth: Default::default(),
            identity: Default::default(),
            metadata: Default::default(),
            type_: Default::default(),
        }
    }
}
///Container App Dapr configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Dapr configuration.",
///  "type": "object",
///  "properties": {
///    "appId": {
///      "description": "Dapr application identifier",
///      "type": "string"
///    },
///    "appPort": {
///      "description": "Tells Dapr which port your application is listening on",
///      "type": "integer",
///      "format": "int32"
///    },
///    "appProtocol": {
///      "description": "Tells Dapr which protocol your application is using. Valid options are http and grpc. Default is http",
///      "default": "http",
///      "type": "string",
///      "enum": [
///        "http",
///        "grpc"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "appProtocol"
///      }
///    },
///    "enableApiLogging": {
///      "description": "Enables API logging for the Dapr sidecar",
///      "type": "boolean"
///    },
///    "enabled": {
///      "description": "Boolean indicating if the Dapr side car is enabled",
///      "default": false,
///      "type": "boolean"
///    },
///    "httpMaxRequestSize": {
///      "description": "Increasing max size of request body http and grpc servers parameter in MB to handle uploading of big files. Default is 4 MB.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "httpReadBufferSize": {
///      "description": "Dapr max size of http header read buffer in KB to handle when sending multi-KB headers. Default is 65KB.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "logLevel": {
///      "description": "Sets the log level for the Dapr sidecar. Allowed values are debug, info, warn, error. Default is info.",
///      "type": "string",
///      "enum": [
///        "info",
///        "debug",
///        "warn",
///        "error"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "logLevel"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Dapr {
    ///Dapr application identifier
    #[serde(
        rename = "appId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub app_id: ::std::option::Option<::std::string::String>,
    ///Tells Dapr which port your application is listening on
    #[serde(
        rename = "appPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub app_port: ::std::option::Option<i32>,
    ///Tells Dapr which protocol your application is using. Valid options are http and grpc. Default is http
    #[serde(
        rename = "appProtocol",
        default = "defaults::dapr_app_protocol",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub app_protocol: DaprAppProtocol,
    ///Enables API logging for the Dapr sidecar
    #[serde(
        rename = "enableApiLogging",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_api_logging: ::std::option::Option<bool>,
    ///Boolean indicating if the Dapr side car is enabled
    #[serde(
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: bool,
    ///Increasing max size of request body http and grpc servers parameter in MB to handle uploading of big files. Default is 4 MB.
    #[serde(
        rename = "httpMaxRequestSize",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub http_max_request_size: ::std::option::Option<i32>,
    ///Dapr max size of http header read buffer in KB to handle when sending multi-KB headers. Default is 65KB.
    #[serde(
        rename = "httpReadBufferSize",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub http_read_buffer_size: ::std::option::Option<i32>,
    ///Sets the log level for the Dapr sidecar. Allowed values are debug, info, warn, error. Default is info.
    #[serde(
        rename = "logLevel",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub log_level: ::std::option::Option<DaprLogLevel>,
}
impl ::std::convert::From<&Dapr> for Dapr {
    fn from(value: &Dapr) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Dapr {
    fn default() -> Self {
        Self {
            app_id: Default::default(),
            app_port: Default::default(),
            app_protocol: defaults::dapr_app_protocol(),
            enable_api_logging: Default::default(),
            enabled: Default::default(),
            http_max_request_size: Default::default(),
            http_read_buffer_size: Default::default(),
            log_level: Default::default(),
        }
    }
}
///Tells Dapr which protocol your application is using. Valid options are http and grpc. Default is http
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tells Dapr which protocol your application is using. Valid options are http and grpc. Default is http",
///  "default": "http",
///  "type": "string",
///  "enum": [
///    "http",
///    "grpc"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "appProtocol"
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
pub enum DaprAppProtocol {
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "grpc")]
    Grpc,
}
impl ::std::convert::From<&Self> for DaprAppProtocol {
    fn from(value: &DaprAppProtocol) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DaprAppProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Http => f.write_str("http"),
            Self::Grpc => f.write_str("grpc"),
        }
    }
}
impl ::std::str::FromStr for DaprAppProtocol {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "http" => Ok(Self::Http),
            "grpc" => Ok(Self::Grpc),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DaprAppProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DaprAppProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DaprAppProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for DaprAppProtocol {
    fn default() -> Self {
        DaprAppProtocol::Http
    }
}
///Sets the log level for the Dapr sidecar. Allowed values are debug, info, warn, error. Default is info.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sets the log level for the Dapr sidecar. Allowed values are debug, info, warn, error. Default is info.",
///  "type": "string",
///  "enum": [
///    "info",
///    "debug",
///    "warn",
///    "error"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "logLevel"
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
pub enum DaprLogLevel {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "error")]
    Error,
}
impl ::std::convert::From<&Self> for DaprLogLevel {
    fn from(value: &DaprLogLevel) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DaprLogLevel {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Info => f.write_str("info"),
            Self::Debug => f.write_str("debug"),
            Self::Warn => f.write_str("warn"),
            Self::Error => f.write_str("error"),
        }
    }
}
impl ::std::str::FromStr for DaprLogLevel {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DaprLogLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DaprLogLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DaprLogLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///Container App container environment variable.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container environment variable.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "Environment variable name.",
///      "type": "string"
///    },
///    "secretRef": {
///      "description": "Name of the Container App secret from which to pull the environment variable value.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Non-secret environment variable value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EnvironmentVar {
    ///Environment variable name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Name of the Container App secret from which to pull the environment variable value.
    #[serde(
        rename = "secretRef",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secret_ref: ::std::option::Option<::std::string::String>,
    ///Non-secret environment variable value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EnvironmentVar> for EnvironmentVar {
    fn from(value: &EnvironmentVar) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EnvironmentVar {
    fn default() -> Self {
        Self {
            name: Default::default(),
            secret_ref: Default::default(),
            value: Default::default(),
        }
    }
}
///The complex type of the extended location.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The complex type of the extended location.",
///  "type": "object",
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
///The type of extendedLocation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of extendedLocation.",
///  "type": "string",
///  "enum": [
///    "CustomLocation"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum ExtendedLocationType {
    CustomLocation,
}
impl ::std::convert::From<&Self> for ExtendedLocationType {
    fn from(value: &ExtendedLocationType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ExtendedLocationType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::CustomLocation => f.write_str("CustomLocation"),
        }
    }
}
impl ::std::str::FromStr for ExtendedLocationType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "customlocation" => Ok(Self::CustomLocation),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExtendedLocationType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Container App container Http scaling rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container Http scaling rule.",
///  "type": "object",
///  "properties": {
///    "auth": {
///      "description": "Authentication secrets for the custom scale rule.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScaleRuleAuth"
///      },
///      "x-ms-identifiers": [
///        "triggerParameter"
///      ]
///    },
///    "identity": {
///      "description": "The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.",
///      "type": "string"
///    },
///    "metadata": {
///      "description": "Metadata properties to describe http scale rule.",
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
pub struct HttpScaleRule {
    ///Authentication secrets for the custom scale rule.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auth: ::std::vec::Vec<ScaleRuleAuth>,
    ///The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///Metadata properties to describe http scale rule.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::std::collections::HashMap<
        ::std::string::String,
        ::std::string::String,
    >,
}
impl ::std::convert::From<&HttpScaleRule> for HttpScaleRule {
    fn from(value: &HttpScaleRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for HttpScaleRule {
    fn default() -> Self {
        Self {
            auth: Default::default(),
            identity: Default::default(),
            metadata: Default::default(),
        }
    }
}
///Optional settings for a Managed Identity that is assigned to the Container App.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Optional settings for a Managed Identity that is assigned to the Container App.",
///  "type": "object",
///  "required": [
///    "identity"
///  ],
///  "properties": {
///    "identity": {
///      "description": "The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.",
///      "type": "string"
///    },
///    "lifecycle": {
///      "description": "Use to select the lifecycle stages of a Container App during which the Managed Identity should be available.",
///      "default": "All",
///      "type": "string",
///      "enum": [
///        "Init",
///        "Main",
///        "None",
///        "All"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "IdentitySettingsLifeCycle"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IdentitySettings {
    ///The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.
    pub identity: ::std::string::String,
    ///Use to select the lifecycle stages of a Container App during which the Managed Identity should be available.
    #[serde(
        default = "defaults::identity_settings_lifecycle",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lifecycle: IdentitySettingsLifecycle,
}
impl ::std::convert::From<&IdentitySettings> for IdentitySettings {
    fn from(value: &IdentitySettings) -> Self {
        value.clone()
    }
}
///Use to select the lifecycle stages of a Container App during which the Managed Identity should be available.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Use to select the lifecycle stages of a Container App during which the Managed Identity should be available.",
///  "default": "All",
///  "type": "string",
///  "enum": [
///    "Init",
///    "Main",
///    "None",
///    "All"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IdentitySettingsLifeCycle"
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
pub enum IdentitySettingsLifecycle {
    Init,
    Main,
    None,
    All,
}
impl ::std::convert::From<&Self> for IdentitySettingsLifecycle {
    fn from(value: &IdentitySettingsLifecycle) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IdentitySettingsLifecycle {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Init => f.write_str("Init"),
            Self::Main => f.write_str("Main"),
            Self::None => f.write_str("None"),
            Self::All => f.write_str("All"),
        }
    }
}
impl ::std::str::FromStr for IdentitySettingsLifecycle {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "init" => Ok(Self::Init),
            "main" => Ok(Self::Main),
            "none" => Ok(Self::None),
            "all" => Ok(Self::All),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IdentitySettingsLifecycle {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IdentitySettingsLifecycle {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IdentitySettingsLifecycle {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for IdentitySettingsLifecycle {
    fn default() -> Self {
        IdentitySettingsLifecycle::All
    }
}
///Container App Ingress configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Ingress configuration.",
///  "type": "object",
///  "properties": {
///    "additionalPortMappings": {
///      "description": "Settings to expose additional ports on container app",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IngressPortMapping"
///      },
///      "x-ms-identifiers": [
///        "targetPort"
///      ]
///    },
///    "allowInsecure": {
///      "description": "Bool indicating if HTTP connections to is allowed. If set to false HTTP connections are automatically redirected to HTTPS connections",
///      "default": false,
///      "type": "boolean"
///    },
///    "clientCertificateMode": {
///      "description": "Client certificate mode for mTLS authentication. Ignore indicates server drops client certificate on forwarding. Accept indicates server forwards client certificate but does not require a client certificate. Require indicates server requires a client certificate.",
///      "type": "string",
///      "enum": [
///        "ignore",
///        "accept",
///        "require"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "IngressClientCertificateMode"
///      }
///    },
///    "corsPolicy": {
///      "$ref": "#/components/schemas/CorsPolicy"
///    },
///    "customDomains": {
///      "description": "custom domain bindings for Container Apps' hostnames.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CustomDomain"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "exposedPort": {
///      "description": "Exposed Port in containers for TCP traffic from ingress",
///      "type": "integer",
///      "format": "int32"
///    },
///    "external": {
///      "description": "Bool indicating if app exposes an external http endpoint",
///      "default": false,
///      "type": "boolean"
///    },
///    "fqdn": {
///      "description": "Hostname.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "ipSecurityRestrictions": {
///      "description": "Rules to restrict incoming IP address.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IpSecurityRestrictionRule"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "stickySessions": {
///      "description": "Sticky Sessions for Single Revision Mode",
///      "type": "object",
///      "properties": {
///        "affinity": {
///          "description": "Sticky Session Affinity",
///          "type": "string",
///          "enum": [
///            "sticky",
///            "none"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "affinity"
///          }
///        }
///      }
///    },
///    "targetPort": {
///      "description": "Target Port in containers for traffic from ingress",
///      "type": "integer",
///      "format": "int32"
///    },
///    "traffic": {
///      "description": "Traffic weights for app's revisions",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TrafficWeight"
///      },
///      "x-ms-identifiers": [
///        "revisionName"
///      ]
///    },
///    "transport": {
///      "description": "Ingress transport protocol",
///      "default": "auto",
///      "type": "string",
///      "enum": [
///        "auto",
///        "http",
///        "http2",
///        "tcp"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "IngressTransportMethod"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Ingress {
    ///Settings to expose additional ports on container app
    #[serde(
        rename = "additionalPortMappings",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub additional_port_mappings: ::std::vec::Vec<IngressPortMapping>,
    ///Bool indicating if HTTP connections to is allowed. If set to false HTTP connections are automatically redirected to HTTPS connections
    #[serde(
        rename = "allowInsecure",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_insecure: bool,
    ///Client certificate mode for mTLS authentication. Ignore indicates server drops client certificate on forwarding. Accept indicates server forwards client certificate but does not require a client certificate. Require indicates server requires a client certificate.
    #[serde(
        rename = "clientCertificateMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_certificate_mode: ::std::option::Option<IngressClientCertificateMode>,
    #[serde(
        rename = "corsPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cors_policy: ::std::option::Option<CorsPolicy>,
    ///custom domain bindings for Container Apps' hostnames.
    #[serde(
        rename = "customDomains",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domains: ::std::vec::Vec<CustomDomain>,
    ///Exposed Port in containers for TCP traffic from ingress
    #[serde(
        rename = "exposedPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exposed_port: ::std::option::Option<i32>,
    ///Bool indicating if app exposes an external http endpoint
    #[serde(
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub external: bool,
    ///Hostname.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fqdn: ::std::option::Option<::std::string::String>,
    ///Rules to restrict incoming IP address.
    #[serde(
        rename = "ipSecurityRestrictions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_security_restrictions: ::std::vec::Vec<IpSecurityRestrictionRule>,
    #[serde(
        rename = "stickySessions",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sticky_sessions: ::std::option::Option<IngressStickySessions>,
    ///Target Port in containers for traffic from ingress
    #[serde(
        rename = "targetPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_port: ::std::option::Option<i32>,
    ///Traffic weights for app's revisions
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub traffic: ::std::vec::Vec<TrafficWeight>,
    ///Ingress transport protocol
    #[serde(
        default = "defaults::ingress_transport",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub transport: IngressTransport,
}
impl ::std::convert::From<&Ingress> for Ingress {
    fn from(value: &Ingress) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Ingress {
    fn default() -> Self {
        Self {
            additional_port_mappings: Default::default(),
            allow_insecure: Default::default(),
            client_certificate_mode: Default::default(),
            cors_policy: Default::default(),
            custom_domains: Default::default(),
            exposed_port: Default::default(),
            external: Default::default(),
            fqdn: Default::default(),
            ip_security_restrictions: Default::default(),
            sticky_sessions: Default::default(),
            target_port: Default::default(),
            traffic: Default::default(),
            transport: defaults::ingress_transport(),
        }
    }
}
///Client certificate mode for mTLS authentication. Ignore indicates server drops client certificate on forwarding. Accept indicates server forwards client certificate but does not require a client certificate. Require indicates server requires a client certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Client certificate mode for mTLS authentication. Ignore indicates server drops client certificate on forwarding. Accept indicates server forwards client certificate but does not require a client certificate. Require indicates server requires a client certificate.",
///  "type": "string",
///  "enum": [
///    "ignore",
///    "accept",
///    "require"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IngressClientCertificateMode"
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
pub enum IngressClientCertificateMode {
    #[serde(rename = "ignore")]
    Ignore,
    #[serde(rename = "accept")]
    Accept,
    #[serde(rename = "require")]
    Require,
}
impl ::std::convert::From<&Self> for IngressClientCertificateMode {
    fn from(value: &IngressClientCertificateMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IngressClientCertificateMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Ignore => f.write_str("ignore"),
            Self::Accept => f.write_str("accept"),
            Self::Require => f.write_str("require"),
        }
    }
}
impl ::std::str::FromStr for IngressClientCertificateMode {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "ignore" => Ok(Self::Ignore),
            "accept" => Ok(Self::Accept),
            "require" => Ok(Self::Require),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IngressClientCertificateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IngressClientCertificateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IngressClientCertificateMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Port mappings of container app ingress
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Port mappings of container app ingress",
///  "type": "object",
///  "required": [
///    "external",
///    "targetPort"
///  ],
///  "properties": {
///    "exposedPort": {
///      "description": "Specifies the exposed port for the target port. If not specified, it defaults to target port",
///      "type": "integer",
///      "format": "int32"
///    },
///    "external": {
///      "description": "Specifies whether the app port is accessible outside of the environment",
///      "type": "boolean"
///    },
///    "targetPort": {
///      "description": "Specifies the port user's container listens on",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IngressPortMapping {
    ///Specifies the exposed port for the target port. If not specified, it defaults to target port
    #[serde(
        rename = "exposedPort",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exposed_port: ::std::option::Option<i32>,
    ///Specifies whether the app port is accessible outside of the environment
    pub external: bool,
    ///Specifies the port user's container listens on
    #[serde(rename = "targetPort")]
    pub target_port: i32,
}
impl ::std::convert::From<&IngressPortMapping> for IngressPortMapping {
    fn from(value: &IngressPortMapping) -> Self {
        value.clone()
    }
}
///Sticky Sessions for Single Revision Mode
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sticky Sessions for Single Revision Mode",
///  "type": "object",
///  "properties": {
///    "affinity": {
///      "description": "Sticky Session Affinity",
///      "type": "string",
///      "enum": [
///        "sticky",
///        "none"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "affinity"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IngressStickySessions {
    ///Sticky Session Affinity
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub affinity: ::std::option::Option<IngressStickySessionsAffinity>,
}
impl ::std::convert::From<&IngressStickySessions> for IngressStickySessions {
    fn from(value: &IngressStickySessions) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IngressStickySessions {
    fn default() -> Self {
        Self {
            affinity: Default::default(),
        }
    }
}
///Sticky Session Affinity
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sticky Session Affinity",
///  "type": "string",
///  "enum": [
///    "sticky",
///    "none"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "affinity"
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
pub enum IngressStickySessionsAffinity {
    #[serde(rename = "sticky")]
    Sticky,
    #[serde(rename = "none")]
    None,
}
impl ::std::convert::From<&Self> for IngressStickySessionsAffinity {
    fn from(value: &IngressStickySessionsAffinity) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IngressStickySessionsAffinity {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Sticky => f.write_str("sticky"),
            Self::None => f.write_str("none"),
        }
    }
}
impl ::std::str::FromStr for IngressStickySessionsAffinity {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "sticky" => Ok(Self::Sticky),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IngressStickySessionsAffinity {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IngressStickySessionsAffinity {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IngressStickySessionsAffinity {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Ingress transport protocol
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Ingress transport protocol",
///  "default": "auto",
///  "type": "string",
///  "enum": [
///    "auto",
///    "http",
///    "http2",
///    "tcp"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IngressTransportMethod"
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
pub enum IngressTransport {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "http2")]
    Http2,
    #[serde(rename = "tcp")]
    Tcp,
}
impl ::std::convert::From<&Self> for IngressTransport {
    fn from(value: &IngressTransport) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IngressTransport {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Auto => f.write_str("auto"),
            Self::Http => f.write_str("http"),
            Self::Http2 => f.write_str("http2"),
            Self::Tcp => f.write_str("tcp"),
        }
    }
}
impl ::std::str::FromStr for IngressTransport {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "http" => Ok(Self::Http),
            "http2" => Ok(Self::Http2),
            "tcp" => Ok(Self::Tcp),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IngressTransport {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IngressTransport {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IngressTransport {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for IngressTransport {
    fn default() -> Self {
        IngressTransport::Auto
    }
}
///Container App init container definition
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App init container definition",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/BaseContainer"
///    }
///  ]
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct InitContainer(pub BaseContainer);
impl ::std::ops::Deref for InitContainer {
    type Target = BaseContainer;
    fn deref(&self) -> &BaseContainer {
        &self.0
    }
}
impl ::std::convert::From<InitContainer> for BaseContainer {
    fn from(value: InitContainer) -> Self {
        value.0
    }
}
impl ::std::convert::From<&InitContainer> for InitContainer {
    fn from(value: &InitContainer) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<BaseContainer> for InitContainer {
    fn from(value: BaseContainer) -> Self {
        Self(value)
    }
}
///Rule to restrict incoming IP address.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Rule to restrict incoming IP address.",
///  "type": "object",
///  "required": [
///    "action",
///    "ipAddressRange",
///    "name"
///  ],
///  "properties": {
///    "action": {
///      "description": "Allow or Deny rules to determine for incoming IP. Note: Rules can only consist of ALL Allow or ALL Deny",
///      "type": "string",
///      "enum": [
///        "Allow",
///        "Deny"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "action"
///      }
///    },
///    "description": {
///      "description": "Describe the IP restriction rule that is being sent to the container-app. This is an optional field.",
///      "type": "string"
///    },
///    "ipAddressRange": {
///      "description": "CIDR notation to match incoming IP address",
///      "type": "string"
///    },
///    "name": {
///      "description": "Name for the IP restriction rule.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpSecurityRestrictionRule {
    ///Allow or Deny rules to determine for incoming IP. Note: Rules can only consist of ALL Allow or ALL Deny
    pub action: IpSecurityRestrictionRuleAction,
    ///Describe the IP restriction rule that is being sent to the container-app. This is an optional field.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///CIDR notation to match incoming IP address
    #[serde(rename = "ipAddressRange")]
    pub ip_address_range: ::std::string::String,
    ///Name for the IP restriction rule.
    pub name: ::std::string::String,
}
impl ::std::convert::From<&IpSecurityRestrictionRule> for IpSecurityRestrictionRule {
    fn from(value: &IpSecurityRestrictionRule) -> Self {
        value.clone()
    }
}
///Allow or Deny rules to determine for incoming IP. Note: Rules can only consist of ALL Allow or ALL Deny
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allow or Deny rules to determine for incoming IP. Note: Rules can only consist of ALL Allow or ALL Deny",
///  "type": "string",
///  "enum": [
///    "Allow",
///    "Deny"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "action"
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
pub enum IpSecurityRestrictionRuleAction {
    Allow,
    Deny,
}
impl ::std::convert::From<&Self> for IpSecurityRestrictionRuleAction {
    fn from(value: &IpSecurityRestrictionRuleAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IpSecurityRestrictionRuleAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
            Self::Deny => f.write_str("Deny"),
        }
    }
}
impl ::std::str::FromStr for IpSecurityRestrictionRuleAction {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IpSecurityRestrictionRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for IpSecurityRestrictionRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IpSecurityRestrictionRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Managed service identity (system assigned and/or user assigned identities)
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Managed service identity (system assigned and/or user assigned identities)",
///  "type": "object",
///  "required": [
///    "type"
///  ],
///  "properties": {
///    "principalId": {
///      "description": "The service principal ID of the system assigned identity. This property will only be provided for a system assigned identity.",
///      "readOnly": true,
///      "type": "string",
///      "format": "uuid"
///    },
///    "tenantId": {
///      "description": "The tenant ID of the system assigned identity. This property will only be provided for a system assigned identity.",
///      "readOnly": true,
///      "type": "string",
///      "format": "uuid"
///    },
///    "type": {
///      "$ref": "#/components/schemas/ManagedServiceIdentityType"
///    },
///    "userAssignedIdentities": {
///      "$ref": "#/components/schemas/UserAssignedIdentities"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedServiceIdentity {
    ///The service principal ID of the system assigned identity. This property will only be provided for a system assigned identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::uuid::Uuid>,
    ///The tenant ID of the system assigned identity. This property will only be provided for a system assigned identity.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::uuid::Uuid>,
    #[serde(rename = "type")]
    pub type_: ManagedServiceIdentityType,
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities: ::std::option::Option<UserAssignedIdentities>,
}
impl ::std::convert::From<&ManagedServiceIdentity> for ManagedServiceIdentity {
    fn from(value: &ManagedServiceIdentity) -> Self {
        value.clone()
    }
}
///Type of managed service identity (where both SystemAssigned and UserAssigned types are allowed).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Type of managed service identity (where both SystemAssigned and UserAssigned types are allowed).",
///  "type": "string",
///  "enum": [
///    "None",
///    "SystemAssigned",
///    "UserAssigned",
///    "SystemAssigned,UserAssigned"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
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
pub enum ManagedServiceIdentityType {
    None,
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned,UserAssigned")]
    SystemAssignedUserAssigned,
}
impl ::std::convert::From<&Self> for ManagedServiceIdentityType {
    fn from(value: &ManagedServiceIdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ManagedServiceIdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => {
                f.write_str("SystemAssigned,UserAssigned")
            }
        }
    }
}
impl ::std::str::FromStr for ManagedServiceIdentityType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "systemassigned" => Ok(Self::SystemAssigned),
            "userassigned" => Ok(Self::UserAssigned),
            "systemassigned,userassigned" => Ok(Self::SystemAssignedUserAssigned),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ManagedServiceIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Container App container Azure Queue based scaling rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container Azure Queue based scaling rule.",
///  "type": "object",
///  "properties": {
///    "accountName": {
///      "description": "Storage account name. required if using managed identity to authenticate",
///      "type": "string"
///    },
///    "auth": {
///      "description": "Authentication secrets for the queue scale rule.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScaleRuleAuth"
///      },
///      "x-ms-identifiers": [
///        "triggerParameter"
///      ]
///    },
///    "identity": {
///      "description": "The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.",
///      "type": "string"
///    },
///    "queueLength": {
///      "description": "Queue length.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "queueName": {
///      "description": "Queue name.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct QueueScaleRule {
    ///Storage account name. required if using managed identity to authenticate
    #[serde(
        rename = "accountName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub account_name: ::std::option::Option<::std::string::String>,
    ///Authentication secrets for the queue scale rule.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auth: ::std::vec::Vec<ScaleRuleAuth>,
    ///The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///Queue length.
    #[serde(
        rename = "queueLength",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub queue_length: ::std::option::Option<i32>,
    ///Queue name.
    #[serde(
        rename = "queueName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub queue_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&QueueScaleRule> for QueueScaleRule {
    fn from(value: &QueueScaleRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for QueueScaleRule {
    fn default() -> Self {
        Self {
            account_name: Default::default(),
            auth: Default::default(),
            identity: Default::default(),
            queue_length: Default::default(),
            queue_name: Default::default(),
        }
    }
}
///Container App Private Registry
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Private Registry",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "description": "A Managed Identity to use to authenticate with Azure Container Registry. For user-assigned identities, use the full user-assigned identity Resource ID. For system-assigned identities, use 'system'",
///      "type": "string"
///    },
///    "passwordSecretRef": {
///      "description": "The name of the Secret that contains the registry login password",
///      "type": "string"
///    },
///    "server": {
///      "description": "Container Registry Server",
///      "type": "string"
///    },
///    "username": {
///      "description": "Container Registry Username",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistryCredentials {
    ///A Managed Identity to use to authenticate with Azure Container Registry. For user-assigned identities, use the full user-assigned identity Resource ID. For system-assigned identities, use 'system'
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///The name of the Secret that contains the registry login password
    #[serde(
        rename = "passwordSecretRef",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub password_secret_ref: ::std::option::Option<::std::string::String>,
    ///Container Registry Server
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub server: ::std::option::Option<::std::string::String>,
    ///Container Registry Username
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub username: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistryCredentials> for RegistryCredentials {
    fn from(value: &RegistryCredentials) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistryCredentials {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            password_secret_ref: Default::default(),
            server: Default::default(),
            username: Default::default(),
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
///Container App Runtime configuration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Runtime configuration.",
///  "type": "object",
///  "properties": {
///    "java": {
///      "description": "Java app configuration",
///      "type": "object",
///      "properties": {
///        "enableMetrics": {
///          "description": "Enable jmx core metrics for the java app",
///          "type": "boolean"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Runtime {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub java: ::std::option::Option<RuntimeJava>,
}
impl ::std::convert::From<&Runtime> for Runtime {
    fn from(value: &Runtime) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Runtime {
    fn default() -> Self {
        Self { java: Default::default() }
    }
}
///Java app configuration
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Java app configuration",
///  "type": "object",
///  "properties": {
///    "enableMetrics": {
///      "description": "Enable jmx core metrics for the java app",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RuntimeJava {
    ///Enable jmx core metrics for the java app
    #[serde(
        rename = "enableMetrics",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_metrics: ::std::option::Option<bool>,
}
impl ::std::convert::From<&RuntimeJava> for RuntimeJava {
    fn from(value: &RuntimeJava) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RuntimeJava {
    fn default() -> Self {
        Self {
            enable_metrics: Default::default(),
        }
    }
}
///Container App scaling configurations.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App scaling configurations.",
///  "type": "object",
///  "properties": {
///    "cooldownPeriod": {
///      "description": "Optional. KEDA Cooldown Period in seconds. Defaults to 300 seconds if not set.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "maxReplicas": {
///      "description": "Optional. Maximum number of container replicas. Defaults to 10 if not set.",
///      "default": 10,
///      "type": "integer",
///      "format": "int32"
///    },
///    "minReplicas": {
///      "description": "Optional. Minimum number of container replicas.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "pollingInterval": {
///      "description": "Optional. KEDA Polling Interval in seconds. Defaults to 30 seconds if not set.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "rules": {
///      "description": "Scaling rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScaleRule"
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
pub struct Scale {
    ///Optional. KEDA Cooldown Period in seconds. Defaults to 300 seconds if not set.
    #[serde(
        rename = "cooldownPeriod",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cooldown_period: ::std::option::Option<i32>,
    ///Optional. Maximum number of container replicas. Defaults to 10 if not set.
    #[serde(
        rename = "maxReplicas",
        default = "defaults::default_u64::<i32, 10>",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_replicas: i32,
    ///Optional. Minimum number of container replicas.
    #[serde(
        rename = "minReplicas",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub min_replicas: ::std::option::Option<i32>,
    ///Optional. KEDA Polling Interval in seconds. Defaults to 30 seconds if not set.
    #[serde(
        rename = "pollingInterval",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub polling_interval: ::std::option::Option<i32>,
    ///Scaling rules.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rules: ::std::vec::Vec<ScaleRule>,
}
impl ::std::convert::From<&Scale> for Scale {
    fn from(value: &Scale) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Scale {
    fn default() -> Self {
        Self {
            cooldown_period: Default::default(),
            max_replicas: defaults::default_u64::<i32, 10>(),
            min_replicas: Default::default(),
            polling_interval: Default::default(),
            rules: Default::default(),
        }
    }
}
///Container App container scaling rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container scaling rule.",
///  "type": "object",
///  "properties": {
///    "azureQueue": {
///      "$ref": "#/components/schemas/QueueScaleRule"
///    },
///    "custom": {
///      "$ref": "#/components/schemas/CustomScaleRule"
///    },
///    "http": {
///      "$ref": "#/components/schemas/HttpScaleRule"
///    },
///    "name": {
///      "description": "Scale Rule Name",
///      "type": "string"
///    },
///    "tcp": {
///      "$ref": "#/components/schemas/TcpScaleRule"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScaleRule {
    #[serde(
        rename = "azureQueue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub azure_queue: ::std::option::Option<QueueScaleRule>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom: ::std::option::Option<CustomScaleRule>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub http: ::std::option::Option<HttpScaleRule>,
    ///Scale Rule Name
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
    pub tcp: ::std::option::Option<TcpScaleRule>,
}
impl ::std::convert::From<&ScaleRule> for ScaleRule {
    fn from(value: &ScaleRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ScaleRule {
    fn default() -> Self {
        Self {
            azure_queue: Default::default(),
            custom: Default::default(),
            http: Default::default(),
            name: Default::default(),
            tcp: Default::default(),
        }
    }
}
///Auth Secrets for Scale Rule
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Auth Secrets for Scale Rule",
///  "type": "object",
///  "properties": {
///    "secretRef": {
///      "description": "Name of the secret from which to pull the auth params.",
///      "type": "string"
///    },
///    "triggerParameter": {
///      "description": "Trigger Parameter that uses the secret",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ScaleRuleAuth {
    ///Name of the secret from which to pull the auth params.
    #[serde(
        rename = "secretRef",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secret_ref: ::std::option::Option<::std::string::String>,
    ///Trigger Parameter that uses the secret
    #[serde(
        rename = "triggerParameter",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub trigger_parameter: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ScaleRuleAuth> for ScaleRuleAuth {
    fn from(value: &ScaleRuleAuth) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ScaleRuleAuth {
    fn default() -> Self {
        Self {
            secret_ref: Default::default(),
            trigger_parameter: Default::default(),
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
///Secret to be added to volume.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Secret to be added to volume.",
///  "type": "object",
///  "properties": {
///    "path": {
///      "description": "Path to project secret to. If no path is provided, path defaults to name of secret listed in secretRef.",
///      "type": "string"
///    },
///    "secretRef": {
///      "description": "Name of the Container App secret from which to pull the secret value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretVolumeItem {
    ///Path to project secret to. If no path is provided, path defaults to name of secret listed in secretRef.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub path: ::std::option::Option<::std::string::String>,
    ///Name of the Container App secret from which to pull the secret value.
    #[serde(
        rename = "secretRef",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secret_ref: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SecretVolumeItem> for SecretVolumeItem {
    fn from(value: &SecretVolumeItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretVolumeItem {
    fn default() -> Self {
        Self {
            path: Default::default(),
            secret_ref: Default::default(),
        }
    }
}
///Container App Secrets Collection ARM resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Secrets Collection ARM resource.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "Collection of resources.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ContainerAppSecret"
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
pub struct SecretsCollection {
    ///Collection of resources.
    pub value: ::std::vec::Vec<ContainerAppSecret>,
}
impl ::std::convert::From<&SecretsCollection> for SecretsCollection {
    fn from(value: &SecretsCollection) -> Self {
        value.clone()
    }
}
///Container App to be a dev service
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App to be a dev service",
///  "type": "object",
///  "required": [
///    "type"
///  ],
///  "properties": {
///    "type": {
///      "description": "Dev ContainerApp service type",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Service {
    ///Dev ContainerApp service type
    #[serde(rename = "type")]
    pub type_: ::std::string::String,
}
impl ::std::convert::From<&Service> for Service {
    fn from(value: &Service) -> Self {
        value.clone()
    }
}
///Configuration to bind a ContainerApp to a dev ContainerApp Service
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration to bind a ContainerApp to a dev ContainerApp Service",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "Name of the service bind",
///      "type": "string"
///    },
///    "serviceId": {
///      "description": "Resource id of the target service",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceBind {
    ///Name of the service bind
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Resource id of the target service
    #[serde(
        rename = "serviceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ServiceBind> for ServiceBind {
    fn from(value: &ServiceBind) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceBind {
    fn default() -> Self {
        Self {
            name: Default::default(),
            service_id: Default::default(),
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
///Container App container Tcp scaling rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App container Tcp scaling rule.",
///  "type": "object",
///  "properties": {
///    "auth": {
///      "description": "Authentication secrets for the tcp scale rule.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScaleRuleAuth"
///      },
///      "x-ms-identifiers": [
///        "triggerParameter"
///      ]
///    },
///    "identity": {
///      "description": "The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.",
///      "type": "string"
///    },
///    "metadata": {
///      "description": "Metadata properties to describe tcp scale rule.",
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
pub struct TcpScaleRule {
    ///Authentication secrets for the tcp scale rule.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auth: ::std::vec::Vec<ScaleRuleAuth>,
    ///The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///Metadata properties to describe tcp scale rule.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::std::collections::HashMap<
        ::std::string::String,
        ::std::string::String,
    >,
}
impl ::std::convert::From<&TcpScaleRule> for TcpScaleRule {
    fn from(value: &TcpScaleRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TcpScaleRule {
    fn default() -> Self {
        Self {
            auth: Default::default(),
            identity: Default::default(),
            metadata: Default::default(),
        }
    }
}
/**Container App versioned application definition.
Defines the desired state of an immutable revision.
Any changes to this section Will result in a new revision being created*/
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App versioned application definition.\nDefines the desired state of an immutable revision.\nAny changes to this section Will result in a new revision being created",
///  "type": "object",
///  "properties": {
///    "containers": {
///      "description": "List of container definitions for the Container App.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Container"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "initContainers": {
///      "description": "List of specialized containers that run before app containers.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/InitContainer"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "revisionSuffix": {
///      "description": "User friendly suffix that is appended to the revision name",
///      "type": "string"
///    },
///    "scale": {
///      "$ref": "#/components/schemas/Scale"
///    },
///    "serviceBinds": {
///      "description": "List of container app services bound to the app",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ServiceBind"
///      },
///      "x-ms-identifier": [
///        "name"
///      ]
///    },
///    "terminationGracePeriodSeconds": {
///      "description": "Optional duration in seconds the Container App Instance needs to terminate gracefully. Value must be non-negative integer. The value zero indicates stop immediately via the kill signal (no opportunity to shut down). If this value is nil, the default grace period will be used instead. Set this value longer than the expected cleanup time for your process. Defaults to 30 seconds.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "volumes": {
///      "description": "List of volume definitions for the Container App.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Volume"
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
pub struct Template {
    ///List of container definitions for the Container App.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub containers: ::std::vec::Vec<Container>,
    ///List of specialized containers that run before app containers.
    #[serde(
        rename = "initContainers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub init_containers: ::std::vec::Vec<InitContainer>,
    ///User friendly suffix that is appended to the revision name
    #[serde(
        rename = "revisionSuffix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub revision_suffix: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scale: ::std::option::Option<Scale>,
    ///List of container app services bound to the app
    #[serde(
        rename = "serviceBinds",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_binds: ::std::vec::Vec<ServiceBind>,
    ///Optional duration in seconds the Container App Instance needs to terminate gracefully. Value must be non-negative integer. The value zero indicates stop immediately via the kill signal (no opportunity to shut down). If this value is nil, the default grace period will be used instead. Set this value longer than the expected cleanup time for your process. Defaults to 30 seconds.
    #[serde(
        rename = "terminationGracePeriodSeconds",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub termination_grace_period_seconds: ::std::option::Option<i64>,
    ///List of volume definitions for the Container App.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub volumes: ::std::vec::Vec<Volume>,
}
impl ::std::convert::From<&Template> for Template {
    fn from(value: &Template) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Template {
    fn default() -> Self {
        Self {
            containers: Default::default(),
            init_containers: Default::default(),
            revision_suffix: Default::default(),
            scale: Default::default(),
            service_binds: Default::default(),
            termination_grace_period_seconds: Default::default(),
            volumes: Default::default(),
        }
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
///Traffic weight assigned to a revision
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Traffic weight assigned to a revision",
///  "type": "object",
///  "properties": {
///    "label": {
///      "description": "Associates a traffic label with a revision",
///      "type": "string"
///    },
///    "latestRevision": {
///      "description": "Indicates that the traffic weight belongs to a latest stable revision",
///      "default": false,
///      "type": "boolean"
///    },
///    "revisionName": {
///      "description": "Name of a revision",
///      "type": "string"
///    },
///    "weight": {
///      "description": "Traffic weight assigned to a revision",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TrafficWeight {
    ///Associates a traffic label with a revision
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub label: ::std::option::Option<::std::string::String>,
    ///Indicates that the traffic weight belongs to a latest stable revision
    #[serde(
        rename = "latestRevision",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub latest_revision: bool,
    ///Name of a revision
    #[serde(
        rename = "revisionName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub revision_name: ::std::option::Option<::std::string::String>,
    ///Traffic weight assigned to a revision
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub weight: ::std::option::Option<i32>,
}
impl ::std::convert::From<&TrafficWeight> for TrafficWeight {
    fn from(value: &TrafficWeight) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TrafficWeight {
    fn default() -> Self {
        Self {
            label: Default::default(),
            latest_revision: Default::default(),
            revision_name: Default::default(),
            weight: Default::default(),
        }
    }
}
///The set of user assigned identities associated with the resource. The userAssignedIdentities dictionary keys will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}. The dictionary values can be empty objects ({}) in requests.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "User-Assigned Identities",
///  "description": "The set of user assigned identities associated with the resource. The userAssignedIdentities dictionary keys will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}. The dictionary values can be empty objects ({}) in requests.",
///  "type": "object",
///  "additionalProperties": {
///    "$ref": "#/components/schemas/UserAssignedIdentity"
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct UserAssignedIdentities(
    pub ::std::collections::HashMap<::std::string::String, UserAssignedIdentity>,
);
impl ::std::ops::Deref for UserAssignedIdentities {
    type Target = ::std::collections::HashMap<
        ::std::string::String,
        UserAssignedIdentity,
    >;
    fn deref(
        &self,
    ) -> &::std::collections::HashMap<::std::string::String, UserAssignedIdentity> {
        &self.0
    }
}
impl ::std::convert::From<UserAssignedIdentities>
for ::std::collections::HashMap<::std::string::String, UserAssignedIdentity> {
    fn from(value: UserAssignedIdentities) -> Self {
        value.0
    }
}
impl ::std::convert::From<&UserAssignedIdentities> for UserAssignedIdentities {
    fn from(value: &UserAssignedIdentities) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<
    ::std::collections::HashMap<::std::string::String, UserAssignedIdentity>,
> for UserAssignedIdentities {
    fn from(
        value: ::std::collections::HashMap<::std::string::String, UserAssignedIdentity>,
    ) -> Self {
        Self(value)
    }
}
///User assigned identity properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "User assigned identity properties",
///  "type": "object",
///  "properties": {
///    "clientId": {
///      "description": "The client ID of the assigned identity.",
///      "readOnly": true,
///      "type": "string",
///      "format": "uuid"
///    },
///    "principalId": {
///      "description": "The principal ID of the assigned identity.",
///      "readOnly": true,
///      "type": "string",
///      "format": "uuid"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserAssignedIdentity {
    ///The client ID of the assigned identity.
    #[serde(
        rename = "clientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_id: ::std::option::Option<::uuid::Uuid>,
    ///The principal ID of the assigned identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::uuid::Uuid>,
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
///Volume definitions for the Container App.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Volume definitions for the Container App.",
///  "type": "object",
///  "properties": {
///    "mountOptions": {
///      "description": "Mount options used while mounting the Azure file share or NFS Azure file share. Must be a comma-separated string.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Volume name.",
///      "type": "string"
///    },
///    "secrets": {
///      "description": "List of secrets to be added in volume. If no secrets are provided, all secrets in collection will be added to volume.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SecretVolumeItem"
///      },
///      "x-ms-identifiers": [
///        "secretRef"
///      ]
///    },
///    "storageName": {
///      "description": "Name of storage resource. No need to provide for EmptyDir and Secret.",
///      "type": "string"
///    },
///    "storageType": {
///      "description": "Storage type for the volume. If not provided, use EmptyDir.",
///      "type": "string",
///      "enum": [
///        "AzureFile",
///        "EmptyDir",
///        "Secret",
///        "NfsAzureFile"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "StorageType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Volume {
    ///Mount options used while mounting the Azure file share or NFS Azure file share. Must be a comma-separated string.
    #[serde(
        rename = "mountOptions",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub mount_options: ::std::option::Option<::std::string::String>,
    ///Volume name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///List of secrets to be added in volume. If no secrets are provided, all secrets in collection will be added to volume.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secrets: ::std::vec::Vec<SecretVolumeItem>,
    ///Name of storage resource. No need to provide for EmptyDir and Secret.
    #[serde(
        rename = "storageName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub storage_name: ::std::option::Option<::std::string::String>,
    ///Storage type for the volume. If not provided, use EmptyDir.
    #[serde(
        rename = "storageType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub storage_type: ::std::option::Option<VolumeStorageType>,
}
impl ::std::convert::From<&Volume> for Volume {
    fn from(value: &Volume) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Volume {
    fn default() -> Self {
        Self {
            mount_options: Default::default(),
            name: Default::default(),
            secrets: Default::default(),
            storage_name: Default::default(),
            storage_type: Default::default(),
        }
    }
}
///Volume mount for the Container App.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Volume mount for the Container App.",
///  "type": "object",
///  "properties": {
///    "mountPath": {
///      "description": "Path within the container at which the volume should be mounted.Must not contain ':'.",
///      "type": "string"
///    },
///    "subPath": {
///      "description": "Path within the volume from which the container's volume should be mounted. Defaults to \"\" (volume's root).",
///      "type": "string"
///    },
///    "volumeName": {
///      "description": "This must match the Name of a Volume.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VolumeMount {
    ///Path within the container at which the volume should be mounted.Must not contain ':'.
    #[serde(
        rename = "mountPath",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub mount_path: ::std::option::Option<::std::string::String>,
    ///Path within the volume from which the container's volume should be mounted. Defaults to "" (volume's root).
    #[serde(
        rename = "subPath",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sub_path: ::std::option::Option<::std::string::String>,
    ///This must match the Name of a Volume.
    #[serde(
        rename = "volumeName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub volume_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&VolumeMount> for VolumeMount {
    fn from(value: &VolumeMount) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VolumeMount {
    fn default() -> Self {
        Self {
            mount_path: Default::default(),
            sub_path: Default::default(),
            volume_name: Default::default(),
        }
    }
}
///Storage type for the volume. If not provided, use EmptyDir.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Storage type for the volume. If not provided, use EmptyDir.",
///  "type": "string",
///  "enum": [
///    "AzureFile",
///    "EmptyDir",
///    "Secret",
///    "NfsAzureFile"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "StorageType"
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
pub enum VolumeStorageType {
    AzureFile,
    EmptyDir,
    Secret,
    NfsAzureFile,
}
impl ::std::convert::From<&Self> for VolumeStorageType {
    fn from(value: &VolumeStorageType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VolumeStorageType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AzureFile => f.write_str("AzureFile"),
            Self::EmptyDir => f.write_str("EmptyDir"),
            Self::Secret => f.write_str("Secret"),
            Self::NfsAzureFile => f.write_str("NfsAzureFile"),
        }
    }
}
impl ::std::str::FromStr for VolumeStorageType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "azurefile" => Ok(Self::AzureFile),
            "emptydir" => Ok(Self::EmptyDir),
            "secret" => Ok(Self::Secret),
            "nfsazurefile" => Ok(Self::NfsAzureFile),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VolumeStorageType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VolumeStorageType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VolumeStorageType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Workload profile name for container apps to execute on.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Workload profile name for container apps to execute on.",
///  "type": "string"
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
#[serde(transparent)]
pub struct WorkloadProfileName(pub ::std::string::String);
impl ::std::ops::Deref for WorkloadProfileName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<WorkloadProfileName> for ::std::string::String {
    fn from(value: WorkloadProfileName) -> Self {
        value.0
    }
}
impl ::std::convert::From<&WorkloadProfileName> for WorkloadProfileName {
    fn from(value: &WorkloadProfileName) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::string::String> for WorkloadProfileName {
    fn from(value: ::std::string::String) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for WorkloadProfileName {
    type Err = ::std::convert::Infallible;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.to_string()))
    }
}
impl ::std::fmt::Display for WorkloadProfileName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn default_u64<T, const V: u64>() -> T
    where
        T: ::std::convert::TryFrom<u64>,
        <T as ::std::convert::TryFrom<u64>>::Error: ::std::fmt::Debug,
    {
        T::try_from(V).unwrap()
    }
    pub(super) fn configuration_active_revisions_mode() -> super::ConfigurationActiveRevisionsMode {
        super::ConfigurationActiveRevisionsMode::Single
    }
    pub(super) fn dapr_app_protocol() -> super::DaprAppProtocol {
        super::DaprAppProtocol::Http
    }
    pub(super) fn identity_settings_lifecycle() -> super::IdentitySettingsLifecycle {
        super::IdentitySettingsLifecycle::All
    }
    pub(super) fn ingress_transport() -> super::IngressTransport {
        super::IngressTransport::Auto
    }
}
