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
///The alias type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The alias type. ",
///  "properties": {
///    "defaultMetadata": {
///      "$ref": "#/components/schemas/AliasPathMetadata"
///    },
///    "defaultPath": {
///      "description": "The default path for an alias.",
///      "type": "string"
///    },
///    "defaultPattern": {
///      "$ref": "#/components/schemas/AliasPattern"
///    },
///    "name": {
///      "description": "The alias name.",
///      "type": "string"
///    },
///    "paths": {
///      "description": "The paths for an alias.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AliasPath"
///      },
///      "x-ms-identifiers": []
///    },
///    "type": {
///      "description": "The type of the alias.",
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "PlainText",
///        "Mask"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AliasType",
///        "values": [
///          {
///            "description": "Alias type is unknown (same as not providing alias type).",
///            "value": "NotSpecified"
///          },
///          {
///            "description": "Alias value is not secret.",
///            "value": "PlainText"
///          },
///          {
///            "description": "Alias value is secret.",
///            "value": "Mask"
///          }
///        ]
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Alias {
    #[serde(
        rename = "defaultMetadata",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_metadata: ::std::option::Option<AliasPathMetadata>,
    ///The default path for an alias.
    #[serde(
        rename = "defaultPath",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_path: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "defaultPattern",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_pattern: ::std::option::Option<AliasPattern>,
    ///The alias name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The paths for an alias.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub paths: ::std::vec::Vec<AliasPath>,
    ///The type of the alias.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<AliasType>,
}
impl ::std::convert::From<&Alias> for Alias {
    fn from(value: &Alias) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Alias {
    fn default() -> Self {
        Self {
            default_metadata: Default::default(),
            default_path: Default::default(),
            default_pattern: Default::default(),
            name: Default::default(),
            paths: Default::default(),
            type_: Default::default(),
        }
    }
}
///The type of the paths for alias.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of the paths for alias.",
///  "properties": {
///    "apiVersions": {
///      "description": "The API versions.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "metadata": {
///      "$ref": "#/components/schemas/AliasPathMetadata"
///    },
///    "path": {
///      "description": "The path of an alias.",
///      "type": "string"
///    },
///    "pattern": {
///      "$ref": "#/components/schemas/AliasPattern"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AliasPath {
    ///The API versions.
    #[serde(
        rename = "apiVersions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub api_versions: ::std::vec::Vec<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub metadata: ::std::option::Option<AliasPathMetadata>,
    ///The path of an alias.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub path: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub pattern: ::std::option::Option<AliasPattern>,
}
impl ::std::convert::From<&AliasPath> for AliasPath {
    fn from(value: &AliasPath) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AliasPath {
    fn default() -> Self {
        Self {
            api_versions: Default::default(),
            metadata: Default::default(),
            path: Default::default(),
            pattern: Default::default(),
        }
    }
}
///`AliasPathMetadata`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "properties": {
///    "attributes": {
///      "description": "The attributes of the token that the alias path is referring to.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "None",
///        "Modifiable"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AliasPathAttributes",
///        "values": [
///          {
///            "description": "The token that the alias path is referring to has no attributes.",
///            "value": "None"
///          },
///          {
///            "description": "The token that the alias path is referring to is modifiable by policies with 'modify' effect.",
///            "value": "Modifiable"
///          }
///        ]
///      }
///    },
///    "type": {
///      "description": "The type of the token that the alias path is referring to.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Any",
///        "String",
///        "Object",
///        "Array",
///        "Integer",
///        "Number",
///        "Boolean"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "AliasPathTokenType",
///        "values": [
///          {
///            "description": "The token type is not specified.",
///            "value": "NotSpecified"
///          },
///          {
///            "description": "The token type can be anything.",
///            "value": "Any"
///          },
///          {
///            "description": "The token type is string.",
///            "value": "String"
///          },
///          {
///            "description": "The token type is object.",
///            "value": "Object"
///          },
///          {
///            "description": "The token type is array.",
///            "value": "Array"
///          },
///          {
///            "description": "The token type is integer.",
///            "value": "Integer"
///          },
///          {
///            "description": "The token type is number.",
///            "value": "Number"
///          },
///          {
///            "description": "The token type is boolean.",
///            "value": "Boolean"
///          }
///        ]
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AliasPathMetadata {
    ///The attributes of the token that the alias path is referring to.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<AliasPathMetadataAttributes>,
    ///The type of the token that the alias path is referring to.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<AliasPathMetadataType>,
}
impl ::std::convert::From<&AliasPathMetadata> for AliasPathMetadata {
    fn from(value: &AliasPathMetadata) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AliasPathMetadata {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            type_: Default::default(),
        }
    }
}
///The attributes of the token that the alias path is referring to.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The attributes of the token that the alias path is referring to.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "None",
///    "Modifiable"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AliasPathAttributes",
///    "values": [
///      {
///        "description": "The token that the alias path is referring to has no attributes.",
///        "value": "None"
///      },
///      {
///        "description": "The token that the alias path is referring to is modifiable by policies with 'modify' effect.",
///        "value": "Modifiable"
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
pub enum AliasPathMetadataAttributes {
    None,
    Modifiable,
}
impl ::std::convert::From<&Self> for AliasPathMetadataAttributes {
    fn from(value: &AliasPathMetadataAttributes) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AliasPathMetadataAttributes {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("None"),
            Self::Modifiable => f.write_str("Modifiable"),
        }
    }
}
impl ::std::str::FromStr for AliasPathMetadataAttributes {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "modifiable" => Ok(Self::Modifiable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AliasPathMetadataAttributes {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AliasPathMetadataAttributes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AliasPathMetadataAttributes {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The type of the token that the alias path is referring to.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of the token that the alias path is referring to.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Any",
///    "String",
///    "Object",
///    "Array",
///    "Integer",
///    "Number",
///    "Boolean"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "AliasPathTokenType",
///    "values": [
///      {
///        "description": "The token type is not specified.",
///        "value": "NotSpecified"
///      },
///      {
///        "description": "The token type can be anything.",
///        "value": "Any"
///      },
///      {
///        "description": "The token type is string.",
///        "value": "String"
///      },
///      {
///        "description": "The token type is object.",
///        "value": "Object"
///      },
///      {
///        "description": "The token type is array.",
///        "value": "Array"
///      },
///      {
///        "description": "The token type is integer.",
///        "value": "Integer"
///      },
///      {
///        "description": "The token type is number.",
///        "value": "Number"
///      },
///      {
///        "description": "The token type is boolean.",
///        "value": "Boolean"
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
pub enum AliasPathMetadataType {
    NotSpecified,
    Any,
    String,
    Object,
    Array,
    Integer,
    Number,
    Boolean,
}
impl ::std::convert::From<&Self> for AliasPathMetadataType {
    fn from(value: &AliasPathMetadataType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AliasPathMetadataType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Any => f.write_str("Any"),
            Self::String => f.write_str("String"),
            Self::Object => f.write_str("Object"),
            Self::Array => f.write_str("Array"),
            Self::Integer => f.write_str("Integer"),
            Self::Number => f.write_str("Number"),
            Self::Boolean => f.write_str("Boolean"),
        }
    }
}
impl ::std::str::FromStr for AliasPathMetadataType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "any" => Ok(Self::Any),
            "string" => Ok(Self::String),
            "object" => Ok(Self::Object),
            "array" => Ok(Self::Array),
            "integer" => Ok(Self::Integer),
            "number" => Ok(Self::Number),
            "boolean" => Ok(Self::Boolean),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AliasPathMetadataType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AliasPathMetadataType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AliasPathMetadataType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The type of the pattern for an alias path.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of the pattern for an alias path.",
///  "properties": {
///    "phrase": {
///      "description": "The alias pattern phrase.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The type of alias pattern",
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Extract"
///      ],
///      "x-ms-enum": {
///        "modelAsString": false,
///        "name": "AliasPatternType",
///        "values": [
///          {
///            "description": "NotSpecified is not allowed.",
///            "value": "NotSpecified"
///          },
///          {
///            "description": "Extract is the only allowed value.",
///            "value": "Extract"
///          }
///        ]
///      }
///    },
///    "variable": {
///      "description": "The alias pattern variable.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AliasPattern {
    ///The alias pattern phrase.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub phrase: ::std::option::Option<::std::string::String>,
    ///The type of alias pattern
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<AliasPatternType>,
    ///The alias pattern variable.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub variable: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&AliasPattern> for AliasPattern {
    fn from(value: &AliasPattern) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AliasPattern {
    fn default() -> Self {
        Self {
            phrase: Default::default(),
            type_: Default::default(),
            variable: Default::default(),
        }
    }
}
///The type of alias pattern
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of alias pattern",
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Extract"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AliasPatternType",
///    "values": [
///      {
///        "description": "NotSpecified is not allowed.",
///        "value": "NotSpecified"
///      },
///      {
///        "description": "Extract is the only allowed value.",
///        "value": "Extract"
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
pub enum AliasPatternType {
    NotSpecified,
    Extract,
}
impl ::std::convert::From<&Self> for AliasPatternType {
    fn from(value: &AliasPatternType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AliasPatternType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Extract => f.write_str("Extract"),
        }
    }
}
impl ::std::str::FromStr for AliasPatternType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "extract" => Ok(Self::Extract),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AliasPatternType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AliasPatternType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AliasPatternType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The type of the alias.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of the alias.",
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "PlainText",
///    "Mask"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "AliasType",
///    "values": [
///      {
///        "description": "Alias type is unknown (same as not providing alias type).",
///        "value": "NotSpecified"
///      },
///      {
///        "description": "Alias value is not secret.",
///        "value": "PlainText"
///      },
///      {
///        "description": "Alias value is secret.",
///        "value": "Mask"
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
pub enum AliasType {
    NotSpecified,
    PlainText,
    Mask,
}
impl ::std::convert::From<&Self> for AliasType {
    fn from(value: &AliasType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for AliasType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::PlainText => f.write_str("PlainText"),
            Self::Mask => f.write_str("Mask"),
        }
    }
}
impl ::std::str::FromStr for AliasType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "plaintext" => Ok(Self::PlainText),
            "mask" => Ok(Self::Mask),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AliasType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AliasType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AliasType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`ApiProfile`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "properties": {
///    "apiVersion": {
///      "description": "The API version.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "profileVersion": {
///      "description": "The profile version.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ApiProfile {
    ///The API version.
    #[serde(
        rename = "apiVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub api_version: ::std::option::Option<::std::string::String>,
    ///The profile version.
    #[serde(
        rename = "profileVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub profile_version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ApiProfile> for ApiProfile {
    fn from(value: &ApiProfile) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ApiProfile {
    fn default() -> Self {
        Self {
            api_version: Default::default(),
            profile_version: Default::default(),
        }
    }
}
///An error response for a resource management request.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An error response for a resource management request.",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/ErrorResponse"
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
    pub error: ::std::option::Option<ErrorResponse>,
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
///Common error response for all Azure Resource Manager APIs to return error details for failed operations. (This also follows the OData error response format.)
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "title": "Error Response",
///  "description": "Common error response for all Azure Resource Manager APIs to return error details for failed operations. (This also follows the OData error response format.)",
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
///        "$ref": "#/components/schemas/ErrorResponse"
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
pub struct ErrorResponse {
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
impl ::std::convert::From<&ErrorResponse> for ErrorResponse {
    fn from(value: &ErrorResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorResponse {
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
///Export resource group template request parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Export resource group template request parameters.",
///  "properties": {
///    "options": {
///      "description": "The export template options. A CSV-formatted list containing zero or more of the following: 'IncludeParameterDefaultValue', 'IncludeComments', 'SkipResourceNameParameterization', 'SkipAllParameterization'",
///      "type": "string"
///    },
///    "outputFormat": {
///      "description": "The output format for the exported resources.",
///      "type": "string",
///      "enum": [
///        "Json",
///        "Bicep"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ExportTemplateOutputFormat"
///      }
///    },
///    "resources": {
///      "description": "The IDs of the resources to filter the export by. To export all resources, supply an array with single entry '*'.",
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
pub struct ExportTemplateRequest {
    ///The export template options. A CSV-formatted list containing zero or more of the following: 'IncludeParameterDefaultValue', 'IncludeComments', 'SkipResourceNameParameterization', 'SkipAllParameterization'
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub options: ::std::option::Option<::std::string::String>,
    ///The output format for the exported resources.
    #[serde(
        rename = "outputFormat",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub output_format: ::std::option::Option<ExportTemplateRequestOutputFormat>,
    ///The IDs of the resources to filter the export by. To export all resources, supply an array with single entry '*'.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resources: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ExportTemplateRequest> for ExportTemplateRequest {
    fn from(value: &ExportTemplateRequest) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ExportTemplateRequest {
    fn default() -> Self {
        Self {
            options: Default::default(),
            output_format: Default::default(),
            resources: Default::default(),
        }
    }
}
///The output format for the exported resources.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The output format for the exported resources.",
///  "type": "string",
///  "enum": [
///    "Json",
///    "Bicep"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ExportTemplateOutputFormat"
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
pub enum ExportTemplateRequestOutputFormat {
    Json,
    Bicep,
}
impl ::std::convert::From<&Self> for ExportTemplateRequestOutputFormat {
    fn from(value: &ExportTemplateRequestOutputFormat) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ExportTemplateRequestOutputFormat {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Json => f.write_str("Json"),
            Self::Bicep => f.write_str("Bicep"),
        }
    }
}
impl ::std::str::FromStr for ExportTemplateRequestOutputFormat {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "bicep" => Ok(Self::Bicep),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExportTemplateRequestOutputFormat {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExportTemplateRequestOutputFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExportTemplateRequestOutputFormat {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Resource extended location.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource extended location.",
///  "properties": {
///    "name": {
///      "description": "The extended location name.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The extended location type.",
///      "type": "string",
///      "enum": [
///        "EdgeZone"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ExtendedLocationType"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ExtendedLocation {
    ///The extended location name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The extended location type.
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
///The extended location type.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The extended location type.",
///  "type": "string",
///  "enum": [
///    "EdgeZone"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ExtendedLocationType"
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
///Resource information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource information.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/Resource"
///    }
///  ],
///  "properties": {
///    "identity": {
///      "$ref": "#/components/schemas/Identity"
///    },
///    "kind": {
///      "description": "The kind of the resource.",
///      "type": "string",
///      "pattern": "^[-\\w\\._,\\(\\)]+$"
///    },
///    "managedBy": {
///      "description": "ID of the resource that manages this resource.",
///      "type": "string"
///    },
///    "plan": {
///      "$ref": "#/components/schemas/Plan"
///    },
///    "properties": {
///      "description": "The resource properties.",
///      "type": "object"
///    },
///    "sku": {
///      "$ref": "#/components/schemas/Sku"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GenericResource {
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Resource ID
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
    ///The kind of the resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<GenericResourceKind>,
    ///Resource location
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///ID of the resource that manages this resource.
    #[serde(
        rename = "managedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by: ::std::option::Option<::std::string::String>,
    ///Resource name
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
    pub plan: ::std::option::Option<Plan>,
    ///The resource properties.
    #[serde(
        default,
        skip_serializing_if = "::serde_json::Map::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&GenericResource> for GenericResource {
    fn from(value: &GenericResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GenericResource {
    fn default() -> Self {
        Self {
            extended_location: Default::default(),
            id: Default::default(),
            identity: Default::default(),
            kind: Default::default(),
            location: Default::default(),
            managed_by: Default::default(),
            name: Default::default(),
            plan: Default::default(),
            properties: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Resource information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource information.",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/GenericResource"
///    }
///  ],
///  "properties": {
///    "changedTime": {
///      "description": "The changed time of the resource. This is only present if requested via the $expand query parameter.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "createdTime": {
///      "description": "The created time of the resource. This is only present if requested via the $expand query parameter.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "The provisioning state of the resource. This is only present if requested via the $expand query parameter.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GenericResourceExpanded {
    ///The changed time of the resource. This is only present if requested via the $expand query parameter.
    #[serde(
        rename = "changedTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub changed_time: ::std::option::Option<::std::string::String>,
    ///The created time of the resource. This is only present if requested via the $expand query parameter.
    #[serde(
        rename = "createdTime",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created_time: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Resource ID
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
    ///The kind of the resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<GenericResourceExpandedKind>,
    ///Resource location
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///ID of the resource that manages this resource.
    #[serde(
        rename = "managedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by: ::std::option::Option<::std::string::String>,
    ///Resource name
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
    pub plan: ::std::option::Option<Plan>,
    ///The resource properties.
    #[serde(
        default,
        skip_serializing_if = "::serde_json::Map::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    ///The provisioning state of the resource. This is only present if requested via the $expand query parameter.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sku: ::std::option::Option<Sku>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&GenericResourceExpanded> for GenericResourceExpanded {
    fn from(value: &GenericResourceExpanded) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GenericResourceExpanded {
    fn default() -> Self {
        Self {
            changed_time: Default::default(),
            created_time: Default::default(),
            extended_location: Default::default(),
            id: Default::default(),
            identity: Default::default(),
            kind: Default::default(),
            location: Default::default(),
            managed_by: Default::default(),
            name: Default::default(),
            plan: Default::default(),
            properties: Default::default(),
            provisioning_state: Default::default(),
            sku: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///The kind of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The kind of the resource.",
///  "type": "string",
///  "pattern": "^[-\\w\\._,\\(\\)]+$"
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct GenericResourceExpandedKind(::std::string::String);
impl ::std::ops::Deref for GenericResourceExpandedKind {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<GenericResourceExpandedKind> for ::std::string::String {
    fn from(value: GenericResourceExpandedKind) -> Self {
        value.0
    }
}
impl ::std::convert::From<&GenericResourceExpandedKind> for GenericResourceExpandedKind {
    fn from(value: &GenericResourceExpandedKind) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for GenericResourceExpandedKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^[-\\w\\._,\\(\\)]+$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[-\\w\\._,\\(\\)]+$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for GenericResourceExpandedKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GenericResourceExpandedKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GenericResourceExpandedKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for GenericResourceExpandedKind {
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
///Resource filter.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource filter.",
///  "properties": {
///    "resourceType": {
///      "description": "The resource type.",
///      "type": "string"
///    },
///    "tagname": {
///      "description": "The tag name.",
///      "type": "string"
///    },
///    "tagvalue": {
///      "description": "The tag value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct GenericResourceFilter {
    ///The resource type.
    #[serde(
        rename = "resourceType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_type: ::std::option::Option<::std::string::String>,
    ///The tag name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tagname: ::std::option::Option<::std::string::String>,
    ///The tag value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tagvalue: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&GenericResourceFilter> for GenericResourceFilter {
    fn from(value: &GenericResourceFilter) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for GenericResourceFilter {
    fn default() -> Self {
        Self {
            resource_type: Default::default(),
            tagname: Default::default(),
            tagvalue: Default::default(),
        }
    }
}
///The kind of the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The kind of the resource.",
///  "type": "string",
///  "pattern": "^[-\\w\\._,\\(\\)]+$"
///}
/// ```
/// </details>
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct GenericResourceKind(::std::string::String);
impl ::std::ops::Deref for GenericResourceKind {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<GenericResourceKind> for ::std::string::String {
    fn from(value: GenericResourceKind) -> Self {
        value.0
    }
}
impl ::std::convert::From<&GenericResourceKind> for GenericResourceKind {
    fn from(value: &GenericResourceKind) -> Self {
        value.clone()
    }
}
impl ::std::str::FromStr for GenericResourceKind {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| ::regress::Regex::new("^[-\\w\\._,\\(\\)]+$").unwrap());
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[-\\w\\._,\\(\\)]+$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for GenericResourceKind {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for GenericResourceKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for GenericResourceKind {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for GenericResourceKind {
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
///Identity for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Identity for the resource.",
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
///      "description": "The list of user identities associated with the resource. The user identity dictionary key references will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.",
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
///        },
///        "x-ms-client-name": "IdentityUserAssignedIdentitiesValue"
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
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<IdentityType>,
    ///The list of user identities associated with the resource. The user identity dictionary key references will be ARM resource ids in the form: '/subscriptions/{subscriptionId}/resourceGroups/{resourceGroupName}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{identityName}'.
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub user_assigned_identities:
        ::std::collections::HashMap<::std::string::String, IdentityUserAssignedIdentitiesValue>,
}
impl ::std::convert::From<&Identity> for Identity {
    fn from(value: &Identity) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Identity {
    fn default() -> Self {
        Self {
            principal_id: Default::default(),
            tenant_id: Default::default(),
            type_: Default::default(),
            user_assigned_identities: Default::default(),
        }
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
pub enum IdentityType {
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned, UserAssigned")]
    SystemAssignedUserAssigned,
    None,
}
impl ::std::convert::From<&Self> for IdentityType {
    fn from(value: &IdentityType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for IdentityType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::SystemAssigned => f.write_str("SystemAssigned"),
            Self::UserAssigned => f.write_str("UserAssigned"),
            Self::SystemAssignedUserAssigned => f.write_str("SystemAssigned, UserAssigned"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for IdentityType {
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
///`IdentityUserAssignedIdentitiesValue`
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
///  },
///  "x-ms-client-name": "IdentityUserAssignedIdentitiesValue"
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IdentityUserAssignedIdentitiesValue {
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
impl ::std::convert::From<&IdentityUserAssignedIdentitiesValue>
    for IdentityUserAssignedIdentitiesValue
{
    fn from(value: &IdentityUserAssignedIdentitiesValue) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IdentityUserAssignedIdentitiesValue {
    fn default() -> Self {
        Self {
            client_id: Default::default(),
            principal_id: Default::default(),
        }
    }
}
///Microsoft.Resources operation
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Microsoft.Resources operation",
///  "type": "object",
///  "properties": {
///    "display": {
///      "description": "The object that represents the operation.",
///      "properties": {
///        "description": {
///          "description": "Description of the operation.",
///          "type": "string"
///        },
///        "operation": {
///          "description": "Operation type: Read, write, delete, etc.",
///          "type": "string"
///        },
///        "provider": {
///          "description": "Service provider: Microsoft.Resources",
///          "type": "string"
///        },
///        "resource": {
///          "description": "Resource on which the operation is performed: Profile, endpoint, etc.",
///          "type": "string"
///        }
///      }
///    },
///    "name": {
///      "description": "Operation name: {provider}/{resource}/{operation}",
///      "type": "string"
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
        }
    }
}
///The object that represents the operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The object that represents the operation.",
///  "properties": {
///    "description": {
///      "description": "Description of the operation.",
///      "type": "string"
///    },
///    "operation": {
///      "description": "Operation type: Read, write, delete, etc.",
///      "type": "string"
///    },
///    "provider": {
///      "description": "Service provider: Microsoft.Resources",
///      "type": "string"
///    },
///    "resource": {
///      "description": "Resource on which the operation is performed: Profile, endpoint, etc.",
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
    ///Operation type: Read, write, delete, etc.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<::std::string::String>,
    ///Service provider: Microsoft.Resources
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
    ///Resource on which the operation is performed: Profile, endpoint, etc.
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
///Result of the request to list Microsoft.Resources operations. It contains a list of operations and a URL link to get the next set of results.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Result of the request to list Microsoft.Resources operations. It contains a list of operations and a URL link to get the next set of results.",
///  "properties": {
///    "nextLink": {
///      "description": "URL to get the next set of operation list results if there are any.",
///      "type": "string"
///    },
///    "value": {
///      "description": "List of Microsoft.Resources operations.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Operation"
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
pub struct OperationListResult {
    ///URL to get the next set of operation list results if there are any.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///List of Microsoft.Resources operations.
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
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Role definition permissions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Role definition permissions.",
///  "readOnly": true,
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
///Plan for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Plan for the resource.",
///  "properties": {
///    "name": {
///      "description": "The plan ID.",
///      "type": "string"
///    },
///    "product": {
///      "description": "The offer ID.",
///      "type": "string"
///    },
///    "promotionCode": {
///      "description": "The promotion code.",
///      "type": "string"
///    },
///    "publisher": {
///      "description": "The publisher ID.",
///      "type": "string"
///    },
///    "version": {
///      "description": "The plan's version.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Plan {
    ///The plan ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The offer ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub product: ::std::option::Option<::std::string::String>,
    ///The promotion code.
    #[serde(
        rename = "promotionCode",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub promotion_code: ::std::option::Option<::std::string::String>,
    ///The publisher ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub publisher: ::std::option::Option<::std::string::String>,
    ///The plan's version.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Plan> for Plan {
    fn from(value: &Plan) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Plan {
    fn default() -> Self {
        Self {
            name: Default::default(),
            product: Default::default(),
            promotion_code: Default::default(),
            publisher: Default::default(),
            version: Default::default(),
        }
    }
}
///Resource provider information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource provider information.",
///  "properties": {
///    "id": {
///      "description": "The provider ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "namespace": {
///      "description": "The namespace of the resource provider.",
///      "type": "string"
///    },
///    "providerAuthorizationConsentState": {
///      "description": "The provider authorization consent state.",
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Required",
///        "NotRequired",
///        "Consented"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProviderAuthorizationConsentState"
///      }
///    },
///    "registrationPolicy": {
///      "description": "The registration policy of the resource provider.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "registrationState": {
///      "description": "The registration state of the resource provider.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "resourceTypes": {
///      "description": "The collection of provider resource types.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ProviderResourceType"
///      },
///      "x-ms-identifiers": [
///        "resourceType"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Provider {
    ///The provider ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The namespace of the resource provider.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub namespace: ::std::option::Option<::std::string::String>,
    ///The provider authorization consent state.
    #[serde(
        rename = "providerAuthorizationConsentState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider_authorization_consent_state:
        ::std::option::Option<ProviderProviderAuthorizationConsentState>,
    ///The registration policy of the resource provider.
    #[serde(
        rename = "registrationPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registration_policy: ::std::option::Option<::std::string::String>,
    ///The registration state of the resource provider.
    #[serde(
        rename = "registrationState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registration_state: ::std::option::Option<::std::string::String>,
    ///The collection of provider resource types.
    #[serde(
        rename = "resourceTypes",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_types: ::std::vec::Vec<ProviderResourceType>,
}
impl ::std::convert::From<&Provider> for Provider {
    fn from(value: &Provider) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Provider {
    fn default() -> Self {
        Self {
            id: Default::default(),
            namespace: Default::default(),
            provider_authorization_consent_state: Default::default(),
            registration_policy: Default::default(),
            registration_state: Default::default(),
            resource_types: Default::default(),
        }
    }
}
///The provider consent.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider consent.",
///  "type": "object",
///  "properties": {
///    "consentToAuthorization": {
///      "description": "A value indicating whether authorization is consented or not.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderConsentDefinition {
    ///A value indicating whether authorization is consented or not.
    #[serde(
        rename = "consentToAuthorization",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub consent_to_authorization: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ProviderConsentDefinition> for ProviderConsentDefinition {
    fn from(value: &ProviderConsentDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderConsentDefinition {
    fn default() -> Self {
        Self {
            consent_to_authorization: Default::default(),
        }
    }
}
///The provider extended location.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider extended location. ",
///  "properties": {
///    "extendedLocations": {
///      "description": "The extended locations for the azure location.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "location": {
///      "description": "The azure location.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The extended location type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderExtendedLocation {
    ///The extended locations for the azure location.
    #[serde(
        rename = "extendedLocations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_locations: ::std::vec::Vec<::std::string::String>,
    ///The azure location.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///The extended location type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ProviderExtendedLocation> for ProviderExtendedLocation {
    fn from(value: &ProviderExtendedLocation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderExtendedLocation {
    fn default() -> Self {
        Self {
            extended_locations: Default::default(),
            location: Default::default(),
            type_: Default::default(),
        }
    }
}
///List of resource providers.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of resource providers.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of resource providers.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Provider"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of resource providers.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Provider>,
}
impl ::std::convert::From<&ProviderListResult> for ProviderListResult {
    fn from(value: &ProviderListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The provider permission
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider permission",
///  "type": "object",
///  "properties": {
///    "applicationId": {
///      "description": "The application id.",
///      "type": "string"
///    },
///    "managedByRoleDefinition": {
///      "$ref": "#/components/schemas/RoleDefinition"
///    },
///    "providerAuthorizationConsentState": {
///      "description": "The provider authorization consent state.",
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Required",
///        "NotRequired",
///        "Consented"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProviderAuthorizationConsentState"
///      }
///    },
///    "roleDefinition": {
///      "$ref": "#/components/schemas/RoleDefinition"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderPermission {
    ///The application id.
    #[serde(
        rename = "applicationId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub application_id: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "managedByRoleDefinition",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by_role_definition: ::std::option::Option<RoleDefinition>,
    ///The provider authorization consent state.
    #[serde(
        rename = "providerAuthorizationConsentState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider_authorization_consent_state:
        ::std::option::Option<ProviderPermissionProviderAuthorizationConsentState>,
    #[serde(
        rename = "roleDefinition",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub role_definition: ::std::option::Option<RoleDefinition>,
}
impl ::std::convert::From<&ProviderPermission> for ProviderPermission {
    fn from(value: &ProviderPermission) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderPermission {
    fn default() -> Self {
        Self {
            application_id: Default::default(),
            managed_by_role_definition: Default::default(),
            provider_authorization_consent_state: Default::default(),
            role_definition: Default::default(),
        }
    }
}
///List of provider permissions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of provider permissions.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of provider permissions.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ProviderPermission"
///      },
///      "x-ms-identifiers": []
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderPermissionListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of provider permissions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ProviderPermission>,
}
impl ::std::convert::From<&ProviderPermissionListResult> for ProviderPermissionListResult {
    fn from(value: &ProviderPermissionListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderPermissionListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The provider authorization consent state.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider authorization consent state.",
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Required",
///    "NotRequired",
///    "Consented"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProviderAuthorizationConsentState"
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
pub enum ProviderPermissionProviderAuthorizationConsentState {
    NotSpecified,
    Required,
    NotRequired,
    Consented,
}
impl ::std::convert::From<&Self> for ProviderPermissionProviderAuthorizationConsentState {
    fn from(value: &ProviderPermissionProviderAuthorizationConsentState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ProviderPermissionProviderAuthorizationConsentState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Required => f.write_str("Required"),
            Self::NotRequired => f.write_str("NotRequired"),
            Self::Consented => f.write_str("Consented"),
        }
    }
}
impl ::std::str::FromStr for ProviderPermissionProviderAuthorizationConsentState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "required" => Ok(Self::Required),
            "notrequired" => Ok(Self::NotRequired),
            "consented" => Ok(Self::Consented),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ProviderPermissionProviderAuthorizationConsentState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
    for ProviderPermissionProviderAuthorizationConsentState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
    for ProviderPermissionProviderAuthorizationConsentState
{
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The provider authorization consent state.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider authorization consent state.",
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Required",
///    "NotRequired",
///    "Consented"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ProviderAuthorizationConsentState"
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
pub enum ProviderProviderAuthorizationConsentState {
    NotSpecified,
    Required,
    NotRequired,
    Consented,
}
impl ::std::convert::From<&Self> for ProviderProviderAuthorizationConsentState {
    fn from(value: &ProviderProviderAuthorizationConsentState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ProviderProviderAuthorizationConsentState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Required => f.write_str("Required"),
            Self::NotRequired => f.write_str("NotRequired"),
            Self::Consented => f.write_str("Consented"),
        }
    }
}
impl ::std::str::FromStr for ProviderProviderAuthorizationConsentState {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "required" => Ok(Self::Required),
            "notrequired" => Ok(Self::NotRequired),
            "consented" => Ok(Self::Consented),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ProviderProviderAuthorizationConsentState {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ProviderProviderAuthorizationConsentState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ProviderProviderAuthorizationConsentState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The provider registration definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The provider registration definition.",
///  "type": "object",
///  "properties": {
///    "thirdPartyProviderConsent": {
///      "$ref": "#/components/schemas/ProviderConsentDefinition"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderRegistrationRequest {
    #[serde(
        rename = "thirdPartyProviderConsent",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub third_party_provider_consent: ::std::option::Option<ProviderConsentDefinition>,
}
impl ::std::convert::From<&ProviderRegistrationRequest> for ProviderRegistrationRequest {
    fn from(value: &ProviderRegistrationRequest) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderRegistrationRequest {
    fn default() -> Self {
        Self {
            third_party_provider_consent: Default::default(),
        }
    }
}
///Resource type managed by the resource provider.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource type managed by the resource provider.",
///  "properties": {
///    "aliases": {
///      "description": "The aliases that are supported by this resource type.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Alias"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "apiProfiles": {
///      "description": "The API profiles for the resource provider.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ApiProfile"
///      },
///      "x-ms-identifiers": [
///        "apiVersion",
///        "profileVersion"
///      ]
///    },
///    "apiVersions": {
///      "description": "The API version.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "capabilities": {
///      "description": "The additional capabilities offered by this resource type.",
///      "type": "string"
///    },
///    "defaultApiVersion": {
///      "description": "The default API version.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "locationMappings": {
///      "description": "The location mappings that are supported by this resource type.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ProviderExtendedLocation"
///      },
///      "x-ms-identifiers": [
///        "location",
///        "type"
///      ]
///    },
///    "locations": {
///      "description": "The collection of locations where this resource type can be created.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "properties": {
///      "description": "The properties.",
///      "type": "object",
///      "additionalProperties": {
///        "description": "The additional properties. ",
///        "type": "string"
///      }
///    },
///    "resourceType": {
///      "description": "The resource type.",
///      "type": "string"
///    },
///    "zoneMappings": {
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ZoneMapping"
///      },
///      "x-ms-identifiers": [
///        "location"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderResourceType {
    ///The aliases that are supported by this resource type.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub aliases: ::std::vec::Vec<Alias>,
    ///The API profiles for the resource provider.
    #[serde(
        rename = "apiProfiles",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub api_profiles: ::std::vec::Vec<ApiProfile>,
    ///The API version.
    #[serde(
        rename = "apiVersions",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub api_versions: ::std::vec::Vec<::std::string::String>,
    ///The additional capabilities offered by this resource type.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub capabilities: ::std::option::Option<::std::string::String>,
    ///The default API version.
    #[serde(
        rename = "defaultApiVersion",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_api_version: ::std::option::Option<::std::string::String>,
    ///The location mappings that are supported by this resource type.
    #[serde(
        rename = "locationMappings",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location_mappings: ::std::vec::Vec<ProviderExtendedLocation>,
    ///The collection of locations where this resource type can be created.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub locations: ::std::vec::Vec<::std::string::String>,
    ///The properties.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The resource type.
    #[serde(
        rename = "resourceType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource_type: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "zoneMappings",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zone_mappings: ::std::vec::Vec<ZoneMapping>,
}
impl ::std::convert::From<&ProviderResourceType> for ProviderResourceType {
    fn from(value: &ProviderResourceType) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderResourceType {
    fn default() -> Self {
        Self {
            aliases: Default::default(),
            api_profiles: Default::default(),
            api_versions: Default::default(),
            capabilities: Default::default(),
            default_api_version: Default::default(),
            location_mappings: Default::default(),
            locations: Default::default(),
            properties: Default::default(),
            resource_type: Default::default(),
            zone_mappings: Default::default(),
        }
    }
}
///List of resource types of a resource provider.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of resource types of a resource provider.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of resource types.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ProviderResourceType"
///      },
///      "x-ms-identifiers": [
///        "resourceType"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ProviderResourceTypeListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of resource types.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ProviderResourceType>,
}
impl ::std::convert::From<&ProviderResourceTypeListResult> for ProviderResourceTypeListResult {
    fn from(value: &ProviderResourceTypeListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ProviderResourceTypeListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Specified resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Specified resource.",
///  "properties": {
///    "extendedLocation": {
///      "$ref": "#/components/schemas/ExtendedLocation"
///    },
///    "id": {
///      "description": "Resource ID",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "Resource location",
///      "type": "string"
///    },
///    "name": {
///      "description": "Resource name",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tags": {
///      "description": "Resource tags",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "Resource type",
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
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub extended_location: ::std::option::Option<ExtendedLocation>,
    ///Resource ID
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Resource location
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    ///Resource name
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Resource tags
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Resource type
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
            extended_location: Default::default(),
            id: Default::default(),
            location: Default::default(),
            name: Default::default(),
            tags: Default::default(),
            type_: Default::default(),
        }
    }
}
///Resource group information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource group information.",
///  "required": [
///    "location"
///  ],
///  "properties": {
///    "id": {
///      "description": "The ID of the resource group.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "location": {
///      "description": "The location of the resource group. It cannot be changed after the resource group has been created. It must be one of the supported Azure locations.",
///      "type": "string"
///    },
///    "managedBy": {
///      "description": "The ID of the resource that manages this resource group.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource group.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ResourceGroupProperties"
///    },
///    "tags": {
///      "description": "The tags attached to the resource group.",
///      "type": "object",
///      "additionalProperties": {
///        "description": "The additional properties. ",
///        "type": "string"
///      }
///    },
///    "type": {
///      "description": "The type of the resource group.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceGroup {
    ///The ID of the resource group.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The location of the resource group. It cannot be changed after the resource group has been created. It must be one of the supported Azure locations.
    pub location: ::std::string::String,
    ///The ID of the resource that manages this resource group.
    #[serde(
        rename = "managedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by: ::std::option::Option<::std::string::String>,
    ///The name of the resource group.
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
    pub properties: ::std::option::Option<ResourceGroupProperties>,
    ///The tags attached to the resource group.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The type of the resource group.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceGroup> for ResourceGroup {
    fn from(value: &ResourceGroup) -> Self {
        value.clone()
    }
}
///Resource group export result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource group export result.",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/ErrorResponse"
///    },
///    "output": {
///      "description": "The formatted export content. Used if outputFormat is set to 'Bicep'.",
///      "type": "string"
///    },
///    "template": {
///      "description": "The template content. Used if outputFormat is empty or set to 'Json'.",
///      "type": "object"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceGroupExportResult {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<ErrorResponse>,
    ///The formatted export content. Used if outputFormat is set to 'Bicep'.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub output: ::std::option::Option<::std::string::String>,
    ///The template content. Used if outputFormat is empty or set to 'Json'.
    #[serde(
        default,
        skip_serializing_if = "::serde_json::Map::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub template: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
}
impl ::std::convert::From<&ResourceGroupExportResult> for ResourceGroupExportResult {
    fn from(value: &ResourceGroupExportResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceGroupExportResult {
    fn default() -> Self {
        Self {
            error: Default::default(),
            output: Default::default(),
            template: Default::default(),
        }
    }
}
///Resource group filter.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource group filter.",
///  "properties": {
///    "tagName": {
///      "description": "The tag name.",
///      "type": "string"
///    },
///    "tagValue": {
///      "description": "The tag value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceGroupFilter {
    ///The tag name.
    #[serde(
        rename = "tagName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag_name: ::std::option::Option<::std::string::String>,
    ///The tag value.
    #[serde(
        rename = "tagValue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag_value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceGroupFilter> for ResourceGroupFilter {
    fn from(value: &ResourceGroupFilter) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceGroupFilter {
    fn default() -> Self {
        Self {
            tag_name: Default::default(),
            tag_value: Default::default(),
        }
    }
}
///List of resource groups.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of resource groups.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of resource groups.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ResourceGroup"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceGroupListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of resource groups.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<ResourceGroup>,
}
impl ::std::convert::From<&ResourceGroupListResult> for ResourceGroupListResult {
    fn from(value: &ResourceGroupListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceGroupListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Resource group information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource group information.",
///  "properties": {
///    "managedBy": {
///      "description": "The ID of the resource that manages this resource group.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the resource group.",
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/ResourceGroupProperties"
///    },
///    "tags": {
///      "description": "The tags attached to the resource group.",
///      "type": "object",
///      "additionalProperties": {
///        "description": "The additional properties. ",
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceGroupPatchable {
    ///The ID of the resource that manages this resource group.
    #[serde(
        rename = "managedBy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by: ::std::option::Option<::std::string::String>,
    ///The name of the resource group.
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
    pub properties: ::std::option::Option<ResourceGroupProperties>,
    ///The tags attached to the resource group.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&ResourceGroupPatchable> for ResourceGroupPatchable {
    fn from(value: &ResourceGroupPatchable) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceGroupPatchable {
    fn default() -> Self {
        Self {
            managed_by: Default::default(),
            name: Default::default(),
            properties: Default::default(),
            tags: Default::default(),
        }
    }
}
///The resource group properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The resource group properties.",
///  "properties": {
///    "provisioningState": {
///      "description": "The provisioning state. ",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceGroupProperties {
    ///The provisioning state.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceGroupProperties> for ResourceGroupProperties {
    fn from(value: &ResourceGroupProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceGroupProperties {
    fn default() -> Self {
        Self {
            provisioning_state: Default::default(),
        }
    }
}
///List of resource groups.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of resource groups.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of resources.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/GenericResourceExpanded"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of resources.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<GenericResourceExpanded>,
}
impl ::std::convert::From<&ResourceListResult> for ResourceListResult {
    fn from(value: &ResourceListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Resource provider operation's display properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Resource provider operation's display properties.",
///  "properties": {
///    "description": {
///      "description": "Operation description.",
///      "type": "string"
///    },
///    "operation": {
///      "description": "Resource provider operation.",
///      "type": "string"
///    },
///    "provider": {
///      "description": "Operation provider.",
///      "type": "string"
///    },
///    "publisher": {
///      "description": "Operation description.",
///      "type": "string"
///    },
///    "resource": {
///      "description": "Operation resource.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourceProviderOperationDisplayProperties {
    ///Operation description.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///Resource provider operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<::std::string::String>,
    ///Operation provider.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
    ///Operation description.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub publisher: ::std::option::Option<::std::string::String>,
    ///Operation resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resource: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourceProviderOperationDisplayProperties>
    for ResourceProviderOperationDisplayProperties
{
    fn from(value: &ResourceProviderOperationDisplayProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourceProviderOperationDisplayProperties {
    fn default() -> Self {
        Self {
            description: Default::default(),
            operation: Default::default(),
            provider: Default::default(),
            publisher: Default::default(),
            resource: Default::default(),
        }
    }
}
///Parameters of move resources.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters of move resources.",
///  "properties": {
///    "resources": {
///      "description": "The IDs of the resources.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "targetResourceGroup": {
///      "description": "The target resource group.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ResourcesMoveInfo {
    ///The IDs of the resources.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub resources: ::std::vec::Vec<::std::string::String>,
    ///The target resource group.
    #[serde(
        rename = "targetResourceGroup",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target_resource_group: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ResourcesMoveInfo> for ResourcesMoveInfo {
    fn from(value: &ResourcesMoveInfo) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ResourcesMoveInfo {
    fn default() -> Self {
        Self {
            resources: Default::default(),
            target_resource_group: Default::default(),
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
///    "id": {
///      "description": "The role definition ID.",
///      "type": "string"
///    },
///    "isServiceRole": {
///      "description": "If this is a service role.",
///      "type": "boolean"
///    },
///    "name": {
///      "description": "The role definition name.",
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
///    "scopes": {
///      "description": "Role definition assignable scopes.",
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
pub struct RoleDefinition {
    ///The role definition ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///If this is a service role.
    #[serde(
        rename = "isServiceRole",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub is_service_role: ::std::option::Option<bool>,
    ///The role definition name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Role definition permissions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub permissions: ::std::vec::Vec<Permission>,
    ///Role definition assignable scopes.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scopes: ::std::vec::Vec<::std::string::String>,
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
            is_service_role: Default::default(),
            name: Default::default(),
            permissions: Default::default(),
            scopes: Default::default(),
        }
    }
}
///SKU for the resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "SKU for the resource.",
///  "properties": {
///    "capacity": {
///      "description": "The SKU capacity.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "family": {
///      "description": "The SKU family.",
///      "type": "string"
///    },
///    "model": {
///      "description": "The SKU model.",
///      "type": "string"
///    },
///    "name": {
///      "description": "The SKU name.",
///      "type": "string"
///    },
///    "size": {
///      "description": "The SKU size.",
///      "type": "string"
///    },
///    "tier": {
///      "description": "The SKU tier.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Sku {
    ///The SKU capacity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub capacity: ::std::option::Option<i32>,
    ///The SKU family.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub family: ::std::option::Option<::std::string::String>,
    ///The SKU model.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub model: ::std::option::Option<::std::string::String>,
    ///The SKU name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The SKU size.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub size: ::std::option::Option<::std::string::String>,
    ///The SKU tier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tier: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Sku> for Sku {
    fn from(value: &Sku) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Sku {
    fn default() -> Self {
        Self {
            capacity: Default::default(),
            family: Default::default(),
            model: Default::default(),
            name: Default::default(),
            size: Default::default(),
            tier: Default::default(),
        }
    }
}
///Sub-resource.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Sub-resource.",
///  "properties": {
///    "id": {
///      "description": "Resource ID",
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SubResource {
    ///Resource ID
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
///Tag count.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tag count.",
///  "properties": {
///    "type": {
///      "description": "Type of count.",
///      "type": "string"
///    },
///    "value": {
///      "description": "Value of count.",
///      "type": "integer"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagCount {
    ///Type of count.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
    ///Value of count.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<i64>,
}
impl ::std::convert::From<&TagCount> for TagCount {
    fn from(value: &TagCount) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagCount {
    fn default() -> Self {
        Self {
            type_: Default::default(),
            value: Default::default(),
        }
    }
}
///Tag details.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tag details.",
///  "properties": {
///    "count": {
///      "$ref": "#/components/schemas/TagCount"
///    },
///    "id": {
///      "description": "The tag name ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tagName": {
///      "description": "The tag name.",
///      "type": "string"
///    },
///    "values": {
///      "description": "The list of tag values.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TagValue"
///      }
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagDetails {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub count: ::std::option::Option<TagCount>,
    ///The tag name ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The tag name.
    #[serde(
        rename = "tagName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag_name: ::std::option::Option<::std::string::String>,
    ///The list of tag values.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub values: ::std::vec::Vec<TagValue>,
}
impl ::std::convert::From<&TagDetails> for TagDetails {
    fn from(value: &TagDetails) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagDetails {
    fn default() -> Self {
        Self {
            count: Default::default(),
            id: Default::default(),
            tag_name: Default::default(),
            values: Default::default(),
        }
    }
}
///Tag information.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Tag information.",
///  "properties": {
///    "count": {
///      "$ref": "#/components/schemas/TagCount"
///    },
///    "id": {
///      "description": "The tag value ID.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tagValue": {
///      "description": "The tag value.",
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagValue {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub count: ::std::option::Option<TagCount>,
    ///The tag value ID.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The tag value.
    #[serde(
        rename = "tagValue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tag_value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TagValue> for TagValue {
    fn from(value: &TagValue) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagValue {
    fn default() -> Self {
        Self {
            count: Default::default(),
            id: Default::default(),
            tag_value: Default::default(),
        }
    }
}
///A dictionary of name and value pairs.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A dictionary of name and value pairs.",
///  "properties": {
///    "tags": {
///      "type": "object",
///      "additionalProperties": {
///        "description": "The tag value.",
///        "type": "string"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Tags {
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&Tags> for Tags {
    fn from(value: &Tags) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Tags {
    fn default() -> Self {
        Self {
            tags: Default::default(),
        }
    }
}
///List of subscription tags.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "List of subscription tags.",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to use for getting the next set of results.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "An array of tags.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/TagDetails"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagsListResult {
    ///The URL to use for getting the next set of results.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///An array of tags.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<TagDetails>,
}
impl ::std::convert::From<&TagsListResult> for TagsListResult {
    fn from(value: &TagsListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagsListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Wrapper resource for tags patch API request only.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Wrapper resource for tags patch API request only.",
///  "type": "object",
///  "properties": {
///    "operation": {
///      "description": "The operation type for the patch API.",
///      "type": "string",
///      "enum": [
///        "Replace",
///        "Merge",
///        "Delete"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "tagsPatchOperation",
///        "values": [
///          {
///            "description": "The 'replace' option replaces the entire set of existing tags with a new set.",
///            "value": "Replace"
///          },
///          {
///            "description": "The 'merge' option allows adding tags with new names and updating the values of tags with existing names.",
///            "value": "Merge"
///          },
///          {
///            "description": "The 'delete' option allows selectively deleting tags based on given names or name/value pairs.",
///            "value": "Delete"
///          }
///        ]
///      }
///    },
///    "properties": {
///      "$ref": "#/components/schemas/Tags"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagsPatchResource {
    ///The operation type for the patch API.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<TagsPatchResourceOperation>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<Tags>,
}
impl ::std::convert::From<&TagsPatchResource> for TagsPatchResource {
    fn from(value: &TagsPatchResource) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for TagsPatchResource {
    fn default() -> Self {
        Self {
            operation: Default::default(),
            properties: Default::default(),
        }
    }
}
///The operation type for the patch API.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The operation type for the patch API.",
///  "type": "string",
///  "enum": [
///    "Replace",
///    "Merge",
///    "Delete"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "tagsPatchOperation",
///    "values": [
///      {
///        "description": "The 'replace' option replaces the entire set of existing tags with a new set.",
///        "value": "Replace"
///      },
///      {
///        "description": "The 'merge' option allows adding tags with new names and updating the values of tags with existing names.",
///        "value": "Merge"
///      },
///      {
///        "description": "The 'delete' option allows selectively deleting tags based on given names or name/value pairs.",
///        "value": "Delete"
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
pub enum TagsPatchResourceOperation {
    Replace,
    Merge,
    Delete,
}
impl ::std::convert::From<&Self> for TagsPatchResourceOperation {
    fn from(value: &TagsPatchResourceOperation) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for TagsPatchResourceOperation {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Replace => f.write_str("Replace"),
            Self::Merge => f.write_str("Merge"),
            Self::Delete => f.write_str("Delete"),
        }
    }
}
impl ::std::str::FromStr for TagsPatchResourceOperation {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "replace" => Ok(Self::Replace),
            "merge" => Ok(Self::Merge),
            "delete" => Ok(Self::Delete),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TagsPatchResourceOperation {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TagsPatchResourceOperation {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TagsPatchResourceOperation {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Wrapper resource for tags API requests and responses.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Wrapper resource for tags API requests and responses.",
///  "type": "object",
///  "required": [
///    "properties"
///  ],
///  "properties": {
///    "id": {
///      "description": "The ID of the tags wrapper resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the tags wrapper resource.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/Tags"
///    },
///    "type": {
///      "description": "The type of the tags wrapper resource.",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct TagsResource {
    ///The ID of the tags wrapper resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the tags wrapper resource.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    pub properties: Tags,
    ///The type of the tags wrapper resource.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&TagsResource> for TagsResource {
    fn from(value: &TagsResource) -> Self {
        value.clone()
    }
}
///`ZoneMapping`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "location": {
///      "description": "The location of the zone mapping.",
///      "type": "string"
///    },
///    "zones": {
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
pub struct ZoneMapping {
    ///The location of the zone mapping.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub location: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zones: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&ZoneMapping> for ZoneMapping {
    fn from(value: &ZoneMapping) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ZoneMapping {
    fn default() -> Self {
        Self {
            location: Default::default(),
            zones: Default::default(),
        }
    }
}
