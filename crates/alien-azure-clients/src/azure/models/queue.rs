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
///Entity status.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Entity status.",
///  "type": "string",
///  "enum": [
///    "Active",
///    "Disabled",
///    "Restoring",
///    "SendDisabled",
///    "ReceiveDisabled",
///    "Creating",
///    "Deleting",
///    "Renaming",
///    "Unknown"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "EntityStatus"
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
pub enum EntityStatus {
    Active,
    Disabled,
    Restoring,
    SendDisabled,
    ReceiveDisabled,
    Creating,
    Deleting,
    Renaming,
    Unknown,
}
impl ::std::convert::From<&Self> for EntityStatus {
    fn from(value: &EntityStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EntityStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Active => f.write_str("Active"),
            Self::Disabled => f.write_str("Disabled"),
            Self::Restoring => f.write_str("Restoring"),
            Self::SendDisabled => f.write_str("SendDisabled"),
            Self::ReceiveDisabled => f.write_str("ReceiveDisabled"),
            Self::Creating => f.write_str("Creating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Renaming => f.write_str("Renaming"),
            Self::Unknown => f.write_str("Unknown"),
        }
    }
}
impl ::std::str::FromStr for EntityStatus {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            "restoring" => Ok(Self::Restoring),
            "senddisabled" => Ok(Self::SendDisabled),
            "receivedisabled" => Ok(Self::ReceiveDisabled),
            "creating" => Ok(Self::Creating),
            "deleting" => Ok(Self::Deleting),
            "renaming" => Ok(Self::Renaming),
            "unknown" => Ok(Self::Unknown),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EntityStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EntityStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EntityStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///Message Count Details.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Message Count Details.",
///  "type": "object",
///  "properties": {
///    "activeMessageCount": {
///      "description": "Number of active messages in the queue, topic, or subscription.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "deadLetterMessageCount": {
///      "description": "Number of messages that are dead lettered.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "scheduledMessageCount": {
///      "description": "Number of scheduled messages.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "transferDeadLetterMessageCount": {
///      "description": "Number of messages transferred into dead letters.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "transferMessageCount": {
///      "description": "Number of messages transferred to another queue, topic, or subscription.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct MessageCountDetails {
    ///Number of active messages in the queue, topic, or subscription.
    #[serde(
        rename = "activeMessageCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub active_message_count: ::std::option::Option<i64>,
    ///Number of messages that are dead lettered.
    #[serde(
        rename = "deadLetterMessageCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dead_letter_message_count: ::std::option::Option<i64>,
    ///Number of scheduled messages.
    #[serde(
        rename = "scheduledMessageCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scheduled_message_count: ::std::option::Option<i64>,
    ///Number of messages transferred into dead letters.
    #[serde(
        rename = "transferDeadLetterMessageCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub transfer_dead_letter_message_count: ::std::option::Option<i64>,
    ///Number of messages transferred to another queue, topic, or subscription.
    #[serde(
        rename = "transferMessageCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub transfer_message_count: ::std::option::Option<i64>,
}
impl ::std::convert::From<&MessageCountDetails> for MessageCountDetails {
    fn from(value: &MessageCountDetails) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for MessageCountDetails {
    fn default() -> Self {
        Self {
            active_message_count: Default::default(),
            dead_letter_message_count: Default::default(),
            scheduled_message_count: Default::default(),
            transfer_dead_letter_message_count: Default::default(),
            transfer_message_count: Default::default(),
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
///Description of queue Resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Description of queue Resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/SBQueueProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbQueue {
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
    pub properties: ::std::option::Option<SbQueueProperties>,
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
impl ::std::convert::From<&SbQueue> for SbQueue {
    fn from(value: &SbQueue) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbQueue {
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
///The response to the List Queues operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response to the List Queues operation.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "Link to the next set of results. Not empty if Value contains incomplete list of queues.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Result of the List Queues operation.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SBQueue"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbQueueListResult {
    ///Link to the next set of results. Not empty if Value contains incomplete list of queues.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Result of the List Queues operation.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<SbQueue>,
}
impl ::std::convert::From<&SbQueueListResult> for SbQueueListResult {
    fn from(value: &SbQueueListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbQueueListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The Queue Properties definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Queue Properties definition.",
///  "type": "object",
///  "properties": {
///    "accessedAt": {
///      "description": "Last time a message was sent, or the last time there was a receive request to this queue.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "autoDeleteOnIdle": {
///      "description": "ISO 8061 timeSpan idle interval after which the queue is automatically deleted. The minimum duration is 5 minutes.",
///      "type": "string",
///      "format": "duration"
///    },
///    "countDetails": {
///      "$ref": "#/components/schemas/MessageCountDetails"
///    },
///    "createdAt": {
///      "description": "The exact time the message was created.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "deadLetteringOnMessageExpiration": {
///      "description": "A value that indicates whether this queue has dead letter support when a message expires.",
///      "type": "boolean"
///    },
///    "defaultMessageTimeToLive": {
///      "description": "ISO 8601 default message timespan to live value. This is the duration after which the message expires, starting from when the message is sent to Service Bus. This is the default value used when TimeToLive is not set on a message itself.",
///      "type": "string",
///      "format": "duration"
///    },
///    "duplicateDetectionHistoryTimeWindow": {
///      "description": "ISO 8601 timeSpan structure that defines the duration of the duplicate detection history. The default value is 10 minutes.",
///      "type": "string",
///      "format": "duration"
///    },
///    "enableBatchedOperations": {
///      "description": "Value that indicates whether server-side batched operations are enabled.",
///      "type": "boolean"
///    },
///    "enableExpress": {
///      "description": "A value that indicates whether Express Entities are enabled. An express queue holds a message in memory temporarily before writing it to persistent storage.",
///      "type": "boolean"
///    },
///    "enablePartitioning": {
///      "description": "A value that indicates whether the queue is to be partitioned across multiple message brokers.",
///      "type": "boolean"
///    },
///    "forwardDeadLetteredMessagesTo": {
///      "description": "Queue/Topic name to forward the Dead Letter message",
///      "type": "string"
///    },
///    "forwardTo": {
///      "description": "Queue/Topic name to forward the messages",
///      "type": "string"
///    },
///    "lockDuration": {
///      "description": "ISO 8601 timespan duration of a peek-lock; that is, the amount of time that the message is locked for other receivers. The maximum value for LockDuration is 5 minutes; the default value is 1 minute.",
///      "type": "string",
///      "format": "duration"
///    },
///    "maxDeliveryCount": {
///      "description": "The maximum delivery count. A message is automatically deadlettered after this number of deliveries. default value is 10.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "maxMessageSizeInKilobytes": {
///      "description": "Maximum size (in KB) of the message payload that can be accepted by the queue. This property is only used in Premium today and default is 1024.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "maxSizeInMegabytes": {
///      "description": "The maximum size of the queue in megabytes, which is the size of memory allocated for the queue. Default is 1024.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "messageCount": {
///      "description": "The number of messages in the queue.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "requiresDuplicateDetection": {
///      "description": "A value indicating if this queue requires duplicate detection.",
///      "type": "boolean"
///    },
///    "requiresSession": {
///      "description": "A value that indicates whether the queue supports the concept of sessions.",
///      "type": "boolean"
///    },
///    "sizeInBytes": {
///      "description": "The size of the queue, in bytes.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "status": {
///      "$ref": "#/components/schemas/EntityStatus"
///    },
///    "updatedAt": {
///      "description": "The exact time the message was updated.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SbQueueProperties {
    ///Last time a message was sent, or the last time there was a receive request to this queue.
    #[serde(
        rename = "accessedAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub accessed_at: ::std::option::Option<::std::string::String>,
    ///ISO 8061 timeSpan idle interval after which the queue is automatically deleted. The minimum duration is 5 minutes.
    #[serde(
        rename = "autoDeleteOnIdle",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auto_delete_on_idle: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "countDetails",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub count_details: ::std::option::Option<MessageCountDetails>,
    ///The exact time the message was created.
    #[serde(
        rename = "createdAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_at: ::std::option::Option<::std::string::String>,
    ///A value that indicates whether this queue has dead letter support when a message expires.
    #[serde(
        rename = "deadLetteringOnMessageExpiration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dead_lettering_on_message_expiration: ::std::option::Option<bool>,
    ///ISO 8601 default message timespan to live value. This is the duration after which the message expires, starting from when the message is sent to Service Bus. This is the default value used when TimeToLive is not set on a message itself.
    #[serde(
        rename = "defaultMessageTimeToLive",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_message_time_to_live: ::std::option::Option<::std::string::String>,
    ///ISO 8601 timeSpan structure that defines the duration of the duplicate detection history. The default value is 10 minutes.
    #[serde(
        rename = "duplicateDetectionHistoryTimeWindow",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub duplicate_detection_history_time_window: ::std::option::Option<
        ::std::string::String,
    >,
    ///Value that indicates whether server-side batched operations are enabled.
    #[serde(
        rename = "enableBatchedOperations",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_batched_operations: ::std::option::Option<bool>,
    ///A value that indicates whether Express Entities are enabled. An express queue holds a message in memory temporarily before writing it to persistent storage.
    #[serde(
        rename = "enableExpress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_express: ::std::option::Option<bool>,
    ///A value that indicates whether the queue is to be partitioned across multiple message brokers.
    #[serde(
        rename = "enablePartitioning",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_partitioning: ::std::option::Option<bool>,
    ///Queue/Topic name to forward the Dead Letter message
    #[serde(
        rename = "forwardDeadLetteredMessagesTo",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub forward_dead_lettered_messages_to: ::std::option::Option<::std::string::String>,
    ///Queue/Topic name to forward the messages
    #[serde(
        rename = "forwardTo",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub forward_to: ::std::option::Option<::std::string::String>,
    ///ISO 8601 timespan duration of a peek-lock; that is, the amount of time that the message is locked for other receivers. The maximum value for LockDuration is 5 minutes; the default value is 1 minute.
    #[serde(
        rename = "lockDuration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lock_duration: ::std::option::Option<::std::string::String>,
    ///The maximum delivery count. A message is automatically deadlettered after this number of deliveries. default value is 10.
    #[serde(
        rename = "maxDeliveryCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_delivery_count: ::std::option::Option<i32>,
    ///Maximum size (in KB) of the message payload that can be accepted by the queue. This property is only used in Premium today and default is 1024.
    #[serde(
        rename = "maxMessageSizeInKilobytes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_message_size_in_kilobytes: ::std::option::Option<i64>,
    ///The maximum size of the queue in megabytes, which is the size of memory allocated for the queue. Default is 1024.
    #[serde(
        rename = "maxSizeInMegabytes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_size_in_megabytes: ::std::option::Option<i32>,
    ///The number of messages in the queue.
    #[serde(
        rename = "messageCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message_count: ::std::option::Option<i64>,
    ///A value indicating if this queue requires duplicate detection.
    #[serde(
        rename = "requiresDuplicateDetection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub requires_duplicate_detection: ::std::option::Option<bool>,
    ///A value that indicates whether the queue supports the concept of sessions.
    #[serde(
        rename = "requiresSession",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub requires_session: ::std::option::Option<bool>,
    ///The size of the queue, in bytes.
    #[serde(
        rename = "sizeInBytes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub size_in_bytes: ::std::option::Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<EntityStatus>,
    ///The exact time the message was updated.
    #[serde(
        rename = "updatedAt",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated_at: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SbQueueProperties> for SbQueueProperties {
    fn from(value: &SbQueueProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SbQueueProperties {
    fn default() -> Self {
        Self {
            accessed_at: Default::default(),
            auto_delete_on_idle: Default::default(),
            count_details: Default::default(),
            created_at: Default::default(),
            dead_lettering_on_message_expiration: Default::default(),
            default_message_time_to_live: Default::default(),
            duplicate_detection_history_time_window: Default::default(),
            enable_batched_operations: Default::default(),
            enable_express: Default::default(),
            enable_partitioning: Default::default(),
            forward_dead_lettered_messages_to: Default::default(),
            forward_to: Default::default(),
            lock_duration: Default::default(),
            max_delivery_count: Default::default(),
            max_message_size_in_kilobytes: Default::default(),
            max_size_in_megabytes: Default::default(),
            message_count: Default::default(),
            requires_duplicate_detection: Default::default(),
            requires_session: Default::default(),
            size_in_bytes: Default::default(),
            status: Default::default(),
            updated_at: Default::default(),
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
