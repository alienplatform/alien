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
///The object attributes managed by the KeyVault service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The object attributes managed by the KeyVault service.",
///  "type": "object",
///  "properties": {
///    "created": {
///      "description": "Creation time in UTC.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "enabled": {
///      "description": "Determines whether the object is enabled.",
///      "type": "boolean"
///    },
///    "exp": {
///      "description": "Expiry date in UTC.",
///      "type": "integer",
///      "format": "unixtime",
///      "x-ms-client-name": "Expires"
///    },
///    "nbf": {
///      "description": "Not before date in UTC.",
///      "type": "integer",
///      "format": "unixtime",
///      "x-ms-client-name": "NotBefore"
///    },
///    "updated": {
///      "description": "Last updated time in UTC.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Attributes {
    ///Creation time in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created: ::std::option::Option<i64>,
    ///Determines whether the object is enabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///Expiry date in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exp: ::std::option::Option<i64>,
    ///Not before date in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub nbf: ::std::option::Option<i64>,
    ///Last updated time in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated: ::std::option::Option<i64>,
}
impl ::std::convert::From<&Attributes> for Attributes {
    fn from(value: &Attributes) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Attributes {
    fn default() -> Self {
        Self {
            created: Default::default(),
            enabled: Default::default(),
            exp: Default::default(),
            nbf: Default::default(),
            updated: Default::default(),
        }
    }
}
///The backup secret result, containing the backup blob.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The backup secret result, containing the backup blob.",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "The backup blob containing the backed up secret.",
///      "readOnly": true,
///      "type": "string",
///      "format": "base64url"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BackupSecretResult {
    ///The backup blob containing the backed up secret.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&BackupSecretResult> for BackupSecretResult {
    fn from(value: &BackupSecretResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BackupSecretResult {
    fn default() -> Self {
        Self {
            value: Default::default(),
        }
    }
}
///A Deleted Secret consisting of its previous id, attributes and its tags, as well as information on when it will be purged.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A Deleted Secret consisting of its previous id, attributes and its tags, as well as information on when it will be purged.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/SecretAttributes"
///    },
///    "contentType": {
///      "description": "The content type of the secret.",
///      "type": "string"
///    },
///    "deletedDate": {
///      "description": "The time when the secret was deleted, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "id": {
///      "description": "The secret id.",
///      "type": "string"
///    },
///    "kid": {
///      "description": "If this is a secret backing a KV certificate, then this field specifies the corresponding key backing the KV certificate.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "managed": {
///      "description": "True if the secret's lifetime is managed by key vault. If this is a secret backing a certificate, then managed will be true.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "recoveryId": {
///      "description": "The url of the recovery object, used to identify and recover the deleted secret.",
///      "type": "string"
///    },
///    "scheduledPurgeDate": {
///      "description": "The time when the secret is scheduled to be purged, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "value": {
///      "description": "The secret value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedSecretBundle {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<SecretAttributes>,
    ///The content type of the secret.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///The time when the secret was deleted, in UTC
    #[serde(
        rename = "deletedDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deleted_date: ::std::option::Option<i64>,
    ///The secret id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///If this is a secret backing a KV certificate, then this field specifies the corresponding key backing the KV certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kid: ::std::option::Option<::std::string::String>,
    ///True if the secret's lifetime is managed by key vault. If this is a secret backing a certificate, then managed will be true.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed: ::std::option::Option<bool>,
    ///The url of the recovery object, used to identify and recover the deleted secret.
    #[serde(
        rename = "recoveryId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub recovery_id: ::std::option::Option<::std::string::String>,
    ///The time when the secret is scheduled to be purged, in UTC
    #[serde(
        rename = "scheduledPurgeDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scheduled_purge_date: ::std::option::Option<i64>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The secret value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedSecretBundle> for DeletedSecretBundle {
    fn from(value: &DeletedSecretBundle) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedSecretBundle {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            content_type: Default::default(),
            deleted_date: Default::default(),
            id: Default::default(),
            kid: Default::default(),
            managed: Default::default(),
            recovery_id: Default::default(),
            scheduled_purge_date: Default::default(),
            tags: Default::default(),
            value: Default::default(),
        }
    }
}
///The deleted secret item containing metadata about the deleted secret.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The deleted secret item containing metadata about the deleted secret.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/SecretAttributes"
///    },
///    "contentType": {
///      "description": "Type of the secret value such as a password.",
///      "type": "string"
///    },
///    "deletedDate": {
///      "description": "The time when the secret was deleted, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "id": {
///      "description": "Secret identifier.",
///      "type": "string"
///    },
///    "managed": {
///      "description": "True if the secret's lifetime is managed by key vault. If this is a key backing a certificate, then managed will be true.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "recoveryId": {
///      "description": "The url of the recovery object, used to identify and recover the deleted secret.",
///      "type": "string"
///    },
///    "scheduledPurgeDate": {
///      "description": "The time when the secret is scheduled to be purged, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
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
pub struct DeletedSecretItem {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<SecretAttributes>,
    ///Type of the secret value such as a password.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///The time when the secret was deleted, in UTC
    #[serde(
        rename = "deletedDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deleted_date: ::std::option::Option<i64>,
    ///Secret identifier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///True if the secret's lifetime is managed by key vault. If this is a key backing a certificate, then managed will be true.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed: ::std::option::Option<bool>,
    ///The url of the recovery object, used to identify and recover the deleted secret.
    #[serde(
        rename = "recoveryId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub recovery_id: ::std::option::Option<::std::string::String>,
    ///The time when the secret is scheduled to be purged, in UTC
    #[serde(
        rename = "scheduledPurgeDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scheduled_purge_date: ::std::option::Option<i64>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&DeletedSecretItem> for DeletedSecretItem {
    fn from(value: &DeletedSecretItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedSecretItem {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            content_type: Default::default(),
            deleted_date: Default::default(),
            id: Default::default(),
            managed: Default::default(),
            recovery_id: Default::default(),
            scheduled_purge_date: Default::default(),
            tags: Default::default(),
        }
    }
}
///The deleted secret list result
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The deleted secret list result",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of deleted secrets.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "A response message containing a list of deleted secrets in the key vault along with a link to the next page of deleted secrets.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DeletedSecretItem"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedSecretListResult {
    ///The URL to get the next set of deleted secrets.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A response message containing a list of deleted secrets in the key vault along with a link to the next page of deleted secrets.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<DeletedSecretItem>,
}
impl ::std::convert::From<&DeletedSecretListResult> for DeletedSecretListResult {
    fn from(value: &DeletedSecretListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedSecretListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Reflects the deletion recovery level currently in effect for secrets in the current vault. If it contains 'Purgeable', the secret can be permanently deleted by a privileged user; otherwise, only the system can purge the secret, at the end of the retention interval.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Reflects the deletion recovery level currently in effect for secrets in the current vault. If it contains 'Purgeable', the secret can be permanently deleted by a privileged user; otherwise, only the system can purge the secret, at the end of the retention interval.",
///  "type": "string",
///  "enum": [
///    "Purgeable",
///    "Recoverable+Purgeable",
///    "Recoverable",
///    "Recoverable+ProtectedSubscription",
///    "CustomizedRecoverable+Purgeable",
///    "CustomizedRecoverable",
///    "CustomizedRecoverable+ProtectedSubscription"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "DeletionRecoveryLevel",
///    "values": [
///      {
///        "description": "Denotes a vault state in which deletion is an irreversible operation, without the possibility for recovery. This level corresponds to no protection being available against a Delete operation; the data is irretrievably lost upon accepting a Delete operation at the entity level or higher (vault, resource group, subscription etc.)",
///        "name": "Purgeable",
///        "value": "Purgeable"
///      },
///      {
///        "description": "Denotes a vault state in which deletion is recoverable, and which also permits immediate and permanent deletion (i.e. purge). This level guarantees the recoverability of the deleted entity during the retention interval (90 days), unless a Purge operation is requested, or the subscription is cancelled. System wil permanently delete it after 90 days, if not recovered",
///        "name": "RecoverablePurgeable",
///        "value": "Recoverable+Purgeable"
///      },
///      {
///        "description": "Denotes a vault state in which deletion is recoverable without the possibility for immediate and permanent deletion (i.e. purge). This level guarantees the recoverability of the deleted entity during the retention interval (90 days) and while the subscription is still available. System wil permanently delete it after 90 days, if not recovered",
///        "name": "Recoverable",
///        "value": "Recoverable"
///      },
///      {
///        "description": "Denotes a vault and subscription state in which deletion is recoverable within retention interval (90 days), immediate and permanent deletion (i.e. purge) is not permitted, and in which the subscription itself  cannot be permanently canceled. System wil permanently delete it after 90 days, if not recovered",
///        "name": "RecoverableProtectedSubscription",
///        "value": "Recoverable+ProtectedSubscription"
///      },
///      {
///        "description": "Denotes a vault state in which deletion is recoverable, and which also permits immediate and permanent deletion (i.e. purge when 7 <= SoftDeleteRetentionInDays < 90). This level guarantees the recoverability of the deleted entity during the retention interval, unless a Purge operation is requested, or the subscription is cancelled.",
///        "name": "CustomizedRecoverablePurgeable",
///        "value": "CustomizedRecoverable+Purgeable"
///      },
///      {
///        "description": "Denotes a vault state in which deletion is recoverable without the possibility for immediate and permanent deletion (i.e. purge when 7 <= SoftDeleteRetentionInDays < 90).This level guarantees the recoverability of the deleted entity during the retention interval and while the subscription is still available.",
///        "name": "CustomizedRecoverable",
///        "value": "CustomizedRecoverable"
///      },
///      {
///        "description": "Denotes a vault and subscription state in which deletion is recoverable, immediate and permanent deletion (i.e. purge) is not permitted, and in which the subscription itself cannot be permanently canceled when 7 <= SoftDeleteRetentionInDays < 90. This level guarantees the recoverability of the deleted entity during the retention interval, and also reflects the fact that the subscription itself cannot be cancelled.",
///        "name": "CustomizedRecoverableProtectedSubscription",
///        "value": "CustomizedRecoverable+ProtectedSubscription"
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
pub enum DeletionRecoveryLevel {
    Purgeable,
    #[serde(rename = "Recoverable+Purgeable")]
    RecoverablePurgeable,
    Recoverable,
    #[serde(rename = "Recoverable+ProtectedSubscription")]
    RecoverableProtectedSubscription,
    #[serde(rename = "CustomizedRecoverable+Purgeable")]
    CustomizedRecoverablePurgeable,
    CustomizedRecoverable,
    #[serde(rename = "CustomizedRecoverable+ProtectedSubscription")]
    CustomizedRecoverableProtectedSubscription,
}
impl ::std::convert::From<&Self> for DeletionRecoveryLevel {
    fn from(value: &DeletionRecoveryLevel) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for DeletionRecoveryLevel {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Purgeable => f.write_str("Purgeable"),
            Self::RecoverablePurgeable => f.write_str("Recoverable+Purgeable"),
            Self::Recoverable => f.write_str("Recoverable"),
            Self::RecoverableProtectedSubscription => {
                f.write_str("Recoverable+ProtectedSubscription")
            }
            Self::CustomizedRecoverablePurgeable => f.write_str("CustomizedRecoverable+Purgeable"),
            Self::CustomizedRecoverable => f.write_str("CustomizedRecoverable"),
            Self::CustomizedRecoverableProtectedSubscription => {
                f.write_str("CustomizedRecoverable+ProtectedSubscription")
            }
        }
    }
}
impl ::std::str::FromStr for DeletionRecoveryLevel {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "purgeable" => Ok(Self::Purgeable),
            "recoverable+purgeable" => Ok(Self::RecoverablePurgeable),
            "recoverable" => Ok(Self::Recoverable),
            "recoverable+protectedsubscription" => Ok(Self::RecoverableProtectedSubscription),
            "customizedrecoverable+purgeable" => Ok(Self::CustomizedRecoverablePurgeable),
            "customizedrecoverable" => Ok(Self::CustomizedRecoverable),
            "customizedrecoverable+protectedsubscription" => {
                Ok(Self::CustomizedRecoverableProtectedSubscription)
            }
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DeletionRecoveryLevel {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for DeletionRecoveryLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DeletionRecoveryLevel {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`Error`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": [
///    "object",
///    "null"
///  ],
///  "properties": {
///    "code": {
///      "description": "The error code.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "innererror": {
///      "$ref": "#/components/schemas/Error"
///    },
///    "message": {
///      "description": "The error message.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct Error(pub ::std::option::Option<ErrorInner>);
impl ::std::ops::Deref for Error {
    type Target = ::std::option::Option<ErrorInner>;
    fn deref(&self) -> &::std::option::Option<ErrorInner> {
        &self.0
    }
}
impl ::std::convert::From<Error> for ::std::option::Option<ErrorInner> {
    fn from(value: Error) -> Self {
        value.0
    }
}
impl ::std::convert::From<&Error> for Error {
    fn from(value: &Error) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::option::Option<ErrorInner>> for Error {
    fn from(value: ::std::option::Option<ErrorInner>) -> Self {
        Self(value)
    }
}
///`ErrorInner`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "code": {
///      "description": "The error code.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "innererror": {
///      "$ref": "#/components/schemas/Error"
///    },
///    "message": {
///      "description": "The error message.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ErrorInner {
    ///The error code.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub code: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub innererror: ::std::option::Option<::std::boxed::Box<Error>>,
    ///The error message.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ErrorInner> for ErrorInner {
    fn from(value: &ErrorInner) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ErrorInner {
    fn default() -> Self {
        Self {
            code: Default::default(),
            innererror: Default::default(),
            message: Default::default(),
        }
    }
}
///The key vault error exception.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The key vault error exception.",
///  "type": "object",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/Error"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyVaultError {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<::std::boxed::Box<Error>>,
}
impl ::std::convert::From<&KeyVaultError> for KeyVaultError {
    fn from(value: &KeyVaultError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KeyVaultError {
    fn default() -> Self {
        Self {
            error: Default::default(),
        }
    }
}
///The secret management attributes.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secret management attributes.",
///  "type": "object",
///  "properties": {
///    "created": {
///      "description": "Creation time in UTC.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "enabled": {
///      "description": "Determines whether the object is enabled.",
///      "type": "boolean"
///    },
///    "exp": {
///      "description": "Expiry date in UTC.",
///      "type": "integer",
///      "format": "unixtime",
///      "x-ms-client-name": "Expires"
///    },
///    "nbf": {
///      "description": "Not before date in UTC.",
///      "type": "integer",
///      "format": "unixtime",
///      "x-ms-client-name": "NotBefore"
///    },
///    "recoverableDays": {
///      "description": "softDelete data retention days. Value should be >=7 and <=90 when softDelete enabled, otherwise 0.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "int32"
///    },
///    "recoveryLevel": {
///      "$ref": "#/components/schemas/DeletionRecoveryLevel"
///    },
///    "updated": {
///      "description": "Last updated time in UTC.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretAttributes {
    ///Creation time in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created: ::std::option::Option<i64>,
    ///Determines whether the object is enabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///Expiry date in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exp: ::std::option::Option<i64>,
    ///Not before date in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub nbf: ::std::option::Option<i64>,
    ///softDelete data retention days. Value should be >=7 and <=90 when softDelete enabled, otherwise 0.
    #[serde(
        rename = "recoverableDays",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub recoverable_days: ::std::option::Option<i32>,
    #[serde(
        rename = "recoveryLevel",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub recovery_level: ::std::option::Option<DeletionRecoveryLevel>,
    ///Last updated time in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated: ::std::option::Option<i64>,
}
impl ::std::convert::From<&SecretAttributes> for SecretAttributes {
    fn from(value: &SecretAttributes) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretAttributes {
    fn default() -> Self {
        Self {
            created: Default::default(),
            enabled: Default::default(),
            exp: Default::default(),
            nbf: Default::default(),
            recoverable_days: Default::default(),
            recovery_level: Default::default(),
            updated: Default::default(),
        }
    }
}
///A secret consisting of a value, id and its attributes.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A secret consisting of a value, id and its attributes.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/SecretAttributes"
///    },
///    "contentType": {
///      "description": "The content type of the secret.",
///      "type": "string"
///    },
///    "id": {
///      "description": "The secret id.",
///      "type": "string"
///    },
///    "kid": {
///      "description": "If this is a secret backing a KV certificate, then this field specifies the corresponding key backing the KV certificate.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "managed": {
///      "description": "True if the secret's lifetime is managed by key vault. If this is a secret backing a certificate, then managed will be true.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "value": {
///      "description": "The secret value.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretBundle {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<SecretAttributes>,
    ///The content type of the secret.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///The secret id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///If this is a secret backing a KV certificate, then this field specifies the corresponding key backing the KV certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kid: ::std::option::Option<::std::string::String>,
    ///True if the secret's lifetime is managed by key vault. If this is a secret backing a certificate, then managed will be true.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed: ::std::option::Option<bool>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The secret value.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SecretBundle> for SecretBundle {
    fn from(value: &SecretBundle) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretBundle {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            content_type: Default::default(),
            id: Default::default(),
            kid: Default::default(),
            managed: Default::default(),
            tags: Default::default(),
            value: Default::default(),
        }
    }
}
///The secret item containing secret metadata.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secret item containing secret metadata.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/SecretAttributes"
///    },
///    "contentType": {
///      "description": "Type of the secret value such as a password.",
///      "type": "string"
///    },
///    "id": {
///      "description": "Secret identifier.",
///      "type": "string"
///    },
///    "managed": {
///      "description": "True if the secret's lifetime is managed by key vault. If this is a key backing a certificate, then managed will be true.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
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
pub struct SecretItem {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<SecretAttributes>,
    ///Type of the secret value such as a password.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///Secret identifier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///True if the secret's lifetime is managed by key vault. If this is a key backing a certificate, then managed will be true.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed: ::std::option::Option<bool>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&SecretItem> for SecretItem {
    fn from(value: &SecretItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretItem {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            content_type: Default::default(),
            id: Default::default(),
            managed: Default::default(),
            tags: Default::default(),
        }
    }
}
///The secret list result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secret list result.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of secrets.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "A response message containing a list of secrets in the key vault along with a link to the next page of secrets.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/SecretItem"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretListResult {
    ///The URL to get the next set of secrets.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A response message containing a list of secrets in the key vault along with a link to the next page of secrets.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<SecretItem>,
}
impl ::std::convert::From<&SecretListResult> for SecretListResult {
    fn from(value: &SecretListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///Properties of the key backing a certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the key backing a certificate.",
///  "type": "object",
///  "properties": {
///    "contentType": {
///      "description": "The media type (MIME type).",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretProperties {
    ///The media type (MIME type).
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&SecretProperties> for SecretProperties {
    fn from(value: &SecretProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretProperties {
    fn default() -> Self {
        Self {
            content_type: Default::default(),
        }
    }
}
///The secret restore parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secret restore parameters.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "The backup blob associated with a secret bundle.",
///      "type": "string",
///      "format": "base64url",
///      "x-ms-client-name": "secretBundleBackup"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretRestoreParameters {
    ///The backup blob associated with a secret bundle.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&SecretRestoreParameters> for SecretRestoreParameters {
    fn from(value: &SecretRestoreParameters) -> Self {
        value.clone()
    }
}
///The secret set parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secret set parameters.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/SecretAttributes"
///    },
///    "contentType": {
///      "description": "Type of the secret value such as a password.",
///      "type": "string"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "value": {
///      "description": "The value of the secret.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct SecretSetParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<SecretAttributes>,
    ///Type of the secret value such as a password.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The value of the secret.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&SecretSetParameters> for SecretSetParameters {
    fn from(value: &SecretSetParameters) -> Self {
        value.clone()
    }
}
///The secret update parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The secret update parameters.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/SecretAttributes"
///    },
///    "contentType": {
///      "description": "Type of the secret value such as a password.",
///      "type": "string"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
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
pub struct SecretUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<SecretAttributes>,
    ///Type of the secret value such as a password.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&SecretUpdateParameters> for SecretUpdateParameters {
    fn from(value: &SecretUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SecretUpdateParameters {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            content_type: Default::default(),
            tags: Default::default(),
        }
    }
}
