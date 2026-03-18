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
///This defines account-level immutability policy properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This defines account-level immutability policy properties.",
///  "type": "object",
///  "properties": {
///    "allowProtectedAppendWrites": {
///      "description": "This property can only be changed for disabled and unlocked time-based retention policies. When enabled, new blocks can be written to an append blob while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted.",
///      "type": "boolean"
///    },
///    "immutabilityPeriodSinceCreationInDays": {
///      "description": "The immutability period for the blobs in the container since the policy creation, in days.",
///      "type": "integer",
///      "format": "int32",
///      "maximum": 146000.0,
///      "minimum": 1.0
///    },
///    "state": {
///      "description": "The ImmutabilityPolicy state defines the mode of the policy. Disabled state disables the policy, Unlocked state allows increase and decrease of immutability retention time and also allows toggling allowProtectedAppendWrites property, Locked state only allows the increase of the immutability retention time. A policy can only be created in a Disabled or Unlocked state and can be toggled between the two states. Only a policy in an Unlocked state can transition to a Locked state which cannot be reverted.",
///      "type": "string",
///      "enum": [
///        "Unlocked",
///        "Locked",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AccountImmutabilityPolicyState"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AccountImmutabilityPolicyProperties {
    ///This property can only be changed for disabled and unlocked time-based retention policies. When enabled, new blocks can be written to an append blob while maintaining immutability protection and compliance. Only new blocks can be added and any existing blocks cannot be modified or deleted.
    #[serde(
        rename = "allowProtectedAppendWrites",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_protected_append_writes: ::std::option::Option<bool>,
    ///The immutability period for the blobs in the container since the policy creation, in days.
    #[serde(
        rename = "immutabilityPeriodSinceCreationInDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutability_period_since_creation_in_days: ::std::option::Option<::std::num::NonZeroU32>,
    ///The ImmutabilityPolicy state defines the mode of the policy. Disabled state disables the policy, Unlocked state allows increase and decrease of immutability retention time and also allows toggling allowProtectedAppendWrites property, Locked state only allows the increase of the immutability retention time. A policy can only be created in a Disabled or Unlocked state and can be toggled between the two states. Only a policy in an Unlocked state can transition to a Locked state which cannot be reverted.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub state: ::std::option::Option<AccountImmutabilityPolicyPropertiesState>,
}
impl ::std::convert::From<&AccountImmutabilityPolicyProperties>
    for AccountImmutabilityPolicyProperties
{
    fn from(value: &AccountImmutabilityPolicyProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AccountImmutabilityPolicyProperties {
    fn default() -> Self {
        Self {
            allow_protected_append_writes: Default::default(),
            immutability_period_since_creation_in_days: Default::default(),
            state: Default::default(),
        }
    }
}
///The ImmutabilityPolicy state defines the mode of the policy. Disabled state disables the policy, Unlocked state allows increase and decrease of immutability retention time and also allows toggling allowProtectedAppendWrites property, Locked state only allows the increase of the immutability retention time. A policy can only be created in a Disabled or Unlocked state and can be toggled between the two states. Only a policy in an Unlocked state can transition to a Locked state which cannot be reverted.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The ImmutabilityPolicy state defines the mode of the policy. Disabled state disables the policy, Unlocked state allows increase and decrease of immutability retention time and also allows toggling allowProtectedAppendWrites property, Locked state only allows the increase of the immutability retention time. A policy can only be created in a Disabled or Unlocked state and can be toggled between the two states. Only a policy in an Unlocked state can transition to a Locked state which cannot be reverted.",
///  "type": "string",
///  "enum": [
///    "Unlocked",
///    "Locked",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AccountImmutabilityPolicyState"
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
pub enum AccountImmutabilityPolicyPropertiesState {
    Unlocked,
    Locked,
    Disabled,
}
impl ::std::convert::From<&Self> for AccountImmutabilityPolicyPropertiesState {
    fn from(value: &AccountImmutabilityPolicyPropertiesState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AccountImmutabilityPolicyPropertiesState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Unlocked => f.write_str("Unlocked"),
            Self::Locked => f.write_str("Locked"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for AccountImmutabilityPolicyPropertiesState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "unlocked" => Ok(Self::Unlocked),
            "locked" => Ok(Self::Locked),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AccountImmutabilityPolicyPropertiesState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AccountImmutabilityPolicyPropertiesState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AccountImmutabilityPolicyPropertiesState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters to list SAS credentials of a storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters to list SAS credentials of a storage account.",
///  "required": [
///    "signedExpiry",
///    "signedPermission",
///    "signedResourceTypes",
///    "signedServices"
///  ],
///  "properties": {
///    "keyToSign": {
///      "description": "The key to sign the account SAS token with.",
///      "type": "string"
///    },
///    "signedExpiry": {
///      "description": "The time at which the shared access signature becomes invalid.",
///      "type": "string",
///      "x-ms-client-name": "SharedAccessExpiryTime"
///    },
///    "signedIp": {
///      "description": "An IP address or a range of IP addresses from which to accept requests.",
///      "type": "string",
///      "x-ms-client-name": "IPAddressOrRange"
///    },
///    "signedPermission": {
///      "description": "The signed permissions for the account SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).",
///      "type": "string",
///      "enum": [
///        "r",
///        "d",
///        "w",
///        "l",
///        "a",
///        "c",
///        "u",
///        "p"
///      ],
///      "x-ms-client-name": "Permissions",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Permissions"
///      }
///    },
///    "signedProtocol": {
///      "description": "The protocol permitted for a request made with the account SAS.",
///      "type": "string",
///      "enum": [
///        "https,http",
///        "https"
///      ],
///      "x-ms-client-name": "Protocols",
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "HttpProtocol"
///      }
///    },
///    "signedResourceTypes": {
///      "description": "The signed resource types that are accessible with the account SAS. Service (s): Access to service-level APIs; Container (c): Access to container-level APIs; Object (o): Access to object-level APIs for blobs, queue messages, table entities, and files.",
///      "type": "string",
///      "enum": [
///        "s",
///        "c",
///        "o"
///      ],
///      "x-ms-client-name": "ResourceTypes",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SignedResourceTypes"
///      }
///    },
///    "signedServices": {
///      "description": "The signed services accessible with the account SAS. Possible values include: Blob (b), Queue (q), Table (t), File (f).",
///      "type": "string",
///      "enum": [
///        "b",
///        "q",
///        "t",
///        "f"
///      ],
///      "x-ms-client-name": "Services",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Services"
///      }
///    },
///    "signedStart": {
///      "description": "The time at which the SAS becomes valid.",
///      "type": "string",
///      "x-ms-client-name": "SharedAccessStartTime"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AccountSasParameters {
    ///The key to sign the account SAS token with.
    #[serde(
        rename = "keyToSign",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_to_sign: ::std::option::Option<::std::string::String>,
    ///The time at which the shared access signature becomes invalid.
    #[serde(rename = "signedExpiry")]
    pub signed_expiry: ::std::string::String,
    ///An IP address or a range of IP addresses from which to accept requests.
    #[serde(
        rename = "signedIp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_ip: ::std::option::Option<::std::string::String>,
    ///The signed permissions for the account SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).
    #[serde(rename = "signedPermission")]
    pub signed_permission: AccountSasParametersSignedPermission,
    ///The protocol permitted for a request made with the account SAS.
    #[serde(
        rename = "signedProtocol",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_protocol: ::std::option::Option<AccountSasParametersSignedProtocol>,
    ///The signed resource types that are accessible with the account SAS. Service (s): Access to service-level APIs; Container (c): Access to container-level APIs; Object (o): Access to object-level APIs for blobs, queue messages, table entities, and files.
    #[serde(rename = "signedResourceTypes")]
    pub signed_resource_types: AccountSasParametersSignedResourceTypes,
    ///The signed services accessible with the account SAS. Possible values include: Blob (b), Queue (q), Table (t), File (f).
    #[serde(rename = "signedServices")]
    pub signed_services: AccountSasParametersSignedServices,
    ///The time at which the SAS becomes valid.
    #[serde(
        rename = "signedStart",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_start: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&AccountSasParameters> for AccountSasParameters {
    fn from(value: &AccountSasParameters) -> Self {
        value.clone()
    }
}
///The signed permissions for the account SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The signed permissions for the account SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).",
///  "type": "string",
///  "enum": [
///    "r",
///    "d",
///    "w",
///    "l",
///    "a",
///    "c",
///    "u",
///    "p"
///  ],
///  "x-ms-client-name": "Permissions",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Permissions"
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
pub enum AccountSasParametersSignedPermission {
    #[serde(rename = "r")]
    R,
    #[serde(rename = "d")]
    D,
    #[serde(rename = "w")]
    W,
    #[serde(rename = "l")]
    L,
    #[serde(rename = "a")]
    A,
    #[serde(rename = "c")]
    C,
    #[serde(rename = "u")]
    U,
    #[serde(rename = "p")]
    P,
}
impl ::std::convert::From<&Self> for AccountSasParametersSignedPermission {
    fn from(value: &AccountSasParametersSignedPermission) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AccountSasParametersSignedPermission {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::R => f.write_str("r"),
            Self::D => f.write_str("d"),
            Self::W => f.write_str("w"),
            Self::L => f.write_str("l"),
            Self::A => f.write_str("a"),
            Self::C => f.write_str("c"),
            Self::U => f.write_str("u"),
            Self::P => f.write_str("p"),
        }
    }
}
impl ::std::str::FromStr for AccountSasParametersSignedPermission {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "r" => Ok(Self::R),
            "d" => Ok(Self::D),
            "w" => Ok(Self::W),
            "l" => Ok(Self::L),
            "a" => Ok(Self::A),
            "c" => Ok(Self::C),
            "u" => Ok(Self::U),
            "p" => Ok(Self::P),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AccountSasParametersSignedPermission {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AccountSasParametersSignedPermission {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AccountSasParametersSignedPermission {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The protocol permitted for a request made with the account SAS.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The protocol permitted for a request made with the account SAS.",
///  "type": "string",
///  "enum": [
///    "https,http",
///    "https"
///  ],
///  "x-ms-client-name": "Protocols",
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "HttpProtocol"
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
pub enum AccountSasParametersSignedProtocol {
    #[serde(rename = "https,http")]
    HttpsHttp,
    #[serde(rename = "https")]
    Https,
}
impl ::std::convert::From<&Self> for AccountSasParametersSignedProtocol {
    fn from(value: &AccountSasParametersSignedProtocol) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AccountSasParametersSignedProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::HttpsHttp => f.write_str("https,http"),
            Self::Https => f.write_str("https"),
        }
    }
}
impl ::std::str::FromStr for AccountSasParametersSignedProtocol {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "https,http" => Ok(Self::HttpsHttp),
            "https" => Ok(Self::Https),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AccountSasParametersSignedProtocol {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AccountSasParametersSignedProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AccountSasParametersSignedProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The signed resource types that are accessible with the account SAS. Service (s): Access to service-level APIs; Container (c): Access to container-level APIs; Object (o): Access to object-level APIs for blobs, queue messages, table entities, and files.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The signed resource types that are accessible with the account SAS. Service (s): Access to service-level APIs; Container (c): Access to container-level APIs; Object (o): Access to object-level APIs for blobs, queue messages, table entities, and files.",
///  "type": "string",
///  "enum": [
///    "s",
///    "c",
///    "o"
///  ],
///  "x-ms-client-name": "ResourceTypes",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SignedResourceTypes"
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
pub enum AccountSasParametersSignedResourceTypes {
    #[serde(rename = "s")]
    S,
    #[serde(rename = "c")]
    C,
    #[serde(rename = "o")]
    O,
}
impl ::std::convert::From<&Self> for AccountSasParametersSignedResourceTypes {
    fn from(value: &AccountSasParametersSignedResourceTypes) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AccountSasParametersSignedResourceTypes {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::S => f.write_str("s"),
            Self::C => f.write_str("c"),
            Self::O => f.write_str("o"),
        }
    }
}
impl ::std::str::FromStr for AccountSasParametersSignedResourceTypes {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "s" => Ok(Self::S),
            "c" => Ok(Self::C),
            "o" => Ok(Self::O),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AccountSasParametersSignedResourceTypes {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AccountSasParametersSignedResourceTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AccountSasParametersSignedResourceTypes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The signed services accessible with the account SAS. Possible values include: Blob (b), Queue (q), Table (t), File (f).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The signed services accessible with the account SAS. Possible values include: Blob (b), Queue (q), Table (t), File (f).",
///  "type": "string",
///  "enum": [
///    "b",
///    "q",
///    "t",
///    "f"
///  ],
///  "x-ms-client-name": "Services",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Services"
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
pub enum AccountSasParametersSignedServices {
    #[serde(rename = "b")]
    B,
    #[serde(rename = "q")]
    Q,
    #[serde(rename = "t")]
    T,
    #[serde(rename = "f")]
    F,
}
impl ::std::convert::From<&Self> for AccountSasParametersSignedServices {
    fn from(value: &AccountSasParametersSignedServices) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AccountSasParametersSignedServices {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::B => f.write_str("b"),
            Self::Q => f.write_str("q"),
            Self::T => f.write_str("t"),
            Self::F => f.write_str("f"),
        }
    }
}
impl ::std::str::FromStr for AccountSasParametersSignedServices {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "b" => Ok(Self::B),
            "q" => Ok(Self::Q),
            "t" => Ok(Self::T),
            "f" => Ok(Self::F),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AccountSasParametersSignedServices {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AccountSasParametersSignedServices {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AccountSasParametersSignedServices {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Settings properties for Active Directory (AD).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Settings properties for Active Directory (AD).",
///  "required": [
///    "domainGuid",
///    "domainName"
///  ],
///  "properties": {
///    "accountType": {
///      "description": "Specifies the Active Directory account type for Azure Storage.",
///      "type": "string",
///      "enum": [
///        "User",
///        "Computer"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AccountType"
///      }
///    },
///    "azureStorageSid": {
///      "description": "Specifies the security identifier (SID) for Azure Storage.",
///      "type": "string"
///    },
///    "domainGuid": {
///      "description": "Specifies the domain GUID.",
///      "type": "string"
///    },
///    "domainName": {
///      "description": "Specifies the primary domain that the AD DNS server is authoritative for.",
///      "type": "string"
///    },
///    "domainSid": {
///      "description": "Specifies the security identifier (SID).",
///      "type": "string"
///    },
///    "forestName": {
///      "description": "Specifies the Active Directory forest to get.",
///      "type": "string"
///    },
///    "netBiosDomainName": {
///      "description": "Specifies the NetBIOS domain name.",
///      "type": "string"
///    },
///    "samAccountName": {
///      "description": "Specifies the Active Directory SAMAccountName for Azure Storage.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ActiveDirectoryProperties {
    ///Specifies the Active Directory account type for Azure Storage.
    #[serde(
        rename = "accountType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub account_type: ::std::option::Option<ActiveDirectoryPropertiesAccountType>,
    ///Specifies the security identifier (SID) for Azure Storage.
    #[serde(
        rename = "azureStorageSid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub azure_storage_sid: ::std::option::Option<::std::string::String>,
    ///Specifies the domain GUID.
    #[serde(rename = "domainGuid")]
    pub domain_guid: ::std::string::String,
    ///Specifies the primary domain that the AD DNS server is authoritative for.
    #[serde(rename = "domainName")]
    pub domain_name: ::std::string::String,
    ///Specifies the security identifier (SID).
    #[serde(
        rename = "domainSid",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub domain_sid: ::std::option::Option<::std::string::String>,
    ///Specifies the Active Directory forest to get.
    #[serde(
        rename = "forestName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub forest_name: ::std::option::Option<::std::string::String>,
    ///Specifies the NetBIOS domain name.
    #[serde(
        rename = "netBiosDomainName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub net_bios_domain_name: ::std::option::Option<::std::string::String>,
    ///Specifies the Active Directory SAMAccountName for Azure Storage.
    #[serde(
        rename = "samAccountName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sam_account_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ActiveDirectoryProperties> for ActiveDirectoryProperties {
    fn from(value: &ActiveDirectoryProperties) -> Self {
        value.clone()
    }
}
///Specifies the Active Directory account type for Azure Storage.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies the Active Directory account type for Azure Storage.",
///  "type": "string",
///  "enum": [
///    "User",
///    "Computer"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AccountType"
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
pub enum ActiveDirectoryPropertiesAccountType {
    User,
    Computer,
}
impl ::std::convert::From<&Self> for ActiveDirectoryPropertiesAccountType {
    fn from(value: &ActiveDirectoryPropertiesAccountType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ActiveDirectoryPropertiesAccountType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::User => f.write_str("User"),
            Self::Computer => f.write_str("Computer"),
        }
    }
}
impl ::std::str::FromStr for ActiveDirectoryPropertiesAccountType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "user" => Ok(Self::User),
            "computer" => Ok(Self::Computer),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ActiveDirectoryPropertiesAccountType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ActiveDirectoryPropertiesAccountType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ActiveDirectoryPropertiesAccountType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Settings for Azure Files identity based authentication.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Settings for Azure Files identity based authentication.",
///  "required": [
///    "directoryServiceOptions"
///  ],
///  "properties": {
///    "activeDirectoryProperties": {
///      "$ref": "#/components/schemas/ActiveDirectoryProperties"
///    },
///    "defaultSharePermission": {
///      "description": "Default share permission for users using Kerberos authentication if RBAC role is not assigned.",
///      "type": "string",
///      "enum": [
///        "None",
///        "StorageFileDataSmbShareReader",
///        "StorageFileDataSmbShareContributor",
///        "StorageFileDataSmbShareElevatedContributor"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DefaultSharePermission"
///      }
///    },
///    "directoryServiceOptions": {
///      "description": "Indicates the directory service used. Note that this enum may be extended in the future.",
///      "type": "string",
///      "enum": [
///        "None",
///        "AADDS",
///        "AD",
///        "AADKERB"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DirectoryServiceOptions"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AzureFilesIdentityBasedAuthentication {
    #[serde(
        rename = "activeDirectoryProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub active_directory_properties: ::std::option::Option<ActiveDirectoryProperties>,
    ///Default share permission for users using Kerberos authentication if RBAC role is not assigned.
    #[serde(
        rename = "defaultSharePermission",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_share_permission:
        ::std::option::Option<AzureFilesIdentityBasedAuthenticationDefaultSharePermission>,
    ///Indicates the directory service used. Note that this enum may be extended in the future.
    #[serde(rename = "directoryServiceOptions")]
    pub directory_service_options: AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions,
}
impl ::std::convert::From<&AzureFilesIdentityBasedAuthentication>
    for AzureFilesIdentityBasedAuthentication
{
    fn from(value: &AzureFilesIdentityBasedAuthentication) -> Self {
        value.clone()
    }
}
///Default share permission for users using Kerberos authentication if RBAC role is not assigned.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Default share permission for users using Kerberos authentication if RBAC role is not assigned.",
///  "type": "string",
///  "enum": [
///    "None",
///    "StorageFileDataSmbShareReader",
///    "StorageFileDataSmbShareContributor",
///    "StorageFileDataSmbShareElevatedContributor"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DefaultSharePermission"
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
pub enum AzureFilesIdentityBasedAuthenticationDefaultSharePermission {
    None,
    StorageFileDataSmbShareReader,
    StorageFileDataSmbShareContributor,
    StorageFileDataSmbShareElevatedContributor,
}
impl ::std::convert::From<&Self> for AzureFilesIdentityBasedAuthenticationDefaultSharePermission {
    fn from(value: &AzureFilesIdentityBasedAuthenticationDefaultSharePermission) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AzureFilesIdentityBasedAuthenticationDefaultSharePermission {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::StorageFileDataSmbShareReader => f.write_str("StorageFileDataSmbShareReader"),
            Self::StorageFileDataSmbShareContributor => {
                f.write_str("StorageFileDataSmbShareContributor")
            }
            Self::StorageFileDataSmbShareElevatedContributor => {
                f.write_str("StorageFileDataSmbShareElevatedContributor")
            }
        }
    }
}
impl ::std::str::FromStr for AzureFilesIdentityBasedAuthenticationDefaultSharePermission {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "storagefiledatasmbsharereader" => Ok(Self::StorageFileDataSmbShareReader),
            "storagefiledatasmbsharecontributor" => Ok(Self::StorageFileDataSmbShareContributor),
            "storagefiledatasmbshareelevatedcontributor" => {
                Ok(Self::StorageFileDataSmbShareElevatedContributor)
            }
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AzureFilesIdentityBasedAuthenticationDefaultSharePermission {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for AzureFilesIdentityBasedAuthenticationDefaultSharePermission
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for AzureFilesIdentityBasedAuthenticationDefaultSharePermission
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Indicates the directory service used. Note that this enum may be extended in the future.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates the directory service used. Note that this enum may be extended in the future.",
///  "type": "string",
///  "enum": [
///    "None",
///    "AADDS",
///    "AD",
///    "AADKERB"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DirectoryServiceOptions"
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
pub enum AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions {
    None,
    #[serde(rename = "AADDS")]
    Aadds,
    #[serde(rename = "AD")]
    Ad,
    #[serde(rename = "AADKERB")]
    Aadkerb,
}
impl ::std::convert::From<&Self> for AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions {
    fn from(value: &AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Aadds => f.write_str("AADDS"),
            Self::Ad => f.write_str("AD"),
            Self::Aadkerb => f.write_str("AADKERB"),
        }
    }
}
impl ::std::str::FromStr for AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "aadds" => Ok(Self::Aadds),
            "ad" => Ok(Self::Ad),
            "aadkerb" => Ok(Self::Aadkerb),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
    for AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions
{
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for AzureFilesIdentityBasedAuthenticationDirectoryServiceOptions
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///This property defines the creation time based filtering condition. Blob Inventory schema parameter 'Creation-Time' is mandatory with this filter.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This property defines the creation time based filtering condition. Blob Inventory schema parameter 'Creation-Time' is mandatory with this filter.",
///  "type": "object",
///  "properties": {
///    "lastNDays": {
///      "description": "When set the policy filters the objects that are created in the last N days. Where N is an integer value between 1 to 36500.",
///      "type": "integer",
///      "format": "int32",
///      "maximum": 36500.0,
///      "minimum": 1.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobInventoryCreationTime {
    ///When set the policy filters the objects that are created in the last N days. Where N is an integer value between 1 to 36500.
    #[serde(
        rename = "lastNDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_n_days: ::std::option::Option<::std::num::NonZeroU32>,
}
impl ::std::convert::From<&BlobInventoryCreationTime> for BlobInventoryCreationTime {
    fn from(value: &BlobInventoryCreationTime) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobInventoryCreationTime {
    fn default() -> Self {
        Self {
            last_n_days: Default::default(),
        }
    }
}
///The storage account blob inventory policy.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The storage account blob inventory policy.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/BlobInventoryPolicyProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobInventoryPolicy {
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
    pub properties: ::std::option::Option<BlobInventoryPolicyProperties>,
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
impl ::std::convert::From<&BlobInventoryPolicy> for BlobInventoryPolicy {
    fn from(value: &BlobInventoryPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobInventoryPolicy {
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
///An object that defines the blob inventory rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that defines the blob inventory rule.",
///  "required": [
///    "format",
///    "objectType",
///    "schedule",
///    "schemaFields"
///  ],
///  "properties": {
///    "filters": {
///      "$ref": "#/components/schemas/BlobInventoryPolicyFilter"
///    },
///    "format": {
///      "description": "This is a required field, it specifies the format for the inventory files.",
///      "type": "string",
///      "enum": [
///        "Csv",
///        "Parquet"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "format"
///      }
///    },
///    "objectType": {
///      "description": "This is a required field. This field specifies the scope of the inventory created either at the blob or container level.",
///      "type": "string",
///      "enum": [
///        "Blob",
///        "Container"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "objectType"
///      }
///    },
///    "schedule": {
///      "description": "This is a required field. This field is used to schedule an inventory formation.",
///      "type": "string",
///      "enum": [
///        "Daily",
///        "Weekly"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "schedule"
///      }
///    },
///    "schemaFields": {
///      "description": "This is a required field. This field specifies the fields and properties of the object to be included in the inventory. The Schema field value 'Name' is always required. The valid values for this field for the 'Blob' definition.objectType include 'Name, Creation-Time, Last-Modified, Content-Length, Content-MD5, BlobType, AccessTier, AccessTierChangeTime, AccessTierInferred, Tags, Expiry-Time, hdi_isfolder, Owner, Group, Permissions, Acl, Snapshot, VersionId, IsCurrentVersion, Metadata, LastAccessTime, Tags, Etag, ContentType, ContentEncoding, ContentLanguage, ContentCRC64, CacheControl, ContentDisposition, LeaseStatus, LeaseState, LeaseDuration, ServerEncrypted, Deleted, DeletionId, DeletedTime, RemainingRetentionDays, ImmutabilityPolicyUntilDate, ImmutabilityPolicyMode, LegalHold, CopyId, CopyStatus, CopySource, CopyProgress, CopyCompletionTime, CopyStatusDescription, CustomerProvidedKeySha256, RehydratePriority, ArchiveStatus, XmsBlobSequenceNumber, EncryptionScope, IncrementalCopy, TagCount'. For Blob object type schema field value 'DeletedTime' is applicable only for Hns enabled accounts. The valid values for 'Container' definition.objectType include 'Name, Last-Modified, Metadata, LeaseStatus, LeaseState, LeaseDuration, PublicAccess, HasImmutabilityPolicy, HasLegalHold, Etag, DefaultEncryptionScope, DenyEncryptionScopeOverride, ImmutableStorageWithVersioningEnabled, Deleted, Version, DeletedTime, RemainingRetentionDays'. Schema field values 'Expiry-Time, hdi_isfolder, Owner, Group, Permissions, Acl, DeletionId' are valid only for Hns enabled accounts.Schema field values 'Tags, TagCount' are only valid for Non-Hns accounts.",
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
pub struct BlobInventoryPolicyDefinition {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub filters: ::std::option::Option<BlobInventoryPolicyFilter>,
    ///This is a required field, it specifies the format for the inventory files.
    pub format: BlobInventoryPolicyDefinitionFormat,
    ///This is a required field. This field specifies the scope of the inventory created either at the blob or container level.
    #[serde(rename = "objectType")]
    pub object_type: BlobInventoryPolicyDefinitionObjectType,
    ///This is a required field. This field is used to schedule an inventory formation.
    pub schedule: BlobInventoryPolicyDefinitionSchedule,
    ///This is a required field. This field specifies the fields and properties of the object to be included in the inventory. The Schema field value 'Name' is always required. The valid values for this field for the 'Blob' definition.objectType include 'Name, Creation-Time, Last-Modified, Content-Length, Content-MD5, BlobType, AccessTier, AccessTierChangeTime, AccessTierInferred, Tags, Expiry-Time, hdi_isfolder, Owner, Group, Permissions, Acl, Snapshot, VersionId, IsCurrentVersion, Metadata, LastAccessTime, Tags, Etag, ContentType, ContentEncoding, ContentLanguage, ContentCRC64, CacheControl, ContentDisposition, LeaseStatus, LeaseState, LeaseDuration, ServerEncrypted, Deleted, DeletionId, DeletedTime, RemainingRetentionDays, ImmutabilityPolicyUntilDate, ImmutabilityPolicyMode, LegalHold, CopyId, CopyStatus, CopySource, CopyProgress, CopyCompletionTime, CopyStatusDescription, CustomerProvidedKeySha256, RehydratePriority, ArchiveStatus, XmsBlobSequenceNumber, EncryptionScope, IncrementalCopy, TagCount'. For Blob object type schema field value 'DeletedTime' is applicable only for Hns enabled accounts. The valid values for 'Container' definition.objectType include 'Name, Last-Modified, Metadata, LeaseStatus, LeaseState, LeaseDuration, PublicAccess, HasImmutabilityPolicy, HasLegalHold, Etag, DefaultEncryptionScope, DenyEncryptionScopeOverride, ImmutableStorageWithVersioningEnabled, Deleted, Version, DeletedTime, RemainingRetentionDays'. Schema field values 'Expiry-Time, hdi_isfolder, Owner, Group, Permissions, Acl, DeletionId' are valid only for Hns enabled accounts.Schema field values 'Tags, TagCount' are only valid for Non-Hns accounts.
    #[serde(rename = "schemaFields")]
    pub schema_fields: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&BlobInventoryPolicyDefinition> for BlobInventoryPolicyDefinition {
    fn from(value: &BlobInventoryPolicyDefinition) -> Self {
        value.clone()
    }
}
///This is a required field, it specifies the format for the inventory files.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This is a required field, it specifies the format for the inventory files.",
///  "type": "string",
///  "enum": [
///    "Csv",
///    "Parquet"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "format"
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
pub enum BlobInventoryPolicyDefinitionFormat {
    Csv,
    Parquet,
}
impl ::std::convert::From<&Self> for BlobInventoryPolicyDefinitionFormat {
    fn from(value: &BlobInventoryPolicyDefinitionFormat) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for BlobInventoryPolicyDefinitionFormat {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Csv => f.write_str("Csv"),
            Self::Parquet => f.write_str("Parquet"),
        }
    }
}
impl ::std::str::FromStr for BlobInventoryPolicyDefinitionFormat {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "parquet" => Ok(Self::Parquet),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BlobInventoryPolicyDefinitionFormat {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BlobInventoryPolicyDefinitionFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BlobInventoryPolicyDefinitionFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///This is a required field. This field specifies the scope of the inventory created either at the blob or container level.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This is a required field. This field specifies the scope of the inventory created either at the blob or container level.",
///  "type": "string",
///  "enum": [
///    "Blob",
///    "Container"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "objectType"
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
pub enum BlobInventoryPolicyDefinitionObjectType {
    Blob,
    Container,
}
impl ::std::convert::From<&Self> for BlobInventoryPolicyDefinitionObjectType {
    fn from(value: &BlobInventoryPolicyDefinitionObjectType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for BlobInventoryPolicyDefinitionObjectType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Blob => f.write_str("Blob"),
            Self::Container => f.write_str("Container"),
        }
    }
}
impl ::std::str::FromStr for BlobInventoryPolicyDefinitionObjectType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "blob" => Ok(Self::Blob),
            "container" => Ok(Self::Container),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BlobInventoryPolicyDefinitionObjectType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BlobInventoryPolicyDefinitionObjectType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BlobInventoryPolicyDefinitionObjectType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///This is a required field. This field is used to schedule an inventory formation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This is a required field. This field is used to schedule an inventory formation.",
///  "type": "string",
///  "enum": [
///    "Daily",
///    "Weekly"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "schedule"
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
pub enum BlobInventoryPolicyDefinitionSchedule {
    Daily,
    Weekly,
}
impl ::std::convert::From<&Self> for BlobInventoryPolicyDefinitionSchedule {
    fn from(value: &BlobInventoryPolicyDefinitionSchedule) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for BlobInventoryPolicyDefinitionSchedule {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Daily => f.write_str("Daily"),
            Self::Weekly => f.write_str("Weekly"),
        }
    }
}
impl ::std::str::FromStr for BlobInventoryPolicyDefinitionSchedule {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BlobInventoryPolicyDefinitionSchedule {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BlobInventoryPolicyDefinitionSchedule {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BlobInventoryPolicyDefinitionSchedule {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An object that defines the blob inventory rule filter conditions. For 'Blob' definition.objectType all filter properties are applicable, 'blobTypes' is required and others are optional. For 'Container' definition.objectType only prefixMatch is applicable and is optional.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that defines the blob inventory rule filter conditions. For 'Blob' definition.objectType all filter properties are applicable, 'blobTypes' is required and others are optional. For 'Container' definition.objectType only prefixMatch is applicable and is optional.",
///  "properties": {
///    "blobTypes": {
///      "description": "An array of predefined enum values. Valid values include blockBlob, appendBlob, pageBlob. Hns accounts does not support pageBlobs. This field is required when definition.objectType property is set to 'Blob'.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "creationTime": {
///      "$ref": "#/components/schemas/BlobInventoryCreationTime"
///    },
///    "excludePrefix": {
///      "description": "An array of strings with maximum 10 blob prefixes to be excluded from the inventory.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "includeBlobVersions": {
///      "description": "Includes blob versions in blob inventory when value is set to true. The definition.schemaFields values 'VersionId and IsCurrentVersion' are required if this property is set to true, else they must be excluded.",
///      "type": "boolean"
///    },
///    "includeDeleted": {
///      "description": "For 'Container' definition.objectType the definition.schemaFields must include 'Deleted, Version, DeletedTime and RemainingRetentionDays'. For 'Blob' definition.objectType and HNS enabled storage accounts the definition.schemaFields must include 'DeletionId, Deleted, DeletedTime and RemainingRetentionDays' and for Hns disabled accounts the definition.schemaFields must include 'Deleted and RemainingRetentionDays', else it must be excluded.",
///      "type": "boolean"
///    },
///    "includeSnapshots": {
///      "description": "Includes blob snapshots in blob inventory when value is set to true. The definition.schemaFields value 'Snapshot' is required if this property is set to true, else it must be excluded.",
///      "type": "boolean"
///    },
///    "prefixMatch": {
///      "description": "An array of strings with maximum 10 blob prefixes to be included in the inventory.",
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
pub struct BlobInventoryPolicyFilter {
    ///An array of predefined enum values. Valid values include blockBlob, appendBlob, pageBlob. Hns accounts does not support pageBlobs. This field is required when definition.objectType property is set to 'Blob'.
    #[serde(
        rename = "blobTypes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob_types: ::std::vec::Vec<::std::string::String>,
    #[serde(
        rename = "creationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_time: ::std::option::Option<BlobInventoryCreationTime>,
    ///An array of strings with maximum 10 blob prefixes to be excluded from the inventory.
    #[serde(
        rename = "excludePrefix",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exclude_prefix: ::std::vec::Vec<::std::string::String>,
    ///Includes blob versions in blob inventory when value is set to true. The definition.schemaFields values 'VersionId and IsCurrentVersion' are required if this property is set to true, else they must be excluded.
    #[serde(
        rename = "includeBlobVersions",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub include_blob_versions: ::std::option::Option<bool>,
    ///For 'Container' definition.objectType the definition.schemaFields must include 'Deleted, Version, DeletedTime and RemainingRetentionDays'. For 'Blob' definition.objectType and HNS enabled storage accounts the definition.schemaFields must include 'DeletionId, Deleted, DeletedTime and RemainingRetentionDays' and for Hns disabled accounts the definition.schemaFields must include 'Deleted and RemainingRetentionDays', else it must be excluded.
    #[serde(
        rename = "includeDeleted",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub include_deleted: ::std::option::Option<bool>,
    ///Includes blob snapshots in blob inventory when value is set to true. The definition.schemaFields value 'Snapshot' is required if this property is set to true, else it must be excluded.
    #[serde(
        rename = "includeSnapshots",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub include_snapshots: ::std::option::Option<bool>,
    ///An array of strings with maximum 10 blob prefixes to be included in the inventory.
    #[serde(
        rename = "prefixMatch",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub prefix_match: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&BlobInventoryPolicyFilter> for BlobInventoryPolicyFilter {
    fn from(value: &BlobInventoryPolicyFilter) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobInventoryPolicyFilter {
    fn default() -> Self {
        Self {
            blob_types: Default::default(),
            creation_time: Default::default(),
            exclude_prefix: Default::default(),
            include_blob_versions: Default::default(),
            include_deleted: Default::default(),
            include_snapshots: Default::default(),
            prefix_match: Default::default(),
        }
    }
}
///The storage account blob inventory policy properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The storage account blob inventory policy properties.",
///  "required": [
///    "policy"
///  ],
///  "properties": {
///    "lastModifiedTime": {
///      "description": "Returns the last modified date and time of the blob inventory policy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/BlobInventoryPolicySchema"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobInventoryPolicyProperties {
    ///Returns the last modified date and time of the blob inventory policy.
    #[serde(
        rename = "lastModifiedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_time: ::std::option::Option<::std::string::String>,
    pub policy: BlobInventoryPolicySchema,
}
impl ::std::convert::From<&BlobInventoryPolicyProperties> for BlobInventoryPolicyProperties {
    fn from(value: &BlobInventoryPolicyProperties) -> Self {
        value.clone()
    }
}
///An object that wraps the blob inventory rule. Each rule is uniquely defined by name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that wraps the blob inventory rule. Each rule is uniquely defined by name.",
///  "required": [
///    "definition",
///    "destination",
///    "enabled",
///    "name"
///  ],
///  "properties": {
///    "definition": {
///      "$ref": "#/components/schemas/BlobInventoryPolicyDefinition"
///    },
///    "destination": {
///      "description": "Container name where blob inventory files are stored. Must be pre-created.",
///      "type": "string"
///    },
///    "enabled": {
///      "description": "Rule is enabled when set to true.",
///      "type": "boolean"
///    },
///    "name": {
///      "description": "A rule name can contain any combination of alpha numeric characters. Rule name is case-sensitive. It must be unique within a policy.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobInventoryPolicyRule {
    pub definition: BlobInventoryPolicyDefinition,
    ///Container name where blob inventory files are stored. Must be pre-created.
    pub destination: ::std::string::String,
    ///Rule is enabled when set to true.
    pub enabled: bool,
    ///A rule name can contain any combination of alpha numeric characters. Rule name is case-sensitive. It must be unique within a policy.
    pub name: ::std::string::String,
}
impl ::std::convert::From<&BlobInventoryPolicyRule> for BlobInventoryPolicyRule {
    fn from(value: &BlobInventoryPolicyRule) -> Self {
        value.clone()
    }
}
///The storage account blob inventory policy rules.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The storage account blob inventory policy rules.",
///  "required": [
///    "enabled",
///    "rules",
///    "type"
///  ],
///  "properties": {
///    "destination": {
///      "description": "Deprecated Property from API version 2021-04-01 onwards, the required destination container name must be specified at the rule level 'policy.rule.destination'",
///      "readOnly": true,
///      "type": "string"
///    },
///    "enabled": {
///      "description": "Policy is enabled if set to true.",
///      "type": "boolean"
///    },
///    "rules": {
///      "description": "The storage account blob inventory policy rules. The rule is applied when it is enabled.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/BlobInventoryPolicyRule"
///      }
///    },
///    "type": {
///      "description": "The valid value is Inventory",
///      "type": "string",
///      "enum": [
///        "Inventory"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "InventoryRuleType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobInventoryPolicySchema {
    ///Deprecated Property from API version 2021-04-01 onwards, the required destination container name must be specified at the rule level 'policy.rule.destination'
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination: ::std::option::Option<::std::string::String>,
    ///Policy is enabled if set to true.
    pub enabled: bool,
    ///The storage account blob inventory policy rules. The rule is applied when it is enabled.
    pub rules: ::std::vec::Vec<BlobInventoryPolicyRule>,
    ///The valid value is Inventory
    #[serde(rename = "type")]
    pub type_: BlobInventoryPolicySchemaType,
}
impl ::std::convert::From<&BlobInventoryPolicySchema> for BlobInventoryPolicySchema {
    fn from(value: &BlobInventoryPolicySchema) -> Self {
        value.clone()
    }
}
///The valid value is Inventory
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The valid value is Inventory",
///  "type": "string",
///  "enum": [
///    "Inventory"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "InventoryRuleType"
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
pub enum BlobInventoryPolicySchemaType {
    Inventory,
}
impl ::std::convert::From<&Self> for BlobInventoryPolicySchemaType {
    fn from(value: &BlobInventoryPolicySchemaType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for BlobInventoryPolicySchemaType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Inventory => f.write_str("Inventory"),
        }
    }
}
impl ::std::str::FromStr for BlobInventoryPolicySchemaType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "inventory" => Ok(Self::Inventory),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BlobInventoryPolicySchemaType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BlobInventoryPolicySchemaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BlobInventoryPolicySchemaType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Blob restore parameters
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Blob restore parameters",
///  "required": [
///    "blobRanges",
///    "timeToRestore"
///  ],
///  "properties": {
///    "blobRanges": {
///      "description": "Blob ranges to restore.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/BlobRestoreRange"
///      }
///    },
///    "timeToRestore": {
///      "description": "Restore blob to the specified time.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobRestoreParameters {
    ///Blob ranges to restore.
    #[serde(rename = "blobRanges")]
    pub blob_ranges: ::std::vec::Vec<BlobRestoreRange>,
    ///Restore blob to the specified time.
    #[serde(rename = "timeToRestore")]
    pub time_to_restore: ::std::string::String,
}
impl ::std::convert::From<&BlobRestoreParameters> for BlobRestoreParameters {
    fn from(value: &BlobRestoreParameters) -> Self {
        value.clone()
    }
}
///Blob range
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Blob range",
///  "required": [
///    "endRange",
///    "startRange"
///  ],
///  "properties": {
///    "endRange": {
///      "description": "Blob end range. This is exclusive. Empty means account end.",
///      "type": "string"
///    },
///    "startRange": {
///      "description": "Blob start range. This is inclusive. Empty means account start.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobRestoreRange {
    ///Blob end range. This is exclusive. Empty means account end.
    #[serde(rename = "endRange")]
    pub end_range: ::std::string::String,
    ///Blob start range. This is inclusive. Empty means account start.
    #[serde(rename = "startRange")]
    pub start_range: ::std::string::String,
}
impl ::std::convert::From<&BlobRestoreRange> for BlobRestoreRange {
    fn from(value: &BlobRestoreRange) -> Self {
        value.clone()
    }
}
///Blob restore status.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Blob restore status.",
///  "properties": {
///    "failureReason": {
///      "description": "Failure reason when blob restore is failed.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "parameters": {
///      "$ref": "#/components/schemas/BlobRestoreParameters"
///    },
///    "restoreId": {
///      "description": "Id for tracking blob restore request.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "status": {
///      "description": "The status of blob restore progress. Possible values are: - InProgress: Indicates that blob restore is ongoing. - Complete: Indicates that blob restore has been completed successfully. - Failed: Indicates that blob restore is failed.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "InProgress",
///        "Complete",
///        "Failed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "BlobRestoreProgressStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BlobRestoreStatus {
    ///Failure reason when blob restore is failed.
    #[serde(
        rename = "failureReason",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub failure_reason: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub parameters: ::std::option::Option<BlobRestoreParameters>,
    ///Id for tracking blob restore request.
    #[serde(
        rename = "restoreId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub restore_id: ::std::option::Option<::std::string::String>,
    ///The status of blob restore progress. Possible values are: - InProgress: Indicates that blob restore is ongoing. - Complete: Indicates that blob restore has been completed successfully. - Failed: Indicates that blob restore is failed.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<BlobRestoreStatusStatus>,
}
impl ::std::convert::From<&BlobRestoreStatus> for BlobRestoreStatus {
    fn from(value: &BlobRestoreStatus) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BlobRestoreStatus {
    fn default() -> Self {
        Self {
            failure_reason: Default::default(),
            parameters: Default::default(),
            restore_id: Default::default(),
            status: Default::default(),
        }
    }
}
///The status of blob restore progress. Possible values are: - InProgress: Indicates that blob restore is ongoing. - Complete: Indicates that blob restore has been completed successfully. - Failed: Indicates that blob restore is failed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of blob restore progress. Possible values are: - InProgress: Indicates that blob restore is ongoing. - Complete: Indicates that blob restore has been completed successfully. - Failed: Indicates that blob restore is failed.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "InProgress",
///    "Complete",
///    "Failed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "BlobRestoreProgressStatus"
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
pub enum BlobRestoreStatusStatus {
    InProgress,
    Complete,
    Failed,
}
impl ::std::convert::From<&Self> for BlobRestoreStatusStatus {
    fn from(value: &BlobRestoreStatusStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for BlobRestoreStatusStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::InProgress => f.write_str("InProgress"),
            Self::Complete => f.write_str("Complete"),
            Self::Failed => f.write_str("Failed"),
        }
    }
}
impl ::std::str::FromStr for BlobRestoreStatusStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "inprogress" => Ok(Self::InProgress),
            "complete" => Ok(Self::Complete),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BlobRestoreStatusStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BlobRestoreStatusStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BlobRestoreStatusStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The CheckNameAvailability operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The CheckNameAvailability operation response.",
///  "properties": {
///    "message": {
///      "description": "Gets an error message explaining the Reason value in more detail.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "nameAvailable": {
///      "description": "Gets a boolean value that indicates whether the name is available for you to use. If true, the name is available. If false, the name has already been taken or is invalid and cannot be used.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "reason": {
///      "description": "Gets the reason that a storage account name could not be used. The Reason element is only returned if NameAvailable is false.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "AccountNameInvalid",
///        "AlreadyExists"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "Reason"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CheckNameAvailabilityResult {
    ///Gets an error message explaining the Reason value in more detail.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///Gets a boolean value that indicates whether the name is available for you to use. If true, the name is available. If false, the name has already been taken or is invalid and cannot be used.
    #[serde(
        rename = "nameAvailable",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name_available: ::std::option::Option<bool>,
    ///Gets the reason that a storage account name could not be used. The Reason element is only returned if NameAvailable is false.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reason: ::std::option::Option<CheckNameAvailabilityResultReason>,
}
impl ::std::convert::From<&CheckNameAvailabilityResult> for CheckNameAvailabilityResult {
    fn from(value: &CheckNameAvailabilityResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CheckNameAvailabilityResult {
    fn default() -> Self {
        Self {
            message: Default::default(),
            name_available: Default::default(),
            reason: Default::default(),
        }
    }
}
///Gets the reason that a storage account name could not be used. The Reason element is only returned if NameAvailable is false.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the reason that a storage account name could not be used. The Reason element is only returned if NameAvailable is false.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "AccountNameInvalid",
///    "AlreadyExists"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "Reason"
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
pub enum CheckNameAvailabilityResultReason {
    AccountNameInvalid,
    AlreadyExists,
}
impl ::std::convert::From<&Self> for CheckNameAvailabilityResultReason {
    fn from(value: &CheckNameAvailabilityResultReason) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CheckNameAvailabilityResultReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AccountNameInvalid => f.write_str("AccountNameInvalid"),
            Self::AlreadyExists => f.write_str("AlreadyExists"),
        }
    }
}
impl ::std::str::FromStr for CheckNameAvailabilityResultReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "accountnameinvalid" => Ok(Self::AccountNameInvalid),
            "alreadyexists" => Ok(Self::AlreadyExists),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CheckNameAvailabilityResultReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CheckNameAvailabilityResultReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CheckNameAvailabilityResultReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
///The custom domain assigned to this storage account. This can be set via Update.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The custom domain assigned to this storage account. This can be set via Update.",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "name": {
///      "description": "Gets or sets the custom domain name assigned to the storage account. Name is the CNAME source.",
///      "type": "string"
///    },
///    "useSubDomainName": {
///      "description": "Indicates whether indirect CName validation is enabled. Default value is false. This should only be set on updates.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CustomDomain {
    ///Gets or sets the custom domain name assigned to the storage account. Name is the CNAME source.
    pub name: ::std::string::String,
    ///Indicates whether indirect CName validation is enabled. Default value is false. This should only be set on updates.
    #[serde(
        rename = "useSubDomainName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub use_sub_domain_name: ::std::option::Option<bool>,
}
impl ::std::convert::From<&CustomDomain> for CustomDomain {
    fn from(value: &CustomDomain) -> Self {
        value.clone()
    }
}
///Object to define snapshot and version action conditions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Object to define snapshot and version action conditions.",
///  "required": [
///    "daysAfterCreationGreaterThan"
///  ],
///  "properties": {
///    "daysAfterCreationGreaterThan": {
///      "description": "Value indicating the age in days after creation",
///      "type": "number",
///      "multipleOf": 1.0,
///      "minimum": 0.0
///    },
///    "daysAfterLastTierChangeGreaterThan": {
///      "description": "Value indicating the age in days after last blob tier change time. This property is only applicable for tierToArchive actions and requires daysAfterCreationGreaterThan to be set for snapshots and blob version based actions. The blob will be archived if both the conditions are satisfied.",
///      "type": "number",
///      "multipleOf": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DateAfterCreation {
    #[serde(rename = "daysAfterCreationGreaterThan")]
    pub days_after_creation_greater_than: f64,
    #[serde(
        rename = "daysAfterLastTierChangeGreaterThan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days_after_last_tier_change_greater_than: ::std::option::Option<f64>,
}
impl ::std::convert::From<&DateAfterCreation> for DateAfterCreation {
    fn from(value: &DateAfterCreation) -> Self {
        value.clone()
    }
}
///Object to define the base blob action conditions. Properties daysAfterModificationGreaterThan, daysAfterLastAccessTimeGreaterThan and daysAfterCreationGreaterThan are mutually exclusive. The daysAfterLastTierChangeGreaterThan property is only applicable for tierToArchive actions which requires daysAfterModificationGreaterThan to be set, also it cannot be used in conjunction with daysAfterLastAccessTimeGreaterThan or daysAfterCreationGreaterThan.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Object to define the base blob action conditions. Properties daysAfterModificationGreaterThan, daysAfterLastAccessTimeGreaterThan and daysAfterCreationGreaterThan are mutually exclusive. The daysAfterLastTierChangeGreaterThan property is only applicable for tierToArchive actions which requires daysAfterModificationGreaterThan to be set, also it cannot be used in conjunction with daysAfterLastAccessTimeGreaterThan or daysAfterCreationGreaterThan.",
///  "properties": {
///    "daysAfterCreationGreaterThan": {
///      "description": "Value indicating the age in days after blob creation.",
///      "type": "number",
///      "multipleOf": 1.0,
///      "minimum": 0.0
///    },
///    "daysAfterLastAccessTimeGreaterThan": {
///      "description": "Value indicating the age in days after last blob access. This property can only be used in conjunction with last access time tracking policy",
///      "type": "number",
///      "multipleOf": 1.0,
///      "minimum": 0.0
///    },
///    "daysAfterLastTierChangeGreaterThan": {
///      "description": "Value indicating the age in days after last blob tier change time. This property is only applicable for tierToArchive actions and requires daysAfterModificationGreaterThan to be set for baseBlobs based actions. The blob will be archived if both the conditions are satisfied.",
///      "type": "number",
///      "multipleOf": 1.0,
///      "minimum": 0.0
///    },
///    "daysAfterModificationGreaterThan": {
///      "description": "Value indicating the age in days after last modification",
///      "type": "number",
///      "multipleOf": 1.0,
///      "minimum": 0.0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DateAfterModification {
    #[serde(
        rename = "daysAfterCreationGreaterThan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days_after_creation_greater_than: ::std::option::Option<f64>,
    #[serde(
        rename = "daysAfterLastAccessTimeGreaterThan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days_after_last_access_time_greater_than: ::std::option::Option<f64>,
    #[serde(
        rename = "daysAfterLastTierChangeGreaterThan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days_after_last_tier_change_greater_than: ::std::option::Option<f64>,
    #[serde(
        rename = "daysAfterModificationGreaterThan",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days_after_modification_greater_than: ::std::option::Option<f64>,
}
impl ::std::convert::From<&DateAfterModification> for DateAfterModification {
    fn from(value: &DateAfterModification) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DateAfterModification {
    fn default() -> Self {
        Self {
            days_after_creation_greater_than: Default::default(),
            days_after_last_access_time_greater_than: Default::default(),
            days_after_last_tier_change_greater_than: Default::default(),
            days_after_modification_greater_than: Default::default(),
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
pub struct DefinitionsErrorResponse {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<ErrorDetail>,
}
impl ::std::convert::From<&DefinitionsErrorResponse> for DefinitionsErrorResponse {
    fn from(value: &DefinitionsErrorResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DefinitionsErrorResponse {
    fn default() -> Self {
        Self {
            error: Default::default(),
        }
    }
}
///Deleted storage account
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Deleted storage account",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/DeletedAccountProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedAccount {
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
    pub properties: ::std::option::Option<DeletedAccountProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedAccount> for DeletedAccount {
    fn from(value: &DeletedAccount) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedAccount {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///The response from the List Deleted Accounts operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the List Deleted Accounts operation.",
///  "properties": {
///    "nextLink": {
///      "description": "Request URL that can be used to query next page of deleted accounts. Returned when total number of requested deleted accounts exceed maximum page size.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "Gets the list of deleted accounts and their properties.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DeletedAccount"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedAccountListResult {
    ///Request URL that can be used to query next page of deleted accounts. Returned when total number of requested deleted accounts exceed maximum page size.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Gets the list of deleted accounts and their properties.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<DeletedAccount>,
}
impl ::std::convert::From<&DeletedAccountListResult> for DeletedAccountListResult {
    fn from(value: &DeletedAccountListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedAccountListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Attributes of a deleted storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Attributes of a deleted storage account.",
///  "properties": {
///    "creationTime": {
///      "description": "Creation time of the deleted account.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "deletionTime": {
///      "description": "Deletion time of the deleted account.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "Location of the deleted account.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "restoreReference": {
///      "description": "Can be used to attempt recovering this deleted account via PutStorageAccount API.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "storageAccountResourceId": {
///      "description": "Full resource id of the original storage account.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedAccountProperties {
    ///Creation time of the deleted account.
    #[serde(
        rename = "creationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_time: ::std::option::Option<::std::string::String>,
    ///Deletion time of the deleted account.
    #[serde(
        rename = "deletionTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deletion_time: ::std::option::Option<::std::string::String>,
    ///Location of the deleted account.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Can be used to attempt recovering this deleted account via PutStorageAccount API.
    #[serde(
        rename = "restoreReference",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub restore_reference: ::std::option::Option<::std::string::String>,
    ///Full resource id of the original storage account.
    #[serde(
        rename = "storageAccountResourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub storage_account_resource_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedAccountProperties> for DeletedAccountProperties {
    fn from(value: &DeletedAccountProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedAccountProperties {
    fn default() -> Self {
        Self {
            creation_time: Default::default(),
            deletion_time: Default::default(),
            location: Default::default(),
            restore_reference: Default::default(),
            storage_account_resource_id: Default::default(),
        }
    }
}
///Dimension of blobs, possibly be blob type or access tier.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Dimension of blobs, possibly be blob type or access tier.",
///  "properties": {
///    "displayName": {
///      "description": "Display name of dimension.",
///      "type": "string"
///    },
///    "name": {
///      "description": "Display name of dimension.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Dimension {
    ///Display name of dimension.
    #[serde(
        rename = "displayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_name: ::std::option::Option<::std::string::String>,
    ///Display name of dimension.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Dimension> for Dimension {
    fn from(value: &Dimension) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Dimension {
    fn default() -> Self {
        Self {
            display_name: Default::default(),
            name: Default::default(),
        }
    }
}
///The encryption settings on the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The encryption settings on the storage account.",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/EncryptionIdentity"
///    },
///    "keySource": {
///      "description": "The encryption keySource (provider). Possible values (case-insensitive):  Microsoft.Storage, Microsoft.Keyvault",
///      "default": "Microsoft.Storage",
///      "type": "string",
///      "enum": [
///        "Microsoft.Storage",
///        "Microsoft.Keyvault"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "KeySource"
///      }
///    },
///    "keyvaultproperties": {
///      "$ref": "#/components/schemas/KeyVaultProperties"
///    },
///    "requireInfrastructureEncryption": {
///      "description": "A boolean indicating whether or not the service applies a secondary layer of encryption with platform managed keys for data at rest.",
///      "type": "boolean",
///      "x-ms-client-name": "RequireInfrastructureEncryption"
///    },
///    "services": {
///      "$ref": "#/components/schemas/EncryptionServices"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Encryption {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<EncryptionIdentity>,
    ///The encryption keySource (provider). Possible values (case-insensitive):  Microsoft.Storage, Microsoft.Keyvault
    #[serde(
        rename = "keySource",
        default = "defaults::encryption_key_source",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_source: EncryptionKeySource,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keyvaultproperties: ::std::option::Option<KeyVaultProperties>,
    ///A boolean indicating whether or not the service applies a secondary layer of encryption with platform managed keys for data at rest.
    #[serde(
        rename = "requireInfrastructureEncryption",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub require_infrastructure_encryption: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub services: ::std::option::Option<EncryptionServices>,
}
impl ::std::convert::From<&Encryption> for Encryption {
    fn from(value: &Encryption) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Encryption {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            key_source: defaults::encryption_key_source(),
            keyvaultproperties: Default::default(),
            require_infrastructure_encryption: Default::default(),
            services: Default::default(),
        }
    }
}
///Encryption identity for the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Encryption identity for the storage account.",
///  "properties": {
///    "federatedIdentityClientId": {
///      "description": "ClientId of the multi-tenant application to be used in conjunction with the user-assigned identity for cross-tenant customer-managed-keys server-side encryption on the storage account.",
///      "type": "string",
///      "x-ms-client-name": "EncryptionFederatedIdentityClientId"
///    },
///    "userAssignedIdentity": {
///      "description": "Resource identifier of the UserAssigned identity to be associated with server-side encryption on the storage account.",
///      "type": "string",
///      "x-ms-client-name": "EncryptionUserAssignedIdentity"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionIdentity {
    ///ClientId of the multi-tenant application to be used in conjunction with the user-assigned identity for cross-tenant customer-managed-keys server-side encryption on the storage account.
    #[serde(
        rename = "federatedIdentityClientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub federated_identity_client_id: ::std::option::Option<::std::string::String>,
    ///Resource identifier of the UserAssigned identity to be associated with server-side encryption on the storage account.
    #[serde(
        rename = "userAssignedIdentity",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identity: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EncryptionIdentity> for EncryptionIdentity {
    fn from(value: &EncryptionIdentity) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionIdentity {
    fn default() -> Self {
        Self {
            federated_identity_client_id: Default::default(),
            user_assigned_identity: Default::default(),
        }
    }
}
///The encryption keySource (provider). Possible values (case-insensitive):  Microsoft.Storage, Microsoft.Keyvault
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The encryption keySource (provider). Possible values (case-insensitive):  Microsoft.Storage, Microsoft.Keyvault",
///  "default": "Microsoft.Storage",
///  "type": "string",
///  "enum": [
///    "Microsoft.Storage",
///    "Microsoft.Keyvault"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "KeySource"
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
pub enum EncryptionKeySource {
    #[serde(rename = "Microsoft.Storage")]
    MicrosoftStorage,
    #[serde(rename = "Microsoft.Keyvault")]
    MicrosoftKeyvault,
}
impl ::std::convert::From<&Self> for EncryptionKeySource {
    fn from(value: &EncryptionKeySource) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionKeySource {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftStorage => f.write_str("Microsoft.Storage"),
            Self::MicrosoftKeyvault => f.write_str("Microsoft.Keyvault"),
        }
    }
}
impl ::std::str::FromStr for EncryptionKeySource {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoft.storage" => Ok(Self::MicrosoftStorage),
            "microsoft.keyvault" => Ok(Self::MicrosoftKeyvault),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionKeySource {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionKeySource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionKeySource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for EncryptionKeySource {
    fn default() -> Self {
        EncryptionKeySource::MicrosoftStorage
    }
}
///The Encryption Scope resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Encryption Scope resource.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/EncryptionScopeProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionScope {
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
    pub properties: ::std::option::Option<EncryptionScopeProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EncryptionScope> for EncryptionScope {
    fn from(value: &EncryptionScope) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionScope {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///The key vault properties for the encryption scope. This is a required field if encryption scope 'source' attribute is set to 'Microsoft.KeyVault'.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The key vault properties for the encryption scope. This is a required field if encryption scope 'source' attribute is set to 'Microsoft.KeyVault'.",
///  "properties": {
///    "currentVersionedKeyIdentifier": {
///      "description": "The object identifier of the current versioned Key Vault Key in use.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "keyUri": {
///      "description": "The object identifier for a key vault key object. When applied, the encryption scope will use the key referenced by the identifier to enable customer-managed key support on this encryption scope.",
///      "type": "string"
///    },
///    "lastKeyRotationTimestamp": {
///      "description": "Timestamp of last rotation of the Key Vault Key.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionScopeKeyVaultProperties {
    ///The object identifier of the current versioned Key Vault Key in use.
    #[serde(
        rename = "currentVersionedKeyIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub current_versioned_key_identifier: ::std::option::Option<::std::string::String>,
    ///The object identifier for a key vault key object. When applied, the encryption scope will use the key referenced by the identifier to enable customer-managed key support on this encryption scope.
    #[serde(
        rename = "keyUri",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_uri: ::std::option::Option<::std::string::String>,
    ///Timestamp of last rotation of the Key Vault Key.
    #[serde(
        rename = "lastKeyRotationTimestamp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_key_rotation_timestamp: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EncryptionScopeKeyVaultProperties>
    for EncryptionScopeKeyVaultProperties
{
    fn from(value: &EncryptionScopeKeyVaultProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionScopeKeyVaultProperties {
    fn default() -> Self {
        Self {
            current_versioned_key_identifier: Default::default(),
            key_uri: Default::default(),
            last_key_rotation_timestamp: Default::default(),
        }
    }
}
///List of encryption scopes requested, and if paging is required, a URL to the next page of encryption scopes.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of encryption scopes requested, and if paging is required, a URL to the next page of encryption scopes.",
///  "properties": {
///    "nextLink": {
///      "description": "Request URL that can be used to query next page of encryption scopes. Returned when total number of requested encryption scopes exceeds the maximum page size.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "List of encryption scopes requested.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EncryptionScope"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionScopeListResult {
    ///Request URL that can be used to query next page of encryption scopes. Returned when total number of requested encryption scopes exceeds the maximum page size.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///List of encryption scopes requested.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<EncryptionScope>,
}
impl ::std::convert::From<&EncryptionScopeListResult> for EncryptionScopeListResult {
    fn from(value: &EncryptionScopeListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionScopeListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Properties of the encryption scope.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the encryption scope.",
///  "properties": {
///    "creationTime": {
///      "description": "Gets the creation date and time of the encryption scope in UTC.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "keyVaultProperties": {
///      "$ref": "#/components/schemas/EncryptionScopeKeyVaultProperties"
///    },
///    "lastModifiedTime": {
///      "description": "Gets the last modification date and time of the encryption scope in UTC.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "requireInfrastructureEncryption": {
///      "description": "A boolean indicating whether or not the service applies a secondary layer of encryption with platform managed keys for data at rest.",
///      "type": "boolean"
///    },
///    "source": {
///      "description": "The provider for the encryption scope. Possible values (case-insensitive):  Microsoft.Storage, Microsoft.KeyVault.",
///      "type": "string",
///      "enum": [
///        "Microsoft.Storage",
///        "Microsoft.KeyVault"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "EncryptionScopeSource"
///      }
///    },
///    "state": {
///      "description": "The state of the encryption scope. Possible values (case-insensitive):  Enabled, Disabled.",
///      "type": "string",
///      "enum": [
///        "Enabled",
///        "Disabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "EncryptionScopeState"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionScopeProperties {
    ///Gets the creation date and time of the encryption scope in UTC.
    #[serde(
        rename = "creationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_time: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "keyVaultProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_vault_properties: ::std::option::Option<EncryptionScopeKeyVaultProperties>,
    ///Gets the last modification date and time of the encryption scope in UTC.
    #[serde(
        rename = "lastModifiedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_time: ::std::option::Option<::std::string::String>,
    ///A boolean indicating whether or not the service applies a secondary layer of encryption with platform managed keys for data at rest.
    #[serde(
        rename = "requireInfrastructureEncryption",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub require_infrastructure_encryption: ::std::option::Option<bool>,
    ///The provider for the encryption scope. Possible values (case-insensitive):  Microsoft.Storage, Microsoft.KeyVault.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub source: ::std::option::Option<EncryptionScopePropertiesSource>,
    ///The state of the encryption scope. Possible values (case-insensitive):  Enabled, Disabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub state: ::std::option::Option<EncryptionScopePropertiesState>,
}
impl ::std::convert::From<&EncryptionScopeProperties> for EncryptionScopeProperties {
    fn from(value: &EncryptionScopeProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionScopeProperties {
    fn default() -> Self {
        Self {
            creation_time: Default::default(),
            key_vault_properties: Default::default(),
            last_modified_time: Default::default(),
            require_infrastructure_encryption: Default::default(),
            source: Default::default(),
            state: Default::default(),
        }
    }
}
///The provider for the encryption scope. Possible values (case-insensitive):  Microsoft.Storage, Microsoft.KeyVault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider for the encryption scope. Possible values (case-insensitive):  Microsoft.Storage, Microsoft.KeyVault.",
///  "type": "string",
///  "enum": [
///    "Microsoft.Storage",
///    "Microsoft.KeyVault"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "EncryptionScopeSource"
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
pub enum EncryptionScopePropertiesSource {
    #[serde(rename = "Microsoft.Storage")]
    MicrosoftStorage,
    #[serde(rename = "Microsoft.KeyVault")]
    MicrosoftKeyVault,
}
impl ::std::convert::From<&Self> for EncryptionScopePropertiesSource {
    fn from(value: &EncryptionScopePropertiesSource) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionScopePropertiesSource {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftStorage => f.write_str("Microsoft.Storage"),
            Self::MicrosoftKeyVault => f.write_str("Microsoft.KeyVault"),
        }
    }
}
impl ::std::str::FromStr for EncryptionScopePropertiesSource {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoft.storage" => Ok(Self::MicrosoftStorage),
            "microsoft.keyvault" => Ok(Self::MicrosoftKeyVault),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionScopePropertiesSource {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionScopePropertiesSource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionScopePropertiesSource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The state of the encryption scope. Possible values (case-insensitive):  Enabled, Disabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The state of the encryption scope. Possible values (case-insensitive):  Enabled, Disabled.",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "EncryptionScopeState"
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
pub enum EncryptionScopePropertiesState {
    Enabled,
    Disabled,
}
impl ::std::convert::From<&Self> for EncryptionScopePropertiesState {
    fn from(value: &EncryptionScopePropertiesState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionScopePropertiesState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Enabled => f.write_str("Enabled"),
            Self::Disabled => f.write_str("Disabled"),
        }
    }
}
impl ::std::str::FromStr for EncryptionScopePropertiesState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionScopePropertiesState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionScopePropertiesState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionScopePropertiesState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A service that allows server-side encryption to be used.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A service that allows server-side encryption to be used.",
///  "properties": {
///    "enabled": {
///      "description": "A boolean indicating whether or not the service encrypts the data as it is stored. Encryption at rest is enabled by default today and cannot be disabled.",
///      "type": "boolean"
///    },
///    "keyType": {
///      "description": "Encryption key type to be used for the encryption service. 'Account' key type implies that an account-scoped encryption key will be used. 'Service' key type implies that a default service key is used.",
///      "type": "string",
///      "enum": [
///        "Service",
///        "Account"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "KeyType"
///      },
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "lastEnabledTime": {
///      "description": "Gets a rough estimate of the date/time when the encryption was last enabled by the user. Data is encrypted at rest by default today and cannot be disabled.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionService {
    ///A boolean indicating whether or not the service encrypts the data as it is stored. Encryption at rest is enabled by default today and cannot be disabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///Encryption key type to be used for the encryption service. 'Account' key type implies that an account-scoped encryption key will be used. 'Service' key type implies that a default service key is used.
    #[serde(
        rename = "keyType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_type: ::std::option::Option<EncryptionServiceKeyType>,
    ///Gets a rough estimate of the date/time when the encryption was last enabled by the user. Data is encrypted at rest by default today and cannot be disabled.
    #[serde(
        rename = "lastEnabledTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_enabled_time: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EncryptionService> for EncryptionService {
    fn from(value: &EncryptionService) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionService {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
            key_type: Default::default(),
            last_enabled_time: Default::default(),
        }
    }
}
///Encryption key type to be used for the encryption service. 'Account' key type implies that an account-scoped encryption key will be used. 'Service' key type implies that a default service key is used.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Encryption key type to be used for the encryption service. 'Account' key type implies that an account-scoped encryption key will be used. 'Service' key type implies that a default service key is used.",
///  "type": "string",
///  "enum": [
///    "Service",
///    "Account"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "KeyType"
///  },
///  "x-ms-mutability": [
///    "create",
///    "read"
///  ]
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
pub enum EncryptionServiceKeyType {
    Service,
    Account,
}
impl ::std::convert::From<&Self> for EncryptionServiceKeyType {
    fn from(value: &EncryptionServiceKeyType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for EncryptionServiceKeyType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Service => f.write_str("Service"),
            Self::Account => f.write_str("Account"),
        }
    }
}
impl ::std::str::FromStr for EncryptionServiceKeyType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "service" => Ok(Self::Service),
            "account" => Ok(Self::Account),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for EncryptionServiceKeyType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for EncryptionServiceKeyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for EncryptionServiceKeyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///A list of services that support encryption.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A list of services that support encryption.",
///  "properties": {
///    "blob": {
///      "$ref": "#/components/schemas/EncryptionService"
///    },
///    "file": {
///      "$ref": "#/components/schemas/EncryptionService"
///    },
///    "queue": {
///      "$ref": "#/components/schemas/EncryptionService"
///    },
///    "table": {
///      "$ref": "#/components/schemas/EncryptionService"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EncryptionServices {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob: ::std::option::Option<EncryptionService>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub file: ::std::option::Option<EncryptionService>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub queue: ::std::option::Option<EncryptionService>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub table: ::std::option::Option<EncryptionService>,
}
impl ::std::convert::From<&EncryptionServices> for EncryptionServices {
    fn from(value: &EncryptionServices) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EncryptionServices {
    fn default() -> Self {
        Self {
            blob: Default::default(),
            file: Default::default(),
            queue: Default::default(),
            table: Default::default(),
        }
    }
}
///The URIs that are used to perform a retrieval of a public blob, queue, table, web or dfs object.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The URIs that are used to perform a retrieval of a public blob, queue, table, web or dfs object.",
///  "properties": {
///    "blob": {
///      "description": "Gets the blob endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "dfs": {
///      "description": "Gets the dfs endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "file": {
///      "description": "Gets the file endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "internetEndpoints": {
///      "$ref": "#/components/schemas/StorageAccountInternetEndpoints"
///    },
///    "microsoftEndpoints": {
///      "$ref": "#/components/schemas/StorageAccountMicrosoftEndpoints"
///    },
///    "queue": {
///      "description": "Gets the queue endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "table": {
///      "description": "Gets the table endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "web": {
///      "description": "Gets the web endpoint.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Endpoints {
    ///Gets the blob endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob: ::std::option::Option<::std::string::String>,
    ///Gets the dfs endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dfs: ::std::option::Option<::std::string::String>,
    ///Gets the file endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub file: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "internetEndpoints",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub internet_endpoints: ::std::option::Option<StorageAccountInternetEndpoints>,
    #[serde(
        rename = "microsoftEndpoints",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub microsoft_endpoints: ::std::option::Option<StorageAccountMicrosoftEndpoints>,
    ///Gets the queue endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub queue: ::std::option::Option<::std::string::String>,
    ///Gets the table endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub table: ::std::option::Option<::std::string::String>,
    ///Gets the web endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub web: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Endpoints> for Endpoints {
    fn from(value: &Endpoints) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Endpoints {
    fn default() -> Self {
        Self {
            blob: Default::default(),
            dfs: Default::default(),
            file: Default::default(),
            internet_endpoints: Default::default(),
            microsoft_endpoints: Default::default(),
            queue: Default::default(),
            table: Default::default(),
            web: Default::default(),
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
///An error response from the storage resource provider.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response from the storage resource provider.",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/ErrorResponseBody"
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
    pub error: ::std::option::Option<ErrorResponseBody>,
}
impl ::std::convert::From<&ErrorResponse> for ErrorResponse {
    fn from(value: &ErrorResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorResponse {
    fn default() -> Self {
        Self {
            error: Default::default(),
        }
    }
}
///Error response body contract.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Error response body contract.",
///  "properties": {
///    "code": {
///      "description": "An identifier for the error. Codes are invariant and are intended to be consumed programmatically.",
///      "type": "string"
///    },
///    "message": {
///      "description": "A message describing the error, intended to be suitable for display in a user interface.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ErrorResponseBody {
    ///An identifier for the error. Codes are invariant and are intended to be consumed programmatically.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    ///A message describing the error, intended to be suitable for display in a user interface.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ErrorResponseBody> for ErrorResponseBody {
    fn from(value: &ErrorResponseBody) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorResponseBody {
    fn default() -> Self {
        Self {
            code: Default::default(),
            message: Default::default(),
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
///Statistics related to replication for storage account's Blob, Table, Queue and File services. It is only available when geo-redundant replication is enabled for the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Statistics related to replication for storage account's Blob, Table, Queue and File services. It is only available when geo-redundant replication is enabled for the storage account.",
///  "properties": {
///    "canFailover": {
///      "description": "A boolean flag which indicates whether or not account failover is supported for the account.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "canPlannedFailover": {
///      "description": "A boolean flag which indicates whether or not planned account failover is supported for the account.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "lastSyncTime": {
///      "description": "All primary writes preceding this UTC date/time value are guaranteed to be available for read operations. Primary writes following this point in time may or may not be available for reads. Element may be default value if value of LastSyncTime is not available, this can happen if secondary is offline or we are in bootstrap.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "postFailoverRedundancy": {
///      "description": "The redundancy type of the account after an account failover is performed.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Standard_LRS",
///        "Standard_ZRS"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "postFailoverRedundancy"
///      }
///    },
///    "postPlannedFailoverRedundancy": {
///      "description": "The redundancy type of the account after a planned account failover is performed.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Standard_GRS",
///        "Standard_GZRS",
///        "Standard_RAGRS",
///        "Standard_RAGZRS"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "postPlannedFailoverRedundancy"
///      }
///    },
///    "status": {
///      "description": "The status of the secondary location. Possible values are: - Live: Indicates that the secondary location is active and operational. - Bootstrap: Indicates initial synchronization from the primary location to the secondary location is in progress.This typically occurs when replication is first enabled. - Unavailable: Indicates that the secondary location is temporarily unavailable.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Live",
///        "Bootstrap",
///        "Unavailable"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "GeoReplicationStatus"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GeoReplicationStats {
    ///A boolean flag which indicates whether or not account failover is supported for the account.
    #[serde(
        rename = "canFailover",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub can_failover: ::std::option::Option<bool>,
    ///A boolean flag which indicates whether or not planned account failover is supported for the account.
    #[serde(
        rename = "canPlannedFailover",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub can_planned_failover: ::std::option::Option<bool>,
    ///All primary writes preceding this UTC date/time value are guaranteed to be available for read operations. Primary writes following this point in time may or may not be available for reads. Element may be default value if value of LastSyncTime is not available, this can happen if secondary is offline or we are in bootstrap.
    #[serde(
        rename = "lastSyncTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_sync_time: ::std::option::Option<::std::string::String>,
    ///The redundancy type of the account after an account failover is performed.
    #[serde(
        rename = "postFailoverRedundancy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub post_failover_redundancy: ::std::option::Option<GeoReplicationStatsPostFailoverRedundancy>,
    ///The redundancy type of the account after a planned account failover is performed.
    #[serde(
        rename = "postPlannedFailoverRedundancy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub post_planned_failover_redundancy:
        ::std::option::Option<GeoReplicationStatsPostPlannedFailoverRedundancy>,
    ///The status of the secondary location. Possible values are: - Live: Indicates that the secondary location is active and operational. - Bootstrap: Indicates initial synchronization from the primary location to the secondary location is in progress.This typically occurs when replication is first enabled. - Unavailable: Indicates that the secondary location is temporarily unavailable.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<GeoReplicationStatsStatus>,
}
impl ::std::convert::From<&GeoReplicationStats> for GeoReplicationStats {
    fn from(value: &GeoReplicationStats) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GeoReplicationStats {
    fn default() -> Self {
        Self {
            can_failover: Default::default(),
            can_planned_failover: Default::default(),
            last_sync_time: Default::default(),
            post_failover_redundancy: Default::default(),
            post_planned_failover_redundancy: Default::default(),
            status: Default::default(),
        }
    }
}
///The redundancy type of the account after an account failover is performed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The redundancy type of the account after an account failover is performed.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Standard_LRS",
///    "Standard_ZRS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "postFailoverRedundancy"
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
pub enum GeoReplicationStatsPostFailoverRedundancy {
    #[serde(rename = "Standard_LRS")]
    StandardLrs,
    #[serde(rename = "Standard_ZRS")]
    StandardZrs,
}
impl ::std::convert::From<&Self> for GeoReplicationStatsPostFailoverRedundancy {
    fn from(value: &GeoReplicationStatsPostFailoverRedundancy) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for GeoReplicationStatsPostFailoverRedundancy {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::StandardLrs => f.write_str("Standard_LRS"),
            Self::StandardZrs => f.write_str("Standard_ZRS"),
        }
    }
}
impl ::std::str::FromStr for GeoReplicationStatsPostFailoverRedundancy {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard_lrs" => Ok(Self::StandardLrs),
            "standard_zrs" => Ok(Self::StandardZrs),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for GeoReplicationStatsPostFailoverRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GeoReplicationStatsPostFailoverRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GeoReplicationStatsPostFailoverRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The redundancy type of the account after a planned account failover is performed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The redundancy type of the account after a planned account failover is performed.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Standard_GRS",
///    "Standard_GZRS",
///    "Standard_RAGRS",
///    "Standard_RAGZRS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "postPlannedFailoverRedundancy"
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
pub enum GeoReplicationStatsPostPlannedFailoverRedundancy {
    #[serde(rename = "Standard_GRS")]
    StandardGrs,
    #[serde(rename = "Standard_GZRS")]
    StandardGzrs,
    #[serde(rename = "Standard_RAGRS")]
    StandardRagrs,
    #[serde(rename = "Standard_RAGZRS")]
    StandardRagzrs,
}
impl ::std::convert::From<&Self> for GeoReplicationStatsPostPlannedFailoverRedundancy {
    fn from(value: &GeoReplicationStatsPostPlannedFailoverRedundancy) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for GeoReplicationStatsPostPlannedFailoverRedundancy {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::StandardGrs => f.write_str("Standard_GRS"),
            Self::StandardGzrs => f.write_str("Standard_GZRS"),
            Self::StandardRagrs => f.write_str("Standard_RAGRS"),
            Self::StandardRagzrs => f.write_str("Standard_RAGZRS"),
        }
    }
}
impl ::std::str::FromStr for GeoReplicationStatsPostPlannedFailoverRedundancy {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard_grs" => Ok(Self::StandardGrs),
            "standard_gzrs" => Ok(Self::StandardGzrs),
            "standard_ragrs" => Ok(Self::StandardRagrs),
            "standard_ragzrs" => Ok(Self::StandardRagzrs),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for GeoReplicationStatsPostPlannedFailoverRedundancy {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for GeoReplicationStatsPostPlannedFailoverRedundancy
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for GeoReplicationStatsPostPlannedFailoverRedundancy
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The status of the secondary location. Possible values are: - Live: Indicates that the secondary location is active and operational. - Bootstrap: Indicates initial synchronization from the primary location to the secondary location is in progress.This typically occurs when replication is first enabled. - Unavailable: Indicates that the secondary location is temporarily unavailable.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The status of the secondary location. Possible values are: - Live: Indicates that the secondary location is active and operational. - Bootstrap: Indicates initial synchronization from the primary location to the secondary location is in progress.This typically occurs when replication is first enabled. - Unavailable: Indicates that the secondary location is temporarily unavailable.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Live",
///    "Bootstrap",
///    "Unavailable"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "GeoReplicationStatus"
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
pub enum GeoReplicationStatsStatus {
    Live,
    Bootstrap,
    Unavailable,
}
impl ::std::convert::From<&Self> for GeoReplicationStatsStatus {
    fn from(value: &GeoReplicationStatsStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for GeoReplicationStatsStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Live => f.write_str("Live"),
            Self::Bootstrap => f.write_str("Bootstrap"),
            Self::Unavailable => f.write_str("Unavailable"),
        }
    }
}
impl ::std::str::FromStr for GeoReplicationStatsStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "live" => Ok(Self::Live),
            "bootstrap" => Ok(Self::Bootstrap),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for GeoReplicationStatsStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GeoReplicationStatsStatus {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GeoReplicationStatsStatus {
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
///  "required": [
///    "type"
///  ],
///  "properties": {
///    "principalId": {
///      "description": "The principal ID of resource identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "The tenant ID of resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "type": {
///      "description": "The identity type.",
///      "type": "string",
///      "enum": [
///        "None",
///        "SystemAssigned",
///        "UserAssigned",
///        "SystemAssigned,UserAssigned"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "IdentityType"
///      }
///    },
///    "userAssignedIdentities": {
///      "description": "Gets or sets a list of key value pairs that describe the set of User Assigned identities that will be used with this storage account. The key is the ARM resource identifier of the identity. Only 1 User Assigned identity is permitted here.",
///      "type": "object",
///      "additionalProperties": {
///        "$ref": "#/components/schemas/UserAssignedIdentity"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Identity {
    ///The principal ID of resource identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
    ///The tenant ID of resource.
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
    ///The identity type.
    #[serde(rename = "type")]
    pub type_: IdentityType,
    ///Gets or sets a list of key value pairs that describe the set of User Assigned identities that will be used with this storage account. The key is the ARM resource identifier of the identity. Only 1 User Assigned identity is permitted here.
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities:
        ::std::collections::HashMap<::std::string::String, UserAssignedIdentity>,
}
impl ::std::convert::From<&Identity> for Identity {
    fn from(value: &Identity) -> Self {
        value.clone()
    }
}
///The identity type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The identity type.",
///  "type": "string",
///  "enum": [
///    "None",
///    "SystemAssigned",
///    "UserAssigned",
///    "SystemAssigned,UserAssigned"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "IdentityType"
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
pub enum IdentityType {
    None,
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned,UserAssigned")]
    SystemAssignedUserAssigned,
}
impl ::std::convert::From<&Self> for IdentityType {
    fn from(value: &IdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => f.write_str("SystemAssigned,UserAssigned"),
        }
    }
}
impl ::std::str::FromStr for IdentityType {
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
impl ::std::convert::TryFrom<&str> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IdentityType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///This property enables and defines account-level immutability. Enabling the feature auto-enables Blob Versioning.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This property enables and defines account-level immutability. Enabling the feature auto-enables Blob Versioning.",
///  "type": "object",
///  "properties": {
///    "enabled": {
///      "description": "A boolean flag which enables account-level immutability. All the containers under such an account have object-level immutability enabled by default.",
///      "type": "boolean"
///    },
///    "immutabilityPolicy": {
///      "$ref": "#/components/schemas/AccountImmutabilityPolicyProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ImmutableStorageAccount {
    ///A boolean flag which enables account-level immutability. All the containers under such an account have object-level immutability enabled by default.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    #[serde(
        rename = "immutabilityPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutability_policy: ::std::option::Option<AccountImmutabilityPolicyProperties>,
}
impl ::std::convert::From<&ImmutableStorageAccount> for ImmutableStorageAccount {
    fn from(value: &ImmutableStorageAccount) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ImmutableStorageAccount {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
            immutability_policy: Default::default(),
        }
    }
}
///IP rule with specific IP or IP range in CIDR format.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "IP rule with specific IP or IP range in CIDR format.",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "action": {
///      "description": "The action of IP ACL rule.",
///      "default": "Allow",
///      "type": "string",
///      "enum": [
///        "Allow"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "Action"
///      }
///    },
///    "value": {
///      "description": "Specifies the IP or IP range in CIDR format. Only IPV4 address is allowed.",
///      "type": "string",
///      "x-ms-client-name": "IPAddressOrRange"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IpRule {
    ///The action of IP ACL rule.
    #[serde(
        default = "defaults::ip_rule_action",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action: IpRuleAction,
    ///Specifies the IP or IP range in CIDR format. Only IPV4 address is allowed.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&IpRule> for IpRule {
    fn from(value: &IpRule) -> Self {
        value.clone()
    }
}
///The action of IP ACL rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The action of IP ACL rule.",
///  "default": "Allow",
///  "type": "string",
///  "enum": [
///    "Allow"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "Action"
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
pub enum IpRuleAction {
    Allow,
}
impl ::std::convert::From<&Self> for IpRuleAction {
    fn from(value: &IpRuleAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IpRuleAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
        }
    }
}
impl ::std::str::FromStr for IpRuleAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for IpRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for IpRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for IpRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for IpRuleAction {
    fn default() -> Self {
        IpRuleAction::Allow
    }
}
///Storage account keys creation time.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Storage account keys creation time.",
///  "properties": {
///    "key1": {
///      "examples": [
///        "2021-02-03T05:57:30.917Z"
///      ],
///      "type": "string"
///    },
///    "key2": {
///      "examples": [
///        "2021-02-03T05:57:30.917Z"
///      ],
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyCreationTime {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key1: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key2: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&KeyCreationTime> for KeyCreationTime {
    fn from(value: &KeyCreationTime) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KeyCreationTime {
    fn default() -> Self {
        Self {
            key1: Default::default(),
            key2: Default::default(),
        }
    }
}
///KeyPolicy assigned to the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "KeyPolicy assigned to the storage account.",
///  "required": [
///    "keyExpirationPeriodInDays"
///  ],
///  "properties": {
///    "keyExpirationPeriodInDays": {
///      "description": "The key expiration period in days.",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyPolicy {
    ///The key expiration period in days.
    #[serde(rename = "keyExpirationPeriodInDays")]
    pub key_expiration_period_in_days: i32,
}
impl ::std::convert::From<&KeyPolicy> for KeyPolicy {
    fn from(value: &KeyPolicy) -> Self {
        value.clone()
    }
}
///Properties of key vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of key vault.",
///  "properties": {
///    "currentVersionedKeyExpirationTimestamp": {
///      "description": "This is a read only property that represents the expiration time of the current version of the customer managed key used for encryption.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-client-name": "CurrentVersionedKeyExpirationTimestamp"
///    },
///    "currentVersionedKeyIdentifier": {
///      "description": "The object identifier of the current versioned Key Vault Key in use.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-client-name": "CurrentVersionedKeyIdentifier"
///    },
///    "keyname": {
///      "description": "The name of KeyVault key.",
///      "type": "string",
///      "x-ms-client-name": "KeyName"
///    },
///    "keyvaulturi": {
///      "description": "The Uri of KeyVault.",
///      "type": "string",
///      "x-ms-client-name": "KeyVaultUri"
///    },
///    "keyversion": {
///      "description": "The version of KeyVault key.",
///      "type": "string",
///      "x-ms-client-name": "KeyVersion"
///    },
///    "lastKeyRotationTimestamp": {
///      "description": "Timestamp of last rotation of the Key Vault Key.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-client-name": "LastKeyRotationTimestamp"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyVaultProperties {
    ///This is a read only property that represents the expiration time of the current version of the customer managed key used for encryption.
    #[serde(
        rename = "currentVersionedKeyExpirationTimestamp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub current_versioned_key_expiration_timestamp: ::std::option::Option<::std::string::String>,
    ///The object identifier of the current versioned Key Vault Key in use.
    #[serde(
        rename = "currentVersionedKeyIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub current_versioned_key_identifier: ::std::option::Option<::std::string::String>,
    ///The name of KeyVault key.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keyname: ::std::option::Option<::std::string::String>,
    ///The Uri of KeyVault.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keyvaulturi: ::std::option::Option<::std::string::String>,
    ///The version of KeyVault key.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keyversion: ::std::option::Option<::std::string::String>,
    ///Timestamp of last rotation of the Key Vault Key.
    #[serde(
        rename = "lastKeyRotationTimestamp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_key_rotation_timestamp: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&KeyVaultProperties> for KeyVaultProperties {
    fn from(value: &KeyVaultProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KeyVaultProperties {
    fn default() -> Self {
        Self {
            current_versioned_key_expiration_timestamp: Default::default(),
            current_versioned_key_identifier: Default::default(),
            keyname: Default::default(),
            keyvaulturi: Default::default(),
            keyversion: Default::default(),
            last_key_rotation_timestamp: Default::default(),
        }
    }
}
///The List SAS credentials operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List SAS credentials operation response.",
///  "properties": {
///    "accountSasToken": {
///      "description": "List SAS credentials of storage account.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListAccountSasResponse {
    ///List SAS credentials of storage account.
    #[serde(
        rename = "accountSasToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub account_sas_token: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListAccountSasResponse> for ListAccountSasResponse {
    fn from(value: &ListAccountSasResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListAccountSasResponse {
    fn default() -> Self {
        Self {
            account_sas_token: Default::default(),
        }
    }
}
///List of blob inventory policies returned.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of blob inventory policies returned.",
///  "properties": {
///    "value": {
///      "description": "List of blob inventory policies.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/BlobInventoryPolicy"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListBlobInventoryPolicy {
    ///List of blob inventory policies.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<BlobInventoryPolicy>,
}
impl ::std::convert::From<&ListBlobInventoryPolicy> for ListBlobInventoryPolicy {
    fn from(value: &ListBlobInventoryPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListBlobInventoryPolicy {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///The List service SAS credentials operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The List service SAS credentials operation response.",
///  "properties": {
///    "serviceSasToken": {
///      "description": "List service SAS credentials of specific resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ListServiceSasResponse {
    ///List service SAS credentials of specific resource.
    #[serde(
        rename = "serviceSasToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_sas_token: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ListServiceSasResponse> for ListServiceSasResponse {
    fn from(value: &ListServiceSasResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ListServiceSasResponse {
    fn default() -> Self {
        Self {
            service_sas_token: Default::default(),
        }
    }
}
///The local user associated with the storage accounts.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The local user associated with the storage accounts.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/LocalUserProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LocalUser {
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
    pub properties: ::std::option::Option<LocalUserProperties>,
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
impl ::std::convert::From<&LocalUser> for LocalUser {
    fn from(value: &LocalUser) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LocalUser {
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
///The Storage Account Local User keys.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Storage Account Local User keys.",
///  "type": "object",
///  "properties": {
///    "sharedKey": {
///      "$ref": "#/components/schemas/SharedKey"
///    },
///    "sshAuthorizedKeys": {
///      "$ref": "#/components/schemas/SshAuthorizedKeys"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LocalUserKeys {
    #[serde(
        rename = "sharedKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub shared_key: ::std::option::Option<SharedKey>,
    #[serde(
        rename = "sshAuthorizedKeys",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ssh_authorized_keys: ::std::option::Option<SshAuthorizedKeys>,
}
impl ::std::convert::From<&LocalUserKeys> for LocalUserKeys {
    fn from(value: &LocalUserKeys) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LocalUserKeys {
    fn default() -> Self {
        Self {
            shared_key: Default::default(),
            ssh_authorized_keys: Default::default(),
        }
    }
}
///The Storage Account Local User properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Storage Account Local User properties.",
///  "type": "object",
///  "properties": {
///    "allowAclAuthorization": {
///      "description": "Indicates whether ACL authorization is allowed for this user. Set it to false to disallow using ACL authorization.",
///      "type": "boolean"
///    },
///    "extendedGroups": {
///      "description": "Supplementary group membership. Only applicable for local users enabled for NFSv3 access.",
///      "type": "array",
///      "items": {
///        "type": "integer",
///        "format": "int32"
///      }
///    },
///    "groupId": {
///      "description": "An identifier for associating a group of users.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "hasSharedKey": {
///      "description": "Indicates whether shared key exists. Set it to false to remove existing shared key.",
///      "type": "boolean"
///    },
///    "hasSshKey": {
///      "description": "Indicates whether ssh key exists. Set it to false to remove existing SSH key.",
///      "type": "boolean"
///    },
///    "hasSshPassword": {
///      "description": "Indicates whether ssh password exists. Set it to false to remove existing SSH password.",
///      "type": "boolean"
///    },
///    "homeDirectory": {
///      "description": "Optional, local user home directory.",
///      "type": "string"
///    },
///    "isNFSv3Enabled": {
///      "description": "Indicates if the local user is enabled for access with NFSv3 protocol.",
///      "type": "boolean"
///    },
///    "permissionScopes": {
///      "description": "The permission scopes of the local user.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PermissionScope"
///      }
///    },
///    "sid": {
///      "description": "A unique Security Identifier that is generated by the server.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "sshAuthorizedKeys": {
///      "$ref": "#/components/schemas/SshAuthorizedKeys"
///    },
///    "userId": {
///      "description": "A unique Identifier that is generated by the server.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LocalUserProperties {
    ///Indicates whether ACL authorization is allowed for this user. Set it to false to disallow using ACL authorization.
    #[serde(
        rename = "allowAclAuthorization",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_acl_authorization: ::std::option::Option<bool>,
    ///Supplementary group membership. Only applicable for local users enabled for NFSv3 access.
    #[serde(
        rename = "extendedGroups",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_groups: ::std::vec::Vec<i32>,
    ///An identifier for associating a group of users.
    #[serde(
        rename = "groupId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub group_id: ::std::option::Option<i32>,
    ///Indicates whether shared key exists. Set it to false to remove existing shared key.
    #[serde(
        rename = "hasSharedKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_shared_key: ::std::option::Option<bool>,
    ///Indicates whether ssh key exists. Set it to false to remove existing SSH key.
    #[serde(
        rename = "hasSshKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_ssh_key: ::std::option::Option<bool>,
    ///Indicates whether ssh password exists. Set it to false to remove existing SSH password.
    #[serde(
        rename = "hasSshPassword",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub has_ssh_password: ::std::option::Option<bool>,
    ///Optional, local user home directory.
    #[serde(
        rename = "homeDirectory",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub home_directory: ::std::option::Option<::std::string::String>,
    ///Indicates if the local user is enabled for access with NFSv3 protocol.
    #[serde(
        rename = "isNFSv3Enabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_nf_sv3_enabled: ::std::option::Option<bool>,
    ///The permission scopes of the local user.
    #[serde(
        rename = "permissionScopes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub permission_scopes: ::std::vec::Vec<PermissionScope>,
    ///A unique Security Identifier that is generated by the server.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sid: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "sshAuthorizedKeys",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ssh_authorized_keys: ::std::option::Option<SshAuthorizedKeys>,
    ///A unique Identifier that is generated by the server.
    #[serde(
        rename = "userId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_id: ::std::option::Option<i32>,
}
impl ::std::convert::From<&LocalUserProperties> for LocalUserProperties {
    fn from(value: &LocalUserProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LocalUserProperties {
    fn default() -> Self {
        Self {
            allow_acl_authorization: Default::default(),
            extended_groups: Default::default(),
            group_id: Default::default(),
            has_shared_key: Default::default(),
            has_ssh_key: Default::default(),
            has_ssh_password: Default::default(),
            home_directory: Default::default(),
            is_nf_sv3_enabled: Default::default(),
            permission_scopes: Default::default(),
            sid: Default::default(),
            ssh_authorized_keys: Default::default(),
            user_id: Default::default(),
        }
    }
}
///The secrets of Storage Account Local User.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secrets of Storage Account Local User.",
///  "type": "object",
///  "properties": {
///    "sshPassword": {
///      "description": "Auto generated password by the server for SSH authentication if hasSshPassword is set to true on the creation of local user.",
///      "readOnly": true,
///      "type": "string",
///      "x-ms-secret": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LocalUserRegeneratePasswordResult {
    ///Auto generated password by the server for SSH authentication if hasSshPassword is set to true on the creation of local user.
    #[serde(
        rename = "sshPassword",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ssh_password: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&LocalUserRegeneratePasswordResult>
    for LocalUserRegeneratePasswordResult
{
    fn from(value: &LocalUserRegeneratePasswordResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LocalUserRegeneratePasswordResult {
    fn default() -> Self {
        Self {
            ssh_password: Default::default(),
        }
    }
}
///List of local users requested, and if paging is required, a URL to the next page of local users.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of local users requested, and if paging is required, a URL to the next page of local users.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "Request URL that can be used to query next page of local users. Returned when total number of requested local users exceeds the maximum page size.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of local users associated with the storage account.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/LocalUser"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LocalUsers {
    ///Request URL that can be used to query next page of local users. Returned when total number of requested local users exceeds the maximum page size.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of local users associated with the storage account.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<LocalUser>,
}
impl ::std::convert::From<&LocalUsers> for LocalUsers {
    fn from(value: &LocalUsers) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LocalUsers {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The Get Storage Account ManagementPolicies operation response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Get Storage Account ManagementPolicies operation response.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ManagementPolicyProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicy {
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
    pub properties: ::std::option::Option<ManagementPolicyProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ManagementPolicy> for ManagementPolicy {
    fn from(value: &ManagementPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagementPolicy {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Actions are applied to the filtered blobs when the execution condition is met.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Actions are applied to the filtered blobs when the execution condition is met.",
///  "properties": {
///    "baseBlob": {
///      "$ref": "#/components/schemas/ManagementPolicyBaseBlob"
///    },
///    "snapshot": {
///      "$ref": "#/components/schemas/ManagementPolicySnapShot"
///    },
///    "version": {
///      "$ref": "#/components/schemas/ManagementPolicyVersion"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicyAction {
    #[serde(
        rename = "baseBlob",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub base_blob: ::std::option::Option<ManagementPolicyBaseBlob>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub snapshot: ::std::option::Option<ManagementPolicySnapShot>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<ManagementPolicyVersion>,
}
impl ::std::convert::From<&ManagementPolicyAction> for ManagementPolicyAction {
    fn from(value: &ManagementPolicyAction) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagementPolicyAction {
    fn default() -> Self {
        Self {
            base_blob: Default::default(),
            snapshot: Default::default(),
            version: Default::default(),
        }
    }
}
///Management policy action for base blob.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Management policy action for base blob.",
///  "properties": {
///    "delete": {
///      "$ref": "#/components/schemas/DateAfterModification"
///    },
///    "enableAutoTierToHotFromCool": {
///      "description": "This property enables auto tiering of a blob from cool to hot on a blob access. This property requires tierToCool.daysAfterLastAccessTimeGreaterThan.",
///      "type": "boolean"
///    },
///    "tierToArchive": {
///      "$ref": "#/components/schemas/DateAfterModification"
///    },
///    "tierToCold": {
///      "$ref": "#/components/schemas/DateAfterModification"
///    },
///    "tierToCool": {
///      "$ref": "#/components/schemas/DateAfterModification"
///    },
///    "tierToHot": {
///      "$ref": "#/components/schemas/DateAfterModification"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicyBaseBlob {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delete: ::std::option::Option<DateAfterModification>,
    ///This property enables auto tiering of a blob from cool to hot on a blob access. This property requires tierToCool.daysAfterLastAccessTimeGreaterThan.
    #[serde(
        rename = "enableAutoTierToHotFromCool",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_auto_tier_to_hot_from_cool: ::std::option::Option<bool>,
    #[serde(
        rename = "tierToArchive",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_archive: ::std::option::Option<DateAfterModification>,
    #[serde(
        rename = "tierToCold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_cold: ::std::option::Option<DateAfterModification>,
    #[serde(
        rename = "tierToCool",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_cool: ::std::option::Option<DateAfterModification>,
    #[serde(
        rename = "tierToHot",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_hot: ::std::option::Option<DateAfterModification>,
}
impl ::std::convert::From<&ManagementPolicyBaseBlob> for ManagementPolicyBaseBlob {
    fn from(value: &ManagementPolicyBaseBlob) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagementPolicyBaseBlob {
    fn default() -> Self {
        Self {
            delete: Default::default(),
            enable_auto_tier_to_hot_from_cool: Default::default(),
            tier_to_archive: Default::default(),
            tier_to_cold: Default::default(),
            tier_to_cool: Default::default(),
            tier_to_hot: Default::default(),
        }
    }
}
///An object that defines the Lifecycle rule. Each definition is made up with a filters set and an actions set.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that defines the Lifecycle rule. Each definition is made up with a filters set and an actions set.",
///  "required": [
///    "actions"
///  ],
///  "properties": {
///    "actions": {
///      "$ref": "#/components/schemas/ManagementPolicyAction"
///    },
///    "filters": {
///      "$ref": "#/components/schemas/ManagementPolicyFilter"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicyDefinition {
    pub actions: ManagementPolicyAction,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub filters: ::std::option::Option<ManagementPolicyFilter>,
}
impl ::std::convert::From<&ManagementPolicyDefinition> for ManagementPolicyDefinition {
    fn from(value: &ManagementPolicyDefinition) -> Self {
        value.clone()
    }
}
///Filters limit rule actions to a subset of blobs within the storage account. If multiple filters are defined, a logical AND is performed on all filters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Filters limit rule actions to a subset of blobs within the storage account. If multiple filters are defined, a logical AND is performed on all filters. ",
///  "required": [
///    "blobTypes"
///  ],
///  "properties": {
///    "blobIndexMatch": {
///      "description": "An array of blob index tag based filters, there can be at most 10 tag filters",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TagFilter"
///      }
///    },
///    "blobTypes": {
///      "description": "An array of predefined enum values. Currently blockBlob supports all tiering and delete actions. Only delete actions are supported for appendBlob.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "prefixMatch": {
///      "description": "An array of strings for prefixes to be match.",
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
pub struct ManagementPolicyFilter {
    ///An array of blob index tag based filters, there can be at most 10 tag filters
    #[serde(
        rename = "blobIndexMatch",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob_index_match: ::std::vec::Vec<TagFilter>,
    ///An array of predefined enum values. Currently blockBlob supports all tiering and delete actions. Only delete actions are supported for appendBlob.
    #[serde(rename = "blobTypes")]
    pub blob_types: ::std::vec::Vec<::std::string::String>,
    ///An array of strings for prefixes to be match.
    #[serde(
        rename = "prefixMatch",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub prefix_match: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ManagementPolicyFilter> for ManagementPolicyFilter {
    fn from(value: &ManagementPolicyFilter) -> Self {
        value.clone()
    }
}
///The Storage Account ManagementPolicy properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Storage Account ManagementPolicy properties.",
///  "required": [
///    "policy"
///  ],
///  "properties": {
///    "lastModifiedTime": {
///      "description": "Returns the date and time the ManagementPolicies was last modified.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/ManagementPolicySchema"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicyProperties {
    ///Returns the date and time the ManagementPolicies was last modified.
    #[serde(
        rename = "lastModifiedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_modified_time: ::std::option::Option<::std::string::String>,
    pub policy: ManagementPolicySchema,
}
impl ::std::convert::From<&ManagementPolicyProperties> for ManagementPolicyProperties {
    fn from(value: &ManagementPolicyProperties) -> Self {
        value.clone()
    }
}
///An object that wraps the Lifecycle rule. Each rule is uniquely defined by name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An object that wraps the Lifecycle rule. Each rule is uniquely defined by name.",
///  "required": [
///    "definition",
///    "name",
///    "type"
///  ],
///  "properties": {
///    "definition": {
///      "$ref": "#/components/schemas/ManagementPolicyDefinition"
///    },
///    "enabled": {
///      "description": "Rule is enabled if set to true.",
///      "type": "boolean"
///    },
///    "name": {
///      "description": "A rule name can contain any combination of alpha numeric characters. Rule name is case-sensitive. It must be unique within a policy.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The valid value is Lifecycle",
///      "type": "string",
///      "enum": [
///        "Lifecycle"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "RuleType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicyRule {
    pub definition: ManagementPolicyDefinition,
    ///Rule is enabled if set to true.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///A rule name can contain any combination of alpha numeric characters. Rule name is case-sensitive. It must be unique within a policy.
    pub name: ::std::string::String,
    ///The valid value is Lifecycle
    #[serde(rename = "type")]
    pub type_: ManagementPolicyRuleType,
}
impl ::std::convert::From<&ManagementPolicyRule> for ManagementPolicyRule {
    fn from(value: &ManagementPolicyRule) -> Self {
        value.clone()
    }
}
///The valid value is Lifecycle
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The valid value is Lifecycle",
///  "type": "string",
///  "enum": [
///    "Lifecycle"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "RuleType"
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
pub enum ManagementPolicyRuleType {
    Lifecycle,
}
impl ::std::convert::From<&Self> for ManagementPolicyRuleType {
    fn from(value: &ManagementPolicyRuleType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ManagementPolicyRuleType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Lifecycle => f.write_str("Lifecycle"),
        }
    }
}
impl ::std::str::FromStr for ManagementPolicyRuleType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "lifecycle" => Ok(Self::Lifecycle),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ManagementPolicyRuleType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ManagementPolicyRuleType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ManagementPolicyRuleType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The Storage Account ManagementPolicies Rules. See more details in: https://learn.microsoft.com/azure/storage/blobs/lifecycle-management-overview.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Storage Account ManagementPolicies Rules. See more details in: https://learn.microsoft.com/azure/storage/blobs/lifecycle-management-overview.",
///  "required": [
///    "rules"
///  ],
///  "properties": {
///    "rules": {
///      "description": "The Storage Account ManagementPolicies Rules. See more details in: https://learn.microsoft.com/azure/storage/blobs/lifecycle-management-overview.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ManagementPolicyRule"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicySchema {
    ///The Storage Account ManagementPolicies Rules. See more details in: https://learn.microsoft.com/azure/storage/blobs/lifecycle-management-overview.
    pub rules: ::std::vec::Vec<ManagementPolicyRule>,
}
impl ::std::convert::From<&ManagementPolicySchema> for ManagementPolicySchema {
    fn from(value: &ManagementPolicySchema) -> Self {
        value.clone()
    }
}
///Management policy action for snapshot.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Management policy action for snapshot.",
///  "properties": {
///    "delete": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToArchive": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToCold": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToCool": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToHot": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicySnapShot {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delete: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToArchive",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_archive: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToCold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_cold: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToCool",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_cool: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToHot",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_hot: ::std::option::Option<DateAfterCreation>,
}
impl ::std::convert::From<&ManagementPolicySnapShot> for ManagementPolicySnapShot {
    fn from(value: &ManagementPolicySnapShot) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagementPolicySnapShot {
    fn default() -> Self {
        Self {
            delete: Default::default(),
            tier_to_archive: Default::default(),
            tier_to_cold: Default::default(),
            tier_to_cool: Default::default(),
            tier_to_hot: Default::default(),
        }
    }
}
///Management policy action for blob version.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Management policy action for blob version.",
///  "properties": {
///    "delete": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToArchive": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToCold": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToCool": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    },
///    "tierToHot": {
///      "$ref": "#/components/schemas/DateAfterCreation"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagementPolicyVersion {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delete: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToArchive",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_archive: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToCold",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_cold: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToCool",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_cool: ::std::option::Option<DateAfterCreation>,
    #[serde(
        rename = "tierToHot",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier_to_hot: ::std::option::Option<DateAfterCreation>,
}
impl ::std::convert::From<&ManagementPolicyVersion> for ManagementPolicyVersion {
    fn from(value: &ManagementPolicyVersion) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagementPolicyVersion {
    fn default() -> Self {
        Self {
            delete: Default::default(),
            tier_to_archive: Default::default(),
            tier_to_cold: Default::default(),
            tier_to_cool: Default::default(),
            tier_to_hot: Default::default(),
        }
    }
}
///Metric specification of operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Metric specification of operation.",
///  "properties": {
///    "aggregationType": {
///      "description": "Aggregation type could be Average.",
///      "type": "string"
///    },
///    "category": {
///      "description": "The category this metric specification belong to, could be Capacity.",
///      "type": "string"
///    },
///    "dimensions": {
///      "description": "Dimensions of blobs, including blob type and access tier.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Dimension"
///      }
///    },
///    "displayDescription": {
///      "description": "Display description of metric specification.",
///      "type": "string"
///    },
///    "displayName": {
///      "description": "Display name of metric specification.",
///      "type": "string"
///    },
///    "fillGapWithZero": {
///      "description": "The property to decide fill gap with zero or not.",
///      "type": "boolean"
///    },
///    "name": {
///      "description": "Name of metric specification.",
///      "type": "string"
///    },
///    "resourceIdDimensionNameOverride": {
///      "description": "Account Resource Id.",
///      "type": "string"
///    },
///    "unit": {
///      "description": "Unit could be Bytes or Count.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct MetricSpecification {
    ///Aggregation type could be Average.
    #[serde(
        rename = "aggregationType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub aggregation_type: ::std::option::Option<::std::string::String>,
    ///The category this metric specification belong to, could be Capacity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub category: ::std::option::Option<::std::string::String>,
    ///Dimensions of blobs, including blob type and access tier.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dimensions: ::std::vec::Vec<Dimension>,
    ///Display description of metric specification.
    #[serde(
        rename = "displayDescription",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_description: ::std::option::Option<::std::string::String>,
    ///Display name of metric specification.
    #[serde(
        rename = "displayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display_name: ::std::option::Option<::std::string::String>,
    ///The property to decide fill gap with zero or not.
    #[serde(
        rename = "fillGapWithZero",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub fill_gap_with_zero: ::std::option::Option<bool>,
    ///Name of metric specification.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Account Resource Id.
    #[serde(
        rename = "resourceIdDimensionNameOverride",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_id_dimension_name_override: ::std::option::Option<::std::string::String>,
    ///Unit could be Bytes or Count.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub unit: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&MetricSpecification> for MetricSpecification {
    fn from(value: &MetricSpecification) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for MetricSpecification {
    fn default() -> Self {
        Self {
            aggregation_type: Default::default(),
            category: Default::default(),
            dimensions: Default::default(),
            display_description: Default::default(),
            display_name: Default::default(),
            fill_gap_with_zero: Default::default(),
            name: Default::default(),
            resource_id_dimension_name_override: Default::default(),
            unit: Default::default(),
        }
    }
}
///Network rule set
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Network rule set",
///  "required": [
///    "defaultAction"
///  ],
///  "properties": {
///    "bypass": {
///      "description": "Specifies whether traffic is bypassed for Logging/Metrics/AzureServices. Possible values are any combination of Logging|Metrics|AzureServices (For example, \"Logging, Metrics\"), or None to bypass none of those traffics.",
///      "default": "AzureServices",
///      "type": "string",
///      "enum": [
///        "None",
///        "Logging",
///        "Metrics",
///        "AzureServices"
///      ],
///      "x-ms-client-name": "Bypass",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Bypass"
///      }
///    },
///    "defaultAction": {
///      "description": "Specifies the default action of allow or deny when no other rules match.",
///      "default": "Allow",
///      "type": "string",
///      "enum": [
///        "Allow",
///        "Deny"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "DefaultAction"
///      }
///    },
///    "ipRules": {
///      "description": "Sets the IP ACL rules",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/IPRule"
///      }
///    },
///    "resourceAccessRules": {
///      "description": "Sets the resource access rules",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ResourceAccessRule"
///      }
///    },
///    "virtualNetworkRules": {
///      "description": "Sets the virtual network rules",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/VirtualNetworkRule"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct NetworkRuleSet {
    ///Specifies whether traffic is bypassed for Logging/Metrics/AzureServices. Possible values are any combination of Logging|Metrics|AzureServices (For example, "Logging, Metrics"), or None to bypass none of those traffics.
    #[serde(
        default = "defaults::network_rule_set_bypass",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub bypass: NetworkRuleSetBypass,
    ///Specifies the default action of allow or deny when no other rules match.
    #[serde(
        rename = "defaultAction",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_action: NetworkRuleSetDefaultAction,
    ///Sets the IP ACL rules
    #[serde(
        rename = "ipRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ip_rules: ::std::vec::Vec<IpRule>,
    ///Sets the resource access rules
    #[serde(
        rename = "resourceAccessRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_access_rules: ::std::vec::Vec<ResourceAccessRule>,
    ///Sets the virtual network rules
    #[serde(
        rename = "virtualNetworkRules",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub virtual_network_rules: ::std::vec::Vec<VirtualNetworkRule>,
}
impl ::std::convert::From<&NetworkRuleSet> for NetworkRuleSet {
    fn from(value: &NetworkRuleSet) -> Self {
        value.clone()
    }
}
///Specifies whether traffic is bypassed for Logging/Metrics/AzureServices. Possible values are any combination of Logging|Metrics|AzureServices (For example, "Logging, Metrics"), or None to bypass none of those traffics.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies whether traffic is bypassed for Logging/Metrics/AzureServices. Possible values are any combination of Logging|Metrics|AzureServices (For example, \"Logging, Metrics\"), or None to bypass none of those traffics.",
///  "default": "AzureServices",
///  "type": "string",
///  "enum": [
///    "None",
///    "Logging",
///    "Metrics",
///    "AzureServices"
///  ],
///  "x-ms-client-name": "Bypass",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Bypass"
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
pub enum NetworkRuleSetBypass {
    None,
    Logging,
    Metrics,
    AzureServices,
}
impl ::std::convert::From<&Self> for NetworkRuleSetBypass {
    fn from(value: &NetworkRuleSetBypass) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkRuleSetBypass {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Logging => f.write_str("Logging"),
            Self::Metrics => f.write_str("Metrics"),
            Self::AzureServices => f.write_str("AzureServices"),
        }
    }
}
impl ::std::str::FromStr for NetworkRuleSetBypass {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "logging" => Ok(Self::Logging),
            "metrics" => Ok(Self::Metrics),
            "azureservices" => Ok(Self::AzureServices),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkRuleSetBypass {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkRuleSetBypass {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkRuleSetBypass {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for NetworkRuleSetBypass {
    fn default() -> Self {
        NetworkRuleSetBypass::AzureServices
    }
}
///Specifies the default action of allow or deny when no other rules match.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specifies the default action of allow or deny when no other rules match.",
///  "default": "Allow",
///  "type": "string",
///  "enum": [
///    "Allow",
///    "Deny"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "DefaultAction"
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
pub enum NetworkRuleSetDefaultAction {
    Allow,
    Deny,
}
impl ::std::convert::From<&Self> for NetworkRuleSetDefaultAction {
    fn from(value: &NetworkRuleSetDefaultAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for NetworkRuleSetDefaultAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
            Self::Deny => f.write_str("Deny"),
        }
    }
}
impl ::std::str::FromStr for NetworkRuleSetDefaultAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for NetworkRuleSetDefaultAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for NetworkRuleSetDefaultAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for NetworkRuleSetDefaultAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for NetworkRuleSetDefaultAction {
    fn default() -> Self {
        NetworkRuleSetDefaultAction::Allow
    }
}
///List storage account object replication policies.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List storage account object replication policies.",
///  "properties": {
///    "value": {
///      "description": "The replication policy between two storage accounts.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ObjectReplicationPolicy"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ObjectReplicationPolicies {
    ///The replication policy between two storage accounts.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ObjectReplicationPolicy>,
}
impl ::std::convert::From<&ObjectReplicationPolicies> for ObjectReplicationPolicies {
    fn from(value: &ObjectReplicationPolicies) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ObjectReplicationPolicies {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///The replication policy between two storage accounts. Multiple rules can be defined in one policy.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The replication policy between two storage accounts. Multiple rules can be defined in one policy.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/ObjectReplicationPolicyProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ObjectReplicationPolicy {
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
    pub properties: ::std::option::Option<ObjectReplicationPolicyProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ObjectReplicationPolicy> for ObjectReplicationPolicy {
    fn from(value: &ObjectReplicationPolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ObjectReplicationPolicy {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///Filters limit replication to a subset of blobs within the storage account. A logical OR is performed on values in the filter. If multiple filters are defined, a logical AND is performed on all filters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Filters limit replication to a subset of blobs within the storage account. A logical OR is performed on values in the filter. If multiple filters are defined, a logical AND is performed on all filters.",
///  "properties": {
///    "minCreationTime": {
///      "description": "Blobs created after the time will be replicated to the destination. It must be in datetime format 'yyyy-MM-ddTHH:mm:ssZ'. Example: 2020-02-19T16:05:00Z",
///      "type": "string"
///    },
///    "prefixMatch": {
///      "description": "Optional. Filters the results to replicate only blobs whose names begin with the specified prefix.",
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
pub struct ObjectReplicationPolicyFilter {
    ///Blobs created after the time will be replicated to the destination. It must be in datetime format 'yyyy-MM-ddTHH:mm:ssZ'. Example: 2020-02-19T16:05:00Z
    #[serde(
        rename = "minCreationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub min_creation_time: ::std::option::Option<::std::string::String>,
    ///Optional. Filters the results to replicate only blobs whose names begin with the specified prefix.
    #[serde(
        rename = "prefixMatch",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub prefix_match: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ObjectReplicationPolicyFilter> for ObjectReplicationPolicyFilter {
    fn from(value: &ObjectReplicationPolicyFilter) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ObjectReplicationPolicyFilter {
    fn default() -> Self {
        Self {
            min_creation_time: Default::default(),
            prefix_match: Default::default(),
        }
    }
}
///The Storage Account ObjectReplicationPolicy properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Storage Account ObjectReplicationPolicy properties.",
///  "required": [
///    "destinationAccount",
///    "sourceAccount"
///  ],
///  "properties": {
///    "destinationAccount": {
///      "description": "Required. Destination account name. It should be full resource id if allowCrossTenantReplication set to false.",
///      "type": "string"
///    },
///    "enabledTime": {
///      "description": "Indicates when the policy is enabled on the source account.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "metrics": {
///      "description": "Optional. The object replication policy metrics feature options.",
///      "type": "object",
///      "properties": {
///        "enabled": {
///          "description": "Indicates whether object replication metrics feature is enabled for the policy.",
///          "type": "boolean"
///        }
///      }
///    },
///    "policyId": {
///      "description": "A unique id for object replication policy.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "rules": {
///      "description": "The storage account object replication rules.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ObjectReplicationPolicyRule"
///      }
///    },
///    "sourceAccount": {
///      "description": "Required. Source account name. It should be full resource id if allowCrossTenantReplication set to false.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ObjectReplicationPolicyProperties {
    ///Required. Destination account name. It should be full resource id if allowCrossTenantReplication set to false.
    #[serde(rename = "destinationAccount")]
    pub destination_account: ::std::string::String,
    ///Indicates when the policy is enabled on the source account.
    #[serde(
        rename = "enabledTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled_time: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metrics: ::std::option::Option<ObjectReplicationPolicyPropertiesMetrics>,
    ///A unique id for object replication policy.
    #[serde(
        rename = "policyId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policy_id: ::std::option::Option<::std::string::String>,
    ///The storage account object replication rules.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rules: ::std::vec::Vec<ObjectReplicationPolicyRule>,
    ///Required. Source account name. It should be full resource id if allowCrossTenantReplication set to false.
    #[serde(rename = "sourceAccount")]
    pub source_account: ::std::string::String,
}
impl ::std::convert::From<&ObjectReplicationPolicyProperties>
    for ObjectReplicationPolicyProperties
{
    fn from(value: &ObjectReplicationPolicyProperties) -> Self {
        value.clone()
    }
}
///Optional. The object replication policy metrics feature options.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Optional. The object replication policy metrics feature options.",
///  "type": "object",
///  "properties": {
///    "enabled": {
///      "description": "Indicates whether object replication metrics feature is enabled for the policy.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ObjectReplicationPolicyPropertiesMetrics {
    ///Indicates whether object replication metrics feature is enabled for the policy.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ObjectReplicationPolicyPropertiesMetrics>
    for ObjectReplicationPolicyPropertiesMetrics
{
    fn from(value: &ObjectReplicationPolicyPropertiesMetrics) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ObjectReplicationPolicyPropertiesMetrics {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
        }
    }
}
///The replication policy rule between two containers.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The replication policy rule between two containers.",
///  "required": [
///    "destinationContainer",
///    "sourceContainer"
///  ],
///  "properties": {
///    "destinationContainer": {
///      "description": "Required. Destination container name.",
///      "type": "string"
///    },
///    "filters": {
///      "$ref": "#/components/schemas/ObjectReplicationPolicyFilter"
///    },
///    "ruleId": {
///      "description": "Rule Id is auto-generated for each new rule on destination account. It is required for put policy on source account.",
///      "type": "string"
///    },
///    "sourceContainer": {
///      "description": "Required. Source container name.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ObjectReplicationPolicyRule {
    ///Required. Destination container name.
    #[serde(rename = "destinationContainer")]
    pub destination_container: ::std::string::String,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub filters: ::std::option::Option<ObjectReplicationPolicyFilter>,
    ///Rule Id is auto-generated for each new rule on destination account. It is required for put policy on source account.
    #[serde(
        rename = "ruleId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rule_id: ::std::option::Option<::std::string::String>,
    ///Required. Source container name.
    #[serde(rename = "sourceContainer")]
    pub source_container: ::std::string::String,
}
impl ::std::convert::From<&ObjectReplicationPolicyRule> for ObjectReplicationPolicyRule {
    fn from(value: &ObjectReplicationPolicyRule) -> Self {
        value.clone()
    }
}
///Storage REST API operation definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Storage REST API operation definition.",
///  "type": "object",
///  "properties": {
///    "display": {
///      "description": "Display metadata associated with the operation.",
///      "properties": {
///        "description": {
///          "description": "Description of the operation.",
///          "type": "string"
///        },
///        "operation": {
///          "description": "Type of operation: get, read, delete, etc.",
///          "type": "string"
///        },
///        "provider": {
///          "description": "Service provider: Microsoft Storage.",
///          "type": "string"
///        },
///        "resource": {
///          "description": "Resource on which the operation is performed etc.",
///          "type": "string"
///        }
///      }
///    },
///    "name": {
///      "description": "Operation name: {provider}/{resource}/{operation}",
///      "type": "string"
///    },
///    "origin": {
///      "description": "The origin of operations.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/OperationProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Operation {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub display: ::std::option::Option<OperationDisplay>,
    ///Operation name: {provider}/{resource}/{operation}
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The origin of operations.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub origin: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<OperationProperties>,
}
impl ::std::convert::From<&Operation> for Operation {
    fn from(value: &Operation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Operation {
    fn default() -> Self {
        Self {
            display: Default::default(),
            name: Default::default(),
            origin: Default::default(),
            properties: Default::default(),
        }
    }
}
///Display metadata associated with the operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Display metadata associated with the operation.",
///  "properties": {
///    "description": {
///      "description": "Description of the operation.",
///      "type": "string"
///    },
///    "operation": {
///      "description": "Type of operation: get, read, delete, etc.",
///      "type": "string"
///    },
///    "provider": {
///      "description": "Service provider: Microsoft Storage.",
///      "type": "string"
///    },
///    "resource": {
///      "description": "Resource on which the operation is performed etc.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationDisplay {
    ///Description of the operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Type of operation: get, read, delete, etc.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<::std::string::String>,
    ///Service provider: Microsoft Storage.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
    ///Resource on which the operation is performed etc.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&OperationDisplay> for OperationDisplay {
    fn from(value: &OperationDisplay) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationDisplay {
    fn default() -> Self {
        Self {
            description: Default::default(),
            operation: Default::default(),
            provider: Default::default(),
            resource: Default::default(),
        }
    }
}
///Result of the request to list Storage operations. It contains a list of operations and a URL link to get the next set of results.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Result of the request to list Storage operations. It contains a list of operations and a URL link to get the next set of results.",
///  "properties": {
///    "value": {
///      "description": "List of Storage operations supported by the Storage resource provider.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Operation"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationListResult {
    ///List of Storage operations supported by the Storage resource provider.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Operation>,
}
impl ::std::convert::From<&OperationListResult> for OperationListResult {
    fn from(value: &OperationListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationListResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///Properties of operation, include metric specifications.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of operation, include metric specifications.",
///  "properties": {
///    "serviceSpecification": {
///      "$ref": "#/components/schemas/ServiceSpecification"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationProperties {
    #[serde(
        rename = "serviceSpecification",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub service_specification: ::std::option::Option<ServiceSpecification>,
}
impl ::std::convert::From<&OperationProperties> for OperationProperties {
    fn from(value: &OperationProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationProperties {
    fn default() -> Self {
        Self {
            service_specification: Default::default(),
        }
    }
}
///`PermissionScope`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "required": [
///    "permissions",
///    "resourceName",
///    "service"
///  ],
///  "properties": {
///    "permissions": {
///      "description": "The permissions for the local user. Possible values include: Read (r), Write (w), Delete (d), List (l), Create (c), Modify Ownership (o), and Modify Permissions (p).",
///      "type": "string"
///    },
///    "resourceName": {
///      "description": "The name of resource, normally the container name or the file share name, used by the local user.",
///      "type": "string"
///    },
///    "service": {
///      "description": "The service used by the local user, e.g. blob, file.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PermissionScope {
    ///The permissions for the local user. Possible values include: Read (r), Write (w), Delete (d), List (l), Create (c), Modify Ownership (o), and Modify Permissions (p).
    pub permissions: ::std::string::String,
    ///The name of resource, normally the container name or the file share name, used by the local user.
    #[serde(rename = "resourceName")]
    pub resource_name: ::std::string::String,
    ///The service used by the local user, e.g. blob, file.
    pub service: ::std::string::String,
}
impl ::std::convert::From<&PermissionScope> for PermissionScope {
    fn from(value: &PermissionScope) -> Self {
        value.clone()
    }
}
///The Private Endpoint resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Private Endpoint resource.",
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
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
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
            type_: Default::default(),
        }
    }
}
///List of private endpoint connection associated with the specified storage account
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of private endpoint connection associated with the specified storage account",
///  "properties": {
///    "value": {
///      "description": "Array of private endpoint connections",
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
    ///Array of private endpoint connections
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<PrivateEndpointConnection>,
}
impl ::std::convert::From<&PrivateEndpointConnectionListResult>
    for PrivateEndpointConnectionListResult
{
    fn from(value: &PrivateEndpointConnectionListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PrivateEndpointConnectionListResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
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
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "Creating",
///    "Deleting",
///    "Failed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PrivateEndpointConnectionProvisioningState"
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
///    "name": "PrivateEndpointServiceConnectionStatus"
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
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "$ref": "#/components/schemas/PrivateLinkResourceProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PrivateLinkResource {
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
    pub properties: ::std::option::Option<PrivateLinkResourceProperties>,
    ///The type of the resource. E.g. "Microsoft.Compute/virtualMachines" or "Microsoft.Storage/storageAccounts"
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
///      "description": "The private link resource Private link DNS zone name.",
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
    ///The private link resource Private link DNS zone name.
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
///  "properties": {
///    "actionRequired": {
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
        rename = "actionRequired",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action_required: ::std::option::Option<::std::string::String>,
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
            action_required: Default::default(),
            description: Default::default(),
            status: Default::default(),
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
///Allow, disallow, or let Network Security Perimeter configuration to evaluate public network access to Storage Account. Value is optional but if passed in, must be 'Enabled', 'Disabled' or 'SecuredByPerimeter'.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allow, disallow, or let Network Security Perimeter configuration to evaluate public network access to Storage Account. Value is optional but if passed in, must be 'Enabled', 'Disabled' or 'SecuredByPerimeter'.",
///  "type": "string",
///  "enum": [
///    "Enabled",
///    "Disabled",
///    "SecuredByPerimeter"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "PublicNetworkAccess"
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
    SecuredByPerimeter,
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
            Self::SecuredByPerimeter => f.write_str("SecuredByPerimeter"),
        }
    }
}
impl ::std::str::FromStr for PublicNetworkAccess {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            "securedbyperimeter" => Ok(Self::SecuredByPerimeter),
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
///Resource Access Rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource Access Rule.",
///  "properties": {
///    "resourceId": {
///      "description": "Resource Id",
///      "type": "string"
///    },
///    "tenantId": {
///      "description": "Tenant Id",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceAccessRule {
    ///Resource Id
    #[serde(
        rename = "resourceId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_id: ::std::option::Option<::std::string::String>,
    ///Tenant Id
    #[serde(
        rename = "tenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tenant_id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceAccessRule> for ResourceAccessRule {
    fn from(value: &ResourceAccessRule) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceAccessRule {
    fn default() -> Self {
        Self {
            resource_id: Default::default(),
            tenant_id: Default::default(),
        }
    }
}
///The restriction because of which SKU cannot be used.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The restriction because of which SKU cannot be used.",
///  "properties": {
///    "reasonCode": {
///      "description": "The reason for the restriction. As of now this can be \"QuotaId\" or \"NotAvailableForSubscription\". Quota Id is set when the SKU has requiredQuotas parameter as the subscription does not belong to that quota. The \"NotAvailableForSubscription\" is related to capacity at DC.",
///      "type": "string",
///      "enum": [
///        "QuotaId",
///        "NotAvailableForSubscription"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ReasonCode"
///      }
///    },
///    "type": {
///      "description": "The type of restrictions. As of now only possible value for this is location.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "values": {
///      "description": "The value of restrictions. If the restriction type is set to location. This would be different locations where the SKU is restricted.",
///      "readOnly": true,
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
pub struct Restriction {
    ///The reason for the restriction. As of now this can be "QuotaId" or "NotAvailableForSubscription". Quota Id is set when the SKU has requiredQuotas parameter as the subscription does not belong to that quota. The "NotAvailableForSubscription" is related to capacity at DC.
    #[serde(
        rename = "reasonCode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reason_code: ::std::option::Option<RestrictionReasonCode>,
    ///The type of restrictions. As of now only possible value for this is location.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
    ///The value of restrictions. If the restriction type is set to location. This would be different locations where the SKU is restricted.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub values: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&Restriction> for Restriction {
    fn from(value: &Restriction) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Restriction {
    fn default() -> Self {
        Self {
            reason_code: Default::default(),
            type_: Default::default(),
            values: Default::default(),
        }
    }
}
///The reason for the restriction. As of now this can be "QuotaId" or "NotAvailableForSubscription". Quota Id is set when the SKU has requiredQuotas parameter as the subscription does not belong to that quota. The "NotAvailableForSubscription" is related to capacity at DC.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The reason for the restriction. As of now this can be \"QuotaId\" or \"NotAvailableForSubscription\". Quota Id is set when the SKU has requiredQuotas parameter as the subscription does not belong to that quota. The \"NotAvailableForSubscription\" is related to capacity at DC.",
///  "type": "string",
///  "enum": [
///    "QuotaId",
///    "NotAvailableForSubscription"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ReasonCode"
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
pub enum RestrictionReasonCode {
    QuotaId,
    NotAvailableForSubscription,
}
impl ::std::convert::From<&Self> for RestrictionReasonCode {
    fn from(value: &RestrictionReasonCode) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RestrictionReasonCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::QuotaId => f.write_str("QuotaId"),
            Self::NotAvailableForSubscription => f.write_str("NotAvailableForSubscription"),
        }
    }
}
impl ::std::str::FromStr for RestrictionReasonCode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "quotaid" => Ok(Self::QuotaId),
            "notavailableforsubscription" => Ok(Self::NotAvailableForSubscription),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RestrictionReasonCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RestrictionReasonCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RestrictionReasonCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Routing preference defines the type of network, either microsoft or internet routing to be used to deliver the user data, the default option is microsoft routing
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Routing preference defines the type of network, either microsoft or internet routing to be used to deliver the user data, the default option is microsoft routing",
///  "properties": {
///    "publishInternetEndpoints": {
///      "description": "A boolean flag which indicates whether internet routing storage endpoints are to be published",
///      "type": "boolean"
///    },
///    "publishMicrosoftEndpoints": {
///      "description": "A boolean flag which indicates whether microsoft routing storage endpoints are to be published",
///      "type": "boolean"
///    },
///    "routingChoice": {
///      "description": "Routing Choice defines the kind of network routing opted by the user.",
///      "type": "string",
///      "enum": [
///        "MicrosoftRouting",
///        "InternetRouting"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "RoutingChoice"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RoutingPreference {
    ///A boolean flag which indicates whether internet routing storage endpoints are to be published
    #[serde(
        rename = "publishInternetEndpoints",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub publish_internet_endpoints: ::std::option::Option<bool>,
    ///A boolean flag which indicates whether microsoft routing storage endpoints are to be published
    #[serde(
        rename = "publishMicrosoftEndpoints",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub publish_microsoft_endpoints: ::std::option::Option<bool>,
    ///Routing Choice defines the kind of network routing opted by the user.
    #[serde(
        rename = "routingChoice",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub routing_choice: ::std::option::Option<RoutingPreferenceRoutingChoice>,
}
impl ::std::convert::From<&RoutingPreference> for RoutingPreference {
    fn from(value: &RoutingPreference) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RoutingPreference {
    fn default() -> Self {
        Self {
            publish_internet_endpoints: Default::default(),
            publish_microsoft_endpoints: Default::default(),
            routing_choice: Default::default(),
        }
    }
}
///Routing Choice defines the kind of network routing opted by the user.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Routing Choice defines the kind of network routing opted by the user.",
///  "type": "string",
///  "enum": [
///    "MicrosoftRouting",
///    "InternetRouting"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "RoutingChoice"
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
pub enum RoutingPreferenceRoutingChoice {
    MicrosoftRouting,
    InternetRouting,
}
impl ::std::convert::From<&Self> for RoutingPreferenceRoutingChoice {
    fn from(value: &RoutingPreferenceRoutingChoice) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RoutingPreferenceRoutingChoice {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftRouting => f.write_str("MicrosoftRouting"),
            Self::InternetRouting => f.write_str("InternetRouting"),
        }
    }
}
impl ::std::str::FromStr for RoutingPreferenceRoutingChoice {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoftrouting" => Ok(Self::MicrosoftRouting),
            "internetrouting" => Ok(Self::InternetRouting),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RoutingPreferenceRoutingChoice {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for RoutingPreferenceRoutingChoice {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RoutingPreferenceRoutingChoice {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///SasPolicy assigned to the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SasPolicy assigned to the storage account.",
///  "required": [
///    "expirationAction",
///    "sasExpirationPeriod"
///  ],
///  "properties": {
///    "expirationAction": {
///      "description": "The SAS Expiration Action defines the action to be performed when sasPolicy.sasExpirationPeriod is violated. The 'Log' action can be used for audit purposes and the 'Block' action can be used to block and deny the usage of SAS tokens that do not adhere to the sas policy expiration period.",
///      "default": "Log",
///      "type": "string",
///      "enum": [
///        "Log",
///        "Block"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ExpirationAction"
///      }
///    },
///    "sasExpirationPeriod": {
///      "description": "The SAS expiration period, DD.HH:MM:SS.",
///      "examples": [
///        "1.15:59:59"
///      ],
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SasPolicy {
    ///The SAS Expiration Action defines the action to be performed when sasPolicy.sasExpirationPeriod is violated. The 'Log' action can be used for audit purposes and the 'Block' action can be used to block and deny the usage of SAS tokens that do not adhere to the sas policy expiration period.
    #[serde(rename = "expirationAction")]
    pub expiration_action: SasPolicyExpirationAction,
    ///The SAS expiration period, DD.HH:MM:SS.
    #[serde(rename = "sasExpirationPeriod")]
    pub sas_expiration_period: ::std::string::String,
}
impl ::std::convert::From<&SasPolicy> for SasPolicy {
    fn from(value: &SasPolicy) -> Self {
        value.clone()
    }
}
///The SAS Expiration Action defines the action to be performed when sasPolicy.sasExpirationPeriod is violated. The 'Log' action can be used for audit purposes and the 'Block' action can be used to block and deny the usage of SAS tokens that do not adhere to the sas policy expiration period.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SAS Expiration Action defines the action to be performed when sasPolicy.sasExpirationPeriod is violated. The 'Log' action can be used for audit purposes and the 'Block' action can be used to block and deny the usage of SAS tokens that do not adhere to the sas policy expiration period.",
///  "default": "Log",
///  "type": "string",
///  "enum": [
///    "Log",
///    "Block"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ExpirationAction"
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
pub enum SasPolicyExpirationAction {
    Log,
    Block,
}
impl ::std::convert::From<&Self> for SasPolicyExpirationAction {
    fn from(value: &SasPolicyExpirationAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SasPolicyExpirationAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Log => f.write_str("Log"),
            Self::Block => f.write_str("Block"),
        }
    }
}
impl ::std::str::FromStr for SasPolicyExpirationAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "log" => Ok(Self::Log),
            "block" => Ok(Self::Block),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SasPolicyExpirationAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SasPolicyExpirationAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SasPolicyExpirationAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for SasPolicyExpirationAction {
    fn default() -> Self {
        SasPolicyExpirationAction::Log
    }
}
///The parameters to list service SAS credentials of a specific resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters to list service SAS credentials of a specific resource.",
///  "required": [
///    "canonicalizedResource"
///  ],
///  "properties": {
///    "canonicalizedResource": {
///      "description": "The canonical path to the signed resource.",
///      "type": "string"
///    },
///    "endPk": {
///      "description": "The end of partition key.",
///      "type": "string",
///      "x-ms-client-name": "PartitionKeyEnd"
///    },
///    "endRk": {
///      "description": "The end of row key.",
///      "type": "string",
///      "x-ms-client-name": "RowKeyEnd"
///    },
///    "keyToSign": {
///      "description": "The key to sign the account SAS token with.",
///      "type": "string"
///    },
///    "rscc": {
///      "description": "The response header override for cache control.",
///      "type": "string",
///      "x-ms-client-name": "CacheControl"
///    },
///    "rscd": {
///      "description": "The response header override for content disposition.",
///      "type": "string",
///      "x-ms-client-name": "ContentDisposition"
///    },
///    "rsce": {
///      "description": "The response header override for content encoding.",
///      "type": "string",
///      "x-ms-client-name": "ContentEncoding"
///    },
///    "rscl": {
///      "description": "The response header override for content language.",
///      "type": "string",
///      "x-ms-client-name": "ContentLanguage"
///    },
///    "rsct": {
///      "description": "The response header override for content type.",
///      "type": "string",
///      "x-ms-client-name": "ContentType"
///    },
///    "signedExpiry": {
///      "description": "The time at which the shared access signature becomes invalid.",
///      "type": "string",
///      "x-ms-client-name": "SharedAccessExpiryTime"
///    },
///    "signedIdentifier": {
///      "description": "A unique value up to 64 characters in length that correlates to an access policy specified for the container, queue, or table.",
///      "type": "string",
///      "maxLength": 64,
///      "x-ms-client-name": "Identifier"
///    },
///    "signedIp": {
///      "description": "An IP address or a range of IP addresses from which to accept requests.",
///      "type": "string",
///      "x-ms-client-name": "IPAddressOrRange"
///    },
///    "signedPermission": {
///      "description": "The signed permissions for the service SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).",
///      "type": "string",
///      "enum": [
///        "r",
///        "d",
///        "w",
///        "l",
///        "a",
///        "c",
///        "u",
///        "p"
///      ],
///      "x-ms-client-name": "Permissions",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Permissions"
///      }
///    },
///    "signedProtocol": {
///      "description": "The protocol permitted for a request made with the account SAS.",
///      "type": "string",
///      "enum": [
///        "https,http",
///        "https"
///      ],
///      "x-ms-client-name": "Protocols",
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "HttpProtocol"
///      }
///    },
///    "signedResource": {
///      "description": "The signed services accessible with the service SAS. Possible values include: Blob (b), Container (c), File (f), Share (s).",
///      "type": "string",
///      "enum": [
///        "b",
///        "c",
///        "f",
///        "s"
///      ],
///      "x-ms-client-name": "Resource",
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "signedResource"
///      }
///    },
///    "signedStart": {
///      "description": "The time at which the SAS becomes valid.",
///      "type": "string",
///      "x-ms-client-name": "SharedAccessStartTime"
///    },
///    "startPk": {
///      "description": "The start of partition key.",
///      "type": "string",
///      "x-ms-client-name": "PartitionKeyStart"
///    },
///    "startRk": {
///      "description": "The start of row key.",
///      "type": "string",
///      "x-ms-client-name": "RowKeyStart"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceSasParameters {
    ///The canonical path to the signed resource.
    #[serde(rename = "canonicalizedResource")]
    pub canonicalized_resource: ::std::string::String,
    ///The end of partition key.
    #[serde(
        rename = "endPk",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub end_pk: ::std::option::Option<::std::string::String>,
    ///The end of row key.
    #[serde(
        rename = "endRk",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub end_rk: ::std::option::Option<::std::string::String>,
    ///The key to sign the account SAS token with.
    #[serde(
        rename = "keyToSign",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_to_sign: ::std::option::Option<::std::string::String>,
    ///The response header override for cache control.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rscc: ::std::option::Option<::std::string::String>,
    ///The response header override for content disposition.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rscd: ::std::option::Option<::std::string::String>,
    ///The response header override for content encoding.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rsce: ::std::option::Option<::std::string::String>,
    ///The response header override for content language.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rscl: ::std::option::Option<::std::string::String>,
    ///The response header override for content type.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub rsct: ::std::option::Option<::std::string::String>,
    ///The time at which the shared access signature becomes invalid.
    #[serde(
        rename = "signedExpiry",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_expiry: ::std::option::Option<::std::string::String>,
    ///A unique value up to 64 characters in length that correlates to an access policy specified for the container, queue, or table.
    #[serde(
        rename = "signedIdentifier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_identifier: ::std::option::Option<ServiceSasParametersSignedIdentifier>,
    ///An IP address or a range of IP addresses from which to accept requests.
    #[serde(
        rename = "signedIp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_ip: ::std::option::Option<::std::string::String>,
    ///The signed permissions for the service SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).
    #[serde(
        rename = "signedPermission",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_permission: ::std::option::Option<ServiceSasParametersSignedPermission>,
    ///The protocol permitted for a request made with the account SAS.
    #[serde(
        rename = "signedProtocol",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_protocol: ::std::option::Option<ServiceSasParametersSignedProtocol>,
    ///The signed services accessible with the service SAS. Possible values include: Blob (b), Container (c), File (f), Share (s).
    #[serde(
        rename = "signedResource",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_resource: ::std::option::Option<ServiceSasParametersSignedResource>,
    ///The time at which the SAS becomes valid.
    #[serde(
        rename = "signedStart",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub signed_start: ::std::option::Option<::std::string::String>,
    ///The start of partition key.
    #[serde(
        rename = "startPk",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub start_pk: ::std::option::Option<::std::string::String>,
    ///The start of row key.
    #[serde(
        rename = "startRk",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub start_rk: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ServiceSasParameters> for ServiceSasParameters {
    fn from(value: &ServiceSasParameters) -> Self {
        value.clone()
    }
}
///A unique value up to 64 characters in length that correlates to an access policy specified for the container, queue, or table.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A unique value up to 64 characters in length that correlates to an access policy specified for the container, queue, or table.",
///  "type": "string",
///  "maxLength": 64,
///  "x-ms-client-name": "Identifier"
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ServiceSasParametersSignedIdentifier(::std::string::String);
impl ::std::ops::Deref for ServiceSasParametersSignedIdentifier {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ServiceSasParametersSignedIdentifier> for ::std::string::String {
    fn from(value: ServiceSasParametersSignedIdentifier) -> Self {
        value.0
    }
}
impl ::std::convert::From<&ServiceSasParametersSignedIdentifier>
    for ServiceSasParametersSignedIdentifier
{
    fn from(value: &ServiceSasParametersSignedIdentifier) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for ServiceSasParametersSignedIdentifier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 64usize {
            return Err("longer than 64 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ServiceSasParametersSignedIdentifier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ServiceSasParametersSignedIdentifier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ServiceSasParametersSignedIdentifier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ServiceSasParametersSignedIdentifier {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
///The signed permissions for the service SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The signed permissions for the service SAS. Possible values include: Read (r), Write (w), Delete (d), List (l), Add (a), Create (c), Update (u) and Process (p).",
///  "type": "string",
///  "enum": [
///    "r",
///    "d",
///    "w",
///    "l",
///    "a",
///    "c",
///    "u",
///    "p"
///  ],
///  "x-ms-client-name": "Permissions",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Permissions"
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
pub enum ServiceSasParametersSignedPermission {
    #[serde(rename = "r")]
    R,
    #[serde(rename = "d")]
    D,
    #[serde(rename = "w")]
    W,
    #[serde(rename = "l")]
    L,
    #[serde(rename = "a")]
    A,
    #[serde(rename = "c")]
    C,
    #[serde(rename = "u")]
    U,
    #[serde(rename = "p")]
    P,
}
impl ::std::convert::From<&Self> for ServiceSasParametersSignedPermission {
    fn from(value: &ServiceSasParametersSignedPermission) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ServiceSasParametersSignedPermission {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::R => f.write_str("r"),
            Self::D => f.write_str("d"),
            Self::W => f.write_str("w"),
            Self::L => f.write_str("l"),
            Self::A => f.write_str("a"),
            Self::C => f.write_str("c"),
            Self::U => f.write_str("u"),
            Self::P => f.write_str("p"),
        }
    }
}
impl ::std::str::FromStr for ServiceSasParametersSignedPermission {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "r" => Ok(Self::R),
            "d" => Ok(Self::D),
            "w" => Ok(Self::W),
            "l" => Ok(Self::L),
            "a" => Ok(Self::A),
            "c" => Ok(Self::C),
            "u" => Ok(Self::U),
            "p" => Ok(Self::P),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ServiceSasParametersSignedPermission {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ServiceSasParametersSignedPermission {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ServiceSasParametersSignedPermission {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The protocol permitted for a request made with the account SAS.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The protocol permitted for a request made with the account SAS.",
///  "type": "string",
///  "enum": [
///    "https,http",
///    "https"
///  ],
///  "x-ms-client-name": "Protocols",
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "HttpProtocol"
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
pub enum ServiceSasParametersSignedProtocol {
    #[serde(rename = "https,http")]
    HttpsHttp,
    #[serde(rename = "https")]
    Https,
}
impl ::std::convert::From<&Self> for ServiceSasParametersSignedProtocol {
    fn from(value: &ServiceSasParametersSignedProtocol) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ServiceSasParametersSignedProtocol {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::HttpsHttp => f.write_str("https,http"),
            Self::Https => f.write_str("https"),
        }
    }
}
impl ::std::str::FromStr for ServiceSasParametersSignedProtocol {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "https,http" => Ok(Self::HttpsHttp),
            "https" => Ok(Self::Https),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ServiceSasParametersSignedProtocol {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ServiceSasParametersSignedProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ServiceSasParametersSignedProtocol {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The signed services accessible with the service SAS. Possible values include: Blob (b), Container (c), File (f), Share (s).
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The signed services accessible with the service SAS. Possible values include: Blob (b), Container (c), File (f), Share (s).",
///  "type": "string",
///  "enum": [
///    "b",
///    "c",
///    "f",
///    "s"
///  ],
///  "x-ms-client-name": "Resource",
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "signedResource"
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
pub enum ServiceSasParametersSignedResource {
    #[serde(rename = "b")]
    B,
    #[serde(rename = "c")]
    C,
    #[serde(rename = "f")]
    F,
    #[serde(rename = "s")]
    S,
}
impl ::std::convert::From<&Self> for ServiceSasParametersSignedResource {
    fn from(value: &ServiceSasParametersSignedResource) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ServiceSasParametersSignedResource {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::B => f.write_str("b"),
            Self::C => f.write_str("c"),
            Self::F => f.write_str("f"),
            Self::S => f.write_str("s"),
        }
    }
}
impl ::std::str::FromStr for ServiceSasParametersSignedResource {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "b" => Ok(Self::B),
            "c" => Ok(Self::C),
            "f" => Ok(Self::F),
            "s" => Ok(Self::S),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ServiceSasParametersSignedResource {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ServiceSasParametersSignedResource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ServiceSasParametersSignedResource {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///One property of operation, include metric specifications.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "One property of operation, include metric specifications.",
///  "properties": {
///    "metricSpecifications": {
///      "description": "Metric specifications of operation.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/MetricSpecification"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ServiceSpecification {
    ///Metric specifications of operation.
    #[serde(
        rename = "metricSpecifications",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metric_specifications: ::std::vec::Vec<MetricSpecification>,
}
impl ::std::convert::From<&ServiceSpecification> for ServiceSpecification {
    fn from(value: &ServiceSpecification) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ServiceSpecification {
    fn default() -> Self {
        Self {
            metric_specifications: Default::default(),
        }
    }
}
///Auto generated by the server for SMB authentication.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Auto generated by the server for SMB authentication.",
///  "readOnly": true,
///  "type": "string"
///}
/// ```
/// </details>
#[derive(
    ::serde::Deserialize, ::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd,
)]
#[serde(transparent)]
pub struct SharedKey(pub ::std::string::String);
impl ::std::ops::Deref for SharedKey {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<SharedKey> for ::std::string::String {
    fn from(value: SharedKey) -> Self {
        value.0
    }
}
impl ::std::convert::From<&SharedKey> for SharedKey {
    fn from(value: &SharedKey) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::string::String> for SharedKey {
    fn from(value: ::std::string::String) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for SharedKey {
    type Err = ::std::convert::Infallible;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.to_string()))
    }
}
impl ::std::fmt::Display for SharedKey {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
///The SKU of the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU of the storage account.",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "name": {
///      "$ref": "#/components/schemas/SkuName"
///    },
///    "tier": {
///      "$ref": "#/components/schemas/Tier"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Sku {
    pub name: SkuName,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<Tier>,
}
impl ::std::convert::From<&Sku> for Sku {
    fn from(value: &Sku) -> Self {
        value.clone()
    }
}
///The capability information in the specified SKU, including file encryption, network ACLs, change notification, etc.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The capability information in the specified SKU, including file encryption, network ACLs, change notification, etc.",
///  "properties": {
///    "name": {
///      "description": "The name of capability, The capability information in the specified SKU, including file encryption, network ACLs, change notification, etc.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "A string value to indicate states of given capability. Possibly 'true' or 'false'.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SkuCapability {
    ///The name of capability, The capability information in the specified SKU, including file encryption, network ACLs, change notification, etc.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///A string value to indicate states of given capability. Possibly 'true' or 'false'.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SkuCapability> for SkuCapability {
    fn from(value: &SkuCapability) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SkuCapability {
    fn default() -> Self {
        Self {
            name: Default::default(),
            value: Default::default(),
        }
    }
}
///Storage SKU and its properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Storage SKU and its properties",
///  "required": [
///    "name"
///  ],
///  "properties": {
///    "capabilities": {
///      "description": "The capability information in the specified SKU, including file encryption, network ACLs, change notification, etc.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SKUCapability"
///      }
///    },
///    "kind": {
///      "description": "Indicates the type of storage account.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Storage",
///        "StorageV2",
///        "BlobStorage",
///        "FileStorage",
///        "BlockBlobStorage"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Kind"
///      }
///    },
///    "locations": {
///      "description": "The set of locations that the SKU is available. This will be supported and registered Azure Geo Regions (e.g. West US, East US, Southeast Asia, etc.).",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "name": {
///      "$ref": "#/components/schemas/SkuName"
///    },
///    "resourceType": {
///      "description": "The type of the resource, usually it is 'storageAccounts'.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "restrictions": {
///      "description": "The restrictions because of which SKU cannot be used. This is empty if there are no restrictions.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Restriction"
///      }
///    },
///    "tier": {
///      "$ref": "#/components/schemas/Tier"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SkuInformation {
    ///The capability information in the specified SKU, including file encryption, network ACLs, change notification, etc.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub capabilities: ::std::vec::Vec<SkuCapability>,
    ///Indicates the type of storage account.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<SkuInformationKind>,
    ///The set of locations that the SKU is available. This will be supported and registered Azure Geo Regions (e.g. West US, East US, Southeast Asia, etc.).
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub locations: ::std::vec::Vec<::std::string::String>,
    pub name: SkuName,
    ///The type of the resource, usually it is 'storageAccounts'.
    #[serde(
        rename = "resourceType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_type: ::std::option::Option<::std::string::String>,
    ///The restrictions because of which SKU cannot be used. This is empty if there are no restrictions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub restrictions: ::std::vec::Vec<Restriction>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<Tier>,
}
impl ::std::convert::From<&SkuInformation> for SkuInformation {
    fn from(value: &SkuInformation) -> Self {
        value.clone()
    }
}
///Indicates the type of storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Indicates the type of storage account.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Storage",
///    "StorageV2",
///    "BlobStorage",
///    "FileStorage",
///    "BlockBlobStorage"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Kind"
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
pub enum SkuInformationKind {
    Storage,
    StorageV2,
    BlobStorage,
    FileStorage,
    BlockBlobStorage,
}
impl ::std::convert::From<&Self> for SkuInformationKind {
    fn from(value: &SkuInformationKind) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SkuInformationKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Storage => f.write_str("Storage"),
            Self::StorageV2 => f.write_str("StorageV2"),
            Self::BlobStorage => f.write_str("BlobStorage"),
            Self::FileStorage => f.write_str("FileStorage"),
            Self::BlockBlobStorage => f.write_str("BlockBlobStorage"),
        }
    }
}
impl ::std::str::FromStr for SkuInformationKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "storage" => Ok(Self::Storage),
            "storagev2" => Ok(Self::StorageV2),
            "blobstorage" => Ok(Self::BlobStorage),
            "filestorage" => Ok(Self::FileStorage),
            "blockblobstorage" => Ok(Self::BlockBlobStorage),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SkuInformationKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SkuInformationKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SkuInformationKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The SKU name. Required for account creation; optional for update. Note that in older versions, SKU name was called accountType.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU name. Required for account creation; optional for update. Note that in older versions, SKU name was called accountType.",
///  "type": "string",
///  "enum": [
///    "Standard_LRS",
///    "Standard_GRS",
///    "Standard_RAGRS",
///    "Standard_ZRS",
///    "Premium_LRS",
///    "Premium_ZRS",
///    "Standard_GZRS",
///    "Standard_RAGZRS",
///    "StandardV2_LRS",
///    "StandardV2_GRS",
///    "StandardV2_ZRS",
///    "StandardV2_GZRS",
///    "PremiumV2_LRS",
///    "PremiumV2_ZRS"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SkuName"
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
pub enum SkuName {
    #[serde(rename = "Standard_LRS")]
    StandardLrs,
    #[serde(rename = "Standard_GRS")]
    StandardGrs,
    #[serde(rename = "Standard_RAGRS")]
    StandardRagrs,
    #[serde(rename = "Standard_ZRS")]
    StandardZrs,
    #[serde(rename = "Premium_LRS")]
    PremiumLrs,
    #[serde(rename = "Premium_ZRS")]
    PremiumZrs,
    #[serde(rename = "Standard_GZRS")]
    StandardGzrs,
    #[serde(rename = "Standard_RAGZRS")]
    StandardRagzrs,
    #[serde(rename = "StandardV2_LRS")]
    StandardV2Lrs,
    #[serde(rename = "StandardV2_GRS")]
    StandardV2Grs,
    #[serde(rename = "StandardV2_ZRS")]
    StandardV2Zrs,
    #[serde(rename = "StandardV2_GZRS")]
    StandardV2Gzrs,
    #[serde(rename = "PremiumV2_LRS")]
    PremiumV2Lrs,
    #[serde(rename = "PremiumV2_ZRS")]
    PremiumV2Zrs,
}
impl ::std::convert::From<&Self> for SkuName {
    fn from(value: &SkuName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for SkuName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::StandardLrs => f.write_str("Standard_LRS"),
            Self::StandardGrs => f.write_str("Standard_GRS"),
            Self::StandardRagrs => f.write_str("Standard_RAGRS"),
            Self::StandardZrs => f.write_str("Standard_ZRS"),
            Self::PremiumLrs => f.write_str("Premium_LRS"),
            Self::PremiumZrs => f.write_str("Premium_ZRS"),
            Self::StandardGzrs => f.write_str("Standard_GZRS"),
            Self::StandardRagzrs => f.write_str("Standard_RAGZRS"),
            Self::StandardV2Lrs => f.write_str("StandardV2_LRS"),
            Self::StandardV2Grs => f.write_str("StandardV2_GRS"),
            Self::StandardV2Zrs => f.write_str("StandardV2_ZRS"),
            Self::StandardV2Gzrs => f.write_str("StandardV2_GZRS"),
            Self::PremiumV2Lrs => f.write_str("PremiumV2_LRS"),
            Self::PremiumV2Zrs => f.write_str("PremiumV2_ZRS"),
        }
    }
}
impl ::std::str::FromStr for SkuName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard_lrs" => Ok(Self::StandardLrs),
            "standard_grs" => Ok(Self::StandardGrs),
            "standard_ragrs" => Ok(Self::StandardRagrs),
            "standard_zrs" => Ok(Self::StandardZrs),
            "premium_lrs" => Ok(Self::PremiumLrs),
            "premium_zrs" => Ok(Self::PremiumZrs),
            "standard_gzrs" => Ok(Self::StandardGzrs),
            "standard_ragzrs" => Ok(Self::StandardRagzrs),
            "standardv2_lrs" => Ok(Self::StandardV2Lrs),
            "standardv2_grs" => Ok(Self::StandardV2Grs),
            "standardv2_zrs" => Ok(Self::StandardV2Zrs),
            "standardv2_gzrs" => Ok(Self::StandardV2Gzrs),
            "premiumv2_lrs" => Ok(Self::PremiumV2Lrs),
            "premiumv2_zrs" => Ok(Self::PremiumV2Zrs),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for SkuName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for SkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for SkuName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Optional, local user ssh authorized keys for SFTP.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Optional, local user ssh authorized keys for SFTP.",
///  "type": "array",
///  "items": {
///    "$ref": "#/components/schemas/SshPublicKey"
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct SshAuthorizedKeys(pub ::std::vec::Vec<SshPublicKey>);
impl ::std::ops::Deref for SshAuthorizedKeys {
    type Target = ::std::vec::Vec<SshPublicKey>;
    fn deref(&self) -> &::std::vec::Vec<SshPublicKey> {
        &self.0
    }
}
impl ::std::convert::From<SshAuthorizedKeys> for ::std::vec::Vec<SshPublicKey> {
    fn from(value: SshAuthorizedKeys) -> Self {
        value.0
    }
}
impl ::std::convert::From<&SshAuthorizedKeys> for SshAuthorizedKeys {
    fn from(value: &SshAuthorizedKeys) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::vec::Vec<SshPublicKey>> for SshAuthorizedKeys {
    fn from(value: ::std::vec::Vec<SshPublicKey>) -> Self {
        Self(value)
    }
}
///`SshPublicKey`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "description": {
///      "description": "Optional. It is used to store the function/usage of the key",
///      "type": "string"
///    },
///    "key": {
///      "description": "Ssh public key base64 encoded. The format should be: '<keyType> <keyData>', e.g. ssh-rsa AAAABBBB",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SshPublicKey {
    ///Optional. It is used to store the function/usage of the key
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Ssh public key base64 encoded. The format should be: '<keyType> <keyData>', e.g. ssh-rsa AAAABBBB
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SshPublicKey> for SshPublicKey {
    fn from(value: &SshPublicKey) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SshPublicKey {
    fn default() -> Self {
        Self {
            description: Default::default(),
            key: Default::default(),
        }
    }
}
///The storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The storage account.",
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
///      "$ref": "#/components/schemas/Identity"
///    },
///    "kind": {
///      "description": "Gets the Kind.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Storage",
///        "StorageV2",
///        "BlobStorage",
///        "FileStorage",
///        "BlockBlobStorage"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Kind"
///      }
///    },
///    "properties": {
///      "$ref": "#/components/schemas/StorageAccountProperties"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccount {
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
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<Identity>,
    ///Gets the Kind.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<StorageAccountKind>,
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
    pub properties: ::std::option::Option<StorageAccountProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
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
impl ::std::convert::From<&StorageAccount> for StorageAccount {
    fn from(value: &StorageAccount) -> Self {
        value.clone()
    }
}
///The parameters used to check the availability of the storage account name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used to check the availability of the storage account name.",
///  "required": [
///    "name",
///    "type"
///  ],
///  "properties": {
///    "name": {
///      "description": "The storage account name.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of resource, Microsoft.Storage/storageAccounts",
///      "type": "string",
///      "enum": [
///        "Microsoft.Storage/storageAccounts"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "Type"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountCheckNameAvailabilityParameters {
    ///The storage account name.
    pub name: ::std::string::String,
    ///The type of resource, Microsoft.Storage/storageAccounts
    #[serde(rename = "type")]
    pub type_: StorageAccountCheckNameAvailabilityParametersType,
}
impl ::std::convert::From<&StorageAccountCheckNameAvailabilityParameters>
    for StorageAccountCheckNameAvailabilityParameters
{
    fn from(value: &StorageAccountCheckNameAvailabilityParameters) -> Self {
        value.clone()
    }
}
///The type of resource, Microsoft.Storage/storageAccounts
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of resource, Microsoft.Storage/storageAccounts",
///  "type": "string",
///  "enum": [
///    "Microsoft.Storage/storageAccounts"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
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
pub enum StorageAccountCheckNameAvailabilityParametersType {
    #[serde(rename = "Microsoft.Storage/storageAccounts")]
    MicrosoftStorageStorageAccounts,
}
impl ::std::convert::From<&Self> for StorageAccountCheckNameAvailabilityParametersType {
    fn from(value: &StorageAccountCheckNameAvailabilityParametersType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountCheckNameAvailabilityParametersType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::MicrosoftStorageStorageAccounts => {
                f.write_str("Microsoft.Storage/storageAccounts")
            }
        }
    }
}
impl ::std::str::FromStr for StorageAccountCheckNameAvailabilityParametersType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "microsoft.storage/storageaccounts" => Ok(Self::MicrosoftStorageStorageAccounts),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountCheckNameAvailabilityParametersType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountCheckNameAvailabilityParametersType
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountCheckNameAvailabilityParametersType
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters used when creating a storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used when creating a storage account.",
///  "required": [
///    "kind",
///    "location",
///    "sku"
///  ],
///  "properties": {
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "identity": {
///      "$ref": "#/components/schemas/Identity"
///    },
///    "kind": {
///      "description": "Required. Indicates the type of storage account.",
///      "type": "string",
///      "enum": [
///        "Storage",
///        "StorageV2",
///        "BlobStorage",
///        "FileStorage",
///        "BlockBlobStorage"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Kind"
///      }
///    },
///    "location": {
///      "description": "Required. Gets or sets the location of the resource. This will be one of the supported and registered Azure Geo Regions (e.g. West US, East US, Southeast Asia, etc.). The geo region of a resource cannot be changed once it is created, but if an identical geo region is specified on update, the request will succeed.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/StorageAccountPropertiesCreateParameters"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    },
///    "tags": {
///      "description": "Gets or sets a list of key value pairs that describe the resource. These tags can be used for viewing and grouping this resource (across resource groups). A maximum of 15 tags can be provided for a resource. Each tag must have a key with a length no greater than 128 characters and a value with a length no greater than 256 characters.",
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
pub struct StorageAccountCreateParameters {
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<Identity>,
    ///Required. Indicates the type of storage account.
    pub kind: StorageAccountCreateParametersKind,
    ///Required. Gets or sets the location of the resource. This will be one of the supported and registered Azure Geo Regions (e.g. West US, East US, Southeast Asia, etc.). The geo region of a resource cannot be changed once it is created, but if an identical geo region is specified on update, the request will succeed.
    pub location: ::std::string::String,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<StorageAccountPropertiesCreateParameters>,
    pub sku: Sku,
    ///Gets or sets a list of key value pairs that describe the resource. These tags can be used for viewing and grouping this resource (across resource groups). A maximum of 15 tags can be provided for a resource. Each tag must have a key with a length no greater than 128 characters and a value with a length no greater than 256 characters.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&StorageAccountCreateParameters> for StorageAccountCreateParameters {
    fn from(value: &StorageAccountCreateParameters) -> Self {
        value.clone()
    }
}
///Required. Indicates the type of storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Required. Indicates the type of storage account.",
///  "type": "string",
///  "enum": [
///    "Storage",
///    "StorageV2",
///    "BlobStorage",
///    "FileStorage",
///    "BlockBlobStorage"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Kind"
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
pub enum StorageAccountCreateParametersKind {
    Storage,
    StorageV2,
    BlobStorage,
    FileStorage,
    BlockBlobStorage,
}
impl ::std::convert::From<&Self> for StorageAccountCreateParametersKind {
    fn from(value: &StorageAccountCreateParametersKind) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountCreateParametersKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Storage => f.write_str("Storage"),
            Self::StorageV2 => f.write_str("StorageV2"),
            Self::BlobStorage => f.write_str("BlobStorage"),
            Self::FileStorage => f.write_str("FileStorage"),
            Self::BlockBlobStorage => f.write_str("BlockBlobStorage"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountCreateParametersKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "storage" => Ok(Self::Storage),
            "storagev2" => Ok(Self::StorageV2),
            "blobstorage" => Ok(Self::BlobStorage),
            "filestorage" => Ok(Self::FileStorage),
            "blockblobstorage" => Ok(Self::BlockBlobStorage),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountCreateParametersKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountCreateParametersKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountCreateParametersKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The URIs that are used to perform a retrieval of a public blob, file, web or dfs object via a internet routing endpoint.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The URIs that are used to perform a retrieval of a public blob, file, web or dfs object via a internet routing endpoint.",
///  "properties": {
///    "blob": {
///      "description": "Gets the blob endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "dfs": {
///      "description": "Gets the dfs endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "file": {
///      "description": "Gets the file endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "web": {
///      "description": "Gets the web endpoint.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountInternetEndpoints {
    ///Gets the blob endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob: ::std::option::Option<::std::string::String>,
    ///Gets the dfs endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dfs: ::std::option::Option<::std::string::String>,
    ///Gets the file endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub file: ::std::option::Option<::std::string::String>,
    ///Gets the web endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub web: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&StorageAccountInternetEndpoints> for StorageAccountInternetEndpoints {
    fn from(value: &StorageAccountInternetEndpoints) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountInternetEndpoints {
    fn default() -> Self {
        Self {
            blob: Default::default(),
            dfs: Default::default(),
            file: Default::default(),
            web: Default::default(),
        }
    }
}
///An access key for the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An access key for the storage account.",
///  "properties": {
///    "creationTime": {
///      "description": "Creation time of the key, in round trip date format.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "keyName": {
///      "description": "Name of the key.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "permissions": {
///      "description": "Permissions for the key -- read-only or full permissions.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Read",
///        "Full"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "KeyPermission"
///      }
///    },
///    "value": {
///      "description": "Base 64-encoded value of the key.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountKey {
    ///Creation time of the key, in round trip date format.
    #[serde(
        rename = "creationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_time: ::std::option::Option<::std::string::String>,
    ///Name of the key.
    #[serde(
        rename = "keyName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_name: ::std::option::Option<::std::string::String>,
    ///Permissions for the key -- read-only or full permissions.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub permissions: ::std::option::Option<StorageAccountKeyPermissions>,
    ///Base 64-encoded value of the key.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&StorageAccountKey> for StorageAccountKey {
    fn from(value: &StorageAccountKey) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountKey {
    fn default() -> Self {
        Self {
            creation_time: Default::default(),
            key_name: Default::default(),
            permissions: Default::default(),
            value: Default::default(),
        }
    }
}
///Permissions for the key -- read-only or full permissions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Permissions for the key -- read-only or full permissions.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Read",
///    "Full"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "KeyPermission"
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
pub enum StorageAccountKeyPermissions {
    Read,
    Full,
}
impl ::std::convert::From<&Self> for StorageAccountKeyPermissions {
    fn from(value: &StorageAccountKeyPermissions) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountKeyPermissions {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Read => f.write_str("Read"),
            Self::Full => f.write_str("Full"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountKeyPermissions {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "read" => Ok(Self::Read),
            "full" => Ok(Self::Full),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountKeyPermissions {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountKeyPermissions {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountKeyPermissions {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Gets the Kind.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the Kind.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Storage",
///    "StorageV2",
///    "BlobStorage",
///    "FileStorage",
///    "BlockBlobStorage"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Kind"
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
pub enum StorageAccountKind {
    Storage,
    StorageV2,
    BlobStorage,
    FileStorage,
    BlockBlobStorage,
}
impl ::std::convert::From<&Self> for StorageAccountKind {
    fn from(value: &StorageAccountKind) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Storage => f.write_str("Storage"),
            Self::StorageV2 => f.write_str("StorageV2"),
            Self::BlobStorage => f.write_str("BlobStorage"),
            Self::FileStorage => f.write_str("FileStorage"),
            Self::BlockBlobStorage => f.write_str("BlockBlobStorage"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "storage" => Ok(Self::Storage),
            "storagev2" => Ok(Self::StorageV2),
            "blobstorage" => Ok(Self::BlobStorage),
            "filestorage" => Ok(Self::FileStorage),
            "blockblobstorage" => Ok(Self::BlockBlobStorage),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The response from the ListKeys operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the ListKeys operation.",
///  "properties": {
///    "keys": {
///      "description": "Gets the list of storage account keys and their properties for the specified storage account.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/StorageAccountKey"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountListKeysResult {
    ///Gets the list of storage account keys and their properties for the specified storage account.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keys: ::std::vec::Vec<StorageAccountKey>,
}
impl ::std::convert::From<&StorageAccountListKeysResult> for StorageAccountListKeysResult {
    fn from(value: &StorageAccountListKeysResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountListKeysResult {
    fn default() -> Self {
        Self {
            keys: Default::default(),
        }
    }
}
///The response from the List Storage Accounts operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the List Storage Accounts operation.",
///  "properties": {
///    "nextLink": {
///      "description": "Request URL that can be used to query next page of storage accounts. Returned when total number of requested storage accounts exceed maximum page size.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "Gets the list of storage accounts and their properties.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/StorageAccount"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountListResult {
    ///Request URL that can be used to query next page of storage accounts. Returned when total number of requested storage accounts exceed maximum page size.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Gets the list of storage accounts and their properties.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<StorageAccount>,
}
impl ::std::convert::From<&StorageAccountListResult> for StorageAccountListResult {
    fn from(value: &StorageAccountListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The URIs that are used to perform a retrieval of a public blob, queue, table, web or dfs object via a microsoft routing endpoint.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The URIs that are used to perform a retrieval of a public blob, queue, table, web or dfs object via a microsoft routing endpoint.",
///  "properties": {
///    "blob": {
///      "description": "Gets the blob endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "dfs": {
///      "description": "Gets the dfs endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "file": {
///      "description": "Gets the file endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "queue": {
///      "description": "Gets the queue endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "table": {
///      "description": "Gets the table endpoint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "web": {
///      "description": "Gets the web endpoint.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountMicrosoftEndpoints {
    ///Gets the blob endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob: ::std::option::Option<::std::string::String>,
    ///Gets the dfs endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dfs: ::std::option::Option<::std::string::String>,
    ///Gets the file endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub file: ::std::option::Option<::std::string::String>,
    ///Gets the queue endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub queue: ::std::option::Option<::std::string::String>,
    ///Gets the table endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub table: ::std::option::Option<::std::string::String>,
    ///Gets the web endpoint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub web: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&StorageAccountMicrosoftEndpoints> for StorageAccountMicrosoftEndpoints {
    fn from(value: &StorageAccountMicrosoftEndpoints) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountMicrosoftEndpoints {
    fn default() -> Self {
        Self {
            blob: Default::default(),
            dfs: Default::default(),
            file: Default::default(),
            queue: Default::default(),
            table: Default::default(),
            web: Default::default(),
        }
    }
}
///The parameters or status associated with an ongoing or enqueued storage account migration in order to update its current SKU or region.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters or status associated with an ongoing or enqueued storage account migration in order to update its current SKU or region.",
///  "type": "object",
///  "required": [
///    "properties"
///  ],
///  "properties": {
///    "id": {
///      "description": "Migration Resource Id",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "current value is 'default' for customer initiated migration",
///      "type": "string"
///    },
///    "properties": {
///      "description": "The properties of a storage account’s ongoing or enqueued migration.",
///      "type": "object",
///      "required": [
///        "targetSkuName"
///      ],
///      "properties": {
///        "migrationFailedDetailedReason": {
///          "description": "Reason for migration failure",
///          "readOnly": true,
///          "type": "string"
///        },
///        "migrationFailedReason": {
///          "description": "Error code for migration failure",
///          "readOnly": true,
///          "type": "string"
///        },
///        "migrationStatus": {
///          "description": "Current status of migration",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "Invalid",
///            "SubmittedForConversion",
///            "InProgress",
///            "Complete",
///            "Failed"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "migrationStatus"
///          }
///        },
///        "targetSkuName": {
///          "$ref": "#/components/schemas/SkuName"
///        }
///      },
///      "x-ms-client-flatten": true,
///      "x-ms-client-name": "StorageAccountMigrationDetails"
///    },
///    "type": {
///      "description": "SrpAccountMigrationType in ARM contract which is 'accountMigrations'",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountMigration {
    ///Migration Resource Id
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///current value is 'default' for customer initiated migration
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    pub properties: StorageAccountMigrationProperties,
    ///SrpAccountMigrationType in ARM contract which is 'accountMigrations'
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&StorageAccountMigration> for StorageAccountMigration {
    fn from(value: &StorageAccountMigration) -> Self {
        value.clone()
    }
}
///The properties of a storage account’s ongoing or enqueued migration.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a storage account’s ongoing or enqueued migration.",
///  "type": "object",
///  "required": [
///    "targetSkuName"
///  ],
///  "properties": {
///    "migrationFailedDetailedReason": {
///      "description": "Reason for migration failure",
///      "readOnly": true,
///      "type": "string"
///    },
///    "migrationFailedReason": {
///      "description": "Error code for migration failure",
///      "readOnly": true,
///      "type": "string"
///    },
///    "migrationStatus": {
///      "description": "Current status of migration",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Invalid",
///        "SubmittedForConversion",
///        "InProgress",
///        "Complete",
///        "Failed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "migrationStatus"
///      }
///    },
///    "targetSkuName": {
///      "$ref": "#/components/schemas/SkuName"
///    }
///  },
///  "x-ms-client-flatten": true,
///  "x-ms-client-name": "StorageAccountMigrationDetails"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountMigrationProperties {
    ///Reason for migration failure
    #[serde(
        rename = "migrationFailedDetailedReason",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub migration_failed_detailed_reason: ::std::option::Option<::std::string::String>,
    ///Error code for migration failure
    #[serde(
        rename = "migrationFailedReason",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub migration_failed_reason: ::std::option::Option<::std::string::String>,
    ///Current status of migration
    #[serde(
        rename = "migrationStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub migration_status: ::std::option::Option<StorageAccountMigrationPropertiesMigrationStatus>,
    #[serde(rename = "targetSkuName")]
    pub target_sku_name: SkuName,
}
impl ::std::convert::From<&StorageAccountMigrationProperties>
    for StorageAccountMigrationProperties
{
    fn from(value: &StorageAccountMigrationProperties) -> Self {
        value.clone()
    }
}
///Current status of migration
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Current status of migration",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Invalid",
///    "SubmittedForConversion",
///    "InProgress",
///    "Complete",
///    "Failed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "migrationStatus"
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
pub enum StorageAccountMigrationPropertiesMigrationStatus {
    Invalid,
    SubmittedForConversion,
    InProgress,
    Complete,
    Failed,
}
impl ::std::convert::From<&Self> for StorageAccountMigrationPropertiesMigrationStatus {
    fn from(value: &StorageAccountMigrationPropertiesMigrationStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountMigrationPropertiesMigrationStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Invalid => f.write_str("Invalid"),
            Self::SubmittedForConversion => f.write_str("SubmittedForConversion"),
            Self::InProgress => f.write_str("InProgress"),
            Self::Complete => f.write_str("Complete"),
            Self::Failed => f.write_str("Failed"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountMigrationPropertiesMigrationStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "invalid" => Ok(Self::Invalid),
            "submittedforconversion" => Ok(Self::SubmittedForConversion),
            "inprogress" => Ok(Self::InProgress),
            "complete" => Ok(Self::Complete),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountMigrationPropertiesMigrationStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountMigrationPropertiesMigrationStatus
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountMigrationPropertiesMigrationStatus
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Properties of the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the storage account.",
///  "properties": {
///    "accessTier": {
///      "description": "Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Hot",
///        "Cool",
///        "Premium",
///        "Cold"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AccessTier"
///      }
///    },
///    "accountMigrationInProgress": {
///      "description": "If customer initiated account migration is in progress, the value will be true else it will be null.",
///      "readOnly": true,
///      "type": "boolean",
///      "x-ms-client-name": "AccountMigrationInProgress"
///    },
///    "allowBlobPublicAccess": {
///      "description": "Allow or disallow public access to all blobs or containers in the storage account. The default interpretation is false for this property.",
///      "type": "boolean",
///      "x-ms-client-name": "AllowBlobPublicAccess"
///    },
///    "allowCrossTenantReplication": {
///      "description": "Allow or disallow cross AAD tenant object replication. Set this property to true for new or existing accounts only if object replication policies will involve storage accounts in different AAD tenants. The default interpretation is false for new accounts to follow best security practices by default.",
///      "type": "boolean"
///    },
///    "allowSharedKeyAccess": {
///      "description": "Indicates whether the storage account permits requests to be authorized with the account access key via Shared Key. If false, then all requests, including shared access signatures, must be authorized with Azure Active Directory (Azure AD). The default value is null, which is equivalent to true.",
///      "type": "boolean"
///    },
///    "allowedCopyScope": {
///      "description": "Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.",
///      "type": "string",
///      "enum": [
///        "PrivateLink",
///        "AAD"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AllowedCopyScope"
///      }
///    },
///    "azureFilesIdentityBasedAuthentication": {
///      "$ref": "#/components/schemas/AzureFilesIdentityBasedAuthentication"
///    },
///    "blobRestoreStatus": {
///      "$ref": "#/components/schemas/BlobRestoreStatus"
///    },
///    "creationTime": {
///      "description": "Gets the creation date and time of the storage account in UTC.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "customDomain": {
///      "$ref": "#/components/schemas/CustomDomain"
///    },
///    "defaultToOAuthAuthentication": {
///      "description": "A boolean flag which indicates whether the default authentication is OAuth or not. The default interpretation is false for this property.",
///      "type": "boolean"
///    },
///    "dnsEndpointType": {
///      "description": "Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.",
///      "type": "string",
///      "enum": [
///        "Standard",
///        "AzureDnsZone"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DnsEndpointType"
///      }
///    },
///    "enableExtendedGroups": {
///      "description": "Enables extended group support with local users feature, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "EnableExtendedGroups"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "failoverInProgress": {
///      "description": "If the failover is in progress, the value will be true, otherwise, it will be null.",
///      "readOnly": true,
///      "type": "boolean",
///      "x-ms-client-name": "FailoverInProgress"
///    },
///    "geoReplicationStats": {
///      "$ref": "#/components/schemas/GeoReplicationStats"
///    },
///    "immutableStorageWithVersioning": {
///      "$ref": "#/components/schemas/ImmutableStorageAccount"
///    },
///    "isHnsEnabled": {
///      "description": "Account HierarchicalNamespace enabled if sets to true.",
///      "type": "boolean",
///      "x-ms-client-name": "IsHnsEnabled"
///    },
///    "isLocalUserEnabled": {
///      "description": "Enables local users feature, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "IsLocalUserEnabled"
///    },
///    "isNfsV3Enabled": {
///      "description": "NFS 3.0 protocol support enabled if set to true.",
///      "type": "boolean",
///      "x-ms-client-name": "EnableNfsV3"
///    },
///    "isSftpEnabled": {
///      "description": "Enables Secure File Transfer Protocol, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "IsSftpEnabled"
///    },
///    "isSkuConversionBlocked": {
///      "description": "This property will be set to true or false on an event of ongoing migration. Default value is null.",
///      "readOnly": true,
///      "type": "boolean",
///      "x-ms-client-name": "IsSkuConversionBlocked"
///    },
///    "keyCreationTime": {
///      "$ref": "#/components/schemas/KeyCreationTime"
///    },
///    "keyPolicy": {
///      "$ref": "#/components/schemas/KeyPolicy"
///    },
///    "largeFileSharesState": {
///      "description": "Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.",
///      "type": "string",
///      "enum": [
///        "Disabled",
///        "Enabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LargeFileSharesState"
///      }
///    },
///    "lastGeoFailoverTime": {
///      "description": "Gets the timestamp of the most recent instance of a failover to the secondary location. Only the most recent timestamp is retained. This element is not returned if there has never been a failover instance. Only available if the accountType is Standard_GRS or Standard_RAGRS.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "minimumTlsVersion": {
///      "description": "Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.",
///      "type": "string",
///      "enum": [
///        "TLS1_0",
///        "TLS1_1",
///        "TLS1_2",
///        "TLS1_3"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "MinimumTlsVersion"
///      }
///    },
///    "networkAcls": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "primaryEndpoints": {
///      "$ref": "#/components/schemas/Endpoints"
///    },
///    "primaryLocation": {
///      "description": "Gets the location of the primary data center for the storage account.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "privateEndpointConnections": {
///      "description": "List of private endpoint connection associated with the specified storage account",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/PrivateEndpointConnection"
///      }
///    },
///    "provisioningState": {
///      "description": "Gets the status of the storage account at the time the operation was called.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Creating",
///        "ResolvingDNS",
///        "Succeeded"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "ProvisioningState"
///      }
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "routingPreference": {
///      "$ref": "#/components/schemas/RoutingPreference"
///    },
///    "sasPolicy": {
///      "$ref": "#/components/schemas/SasPolicy"
///    },
///    "secondaryEndpoints": {
///      "$ref": "#/components/schemas/Endpoints"
///    },
///    "secondaryLocation": {
///      "description": "Gets the location of the geo-replicated secondary for the storage account. Only available if the accountType is Standard_GRS or Standard_RAGRS.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "statusOfPrimary": {
///      "description": "Gets the status indicating whether the primary location of the storage account is available or unavailable.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "available",
///        "unavailable"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AccountStatus"
///      }
///    },
///    "statusOfSecondary": {
///      "description": "Gets the status indicating whether the secondary location of the storage account is available or unavailable. Only available if the SKU name is Standard_GRS or Standard_RAGRS.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "available",
///        "unavailable"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AccountStatus"
///      }
///    },
///    "storageAccountSkuConversionStatus": {
///      "$ref": "#/components/schemas/StorageAccountSkuConversionStatus"
///    },
///    "supportsHttpsTrafficOnly": {
///      "description": "Allows https traffic only to storage service if sets to true.",
///      "type": "boolean",
///      "x-ms-client-name": "EnableHttpsTrafficOnly"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountProperties {
    ///Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.
    #[serde(
        rename = "accessTier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_tier: ::std::option::Option<StorageAccountPropertiesAccessTier>,
    ///If customer initiated account migration is in progress, the value will be true else it will be null.
    #[serde(
        rename = "accountMigrationInProgress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub account_migration_in_progress: ::std::option::Option<bool>,
    ///Allow or disallow public access to all blobs or containers in the storage account. The default interpretation is false for this property.
    #[serde(
        rename = "allowBlobPublicAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_blob_public_access: ::std::option::Option<bool>,
    ///Allow or disallow cross AAD tenant object replication. Set this property to true for new or existing accounts only if object replication policies will involve storage accounts in different AAD tenants. The default interpretation is false for new accounts to follow best security practices by default.
    #[serde(
        rename = "allowCrossTenantReplication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_cross_tenant_replication: ::std::option::Option<bool>,
    ///Indicates whether the storage account permits requests to be authorized with the account access key via Shared Key. If false, then all requests, including shared access signatures, must be authorized with Azure Active Directory (Azure AD). The default value is null, which is equivalent to true.
    #[serde(
        rename = "allowSharedKeyAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_shared_key_access: ::std::option::Option<bool>,
    ///Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.
    #[serde(
        rename = "allowedCopyScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allowed_copy_scope: ::std::option::Option<StorageAccountPropertiesAllowedCopyScope>,
    #[serde(
        rename = "azureFilesIdentityBasedAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub azure_files_identity_based_authentication:
        ::std::option::Option<AzureFilesIdentityBasedAuthentication>,
    #[serde(
        rename = "blobRestoreStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub blob_restore_status: ::std::option::Option<BlobRestoreStatus>,
    ///Gets the creation date and time of the storage account in UTC.
    #[serde(
        rename = "creationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub creation_time: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "customDomain",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain: ::std::option::Option<CustomDomain>,
    ///A boolean flag which indicates whether the default authentication is OAuth or not. The default interpretation is false for this property.
    #[serde(
        rename = "defaultToOAuthAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_to_o_auth_authentication: ::std::option::Option<bool>,
    ///Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.
    #[serde(
        rename = "dnsEndpointType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_endpoint_type: ::std::option::Option<StorageAccountPropertiesDnsEndpointType>,
    ///Enables extended group support with local users feature, if set to true
    #[serde(
        rename = "enableExtendedGroups",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_extended_groups: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    ///If the failover is in progress, the value will be true, otherwise, it will be null.
    #[serde(
        rename = "failoverInProgress",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub failover_in_progress: ::std::option::Option<bool>,
    #[serde(
        rename = "geoReplicationStats",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub geo_replication_stats: ::std::option::Option<GeoReplicationStats>,
    #[serde(
        rename = "immutableStorageWithVersioning",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutable_storage_with_versioning: ::std::option::Option<ImmutableStorageAccount>,
    ///Account HierarchicalNamespace enabled if sets to true.
    #[serde(
        rename = "isHnsEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_hns_enabled: ::std::option::Option<bool>,
    ///Enables local users feature, if set to true
    #[serde(
        rename = "isLocalUserEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_local_user_enabled: ::std::option::Option<bool>,
    ///NFS 3.0 protocol support enabled if set to true.
    #[serde(
        rename = "isNfsV3Enabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_nfs_v3_enabled: ::std::option::Option<bool>,
    ///Enables Secure File Transfer Protocol, if set to true
    #[serde(
        rename = "isSftpEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_sftp_enabled: ::std::option::Option<bool>,
    ///This property will be set to true or false on an event of ongoing migration. Default value is null.
    #[serde(
        rename = "isSkuConversionBlocked",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_sku_conversion_blocked: ::std::option::Option<bool>,
    #[serde(
        rename = "keyCreationTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_creation_time: ::std::option::Option<KeyCreationTime>,
    #[serde(
        rename = "keyPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_policy: ::std::option::Option<KeyPolicy>,
    ///Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.
    #[serde(
        rename = "largeFileSharesState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub large_file_shares_state:
        ::std::option::Option<StorageAccountPropertiesLargeFileSharesState>,
    ///Gets the timestamp of the most recent instance of a failover to the secondary location. Only the most recent timestamp is retained. This element is not returned if there has never been a failover instance. Only available if the accountType is Standard_GRS or Standard_RAGRS.
    #[serde(
        rename = "lastGeoFailoverTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_geo_failover_time: ::std::option::Option<::std::string::String>,
    ///Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.
    #[serde(
        rename = "minimumTlsVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub minimum_tls_version: ::std::option::Option<StorageAccountPropertiesMinimumTlsVersion>,
    #[serde(
        rename = "networkAcls",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_acls: ::std::option::Option<NetworkRuleSet>,
    #[serde(
        rename = "primaryEndpoints",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub primary_endpoints: ::std::option::Option<Endpoints>,
    ///Gets the location of the primary data center for the storage account.
    #[serde(
        rename = "primaryLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub primary_location: ::std::option::Option<::std::string::String>,
    ///List of private endpoint connection associated with the specified storage account
    #[serde(
        rename = "privateEndpointConnections",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub private_endpoint_connections: ::std::vec::Vec<PrivateEndpointConnection>,
    ///Gets the status of the storage account at the time the operation was called.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<StorageAccountPropertiesProvisioningState>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "routingPreference",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub routing_preference: ::std::option::Option<RoutingPreference>,
    #[serde(
        rename = "sasPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sas_policy: ::std::option::Option<SasPolicy>,
    #[serde(
        rename = "secondaryEndpoints",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secondary_endpoints: ::std::option::Option<Endpoints>,
    ///Gets the location of the geo-replicated secondary for the storage account. Only available if the accountType is Standard_GRS or Standard_RAGRS.
    #[serde(
        rename = "secondaryLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secondary_location: ::std::option::Option<::std::string::String>,
    ///Gets the status indicating whether the primary location of the storage account is available or unavailable.
    #[serde(
        rename = "statusOfPrimary",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status_of_primary: ::std::option::Option<StorageAccountPropertiesStatusOfPrimary>,
    ///Gets the status indicating whether the secondary location of the storage account is available or unavailable. Only available if the SKU name is Standard_GRS or Standard_RAGRS.
    #[serde(
        rename = "statusOfSecondary",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status_of_secondary: ::std::option::Option<StorageAccountPropertiesStatusOfSecondary>,
    #[serde(
        rename = "storageAccountSkuConversionStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub storage_account_sku_conversion_status:
        ::std::option::Option<StorageAccountSkuConversionStatus>,
    ///Allows https traffic only to storage service if sets to true.
    #[serde(
        rename = "supportsHttpsTrafficOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_https_traffic_only: ::std::option::Option<bool>,
}
impl ::std::convert::From<&StorageAccountProperties> for StorageAccountProperties {
    fn from(value: &StorageAccountProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountProperties {
    fn default() -> Self {
        Self {
            access_tier: Default::default(),
            account_migration_in_progress: Default::default(),
            allow_blob_public_access: Default::default(),
            allow_cross_tenant_replication: Default::default(),
            allow_shared_key_access: Default::default(),
            allowed_copy_scope: Default::default(),
            azure_files_identity_based_authentication: Default::default(),
            blob_restore_status: Default::default(),
            creation_time: Default::default(),
            custom_domain: Default::default(),
            default_to_o_auth_authentication: Default::default(),
            dns_endpoint_type: Default::default(),
            enable_extended_groups: Default::default(),
            encryption: Default::default(),
            failover_in_progress: Default::default(),
            geo_replication_stats: Default::default(),
            immutable_storage_with_versioning: Default::default(),
            is_hns_enabled: Default::default(),
            is_local_user_enabled: Default::default(),
            is_nfs_v3_enabled: Default::default(),
            is_sftp_enabled: Default::default(),
            is_sku_conversion_blocked: Default::default(),
            key_creation_time: Default::default(),
            key_policy: Default::default(),
            large_file_shares_state: Default::default(),
            last_geo_failover_time: Default::default(),
            minimum_tls_version: Default::default(),
            network_acls: Default::default(),
            primary_endpoints: Default::default(),
            primary_location: Default::default(),
            private_endpoint_connections: Default::default(),
            provisioning_state: Default::default(),
            public_network_access: Default::default(),
            routing_preference: Default::default(),
            sas_policy: Default::default(),
            secondary_endpoints: Default::default(),
            secondary_location: Default::default(),
            status_of_primary: Default::default(),
            status_of_secondary: Default::default(),
            storage_account_sku_conversion_status: Default::default(),
            supports_https_traffic_only: Default::default(),
        }
    }
}
///Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Hot",
///    "Cool",
///    "Premium",
///    "Cold"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AccessTier"
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
pub enum StorageAccountPropertiesAccessTier {
    Hot,
    Cool,
    Premium,
    Cold,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesAccessTier {
    fn from(value: &StorageAccountPropertiesAccessTier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesAccessTier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Hot => f.write_str("Hot"),
            Self::Cool => f.write_str("Cool"),
            Self::Premium => f.write_str("Premium"),
            Self::Cold => f.write_str("Cold"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesAccessTier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "hot" => Ok(Self::Hot),
            "cool" => Ok(Self::Cool),
            "premium" => Ok(Self::Premium),
            "cold" => Ok(Self::Cold),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesAccessTier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesAccessTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesAccessTier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.",
///  "type": "string",
///  "enum": [
///    "PrivateLink",
///    "AAD"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AllowedCopyScope"
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
pub enum StorageAccountPropertiesAllowedCopyScope {
    PrivateLink,
    #[serde(rename = "AAD")]
    Aad,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesAllowedCopyScope {
    fn from(value: &StorageAccountPropertiesAllowedCopyScope) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesAllowedCopyScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::PrivateLink => f.write_str("PrivateLink"),
            Self::Aad => f.write_str("AAD"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesAllowedCopyScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "privatelink" => Ok(Self::PrivateLink),
            "aad" => Ok(Self::Aad),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesAllowedCopyScope {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesAllowedCopyScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesAllowedCopyScope {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters used to create the storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used to create the storage account.",
///  "properties": {
///    "accessTier": {
///      "description": "Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.",
///      "type": "string",
///      "enum": [
///        "Hot",
///        "Cool",
///        "Premium",
///        "Cold"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AccessTier"
///      }
///    },
///    "allowBlobPublicAccess": {
///      "description": "Allow or disallow public access to all blobs or containers in the storage account. The default interpretation is false for this property.",
///      "type": "boolean",
///      "x-ms-client-name": "AllowBlobPublicAccess"
///    },
///    "allowCrossTenantReplication": {
///      "description": "Allow or disallow cross AAD tenant object replication. Set this property to true for new or existing accounts only if object replication policies will involve storage accounts in different AAD tenants. The default interpretation is false for new accounts to follow best security practices by default.",
///      "type": "boolean"
///    },
///    "allowSharedKeyAccess": {
///      "description": "Indicates whether the storage account permits requests to be authorized with the account access key via Shared Key. If false, then all requests, including shared access signatures, must be authorized with Azure Active Directory (Azure AD). The default value is null, which is equivalent to true.",
///      "type": "boolean"
///    },
///    "allowedCopyScope": {
///      "description": "Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.",
///      "type": "string",
///      "enum": [
///        "PrivateLink",
///        "AAD"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AllowedCopyScope"
///      }
///    },
///    "azureFilesIdentityBasedAuthentication": {
///      "$ref": "#/components/schemas/AzureFilesIdentityBasedAuthentication"
///    },
///    "customDomain": {
///      "$ref": "#/components/schemas/CustomDomain"
///    },
///    "defaultToOAuthAuthentication": {
///      "description": "A boolean flag which indicates whether the default authentication is OAuth or not. The default interpretation is false for this property.",
///      "type": "boolean"
///    },
///    "dnsEndpointType": {
///      "description": "Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.",
///      "type": "string",
///      "enum": [
///        "Standard",
///        "AzureDnsZone"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DnsEndpointType"
///      }
///    },
///    "enableExtendedGroups": {
///      "description": "Enables extended group support with local users feature, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "EnableExtendedGroups"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "immutableStorageWithVersioning": {
///      "$ref": "#/components/schemas/ImmutableStorageAccount"
///    },
///    "isHnsEnabled": {
///      "description": "Account HierarchicalNamespace enabled if sets to true.",
///      "type": "boolean",
///      "x-ms-client-name": "IsHnsEnabled"
///    },
///    "isLocalUserEnabled": {
///      "description": "Enables local users feature, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "IsLocalUserEnabled"
///    },
///    "isNfsV3Enabled": {
///      "description": "NFS 3.0 protocol support enabled if set to true.",
///      "type": "boolean",
///      "x-ms-client-name": "EnableNfsV3"
///    },
///    "isSftpEnabled": {
///      "description": "Enables Secure File Transfer Protocol, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "IsSftpEnabled"
///    },
///    "keyPolicy": {
///      "$ref": "#/components/schemas/KeyPolicy"
///    },
///    "largeFileSharesState": {
///      "description": "Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.",
///      "type": "string",
///      "enum": [
///        "Disabled",
///        "Enabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LargeFileSharesState"
///      }
///    },
///    "minimumTlsVersion": {
///      "description": "Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.",
///      "type": "string",
///      "enum": [
///        "TLS1_0",
///        "TLS1_1",
///        "TLS1_2",
///        "TLS1_3"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "MinimumTlsVersion"
///      }
///    },
///    "networkAcls": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "routingPreference": {
///      "$ref": "#/components/schemas/RoutingPreference"
///    },
///    "sasPolicy": {
///      "$ref": "#/components/schemas/SasPolicy"
///    },
///    "supportsHttpsTrafficOnly": {
///      "description": "Allows https traffic only to storage service if sets to true. The default value is true since API version 2019-04-01.",
///      "type": "boolean",
///      "x-ms-client-name": "EnableHttpsTrafficOnly"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountPropertiesCreateParameters {
    ///Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.
    #[serde(
        rename = "accessTier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_tier: ::std::option::Option<StorageAccountPropertiesCreateParametersAccessTier>,
    ///Allow or disallow public access to all blobs or containers in the storage account. The default interpretation is false for this property.
    #[serde(
        rename = "allowBlobPublicAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_blob_public_access: ::std::option::Option<bool>,
    ///Allow or disallow cross AAD tenant object replication. Set this property to true for new or existing accounts only if object replication policies will involve storage accounts in different AAD tenants. The default interpretation is false for new accounts to follow best security practices by default.
    #[serde(
        rename = "allowCrossTenantReplication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_cross_tenant_replication: ::std::option::Option<bool>,
    ///Indicates whether the storage account permits requests to be authorized with the account access key via Shared Key. If false, then all requests, including shared access signatures, must be authorized with Azure Active Directory (Azure AD). The default value is null, which is equivalent to true.
    #[serde(
        rename = "allowSharedKeyAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_shared_key_access: ::std::option::Option<bool>,
    ///Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.
    #[serde(
        rename = "allowedCopyScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allowed_copy_scope:
        ::std::option::Option<StorageAccountPropertiesCreateParametersAllowedCopyScope>,
    #[serde(
        rename = "azureFilesIdentityBasedAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub azure_files_identity_based_authentication:
        ::std::option::Option<AzureFilesIdentityBasedAuthentication>,
    #[serde(
        rename = "customDomain",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain: ::std::option::Option<CustomDomain>,
    ///A boolean flag which indicates whether the default authentication is OAuth or not. The default interpretation is false for this property.
    #[serde(
        rename = "defaultToOAuthAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_to_o_auth_authentication: ::std::option::Option<bool>,
    ///Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.
    #[serde(
        rename = "dnsEndpointType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_endpoint_type:
        ::std::option::Option<StorageAccountPropertiesCreateParametersDnsEndpointType>,
    ///Enables extended group support with local users feature, if set to true
    #[serde(
        rename = "enableExtendedGroups",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_extended_groups: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    #[serde(
        rename = "immutableStorageWithVersioning",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutable_storage_with_versioning: ::std::option::Option<ImmutableStorageAccount>,
    ///Account HierarchicalNamespace enabled if sets to true.
    #[serde(
        rename = "isHnsEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_hns_enabled: ::std::option::Option<bool>,
    ///Enables local users feature, if set to true
    #[serde(
        rename = "isLocalUserEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_local_user_enabled: ::std::option::Option<bool>,
    ///NFS 3.0 protocol support enabled if set to true.
    #[serde(
        rename = "isNfsV3Enabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_nfs_v3_enabled: ::std::option::Option<bool>,
    ///Enables Secure File Transfer Protocol, if set to true
    #[serde(
        rename = "isSftpEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_sftp_enabled: ::std::option::Option<bool>,
    #[serde(
        rename = "keyPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_policy: ::std::option::Option<KeyPolicy>,
    ///Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.
    #[serde(
        rename = "largeFileSharesState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub large_file_shares_state:
        ::std::option::Option<StorageAccountPropertiesCreateParametersLargeFileSharesState>,
    ///Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.
    #[serde(
        rename = "minimumTlsVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub minimum_tls_version:
        ::std::option::Option<StorageAccountPropertiesCreateParametersMinimumTlsVersion>,
    #[serde(
        rename = "networkAcls",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_acls: ::std::option::Option<NetworkRuleSet>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "routingPreference",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub routing_preference: ::std::option::Option<RoutingPreference>,
    #[serde(
        rename = "sasPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sas_policy: ::std::option::Option<SasPolicy>,
    ///Allows https traffic only to storage service if sets to true. The default value is true since API version 2019-04-01.
    #[serde(
        rename = "supportsHttpsTrafficOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_https_traffic_only: ::std::option::Option<bool>,
}
impl ::std::convert::From<&StorageAccountPropertiesCreateParameters>
    for StorageAccountPropertiesCreateParameters
{
    fn from(value: &StorageAccountPropertiesCreateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountPropertiesCreateParameters {
    fn default() -> Self {
        Self {
            access_tier: Default::default(),
            allow_blob_public_access: Default::default(),
            allow_cross_tenant_replication: Default::default(),
            allow_shared_key_access: Default::default(),
            allowed_copy_scope: Default::default(),
            azure_files_identity_based_authentication: Default::default(),
            custom_domain: Default::default(),
            default_to_o_auth_authentication: Default::default(),
            dns_endpoint_type: Default::default(),
            enable_extended_groups: Default::default(),
            encryption: Default::default(),
            immutable_storage_with_versioning: Default::default(),
            is_hns_enabled: Default::default(),
            is_local_user_enabled: Default::default(),
            is_nfs_v3_enabled: Default::default(),
            is_sftp_enabled: Default::default(),
            key_policy: Default::default(),
            large_file_shares_state: Default::default(),
            minimum_tls_version: Default::default(),
            network_acls: Default::default(),
            public_network_access: Default::default(),
            routing_preference: Default::default(),
            sas_policy: Default::default(),
            supports_https_traffic_only: Default::default(),
        }
    }
}
///Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.",
///  "type": "string",
///  "enum": [
///    "Hot",
///    "Cool",
///    "Premium",
///    "Cold"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AccessTier"
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
pub enum StorageAccountPropertiesCreateParametersAccessTier {
    Hot,
    Cool,
    Premium,
    Cold,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesCreateParametersAccessTier {
    fn from(value: &StorageAccountPropertiesCreateParametersAccessTier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesCreateParametersAccessTier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Hot => f.write_str("Hot"),
            Self::Cool => f.write_str("Cool"),
            Self::Premium => f.write_str("Premium"),
            Self::Cold => f.write_str("Cold"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesCreateParametersAccessTier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "hot" => Ok(Self::Hot),
            "cool" => Ok(Self::Cool),
            "premium" => Ok(Self::Premium),
            "cold" => Ok(Self::Cold),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesCreateParametersAccessTier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesCreateParametersAccessTier
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesCreateParametersAccessTier
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.",
///  "type": "string",
///  "enum": [
///    "PrivateLink",
///    "AAD"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AllowedCopyScope"
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
pub enum StorageAccountPropertiesCreateParametersAllowedCopyScope {
    PrivateLink,
    #[serde(rename = "AAD")]
    Aad,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesCreateParametersAllowedCopyScope {
    fn from(value: &StorageAccountPropertiesCreateParametersAllowedCopyScope) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesCreateParametersAllowedCopyScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::PrivateLink => f.write_str("PrivateLink"),
            Self::Aad => f.write_str("AAD"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesCreateParametersAllowedCopyScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "privatelink" => Ok(Self::PrivateLink),
            "aad" => Ok(Self::Aad),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesCreateParametersAllowedCopyScope {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesCreateParametersAllowedCopyScope
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesCreateParametersAllowedCopyScope
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.",
///  "type": "string",
///  "enum": [
///    "Standard",
///    "AzureDnsZone"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DnsEndpointType"
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
pub enum StorageAccountPropertiesCreateParametersDnsEndpointType {
    Standard,
    AzureDnsZone,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesCreateParametersDnsEndpointType {
    fn from(value: &StorageAccountPropertiesCreateParametersDnsEndpointType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesCreateParametersDnsEndpointType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::AzureDnsZone => f.write_str("AzureDnsZone"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesCreateParametersDnsEndpointType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "azurednszone" => Ok(Self::AzureDnsZone),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesCreateParametersDnsEndpointType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesCreateParametersDnsEndpointType
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesCreateParametersDnsEndpointType
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.",
///  "type": "string",
///  "enum": [
///    "Disabled",
///    "Enabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LargeFileSharesState"
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
pub enum StorageAccountPropertiesCreateParametersLargeFileSharesState {
    Disabled,
    Enabled,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesCreateParametersLargeFileSharesState {
    fn from(value: &StorageAccountPropertiesCreateParametersLargeFileSharesState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesCreateParametersLargeFileSharesState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Disabled => f.write_str("Disabled"),
            Self::Enabled => f.write_str("Enabled"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesCreateParametersLargeFileSharesState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "enabled" => Ok(Self::Enabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
    for StorageAccountPropertiesCreateParametersLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesCreateParametersLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesCreateParametersLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.",
///  "type": "string",
///  "enum": [
///    "TLS1_0",
///    "TLS1_1",
///    "TLS1_2",
///    "TLS1_3"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "MinimumTlsVersion"
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
pub enum StorageAccountPropertiesCreateParametersMinimumTlsVersion {
    #[serde(rename = "TLS1_0")]
    Tls10,
    #[serde(rename = "TLS1_1")]
    Tls11,
    #[serde(rename = "TLS1_2")]
    Tls12,
    #[serde(rename = "TLS1_3")]
    Tls13,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesCreateParametersMinimumTlsVersion {
    fn from(value: &StorageAccountPropertiesCreateParametersMinimumTlsVersion) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesCreateParametersMinimumTlsVersion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Tls10 => f.write_str("TLS1_0"),
            Self::Tls11 => f.write_str("TLS1_1"),
            Self::Tls12 => f.write_str("TLS1_2"),
            Self::Tls13 => f.write_str("TLS1_3"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesCreateParametersMinimumTlsVersion {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "tls1_0" => Ok(Self::Tls10),
            "tls1_1" => Ok(Self::Tls11),
            "tls1_2" => Ok(Self::Tls12),
            "tls1_3" => Ok(Self::Tls13),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesCreateParametersMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesCreateParametersMinimumTlsVersion
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesCreateParametersMinimumTlsVersion
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.",
///  "type": "string",
///  "enum": [
///    "Standard",
///    "AzureDnsZone"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DnsEndpointType"
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
pub enum StorageAccountPropertiesDnsEndpointType {
    Standard,
    AzureDnsZone,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesDnsEndpointType {
    fn from(value: &StorageAccountPropertiesDnsEndpointType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesDnsEndpointType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::AzureDnsZone => f.write_str("AzureDnsZone"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesDnsEndpointType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "azurednszone" => Ok(Self::AzureDnsZone),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesDnsEndpointType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesDnsEndpointType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesDnsEndpointType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.",
///  "type": "string",
///  "enum": [
///    "Disabled",
///    "Enabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LargeFileSharesState"
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
pub enum StorageAccountPropertiesLargeFileSharesState {
    Disabled,
    Enabled,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesLargeFileSharesState {
    fn from(value: &StorageAccountPropertiesLargeFileSharesState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesLargeFileSharesState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Disabled => f.write_str("Disabled"),
            Self::Enabled => f.write_str("Enabled"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesLargeFileSharesState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "enabled" => Ok(Self::Enabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesLargeFileSharesState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.",
///  "type": "string",
///  "enum": [
///    "TLS1_0",
///    "TLS1_1",
///    "TLS1_2",
///    "TLS1_3"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "MinimumTlsVersion"
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
pub enum StorageAccountPropertiesMinimumTlsVersion {
    #[serde(rename = "TLS1_0")]
    Tls10,
    #[serde(rename = "TLS1_1")]
    Tls11,
    #[serde(rename = "TLS1_2")]
    Tls12,
    #[serde(rename = "TLS1_3")]
    Tls13,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesMinimumTlsVersion {
    fn from(value: &StorageAccountPropertiesMinimumTlsVersion) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesMinimumTlsVersion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Tls10 => f.write_str("TLS1_0"),
            Self::Tls11 => f.write_str("TLS1_1"),
            Self::Tls12 => f.write_str("TLS1_2"),
            Self::Tls13 => f.write_str("TLS1_3"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesMinimumTlsVersion {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "tls1_0" => Ok(Self::Tls10),
            "tls1_1" => Ok(Self::Tls11),
            "tls1_2" => Ok(Self::Tls12),
            "tls1_3" => Ok(Self::Tls13),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Gets the status of the storage account at the time the operation was called.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the status of the storage account at the time the operation was called.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Creating",
///    "ResolvingDNS",
///    "Succeeded"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
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
pub enum StorageAccountPropertiesProvisioningState {
    Creating,
    #[serde(rename = "ResolvingDNS")]
    ResolvingDns,
    Succeeded,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesProvisioningState {
    fn from(value: &StorageAccountPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Creating => f.write_str("Creating"),
            Self::ResolvingDns => f.write_str("ResolvingDNS"),
            Self::Succeeded => f.write_str("Succeeded"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "creating" => Ok(Self::Creating),
            "resolvingdns" => Ok(Self::ResolvingDns),
            "succeeded" => Ok(Self::Succeeded),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Gets the status indicating whether the primary location of the storage account is available or unavailable.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the status indicating whether the primary location of the storage account is available or unavailable.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "available",
///    "unavailable"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AccountStatus"
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
pub enum StorageAccountPropertiesStatusOfPrimary {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "unavailable")]
    Unavailable,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesStatusOfPrimary {
    fn from(value: &StorageAccountPropertiesStatusOfPrimary) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesStatusOfPrimary {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Available => f.write_str("available"),
            Self::Unavailable => f.write_str("unavailable"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesStatusOfPrimary {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "available" => Ok(Self::Available),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesStatusOfPrimary {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesStatusOfPrimary {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesStatusOfPrimary {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Gets the status indicating whether the secondary location of the storage account is available or unavailable. Only available if the SKU name is Standard_GRS or Standard_RAGRS.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the status indicating whether the secondary location of the storage account is available or unavailable. Only available if the SKU name is Standard_GRS or Standard_RAGRS.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "available",
///    "unavailable"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AccountStatus"
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
pub enum StorageAccountPropertiesStatusOfSecondary {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "unavailable")]
    Unavailable,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesStatusOfSecondary {
    fn from(value: &StorageAccountPropertiesStatusOfSecondary) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesStatusOfSecondary {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Available => f.write_str("available"),
            Self::Unavailable => f.write_str("unavailable"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesStatusOfSecondary {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "available" => Ok(Self::Available),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesStatusOfSecondary {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountPropertiesStatusOfSecondary {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountPropertiesStatusOfSecondary {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters used when updating a storage account.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used when updating a storage account.",
///  "properties": {
///    "accessTier": {
///      "description": "Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.",
///      "type": "string",
///      "enum": [
///        "Hot",
///        "Cool",
///        "Premium",
///        "Cold"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AccessTier"
///      }
///    },
///    "allowBlobPublicAccess": {
///      "description": "Allow or disallow public access to all blobs or containers in the storage account. The default interpretation is false for this property.",
///      "type": "boolean",
///      "x-ms-client-name": "AllowBlobPublicAccess"
///    },
///    "allowCrossTenantReplication": {
///      "description": "Allow or disallow cross AAD tenant object replication. Set this property to true for new or existing accounts only if object replication policies will involve storage accounts in different AAD tenants. The default interpretation is false for new accounts to follow best security practices by default.",
///      "type": "boolean"
///    },
///    "allowSharedKeyAccess": {
///      "description": "Indicates whether the storage account permits requests to be authorized with the account access key via Shared Key. If false, then all requests, including shared access signatures, must be authorized with Azure Active Directory (Azure AD). The default value is null, which is equivalent to true.",
///      "type": "boolean"
///    },
///    "allowedCopyScope": {
///      "description": "Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.",
///      "type": "string",
///      "enum": [
///        "PrivateLink",
///        "AAD"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AllowedCopyScope"
///      }
///    },
///    "azureFilesIdentityBasedAuthentication": {
///      "$ref": "#/components/schemas/AzureFilesIdentityBasedAuthentication"
///    },
///    "customDomain": {
///      "$ref": "#/components/schemas/CustomDomain"
///    },
///    "defaultToOAuthAuthentication": {
///      "description": "A boolean flag which indicates whether the default authentication is OAuth or not. The default interpretation is false for this property.",
///      "type": "boolean"
///    },
///    "dnsEndpointType": {
///      "description": "Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.",
///      "type": "string",
///      "enum": [
///        "Standard",
///        "AzureDnsZone"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "DnsEndpointType"
///      }
///    },
///    "enableExtendedGroups": {
///      "description": "Enables extended group support with local users feature, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "EnableExtendedGroups"
///    },
///    "encryption": {
///      "$ref": "#/components/schemas/Encryption"
///    },
///    "immutableStorageWithVersioning": {
///      "$ref": "#/components/schemas/ImmutableStorageAccount"
///    },
///    "isLocalUserEnabled": {
///      "description": "Enables local users feature, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "IsLocalUserEnabled"
///    },
///    "isSftpEnabled": {
///      "description": "Enables Secure File Transfer Protocol, if set to true",
///      "type": "boolean",
///      "x-ms-client-name": "IsSftpEnabled"
///    },
///    "keyPolicy": {
///      "$ref": "#/components/schemas/KeyPolicy"
///    },
///    "largeFileSharesState": {
///      "description": "Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.",
///      "type": "string",
///      "enum": [
///        "Disabled",
///        "Enabled"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "LargeFileSharesState"
///      }
///    },
///    "minimumTlsVersion": {
///      "description": "Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.",
///      "type": "string",
///      "enum": [
///        "TLS1_0",
///        "TLS1_1",
///        "TLS1_2",
///        "TLS1_3"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "MinimumTlsVersion"
///      }
///    },
///    "networkAcls": {
///      "$ref": "#/components/schemas/NetworkRuleSet"
///    },
///    "publicNetworkAccess": {
///      "$ref": "#/components/schemas/PublicNetworkAccess"
///    },
///    "routingPreference": {
///      "$ref": "#/components/schemas/RoutingPreference"
///    },
///    "sasPolicy": {
///      "$ref": "#/components/schemas/SasPolicy"
///    },
///    "supportsHttpsTrafficOnly": {
///      "description": "Allows https traffic only to storage service if sets to true.",
///      "type": "boolean",
///      "x-ms-client-name": "EnableHttpsTrafficOnly"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountPropertiesUpdateParameters {
    ///Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.
    #[serde(
        rename = "accessTier",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub access_tier: ::std::option::Option<StorageAccountPropertiesUpdateParametersAccessTier>,
    ///Allow or disallow public access to all blobs or containers in the storage account. The default interpretation is false for this property.
    #[serde(
        rename = "allowBlobPublicAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_blob_public_access: ::std::option::Option<bool>,
    ///Allow or disallow cross AAD tenant object replication. Set this property to true for new or existing accounts only if object replication policies will involve storage accounts in different AAD tenants. The default interpretation is false for new accounts to follow best security practices by default.
    #[serde(
        rename = "allowCrossTenantReplication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_cross_tenant_replication: ::std::option::Option<bool>,
    ///Indicates whether the storage account permits requests to be authorized with the account access key via Shared Key. If false, then all requests, including shared access signatures, must be authorized with Azure Active Directory (Azure AD). The default value is null, which is equivalent to true.
    #[serde(
        rename = "allowSharedKeyAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allow_shared_key_access: ::std::option::Option<bool>,
    ///Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.
    #[serde(
        rename = "allowedCopyScope",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub allowed_copy_scope:
        ::std::option::Option<StorageAccountPropertiesUpdateParametersAllowedCopyScope>,
    #[serde(
        rename = "azureFilesIdentityBasedAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub azure_files_identity_based_authentication:
        ::std::option::Option<AzureFilesIdentityBasedAuthentication>,
    #[serde(
        rename = "customDomain",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain: ::std::option::Option<CustomDomain>,
    ///A boolean flag which indicates whether the default authentication is OAuth or not. The default interpretation is false for this property.
    #[serde(
        rename = "defaultToOAuthAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_to_o_auth_authentication: ::std::option::Option<bool>,
    ///Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.
    #[serde(
        rename = "dnsEndpointType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_endpoint_type:
        ::std::option::Option<StorageAccountPropertiesUpdateParametersDnsEndpointType>,
    ///Enables extended group support with local users feature, if set to true
    #[serde(
        rename = "enableExtendedGroups",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enable_extended_groups: ::std::option::Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<Encryption>,
    #[serde(
        rename = "immutableStorageWithVersioning",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub immutable_storage_with_versioning: ::std::option::Option<ImmutableStorageAccount>,
    ///Enables local users feature, if set to true
    #[serde(
        rename = "isLocalUserEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_local_user_enabled: ::std::option::Option<bool>,
    ///Enables Secure File Transfer Protocol, if set to true
    #[serde(
        rename = "isSftpEnabled",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_sftp_enabled: ::std::option::Option<bool>,
    #[serde(
        rename = "keyPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_policy: ::std::option::Option<KeyPolicy>,
    ///Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.
    #[serde(
        rename = "largeFileSharesState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub large_file_shares_state:
        ::std::option::Option<StorageAccountPropertiesUpdateParametersLargeFileSharesState>,
    ///Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.
    #[serde(
        rename = "minimumTlsVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub minimum_tls_version:
        ::std::option::Option<StorageAccountPropertiesUpdateParametersMinimumTlsVersion>,
    #[serde(
        rename = "networkAcls",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub network_acls: ::std::option::Option<NetworkRuleSet>,
    #[serde(
        rename = "publicNetworkAccess",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_network_access: ::std::option::Option<PublicNetworkAccess>,
    #[serde(
        rename = "routingPreference",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub routing_preference: ::std::option::Option<RoutingPreference>,
    #[serde(
        rename = "sasPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sas_policy: ::std::option::Option<SasPolicy>,
    ///Allows https traffic only to storage service if sets to true.
    #[serde(
        rename = "supportsHttpsTrafficOnly",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub supports_https_traffic_only: ::std::option::Option<bool>,
}
impl ::std::convert::From<&StorageAccountPropertiesUpdateParameters>
    for StorageAccountPropertiesUpdateParameters
{
    fn from(value: &StorageAccountPropertiesUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountPropertiesUpdateParameters {
    fn default() -> Self {
        Self {
            access_tier: Default::default(),
            allow_blob_public_access: Default::default(),
            allow_cross_tenant_replication: Default::default(),
            allow_shared_key_access: Default::default(),
            allowed_copy_scope: Default::default(),
            azure_files_identity_based_authentication: Default::default(),
            custom_domain: Default::default(),
            default_to_o_auth_authentication: Default::default(),
            dns_endpoint_type: Default::default(),
            enable_extended_groups: Default::default(),
            encryption: Default::default(),
            immutable_storage_with_versioning: Default::default(),
            is_local_user_enabled: Default::default(),
            is_sftp_enabled: Default::default(),
            key_policy: Default::default(),
            large_file_shares_state: Default::default(),
            minimum_tls_version: Default::default(),
            network_acls: Default::default(),
            public_network_access: Default::default(),
            routing_preference: Default::default(),
            sas_policy: Default::default(),
            supports_https_traffic_only: Default::default(),
        }
    }
}
///Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Required for storage accounts where kind = BlobStorage. The access tier is used for billing. The 'Premium' access tier is the default value for premium block blobs storage account type and it cannot be changed for the premium block blobs storage account type.",
///  "type": "string",
///  "enum": [
///    "Hot",
///    "Cool",
///    "Premium",
///    "Cold"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AccessTier"
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
pub enum StorageAccountPropertiesUpdateParametersAccessTier {
    Hot,
    Cool,
    Premium,
    Cold,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesUpdateParametersAccessTier {
    fn from(value: &StorageAccountPropertiesUpdateParametersAccessTier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesUpdateParametersAccessTier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Hot => f.write_str("Hot"),
            Self::Cool => f.write_str("Cool"),
            Self::Premium => f.write_str("Premium"),
            Self::Cold => f.write_str("Cold"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesUpdateParametersAccessTier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "hot" => Ok(Self::Hot),
            "cool" => Ok(Self::Cool),
            "premium" => Ok(Self::Premium),
            "cold" => Ok(Self::Cold),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesUpdateParametersAccessTier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesUpdateParametersAccessTier
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesUpdateParametersAccessTier
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Restrict copy to and from Storage Accounts within an AAD tenant or with Private Links to the same VNet.",
///  "type": "string",
///  "enum": [
///    "PrivateLink",
///    "AAD"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AllowedCopyScope"
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
pub enum StorageAccountPropertiesUpdateParametersAllowedCopyScope {
    PrivateLink,
    #[serde(rename = "AAD")]
    Aad,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesUpdateParametersAllowedCopyScope {
    fn from(value: &StorageAccountPropertiesUpdateParametersAllowedCopyScope) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesUpdateParametersAllowedCopyScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::PrivateLink => f.write_str("PrivateLink"),
            Self::Aad => f.write_str("AAD"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesUpdateParametersAllowedCopyScope {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "privatelink" => Ok(Self::PrivateLink),
            "aad" => Ok(Self::Aad),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesUpdateParametersAllowedCopyScope {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesUpdateParametersAllowedCopyScope
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesUpdateParametersAllowedCopyScope
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allows you to specify the type of endpoint. Set this to AzureDNSZone to create a large number of accounts in a single subscription, which creates accounts in an Azure DNS Zone and the endpoint URL will have an alphanumeric DNS Zone identifier.",
///  "type": "string",
///  "enum": [
///    "Standard",
///    "AzureDnsZone"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DnsEndpointType"
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
pub enum StorageAccountPropertiesUpdateParametersDnsEndpointType {
    Standard,
    AzureDnsZone,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesUpdateParametersDnsEndpointType {
    fn from(value: &StorageAccountPropertiesUpdateParametersDnsEndpointType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesUpdateParametersDnsEndpointType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::AzureDnsZone => f.write_str("AzureDnsZone"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesUpdateParametersDnsEndpointType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "azurednszone" => Ok(Self::AzureDnsZone),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesUpdateParametersDnsEndpointType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesUpdateParametersDnsEndpointType
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesUpdateParametersDnsEndpointType
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Allow large file shares if sets to Enabled. It cannot be disabled once it is enabled.",
///  "type": "string",
///  "enum": [
///    "Disabled",
///    "Enabled"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "LargeFileSharesState"
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
pub enum StorageAccountPropertiesUpdateParametersLargeFileSharesState {
    Disabled,
    Enabled,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesUpdateParametersLargeFileSharesState {
    fn from(value: &StorageAccountPropertiesUpdateParametersLargeFileSharesState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesUpdateParametersLargeFileSharesState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Disabled => f.write_str("Disabled"),
            Self::Enabled => f.write_str("Enabled"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesUpdateParametersLargeFileSharesState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "enabled" => Ok(Self::Enabled),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
    for StorageAccountPropertiesUpdateParametersLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesUpdateParametersLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesUpdateParametersLargeFileSharesState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Set the minimum TLS version to be permitted on requests to storage. The default interpretation is TLS 1.0 for this property.",
///  "type": "string",
///  "enum": [
///    "TLS1_0",
///    "TLS1_1",
///    "TLS1_2",
///    "TLS1_3"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "MinimumTlsVersion"
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
pub enum StorageAccountPropertiesUpdateParametersMinimumTlsVersion {
    #[serde(rename = "TLS1_0")]
    Tls10,
    #[serde(rename = "TLS1_1")]
    Tls11,
    #[serde(rename = "TLS1_2")]
    Tls12,
    #[serde(rename = "TLS1_3")]
    Tls13,
}
impl ::std::convert::From<&Self> for StorageAccountPropertiesUpdateParametersMinimumTlsVersion {
    fn from(value: &StorageAccountPropertiesUpdateParametersMinimumTlsVersion) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountPropertiesUpdateParametersMinimumTlsVersion {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Tls10 => f.write_str("TLS1_0"),
            Self::Tls11 => f.write_str("TLS1_1"),
            Self::Tls12 => f.write_str("TLS1_2"),
            Self::Tls13 => f.write_str("TLS1_3"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountPropertiesUpdateParametersMinimumTlsVersion {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "tls1_0" => Ok(Self::Tls10),
            "tls1_1" => Ok(Self::Tls11),
            "tls1_2" => Ok(Self::Tls12),
            "tls1_3" => Ok(Self::Tls13),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountPropertiesUpdateParametersMinimumTlsVersion {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountPropertiesUpdateParametersMinimumTlsVersion
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountPropertiesUpdateParametersMinimumTlsVersion
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters used to regenerate the storage account key.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters used to regenerate the storage account key.",
///  "required": [
///    "keyName"
///  ],
///  "properties": {
///    "keyName": {
///      "description": "The name of storage keys that want to be regenerated, possible values are key1, key2, kerb1, kerb2.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountRegenerateKeyParameters {
    ///The name of storage keys that want to be regenerated, possible values are key1, key2, kerb1, kerb2.
    #[serde(rename = "keyName")]
    pub key_name: ::std::string::String,
}
impl ::std::convert::From<&StorageAccountRegenerateKeyParameters>
    for StorageAccountRegenerateKeyParameters
{
    fn from(value: &StorageAccountRegenerateKeyParameters) -> Self {
        value.clone()
    }
}
///This defines the sku conversion status object for asynchronous sku conversions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This defines the sku conversion status object for asynchronous sku conversions.",
///  "type": "object",
///  "properties": {
///    "endTime": {
///      "description": "This property represents the sku conversion end time.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "skuConversionStatus": {
///      "description": "This property indicates the current sku conversion status.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "InProgress",
///        "Succeeded",
///        "Failed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "SkuConversionStatus"
///      }
///    },
///    "startTime": {
///      "description": "This property represents the sku conversion start time.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "targetSkuName": {
///      "$ref": "#/components/schemas/SkuName"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageAccountSkuConversionStatus {
    ///This property represents the sku conversion end time.
    #[serde(
        rename = "endTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub end_time: ::std::option::Option<::std::string::String>,
    ///This property indicates the current sku conversion status.
    #[serde(
        rename = "skuConversionStatus",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku_conversion_status:
        ::std::option::Option<StorageAccountSkuConversionStatusSkuConversionStatus>,
    ///This property represents the sku conversion start time.
    #[serde(
        rename = "startTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub start_time: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "targetSkuName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_sku_name: ::std::option::Option<SkuName>,
}
impl ::std::convert::From<&StorageAccountSkuConversionStatus>
    for StorageAccountSkuConversionStatus
{
    fn from(value: &StorageAccountSkuConversionStatus) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountSkuConversionStatus {
    fn default() -> Self {
        Self {
            end_time: Default::default(),
            sku_conversion_status: Default::default(),
            start_time: Default::default(),
            target_sku_name: Default::default(),
        }
    }
}
///This property indicates the current sku conversion status.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This property indicates the current sku conversion status.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "InProgress",
///    "Succeeded",
///    "Failed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "SkuConversionStatus"
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
pub enum StorageAccountSkuConversionStatusSkuConversionStatus {
    InProgress,
    Succeeded,
    Failed,
}
impl ::std::convert::From<&Self> for StorageAccountSkuConversionStatusSkuConversionStatus {
    fn from(value: &StorageAccountSkuConversionStatusSkuConversionStatus) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountSkuConversionStatusSkuConversionStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::InProgress => f.write_str("InProgress"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountSkuConversionStatusSkuConversionStatus {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "inprogress" => Ok(Self::InProgress),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountSkuConversionStatusSkuConversionStatus {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for StorageAccountSkuConversionStatusSkuConversionStatus
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for StorageAccountSkuConversionStatusSkuConversionStatus
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The parameters that can be provided when updating the storage account properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The parameters that can be provided when updating the storage account properties.",
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/Identity"
///    },
///    "kind": {
///      "description": "Optional. Indicates the type of storage account. Currently only StorageV2 value supported by server.",
///      "type": "string",
///      "enum": [
///        "Storage",
///        "StorageV2",
///        "BlobStorage",
///        "FileStorage",
///        "BlockBlobStorage"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "Kind"
///      }
///    },
///    "properties": {
///      "$ref": "#/components/schemas/StorageAccountPropertiesUpdateParameters"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    },
///    "tags": {
///      "description": "Gets or sets a list of key value pairs that describe the resource. These tags can be used in viewing and grouping this resource (across resource groups). A maximum of 15 tags can be provided for a resource. Each tag must have a key no greater in length than 128 characters and a value no greater in length than 256 characters.",
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
pub struct StorageAccountUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<Identity>,
    ///Optional. Indicates the type of storage account. Currently only StorageV2 value supported by server.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<StorageAccountUpdateParametersKind>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<StorageAccountPropertiesUpdateParameters>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
    ///Gets or sets a list of key value pairs that describe the resource. These tags can be used in viewing and grouping this resource (across resource groups). A maximum of 15 tags can be provided for a resource. Each tag must have a key no greater in length than 128 characters and a value no greater in length than 256 characters.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&StorageAccountUpdateParameters> for StorageAccountUpdateParameters {
    fn from(value: &StorageAccountUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageAccountUpdateParameters {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            kind: Default::default(),
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
        }
    }
}
///Optional. Indicates the type of storage account. Currently only StorageV2 value supported by server.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Optional. Indicates the type of storage account. Currently only StorageV2 value supported by server.",
///  "type": "string",
///  "enum": [
///    "Storage",
///    "StorageV2",
///    "BlobStorage",
///    "FileStorage",
///    "BlockBlobStorage"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "Kind"
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
pub enum StorageAccountUpdateParametersKind {
    Storage,
    StorageV2,
    BlobStorage,
    FileStorage,
    BlockBlobStorage,
}
impl ::std::convert::From<&Self> for StorageAccountUpdateParametersKind {
    fn from(value: &StorageAccountUpdateParametersKind) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for StorageAccountUpdateParametersKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Storage => f.write_str("Storage"),
            Self::StorageV2 => f.write_str("StorageV2"),
            Self::BlobStorage => f.write_str("BlobStorage"),
            Self::FileStorage => f.write_str("FileStorage"),
            Self::BlockBlobStorage => f.write_str("BlockBlobStorage"),
        }
    }
}
impl ::std::str::FromStr for StorageAccountUpdateParametersKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "storage" => Ok(Self::Storage),
            "storagev2" => Ok(Self::StorageV2),
            "blobstorage" => Ok(Self::BlobStorage),
            "filestorage" => Ok(Self::FileStorage),
            "blockblobstorage" => Ok(Self::BlockBlobStorage),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StorageAccountUpdateParametersKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for StorageAccountUpdateParametersKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StorageAccountUpdateParametersKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The response from the List Storage SKUs operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the List Storage SKUs operation.",
///  "properties": {
///    "value": {
///      "description": "Get the list result of storage SKUs and their properties.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SkuInformation"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct StorageSkuListResult {
    ///Get the list result of storage SKUs and their properties.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<SkuInformation>,
}
impl ::std::convert::From<&StorageSkuListResult> for StorageSkuListResult {
    fn from(value: &StorageSkuListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for StorageSkuListResult {
    fn default() -> Self {
        Self {
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
///Blob index tag based filtering for blob objects
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Blob index tag based filtering for blob objects",
///  "required": [
///    "name",
///    "op",
///    "value"
///  ],
///  "properties": {
///    "name": {
///      "description": "This is the filter tag name, it can have 1 - 128 characters",
///      "type": "string",
///      "maxLength": 128,
///      "minLength": 1
///    },
///    "op": {
///      "description": "This is the comparison operator which is used for object comparison and filtering. Only == (equality operator) is currently supported",
///      "type": "string"
///    },
///    "value": {
///      "description": "This is the filter tag value field used for tag based filtering, it can have 0 - 256 characters",
///      "type": "string",
///      "maxLength": 256,
///      "minLength": 0
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagFilter {
    ///This is the filter tag name, it can have 1 - 128 characters
    pub name: TagFilterName,
    ///This is the comparison operator which is used for object comparison and filtering. Only == (equality operator) is currently supported
    pub op: ::std::string::String,
    ///This is the filter tag value field used for tag based filtering, it can have 0 - 256 characters
    pub value: TagFilterValue,
}
impl ::std::convert::From<&TagFilter> for TagFilter {
    fn from(value: &TagFilter) -> Self {
        value.clone()
    }
}
///This is the filter tag name, it can have 1 - 128 characters
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This is the filter tag name, it can have 1 - 128 characters",
///  "type": "string",
///  "maxLength": 128,
///  "minLength": 1
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct TagFilterName(::std::string::String);
impl ::std::ops::Deref for TagFilterName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<TagFilterName> for ::std::string::String {
    fn from(value: TagFilterName) -> Self {
        value.0
    }
}
impl ::std::convert::From<&TagFilterName> for TagFilterName {
    fn from(value: &TagFilterName) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for TagFilterName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 128usize {
            return Err("longer than 128 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for TagFilterName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TagFilterName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TagFilterName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for TagFilterName {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
///This is the filter tag value field used for tag based filtering, it can have 0 - 256 characters
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "This is the filter tag value field used for tag based filtering, it can have 0 - 256 characters",
///  "type": "string",
///  "maxLength": 256,
///  "minLength": 0
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct TagFilterValue(::std::string::String);
impl ::std::ops::Deref for TagFilterValue {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<TagFilterValue> for ::std::string::String {
    fn from(value: TagFilterValue) -> Self {
        value.0
    }
}
impl ::std::convert::From<&TagFilterValue> for TagFilterValue {
    fn from(value: &TagFilterValue) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for TagFilterValue {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        if value.chars().count() < 0usize {
            return Err("shorter than 0 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for TagFilterValue {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TagFilterValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TagFilterValue {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for TagFilterValue {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
///The SKU tier. This is based on the SKU name.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The SKU tier. This is based on the SKU name.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Standard",
///    "Premium"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "SkuTier"
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
pub enum Tier {
    Standard,
    Premium,
}
impl ::std::convert::From<&Self> for Tier {
    fn from(value: &Tier) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for Tier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Standard => f.write_str("Standard"),
            Self::Premium => f.write_str("Premium"),
        }
    }
}
impl ::std::str::FromStr for Tier {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "premium" => Ok(Self::Premium),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Tier {
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
///Describes Storage Resource Usage.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Describes Storage Resource Usage.",
///  "properties": {
///    "currentValue": {
///      "description": "Gets the current count of the allocated resources in the subscription.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int32"
///    },
///    "limit": {
///      "description": "Gets the maximum count of the resources that can be allocated in the subscription.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int32"
///    },
///    "name": {
///      "$ref": "#/components/schemas/UsageName"
///    },
///    "unit": {
///      "description": "Gets the unit of measurement.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Count",
///        "Bytes",
///        "Seconds",
///        "Percent",
///        "CountsPerSecond",
///        "BytesPerSecond"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "UsageUnit"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Usage {
    ///Gets the current count of the allocated resources in the subscription.
    #[serde(
        rename = "currentValue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub current_value: ::std::option::Option<i32>,
    ///Gets the maximum count of the resources that can be allocated in the subscription.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub limit: ::std::option::Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<UsageName>,
    ///Gets the unit of measurement.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub unit: ::std::option::Option<UsageUnit>,
}
impl ::std::convert::From<&Usage> for Usage {
    fn from(value: &Usage) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Usage {
    fn default() -> Self {
        Self {
            current_value: Default::default(),
            limit: Default::default(),
            name: Default::default(),
            unit: Default::default(),
        }
    }
}
///The response from the List Usages operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The response from the List Usages operation.",
///  "properties": {
///    "value": {
///      "description": "Gets or sets the list of Storage Resource Usages.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Usage"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UsageListResult {
    ///Gets or sets the list of Storage Resource Usages.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Usage>,
}
impl ::std::convert::From<&UsageListResult> for UsageListResult {
    fn from(value: &UsageListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UsageListResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///The usage names that can be used; currently limited to StorageAccount.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The usage names that can be used; currently limited to StorageAccount.",
///  "properties": {
///    "localizedValue": {
///      "description": "Gets a localized string describing the resource name.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "Gets a string describing the resource name.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UsageName {
    ///Gets a localized string describing the resource name.
    #[serde(
        rename = "localizedValue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub localized_value: ::std::option::Option<::std::string::String>,
    ///Gets a string describing the resource name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&UsageName> for UsageName {
    fn from(value: &UsageName) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for UsageName {
    fn default() -> Self {
        Self {
            localized_value: Default::default(),
            value: Default::default(),
        }
    }
}
///Gets the unit of measurement.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the unit of measurement.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Count",
///    "Bytes",
///    "Seconds",
///    "Percent",
///    "CountsPerSecond",
///    "BytesPerSecond"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "UsageUnit"
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
pub enum UsageUnit {
    Count,
    Bytes,
    Seconds,
    Percent,
    CountsPerSecond,
    BytesPerSecond,
}
impl ::std::convert::From<&Self> for UsageUnit {
    fn from(value: &UsageUnit) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for UsageUnit {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Count => f.write_str("Count"),
            Self::Bytes => f.write_str("Bytes"),
            Self::Seconds => f.write_str("Seconds"),
            Self::Percent => f.write_str("Percent"),
            Self::CountsPerSecond => f.write_str("CountsPerSecond"),
            Self::BytesPerSecond => f.write_str("BytesPerSecond"),
        }
    }
}
impl ::std::str::FromStr for UsageUnit {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "count" => Ok(Self::Count),
            "bytes" => Ok(Self::Bytes),
            "seconds" => Ok(Self::Seconds),
            "percent" => Ok(Self::Percent),
            "countspersecond" => Ok(Self::CountsPerSecond),
            "bytespersecond" => Ok(Self::BytesPerSecond),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for UsageUnit {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for UsageUnit {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for UsageUnit {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///UserAssignedIdentity for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "UserAssignedIdentity for the resource.",
///  "properties": {
///    "clientId": {
///      "description": "The client ID of the identity.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "principalId": {
///      "description": "The principal ID of the identity.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct UserAssignedIdentity {
    ///The client ID of the identity.
    #[serde(
        rename = "clientId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub client_id: ::std::option::Option<::std::string::String>,
    ///The principal ID of the identity.
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id: ::std::option::Option<::std::string::String>,
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
///Virtual Network rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Virtual Network rule.",
///  "required": [
///    "id"
///  ],
///  "properties": {
///    "action": {
///      "description": "The action of virtual network rule.",
///      "default": "Allow",
///      "type": "string",
///      "enum": [
///        "Allow"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "Action"
///      }
///    },
///    "id": {
///      "description": "Resource ID of a subnet, for example: /subscriptions/{subscriptionId}/resourceGroups/{groupName}/providers/Microsoft.Network/virtualNetworks/{vnetName}/subnets/{subnetName}.",
///      "type": "string",
///      "x-ms-client-name": "VirtualNetworkResourceId"
///    },
///    "state": {
///      "description": "Gets the state of virtual network rule.",
///      "type": "string",
///      "enum": [
///        "Provisioning",
///        "Deprovisioning",
///        "Succeeded",
///        "Failed",
///        "NetworkSourceDeleted"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "State"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VirtualNetworkRule {
    ///The action of virtual network rule.
    #[serde(
        default = "defaults::virtual_network_rule_action",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action: VirtualNetworkRuleAction,
    ///Resource ID of a subnet, for example: /subscriptions/{subscriptionId}/resourceGroups/{groupName}/providers/Microsoft.Network/virtualNetworks/{vnetName}/subnets/{subnetName}.
    pub id: ::std::string::String,
    ///Gets the state of virtual network rule.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub state: ::std::option::Option<VirtualNetworkRuleState>,
}
impl ::std::convert::From<&VirtualNetworkRule> for VirtualNetworkRule {
    fn from(value: &VirtualNetworkRule) -> Self {
        value.clone()
    }
}
///The action of virtual network rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The action of virtual network rule.",
///  "default": "Allow",
///  "type": "string",
///  "enum": [
///    "Allow"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "Action"
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
pub enum VirtualNetworkRuleAction {
    Allow,
}
impl ::std::convert::From<&Self> for VirtualNetworkRuleAction {
    fn from(value: &VirtualNetworkRuleAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VirtualNetworkRuleAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Allow => f.write_str("Allow"),
        }
    }
}
impl ::std::str::FromStr for VirtualNetworkRuleAction {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "allow" => Ok(Self::Allow),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VirtualNetworkRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VirtualNetworkRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VirtualNetworkRuleAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for VirtualNetworkRuleAction {
    fn default() -> Self {
        VirtualNetworkRuleAction::Allow
    }
}
///Gets the state of virtual network rule.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Gets the state of virtual network rule.",
///  "type": "string",
///  "enum": [
///    "Provisioning",
///    "Deprovisioning",
///    "Succeeded",
///    "Failed",
///    "NetworkSourceDeleted"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "State"
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
pub enum VirtualNetworkRuleState {
    Provisioning,
    Deprovisioning,
    Succeeded,
    Failed,
    NetworkSourceDeleted,
}
impl ::std::convert::From<&Self> for VirtualNetworkRuleState {
    fn from(value: &VirtualNetworkRuleState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for VirtualNetworkRuleState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Provisioning => f.write_str("Provisioning"),
            Self::Deprovisioning => f.write_str("Deprovisioning"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::NetworkSourceDeleted => f.write_str("NetworkSourceDeleted"),
        }
    }
}
impl ::std::str::FromStr for VirtualNetworkRuleState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "provisioning" => Ok(Self::Provisioning),
            "deprovisioning" => Ok(Self::Deprovisioning),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "networksourcedeleted" => Ok(Self::NetworkSourceDeleted),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for VirtualNetworkRuleState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for VirtualNetworkRuleState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for VirtualNetworkRuleState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn encryption_key_source() -> super::EncryptionKeySource {
        super::EncryptionKeySource::MicrosoftStorage
    }
    pub(super) fn ip_rule_action() -> super::IpRuleAction {
        super::IpRuleAction::Allow
    }
    pub(super) fn network_rule_set_bypass() -> super::NetworkRuleSetBypass {
        super::NetworkRuleSetBypass::AzureServices
    }
    pub(super) fn virtual_network_rule_action() -> super::VirtualNetworkRuleAction {
        super::VirtualNetworkRuleAction::Allow
    }
}
