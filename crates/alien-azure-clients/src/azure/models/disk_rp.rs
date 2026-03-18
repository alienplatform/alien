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
///The Access Level, accepted values include None, Read, Write.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Access Level, accepted values include None, Read, Write.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Read",
///    "Write"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AccessLevel",
///    "values": [
///      {
///        "name": "None",
///        "value": "None"
///      },
///      {
///        "name": "Read",
///        "value": "Read"
///      },
///      {
///        "name": "Write",
///        "value": "Write"
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
pub enum AccessLevel {
    None,
    Read,
    Write,
}
impl ::std::convert::From<&Self> for AccessLevel {
    fn from(value: &AccessLevel) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AccessLevel {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Read => f.write_str("Read"),
            Self::Write => f.write_str("Write"),
        }
    }
}
impl ::std::str::FromStr for AccessLevel {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AccessLevel {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AccessLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AccessLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A disk access SAS uri.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A disk access SAS uri.",
///  "type": "object",
///  "properties": {
///    "accessSAS": {
///      "description": "A SAS uri for accessing a disk.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "securityDataAccessSAS": {
///      "description": "A SAS uri for accessing a VM guest state.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "securityMetadataAccessSAS": {
///      "description": "A SAS uri for accessing a VM metadata.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AccessUri {
    ///A SAS uri for accessing a disk.
    #[serde(
        rename = "accessSAS",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_sas: ::std::option::Option<::std::string::String>,
    ///A SAS uri for accessing a VM guest state.
    #[serde(
        rename = "securityDataAccessSAS",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_data_access_sas: ::std::option::Option<::std::string::String>,
    ///A SAS uri for accessing a VM metadata.
    #[serde(
        rename = "securityMetadataAccessSAS",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_metadata_access_sas: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&AccessUri> for AccessUri {
    fn from(value: &AccessUri) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AccessUri {
    fn default() -> Self {
        Self {
            access_sas: Default::default(),
            security_data_access_sas: Default::default(),
            security_metadata_access_sas: Default::default(),
        }
    }
}
///Api error.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Api error.",
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "The error code.",
///      "type": "string"
///    },
///    "details": {
///      "description": "The Api error details",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApiErrorBase"
///      },
///      "x-ms-identifiers": [
///        "message",
///        "target"
///      ]
///    },
///    "innererror": {
///      "$ref": "#/components/schemas/InnerError"
///    },
///    "message": {
///      "description": "The error message.",
///      "type": "string"
///    },
///    "target": {
///      "description": "The target of the particular error.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApiError {
    ///The error code.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///The Api error details
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub details: ::std::vec::Vec<ApiErrorBase>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub innererror: ::std::option::Option<InnerError>,
    ///The error message.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///The target of the particular error.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApiError> for ApiError {
    fn from(value: &ApiError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApiError {
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
///Api error base.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Api error base.",
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "The error code.",
///      "type": "string"
///    },
///    "message": {
///      "description": "The error message.",
///      "type": "string"
///    },
///    "target": {
///      "description": "The target of the particular error.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApiErrorBase {
    ///The error code.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///The error message.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///The target of the particular error.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApiErrorBase> for ApiErrorBase {
    fn from(value: &ApiErrorBase) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApiErrorBase {
    fn default() -> Self {
        Self {
            code: Default::default(),
            message: Default::default(),
            target: Default::default(),
        }
    }
}
///CPU architecture supported by an OS disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "CPU architecture supported by an OS disk.",
///  "type": "string",
///  "enum": [
///    "x64",
///    "Arm64"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Architecture",
///    "values": [
///      {
///        "name": "x64",
///        "value": "x64"
///      },
///      {
///        "name": "Arm64",
///        "value": "Arm64"
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
pub enum Architecture {
    #[serde(rename = "x64")]
    X64,
    Arm64,
}
impl ::std::convert::From<&Self> for Architecture {
    fn from(value: &Architecture) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::X64 => f.write_str("x64"),
            Self::Arm64 => f.write_str("Arm64"),
        }
    }
}
impl ::std::str::FromStr for Architecture {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "x64" => Ok(Self::X64),
            "arm64" => Ok(Self::Arm64),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Architecture {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Architecture {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Architecture {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///In the case of an availability or connectivity issue with the data disk, specify the behavior of your VM
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "In the case of an availability or connectivity issue with the data disk, specify the behavior of your VM",
///  "type": "object",
///  "properties": {
///    "actionOnDiskDelay": {
///      "$ref": "#/components/schemas/AvailabilityPolicyDiskDelay"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AvailabilityPolicy {
    #[serde(
        rename = "actionOnDiskDelay",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action_on_disk_delay: ::std::option::Option<AvailabilityPolicyDiskDelay>,
}
impl ::std::convert::From<&AvailabilityPolicy> for AvailabilityPolicy {
    fn from(value: &AvailabilityPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AvailabilityPolicy {
    fn default() -> Self {
        Self {
            action_on_disk_delay: Default::default(),
        }
    }
}
///Determines on how to handle disks with slow I/O.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Determines on how to handle disks with slow I/O.",
///  "type": "string",
///  "enum": [
///    "None",
///    "AutomaticReattach"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AvailabilityPolicyDiskDelay",
///    "values": [
///      {
///        "description": "Defaults to behavior without av policy specified, which is VM restart upon slow disk io.",
///        "name": "None",
///        "value": "None"
///      },
///      {
///        "description": "Upon a disk io failure or slow response, try detaching then reattaching the disk.",
///        "name": "AutomaticReattach",
///        "value": "AutomaticReattach"
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
pub enum AvailabilityPolicyDiskDelay {
    None,
    AutomaticReattach,
}
impl ::std::convert::From<&Self> for AvailabilityPolicyDiskDelay {
    fn from(value: &AvailabilityPolicyDiskDelay) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AvailabilityPolicyDiskDelay {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::AutomaticReattach => f.write_str("AutomaticReattach"),
        }
    }
}
impl ::std::str::FromStr for AvailabilityPolicyDiskDelay {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "automaticreattach" => Ok(Self::AutomaticReattach),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AvailabilityPolicyDiskDelay {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AvailabilityPolicyDiskDelay {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AvailabilityPolicyDiskDelay {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An error response from the Compute service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the Compute service.",
///  "type": "object",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/ApiError"
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
    pub error: ::std::option::Option<ApiError>,
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
///Indicates the error details if the background copy of a resource created via the CopyStart operation fails.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates the error details if the background copy of a resource created via the CopyStart operation fails.",
///  "type": "object",
///  "required": [
///    "errorCode",
///    "errorMessage"
///  ],
///  "properties": {
///    "errorCode": {
///      "$ref": "#/components/schemas/CopyCompletionErrorReason"
///    },
///    "errorMessage": {
///      "description": "Indicates the error message if the background copy of a resource created via the CopyStart operation fails.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CopyCompletionError {
    #[serde(rename = "errorCode")]
    pub error_code: CopyCompletionErrorReason,
    ///Indicates the error message if the background copy of a resource created via the CopyStart operation fails.
    #[serde(rename = "errorMessage")]
    pub error_message: ::std::string::String,
}
impl ::std::convert::From<&CopyCompletionError> for CopyCompletionError {
    fn from(value: &CopyCompletionError) -> Self {
        value.clone()
    }
}
///Indicates the error code if the background copy of a resource created via the CopyStart operation fails.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates the error code if the background copy of a resource created via the CopyStart operation fails.",
///  "type": "string",
///  "enum": [
///    "CopySourceNotFound"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CopyCompletionErrorReason",
///    "values": [
///      {
///        "description": "Indicates that the source snapshot was deleted while the background copy of the resource created via CopyStart operation was in progress.",
///        "name": "CopySourceNotFound",
///        "value": "CopySourceNotFound"
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
pub enum CopyCompletionErrorReason {
    CopySourceNotFound,
}
impl ::std::convert::From<&Self> for CopyCompletionErrorReason {
    fn from(value: &CopyCompletionErrorReason) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CopyCompletionErrorReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::CopySourceNotFound => f.write_str("CopySourceNotFound"),
        }
    }
}
impl ::std::str::FromStr for CopyCompletionErrorReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "copysourcenotfound" => Ok(Self::CopySourceNotFound),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CopyCompletionErrorReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CopyCompletionErrorReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CopyCompletionErrorReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Data used when creating a disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Data used when creating a disk.",
///  "type": "object",
///  "required": [
///    "createOption"
///  ],
///  "properties": {
///    "createOption": {
///      "$ref": "#/components/schemas/DiskCreateOption"
///    },
///    "elasticSanResourceId": {
///      "description": "Required if createOption is CopyFromSanSnapshot. This is the ARM id of the source elastic san volume snapshot.",
///      "type": "string"
///    },
///    "galleryImageReference": {
///      "$ref": "#/components/schemas/ImageDiskReference"
///    },
///    "imageReference": {
///      "$ref": "#/components/schemas/ImageDiskReference"
///    },
///    "instantAccessDurationMinutes": {
///      "description": "For snapshots created from Premium SSD v2 or Ultra disk, this property determines the time in minutes the snapshot is retained for instant access to enable faster restore.",
///      "type": "integer",
///      "format": "int64",
///      "minimum": 1.0
///    },
///    "logicalSectorSize": {
///      "description": "Logical sector size in bytes for Ultra disks. Supported values are 512 ad 4096. 4096 is the default.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "performancePlus": {
///      "description": "Set this flag to true to get a boost on the performance target of the disk deployed, see here on the respective performance target. This flag can only be set on disk creation time and cannot be disabled after enabled.",
///      "type": "boolean"
///    },
///    "provisionedBandwidthCopySpeed": {
///      "$ref": "#/components/schemas/ProvisionedBandwidthCopyOption"
///    },
///    "securityDataUri": {
///      "description": "If createOption is ImportSecure, this is the URI of a blob to be imported into VM guest state.",
///      "type": "string"
///    },
///    "securityMetadataUri": {
///      "description": "If createOption is ImportSecure, this is the URI of a blob to be imported into VM metadata for Confidential VM.",
///      "type": "string",
///      "format": "uri"
///    },
///    "sourceResourceId": {
///      "description": "If createOption is Copy, this is the ARM id of the source snapshot or disk.",
///      "type": "string"
///    },
///    "sourceUniqueId": {
///      "description": "If this field is set, this is the unique id identifying the source of this resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "sourceUri": {
///      "description": "If createOption is Import, this is the URI of a blob to be imported into a managed disk.",
///      "type": "string"
///    },
///    "storageAccountId": {
///      "description": "Required if createOption is Import. The Azure Resource Manager identifier of the storage account containing the blob to import as a disk.",
///      "type": "string"
///    },
///    "uploadSizeBytes": {
///      "description": "If createOption is Upload, this is the size of the contents of the upload including the VHD footer. This value should be between 20972032 (20 MiB + 512 bytes for the VHD footer) and 35183298347520 bytes (32 TiB + 512 bytes for the VHD footer).",
///      "type": "integer",
///      "format": "int64"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CreationData {
    #[serde(rename = "createOption")]
    pub create_option: DiskCreateOption,
    ///Required if createOption is CopyFromSanSnapshot. This is the ARM id of the source elastic san volume snapshot.
    #[serde(
        rename = "elasticSanResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub elastic_san_resource_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "galleryImageReference",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub gallery_image_reference: ::std::option::Option<ImageDiskReference>,
    #[serde(
        rename = "imageReference",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub image_reference: ::std::option::Option<ImageDiskReference>,
    ///For snapshots created from Premium SSD v2 or Ultra disk, this property determines the time in minutes the snapshot is retained for instant access to enable faster restore.
    #[serde(
        rename = "instantAccessDurationMinutes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub instant_access_duration_minutes: ::std::option::Option<::std::num::NonZeroU64>,
    ///Logical sector size in bytes for Ultra disks. Supported values are 512 ad 4096. 4096 is the default.
    #[serde(
        rename = "logicalSectorSize",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub logical_sector_size: ::std::option::Option<i32>,
    ///Set this flag to true to get a boost on the performance target of the disk deployed, see here on the respective performance target. This flag can only be set on disk creation time and cannot be disabled after enabled.
    #[serde(
        rename = "performancePlus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub performance_plus: ::std::option::Option<bool>,
    #[serde(
        rename = "provisionedBandwidthCopySpeed",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioned_bandwidth_copy_speed: ::std::option::Option<ProvisionedBandwidthCopyOption>,
    ///If createOption is ImportSecure, this is the URI of a blob to be imported into VM guest state.
    #[serde(
        rename = "securityDataUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_data_uri: ::std::option::Option<::std::string::String>,
    ///If createOption is ImportSecure, this is the URI of a blob to be imported into VM metadata for Confidential VM.
    #[serde(
        rename = "securityMetadataUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_metadata_uri: ::std::option::Option<::std::string::String>,
    ///If createOption is Copy, this is the ARM id of the source snapshot or disk.
    #[serde(
        rename = "sourceResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_resource_id: ::std::option::Option<::std::string::String>,
    ///If this field is set, this is the unique id identifying the source of this resource.
    #[serde(
        rename = "sourceUniqueId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_unique_id: ::std::option::Option<::std::string::String>,
    ///If createOption is Import, this is the URI of a blob to be imported into a managed disk.
    #[serde(
        rename = "sourceUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_uri: ::std::option::Option<::std::string::String>,
    ///Required if createOption is Import. The Azure Resource Manager identifier of the storage account containing the blob to import as a disk.
    #[serde(
        rename = "storageAccountId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub storage_account_id: ::std::option::Option<::std::string::String>,
    ///If createOption is Upload, this is the size of the contents of the upload including the VHD footer. This value should be between 20972032 (20 MiB + 512 bytes for the VHD footer) and 35183298347520 bytes (32 TiB + 512 bytes for the VHD footer).
    #[serde(
        rename = "uploadSizeBytes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub upload_size_bytes: ::std::option::Option<i64>,
}
impl ::std::convert::From<&CreationData> for CreationData {
    fn from(value: &CreationData) -> Self {
        value.clone()
    }
}
///Additional authentication requirements when exporting or uploading to a disk or snapshot.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Additional authentication requirements when exporting or uploading to a disk or snapshot.",
///  "type": "string",
///  "enum": [
///    "AzureActiveDirectory",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DataAccessAuthMode",
///    "values": [
///      {
///        "description": "When export/upload URL is used, the system checks if the user has an identity in Azure Active Directory and has necessary permissions to export/upload the data. Please refer to aka.ms/DisksAzureADAuth.",
///        "name": "AzureActiveDirectory",
///        "value": "AzureActiveDirectory"
///      },
///      {
///        "description": "No additional authentication would be performed when accessing export/upload URL.",
///        "name": "None",
///        "value": "None"
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
pub enum DataAccessAuthMode {
    AzureActiveDirectory,
    None,
}
impl ::std::convert::From<&Self> for DataAccessAuthMode {
    fn from(value: &DataAccessAuthMode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DataAccessAuthMode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AzureActiveDirectory => f.write_str("AzureActiveDirectory"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for DataAccessAuthMode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "azureactivedirectory" => Ok(Self::AzureActiveDirectory),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DataAccessAuthMode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DataAccessAuthMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DataAccessAuthMode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Disk resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Disk resource.",
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
///    "managedBy": {
///      "description": "A relative URI containing the ID of the VM that has the disk attached.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "managedByExtended": {
///      "description": "List of relative URIs containing the IDs of the VMs that have the disk attached. maxShares should be set to a value greater than one for disks to allow attaching them to multiple VMs.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "properties": {
///      "$ref": "#/components/schemas/DiskProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/DiskSku"
///    },
///    "zones": {
///      "description": "The Logical zone list for Disk.",
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
pub struct Disk {
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The geo-location where the resource lives
    pub location: ::std::string::String,
    ///A relative URI containing the ID of the VM that has the disk attached.
    #[serde(
        rename = "managedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by: ::std::option::Option<::std::string::String>,
    ///List of relative URIs containing the IDs of the VMs that have the disk attached. maxShares should be set to a value greater than one for disks to allow attaching them to multiple VMs.
    #[serde(
        rename = "managedByExtended",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by_extended: ::std::vec::Vec<::std::string::String>,
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
    pub properties: ::std::option::Option<DiskProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<DiskSku>,
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
    ///The Logical zone list for Disk.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zones: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&Disk> for Disk {
    fn from(value: &Disk) -> Self {
        value.clone()
    }
}
///disk access resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "disk access resource.",
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
///    "properties": {
///      "$ref": "#/components/schemas/DiskAccessProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskAccess {
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
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
    pub properties: ::std::option::Option<DiskAccessProperties>,
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
impl ::std::convert::From<&DiskAccess> for DiskAccess {
    fn from(value: &DiskAccess) -> Self {
        value.clone()
    }
}
///The List disk access operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List disk access operation response.",
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
///      "description": "The DiskAccess items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DiskAccess"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskAccessList {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The DiskAccess items on this page
    pub value: ::std::vec::Vec<DiskAccess>,
}
impl ::std::convert::From<&DiskAccessList> for DiskAccessList {
    fn from(value: &DiskAccessList) -> Self {
        value.clone()
    }
}
///`DiskAccessProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "privateEndpointConnections": {
///      "description": "A readonly collection of private endpoint connections created on the disk. Currently only one endpoint connection is supported.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    },
///    "provisioningState": {
///      "description": "The disk access resource provisioning state.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "timeCreated": {
///      "description": "The time when the disk access was created.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskAccessProperties {
    ///A readonly collection of private endpoint connections created on the disk. Currently only one endpoint connection is supported.
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnection>,
    ///The disk access resource provisioning state.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    ///The time when the disk access was created.
    #[serde(
        rename = "timeCreated",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub time_created: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DiskAccessProperties> for DiskAccessProperties {
    fn from(value: &DiskAccessProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskAccessProperties {
    fn default() -> Self {
        Self {
            private_endpoint_connections: Default::default(),
            provisioning_state: Default::default(),
            time_created: Default::default(),
        }
    }
}
///Used for updating a disk access resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Used for updating a disk access resource.",
///  "type": "object",
///  "properties": {
///    "tags": {
///      "description": "Resource tags",
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
pub struct DiskAccessUpdate {
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&DiskAccessUpdate> for DiskAccessUpdate {
    fn from(value: &DiskAccessUpdate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskAccessUpdate {
    fn default() -> Self {
        Self {
            tags: Default::default(),
        }
    }
}
///This enumerates the possible sources of a disk's creation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This enumerates the possible sources of a disk's creation.",
///  "type": "string",
///  "enum": [
///    "Empty",
///    "Attach",
///    "FromImage",
///    "Import",
///    "Copy",
///    "Restore",
///    "Upload",
///    "CopyStart",
///    "ImportSecure",
///    "UploadPreparedSecure",
///    "CopyFromSanSnapshot"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DiskCreateOption",
///    "values": [
///      {
///        "description": "Create an empty data disk of a size given by diskSizeGB.",
///        "name": "Empty",
///        "value": "Empty"
///      },
///      {
///        "description": "Disk will be attached to a VM.",
///        "name": "Attach",
///        "value": "Attach"
///      },
///      {
///        "description": "Create a new disk from a platform image specified by the given imageReference or galleryImageReference.",
///        "name": "FromImage",
///        "value": "FromImage"
///      },
///      {
///        "description": "Create a disk by importing from a blob specified by a sourceUri in a storage account specified by storageAccountId.",
///        "name": "Import",
///        "value": "Import"
///      },
///      {
///        "description": "Create a new disk or snapshot by copying from a disk or snapshot specified by the given sourceResourceId.",
///        "name": "Copy",
///        "value": "Copy"
///      },
///      {
///        "description": "Create a new disk by copying from a backup recovery point.",
///        "name": "Restore",
///        "value": "Restore"
///      },
///      {
///        "description": "Create a new disk by obtaining a write token and using it to directly upload the contents of the disk.",
///        "name": "Upload",
///        "value": "Upload"
///      },
///      {
///        "description": "Create a new disk by using a deep copy process, where the resource creation is considered complete only after all data has been copied from the source.",
///        "name": "CopyStart",
///        "value": "CopyStart"
///      },
///      {
///        "description": "Similar to Import create option. Create a new Trusted Launch VM or Confidential VM supported disk by importing additional blobs for VM guest state specified by securityDataUri and VM metadata specified by securityMetadataUri in storage account specified by storageAccountId. The VM metadata is optional and only required for certain Confidential VM configurations and not required for Trusted Launch VM.",
///        "name": "ImportSecure",
///        "value": "ImportSecure"
///      },
///      {
///        "description": "Similar to Upload create option. Create a new Trusted Launch VM or Confidential VM supported disk and upload using write token in disk, VM guest state and VM metadata. The VM metadata is optional and only required for certain Confidential VM configurations and not required for Trusted Launch VM.",
///        "name": "UploadPreparedSecure",
///        "value": "UploadPreparedSecure"
///      },
///      {
///        "description": "Create a new disk by exporting from elastic san volume snapshot",
///        "name": "CopyFromSanSnapshot",
///        "value": "CopyFromSanSnapshot"
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
pub enum DiskCreateOption {
    Empty,
    Attach,
    FromImage,
    Import,
    Copy,
    Restore,
    Upload,
    CopyStart,
    ImportSecure,
    UploadPreparedSecure,
    CopyFromSanSnapshot,
}
impl ::std::convert::From<&Self> for DiskCreateOption {
    fn from(value: &DiskCreateOption) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DiskCreateOption {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Empty => f.write_str("Empty"),
            Self::Attach => f.write_str("Attach"),
            Self::FromImage => f.write_str("FromImage"),
            Self::Import => f.write_str("Import"),
            Self::Copy => f.write_str("Copy"),
            Self::Restore => f.write_str("Restore"),
            Self::Upload => f.write_str("Upload"),
            Self::CopyStart => f.write_str("CopyStart"),
            Self::ImportSecure => f.write_str("ImportSecure"),
            Self::UploadPreparedSecure => f.write_str("UploadPreparedSecure"),
            Self::CopyFromSanSnapshot => f.write_str("CopyFromSanSnapshot"),
        }
    }
}
impl ::std::str::FromStr for DiskCreateOption {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "empty" => Ok(Self::Empty),
            "attach" => Ok(Self::Attach),
            "fromimage" => Ok(Self::FromImage),
            "import" => Ok(Self::Import),
            "copy" => Ok(Self::Copy),
            "restore" => Ok(Self::Restore),
            "upload" => Ok(Self::Upload),
            "copystart" => Ok(Self::CopyStart),
            "importsecure" => Ok(Self::ImportSecure),
            "uploadpreparedsecure" => Ok(Self::UploadPreparedSecure),
            "copyfromsansnapshot" => Ok(Self::CopyFromSanSnapshot),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DiskCreateOption {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DiskCreateOption {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DiskCreateOption {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///disk encryption set resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "disk encryption set resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/EncryptionSetIdentity"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/EncryptionSetProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskEncryptionSet {
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
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
    pub identity: ::std::option::Option<EncryptionSetIdentity>,
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
    pub properties: ::std::option::Option<EncryptionSetProperties>,
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
impl ::std::convert::From<&DiskEncryptionSet> for DiskEncryptionSet {
    fn from(value: &DiskEncryptionSet) -> Self {
        value.clone()
    }
}
///The type of Managed Identity used by the DiskEncryptionSet. Only SystemAssigned is supported for new creations. Disk Encryption Sets can be updated with Identity type None during migration of subscription to a new Azure Active Directory tenant; it will cause the encrypted resources to lose access to the keys.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of Managed Identity used by the DiskEncryptionSet. Only SystemAssigned is supported for new creations. Disk Encryption Sets can be updated with Identity type None during migration of subscription to a new Azure Active Directory tenant; it will cause the encrypted resources to lose access to the keys.",
///  "type": "string",
///  "enum": [
///    "SystemAssigned",
///    "UserAssigned",
///    "SystemAssigned, UserAssigned",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DiskEncryptionSetIdentityType",
///    "values": [
///      {
///        "name": "SystemAssigned",
///        "value": "SystemAssigned"
///      },
///      {
///        "name": "UserAssigned",
///        "value": "UserAssigned"
///      },
///      {
///        "name": "SystemAssigned, UserAssigned",
///        "value": "SystemAssigned, UserAssigned"
///      },
///      {
///        "name": "None",
///        "value": "None"
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
pub enum DiskEncryptionSetIdentityType {
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned, UserAssigned")]
    SystemAssignedUserAssigned,
    None,
}
impl ::std::convert::From<&Self> for DiskEncryptionSetIdentityType {
    fn from(value: &DiskEncryptionSetIdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DiskEncryptionSetIdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => f.write_str("SystemAssigned, UserAssigned"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for DiskEncryptionSetIdentityType {
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
impl ::std::convert::TryFrom<&str> for DiskEncryptionSetIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DiskEncryptionSetIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DiskEncryptionSetIdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The List disk encryption set operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List disk encryption set operation response.",
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
///      "description": "The DiskEncryptionSet items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DiskEncryptionSet"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskEncryptionSetList {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The DiskEncryptionSet items on this page
    pub value: ::std::vec::Vec<DiskEncryptionSet>,
}
impl ::std::convert::From<&DiskEncryptionSetList> for DiskEncryptionSetList {
    fn from(value: &DiskEncryptionSetList) -> Self {
        value.clone()
    }
}
///The type of key used to encrypt the data of the disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of key used to encrypt the data of the disk.",
///  "type": "string",
///  "enum": [
///    "EncryptionAtRestWithCustomerKey",
///    "EncryptionAtRestWithPlatformAndCustomerKeys",
///    "ConfidentialVmEncryptedWithCustomerKey"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DiskEncryptionSetType",
///    "values": [
///      {
///        "description": "Resource using diskEncryptionSet would be encrypted at rest with Customer managed key that can be changed and revoked by a customer.",
///        "name": "EncryptionAtRestWithCustomerKey",
///        "value": "EncryptionAtRestWithCustomerKey"
///      },
///      {
///        "description": "Resource using diskEncryptionSet would be encrypted at rest with two layers of encryption. One of the keys is Customer managed and the other key is Platform managed.",
///        "name": "EncryptionAtRestWithPlatformAndCustomerKeys",
///        "value": "EncryptionAtRestWithPlatformAndCustomerKeys"
///      },
///      {
///        "description": "Confidential VM supported disk and VM guest state would be encrypted with customer managed key.",
///        "name": "ConfidentialVmEncryptedWithCustomerKey",
///        "value": "ConfidentialVmEncryptedWithCustomerKey"
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
pub enum DiskEncryptionSetType {
    EncryptionAtRestWithCustomerKey,
    EncryptionAtRestWithPlatformAndCustomerKeys,
    ConfidentialVmEncryptedWithCustomerKey,
}
impl ::std::convert::From<&Self> for DiskEncryptionSetType {
    fn from(value: &DiskEncryptionSetType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DiskEncryptionSetType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EncryptionAtRestWithCustomerKey => f.write_str("EncryptionAtRestWithCustomerKey"),
            Self::EncryptionAtRestWithPlatformAndCustomerKeys => {
                f.write_str("EncryptionAtRestWithPlatformAndCustomerKeys")
            }
            Self::ConfidentialVmEncryptedWithCustomerKey => {
                f.write_str("ConfidentialVmEncryptedWithCustomerKey")
            }
        }
    }
}
impl ::std::str::FromStr for DiskEncryptionSetType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "encryptionatrestwithcustomerkey" => Ok(Self::EncryptionAtRestWithCustomerKey),
            "encryptionatrestwithplatformandcustomerkeys" => {
                Ok(Self::EncryptionAtRestWithPlatformAndCustomerKeys)
            }
            "confidentialvmencryptedwithcustomerkey" => {
                Ok(Self::ConfidentialVmEncryptedWithCustomerKey)
            }
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DiskEncryptionSetType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DiskEncryptionSetType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DiskEncryptionSetType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///disk encryption set update resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "disk encryption set update resource.",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/EncryptionSetIdentity"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/DiskEncryptionSetUpdateProperties"
///    },
///    "tags": {
///      "description": "Resource tags",
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
pub struct DiskEncryptionSetUpdate {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<EncryptionSetIdentity>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<DiskEncryptionSetUpdateProperties>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&DiskEncryptionSetUpdate> for DiskEncryptionSetUpdate {
    fn from(value: &DiskEncryptionSetUpdate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskEncryptionSetUpdate {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
        }
    }
}
///disk encryption set resource update properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "disk encryption set resource update properties.",
///  "type": "object",
///  "properties": {
///    "activeKey": {
///      "$ref": "#/components/schemas/KeyForDiskEncryptionSet"
///    },
///    "encryptionType": {
///      "$ref": "#/components/schemas/DiskEncryptionSetType"
///    },
///    "federatedClientId": {
///      "description": "Multi-tenant application client id to access key vault in a different tenant. Setting the value to 'None' will clear the property.",
///      "type": "string"
///    },
///    "rotationToLatestKeyVersionEnabled": {
///      "description": "Set this flag to true to enable auto-updating of this disk encryption set to the latest key version.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskEncryptionSetUpdateProperties {
    #[serde(
        rename = "activeKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub active_key: ::std::option::Option<KeyForDiskEncryptionSet>,
    #[serde(
        rename = "encryptionType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_type: ::std::option::Option<DiskEncryptionSetType>,
    ///Multi-tenant application client id to access key vault in a different tenant. Setting the value to 'None' will clear the property.
    #[serde(
        rename = "federatedClientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub federated_client_id: ::std::option::Option<::std::string::String>,
    ///Set this flag to true to enable auto-updating of this disk encryption set to the latest key version.
    #[serde(
        rename = "rotationToLatestKeyVersionEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rotation_to_latest_key_version_enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<&DiskEncryptionSetUpdateProperties>
    for DiskEncryptionSetUpdateProperties
{
    fn from(value: &DiskEncryptionSetUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskEncryptionSetUpdateProperties {
    fn default() -> Self {
        Self {
            active_key: Default::default(),
            encryption_type: Default::default(),
            federated_client_id: Default::default(),
            rotation_to_latest_key_version_enabled: Default::default(),
        }
    }
}
///The List Disks operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List Disks operation response.",
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
///      "description": "The Disk items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Disk"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskList {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The Disk items on this page
    pub value: ::std::vec::Vec<Disk>,
}
impl ::std::convert::From<&DiskList> for DiskList {
    fn from(value: &DiskList) -> Self {
        value.clone()
    }
}
///Disk resource properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Disk resource properties.",
///  "type": "object",
///  "required": [
///    "creationData"
///  ],
///  "properties": {
///    "LastOwnershipUpdateTime": {
///      "description": "The UTC time when the ownership state of the disk was last changed i.e., the time the disk was last attached or detached from a VM or the time when the VM to which the disk was attached was deallocated or started.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-client-name": "lastOwnershipUpdateTime"
///    },
///    "availabilityPolicy": {
///      "$ref": "#/components/schemas/AvailabilityPolicy"
///    },
///    "burstingEnabled": {
///      "description": "Set to true to enable bursting beyond the provisioned performance target of the disk. Bursting is disabled by default. Does not apply to Ultra disks.",
///      "type": "boolean"
///    },
///    "burstingEnabledTime": {
///      "description": "Latest time when bursting was last enabled on a disk.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "completionPercent": {
///      "description": "Percentage complete for the background copy when a resource is created via the CopyStart operation.",
///      "type": "number",
///      "format": "float"
///    },
///    "creationData": {
///      "$ref": "#/components/schemas/CreationData"
///    },
///    "dataAccessAuthMode": {
///      "$ref": "#/components/schemas/DataAccessAuthMode"
///    },
///    "diskAccessId": {
///      "description": "ARM id of the DiskAccess resource for using private endpoints on disks.",
///      "type": "string"
///    },
///    "diskIOPSReadOnly": {
///      "description": "The total number of IOPS that will be allowed across all VMs mounting the shared disk as ReadOnly. One operation can transfer between 4k and 256k bytes.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskIOPSReadWrite": {
///      "description": "The number of IOPS allowed for this disk; only settable for UltraSSD disks. One operation can transfer between 4k and 256k bytes.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskMBpsReadOnly": {
///      "description": "The total throughput (MBps) that will be allowed across all VMs mounting the shared disk as ReadOnly. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskMBpsReadWrite": {
///      "description": "The bandwidth allowed for this disk; only settable for UltraSSD disks. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskSizeBytes": {
///      "description": "The size of the disk in bytes. This field is read only.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskSizeGB": {
///      "description": "If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "diskState": {
///      "$ref": "#/components/schemas/DiskState"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "encryptionSettingsCollection": {
///      "$ref": "#/components/schemas/EncryptionSettingsCollection"
///    },
///    "hyperVGeneration": {
///      "$ref": "#/components/schemas/HyperVGeneration"
///    },
///    "maxShares": {
///      "description": "The maximum number of VMs that can attach to the disk at the same time. Value greater than one indicates a disk that can be mounted on multiple VMs at the same time.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "networkAccessPolicy": {
///      "$ref": "#/components/schemas/NetworkAccessPolicy"
///    },
///    "optimizedForFrequentAttach": {
///      "description": "Setting this property to true improves reliability and performance of data disks that are frequently (more than 5 times a day) by detached from one virtual machine and attached to another. This property should not be set for disks that are not detached and attached frequently as it causes the disks to not align with the fault domain of the virtual machine.",
///      "type": "boolean"
///    },
///    "osType": {
///      "$ref": "#/components/schemas/OperatingSystemTypes"
///    },
///    "propertyUpdatesInProgress": {
///      "$ref": "#/components/schemas/PropertyUpdatesInProgress"
///    },
///    "provisioningState": {
///      "description": "The disk provisioning state.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "purchasePlan": {
///      "$ref": "#/components/schemas/DiskPurchasePlan"
///    },
///    "securityProfile": {
///      "$ref": "#/components/schemas/DiskSecurityProfile"
///    },
///    "shareInfo": {
///      "description": "Details of the list of all VMs that have the disk attached. maxShares should be set to a value greater than one for disks to allow attaching them to multiple VMs.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ShareInfoElement"
///      },
///      "x-ms-identifiers": [
///        "vmUri"
///      ]
///    },
///    "supportedCapabilities": {
///      "$ref": "#/components/schemas/SupportedCapabilities"
///    },
///    "supportsHibernation": {
///      "description": "Indicates the OS on a disk supports hibernation.",
///      "type": "boolean"
///    },
///    "tier": {
///      "description": "Performance tier of the disk (e.g, P4, S10) as described here: https://azure.microsoft.com/en-us/pricing/details/managed-disks/. Does not apply to Ultra disks.",
///      "type": "string"
///    },
///    "timeCreated": {
///      "description": "The time when the disk was created.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "uniqueId": {
///      "description": "Unique Guid identifying the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskProperties {
    #[serde(
        rename = "availabilityPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub availability_policy: ::std::option::Option<AvailabilityPolicy>,
    ///Set to true to enable bursting beyond the provisioned performance target of the disk. Bursting is disabled by default. Does not apply to Ultra disks.
    #[serde(
        rename = "burstingEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub bursting_enabled: ::std::option::Option<bool>,
    ///Latest time when bursting was last enabled on a disk.
    #[serde(
        rename = "burstingEnabledTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub bursting_enabled_time: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "completionPercent",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub completion_percent: ::std::option::Option<f32>,
    #[serde(rename = "creationData")]
    pub creation_data: CreationData,
    #[serde(
        rename = "dataAccessAuthMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_access_auth_mode: ::std::option::Option<DataAccessAuthMode>,
    ///ARM id of the DiskAccess resource for using private endpoints on disks.
    #[serde(
        rename = "diskAccessId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_access_id: ::std::option::Option<::std::string::String>,
    ///The total number of IOPS that will be allowed across all VMs mounting the shared disk as ReadOnly. One operation can transfer between 4k and 256k bytes.
    #[serde(
        rename = "diskIOPSReadOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_iops_read_only: ::std::option::Option<i64>,
    ///The number of IOPS allowed for this disk; only settable for UltraSSD disks. One operation can transfer between 4k and 256k bytes.
    #[serde(
        rename = "diskIOPSReadWrite",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_iops_read_write: ::std::option::Option<i64>,
    ///The total throughput (MBps) that will be allowed across all VMs mounting the shared disk as ReadOnly. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.
    #[serde(
        rename = "diskMBpsReadOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_m_bps_read_only: ::std::option::Option<i64>,
    ///The bandwidth allowed for this disk; only settable for UltraSSD disks. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.
    #[serde(
        rename = "diskMBpsReadWrite",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_m_bps_read_write: ::std::option::Option<i64>,
    ///The size of the disk in bytes. This field is read only.
    #[serde(
        rename = "diskSizeBytes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_size_bytes: ::std::option::Option<i64>,
    ///If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.
    #[serde(
        rename = "diskSizeGB",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_size_gb: ::std::option::Option<i32>,
    #[serde(
        rename = "diskState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_state: ::std::option::Option<DiskState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    #[serde(
        rename = "encryptionSettingsCollection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_settings_collection: ::std::option::Option<EncryptionSettingsCollection>,
    #[serde(
        rename = "hyperVGeneration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub hyper_v_generation: ::std::option::Option<HyperVGeneration>,
    ///The UTC time when the ownership state of the disk was last changed i.e., the time the disk was last attached or detached from a VM or the time when the VM to which the disk was attached was deallocated or started.
    #[serde(
        rename = "LastOwnershipUpdateTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_ownership_update_time: ::std::option::Option<::std::string::String>,
    ///The maximum number of VMs that can attach to the disk at the same time. Value greater than one indicates a disk that can be mounted on multiple VMs at the same time.
    #[serde(
        rename = "maxShares",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_shares: ::std::option::Option<i32>,
    #[serde(
        rename = "networkAccessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_access_policy: ::std::option::Option<NetworkAccessPolicy>,
    ///Setting this property to true improves reliability and performance of data disks that are frequently (more than 5 times a day) by detached from one virtual machine and attached to another. This property should not be set for disks that are not detached and attached frequently as it causes the disks to not align with the fault domain of the virtual machine.
    #[serde(
        rename = "optimizedForFrequentAttach",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub optimized_for_frequent_attach: ::std::option::Option<bool>,
    #[serde(
        rename = "osType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub os_type: ::std::option::Option<OperatingSystemTypes>,
    #[serde(
        rename = "propertyUpdatesInProgress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub property_updates_in_progress: ::std::option::Option<PropertyUpdatesInProgress>,
    ///The disk provisioning state.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "purchasePlan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub purchase_plan: ::std::option::Option<DiskPurchasePlan>,
    #[serde(
        rename = "securityProfile",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_profile: ::std::option::Option<DiskSecurityProfile>,
    ///Details of the list of all VMs that have the disk attached. maxShares should be set to a value greater than one for disks to allow attaching them to multiple VMs.
    #[serde(
        rename = "shareInfo",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub share_info: ::std::vec::Vec<ShareInfoElement>,
    #[serde(
        rename = "supportedCapabilities",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supported_capabilities: ::std::option::Option<SupportedCapabilities>,
    ///Indicates the OS on a disk supports hibernation.
    #[serde(
        rename = "supportsHibernation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_hibernation: ::std::option::Option<bool>,
    ///Performance tier of the disk (e.g, P4, S10) as described here: https://azure.microsoft.com/en-us/pricing/details/managed-disks/. Does not apply to Ultra disks.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<::std::string::String>,
    ///The time when the disk was created.
    #[serde(
        rename = "timeCreated",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub time_created: ::std::option::Option<::std::string::String>,
    ///Unique Guid identifying the resource.
    #[serde(
        rename = "uniqueId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub unique_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DiskProperties> for DiskProperties {
    fn from(value: &DiskProperties) -> Self {
        value.clone()
    }
}
///Used for establishing the purchase context of any 3rd Party artifact through MarketPlace.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Used for establishing the purchase context of any 3rd Party artifact through MarketPlace.",
///  "type": "object",
///  "required": [
///    "name",
///    "product",
///    "publisher"
///  ],
///  "properties": {
///    "name": {
///      "description": "The plan ID.",
///      "type": "string"
///    },
///    "product": {
///      "description": "Specifies the product of the image from the marketplace. This is the same value as Offer under the imageReference element.",
///      "type": "string"
///    },
///    "promotionCode": {
///      "description": "The Offer Promotion Code.",
///      "type": "string"
///    },
///    "publisher": {
///      "description": "The publisher ID.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskPurchasePlan {
    ///The plan ID.
    pub name: ::std::string::String,
    ///Specifies the product of the image from the marketplace. This is the same value as Offer under the imageReference element.
    pub product: ::std::string::String,
    ///The Offer Promotion Code.
    #[serde(
        rename = "promotionCode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub promotion_code: ::std::option::Option<::std::string::String>,
    ///The publisher ID.
    pub publisher: ::std::string::String,
}
impl ::std::convert::From<&DiskPurchasePlan> for DiskPurchasePlan {
    fn from(value: &DiskPurchasePlan) -> Self {
        value.clone()
    }
}
///Properties of disk restore point
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of disk restore point",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/DiskRestorePointProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskRestorePoint {
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
    pub properties: ::std::option::Option<DiskRestorePointProperties>,
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
impl ::std::convert::From<&DiskRestorePoint> for DiskRestorePoint {
    fn from(value: &DiskRestorePoint) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskRestorePoint {
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
///The List Disk Restore Points operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List Disk Restore Points operation response.",
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
///      "description": "The DiskRestorePoint items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DiskRestorePoint"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskRestorePointList {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The DiskRestorePoint items on this page
    pub value: ::std::vec::Vec<DiskRestorePoint>,
}
impl ::std::convert::From<&DiskRestorePointList> for DiskRestorePointList {
    fn from(value: &DiskRestorePointList) -> Self {
        value.clone()
    }
}
///Properties of an incremental disk restore point
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of an incremental disk restore point",
///  "type": "object",
///  "properties": {
///    "completionPercent": {
///      "description": "Percentage complete for the background copy of disk restore point when source resource is from a different region.",
///      "type": "number",
///      "format": "float"
///    },
///    "diskAccessId": {
///      "description": "ARM id of the DiskAccess resource for using private endpoints on disks.",
///      "type": "string"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "familyId": {
///      "description": "id of the backing snapshot's MIS family",
///      "readOnly": true,
///      "type": "string"
///    },
///    "hyperVGeneration": {
///      "$ref": "#/components/schemas/HyperVGeneration"
///    },
///    "logicalSectorSize": {
///      "description": "Logical sector size in bytes for disk restore points of UltraSSD_LRS and PremiumV2_LRS disks. Supported values are 512 and 4096. 4096 is the default.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int32"
///    },
///    "networkAccessPolicy": {
///      "$ref": "#/components/schemas/NetworkAccessPolicy"
///    },
///    "osType": {
///      "$ref": "#/components/schemas/OperatingSystemTypes"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "purchasePlan": {
///      "$ref": "#/components/schemas/DiskPurchasePlan"
///    },
///    "replicationState": {
///      "description": "Replication state of disk restore point when source resource is from a different region.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "securityProfile": {
///      "$ref": "#/components/schemas/DiskSecurityProfile"
///    },
///    "sourceResourceId": {
///      "description": "arm id of source disk or source disk restore point.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "sourceResourceLocation": {
///      "description": "Location of source disk or source disk restore point when source resource is from a different region.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "sourceUniqueId": {
///      "description": "unique incarnation id of the source disk",
///      "readOnly": true,
///      "type": "string"
///    },
///    "supportedCapabilities": {
///      "$ref": "#/components/schemas/SupportedCapabilities"
///    },
///    "supportsHibernation": {
///      "description": "Indicates the OS on a disk supports hibernation.",
///      "type": "boolean"
///    },
///    "timeCreated": {
///      "description": "The timestamp of restorePoint creation",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskRestorePointProperties {
    #[serde(
        rename = "completionPercent",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub completion_percent: ::std::option::Option<f32>,
    ///ARM id of the DiskAccess resource for using private endpoints on disks.
    #[serde(
        rename = "diskAccessId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_access_id: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    ///id of the backing snapshot's MIS family
    #[serde(
        rename = "familyId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub family_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "hyperVGeneration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub hyper_v_generation: ::std::option::Option<HyperVGeneration>,
    ///Logical sector size in bytes for disk restore points of UltraSSD_LRS and PremiumV2_LRS disks. Supported values are 512 and 4096. 4096 is the default.
    #[serde(
        rename = "logicalSectorSize",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub logical_sector_size: ::std::option::Option<i32>,
    #[serde(
        rename = "networkAccessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_access_policy: ::std::option::Option<NetworkAccessPolicy>,
    #[serde(
        rename = "osType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub os_type: ::std::option::Option<OperatingSystemTypes>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "purchasePlan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub purchase_plan: ::std::option::Option<DiskPurchasePlan>,
    ///Replication state of disk restore point when source resource is from a different region.
    #[serde(
        rename = "replicationState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub replication_state: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "securityProfile",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_profile: ::std::option::Option<DiskSecurityProfile>,
    ///arm id of source disk or source disk restore point.
    #[serde(
        rename = "sourceResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_resource_id: ::std::option::Option<::std::string::String>,
    ///Location of source disk or source disk restore point when source resource is from a different region.
    #[serde(
        rename = "sourceResourceLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_resource_location: ::std::option::Option<::std::string::String>,
    ///unique incarnation id of the source disk
    #[serde(
        rename = "sourceUniqueId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_unique_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "supportedCapabilities",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supported_capabilities: ::std::option::Option<SupportedCapabilities>,
    ///Indicates the OS on a disk supports hibernation.
    #[serde(
        rename = "supportsHibernation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_hibernation: ::std::option::Option<bool>,
    ///The timestamp of restorePoint creation
    #[serde(
        rename = "timeCreated",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub time_created: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DiskRestorePointProperties> for DiskRestorePointProperties {
    fn from(value: &DiskRestorePointProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskRestorePointProperties {
    fn default() -> Self {
        Self {
            completion_percent: Default::default(),
            disk_access_id: Default::default(),
            encryption: Default::default(),
            family_id: Default::default(),
            hyper_v_generation: Default::default(),
            logical_sector_size: Default::default(),
            network_access_policy: Default::default(),
            os_type: Default::default(),
            public_network_access: Default::default(),
            purchase_plan: Default::default(),
            replication_state: Default::default(),
            security_profile: Default::default(),
            source_resource_id: Default::default(),
            source_resource_location: Default::default(),
            source_unique_id: Default::default(),
            supported_capabilities: Default::default(),
            supports_hibernation: Default::default(),
            time_created: Default::default(),
        }
    }
}
///Contains the security related information for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Contains the security related information for the resource.",
///  "type": "object",
///  "properties": {
///    "secureVMDiskEncryptionSetId": {
///      "description": "ResourceId of the disk encryption set associated to Confidential VM supported disk encrypted with customer managed key",
///      "type": "string"
///    },
///    "securityType": {
///      "$ref": "#/components/schemas/DiskSecurityTypes"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskSecurityProfile {
    ///ResourceId of the disk encryption set associated to Confidential VM supported disk encrypted with customer managed key
    #[serde(
        rename = "secureVMDiskEncryptionSetId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secure_vm_disk_encryption_set_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "securityType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_type: ::std::option::Option<DiskSecurityTypes>,
}
impl ::std::convert::From<&DiskSecurityProfile> for DiskSecurityProfile {
    fn from(value: &DiskSecurityProfile) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskSecurityProfile {
    fn default() -> Self {
        Self {
            secure_vm_disk_encryption_set_id: Default::default(),
            security_type: Default::default(),
        }
    }
}
///Specifies the SecurityType of the VM. Applicable for OS disks only.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies the SecurityType of the VM. Applicable for OS disks only.",
///  "type": "string",
///  "enum": [
///    "TrustedLaunch",
///    "ConfidentialVM_VMGuestStateOnlyEncryptedWithPlatformKey",
///    "ConfidentialVM_DiskEncryptedWithPlatformKey",
///    "ConfidentialVM_DiskEncryptedWithCustomerKey",
///    "ConfidentialVM_NonPersistedTPM"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DiskSecurityTypes",
///    "values": [
///      {
///        "description": "Trusted Launch provides security features such as secure boot and virtual Trusted Platform Module (vTPM)",
///        "name": "TrustedLaunch",
///        "value": "TrustedLaunch"
///      },
///      {
///        "description": "Indicates Confidential VM disk with only VM guest state encrypted",
///        "name": "ConfidentialVM_VMGuestStateOnlyEncryptedWithPlatformKey",
///        "value": "ConfidentialVM_VMGuestStateOnlyEncryptedWithPlatformKey"
///      },
///      {
///        "description": "Indicates Confidential VM disk with both OS disk and VM guest state encrypted with a platform managed key",
///        "name": "ConfidentialVM_DiskEncryptedWithPlatformKey",
///        "value": "ConfidentialVM_DiskEncryptedWithPlatformKey"
///      },
///      {
///        "description": "Indicates Confidential VM disk with both OS disk and VM guest state encrypted with a customer managed key",
///        "name": "ConfidentialVM_DiskEncryptedWithCustomerKey",
///        "value": "ConfidentialVM_DiskEncryptedWithCustomerKey"
///      },
///      {
///        "description": "Indicates Confidential VM disk with a ephemeral vTPM. vTPM state is not persisted across VM reboots.",
///        "name": "ConfidentialVM_NonPersistedTPM",
///        "value": "ConfidentialVM_NonPersistedTPM"
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
pub enum DiskSecurityTypes {
    TrustedLaunch,
    #[serde(rename = "ConfidentialVM_VMGuestStateOnlyEncryptedWithPlatformKey")]
    ConfidentialVmVmGuestStateOnlyEncryptedWithPlatformKey,
    #[serde(rename = "ConfidentialVM_DiskEncryptedWithPlatformKey")]
    ConfidentialVmDiskEncryptedWithPlatformKey,
    #[serde(rename = "ConfidentialVM_DiskEncryptedWithCustomerKey")]
    ConfidentialVmDiskEncryptedWithCustomerKey,
    #[serde(rename = "ConfidentialVM_NonPersistedTPM")]
    ConfidentialVmNonPersistedTpm,
}
impl ::std::convert::From<&Self> for DiskSecurityTypes {
    fn from(value: &DiskSecurityTypes) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DiskSecurityTypes {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::TrustedLaunch => f.write_str("TrustedLaunch"),
            Self::ConfidentialVmVmGuestStateOnlyEncryptedWithPlatformKey => {
                f.write_str("ConfidentialVM_VMGuestStateOnlyEncryptedWithPlatformKey")
            }
            Self::ConfidentialVmDiskEncryptedWithPlatformKey => {
                f.write_str("ConfidentialVM_DiskEncryptedWithPlatformKey")
            }
            Self::ConfidentialVmDiskEncryptedWithCustomerKey => {
                f.write_str("ConfidentialVM_DiskEncryptedWithCustomerKey")
            }
            Self::ConfidentialVmNonPersistedTpm => f.write_str("ConfidentialVM_NonPersistedTPM"),
        }
    }
}
impl ::std::str::FromStr for DiskSecurityTypes {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "trustedlaunch" => Ok(Self::TrustedLaunch),
            "confidentialvm_vmgueststateonlyencryptedwithplatformkey" => {
                Ok(Self::ConfidentialVmVmGuestStateOnlyEncryptedWithPlatformKey)
            }
            "confidentialvm_diskencryptedwithplatformkey" => {
                Ok(Self::ConfidentialVmDiskEncryptedWithPlatformKey)
            }
            "confidentialvm_diskencryptedwithcustomerkey" => {
                Ok(Self::ConfidentialVmDiskEncryptedWithCustomerKey)
            }
            "confidentialvm_nonpersistedtpm" => Ok(Self::ConfidentialVmNonPersistedTpm),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DiskSecurityTypes {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DiskSecurityTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DiskSecurityTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The disks sku name. Can be Standard_LRS, Premium_LRS, StandardSSD_LRS, UltraSSD_LRS, Premium_ZRS, StandardSSD_ZRS, or PremiumV2_LRS.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The disks sku name. Can be Standard_LRS, Premium_LRS, StandardSSD_LRS, UltraSSD_LRS, Premium_ZRS, StandardSSD_ZRS, or PremiumV2_LRS.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "$ref": "#/components/schemas/DiskStorageAccountTypes"
///    },
///    "tier": {
///      "description": "The sku tier.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskSku {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<DiskStorageAccountTypes>,
    ///The sku tier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DiskSku> for DiskSku {
    fn from(value: &DiskSku) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskSku {
    fn default() -> Self {
        Self {
            name: Default::default(),
            tier: Default::default(),
        }
    }
}
///This enumerates the possible state of the disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This enumerates the possible state of the disk.",
///  "type": "string",
///  "enum": [
///    "Unattached",
///    "Attached",
///    "Reserved",
///    "Frozen",
///    "ActiveSAS",
///    "ActiveSASFrozen",
///    "ReadyToUpload",
///    "ActiveUpload"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DiskState",
///    "values": [
///      {
///        "description": "The disk is not being used and can be attached to a VM.",
///        "name": "Unattached",
///        "value": "Unattached"
///      },
///      {
///        "description": "The disk is currently attached to a running VM.",
///        "name": "Attached",
///        "value": "Attached"
///      },
///      {
///        "description": "The disk is attached to a stopped-deallocated VM.",
///        "name": "Reserved",
///        "value": "Reserved"
///      },
///      {
///        "description": "The disk is attached to a VM which is in hibernated state.",
///        "name": "Frozen",
///        "value": "Frozen"
///      },
///      {
///        "description": "The disk currently has an Active SAS Uri associated with it.",
///        "name": "ActiveSAS",
///        "value": "ActiveSAS"
///      },
///      {
///        "description": "The disk is attached to a VM in hibernated state and has an active SAS URI associated with it.",
///        "name": "ActiveSASFrozen",
///        "value": "ActiveSASFrozen"
///      },
///      {
///        "description": "A disk is ready to be created by upload by requesting a write token.",
///        "name": "ReadyToUpload",
///        "value": "ReadyToUpload"
///      },
///      {
///        "description": "A disk is created for upload and a write token has been issued for uploading to it.",
///        "name": "ActiveUpload",
///        "value": "ActiveUpload"
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
pub enum DiskState {
    Unattached,
    Attached,
    Reserved,
    Frozen,
    #[serde(rename = "ActiveSAS")]
    ActiveSas,
    #[serde(rename = "ActiveSASFrozen")]
    ActiveSasFrozen,
    ReadyToUpload,
    ActiveUpload,
}
impl ::std::convert::From<&Self> for DiskState {
    fn from(value: &DiskState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DiskState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Unattached => f.write_str("Unattached"),
            Self::Attached => f.write_str("Attached"),
            Self::Reserved => f.write_str("Reserved"),
            Self::Frozen => f.write_str("Frozen"),
            Self::ActiveSas => f.write_str("ActiveSAS"),
            Self::ActiveSasFrozen => f.write_str("ActiveSASFrozen"),
            Self::ReadyToUpload => f.write_str("ReadyToUpload"),
            Self::ActiveUpload => f.write_str("ActiveUpload"),
        }
    }
}
impl ::std::str::FromStr for DiskState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "unattached" => Ok(Self::Unattached),
            "attached" => Ok(Self::Attached),
            "reserved" => Ok(Self::Reserved),
            "frozen" => Ok(Self::Frozen),
            "activesas" => Ok(Self::ActiveSas),
            "activesasfrozen" => Ok(Self::ActiveSasFrozen),
            "readytoupload" => Ok(Self::ReadyToUpload),
            "activeupload" => Ok(Self::ActiveUpload),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DiskState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DiskState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DiskState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The sku name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The sku name.",
///  "type": "string",
///  "enum": [
///    "Standard_LRS",
///    "Premium_LRS",
///    "StandardSSD_LRS",
///    "UltraSSD_LRS",
///    "Premium_ZRS",
///    "StandardSSD_ZRS",
///    "PremiumV2_LRS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DiskStorageAccountTypes",
///    "values": [
///      {
///        "description": "Standard HDD locally redundant storage. Best for backup, non-critical, and infrequent access.",
///        "name": "Standard_LRS",
///        "value": "Standard_LRS"
///      },
///      {
///        "description": "Premium SSD locally redundant storage. Best for production and performance sensitive workloads.",
///        "name": "Premium_LRS",
///        "value": "Premium_LRS"
///      },
///      {
///        "description": "Standard SSD locally redundant storage. Best for web servers, lightly used enterprise applications and dev/test.",
///        "name": "StandardSSD_LRS",
///        "value": "StandardSSD_LRS"
///      },
///      {
///        "description": "Ultra SSD locally redundant storage. Best for IO-intensive workloads such as SAP HANA, top tier databases (for example, SQL, Oracle), and other transaction-heavy workloads.",
///        "name": "UltraSSD_LRS",
///        "value": "UltraSSD_LRS"
///      },
///      {
///        "description": "Premium SSD zone redundant storage. Best for the production workloads that need storage resiliency against zone failures.",
///        "name": "Premium_ZRS",
///        "value": "Premium_ZRS"
///      },
///      {
///        "description": "Standard SSD zone redundant storage. Best for web servers, lightly used enterprise applications and dev/test that need storage resiliency against zone failures.",
///        "name": "StandardSSD_ZRS",
///        "value": "StandardSSD_ZRS"
///      },
///      {
///        "description": "Premium SSD v2 locally redundant storage. Best for production and performance-sensitive workloads that consistently require low latency and high IOPS and throughput.",
///        "name": "PremiumV2_LRS",
///        "value": "PremiumV2_LRS"
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
pub enum DiskStorageAccountTypes {
    #[serde(rename = "Standard_LRS")]
    StandardLrs,
    #[serde(rename = "Premium_LRS")]
    PremiumLrs,
    #[serde(rename = "StandardSSD_LRS")]
    StandardSsdLrs,
    #[serde(rename = "UltraSSD_LRS")]
    UltraSsdLrs,
    #[serde(rename = "Premium_ZRS")]
    PremiumZrs,
    #[serde(rename = "StandardSSD_ZRS")]
    StandardSsdZrs,
    #[serde(rename = "PremiumV2_LRS")]
    PremiumV2Lrs,
}
impl ::std::convert::From<&Self> for DiskStorageAccountTypes {
    fn from(value: &DiskStorageAccountTypes) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DiskStorageAccountTypes {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::StandardLrs => f.write_str("Standard_LRS"),
            Self::PremiumLrs => f.write_str("Premium_LRS"),
            Self::StandardSsdLrs => f.write_str("StandardSSD_LRS"),
            Self::UltraSsdLrs => f.write_str("UltraSSD_LRS"),
            Self::PremiumZrs => f.write_str("Premium_ZRS"),
            Self::StandardSsdZrs => f.write_str("StandardSSD_ZRS"),
            Self::PremiumV2Lrs => f.write_str("PremiumV2_LRS"),
        }
    }
}
impl ::std::str::FromStr for DiskStorageAccountTypes {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard_lrs" => Ok(Self::StandardLrs),
            "premium_lrs" => Ok(Self::PremiumLrs),
            "standardssd_lrs" => Ok(Self::StandardSsdLrs),
            "ultrassd_lrs" => Ok(Self::UltraSsdLrs),
            "premium_zrs" => Ok(Self::PremiumZrs),
            "standardssd_zrs" => Ok(Self::StandardSsdZrs),
            "premiumv2_lrs" => Ok(Self::PremiumV2Lrs),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DiskStorageAccountTypes {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DiskStorageAccountTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DiskStorageAccountTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Disk update resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Disk update resource.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/DiskUpdateProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/DiskSku"
///    },
///    "tags": {
///      "description": "Resource tags",
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
pub struct DiskUpdate {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<DiskUpdateProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<DiskSku>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&DiskUpdate> for DiskUpdate {
    fn from(value: &DiskUpdate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskUpdate {
    fn default() -> Self {
        Self {
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
        }
    }
}
///Disk resource update properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Disk resource update properties.",
///  "type": "object",
///  "properties": {
///    "availabilityPolicy": {
///      "$ref": "#/components/schemas/AvailabilityPolicy"
///    },
///    "burstingEnabled": {
///      "description": "Set to true to enable bursting beyond the provisioned performance target of the disk. Bursting is disabled by default. Does not apply to Ultra disks.",
///      "type": "boolean"
///    },
///    "dataAccessAuthMode": {
///      "$ref": "#/components/schemas/DataAccessAuthMode"
///    },
///    "diskAccessId": {
///      "description": "ARM id of the DiskAccess resource for using private endpoints on disks.",
///      "type": "string"
///    },
///    "diskIOPSReadOnly": {
///      "description": "The total number of IOPS that will be allowed across all VMs mounting the shared disk as ReadOnly. One operation can transfer between 4k and 256k bytes.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskIOPSReadWrite": {
///      "description": "The number of IOPS allowed for this disk; only settable for UltraSSD disks. One operation can transfer between 4k and 256k bytes.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskMBpsReadOnly": {
///      "description": "The total throughput (MBps) that will be allowed across all VMs mounting the shared disk as ReadOnly. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskMBpsReadWrite": {
///      "description": "The bandwidth allowed for this disk; only settable for UltraSSD disks. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.",
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskSizeGB": {
///      "description": "If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "encryptionSettingsCollection": {
///      "$ref": "#/components/schemas/EncryptionSettingsCollection"
///    },
///    "maxShares": {
///      "description": "The maximum number of VMs that can attach to the disk at the same time. Value greater than one indicates a disk that can be mounted on multiple VMs at the same time.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "networkAccessPolicy": {
///      "$ref": "#/components/schemas/NetworkAccessPolicy"
///    },
///    "optimizedForFrequentAttach": {
///      "description": "Setting this property to true improves reliability and performance of data disks that are frequently (more than 5 times a day) by detached from one virtual machine and attached to another. This property should not be set for disks that are not detached and attached frequently as it causes the disks to not align with the fault domain of the virtual machine.",
///      "type": "boolean"
///    },
///    "osType": {
///      "$ref": "#/components/schemas/OperatingSystemTypes"
///    },
///    "propertyUpdatesInProgress": {
///      "$ref": "#/components/schemas/PropertyUpdatesInProgress"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "purchasePlan": {
///      "$ref": "#/components/schemas/DiskPurchasePlan"
///    },
///    "supportedCapabilities": {
///      "$ref": "#/components/schemas/SupportedCapabilities"
///    },
///    "supportsHibernation": {
///      "description": "Indicates the OS on a disk supports hibernation.",
///      "type": "boolean"
///    },
///    "tier": {
///      "description": "Performance tier of the disk (e.g, P4, S10) as described here: https://azure.microsoft.com/en-us/pricing/details/managed-disks/. Does not apply to Ultra disks.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DiskUpdateProperties {
    #[serde(
        rename = "availabilityPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub availability_policy: ::std::option::Option<AvailabilityPolicy>,
    ///Set to true to enable bursting beyond the provisioned performance target of the disk. Bursting is disabled by default. Does not apply to Ultra disks.
    #[serde(
        rename = "burstingEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub bursting_enabled: ::std::option::Option<bool>,
    #[serde(
        rename = "dataAccessAuthMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_access_auth_mode: ::std::option::Option<DataAccessAuthMode>,
    ///ARM id of the DiskAccess resource for using private endpoints on disks.
    #[serde(
        rename = "diskAccessId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_access_id: ::std::option::Option<::std::string::String>,
    ///The total number of IOPS that will be allowed across all VMs mounting the shared disk as ReadOnly. One operation can transfer between 4k and 256k bytes.
    #[serde(
        rename = "diskIOPSReadOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_iops_read_only: ::std::option::Option<i64>,
    ///The number of IOPS allowed for this disk; only settable for UltraSSD disks. One operation can transfer between 4k and 256k bytes.
    #[serde(
        rename = "diskIOPSReadWrite",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_iops_read_write: ::std::option::Option<i64>,
    ///The total throughput (MBps) that will be allowed across all VMs mounting the shared disk as ReadOnly. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.
    #[serde(
        rename = "diskMBpsReadOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_m_bps_read_only: ::std::option::Option<i64>,
    ///The bandwidth allowed for this disk; only settable for UltraSSD disks. MBps means millions of bytes per second - MB here uses the ISO notation, of powers of 10.
    #[serde(
        rename = "diskMBpsReadWrite",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_m_bps_read_write: ::std::option::Option<i64>,
    ///If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.
    #[serde(
        rename = "diskSizeGB",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_size_gb: ::std::option::Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    #[serde(
        rename = "encryptionSettingsCollection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_settings_collection: ::std::option::Option<EncryptionSettingsCollection>,
    ///The maximum number of VMs that can attach to the disk at the same time. Value greater than one indicates a disk that can be mounted on multiple VMs at the same time.
    #[serde(
        rename = "maxShares",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub max_shares: ::std::option::Option<i32>,
    #[serde(
        rename = "networkAccessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_access_policy: ::std::option::Option<NetworkAccessPolicy>,
    ///Setting this property to true improves reliability and performance of data disks that are frequently (more than 5 times a day) by detached from one virtual machine and attached to another. This property should not be set for disks that are not detached and attached frequently as it causes the disks to not align with the fault domain of the virtual machine.
    #[serde(
        rename = "optimizedForFrequentAttach",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub optimized_for_frequent_attach: ::std::option::Option<bool>,
    #[serde(
        rename = "osType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub os_type: ::std::option::Option<OperatingSystemTypes>,
    #[serde(
        rename = "propertyUpdatesInProgress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub property_updates_in_progress: ::std::option::Option<PropertyUpdatesInProgress>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "purchasePlan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub purchase_plan: ::std::option::Option<DiskPurchasePlan>,
    #[serde(
        rename = "supportedCapabilities",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supported_capabilities: ::std::option::Option<SupportedCapabilities>,
    ///Indicates the OS on a disk supports hibernation.
    #[serde(
        rename = "supportsHibernation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_hibernation: ::std::option::Option<bool>,
    ///Performance tier of the disk (e.g, P4, S10) as described here: https://azure.microsoft.com/en-us/pricing/details/managed-disks/. Does not apply to Ultra disks.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DiskUpdateProperties> for DiskUpdateProperties {
    fn from(value: &DiskUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DiskUpdateProperties {
    fn default() -> Self {
        Self {
            availability_policy: Default::default(),
            bursting_enabled: Default::default(),
            data_access_auth_mode: Default::default(),
            disk_access_id: Default::default(),
            disk_iops_read_only: Default::default(),
            disk_iops_read_write: Default::default(),
            disk_m_bps_read_only: Default::default(),
            disk_m_bps_read_write: Default::default(),
            disk_size_gb: Default::default(),
            encryption: Default::default(),
            encryption_settings_collection: Default::default(),
            max_shares: Default::default(),
            network_access_policy: Default::default(),
            optimized_for_frequent_attach: Default::default(),
            os_type: Default::default(),
            property_updates_in_progress: Default::default(),
            public_network_access: Default::default(),
            purchase_plan: Default::default(),
            supported_capabilities: Default::default(),
            supports_hibernation: Default::default(),
            tier: Default::default(),
        }
    }
}
///Encryption at rest settings for disk or snapshot
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Encryption at rest settings for disk or snapshot",
///  "type": "object",
///  "properties": {
///    "diskEncryptionSetId": {
///      "description": "ResourceId of the disk encryption set to use for enabling encryption at rest.",
///      "type": "string"
///    },
///    "type": {
///      "$ref": "#/components/schemas/EncryptionType"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Encryption {
    ///ResourceId of the disk encryption set to use for enabling encryption at rest.
    #[serde(
        rename = "diskEncryptionSetId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_encryption_set_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<EncryptionType>,
}
impl ::std::convert::From<&Encryption> for Encryption {
    fn from(value: &Encryption) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Encryption {
    fn default() -> Self {
        Self {
            disk_encryption_set_id: Default::default(),
            type_: Default::default(),
        }
    }
}
///The managed identity for the disk encryption set. It should be given permission on the key vault before it can be used to encrypt disks.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The managed identity for the disk encryption set. It should be given permission on the key vault before it can be used to encrypt disks.",
///  "type": "object",
///  "properties": {
///    "principalId": {
///      "description": "The object id of the Managed Identity Resource. This will be sent to the RP from ARM via the x-ms-identity-principal-id header in the PUT request if the resource has a systemAssigned(implicit) identity",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "The tenant id of the Managed Identity Resource. This will be sent to the RP from ARM via the x-ms-client-tenant-id header in the PUT request if the resource has a systemAssigned(implicit) identity",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "$ref": "#/components/schemas/DiskEncryptionSetIdentityType"
///    },
///    "userAssignedIdentities": {
///      "description": "The list of user identities associated with the disk encryption set. The user identity dictionary key references will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.",
///      "type": "object",
///      "additionalProperties": {
///        "$ref": "#/components/schemas/UserAssignedIdentitiesValue"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionSetIdentity {
    ///The object id of the Managed Identity Resource. This will be sent to the RP from ARM via the x-ms-identity-principal-id header in the PUT request if the resource has a systemAssigned(implicit) identity
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
    ///The tenant id of the Managed Identity Resource. This will be sent to the RP from ARM via the x-ms-client-tenant-id header in the PUT request if the resource has a systemAssigned(implicit) identity
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<DiskEncryptionSetIdentityType>,
    ///The list of user identities associated with the disk encryption set. The user identity dictionary key references will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities:
        ::std::collections::HashMap<::std::string::String, UserAssignedIdentitiesValue>,
}
impl ::std::convert::From<&EncryptionSetIdentity> for EncryptionSetIdentity {
    fn from(value: &EncryptionSetIdentity) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionSetIdentity {
    fn default() -> Self {
        Self {
            principal_id: Default::default(),
            tenant_id: Default::default(),
            type_: Default::default(),
            user_assigned_identities: Default::default(),
        }
    }
}
///`EncryptionSetProperties`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "activeKey": {
///      "$ref": "#/components/schemas/KeyForDiskEncryptionSet"
///    },
///    "autoKeyRotationError": {
///      "$ref": "#/components/schemas/ApiError"
///    },
///    "encryptionType": {
///      "$ref": "#/components/schemas/DiskEncryptionSetType"
///    },
///    "federatedClientId": {
///      "description": "Multi-tenant application client id to access key vault in a different tenant. Setting the value to 'None' will clear the property.",
///      "type": "string"
///    },
///    "lastKeyRotationTimestamp": {
///      "description": "The time when the active key of this disk encryption set was updated.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "previousKeys": {
///      "description": "A readonly collection of key vault keys previously used by this disk encryption set while a key rotation is in progress. It will be empty if there is no ongoing key rotation.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/KeyForDiskEncryptionSet"
///      },
///      "x-ms-identifiers": [
///        "sourceVault/id"
///      ]
///    },
///    "provisioningState": {
///      "description": "The disk encryption set provisioning state.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "rotationToLatestKeyVersionEnabled": {
///      "description": "Set this flag to true to enable auto-updating of this disk encryption set to the latest key version.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionSetProperties {
    #[serde(
        rename = "activeKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub active_key: ::std::option::Option<KeyForDiskEncryptionSet>,
    #[serde(
        rename = "autoKeyRotationError",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub auto_key_rotation_error: ::std::option::Option<ApiError>,
    #[serde(
        rename = "encryptionType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_type: ::std::option::Option<DiskEncryptionSetType>,
    ///Multi-tenant application client id to access key vault in a different tenant. Setting the value to 'None' will clear the property.
    #[serde(
        rename = "federatedClientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub federated_client_id: ::std::option::Option<::std::string::String>,
    ///The time when the active key of this disk encryption set was updated.
    #[serde(
        rename = "lastKeyRotationTimestamp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_key_rotation_timestamp: ::std::option::Option<::std::string::String>,
    ///A readonly collection of key vault keys previously used by this disk encryption set while a key rotation is in progress. It will be empty if there is no ongoing key rotation.
    #[serde(
        rename = "previousKeys",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub previous_keys: ::std::vec::Vec<KeyForDiskEncryptionSet>,
    ///The disk encryption set provisioning state.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    ///Set this flag to true to enable auto-updating of this disk encryption set to the latest key version.
    #[serde(
        rename = "rotationToLatestKeyVersionEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rotation_to_latest_key_version_enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<&EncryptionSetProperties> for EncryptionSetProperties {
    fn from(value: &EncryptionSetProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionSetProperties {
    fn default() -> Self {
        Self {
            active_key: Default::default(),
            auto_key_rotation_error: Default::default(),
            encryption_type: Default::default(),
            federated_client_id: Default::default(),
            last_key_rotation_timestamp: Default::default(),
            previous_keys: Default::default(),
            provisioning_state: Default::default(),
            rotation_to_latest_key_version_enabled: Default::default(),
        }
    }
}
///Encryption settings for disk or snapshot
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Encryption settings for disk or snapshot",
///  "type": "object",
///  "required": [
///    "enabled"
///  ],
///  "properties": {
///    "enabled": {
///      "description": "Set this flag to true and provide DiskEncryptionKey and optional KeyEncryptionKey to enable encryption. Set this flag to false and remove DiskEncryptionKey and KeyEncryptionKey to disable encryption. If EncryptionSettings is null in the request object, the existing settings remain unchanged.",
///      "type": "boolean"
///    },
///    "encryptionSettings": {
///      "description": "A collection of encryption settings, one for each disk volume.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EncryptionSettingsElement"
///      },
///      "x-ms-identifiers": [
///        "diskEncryptionKey/sourceVault/id"
///      ]
///    },
///    "encryptionSettingsVersion": {
///      "description": "Describes what type of encryption is used for the disks. Once this field is set, it cannot be overwritten. '1.0' corresponds to Azure Disk Encryption with AAD app.'1.1' corresponds to Azure Disk Encryption.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionSettingsCollection {
    ///Set this flag to true and provide DiskEncryptionKey and optional KeyEncryptionKey to enable encryption. Set this flag to false and remove DiskEncryptionKey and KeyEncryptionKey to disable encryption. If EncryptionSettings is null in the request object, the existing settings remain unchanged.
    pub enabled: bool,
    ///A collection of encryption settings, one for each disk volume.
    #[serde(
        rename = "encryptionSettings",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_settings: ::std::vec::Vec<EncryptionSettingsElement>,
    ///Describes what type of encryption is used for the disks. Once this field is set, it cannot be overwritten. '1.0' corresponds to Azure Disk Encryption with AAD app.'1.1' corresponds to Azure Disk Encryption.
    #[serde(
        rename = "encryptionSettingsVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_settings_version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EncryptionSettingsCollection> for EncryptionSettingsCollection {
    fn from(value: &EncryptionSettingsCollection) -> Self {
        value.clone()
    }
}
///Encryption settings for one disk volume.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Encryption settings for one disk volume.",
///  "type": "object",
///  "properties": {
///    "diskEncryptionKey": {
///      "$ref": "#/components/schemas/KeyVaultAndSecretReference"
///    },
///    "keyEncryptionKey": {
///      "$ref": "#/components/schemas/KeyVaultAndKeyReference"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionSettingsElement {
    #[serde(
        rename = "diskEncryptionKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_encryption_key: ::std::option::Option<KeyVaultAndSecretReference>,
    #[serde(
        rename = "keyEncryptionKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_encryption_key: ::std::option::Option<KeyVaultAndKeyReference>,
}
impl ::std::convert::From<&EncryptionSettingsElement> for EncryptionSettingsElement {
    fn from(value: &EncryptionSettingsElement) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionSettingsElement {
    fn default() -> Self {
        Self {
            disk_encryption_key: Default::default(),
            key_encryption_key: Default::default(),
        }
    }
}
///The type of key used to encrypt the data of the disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of key used to encrypt the data of the disk.",
///  "type": "string",
///  "enum": [
///    "EncryptionAtRestWithPlatformKey",
///    "EncryptionAtRestWithCustomerKey",
///    "EncryptionAtRestWithPlatformAndCustomerKeys"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "EncryptionType",
///    "values": [
///      {
///        "description": "Disk is encrypted at rest with Platform managed key. It is the default encryption type. This is not a valid encryption type for disk encryption sets.",
///        "name": "EncryptionAtRestWithPlatformKey",
///        "value": "EncryptionAtRestWithPlatformKey"
///      },
///      {
///        "description": "Disk is encrypted at rest with Customer managed key that can be changed and revoked by a customer.",
///        "name": "EncryptionAtRestWithCustomerKey",
///        "value": "EncryptionAtRestWithCustomerKey"
///      },
///      {
///        "description": "Disk is encrypted at rest with 2 layers of encryption. One of the keys is Customer managed and the other key is Platform managed.",
///        "name": "EncryptionAtRestWithPlatformAndCustomerKeys",
///        "value": "EncryptionAtRestWithPlatformAndCustomerKeys"
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
pub enum EncryptionType {
    EncryptionAtRestWithPlatformKey,
    EncryptionAtRestWithCustomerKey,
    EncryptionAtRestWithPlatformAndCustomerKeys,
}
impl ::std::convert::From<&Self> for EncryptionType {
    fn from(value: &EncryptionType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EncryptionAtRestWithPlatformKey => f.write_str("EncryptionAtRestWithPlatformKey"),
            Self::EncryptionAtRestWithCustomerKey => f.write_str("EncryptionAtRestWithCustomerKey"),
            Self::EncryptionAtRestWithPlatformAndCustomerKeys => {
                f.write_str("EncryptionAtRestWithPlatformAndCustomerKeys")
            }
        }
    }
}
impl ::std::str::FromStr for EncryptionType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "encryptionatrestwithplatformkey" => Ok(Self::EncryptionAtRestWithPlatformKey),
            "encryptionatrestwithcustomerkey" => Ok(Self::EncryptionAtRestWithCustomerKey),
            "encryptionatrestwithplatformandcustomerkeys" => {
                Ok(Self::EncryptionAtRestWithPlatformAndCustomerKeys)
            }
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///Used to specify the file format when making request for SAS on a VHDX file format snapshot
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Used to specify the file format when making request for SAS on a VHDX file format snapshot",
///  "type": "string",
///  "enum": [
///    "VHD",
///    "VHDX"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "FileFormat",
///    "values": [
///      {
///        "description": "A VHD file is a disk image file in the Virtual Hard Disk file format.",
///        "name": "VHD",
///        "value": "VHD"
///      },
///      {
///        "description": "A VHDX file is a disk image file in the Virtual Hard Disk v2 file format.",
///        "name": "VHDX",
///        "value": "VHDX"
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
pub enum FileFormat {
    #[serde(rename = "VHD")]
    Vhd,
    #[serde(rename = "VHDX")]
    Vhdx,
}
impl ::std::convert::From<&Self> for FileFormat {
    fn from(value: &FileFormat) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for FileFormat {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Vhd => f.write_str("VHD"),
            Self::Vhdx => f.write_str("VHDX"),
        }
    }
}
impl ::std::str::FromStr for FileFormat {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "vhd" => Ok(Self::Vhd),
            "vhdx" => Ok(Self::Vhdx),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for FileFormat {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for FileFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for FileFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Data used for requesting a SAS.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Data used for requesting a SAS.",
///  "type": "object",
///  "required": [
///    "access",
///    "durationInSeconds"
///  ],
///  "properties": {
///    "access": {
///      "$ref": "#/components/schemas/AccessLevel"
///    },
///    "durationInSeconds": {
///      "description": "Time duration in seconds until the SAS access expires.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "fileFormat": {
///      "$ref": "#/components/schemas/FileFormat"
///    },
///    "getSecureVMGuestStateSAS": {
///      "description": "Set this flag to true to get additional SAS for VM guest state",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GrantAccessData {
    pub access: AccessLevel,
    ///Time duration in seconds until the SAS access expires.
    #[serde(rename = "durationInSeconds")]
    pub duration_in_seconds: i32,
    #[serde(
        rename = "fileFormat",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub file_format: ::std::option::Option<FileFormat>,
    ///Set this flag to true to get additional SAS for VM guest state
    #[serde(
        rename = "getSecureVMGuestStateSAS",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub get_secure_vm_guest_state_sas: ::std::option::Option<bool>,
}
impl ::std::convert::From<&GrantAccessData> for GrantAccessData {
    fn from(value: &GrantAccessData) -> Self {
        value.clone()
    }
}
///The hypervisor generation of the Virtual Machine. Applicable to OS disks only.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The hypervisor generation of the Virtual Machine. Applicable to OS disks only.",
///  "type": "string",
///  "enum": [
///    "V1",
///    "V2"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "HyperVGeneration",
///    "values": [
///      {
///        "name": "V1",
///        "value": "V1"
///      },
///      {
///        "name": "V2",
///        "value": "V2"
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
pub enum HyperVGeneration {
    V1,
    V2,
}
impl ::std::convert::From<&Self> for HyperVGeneration {
    fn from(value: &HyperVGeneration) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for HyperVGeneration {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::V1 => f.write_str("V1"),
            Self::V2 => f.write_str("V2"),
        }
    }
}
impl ::std::str::FromStr for HyperVGeneration {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "v1" => Ok(Self::V1),
            "v2" => Ok(Self::V2),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for HyperVGeneration {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for HyperVGeneration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for HyperVGeneration {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The source image used for creating the disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The source image used for creating the disk.",
///  "type": "object",
///  "properties": {
///    "communityGalleryImageId": {
///      "description": "A relative uri containing a community Azure Compute Gallery image reference.",
///      "type": "string"
///    },
///    "id": {
///      "description": "A relative uri containing either a Platform Image Repository, user image, or Azure Compute Gallery image reference.",
///      "type": "string"
///    },
///    "lun": {
///      "description": "If the disk is created from an image's data disk, this is an index that indicates which of the data disks in the image to use. For OS disks, this field is null.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "sharedGalleryImageId": {
///      "description": "A relative uri containing a direct shared Azure Compute Gallery image reference.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImageDiskReference {
    ///A relative uri containing a community Azure Compute Gallery image reference.
    #[serde(
        rename = "communityGalleryImageId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub community_gallery_image_id: ::std::option::Option<::std::string::String>,
    ///A relative uri containing either a Platform Image Repository, user image, or Azure Compute Gallery image reference.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///If the disk is created from an image's data disk, this is an index that indicates which of the data disks in the image to use. For OS disks, this field is null.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lun: ::std::option::Option<i32>,
    ///A relative uri containing a direct shared Azure Compute Gallery image reference.
    #[serde(
        rename = "sharedGalleryImageId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub shared_gallery_image_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ImageDiskReference> for ImageDiskReference {
    fn from(value: &ImageDiskReference) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ImageDiskReference {
    fn default() -> Self {
        Self {
            community_gallery_image_id: Default::default(),
            id: Default::default(),
            lun: Default::default(),
            shared_gallery_image_id: Default::default(),
        }
    }
}
///Inner error details.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Inner error details.",
///  "type": "object",
///  "properties": {
///    "errordetail": {
///      "description": "The internal error message or exception dump.",
///      "type": "string"
///    },
///    "exceptiontype": {
///      "description": "The exception type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct InnerError {
    ///The internal error message or exception dump.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub errordetail: ::std::option::Option<::std::string::String>,
    ///The exception type.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exceptiontype: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&InnerError> for InnerError {
    fn from(value: &InnerError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for InnerError {
    fn default() -> Self {
        Self {
            errordetail: Default::default(),
            exceptiontype: Default::default(),
        }
    }
}
///Key Vault Key Url to be used for server side encryption of Managed Disks and Snapshots
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Key Vault Key Url to be used for server side encryption of Managed Disks and Snapshots",
///  "type": "object",
///  "required": [
///    "keyUrl"
///  ],
///  "properties": {
///    "keyUrl": {
///      "description": "Fully versioned Key Url pointing to a key in KeyVault. Version segment of the Url is required regardless of rotationToLatestKeyVersionEnabled value.",
///      "type": "string"
///    },
///    "sourceVault": {
///      "$ref": "#/components/schemas/SourceVault"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyForDiskEncryptionSet {
    ///Fully versioned Key Url pointing to a key in KeyVault. Version segment of the Url is required regardless of rotationToLatestKeyVersionEnabled value.
    #[serde(rename = "keyUrl")]
    pub key_url: ::std::string::String,
    #[serde(
        rename = "sourceVault",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source_vault: ::std::option::Option<SourceVault>,
}
impl ::std::convert::From<&KeyForDiskEncryptionSet> for KeyForDiskEncryptionSet {
    fn from(value: &KeyForDiskEncryptionSet) -> Self {
        value.clone()
    }
}
///Key Vault Key Url and vault id of KeK, KeK is optional and when provided is used to unwrap the encryptionKey
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Key Vault Key Url and vault id of KeK, KeK is optional and when provided is used to unwrap the encryptionKey",
///  "type": "object",
///  "required": [
///    "keyUrl",
///    "sourceVault"
///  ],
///  "properties": {
///    "keyUrl": {
///      "description": "Url pointing to a key or secret in KeyVault",
///      "type": "string"
///    },
///    "sourceVault": {
///      "$ref": "#/components/schemas/SourceVault"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyVaultAndKeyReference {
    ///Url pointing to a key or secret in KeyVault
    #[serde(rename = "keyUrl")]
    pub key_url: ::std::string::String,
    #[serde(rename = "sourceVault")]
    pub source_vault: SourceVault,
}
impl ::std::convert::From<&KeyVaultAndKeyReference> for KeyVaultAndKeyReference {
    fn from(value: &KeyVaultAndKeyReference) -> Self {
        value.clone()
    }
}
///Key Vault Secret Url and vault id of the encryption key
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Key Vault Secret Url and vault id of the encryption key",
///  "type": "object",
///  "required": [
///    "secretUrl",
///    "sourceVault"
///  ],
///  "properties": {
///    "secretUrl": {
///      "description": "Url pointing to a key or secret in KeyVault",
///      "type": "string"
///    },
///    "sourceVault": {
///      "$ref": "#/components/schemas/SourceVault"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyVaultAndSecretReference {
    ///Url pointing to a key or secret in KeyVault
    #[serde(rename = "secretUrl")]
    pub secret_url: ::std::string::String,
    #[serde(rename = "sourceVault")]
    pub source_vault: SourceVault,
}
impl ::std::convert::From<&KeyVaultAndSecretReference> for KeyVaultAndSecretReference {
    fn from(value: &KeyVaultAndSecretReference) -> Self {
        value.clone()
    }
}
///Policy for accessing the disk via network.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Policy for accessing the disk via network.",
///  "type": "string",
///  "enum": [
///    "AllowAll",
///    "AllowPrivate",
///    "DenyAll"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "NetworkAccessPolicy",
///    "values": [
///      {
///        "description": "The disk can be exported or uploaded to from any network.",
///        "name": "AllowAll",
///        "value": "AllowAll"
///      },
///      {
///        "description": "The disk can be exported or uploaded to using a DiskAccess resource's private endpoints.",
///        "name": "AllowPrivate",
///        "value": "AllowPrivate"
///      },
///      {
///        "description": "The disk cannot be exported.",
///        "name": "DenyAll",
///        "value": "DenyAll"
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
pub enum NetworkAccessPolicy {
    AllowAll,
    AllowPrivate,
    DenyAll,
}
impl ::std::convert::From<&Self> for NetworkAccessPolicy {
    fn from(value: &NetworkAccessPolicy) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkAccessPolicy {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AllowAll => f.write_str("AllowAll"),
            Self::AllowPrivate => f.write_str("AllowPrivate"),
            Self::DenyAll => f.write_str("DenyAll"),
        }
    }
}
impl ::std::str::FromStr for NetworkAccessPolicy {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allowall" => Ok(Self::AllowAll),
            "allowprivate" => Ok(Self::AllowPrivate),
            "denyall" => Ok(Self::DenyAll),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkAccessPolicy {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkAccessPolicy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkAccessPolicy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The Operating System type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Operating System type.",
///  "type": "string",
///  "enum": [
///    "Windows",
///    "Linux"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "OperatingSystemTypes"
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
pub enum OperatingSystemTypes {
    Windows,
    Linux,
}
impl ::std::convert::From<&Self> for OperatingSystemTypes {
    fn from(value: &OperatingSystemTypes) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for OperatingSystemTypes {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Windows => f.write_str("Windows"),
            Self::Linux => f.write_str("Linux"),
        }
    }
}
impl ::std::str::FromStr for OperatingSystemTypes {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for OperatingSystemTypes {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for OperatingSystemTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for OperatingSystemTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The Private Endpoint resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Private Endpoint resource.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The ARM identifier for Private Endpoint",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpoint {
    ///The ARM identifier for Private Endpoint
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateEndpoint> for PrivateEndpoint {
    fn from(value: &PrivateEndpoint) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpoint {
    fn default() -> Self {
        Self {
            id: Default::default(),
        }
    }
}
///The Private Endpoint Connection resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Private Endpoint Connection resource.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnection {
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
    pub properties: ::std::option::Option<PrivateEndpointConnectionProperties>,
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
impl ::std::convert::From<&PrivateEndpointConnection> for PrivateEndpointConnection {
    fn from(value: &PrivateEndpointConnection) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointConnection {
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
///A list of private link resources
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A list of private link resources",
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
///      "description": "The PrivateEndpointConnection items on this page",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnectionListResult {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The PrivateEndpointConnection items on this page
    pub value: ::std::vec::Vec<PrivateEndpointConnection>,
}
impl ::std::convert::From<&PrivateEndpointConnectionListResult>
    for PrivateEndpointConnectionListResult
{
    fn from(value: &PrivateEndpointConnectionListResult) -> Self {
        value.clone()
    }
}
///Properties of the PrivateEndpointConnectProperties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the PrivateEndpointConnectProperties.",
///  "type": "object",
///  "required": [
///    "privateLinkServiceConnectionState"
///  ],
///  "properties": {
///    "privateEndpoint": {
///      "$ref": "#/components/schemas/PrivateEndpoint"
///    },
///    "privateLinkServiceConnectionState": {
///      "$ref": "#/components/schemas/PrivateLinkServiceConnectionState"
///    },
///    "provisioningState": {
///      "$ref": "#/components/schemas/PrivateEndpointConnectionProvisioningState"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateEndpointConnectionProperties {
    #[serde(
        rename = "privateEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint: ::std::option::Option<PrivateEndpoint>,
    #[serde(rename = "privateLinkServiceConnectionState")]
    pub private_link_service_connection_state: PrivateLinkServiceConnectionState,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<PrivateEndpointConnectionProvisioningState>,
}
impl ::std::convert::From<&PrivateEndpointConnectionProperties>
    for PrivateEndpointConnectionProperties
{
    fn from(value: &PrivateEndpointConnectionProperties) -> Self {
        value.clone()
    }
}
///The current provisioning state.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The current provisioning state.",
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "Creating",
///    "Deleting",
///    "Failed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateEndpointConnectionProvisioningState",
///    "values": [
///      {
///        "name": "Succeeded",
///        "value": "Succeeded"
///      },
///      {
///        "name": "Creating",
///        "value": "Creating"
///      },
///      {
///        "name": "Deleting",
///        "value": "Deleting"
///      },
///      {
///        "name": "Failed",
///        "value": "Failed"
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
pub enum PrivateEndpointConnectionProvisioningState {
    Succeeded,
    Creating,
    Deleting,
    Failed,
}
impl ::std::convert::From<&Self> for PrivateEndpointConnectionProvisioningState {
    fn from(value: &PrivateEndpointConnectionProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateEndpointConnectionProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Creating => f.write_str("Creating"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Failed => f.write_str("Failed"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointConnectionProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "creating" => Ok(Self::Creating),
            "deleting" => Ok(Self::Deleting),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateEndpointConnectionProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for PrivateEndpointConnectionProvisioningState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateEndpointConnectionProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The private endpoint connection status.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The private endpoint connection status.",
///  "type": "string",
///  "enum": [
///    "Pending",
///    "Approved",
///    "Rejected"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateEndpointServiceConnectionStatus",
///    "values": [
///      {
///        "name": "Pending",
///        "value": "Pending"
///      },
///      {
///        "name": "Approved",
///        "value": "Approved"
///      },
///      {
///        "name": "Rejected",
///        "value": "Rejected"
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
pub enum PrivateEndpointServiceConnectionStatus {
    Pending,
    Approved,
    Rejected,
}
impl ::std::convert::From<&Self> for PrivateEndpointServiceConnectionStatus {
    fn from(value: &PrivateEndpointServiceConnectionStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PrivateEndpointServiceConnectionStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Pending => f.write_str("Pending"),
            Self::Approved => f.write_str("Approved"),
            Self::Rejected => f.write_str("Rejected"),
        }
    }
}
impl ::std::str::FromStr for PrivateEndpointServiceConnectionStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PrivateEndpointServiceConnectionStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PrivateEndpointServiceConnectionStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PrivateEndpointServiceConnectionStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A private link resource
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A private link resource",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "private link resource Id",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "private link resource name",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkResourceProperties"
///    },
///    "type": {
///      "description": "private link resource type",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResource {
    ///private link resource Id
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///private link resource name
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
    pub properties: ::std::option::Option<PrivateLinkResourceProperties>,
    ///private link resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkResource> for PrivateLinkResource {
    fn from(value: &PrivateLinkResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResource {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///A list of private link resources
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A list of private link resources",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "Array of private link resources",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateLinkResource"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResourceListResult {
    ///Array of private link resources
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PrivateLinkResource>,
}
impl ::std::convert::From<&PrivateLinkResourceListResult> for PrivateLinkResourceListResult {
    fn from(value: &PrivateLinkResourceListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResourceListResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///Properties of a private link resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of a private link resource.",
///  "type": "object",
///  "properties": {
///    "groupId": {
///      "description": "The private link resource group id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "requiredMembers": {
///      "description": "The private link resource required member names.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "requiredZoneNames": {
///      "description": "The private link resource DNS zone name.",
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
pub struct PrivateLinkResourceProperties {
    ///The private link resource group id.
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<::std::string::String>,
    ///The private link resource required member names.
    #[serde(
        rename = "requiredMembers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_members: ::std::vec::Vec<::std::string::String>,
    ///The private link resource DNS zone name.
    #[serde(
        rename = "requiredZoneNames",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub required_zone_names: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&PrivateLinkResourceProperties> for PrivateLinkResourceProperties {
    fn from(value: &PrivateLinkResourceProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateLinkResourceProperties {
    fn default() -> Self {
        Self {
            group_id: Default::default(),
            required_members: Default::default(),
            required_zone_names: Default::default(),
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
///  "type": "object",
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
///      "$ref": "#/components/schemas/PrivateEndpointServiceConnectionStatus"
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
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<PrivateEndpointServiceConnectionStatus>,
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
///Properties of the disk for which update is pending.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the disk for which update is pending.",
///  "type": "object",
///  "properties": {
///    "targetTier": {
///      "description": "The target performance tier of the disk if a tier change operation is in progress.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PropertyUpdatesInProgress {
    ///The target performance tier of the disk if a tier change operation is in progress.
    #[serde(
        rename = "targetTier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_tier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PropertyUpdatesInProgress> for PropertyUpdatesInProgress {
    fn from(value: &PropertyUpdatesInProgress) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PropertyUpdatesInProgress {
    fn default() -> Self {
        Self {
            target_tier: Default::default(),
        }
    }
}
///If this field is set on a snapshot and createOption is CopyStart, the snapshot will be copied at a quicker speed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "If this field is set on a snapshot and createOption is CopyStart, the snapshot will be copied at a quicker speed.",
///  "type": "string",
///  "enum": [
///    "None",
///    "Enhanced"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProvisionedBandwidthCopyOption",
///    "values": [
///      {
///        "name": "None",
///        "value": "None"
///      },
///      {
///        "name": "Enhanced",
///        "value": "Enhanced"
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
pub enum ProvisionedBandwidthCopyOption {
    None,
    Enhanced,
}
impl ::std::convert::From<&Self> for ProvisionedBandwidthCopyOption {
    fn from(value: &ProvisionedBandwidthCopyOption) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ProvisionedBandwidthCopyOption {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Enhanced => f.write_str("Enhanced"),
        }
    }
}
impl ::std::str::FromStr for ProvisionedBandwidthCopyOption {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "enhanced" => Ok(Self::Enhanced),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ProvisionedBandwidthCopyOption {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ProvisionedBandwidthCopyOption {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ProvisionedBandwidthCopyOption {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///Policy for controlling export on the disk.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Policy for controlling export on the disk.",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicNetworkAccess",
///    "values": [
///      {
///        "description": "You can generate a SAS URI to access the underlying data of the disk publicly on the internet when NetworkAccessPolicy is set to AllowAll. You can access the data via the SAS URI only from your trusted Azure VNET when NetworkAccessPolicy is set to AllowPrivate.",
///        "name": "Enabled",
///        "value": "Enabled"
///      },
///      {
///        "description": "You cannot access the underlying data of the disk publicly on the internet even when NetworkAccessPolicy is set to AllowAll. You can access the data via the SAS URI only from your trusted Azure VNET when NetworkAccessPolicy is set to AllowPrivate.",
///        "name": "Disabled",
///        "value": "Disabled"
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
pub enum PublicNetworkAccess {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for PublicNetworkAccess {
    fn from(value: &PublicNetworkAccess) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for PublicNetworkAccess {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for PublicNetworkAccess {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for PublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for PublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for PublicNetworkAccess {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///The List resources which are encrypted with the disk encryption set.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List resources which are encrypted with the disk encryption set.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "nextLink": {
///      "description": "The uri to fetch the next page of encrypted resources. Call ListNext() with this to fetch the next page of encrypted resources.",
///      "type": "string",
///      "format": "uri"
///    },
///    "value": {
///      "description": "A list of IDs or Owner IDs of resources which are encrypted with the disk encryption set.",
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
pub struct ResourceUriList {
    ///The uri to fetch the next page of encrypted resources. Call ListNext() with this to fetch the next page of encrypted resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A list of IDs or Owner IDs of resources which are encrypted with the disk encryption set.
    pub value: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ResourceUriList> for ResourceUriList {
    fn from(value: &ResourceUriList) -> Self {
        value.clone()
    }
}
///`ShareInfoElement`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "vmUri": {
///      "description": "A relative URI containing the ID of the VM that has the disk attached.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ShareInfoElement {
    ///A relative URI containing the ID of the VM that has the disk attached.
    #[serde(
        rename = "vmUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub vm_uri: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ShareInfoElement> for ShareInfoElement {
    fn from(value: &ShareInfoElement) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ShareInfoElement {
    fn default() -> Self {
        Self {
            vm_uri: Default::default(),
        }
    }
}
///Snapshot resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Snapshot resource.",
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
///    "managedBy": {
///      "description": "Unused. Always Null.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/SnapshotProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/SnapshotSku"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Snapshot {
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The geo-location where the resource lives
    pub location: ::std::string::String,
    ///Unused. Always Null.
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
    pub properties: ::std::option::Option<SnapshotProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<SnapshotSku>,
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
impl ::std::convert::From<&Snapshot> for Snapshot {
    fn from(value: &Snapshot) -> Self {
        value.clone()
    }
}
///The state of snapshot which determines the access availability of the snapshot.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The state of snapshot which determines the access availability of the snapshot.",
///  "type": "string",
///  "enum": [
///    "Unknown",
///    "Pending",
///    "Available",
///    "InstantAccess",
///    "AvailableWithInstantAccess"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SnapshotAccessState",
///    "values": [
///      {
///        "description": "Default value.",
///        "name": "Unknown",
///        "value": "Unknown"
///      },
///      {
///        "description": "The snapshot cannot be used for restore, copy or download to offline.",
///        "name": "Pending",
///        "value": "Pending"
///      },
///      {
///        "description": "The snapshot can be used for restore, copy to different region, and download to offline.",
///        "name": "Available",
///        "value": "Available"
///      },
///      {
///        "description": "The snapshot can be used for restoring disks with fast performance but cannot be copied or downloaded.",
///        "name": "InstantAccess",
///        "value": "InstantAccess"
///      },
///      {
///        "description": "The snapshot can be used for restoring disks with fast performance, copied and downloaded.",
///        "name": "AvailableWithInstantAccess",
///        "value": "AvailableWithInstantAccess"
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
pub enum SnapshotAccessState {
    Unknown,
    Pending,
    Available,
    InstantAccess,
    AvailableWithInstantAccess,
}
impl ::std::convert::From<&Self> for SnapshotAccessState {
    fn from(value: &SnapshotAccessState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SnapshotAccessState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Unknown => f.write_str("Unknown"),
            Self::Pending => f.write_str("Pending"),
            Self::Available => f.write_str("Available"),
            Self::InstantAccess => f.write_str("InstantAccess"),
            Self::AvailableWithInstantAccess => f.write_str("AvailableWithInstantAccess"),
        }
    }
}
impl ::std::str::FromStr for SnapshotAccessState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "unknown" => Ok(Self::Unknown),
            "pending" => Ok(Self::Pending),
            "available" => Ok(Self::Available),
            "instantaccess" => Ok(Self::InstantAccess),
            "availablewithinstantaccess" => Ok(Self::AvailableWithInstantAccess),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SnapshotAccessState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SnapshotAccessState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SnapshotAccessState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The List Snapshots operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List Snapshots operation response.",
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
///      "description": "A list of snapshots.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Snapshot"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SnapshotList {
    ///The link to the next page of items
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A list of snapshots.
    pub value: ::std::vec::Vec<Snapshot>,
}
impl ::std::convert::From<&SnapshotList> for SnapshotList {
    fn from(value: &SnapshotList) -> Self {
        value.clone()
    }
}
///Snapshot resource properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Snapshot resource properties.",
///  "type": "object",
///  "required": [
///    "creationData"
///  ],
///  "properties": {
///    "completionPercent": {
///      "description": "Percentage complete for the background copy when a resource is created via the CopyStart operation.",
///      "type": "number",
///      "format": "float"
///    },
///    "copyCompletionError": {
///      "$ref": "#/components/schemas/CopyCompletionError"
///    },
///    "creationData": {
///      "$ref": "#/components/schemas/CreationData"
///    },
///    "dataAccessAuthMode": {
///      "$ref": "#/components/schemas/DataAccessAuthMode"
///    },
///    "diskAccessId": {
///      "description": "ARM id of the DiskAccess resource for using private endpoints on disks.",
///      "type": "string"
///    },
///    "diskSizeBytes": {
///      "description": "The size of the disk in bytes. This field is read only.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int64"
///    },
///    "diskSizeGB": {
///      "description": "If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "diskState": {
///      "$ref": "#/components/schemas/DiskState"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "encryptionSettingsCollection": {
///      "$ref": "#/components/schemas/EncryptionSettingsCollection"
///    },
///    "hyperVGeneration": {
///      "$ref": "#/components/schemas/HyperVGeneration"
///    },
///    "incremental": {
///      "description": "Whether a snapshot is incremental. Incremental snapshots on the same disk occupy less space than full snapshots and can be diffed.",
///      "type": "boolean"
///    },
///    "incrementalSnapshotFamilyId": {
///      "description": "Incremental snapshots for a disk share an incremental snapshot family id. The Get Page Range Diff API can only be called on incremental snapshots with the same family id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "networkAccessPolicy": {
///      "$ref": "#/components/schemas/NetworkAccessPolicy"
///    },
///    "osType": {
///      "$ref": "#/components/schemas/OperatingSystemTypes"
///    },
///    "provisioningState": {
///      "description": "The disk provisioning state.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "purchasePlan": {
///      "$ref": "#/components/schemas/DiskPurchasePlan"
///    },
///    "securityProfile": {
///      "$ref": "#/components/schemas/DiskSecurityProfile"
///    },
///    "snapshotAccessState": {
///      "$ref": "#/components/schemas/SnapshotAccessState"
///    },
///    "supportedCapabilities": {
///      "$ref": "#/components/schemas/SupportedCapabilities"
///    },
///    "supportsHibernation": {
///      "description": "Indicates the OS on a snapshot supports hibernation.",
///      "type": "boolean"
///    },
///    "timeCreated": {
///      "description": "The time when the snapshot was created.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "uniqueId": {
///      "description": "Unique Guid identifying the resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SnapshotProperties {
    #[serde(
        rename = "completionPercent",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub completion_percent: ::std::option::Option<f32>,
    #[serde(
        rename = "copyCompletionError",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub copy_completion_error: ::std::option::Option<CopyCompletionError>,
    #[serde(rename = "creationData")]
    pub creation_data: CreationData,
    #[serde(
        rename = "dataAccessAuthMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_access_auth_mode: ::std::option::Option<DataAccessAuthMode>,
    ///ARM id of the DiskAccess resource for using private endpoints on disks.
    #[serde(
        rename = "diskAccessId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_access_id: ::std::option::Option<::std::string::String>,
    ///The size of the disk in bytes. This field is read only.
    #[serde(
        rename = "diskSizeBytes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_size_bytes: ::std::option::Option<i64>,
    ///If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.
    #[serde(
        rename = "diskSizeGB",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_size_gb: ::std::option::Option<i32>,
    #[serde(
        rename = "diskState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_state: ::std::option::Option<DiskState>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    #[serde(
        rename = "encryptionSettingsCollection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_settings_collection: ::std::option::Option<EncryptionSettingsCollection>,
    #[serde(
        rename = "hyperVGeneration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub hyper_v_generation: ::std::option::Option<HyperVGeneration>,
    ///Whether a snapshot is incremental. Incremental snapshots on the same disk occupy less space than full snapshots and can be diffed.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub incremental: ::std::option::Option<bool>,
    ///Incremental snapshots for a disk share an incremental snapshot family id. The Get Page Range Diff API can only be called on incremental snapshots with the same family id.
    #[serde(
        rename = "incrementalSnapshotFamilyId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub incremental_snapshot_family_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "networkAccessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_access_policy: ::std::option::Option<NetworkAccessPolicy>,
    #[serde(
        rename = "osType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub os_type: ::std::option::Option<OperatingSystemTypes>,
    ///The disk provisioning state.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "purchasePlan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub purchase_plan: ::std::option::Option<DiskPurchasePlan>,
    #[serde(
        rename = "securityProfile",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub security_profile: ::std::option::Option<DiskSecurityProfile>,
    #[serde(
        rename = "snapshotAccessState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub snapshot_access_state: ::std::option::Option<SnapshotAccessState>,
    #[serde(
        rename = "supportedCapabilities",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supported_capabilities: ::std::option::Option<SupportedCapabilities>,
    ///Indicates the OS on a snapshot supports hibernation.
    #[serde(
        rename = "supportsHibernation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_hibernation: ::std::option::Option<bool>,
    ///The time when the snapshot was created.
    #[serde(
        rename = "timeCreated",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub time_created: ::std::option::Option<::std::string::String>,
    ///Unique Guid identifying the resource.
    #[serde(
        rename = "uniqueId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub unique_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SnapshotProperties> for SnapshotProperties {
    fn from(value: &SnapshotProperties) -> Self {
        value.clone()
    }
}
///The snapshots sku name. Can be Standard_LRS, Premium_LRS, or Standard_ZRS. This is an optional parameter for incremental snapshot and the default behavior is the SKU will be set to the same sku as the previous snapshot
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The snapshots sku name. Can be Standard_LRS, Premium_LRS, or Standard_ZRS. This is an optional parameter for incremental snapshot and the default behavior is the SKU will be set to the same sku as the previous snapshot",
///  "type": "object",
///  "properties": {
///    "name": {
///      "$ref": "#/components/schemas/SnapshotStorageAccountTypes"
///    },
///    "tier": {
///      "description": "The sku tier.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SnapshotSku {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<SnapshotStorageAccountTypes>,
    ///The sku tier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SnapshotSku> for SnapshotSku {
    fn from(value: &SnapshotSku) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SnapshotSku {
    fn default() -> Self {
        Self {
            name: Default::default(),
            tier: Default::default(),
        }
    }
}
///The sku name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The sku name.",
///  "type": "string",
///  "enum": [
///    "Standard_LRS",
///    "Premium_LRS",
///    "Standard_ZRS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SnapshotStorageAccountTypes",
///    "values": [
///      {
///        "description": "Standard HDD locally redundant storage",
///        "name": "Standard_LRS",
///        "value": "Standard_LRS"
///      },
///      {
///        "description": "Premium SSD locally redundant storage",
///        "name": "Premium_LRS",
///        "value": "Premium_LRS"
///      },
///      {
///        "description": "Standard zone redundant storage",
///        "name": "Standard_ZRS",
///        "value": "Standard_ZRS"
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
pub enum SnapshotStorageAccountTypes {
    #[serde(rename = "Standard_LRS")]
    StandardLrs,
    #[serde(rename = "Premium_LRS")]
    PremiumLrs,
    #[serde(rename = "Standard_ZRS")]
    StandardZrs,
}
impl ::std::convert::From<&Self> for SnapshotStorageAccountTypes {
    fn from(value: &SnapshotStorageAccountTypes) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SnapshotStorageAccountTypes {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::StandardLrs => f.write_str("Standard_LRS"),
            Self::PremiumLrs => f.write_str("Premium_LRS"),
            Self::StandardZrs => f.write_str("Standard_ZRS"),
        }
    }
}
impl ::std::str::FromStr for SnapshotStorageAccountTypes {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard_lrs" => Ok(Self::StandardLrs),
            "premium_lrs" => Ok(Self::PremiumLrs),
            "standard_zrs" => Ok(Self::StandardZrs),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SnapshotStorageAccountTypes {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SnapshotStorageAccountTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SnapshotStorageAccountTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Snapshot update resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Snapshot update resource.",
///  "type": "object",
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/SnapshotUpdateProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/SnapshotSku"
///    },
///    "tags": {
///      "description": "Resource tags",
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
pub struct SnapshotUpdate {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<SnapshotUpdateProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<SnapshotSku>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&SnapshotUpdate> for SnapshotUpdate {
    fn from(value: &SnapshotUpdate) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SnapshotUpdate {
    fn default() -> Self {
        Self {
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
        }
    }
}
///Snapshot resource update properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Snapshot resource update properties.",
///  "type": "object",
///  "properties": {
///    "dataAccessAuthMode": {
///      "$ref": "#/components/schemas/DataAccessAuthMode"
///    },
///    "diskAccessId": {
///      "description": "ARM id of the DiskAccess resource for using private endpoints on disks.",
///      "type": "string"
///    },
///    "diskSizeGB": {
///      "description": "If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "encryptionSettingsCollection": {
///      "$ref": "#/components/schemas/EncryptionSettingsCollection"
///    },
///    "networkAccessPolicy": {
///      "$ref": "#/components/schemas/NetworkAccessPolicy"
///    },
///    "osType": {
///      "$ref": "#/components/schemas/OperatingSystemTypes"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "snapshotAccessState": {
///      "$ref": "#/components/schemas/SnapshotAccessState"
///    },
///    "supportedCapabilities": {
///      "$ref": "#/components/schemas/SupportedCapabilities"
///    },
///    "supportsHibernation": {
///      "description": "Indicates the OS on a snapshot supports hibernation.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SnapshotUpdateProperties {
    #[serde(
        rename = "dataAccessAuthMode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub data_access_auth_mode: ::std::option::Option<DataAccessAuthMode>,
    ///ARM id of the DiskAccess resource for using private endpoints on disks.
    #[serde(
        rename = "diskAccessId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_access_id: ::std::option::Option<::std::string::String>,
    ///If creationData.createOption is Empty, this field is mandatory and it indicates the size of the disk to create. If this field is present for updates or creation with other options, it indicates a resize. Resizes are only allowed if the disk is not attached to a running VM, and can only increase the disk's size.
    #[serde(
        rename = "diskSizeGB",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_size_gb: ::std::option::Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    #[serde(
        rename = "encryptionSettingsCollection",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption_settings_collection: ::std::option::Option<EncryptionSettingsCollection>,
    #[serde(
        rename = "networkAccessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_access_policy: ::std::option::Option<NetworkAccessPolicy>,
    #[serde(
        rename = "osType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub os_type: ::std::option::Option<OperatingSystemTypes>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "snapshotAccessState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub snapshot_access_state: ::std::option::Option<SnapshotAccessState>,
    #[serde(
        rename = "supportedCapabilities",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supported_capabilities: ::std::option::Option<SupportedCapabilities>,
    ///Indicates the OS on a snapshot supports hibernation.
    #[serde(
        rename = "supportsHibernation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_hibernation: ::std::option::Option<bool>,
}
impl ::std::convert::From<&SnapshotUpdateProperties> for SnapshotUpdateProperties {
    fn from(value: &SnapshotUpdateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SnapshotUpdateProperties {
    fn default() -> Self {
        Self {
            data_access_auth_mode: Default::default(),
            disk_access_id: Default::default(),
            disk_size_gb: Default::default(),
            encryption: Default::default(),
            encryption_settings_collection: Default::default(),
            network_access_policy: Default::default(),
            os_type: Default::default(),
            public_network_access: Default::default(),
            snapshot_access_state: Default::default(),
            supported_capabilities: Default::default(),
            supports_hibernation: Default::default(),
        }
    }
}
///The vault id is an Azure Resource Manager Resource id in the form /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.KeyVault/vaults/{vaultName}
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The vault id is an Azure Resource Manager Resource id in the form /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.KeyVault/vaults/{vaultName}",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Resource Id",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SourceVault {
    ///Resource Id
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SourceVault> for SourceVault {
    fn from(value: &SourceVault) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SourceVault {
    fn default() -> Self {
        Self {
            id: Default::default(),
        }
    }
}
///List of supported capabilities persisted on the disk resource for VM use.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of supported capabilities persisted on the disk resource for VM use.",
///  "type": "object",
///  "properties": {
///    "acceleratedNetwork": {
///      "description": "True if the image from which the OS disk is created supports accelerated networking.",
///      "type": "boolean"
///    },
///    "architecture": {
///      "$ref": "#/components/schemas/Architecture"
///    },
///    "diskControllerTypes": {
///      "description": "The disk controllers that an OS disk supports. If set it can be SCSI or SCSI, NVME or NVME, SCSI.",
///      "type": "string"
///    },
///    "supportedSecurityOption": {
///      "$ref": "#/components/schemas/SupportedSecurityOption"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SupportedCapabilities {
    ///True if the image from which the OS disk is created supports accelerated networking.
    #[serde(
        rename = "acceleratedNetwork",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub accelerated_network: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub architecture: ::std::option::Option<Architecture>,
    ///The disk controllers that an OS disk supports. If set it can be SCSI or SCSI, NVME or NVME, SCSI.
    #[serde(
        rename = "diskControllerTypes",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub disk_controller_types: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "supportedSecurityOption",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supported_security_option: ::std::option::Option<SupportedSecurityOption>,
}
impl ::std::convert::From<&SupportedCapabilities> for SupportedCapabilities {
    fn from(value: &SupportedCapabilities) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SupportedCapabilities {
    fn default() -> Self {
        Self {
            accelerated_network: Default::default(),
            architecture: Default::default(),
            disk_controller_types: Default::default(),
            supported_security_option: Default::default(),
        }
    }
}
///Refers to the security capability of the disk supported to create a Trusted launch or Confidential VM
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Refers to the security capability of the disk supported to create a Trusted launch or Confidential VM",
///  "type": "string",
///  "enum": [
///    "TrustedLaunchSupported",
///    "TrustedLaunchAndConfidentialVMSupported"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SupportedSecurityOption",
///    "values": [
///      {
///        "description": "The disk supports creating Trusted Launch VMs.",
///        "name": "TrustedLaunchSupported",
///        "value": "TrustedLaunchSupported"
///      },
///      {
///        "description": "The disk supports creating both Trusted Launch and Confidential VMs.",
///        "name": "TrustedLaunchAndConfidentialVMSupported",
///        "value": "TrustedLaunchAndConfidentialVMSupported"
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
pub enum SupportedSecurityOption {
    TrustedLaunchSupported,
    #[serde(rename = "TrustedLaunchAndConfidentialVMSupported")]
    TrustedLaunchAndConfidentialVmSupported,
}
impl ::std::convert::From<&Self> for SupportedSecurityOption {
    fn from(value: &SupportedSecurityOption) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SupportedSecurityOption {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::TrustedLaunchSupported => f.write_str("TrustedLaunchSupported"),
            Self::TrustedLaunchAndConfidentialVmSupported => {
                f.write_str("TrustedLaunchAndConfidentialVMSupported")
            }
        }
    }
}
impl ::std::str::FromStr for SupportedSecurityOption {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "trustedlaunchsupported" => Ok(Self::TrustedLaunchSupported),
            "trustedlaunchandconfidentialvmsupported" => {
                Ok(Self::TrustedLaunchAndConfidentialVmSupported)
            }
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SupportedSecurityOption {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SupportedSecurityOption {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SupportedSecurityOption {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
    ///Fully qualified resource ID for the resource. Ex - /subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/{resourceProviderNamespace}/{resourceType}/{resourceName}
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
///`UserAssignedIdentitiesValue`
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
pub struct UserAssignedIdentitiesValue {
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
impl ::std::convert::From<&UserAssignedIdentitiesValue> for UserAssignedIdentitiesValue {
    fn from(value: &UserAssignedIdentitiesValue) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UserAssignedIdentitiesValue {
    fn default() -> Self {
        Self {
            client_id: Default::default(),
            principal_id: Default::default(),
        }
    }
}
