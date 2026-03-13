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
///Configuration of application logs
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration of application logs",
///  "type": "object",
///  "properties": {
///    "destination": {
///      "description": "Logs destination, can be 'log-analytics', 'azure-monitor' or 'none'",
///      "type": "string"
///    },
///    "logAnalyticsConfiguration": {
///      "$ref": "#/components/schemas/LogAnalyticsConfiguration"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AppLogsConfiguration {
    ///Logs destination, can be 'log-analytics', 'azure-monitor' or 'none'
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub destination: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "logAnalyticsConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub log_analytics_configuration: ::std::option::Option<LogAnalyticsConfiguration>,
}
impl ::std::convert::From<&AppLogsConfiguration> for AppLogsConfiguration {
    fn from(value: &AppLogsConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AppLogsConfiguration {
    fn default() -> Self {
        Self {
            destination: Default::default(),
            log_analytics_configuration: Default::default(),
        }
    }
}
///Certificate used for Custom Domain bindings of Container Apps in a Managed Environment
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Certificate used for Custom Domain bindings of Container Apps in a Managed Environment",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "Certificate resource specific properties",
///      "type": "object",
///      "properties": {
///        "certificateKeyVaultProperties": {
///          "$ref": "#/components/schemas/CertificateKeyVaultProperties"
///        },
///        "expirationDate": {
///          "description": "Certificate expiration date.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "issueDate": {
///          "description": "Certificate issue Date.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "issuer": {
///          "description": "Certificate issuer.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "password": {
///          "description": "Certificate password.",
///          "type": "string",
///          "x-ms-mutability": [
///            "create"
///          ],
///          "x-ms-secret": true
///        },
///        "provisioningState": {
///          "description": "Provisioning state of the certificate.",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "Succeeded",
///            "Failed",
///            "Canceled",
///            "DeleteFailed",
///            "Pending"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "CertificateProvisioningState"
///          }
///        },
///        "publicKeyHash": {
///          "description": "Public key hash.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "subjectAlternativeNames": {
///          "description": "Subject alternative names the certificate applies to.",
///          "readOnly": true,
///          "type": "array",
///          "items": {
///            "type": "string"
///          }
///        },
///        "subjectName": {
///          "description": "Subject name of the certificate.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "thumbprint": {
///          "description": "Certificate thumbprint.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "valid": {
///          "description": "Is the certificate valid?.",
///          "readOnly": true,
///          "type": "boolean"
///        },
///        "value": {
///          "description": "PFX or PEM blob",
///          "type": "string",
///          "format": "byte",
///          "x-ms-mutability": [
///            "create"
///          ],
///          "x-ms-secret": true
///        }
///      }
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Certificate {
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
    pub properties: ::std::option::Option<CertificateProperties>,
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
impl ::std::convert::From<&Certificate> for Certificate {
    fn from(value: &Certificate) -> Self {
        value.clone()
    }
}
///Collection of Certificates.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Collection of Certificates.",
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
///        "$ref": "#/components/schemas/Certificate"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<Certificate>,
}
impl ::std::convert::From<&CertificateCollection> for CertificateCollection {
    fn from(value: &CertificateCollection) -> Self {
        value.clone()
    }
}
///Properties for a certificate stored in a Key Vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties for a certificate stored in a Key Vault.",
///  "type": "object",
///  "properties": {
///    "identity": {
///      "description": "Resource ID of a managed identity to authenticate with Azure Key Vault, or System to use a system-assigned identity.",
///      "type": "string"
///    },
///    "keyVaultUrl": {
///      "description": "URL pointing to the Azure Key Vault secret that holds the certificate.",
///      "type": "string",
///      "format": "uri"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateKeyVaultProperties {
    ///Resource ID of a managed identity to authenticate with Azure Key Vault, or System to use a system-assigned identity.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub identity: ::std::option::Option<::std::string::String>,
    ///URL pointing to the Azure Key Vault secret that holds the certificate.
    #[serde(
        rename = "keyVaultUrl",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_vault_url: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateKeyVaultProperties>
for CertificateKeyVaultProperties {
    fn from(value: &CertificateKeyVaultProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateKeyVaultProperties {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            key_vault_url: Default::default(),
        }
    }
}
///A certificate to update
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A certificate to update",
///  "type": "object",
///  "properties": {
///    "tags": {
///      "description": "Application-specific metadata in the form of key-value pairs.",
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
pub struct CertificatePatch {
    ///Application-specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&CertificatePatch> for CertificatePatch {
    fn from(value: &CertificatePatch) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificatePatch {
    fn default() -> Self {
        Self { tags: Default::default() }
    }
}
///Certificate resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Certificate resource specific properties",
///  "type": "object",
///  "properties": {
///    "certificateKeyVaultProperties": {
///      "$ref": "#/components/schemas/CertificateKeyVaultProperties"
///    },
///    "expirationDate": {
///      "description": "Certificate expiration date.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "issueDate": {
///      "description": "Certificate issue Date.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "issuer": {
///      "description": "Certificate issuer.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "password": {
///      "description": "Certificate password.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create"
///      ],
///      "x-ms-secret": true
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the certificate.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Succeeded",
///        "Failed",
///        "Canceled",
///        "DeleteFailed",
///        "Pending"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "CertificateProvisioningState"
///      }
///    },
///    "publicKeyHash": {
///      "description": "Public key hash.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "subjectAlternativeNames": {
///      "description": "Subject alternative names the certificate applies to.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "subjectName": {
///      "description": "Subject name of the certificate.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "thumbprint": {
///      "description": "Certificate thumbprint.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "valid": {
///      "description": "Is the certificate valid?.",
///      "readOnly": true,
///      "type": "boolean"
///    },
///    "value": {
///      "description": "PFX or PEM blob",
///      "type": "string",
///      "format": "byte",
///      "x-ms-mutability": [
///        "create"
///      ],
///      "x-ms-secret": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateProperties {
    #[serde(
        rename = "certificateKeyVaultProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificate_key_vault_properties: ::std::option::Option<
        CertificateKeyVaultProperties,
    >,
    ///Certificate expiration date.
    #[serde(
        rename = "expirationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expiration_date: ::std::option::Option<::std::string::String>,
    ///Certificate issue Date.
    #[serde(
        rename = "issueDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub issue_date: ::std::option::Option<::std::string::String>,
    ///Certificate issuer.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub issuer: ::std::option::Option<::std::string::String>,
    ///Certificate password.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub password: ::std::option::Option<::std::string::String>,
    ///Provisioning state of the certificate.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        CertificatePropertiesProvisioningState,
    >,
    ///Public key hash.
    #[serde(
        rename = "publicKeyHash",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub public_key_hash: ::std::option::Option<::std::string::String>,
    ///Subject alternative names the certificate applies to.
    #[serde(
        rename = "subjectAlternativeNames",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subject_alternative_names: ::std::vec::Vec<::std::string::String>,
    ///Subject name of the certificate.
    #[serde(
        rename = "subjectName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subject_name: ::std::option::Option<::std::string::String>,
    ///Certificate thumbprint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub thumbprint: ::std::option::Option<::std::string::String>,
    ///Is the certificate valid?.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub valid: ::std::option::Option<bool>,
    ///PFX or PEM blob
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateProperties> for CertificateProperties {
    fn from(value: &CertificateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateProperties {
    fn default() -> Self {
        Self {
            certificate_key_vault_properties: Default::default(),
            expiration_date: Default::default(),
            issue_date: Default::default(),
            issuer: Default::default(),
            password: Default::default(),
            provisioning_state: Default::default(),
            public_key_hash: Default::default(),
            subject_alternative_names: Default::default(),
            subject_name: Default::default(),
            thumbprint: Default::default(),
            valid: Default::default(),
            value: Default::default(),
        }
    }
}
///Provisioning state of the certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the certificate.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "Failed",
///    "Canceled",
///    "DeleteFailed",
///    "Pending"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CertificateProvisioningState"
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
pub enum CertificatePropertiesProvisioningState {
    Succeeded,
    Failed,
    Canceled,
    DeleteFailed,
    Pending,
}
impl ::std::convert::From<&Self> for CertificatePropertiesProvisioningState {
    fn from(value: &CertificatePropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CertificatePropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
            Self::DeleteFailed => f.write_str("DeleteFailed"),
            Self::Pending => f.write_str("Pending"),
        }
    }
}
impl ::std::str::FromStr for CertificatePropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            "deletefailed" => Ok(Self::DeleteFailed),
            "pending" => Ok(Self::Pending),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CertificatePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for CertificatePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for CertificatePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The check availability request body.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The check availability request body.",
///  "type": "object",
///  "properties": {
///    "name": {
///      "description": "The name of the resource for which availability needs to be checked.",
///      "type": "string"
///    },
///    "type": {
///      "description": "The resource type.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CheckNameAvailabilityRequest {
    ///The name of the resource for which availability needs to be checked.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///The resource type.
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CheckNameAvailabilityRequest>
for CheckNameAvailabilityRequest {
    fn from(value: &CheckNameAvailabilityRequest) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CheckNameAvailabilityRequest {
    fn default() -> Self {
        Self {
            name: Default::default(),
            type_: Default::default(),
        }
    }
}
///The check availability result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The check availability result.",
///  "type": "object",
///  "properties": {
///    "message": {
///      "description": "Detailed reason why the given name is available.",
///      "type": "string"
///    },
///    "nameAvailable": {
///      "description": "Indicates if the resource name is available.",
///      "type": "boolean"
///    },
///    "reason": {
///      "description": "The reason why the given name is not available.",
///      "type": "string",
///      "enum": [
///        "Invalid",
///        "AlreadyExists"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "CheckNameAvailabilityReason"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CheckNameAvailabilityResponse {
    ///Detailed reason why the given name is available.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub message: ::std::option::Option<::std::string::String>,
    ///Indicates if the resource name is available.
    #[serde(
        rename = "nameAvailable",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name_available: ::std::option::Option<bool>,
    ///The reason why the given name is not available.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reason: ::std::option::Option<CheckNameAvailabilityResponseReason>,
}
impl ::std::convert::From<&CheckNameAvailabilityResponse>
for CheckNameAvailabilityResponse {
    fn from(value: &CheckNameAvailabilityResponse) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CheckNameAvailabilityResponse {
    fn default() -> Self {
        Self {
            message: Default::default(),
            name_available: Default::default(),
            reason: Default::default(),
        }
    }
}
///The reason why the given name is not available.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The reason why the given name is not available.",
///  "type": "string",
///  "enum": [
///    "Invalid",
///    "AlreadyExists"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CheckNameAvailabilityReason"
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
pub enum CheckNameAvailabilityResponseReason {
    Invalid,
    AlreadyExists,
}
impl ::std::convert::From<&Self> for CheckNameAvailabilityResponseReason {
    fn from(value: &CheckNameAvailabilityResponseReason) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CheckNameAvailabilityResponseReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Invalid => f.write_str("Invalid"),
            Self::AlreadyExists => f.write_str("AlreadyExists"),
        }
    }
}
impl ::std::str::FromStr for CheckNameAvailabilityResponseReason {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "invalid" => Ok(Self::Invalid),
            "alreadyexists" => Ok(Self::AlreadyExists),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CheckNameAvailabilityResponseReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for CheckNameAvailabilityResponseReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for CheckNameAvailabilityResponseReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Configuration properties for apps environment custom domain
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration properties for apps environment custom domain",
///  "type": "object",
///  "properties": {
///    "certificateKeyVaultProperties": {
///      "$ref": "#/components/schemas/CertificateKeyVaultProperties"
///    },
///    "certificatePassword": {
///      "description": "Certificate password",
///      "type": "string",
///      "x-ms-secret": true
///    },
///    "certificateValue": {
///      "description": "PFX or PEM blob",
///      "type": "string",
///      "format": "byte",
///      "x-ms-secret": true
///    },
///    "customDomainVerificationId": {
///      "description": "Id used to verify domain name ownership",
///      "readOnly": true,
///      "type": "string"
///    },
///    "dnsSuffix": {
///      "description": "Dns suffix for the environment domain",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "expirationDate": {
///      "description": "Certificate expiration date.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "subjectName": {
///      "description": "Subject name of the certificate.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "thumbprint": {
///      "description": "Certificate thumbprint.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CustomDomainConfiguration {
    #[serde(
        rename = "certificateKeyVaultProperties",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificate_key_vault_properties: ::std::option::Option<
        CertificateKeyVaultProperties,
    >,
    ///Certificate password
    #[serde(
        rename = "certificatePassword",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificate_password: ::std::option::Option<::std::string::String>,
    ///PFX or PEM blob
    #[serde(
        rename = "certificateValue",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub certificate_value: ::std::option::Option<::std::string::String>,
    ///Id used to verify domain name ownership
    #[serde(
        rename = "customDomainVerificationId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain_verification_id: ::std::option::Option<::std::string::String>,
    ///Dns suffix for the environment domain
    #[serde(
        rename = "dnsSuffix",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_suffix: ::std::option::Option<::std::string::String>,
    ///Certificate expiration date.
    #[serde(
        rename = "expirationDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub expiration_date: ::std::option::Option<::std::string::String>,
    ///Subject name of the certificate.
    #[serde(
        rename = "subjectName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subject_name: ::std::option::Option<::std::string::String>,
    ///Certificate thumbprint.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub thumbprint: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CustomDomainConfiguration> for CustomDomainConfiguration {
    fn from(value: &CustomDomainConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CustomDomainConfiguration {
    fn default() -> Self {
        Self {
            certificate_key_vault_properties: Default::default(),
            certificate_password: Default::default(),
            certificate_value: Default::default(),
            custom_domain_verification_id: Default::default(),
            dns_suffix: Default::default(),
            expiration_date: Default::default(),
            subject_name: Default::default(),
            thumbprint: Default::default(),
        }
    }
}
///Configuration properties Dapr component
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration properties Dapr component",
///  "type": "object",
///  "properties": {
///    "version": {
///      "description": "The version of Dapr",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DaprConfiguration {
    ///The version of Dapr
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DaprConfiguration> for DaprConfiguration {
    fn from(value: &DaprConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DaprConfiguration {
    fn default() -> Self {
        Self {
            version: Default::default(),
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
///Environment Auth Token.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Environment Auth Token.",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "Environment auth token resource specific properties",
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
pub struct EnvironmentAuthToken {
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
    pub properties: ::std::option::Option<EnvironmentAuthTokenProperties>,
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
impl ::std::convert::From<&EnvironmentAuthToken> for EnvironmentAuthToken {
    fn from(value: &EnvironmentAuthToken) -> Self {
        value.clone()
    }
}
///Environment auth token resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Environment auth token resource specific properties",
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
pub struct EnvironmentAuthTokenProperties {
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
impl ::std::convert::From<&EnvironmentAuthTokenProperties>
for EnvironmentAuthTokenProperties {
    fn from(value: &EnvironmentAuthTokenProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for EnvironmentAuthTokenProperties {
    fn default() -> Self {
        Self {
            expires: Default::default(),
            token: Default::default(),
        }
    }
}
///Configuration properties Keda component
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration properties Keda component",
///  "type": "object",
///  "properties": {
///    "version": {
///      "description": "The version of Keda",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KedaConfiguration {
    ///The version of Keda
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub version: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&KedaConfiguration> for KedaConfiguration {
    fn from(value: &KedaConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KedaConfiguration {
    fn default() -> Self {
        Self {
            version: Default::default(),
        }
    }
}
///Log Analytics configuration, must only be provided when destination is configured as 'log-analytics'
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Log Analytics configuration, must only be provided when destination is configured as 'log-analytics'",
///  "type": "object",
///  "properties": {
///    "customerId": {
///      "description": "Log analytics customer id",
///      "type": "string"
///    },
///    "sharedKey": {
///      "description": "Log analytics customer key",
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
pub struct LogAnalyticsConfiguration {
    ///Log analytics customer id
    #[serde(
        rename = "customerId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub customer_id: ::std::option::Option<::std::string::String>,
    ///Log analytics customer key
    #[serde(
        rename = "sharedKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub shared_key: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&LogAnalyticsConfiguration> for LogAnalyticsConfiguration {
    fn from(value: &LogAnalyticsConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LogAnalyticsConfiguration {
    fn default() -> Self {
        Self {
            customer_id: Default::default(),
            shared_key: Default::default(),
        }
    }
}
///Managed certificates used for Custom Domain bindings of Container Apps in a Managed Environment
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Managed certificates used for Custom Domain bindings of Container Apps in a Managed Environment",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/TrackedResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "Certificate resource specific properties",
///      "type": "object",
///      "properties": {
///        "domainControlValidation": {
///          "description": "Selected type of domain control validation for managed certificates.",
///          "type": "string",
///          "enum": [
///            "CNAME",
///            "HTTP",
///            "TXT"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "ManagedCertificateDomainControlValidation"
///          }
///        },
///        "error": {
///          "description": "Any error occurred during the certificate provision.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "provisioningState": {
///          "description": "Provisioning state of the certificate.",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "Succeeded",
///            "Failed",
///            "Canceled",
///            "DeleteFailed",
///            "Pending"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "CertificateProvisioningState"
///          }
///        },
///        "subjectName": {
///          "description": "Subject name of the certificate.",
///          "type": "string"
///        },
///        "validationToken": {
///          "description": "A TXT token used for DNS TXT domain control validation when issuing this type of managed certificates.",
///          "readOnly": true,
///          "type": "string"
///        }
///      }
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedCertificate {
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
    pub properties: ::std::option::Option<ManagedCertificateProperties>,
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
impl ::std::convert::From<&ManagedCertificate> for ManagedCertificate {
    fn from(value: &ManagedCertificate) -> Self {
        value.clone()
    }
}
///Collection of Managed Certificates.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Collection of Managed Certificates.",
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
///        "$ref": "#/components/schemas/ManagedCertificate"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedCertificateCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<ManagedCertificate>,
}
impl ::std::convert::From<&ManagedCertificateCollection>
for ManagedCertificateCollection {
    fn from(value: &ManagedCertificateCollection) -> Self {
        value.clone()
    }
}
///A managed certificate to update
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A managed certificate to update",
///  "type": "object",
///  "properties": {
///    "tags": {
///      "description": "Application-specific metadata in the form of key-value pairs.",
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
pub struct ManagedCertificatePatch {
    ///Application-specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&ManagedCertificatePatch> for ManagedCertificatePatch {
    fn from(value: &ManagedCertificatePatch) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedCertificatePatch {
    fn default() -> Self {
        Self { tags: Default::default() }
    }
}
///Certificate resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Certificate resource specific properties",
///  "type": "object",
///  "properties": {
///    "domainControlValidation": {
///      "description": "Selected type of domain control validation for managed certificates.",
///      "type": "string",
///      "enum": [
///        "CNAME",
///        "HTTP",
///        "TXT"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ManagedCertificateDomainControlValidation"
///      }
///    },
///    "error": {
///      "description": "Any error occurred during the certificate provision.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the certificate.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Succeeded",
///        "Failed",
///        "Canceled",
///        "DeleteFailed",
///        "Pending"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "CertificateProvisioningState"
///      }
///    },
///    "subjectName": {
///      "description": "Subject name of the certificate.",
///      "type": "string"
///    },
///    "validationToken": {
///      "description": "A TXT token used for DNS TXT domain control validation when issuing this type of managed certificates.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedCertificateProperties {
    ///Selected type of domain control validation for managed certificates.
    #[serde(
        rename = "domainControlValidation",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub domain_control_validation: ::std::option::Option<
        ManagedCertificatePropertiesDomainControlValidation,
    >,
    ///Any error occurred during the certificate provision.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<::std::string::String>,
    ///Provisioning state of the certificate.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        ManagedCertificatePropertiesProvisioningState,
    >,
    ///Subject name of the certificate.
    #[serde(
        rename = "subjectName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subject_name: ::std::option::Option<::std::string::String>,
    ///A TXT token used for DNS TXT domain control validation when issuing this type of managed certificates.
    #[serde(
        rename = "validationToken",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub validation_token: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&ManagedCertificateProperties>
for ManagedCertificateProperties {
    fn from(value: &ManagedCertificateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedCertificateProperties {
    fn default() -> Self {
        Self {
            domain_control_validation: Default::default(),
            error: Default::default(),
            provisioning_state: Default::default(),
            subject_name: Default::default(),
            validation_token: Default::default(),
        }
    }
}
///Selected type of domain control validation for managed certificates.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Selected type of domain control validation for managed certificates.",
///  "type": "string",
///  "enum": [
///    "CNAME",
///    "HTTP",
///    "TXT"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "ManagedCertificateDomainControlValidation"
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
pub enum ManagedCertificatePropertiesDomainControlValidation {
    #[serde(rename = "CNAME")]
    Cname,
    #[serde(rename = "HTTP")]
    Http,
    #[serde(rename = "TXT")]
    Txt,
}
impl ::std::convert::From<&Self>
for ManagedCertificatePropertiesDomainControlValidation {
    fn from(value: &ManagedCertificatePropertiesDomainControlValidation) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ManagedCertificatePropertiesDomainControlValidation {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Cname => f.write_str("CNAME"),
            Self::Http => f.write_str("HTTP"),
            Self::Txt => f.write_str("TXT"),
        }
    }
}
impl ::std::str::FromStr for ManagedCertificatePropertiesDomainControlValidation {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "cname" => Ok(Self::Cname),
            "http" => Ok(Self::Http),
            "txt" => Ok(Self::Txt),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
for ManagedCertificatePropertiesDomainControlValidation {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for ManagedCertificatePropertiesDomainControlValidation {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for ManagedCertificatePropertiesDomainControlValidation {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Provisioning state of the certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the certificate.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "Failed",
///    "Canceled",
///    "DeleteFailed",
///    "Pending"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "CertificateProvisioningState"
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
pub enum ManagedCertificatePropertiesProvisioningState {
    Succeeded,
    Failed,
    Canceled,
    DeleteFailed,
    Pending,
}
impl ::std::convert::From<&Self> for ManagedCertificatePropertiesProvisioningState {
    fn from(value: &ManagedCertificatePropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ManagedCertificatePropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
            Self::DeleteFailed => f.write_str("DeleteFailed"),
            Self::Pending => f.write_str("Pending"),
        }
    }
}
impl ::std::str::FromStr for ManagedCertificatePropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            "deletefailed" => Ok(Self::DeleteFailed),
            "pending" => Ok(Self::Pending),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ManagedCertificatePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for ManagedCertificatePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for ManagedCertificatePropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///An environment for hosting container apps
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "An environment for hosting container apps",
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
///    "kind": {
///      "description": "Kind of the Environment.",
///      "type": "string"
///    },
///    "properties": {
///      "description": "Managed environment resource specific properties",
///      "type": "object",
///      "properties": {
///        "appLogsConfiguration": {
///          "$ref": "#/components/schemas/AppLogsConfiguration"
///        },
///        "customDomainConfiguration": {
///          "$ref": "#/components/schemas/CustomDomainConfiguration"
///        },
///        "daprAIConnectionString": {
///          "description": "Application Insights connection string used by Dapr to export Service to Service communication telemetry",
///          "type": "string",
///          "x-ms-secret": true
///        },
///        "daprAIInstrumentationKey": {
///          "description": "Azure Monitor instrumentation key used by Dapr to export Service to Service communication telemetry",
///          "type": "string",
///          "x-ms-secret": true
///        },
///        "daprConfiguration": {
///          "$ref": "#/components/schemas/DaprConfiguration"
///        },
///        "defaultDomain": {
///          "description": "Default Domain Name for the cluster",
///          "readOnly": true,
///          "type": "string"
///        },
///        "deploymentErrors": {
///          "description": "Any errors that occurred during deployment or deployment validation",
///          "readOnly": true,
///          "type": "string"
///        },
///        "eventStreamEndpoint": {
///          "description": "The endpoint of the eventstream of the Environment.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "infrastructureResourceGroup": {
///          "description": "Name of the platform-managed resource group created for the Managed Environment to host infrastructure resources. If a subnet ID is provided, this resource group will be created in the same subscription as the subnet.",
///          "type": "string",
///          "x-ms-mutability": [
///            "create",
///            "read"
///          ]
///        },
///        "kedaConfiguration": {
///          "$ref": "#/components/schemas/KedaConfiguration"
///        },
///        "peerAuthentication": {
///          "description": "Peer authentication settings for the Managed Environment",
///          "type": "object",
///          "properties": {
///            "mtls": {
///              "$ref": "#/components/schemas/Mtls"
///            }
///          }
///        },
///        "peerTrafficConfiguration": {
///          "description": "Peer traffic settings for the Managed Environment",
///          "type": "object",
///          "properties": {
///            "encryption": {
///              "description": "Peer traffic encryption settings for the Managed Environment",
///              "type": "object",
///              "properties": {
///                "enabled": {
///                  "description": "Boolean indicating whether the peer traffic encryption is enabled",
///                  "type": "boolean"
///                }
///              }
///            }
///          }
///        },
///        "provisioningState": {
///          "description": "Provisioning state of the Environment.",
///          "readOnly": true,
///          "type": "string",
///          "enum": [
///            "Succeeded",
///            "Failed",
///            "Canceled",
///            "Waiting",
///            "InitializationInProgress",
///            "InfrastructureSetupInProgress",
///            "InfrastructureSetupComplete",
///            "ScheduledForDelete",
///            "UpgradeRequested",
///            "UpgradeFailed"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "EnvironmentProvisioningState"
///          }
///        },
///        "staticIp": {
///          "description": "Static IP of the Environment",
///          "readOnly": true,
///          "type": "string"
///        },
///        "vnetConfiguration": {
///          "$ref": "#/components/schemas/VnetConfiguration"
///        },
///        "workloadProfiles": {
///          "description": "Workload profiles configured for the Managed Environment.",
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/WorkloadProfile"
///          },
///          "x-ms-identifiers": [
///            "name"
///          ]
///        },
///        "zoneRedundant": {
///          "description": "Whether or not this Managed Environment is zone-redundant.",
///          "type": "boolean",
///          "x-ms-mutability": [
///            "create",
///            "read"
///          ]
///        }
///      },
///      "x-ms-client-flatten": true
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedEnvironment {
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
    ///Kind of the Environment.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kind: ::std::option::Option<::std::string::String>,
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
    pub properties: ::std::option::Option<ManagedEnvironmentProperties>,
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
impl ::std::convert::From<&ManagedEnvironment> for ManagedEnvironment {
    fn from(value: &ManagedEnvironment) -> Self {
        value.clone()
    }
}
///Managed environment resource specific properties
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Managed environment resource specific properties",
///  "type": "object",
///  "properties": {
///    "appLogsConfiguration": {
///      "$ref": "#/components/schemas/AppLogsConfiguration"
///    },
///    "customDomainConfiguration": {
///      "$ref": "#/components/schemas/CustomDomainConfiguration"
///    },
///    "daprAIConnectionString": {
///      "description": "Application Insights connection string used by Dapr to export Service to Service communication telemetry",
///      "type": "string",
///      "x-ms-secret": true
///    },
///    "daprAIInstrumentationKey": {
///      "description": "Azure Monitor instrumentation key used by Dapr to export Service to Service communication telemetry",
///      "type": "string",
///      "x-ms-secret": true
///    },
///    "daprConfiguration": {
///      "$ref": "#/components/schemas/DaprConfiguration"
///    },
///    "defaultDomain": {
///      "description": "Default Domain Name for the cluster",
///      "readOnly": true,
///      "type": "string"
///    },
///    "deploymentErrors": {
///      "description": "Any errors that occurred during deployment or deployment validation",
///      "readOnly": true,
///      "type": "string"
///    },
///    "eventStreamEndpoint": {
///      "description": "The endpoint of the eventstream of the Environment.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "infrastructureResourceGroup": {
///      "description": "Name of the platform-managed resource group created for the Managed Environment to host infrastructure resources. If a subnet ID is provided, this resource group will be created in the same subscription as the subnet.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "kedaConfiguration": {
///      "$ref": "#/components/schemas/KedaConfiguration"
///    },
///    "peerAuthentication": {
///      "description": "Peer authentication settings for the Managed Environment",
///      "type": "object",
///      "properties": {
///        "mtls": {
///          "$ref": "#/components/schemas/Mtls"
///        }
///      }
///    },
///    "peerTrafficConfiguration": {
///      "description": "Peer traffic settings for the Managed Environment",
///      "type": "object",
///      "properties": {
///        "encryption": {
///          "description": "Peer traffic encryption settings for the Managed Environment",
///          "type": "object",
///          "properties": {
///            "enabled": {
///              "description": "Boolean indicating whether the peer traffic encryption is enabled",
///              "type": "boolean"
///            }
///          }
///        }
///      }
///    },
///    "provisioningState": {
///      "description": "Provisioning state of the Environment.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "Succeeded",
///        "Failed",
///        "Canceled",
///        "Waiting",
///        "InitializationInProgress",
///        "InfrastructureSetupInProgress",
///        "InfrastructureSetupComplete",
///        "ScheduledForDelete",
///        "UpgradeRequested",
///        "UpgradeFailed"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "EnvironmentProvisioningState"
///      }
///    },
///    "staticIp": {
///      "description": "Static IP of the Environment",
///      "readOnly": true,
///      "type": "string"
///    },
///    "vnetConfiguration": {
///      "$ref": "#/components/schemas/VnetConfiguration"
///    },
///    "workloadProfiles": {
///      "description": "Workload profiles configured for the Managed Environment.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/WorkloadProfile"
///      },
///      "x-ms-identifiers": [
///        "name"
///      ]
///    },
///    "zoneRedundant": {
///      "description": "Whether or not this Managed Environment is zone-redundant.",
///      "type": "boolean",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    }
///  },
///  "x-ms-client-flatten": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedEnvironmentProperties {
    #[serde(
        rename = "appLogsConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub app_logs_configuration: ::std::option::Option<AppLogsConfiguration>,
    #[serde(
        rename = "customDomainConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub custom_domain_configuration: ::std::option::Option<CustomDomainConfiguration>,
    ///Application Insights connection string used by Dapr to export Service to Service communication telemetry
    #[serde(
        rename = "daprAIConnectionString",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dapr_ai_connection_string: ::std::option::Option<::std::string::String>,
    ///Azure Monitor instrumentation key used by Dapr to export Service to Service communication telemetry
    #[serde(
        rename = "daprAIInstrumentationKey",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dapr_ai_instrumentation_key: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "daprConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dapr_configuration: ::std::option::Option<DaprConfiguration>,
    ///Default Domain Name for the cluster
    #[serde(
        rename = "defaultDomain",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub default_domain: ::std::option::Option<::std::string::String>,
    ///Any errors that occurred during deployment or deployment validation
    #[serde(
        rename = "deploymentErrors",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deployment_errors: ::std::option::Option<::std::string::String>,
    ///The endpoint of the eventstream of the Environment.
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub event_stream_endpoint: ::std::option::Option<::std::string::String>,
    ///Name of the platform-managed resource group created for the Managed Environment to host infrastructure resources. If a subnet ID is provided, this resource group will be created in the same subscription as the subnet.
    #[serde(
        rename = "infrastructureResourceGroup",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub infrastructure_resource_group: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "kedaConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub keda_configuration: ::std::option::Option<KedaConfiguration>,
    #[serde(
        rename = "peerAuthentication",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub peer_authentication: ::std::option::Option<
        ManagedEnvironmentPropertiesPeerAuthentication,
    >,
    #[serde(
        rename = "peerTrafficConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub peer_traffic_configuration: ::std::option::Option<
        ManagedEnvironmentPropertiesPeerTrafficConfiguration,
    >,
    ///Provisioning state of the Environment.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        ManagedEnvironmentPropertiesProvisioningState,
    >,
    ///Static IP of the Environment
    #[serde(
        rename = "staticIp",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub static_ip: ::std::option::Option<::std::string::String>,
    #[serde(
        rename = "vnetConfiguration",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub vnet_configuration: ::std::option::Option<VnetConfiguration>,
    ///Workload profiles configured for the Managed Environment.
    #[serde(
        rename = "workloadProfiles",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub workload_profiles: ::std::vec::Vec<WorkloadProfile>,
    ///Whether or not this Managed Environment is zone-redundant.
    #[serde(
        rename = "zoneRedundant",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub zone_redundant: ::std::option::Option<bool>,
}
impl ::std::convert::From<&ManagedEnvironmentProperties>
for ManagedEnvironmentProperties {
    fn from(value: &ManagedEnvironmentProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedEnvironmentProperties {
    fn default() -> Self {
        Self {
            app_logs_configuration: Default::default(),
            custom_domain_configuration: Default::default(),
            dapr_ai_connection_string: Default::default(),
            dapr_ai_instrumentation_key: Default::default(),
            dapr_configuration: Default::default(),
            default_domain: Default::default(),
            deployment_errors: Default::default(),
            event_stream_endpoint: Default::default(),
            infrastructure_resource_group: Default::default(),
            keda_configuration: Default::default(),
            peer_authentication: Default::default(),
            peer_traffic_configuration: Default::default(),
            provisioning_state: Default::default(),
            static_ip: Default::default(),
            vnet_configuration: Default::default(),
            workload_profiles: Default::default(),
            zone_redundant: Default::default(),
        }
    }
}
///Peer authentication settings for the Managed Environment
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Peer authentication settings for the Managed Environment",
///  "type": "object",
///  "properties": {
///    "mtls": {
///      "$ref": "#/components/schemas/Mtls"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedEnvironmentPropertiesPeerAuthentication {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub mtls: ::std::option::Option<Mtls>,
}
impl ::std::convert::From<&ManagedEnvironmentPropertiesPeerAuthentication>
for ManagedEnvironmentPropertiesPeerAuthentication {
    fn from(value: &ManagedEnvironmentPropertiesPeerAuthentication) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedEnvironmentPropertiesPeerAuthentication {
    fn default() -> Self {
        Self { mtls: Default::default() }
    }
}
///Peer traffic settings for the Managed Environment
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Peer traffic settings for the Managed Environment",
///  "type": "object",
///  "properties": {
///    "encryption": {
///      "description": "Peer traffic encryption settings for the Managed Environment",
///      "type": "object",
///      "properties": {
///        "enabled": {
///          "description": "Boolean indicating whether the peer traffic encryption is enabled",
///          "type": "boolean"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedEnvironmentPropertiesPeerTrafficConfiguration {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub encryption: ::std::option::Option<
        ManagedEnvironmentPropertiesPeerTrafficConfigurationEncryption,
    >,
}
impl ::std::convert::From<&ManagedEnvironmentPropertiesPeerTrafficConfiguration>
for ManagedEnvironmentPropertiesPeerTrafficConfiguration {
    fn from(value: &ManagedEnvironmentPropertiesPeerTrafficConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for ManagedEnvironmentPropertiesPeerTrafficConfiguration {
    fn default() -> Self {
        Self {
            encryption: Default::default(),
        }
    }
}
///Peer traffic encryption settings for the Managed Environment
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Peer traffic encryption settings for the Managed Environment",
///  "type": "object",
///  "properties": {
///    "enabled": {
///      "description": "Boolean indicating whether the peer traffic encryption is enabled",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedEnvironmentPropertiesPeerTrafficConfigurationEncryption {
    ///Boolean indicating whether the peer traffic encryption is enabled
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<
    &ManagedEnvironmentPropertiesPeerTrafficConfigurationEncryption,
> for ManagedEnvironmentPropertiesPeerTrafficConfigurationEncryption {
    fn from(
        value: &ManagedEnvironmentPropertiesPeerTrafficConfigurationEncryption,
    ) -> Self {
        value.clone()
    }
}
impl ::std::default::Default
for ManagedEnvironmentPropertiesPeerTrafficConfigurationEncryption {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
        }
    }
}
///Provisioning state of the Environment.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Provisioning state of the Environment.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "Succeeded",
///    "Failed",
///    "Canceled",
///    "Waiting",
///    "InitializationInProgress",
///    "InfrastructureSetupInProgress",
///    "InfrastructureSetupComplete",
///    "ScheduledForDelete",
///    "UpgradeRequested",
///    "UpgradeFailed"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "EnvironmentProvisioningState"
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
pub enum ManagedEnvironmentPropertiesProvisioningState {
    Succeeded,
    Failed,
    Canceled,
    Waiting,
    InitializationInProgress,
    InfrastructureSetupInProgress,
    InfrastructureSetupComplete,
    ScheduledForDelete,
    UpgradeRequested,
    UpgradeFailed,
}
impl ::std::convert::From<&Self> for ManagedEnvironmentPropertiesProvisioningState {
    fn from(value: &ManagedEnvironmentPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for ManagedEnvironmentPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Failed => f.write_str("Failed"),
            Self::Canceled => f.write_str("Canceled"),
            Self::Waiting => f.write_str("Waiting"),
            Self::InitializationInProgress => f.write_str("InitializationInProgress"),
            Self::InfrastructureSetupInProgress => {
                f.write_str("InfrastructureSetupInProgress")
            }
            Self::InfrastructureSetupComplete => {
                f.write_str("InfrastructureSetupComplete")
            }
            Self::ScheduledForDelete => f.write_str("ScheduledForDelete"),
            Self::UpgradeRequested => f.write_str("UpgradeRequested"),
            Self::UpgradeFailed => f.write_str("UpgradeFailed"),
        }
    }
}
impl ::std::str::FromStr for ManagedEnvironmentPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            "waiting" => Ok(Self::Waiting),
            "initializationinprogress" => Ok(Self::InitializationInProgress),
            "infrastructuresetupinprogress" => Ok(Self::InfrastructureSetupInProgress),
            "infrastructuresetupcomplete" => Ok(Self::InfrastructureSetupComplete),
            "scheduledfordelete" => Ok(Self::ScheduledForDelete),
            "upgraderequested" => Ok(Self::UpgradeRequested),
            "upgradefailed" => Ok(Self::UpgradeFailed),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ManagedEnvironmentPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for ManagedEnvironmentPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for ManagedEnvironmentPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Collection of Environments
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Collection of Environments",
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
///        "$ref": "#/components/schemas/ManagedEnvironment"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ManagedEnvironmentsCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<ManagedEnvironment>,
}
impl ::std::convert::From<&ManagedEnvironmentsCollection>
for ManagedEnvironmentsCollection {
    fn from(value: &ManagedEnvironmentsCollection) -> Self {
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
///Configuration properties for mutual TLS authentication
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration properties for mutual TLS authentication",
///  "type": "object",
///  "properties": {
///    "enabled": {
///      "description": "Boolean indicating whether the mutual TLS authentication is enabled",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Mtls {
    ///Boolean indicating whether the mutual TLS authentication is enabled
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
}
impl ::std::convert::From<&Mtls> for Mtls {
    fn from(value: &Mtls) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Mtls {
    fn default() -> Self {
        Self {
            enabled: Default::default(),
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
///Configuration properties for apps environment to join a Virtual Network
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Configuration properties for apps environment to join a Virtual Network",
///  "type": "object",
///  "properties": {
///    "dockerBridgeCidr": {
///      "description": "CIDR notation IP range assigned to the Docker bridge, network. Must not overlap with any other provided IP ranges.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "infrastructureSubnetId": {
///      "description": "Resource ID of a subnet for infrastructure components. Must not overlap with any other provided IP ranges.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "internal": {
///      "description": "Boolean indicating the environment only has an internal load balancer. These environments do not have a public static IP resource. They must provide infrastructureSubnetId if enabling this property",
///      "type": "boolean",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "platformReservedCidr": {
///      "description": "IP range in CIDR notation that can be reserved for environment infrastructure IP addresses. Must not overlap with any other provided IP ranges.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    },
///    "platformReservedDnsIP": {
///      "description": " An IP address from the IP range defined by platformReservedCidr that will be reserved for the internal DNS server.",
///      "type": "string",
///      "x-ms-mutability": [
///        "create",
///        "read"
///      ]
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct VnetConfiguration {
    ///CIDR notation IP range assigned to the Docker bridge, network. Must not overlap with any other provided IP ranges.
    #[serde(
        rename = "dockerBridgeCidr",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub docker_bridge_cidr: ::std::option::Option<::std::string::String>,
    ///Resource ID of a subnet for infrastructure components. Must not overlap with any other provided IP ranges.
    #[serde(
        rename = "infrastructureSubnetId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub infrastructure_subnet_id: ::std::option::Option<::std::string::String>,
    ///Boolean indicating the environment only has an internal load balancer. These environments do not have a public static IP resource. They must provide infrastructureSubnetId if enabling this property
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub internal: ::std::option::Option<bool>,
    ///IP range in CIDR notation that can be reserved for environment infrastructure IP addresses. Must not overlap with any other provided IP ranges.
    #[serde(
        rename = "platformReservedCidr",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub platform_reserved_cidr: ::std::option::Option<::std::string::String>,
    /// An IP address from the IP range defined by platformReservedCidr that will be reserved for the internal DNS server.
    #[serde(
        rename = "platformReservedDnsIP",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub platform_reserved_dns_ip: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&VnetConfiguration> for VnetConfiguration {
    fn from(value: &VnetConfiguration) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for VnetConfiguration {
    fn default() -> Self {
        Self {
            docker_bridge_cidr: Default::default(),
            infrastructure_subnet_id: Default::default(),
            internal: Default::default(),
            platform_reserved_cidr: Default::default(),
            platform_reserved_dns_ip: Default::default(),
        }
    }
}
///Workload profile to scope container app execution.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Workload profile to scope container app execution.",
///  "type": "object",
///  "required": [
///    "name",
///    "workloadProfileType"
///  ],
///  "properties": {
///    "maximumCount": {
///      "description": "The maximum capacity.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "minimumCount": {
///      "description": "The minimum capacity.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "name": {
///      "$ref": "#/components/schemas/WorkloadProfileName"
///    },
///    "workloadProfileType": {
///      "$ref": "#/components/schemas/WorkloadProfileType"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WorkloadProfile {
    ///The maximum capacity.
    #[serde(
        rename = "maximumCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub maximum_count: ::std::option::Option<i32>,
    ///The minimum capacity.
    #[serde(
        rename = "minimumCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub minimum_count: ::std::option::Option<i32>,
    pub name: WorkloadProfileName,
    #[serde(rename = "workloadProfileType")]
    pub workload_profile_type: WorkloadProfileType,
}
impl ::std::convert::From<&WorkloadProfile> for WorkloadProfile {
    fn from(value: &WorkloadProfile) -> Self {
        value.clone()
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
///Collection of all the workload Profile States for a Managed Environment..
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Collection of all the workload Profile States for a Managed Environment..",
///  "type": "object",
///  "allOf": [
///    {
///      "$ref": "#/components/schemas/ProxyResource"
///    }
///  ],
///  "properties": {
///    "properties": {
///      "description": "Workload Profile resource specific properties.",
///      "type": "object",
///      "properties": {
///        "currentCount": {
///          "description": "Current count of nodes.",
///          "type": "integer",
///          "format": "int32"
///        },
///        "maximumCount": {
///          "description": "Maximum count of nodes.",
///          "type": "integer",
///          "format": "int32"
///        },
///        "minimumCount": {
///          "description": "Minimum count of instances.",
///          "type": "integer",
///          "format": "int32"
///        }
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WorkloadProfileStates {
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
    pub properties: ::std::option::Option<WorkloadProfileStatesProperties>,
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
impl ::std::convert::From<&WorkloadProfileStates> for WorkloadProfileStates {
    fn from(value: &WorkloadProfileStates) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for WorkloadProfileStates {
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
///Collection of workloadProfileStates
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Collection of workloadProfileStates",
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
///        "$ref": "#/components/schemas/workloadProfileStates"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WorkloadProfileStatesCollection {
    ///Link to next page of resources.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///Collection of resources.
    pub value: ::std::vec::Vec<WorkloadProfileStates>,
}
impl ::std::convert::From<&WorkloadProfileStatesCollection>
for WorkloadProfileStatesCollection {
    fn from(value: &WorkloadProfileStatesCollection) -> Self {
        value.clone()
    }
}
///Workload Profile resource specific properties.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Workload Profile resource specific properties.",
///  "type": "object",
///  "properties": {
///    "currentCount": {
///      "description": "Current count of nodes.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "maximumCount": {
///      "description": "Maximum count of nodes.",
///      "type": "integer",
///      "format": "int32"
///    },
///    "minimumCount": {
///      "description": "Minimum count of instances.",
///      "type": "integer",
///      "format": "int32"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct WorkloadProfileStatesProperties {
    ///Current count of nodes.
    #[serde(
        rename = "currentCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub current_count: ::std::option::Option<i32>,
    ///Maximum count of nodes.
    #[serde(
        rename = "maximumCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub maximum_count: ::std::option::Option<i32>,
    ///Minimum count of instances.
    #[serde(
        rename = "minimumCount",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub minimum_count: ::std::option::Option<i32>,
}
impl ::std::convert::From<&WorkloadProfileStatesProperties>
for WorkloadProfileStatesProperties {
    fn from(value: &WorkloadProfileStatesProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for WorkloadProfileStatesProperties {
    fn default() -> Self {
        Self {
            current_count: Default::default(),
            maximum_count: Default::default(),
            minimum_count: Default::default(),
        }
    }
}
///Workload profile type for container apps to execute on.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Workload profile type for container apps to execute on.",
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
pub struct WorkloadProfileType(pub ::std::string::String);
impl ::std::ops::Deref for WorkloadProfileType {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<WorkloadProfileType> for ::std::string::String {
    fn from(value: WorkloadProfileType) -> Self {
        value.0
    }
}
impl ::std::convert::From<&WorkloadProfileType> for WorkloadProfileType {
    fn from(value: &WorkloadProfileType) -> Self {
        value.clone()
    }
}
impl ::std::convert::From<::std::string::String> for WorkloadProfileType {
    fn from(value: ::std::string::String) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for WorkloadProfileType {
    type Err = ::std::convert::Infallible;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.to_string()))
    }
}
impl ::std::fmt::Display for WorkloadProfileType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
