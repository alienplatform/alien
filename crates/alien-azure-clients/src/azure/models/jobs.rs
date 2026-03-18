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
///Container App executions collection ARM resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App executions collection ARM resource.",
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
///        "$ref": "#/components/schemas/JobExecution"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ContainerAppJobExecutions {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<JobExecution>,
}
impl ::std::convert::From<&ContainerAppJobExecutions> for ContainerAppJobExecutions {
    fn from(value: &ContainerAppJobExecutions) -> Self {
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
    for ContainerAppProbeHttpGetHttpHeadersItem
{
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
    PartialOrd,
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
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ContainerAppProbeHttpGetScheme {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    PartialOrd,
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
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
        Self {
            error: Default::default(),
        }
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
    for DefaultErrorResponseErrorDetailsItem
{
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
    PartialOrd,
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
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Container App Job
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App Job",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/ManagedServiceIdentity"
///    },
///    "properties": {
///      "description": "Container Apps Job resource specific properties.",
///      "type": "object",
///      "properties": {
///        "configuration": {
///          "$ref": "#/components/schemas/JobConfiguration"
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
///          "description": "The endpoint of the eventstream of the container apps job.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "outboundIpAddresses": {
///          "description": "Outbound IP Addresses of a container apps job.",
///          "readOnly": true,
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "provisioningState": {
///          "description": "Provisioning state of the Container Apps Job.",
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
///            "name": "JobProvisioningState"
///          }
///        },
///        "template": {
///          "$ref": "#/components/schemas/JobTemplate"
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
pub struct Job {
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
    pub properties: ::std::option::Option<JobProperties>,
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
impl ::std::convert::From<&Job> for Job {
    fn from(value: &Job) -> Self {
        value.clone()
    }
}
///Non versioned Container Apps Job configuration properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Non versioned Container Apps Job configuration properties",
///  "type": "object",
///  "required": [
///    "replicaTimeout",
///    "triggerType"
///  ],
///  "properties": {
///    "eventTriggerConfig": {
///      "description": "Trigger configuration of an event driven job.",
///      "type": "object",
///      "properties": {
///        "parallelism": {
///          "$ref": "#/components/schemas/Parallelism"
///        },
///        "replicaCompletionCount": {
///          "$ref": "#/components/schemas/ReplicaCompletionCount"
///        },
///        "scale": {
///          "$ref": "#/components/schemas/JobScale"
///        }
///      }
///    },
///    "identitySettings": {
///      "description": "Optional settings for Managed Identities that are assigned to the Container App Job. If a Managed Identity is not specified here, default settings will be used.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IdentitySettings"
///      },
///      "x-ms-identifiers": [
///        "identity"
///      ]
///    },
///    "manualTriggerConfig": {
///      "description": "Manual trigger configuration for a single execution job. Properties replicaCompletionCount and parallelism would be set to 1 by default",
///      "type": "object",
///      "properties": {
///        "parallelism": {
///          "$ref": "#/components/schemas/Parallelism"
///        },
///        "replicaCompletionCount": {
///          "$ref": "#/components/schemas/ReplicaCompletionCount"
///        }
///      }
///    },
///    "registries": {
///      "description": "Collection of private container registry credentials used by a Container apps job",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RegistryCredentials"
///      },
///      "x-ms-identifiers": [
///        "server"
///      ]
///    },
///    "replicaRetryLimit": {
///      "description": "Maximum number of retries before failing the job.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "replicaTimeout": {
///      "description": "Maximum number of seconds a replica is allowed to run.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "scheduleTriggerConfig": {
///      "description": "Cron formatted repeating trigger schedule (\"* * * * *\") for cronjobs. Properties completions and parallelism would be set to 1 by default",
///      "type": "object",
///      "required": [
///        "cronExpression"
///      ],
///      "properties": {
///        "cronExpression": {
///          "description": "Cron formatted repeating schedule (\"* * * * *\") of a Cron Job.",
///          "type": "string"
///        },
///        "parallelism": {
///          "$ref": "#/components/schemas/Parallelism"
///        },
///        "replicaCompletionCount": {
///          "$ref": "#/components/schemas/ReplicaCompletionCount"
///        }
///      }
///    },
///    "secrets": {
///      "description": "Collection of secrets used by a Container Apps Job",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Secret"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "triggerType": {
///      "description": "Trigger type of the job",
///      "default": "Manual",
///      "type": "string",
///      "enum": [
///        "Schedule",
///        "Event",
///        "Manual"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "TriggerType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobConfiguration {
    #[serde(
        rename = "eventTriggerConfig",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_trigger_config: ::std::option::Option<JobConfigurationEventTriggerConfig>,
    ///Optional settings for Managed Identities that are assigned to the Container App Job. If a Managed Identity is not specified here, default settings will be used.
    #[serde(
        rename = "identitySettings",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity_settings: ::std::vec::Vec<IdentitySettings>,
    #[serde(
        rename = "manualTriggerConfig",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub manual_trigger_config: ::std::option::Option<JobConfigurationManualTriggerConfig>,
    ///Collection of private container registry credentials used by a Container apps job
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registries: ::std::vec::Vec<RegistryCredentials>,
    ///Maximum number of retries before failing the job.
    #[serde(
        rename = "replicaRetryLimit",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub replica_retry_limit: ::std::option::Option<i32>,
    ///Maximum number of seconds a replica is allowed to run.
    #[serde(rename = "replicaTimeout")]
    pub replica_timeout: i32,
    #[serde(
        rename = "scheduleTriggerConfig",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub schedule_trigger_config: ::std::option::Option<JobConfigurationScheduleTriggerConfig>,
    ///Collection of secrets used by a Container Apps Job
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secrets: ::std::vec::Vec<Secret>,
    ///Trigger type of the job
    #[serde(rename = "triggerType")]
    pub trigger_type: JobConfigurationTriggerType,
}
impl ::std::convert::From<&JobConfiguration> for JobConfiguration {
    fn from(value: &JobConfiguration) -> Self {
        value.clone()
    }
}
///Trigger configuration of an event driven job.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Trigger configuration of an event driven job.",
///  "type": "object",
///  "properties": {
///    "parallelism": {
///      "$ref": "#/components/schemas/Parallelism"
///    },
///    "replicaCompletionCount": {
///      "$ref": "#/components/schemas/ReplicaCompletionCount"
///    },
///    "scale": {
///      "$ref": "#/components/schemas/JobScale"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobConfigurationEventTriggerConfig {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub parallelism: ::std::option::Option<Parallelism>,
    #[serde(
        rename = "replicaCompletionCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub replica_completion_count: ::std::option::Option<ReplicaCompletionCount>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scale: ::std::option::Option<JobScale>,
}
impl ::std::convert::From<&JobConfigurationEventTriggerConfig>
    for JobConfigurationEventTriggerConfig
{
    fn from(value: &JobConfigurationEventTriggerConfig) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobConfigurationEventTriggerConfig {
    fn default() -> Self {
        Self {
            parallelism: Default::default(),
            replica_completion_count: Default::default(),
            scale: Default::default(),
        }
    }
}
///Manual trigger configuration for a single execution job. Properties replicaCompletionCount and parallelism would be set to 1 by default
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Manual trigger configuration for a single execution job. Properties replicaCompletionCount and parallelism would be set to 1 by default",
///  "type": "object",
///  "properties": {
///    "parallelism": {
///      "$ref": "#/components/schemas/Parallelism"
///    },
///    "replicaCompletionCount": {
///      "$ref": "#/components/schemas/ReplicaCompletionCount"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobConfigurationManualTriggerConfig {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub parallelism: ::std::option::Option<Parallelism>,
    #[serde(
        rename = "replicaCompletionCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub replica_completion_count: ::std::option::Option<ReplicaCompletionCount>,
}
impl ::std::convert::From<&JobConfigurationManualTriggerConfig>
    for JobConfigurationManualTriggerConfig
{
    fn from(value: &JobConfigurationManualTriggerConfig) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobConfigurationManualTriggerConfig {
    fn default() -> Self {
        Self {
            parallelism: Default::default(),
            replica_completion_count: Default::default(),
        }
    }
}
///Cron formatted repeating trigger schedule ("* * * * *") for cronjobs. Properties completions and parallelism would be set to 1 by default
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Cron formatted repeating trigger schedule (\"* * * * *\") for cronjobs. Properties completions and parallelism would be set to 1 by default",
///  "type": "object",
///  "required": [
///    "cronExpression"
///  ],
///  "properties": {
///    "cronExpression": {
///      "description": "Cron formatted repeating schedule (\"* * * * *\") of a Cron Job.",
///      "type": "string"
///    },
///    "parallelism": {
///      "$ref": "#/components/schemas/Parallelism"
///    },
///    "replicaCompletionCount": {
///      "$ref": "#/components/schemas/ReplicaCompletionCount"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobConfigurationScheduleTriggerConfig {
    ///Cron formatted repeating schedule ("* * * * *") of a Cron Job.
    #[serde(rename = "cronExpression")]
    pub cron_expression: ::std::string::String,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub parallelism: ::std::option::Option<Parallelism>,
    #[serde(
        rename = "replicaCompletionCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub replica_completion_count: ::std::option::Option<ReplicaCompletionCount>,
}
impl ::std::convert::From<&JobConfigurationScheduleTriggerConfig>
    for JobConfigurationScheduleTriggerConfig
{
    fn from(value: &JobConfigurationScheduleTriggerConfig) -> Self {
        value.clone()
    }
}
///Trigger type of the job
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Trigger type of the job",
///  "default": "Manual",
///  "type": "string",
///  "enum": [
///    "Schedule",
///    "Event",
///    "Manual"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "TriggerType"
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
pub enum JobConfigurationTriggerType {
    Schedule,
    Event,
    Manual,
}
impl ::std::convert::From<&Self> for JobConfigurationTriggerType {
    fn from(value: &JobConfigurationTriggerType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for JobConfigurationTriggerType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Schedule => f.write_str("Schedule"),
            Self::Event => f.write_str("Event"),
            Self::Manual => f.write_str("Manual"),
        }
    }
}
impl ::std::str::FromStr for JobConfigurationTriggerType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "schedule" => Ok(Self::Schedule),
            "event" => Ok(Self::Event),
            "manual" => Ok(Self::Manual),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for JobConfigurationTriggerType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for JobConfigurationTriggerType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for JobConfigurationTriggerType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for JobConfigurationTriggerType {
    fn default() -> Self {
        JobConfigurationTriggerType::Manual
    }
}
///Container Apps Job execution.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Job execution.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Job execution Id.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Job execution Name.",
///      "type": "string"
///    },
///    "properties": {
///      "description": "Container Apps Job execution specific properties.",
///      "type": "object",
///      "properties": {
///        "endTime": {
///          "description": "Job execution end time.",
///          "type": "string"
///        },
///        "startTime": {
///          "description": "Job execution start time.",
///          "type": "string"
///        },
///        "status": {
///          "description": "Current running State of the job",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "Running",
///            "Processing",
///            "Stopped",
///            "Degraded",
///            "Failed",
///            "Unknown",
///            "Succeeded"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "JobExecutionRunningState"
///          }
///        },
///        "template": {
///          "$ref": "#/components/schemas/JobExecutionTemplate"
///        }
///      },
///      "x-ms-client-flatten": true
///    },
///    "type": {
///      "description": "Job execution type",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobExecution {
    ///Job execution Id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Job execution Name.
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
    pub properties: ::std::option::Option<JobExecutionProperties>,
    ///Job execution type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&JobExecution> for JobExecution {
    fn from(value: &JobExecution) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobExecution {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Container App's Job execution name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App's Job execution name.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Job execution Id.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Job execution name.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobExecutionBase {
    ///Job execution Id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Job execution name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&JobExecutionBase> for JobExecutionBase {
    fn from(value: &JobExecutionBase) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobExecutionBase {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
        }
    }
}
///Container Apps Jobs execution container definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Jobs execution container definition.",
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
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobExecutionContainer {
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
}
impl ::std::convert::From<&JobExecutionContainer> for JobExecutionContainer {
    fn from(value: &JobExecutionContainer) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobExecutionContainer {
    fn default() -> Self {
        Self {
            args: Default::default(),
            command: Default::default(),
            env: Default::default(),
            image: Default::default(),
            name: Default::default(),
            resources: Default::default(),
        }
    }
}
///Container App executions names list.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container App executions names list.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "Collection of resources.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/JobExecutionBase"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobExecutionNamesCollection {
    ///Collection of resources.
    pub value: ::std::vec::Vec<JobExecutionBase>,
}
impl ::std::convert::From<&JobExecutionNamesCollection> for JobExecutionNamesCollection {
    fn from(value: &JobExecutionNamesCollection) -> Self {
        value.clone()
    }
}
///Container Apps Job execution specific properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Job execution specific properties.",
///  "type": "object",
///  "properties": {
///    "endTime": {
///      "description": "Job execution end time.",
///      "type": "string"
///    },
///    "startTime": {
///      "description": "Job execution start time.",
///      "type": "string"
///    },
///    "status": {
///      "description": "Current running State of the job",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Running",
///        "Processing",
///        "Stopped",
///        "Degraded",
///        "Failed",
///        "Unknown",
///        "Succeeded"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "JobExecutionRunningState"
///      }
///    },
///    "template": {
///      "$ref": "#/components/schemas/JobExecutionTemplate"
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobExecutionProperties {
    ///Job execution end time.
    #[serde(
        rename = "endTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub end_time: ::std::option::Option<::std::string::String>,
    ///Job execution start time.
    #[serde(
        rename = "startTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub start_time: ::std::option::Option<::std::string::String>,
    ///Current running State of the job
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<JobExecutionPropertiesStatus>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub template: ::std::option::Option<JobExecutionTemplate>,
}
impl ::std::convert::From<&JobExecutionProperties> for JobExecutionProperties {
    fn from(value: &JobExecutionProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobExecutionProperties {
    fn default() -> Self {
        Self {
            end_time: Default::default(),
            start_time: Default::default(),
            status: Default::default(),
            template: Default::default(),
        }
    }
}
///Current running State of the job
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Current running State of the job",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Running",
///    "Processing",
///    "Stopped",
///    "Degraded",
///    "Failed",
///    "Unknown",
///    "Succeeded"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "JobExecutionRunningState"
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
pub enum JobExecutionPropertiesStatus {
    Running,
    Processing,
    Stopped,
    Degraded,
    Failed,
    Unknown,
    Succeeded,
}
impl ::std::convert::From<&Self> for JobExecutionPropertiesStatus {
    fn from(value: &JobExecutionPropertiesStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for JobExecutionPropertiesStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Running => f.write_str("Running"),
            Self::Processing => f.write_str("Processing"),
            Self::Stopped => f.write_str("Stopped"),
            Self::Degraded => f.write_str("Degraded"),
            Self::Failed => f.write_str("Failed"),
            Self::Unknown => f.write_str("Unknown"),
            Self::Succeeded => f.write_str("Succeeded"),
        }
    }
}
impl ::std::str::FromStr for JobExecutionPropertiesStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "running" => Ok(Self::Running),
            "processing" => Ok(Self::Processing),
            "stopped" => Ok(Self::Stopped),
            "degraded" => Ok(Self::Degraded),
            "failed" => Ok(Self::Failed),
            "unknown" => Ok(Self::Unknown),
            "succeeded" => Ok(Self::Succeeded),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for JobExecutionPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for JobExecutionPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for JobExecutionPropertiesStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Job's execution template, containing container configuration for a job's execution
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Job's execution template, containing container configuration for a job's execution",
///  "type": "object",
///  "properties": {
///    "containers": {
///      "description": "List of container definitions for the Container Apps Job.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/JobExecutionContainer"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "initContainers": {
///      "description": "List of specialized containers that run before job containers.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/JobExecutionContainer"
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
pub struct JobExecutionTemplate {
    ///List of container definitions for the Container Apps Job.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub containers: ::std::vec::Vec<JobExecutionContainer>,
    ///List of specialized containers that run before job containers.
    #[serde(
        rename = "initContainers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub init_containers: ::std::vec::Vec<JobExecutionContainer>,
}
impl ::std::convert::From<&JobExecutionTemplate> for JobExecutionTemplate {
    fn from(value: &JobExecutionTemplate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobExecutionTemplate {
    fn default() -> Self {
        Self {
            containers: Default::default(),
            init_containers: Default::default(),
        }
    }
}
///Container Apps Job resource specific properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Job resource specific properties.",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/ManagedServiceIdentity"
///    },
///    "properties": {
///      "type": "object",
///      "properties": {
///        "configuration": {
///          "$ref": "#/components/schemas/JobConfiguration"
///        },
///        "environmentId": {
///          "description": "Resource ID of environment.",
///          "type": "string",
///          "x-ms-mutability": [
///            "create",
///            "read",
///            "update"
///          ]
///        },
///        "eventStreamEndpoint": {
///          "description": "The endpoint of the eventstream of the container apps job.",
///          "type": "string"
///        },
///        "outboundIpAddresses": {
///          "description": "Outbound IP Addresses of a container apps job.",
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "template": {
///          "$ref": "#/components/schemas/JobTemplate"
///        }
///      }
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
pub struct JobPatchProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<ManagedServiceIdentity>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<JobPatchPropertiesProperties>,
    ///Resource tags.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&JobPatchProperties> for JobPatchProperties {
    fn from(value: &JobPatchProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobPatchProperties {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
        }
    }
}
///`JobPatchPropertiesProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "configuration": {
///      "$ref": "#/components/schemas/JobConfiguration"
///    },
///    "environmentId": {
///      "description": "Resource ID of environment.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read",
///        "update"
///      ]
///    },
///    "eventStreamEndpoint": {
///      "description": "The endpoint of the eventstream of the container apps job.",
///      "type": "string"
///    },
///    "outboundIpAddresses": {
///      "description": "Outbound IP Addresses of a container apps job.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "template": {
///      "$ref": "#/components/schemas/JobTemplate"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobPatchPropertiesProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub configuration: ::std::option::Option<JobConfiguration>,
    ///Resource ID of environment.
    #[serde(
        rename = "environmentId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub environment_id: ::std::option::Option<::std::string::String>,
    ///The endpoint of the eventstream of the container apps job.
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_stream_endpoint: ::std::option::Option<::std::string::String>,
    ///Outbound IP Addresses of a container apps job.
    #[serde(
        rename = "outboundIpAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub outbound_ip_addresses: ::std::vec::Vec<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub template: ::std::option::Option<JobTemplate>,
}
impl ::std::convert::From<&JobPatchPropertiesProperties> for JobPatchPropertiesProperties {
    fn from(value: &JobPatchPropertiesProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobPatchPropertiesProperties {
    fn default() -> Self {
        Self {
            configuration: Default::default(),
            environment_id: Default::default(),
            event_stream_endpoint: Default::default(),
            outbound_ip_addresses: Default::default(),
            template: Default::default(),
        }
    }
}
///Container Apps Job resource specific properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Job resource specific properties.",
///  "type": "object",
///  "properties": {
///    "configuration": {
///      "$ref": "#/components/schemas/JobConfiguration"
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
///      "description": "The endpoint of the eventstream of the container apps job.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "outboundIpAddresses": {
///      "description": "Outbound IP Addresses of a container apps job.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the Container Apps Job.",
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
///        "name": "JobProvisioningState"
///      }
///    },
///    "template": {
///      "$ref": "#/components/schemas/JobTemplate"
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
pub struct JobProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub configuration: ::std::option::Option<JobConfiguration>,
    ///Resource ID of environment.
    #[serde(
        rename = "environmentId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub environment_id: ::std::option::Option<::std::string::String>,
    ///The endpoint of the eventstream of the container apps job.
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_stream_endpoint: ::std::option::Option<::std::string::String>,
    ///Outbound IP Addresses of a container apps job.
    #[serde(
        rename = "outboundIpAddresses",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub outbound_ip_addresses: ::std::vec::Vec<::std::string::String>,
    ///Provisioning state of the Container Apps Job.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<JobPropertiesProvisioningState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub template: ::std::option::Option<JobTemplate>,
    #[serde(
        rename = "workloadProfileName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workload_profile_name: ::std::option::Option<WorkloadProfileName>,
}
impl ::std::convert::From<&JobProperties> for JobProperties {
    fn from(value: &JobProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobProperties {
    fn default() -> Self {
        Self {
            configuration: Default::default(),
            environment_id: Default::default(),
            event_stream_endpoint: Default::default(),
            outbound_ip_addresses: Default::default(),
            provisioning_state: Default::default(),
            template: Default::default(),
            workload_profile_name: Default::default(),
        }
    }
}
///Provisioning state of the Container Apps Job.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the Container Apps Job.",
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
///    "name": "JobProvisioningState"
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
pub enum JobPropertiesProvisioningState {
    InProgress,
    Succeeded,
    Failed,
    Canceled,
    Deleting,
}
impl ::std::convert::From<&Self> for JobPropertiesProvisioningState {
    fn from(value: &JobPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for JobPropertiesProvisioningState {
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
impl ::std::str::FromStr for JobPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
impl ::std::convert::TryFrom<&str> for JobPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for JobPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for JobPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Scaling configurations for event driven jobs.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Scaling configurations for event driven jobs.",
///  "type": "object",
///  "properties": {
///    "maxExecutions": {
///      "description": "Maximum number of job executions that are created for a trigger, default 100.",
///      "default": 100,
///      "type": "integer",
///      "format": "int32"
///    },
///    "minExecutions": {
///      "description": "Minimum number of job executions that are created for a trigger, default 0",
///      "default": 0,
///      "type": "integer",
///      "format": "int32"
///    },
///    "pollingInterval": {
///      "$ref": "#/components/schemas/PollingInterval"
///    },
///    "rules": {
///      "description": "Scaling rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/JobScaleRule"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobScale {
    ///Maximum number of job executions that are created for a trigger, default 100.
    #[serde(
        rename = "maxExecutions",
        default = "defaults::default_u64::<i32, 100>",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_executions: i32,
    ///Minimum number of job executions that are created for a trigger, default 0
    #[serde(
        rename = "minExecutions",
        default,
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub min_executions: i32,
    #[serde(
        rename = "pollingInterval",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub polling_interval: ::std::option::Option<PollingInterval>,
    ///Scaling rules.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rules: ::std::vec::Vec<JobScaleRule>,
}
impl ::std::convert::From<&JobScale> for JobScale {
    fn from(value: &JobScale) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobScale {
    fn default() -> Self {
        Self {
            max_executions: defaults::default_u64::<i32, 100>(),
            min_executions: Default::default(),
            polling_interval: Default::default(),
            rules: Default::default(),
        }
    }
}
///Scaling rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Scaling rule.",
///  "type": "object",
///  "properties": {
///    "auth": {
///      "description": "Authentication secrets for the scale rule.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ScaleRuleAuth"
///      }
///    },
///    "identity": {
///      "description": "The resource ID of a user-assigned managed identity that is assigned to the Container App, or 'system' for system-assigned identity.",
///      "type": "string"
///    },
///    "metadata": {
///      "description": "Metadata properties to describe the scale rule.",
///      "type": "object"
///    },
///    "name": {
///      "description": "Scale Rule Name",
///      "type": "string"
///    },
///    "type": {
///      "description": "Type of the scale rule\neg: azure-servicebus, redis etc.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobScaleRule {
    ///Authentication secrets for the scale rule.
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
    ///Metadata properties to describe the scale rule.
    #[serde(
        default,
        skip_serializing_if = "::serde_json::Map::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///Scale Rule Name
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    /**Type of the scale rule
    eg: azure-servicebus, redis etc.*/
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&JobScaleRule> for JobScaleRule {
    fn from(value: &JobScaleRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobScaleRule {
    fn default() -> Self {
        Self {
            auth: Default::default(),
            identity: Default::default(),
            metadata: Default::default(),
            name: Default::default(),
            type_: Default::default(),
        }
    }
}
///Container Apps Job Secrets Collection ARM resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Job Secrets Collection ARM resource.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "Collection of resources.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Secret"
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
pub struct JobSecretsCollection {
    ///Collection of resources.
    pub value: ::std::vec::Vec<Secret>,
}
impl ::std::convert::From<&JobSecretsCollection> for JobSecretsCollection {
    fn from(value: &JobSecretsCollection) -> Self {
        value.clone()
    }
}
///Container Apps Job versioned application definition. Defines the desired state of an immutable revision. Any changes to this section Will result in a new revision being created
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Job versioned application definition. Defines the desired state of an immutable revision. Any changes to this section Will result in a new revision being created",
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
pub struct JobTemplate {
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
    ///List of volume definitions for the Container App.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub volumes: ::std::vec::Vec<Volume>,
}
impl ::std::convert::From<&JobTemplate> for JobTemplate {
    fn from(value: &JobTemplate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for JobTemplate {
    fn default() -> Self {
        Self {
            containers: Default::default(),
            init_containers: Default::default(),
            volumes: Default::default(),
        }
    }
}
///Container Apps Jobs collection ARM resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Container Apps Jobs collection ARM resource.",
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
///        "$ref": "#/components/schemas/Job"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JobsCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<Job>,
}
impl ::std::convert::From<&JobsCollection> for JobsCollection {
    fn from(value: &JobsCollection) -> Self {
        value.clone()
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
    PartialOrd,
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
            Self::SystemAssignedUserAssigned => f.write_str("SystemAssigned,UserAssigned"),
        }
    }
}
impl ::std::str::FromStr for ManagedServiceIdentityType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
///Number of parallel replicas of a job that can run at a given time.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Number of parallel replicas of a job that can run at a given time.",
///  "type": "integer",
///  "format": "int32"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct Parallelism(pub i32);
impl ::std::ops::Deref for Parallelism {
    type Target = i32;
    fn deref(&self) -> &i32 {
        &self.0
    }
}
impl ::std::convert::From<Parallelism> for i32 {
    fn from(value: Parallelism) -> Self {
        value.0
    }
}
impl ::std::convert::From<&Parallelism> for Parallelism {
    fn from(value: &Parallelism) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<i32> for Parallelism {
    fn from(value: i32) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for Parallelism {
    type Err = <i32 as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for Parallelism {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&String> for Parallelism {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: &String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for Parallelism {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for Parallelism {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
///Interval to check each event source in seconds. Defaults to 30s
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Interval to check each event source in seconds. Defaults to 30s",
///  "type": "integer",
///  "format": "int32"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct PollingInterval(pub i32);
impl ::std::ops::Deref for PollingInterval {
    type Target = i32;
    fn deref(&self) -> &i32 {
        &self.0
    }
}
impl ::std::convert::From<PollingInterval> for i32 {
    fn from(value: PollingInterval) -> Self {
        value.0
    }
}
impl ::std::convert::From<&PollingInterval> for PollingInterval {
    fn from(value: &PollingInterval) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<i32> for PollingInterval {
    fn from(value: i32) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for PollingInterval {
    type Err = <i32 as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for PollingInterval {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&String> for PollingInterval {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: &String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for PollingInterval {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for PollingInterval {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
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
///Minimum number of successful replica completions before overall job completion.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Minimum number of successful replica completions before overall job completion.",
///  "type": "integer",
///  "format": "int32"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct ReplicaCompletionCount(pub i32);
impl ::std::ops::Deref for ReplicaCompletionCount {
    type Target = i32;
    fn deref(&self) -> &i32 {
        &self.0
    }
}
impl ::std::convert::From<ReplicaCompletionCount> for i32 {
    fn from(value: ReplicaCompletionCount) -> Self {
        value.0
    }
}
impl ::std::convert::From<&ReplicaCompletionCount> for ReplicaCompletionCount {
    fn from(value: &ReplicaCompletionCount) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<i32> for ReplicaCompletionCount {
    fn from(value: i32) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for ReplicaCompletionCount {
    type Err = <i32 as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for ReplicaCompletionCount {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&String> for ReplicaCompletionCount {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: &String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for ReplicaCompletionCount {
    type Error = <i32 as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for ReplicaCompletionCount {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
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
    type Target = ::std::collections::HashMap<::std::string::String, UserAssignedIdentity>;
    fn deref(&self) -> &::std::collections::HashMap<::std::string::String, UserAssignedIdentity> {
        &self.0
    }
}
impl ::std::convert::From<UserAssignedIdentities>
    for ::std::collections::HashMap<::std::string::String, UserAssignedIdentity>
{
    fn from(value: UserAssignedIdentities) -> Self {
        value.0
    }
}
impl ::std::convert::From<&UserAssignedIdentities> for UserAssignedIdentities {
    fn from(value: &UserAssignedIdentities) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::collections::HashMap<::std::string::String, UserAssignedIdentity>>
    for UserAssignedIdentities
{
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
    PartialOrd,
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
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
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
    ::serde::Deserialize, ::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd,
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
    pub(super) fn identity_settings_lifecycle() -> super::IdentitySettingsLifecycle {
        super::IdentitySettingsLifecycle::All
    }
}
