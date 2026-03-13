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
///The action that will be executed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The action that will be executed.",
///  "type": "object",
///  "properties": {
///    "action_type": {
///      "$ref": "#/components/schemas/CertificatePolicyAction"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Action {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action_type: ::std::option::Option<CertificatePolicyAction>,
}
impl ::std::convert::From<&Action> for Action {
    fn from(value: &Action) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Action {
    fn default() -> Self {
        Self {
            action_type: Default::default(),
        }
    }
}
///Details of the organization administrator of the certificate issuer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Details of the organization administrator of the certificate issuer.",
///  "type": "object",
///  "properties": {
///    "email": {
///      "description": "Email address.",
///      "type": "string",
///      "x-ms-client-name": "EmailAddress"
///    },
///    "first_name": {
///      "description": "First name.",
///      "type": "string"
///    },
///    "last_name": {
///      "description": "Last name.",
///      "type": "string"
///    },
///    "phone": {
///      "description": "Phone number.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct AdministratorDetails {
    ///Email address.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub email: ::std::option::Option<::std::string::String>,
    ///First name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub first_name: ::std::option::Option<::std::string::String>,
    ///Last name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub last_name: ::std::option::Option<::std::string::String>,
    ///Phone number.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub phone: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&AdministratorDetails> for AdministratorDetails {
    fn from(value: &AdministratorDetails) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for AdministratorDetails {
    fn default() -> Self {
        Self {
            email: Default::default(),
            first_name: Default::default(),
            last_name: Default::default(),
            phone: Default::default(),
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
///The backup certificate result, containing the backup blob.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The backup certificate result, containing the backup blob.",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "The backup blob containing the backed up certificate.",
///      "readOnly": true,
///      "type": "string",
///      "format": "base64url"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct BackupCertificateResult {
    ///The backup blob containing the backed up certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&BackupCertificateResult> for BackupCertificateResult {
    fn from(value: &BackupCertificateResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for BackupCertificateResult {
    fn default() -> Self {
        Self { value: Default::default() }
    }
}
///The certificate management attributes.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate management attributes.",
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
pub struct CertificateAttributes {
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
impl ::std::convert::From<&CertificateAttributes> for CertificateAttributes {
    fn from(value: &CertificateAttributes) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateAttributes {
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
///A certificate bundle consists of a certificate (X509) plus its attributes.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A certificate bundle consists of a certificate (X509) plus its attributes.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "cer": {
///      "description": "CER contents of x509 certificate.",
///      "type": "string",
///      "format": "byte"
///    },
///    "contentType": {
///      "description": "The content type of the secret. eg. 'application/x-pem-file' or 'application/x-pkcs12'.",
///      "type": "string"
///    },
///    "id": {
///      "description": "The certificate id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "kid": {
///      "description": "The key id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/CertificatePolicy"
///    },
///    "preserveCertOrder": {
///      "description": "Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.",
///      "type": "boolean"
///    },
///    "sid": {
///      "description": "The secret id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "x5t": {
///      "description": "Thumbprint of the certificate.",
///      "readOnly": true,
///      "type": "string",
///      "format": "base64url",
///      "x-ms-client-name": "X509Thumbprint"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateBundle {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    ///CER contents of x509 certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cer: ::std::option::Option<::std::string::String>,
    ///The content type of the secret. eg. 'application/x-pem-file' or 'application/x-pkcs12'.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///The certificate id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The key id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kid: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policy: ::std::option::Option<CertificatePolicy>,
    ///Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.
    #[serde(
        rename = "preserveCertOrder",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub preserve_cert_order: ::std::option::Option<bool>,
    ///The secret id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sid: ::std::option::Option<::std::string::String>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Thumbprint of the certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub x5t: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateBundle> for CertificateBundle {
    fn from(value: &CertificateBundle) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateBundle {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            cer: Default::default(),
            content_type: Default::default(),
            id: Default::default(),
            kid: Default::default(),
            policy: Default::default(),
            preserve_cert_order: Default::default(),
            sid: Default::default(),
            tags: Default::default(),
            x5t: Default::default(),
        }
    }
}
///The certificate create parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate create parameters.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/CertificatePolicy"
///    },
///    "preserveCertOrder": {
///      "description": "Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.",
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
pub struct CertificateCreateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policy: ::std::option::Option<CertificatePolicy>,
    ///Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.
    #[serde(
        rename = "preserveCertOrder",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub preserve_cert_order: ::std::option::Option<bool>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&CertificateCreateParameters> for CertificateCreateParameters {
    fn from(value: &CertificateCreateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateCreateParameters {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            policy: Default::default(),
            preserve_cert_order: Default::default(),
            tags: Default::default(),
        }
    }
}
///The certificate import parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate import parameters.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/CertificatePolicy"
///    },
///    "preserveCertOrder": {
///      "description": "Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.",
///      "type": "boolean"
///    },
///    "pwd": {
///      "description": "If the private key in base64EncodedCertificate is encrypted, the password used for encryption.",
///      "type": "string",
///      "x-ms-client-name": "password"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "value": {
///      "description": "Base64 encoded representation of the certificate object to import. This certificate needs to contain the private key.",
///      "type": "string",
///      "x-ms-client-name": "base64EncodedCertificate"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateImportParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policy: ::std::option::Option<CertificatePolicy>,
    ///Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.
    #[serde(
        rename = "preserveCertOrder",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub preserve_cert_order: ::std::option::Option<bool>,
    ///If the private key in base64EncodedCertificate is encrypted, the password used for encryption.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub pwd: ::std::option::Option<::std::string::String>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Base64 encoded representation of the certificate object to import. This certificate needs to contain the private key.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&CertificateImportParameters> for CertificateImportParameters {
    fn from(value: &CertificateImportParameters) -> Self {
        value.clone()
    }
}
///The certificate issuer item containing certificate issuer metadata.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate issuer item containing certificate issuer metadata.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "Certificate Identifier.",
///      "type": "string"
///    },
///    "provider": {
///      "description": "The issuer provider.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateIssuerItem {
    ///Certificate Identifier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The issuer provider.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateIssuerItem> for CertificateIssuerItem {
    fn from(value: &CertificateIssuerItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateIssuerItem {
    fn default() -> Self {
        Self {
            id: Default::default(),
            provider: Default::default(),
        }
    }
}
///The certificate issuer list result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate issuer list result.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of certificate issuers.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "A response message containing a list of certificate issuers in the key vault along with a link to the next page of certificate issuers.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CertificateIssuerItem"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateIssuerListResult {
    ///The URL to get the next set of certificate issuers.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A response message containing a list of certificate issuers in the key vault along with a link to the next page of certificate issuers.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<CertificateIssuerItem>,
}
impl ::std::convert::From<&CertificateIssuerListResult> for CertificateIssuerListResult {
    fn from(value: &CertificateIssuerListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateIssuerListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The certificate issuer set parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate issuer set parameters.",
///  "type": "object",
///  "required": [
///    "provider"
///  ],
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/IssuerAttributes"
///    },
///    "credentials": {
///      "$ref": "#/components/schemas/IssuerCredentials"
///    },
///    "org_details": {
///      "$ref": "#/components/schemas/OrganizationDetails"
///    },
///    "provider": {
///      "description": "The issuer provider.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateIssuerSetParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<IssuerAttributes>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credentials: ::std::option::Option<IssuerCredentials>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub org_details: ::std::option::Option<OrganizationDetails>,
    ///The issuer provider.
    pub provider: ::std::string::String,
}
impl ::std::convert::From<&CertificateIssuerSetParameters>
for CertificateIssuerSetParameters {
    fn from(value: &CertificateIssuerSetParameters) -> Self {
        value.clone()
    }
}
///The certificate issuer update parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate issuer update parameters.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/IssuerAttributes"
///    },
///    "credentials": {
///      "$ref": "#/components/schemas/IssuerCredentials"
///    },
///    "org_details": {
///      "$ref": "#/components/schemas/OrganizationDetails"
///    },
///    "provider": {
///      "description": "The issuer provider.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateIssuerUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<IssuerAttributes>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credentials: ::std::option::Option<IssuerCredentials>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub org_details: ::std::option::Option<OrganizationDetails>,
    ///The issuer provider.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateIssuerUpdateParameters>
for CertificateIssuerUpdateParameters {
    fn from(value: &CertificateIssuerUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateIssuerUpdateParameters {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            credentials: Default::default(),
            org_details: Default::default(),
            provider: Default::default(),
        }
    }
}
///The certificate item containing certificate metadata.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate item containing certificate metadata.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "id": {
///      "description": "Certificate identifier.",
///      "type": "string"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "x5t": {
///      "description": "Thumbprint of the certificate.",
///      "type": "string",
///      "format": "base64url",
///      "x-ms-client-name": "X509Thumbprint"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateItem {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    ///Certificate identifier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Thumbprint of the certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub x5t: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateItem> for CertificateItem {
    fn from(value: &CertificateItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateItem {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            id: Default::default(),
            tags: Default::default(),
            x5t: Default::default(),
        }
    }
}
///The certificate list result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate list result.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of certificates.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "A response message containing a list of certificates in the key vault along with a link to the next page of certificates.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/CertificateItem"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateListResult {
    ///The URL to get the next set of certificates.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A response message containing a list of certificates in the key vault along with a link to the next page of certificates.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<CertificateItem>,
}
impl ::std::convert::From<&CertificateListResult> for CertificateListResult {
    fn from(value: &CertificateListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateListResult {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The certificate merge parameters
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate merge parameters",
///  "type": "object",
///  "required": [
///    "x5c"
///  ],
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "x5c": {
///      "description": "The certificate or the certificate chain to merge.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "format": "byte"
///      },
///      "x-ms-client-name": "x509Certificates"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateMergeParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///The certificate or the certificate chain to merge.
    pub x5c: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&CertificateMergeParameters> for CertificateMergeParameters {
    fn from(value: &CertificateMergeParameters) -> Self {
        value.clone()
    }
}
///A certificate operation is returned in case of asynchronous requests.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A certificate operation is returned in case of asynchronous requests.",
///  "type": "object",
///  "properties": {
///    "cancellation_requested": {
///      "description": "Indicates if cancellation was requested on the certificate operation.",
///      "type": "boolean"
///    },
///    "csr": {
///      "description": "The certificate signing request (CSR) that is being used in the certificate operation.",
///      "type": "string",
///      "format": "byte"
///    },
///    "error": {
///      "$ref": "#/components/schemas/Error"
///    },
///    "id": {
///      "description": "The certificate id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "issuer": {
///      "$ref": "#/components/schemas/IssuerParameters"
///    },
///    "preserveCertOrder": {
///      "description": "Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.",
///      "type": "boolean"
///    },
///    "request_id": {
///      "description": "Identifier for the certificate operation.",
///      "type": "string"
///    },
///    "status": {
///      "description": "Status of the certificate operation.",
///      "type": "string"
///    },
///    "status_details": {
///      "description": "The status details of the certificate operation.",
///      "type": "string"
///    },
///    "target": {
///      "description": "Location which contains the result of the certificate operation.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateOperation {
    ///Indicates if cancellation was requested on the certificate operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cancellation_requested: ::std::option::Option<bool>,
    ///The certificate signing request (CSR) that is being used in the certificate operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub csr: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub error: ::std::option::Option<Error>,
    ///The certificate id.
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
    pub issuer: ::std::option::Option<IssuerParameters>,
    ///Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.
    #[serde(
        rename = "preserveCertOrder",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub preserve_cert_order: ::std::option::Option<bool>,
    ///Identifier for the certificate operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub request_id: ::std::option::Option<::std::string::String>,
    ///Status of the certificate operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status: ::std::option::Option<::std::string::String>,
    ///The status details of the certificate operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub status_details: ::std::option::Option<::std::string::String>,
    ///Location which contains the result of the certificate operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub target: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&CertificateOperation> for CertificateOperation {
    fn from(value: &CertificateOperation) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateOperation {
    fn default() -> Self {
        Self {
            cancellation_requested: Default::default(),
            csr: Default::default(),
            error: Default::default(),
            id: Default::default(),
            issuer: Default::default(),
            preserve_cert_order: Default::default(),
            request_id: Default::default(),
            status: Default::default(),
            status_details: Default::default(),
            target: Default::default(),
        }
    }
}
///The certificate operation update parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate operation update parameters.",
///  "type": "object",
///  "required": [
///    "cancellation_requested"
///  ],
///  "properties": {
///    "cancellation_requested": {
///      "description": "Indicates if cancellation was requested on the certificate operation.",
///      "type": "boolean"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateOperationUpdateParameter {
    ///Indicates if cancellation was requested on the certificate operation.
    pub cancellation_requested: bool,
}
impl ::std::convert::From<&CertificateOperationUpdateParameter>
for CertificateOperationUpdateParameter {
    fn from(value: &CertificateOperationUpdateParameter) -> Self {
        value.clone()
    }
}
///Management policy for a certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Management policy for a certificate.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "id": {
///      "description": "The certificate id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "issuer": {
///      "$ref": "#/components/schemas/IssuerParameters"
///    },
///    "key_props": {
///      "$ref": "#/components/schemas/KeyProperties"
///    },
///    "lifetime_actions": {
///      "description": "Actions that will be performed by Key Vault over the lifetime of a certificate.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/LifetimeAction"
///      },
///      "x-ms-client-name": "lifetimeActions"
///    },
///    "secret_props": {
///      "$ref": "#/components/schemas/SecretProperties"
///    },
///    "x509_props": {
///      "$ref": "#/components/schemas/X509CertificateProperties"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificatePolicy {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    ///The certificate id.
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
    pub issuer: ::std::option::Option<IssuerParameters>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_props: ::std::option::Option<KeyProperties>,
    ///Actions that will be performed by Key Vault over the lifetime of a certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lifetime_actions: ::std::vec::Vec<LifetimeAction>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub secret_props: ::std::option::Option<SecretProperties>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub x509_props: ::std::option::Option<X509CertificateProperties>,
}
impl ::std::convert::From<&CertificatePolicy> for CertificatePolicy {
    fn from(value: &CertificatePolicy) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificatePolicy {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            id: Default::default(),
            issuer: Default::default(),
            key_props: Default::default(),
            lifetime_actions: Default::default(),
            secret_props: Default::default(),
            x509_props: Default::default(),
        }
    }
}
///The type of the action.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of the action.",
///  "type": "string",
///  "enum": [
///    "EmailContacts",
///    "AutoRenew"
///  ],
///  "x-ms-enum": {
///    "modelAsString": false,
///    "name": "CertificatePolicyAction",
///    "values": [
///      {
///        "description": "A certificate policy that will email certificate contacts.",
///        "name": "EmailContacts",
///        "value": "EmailContacts"
///      },
///      {
///        "description": "A certificate policy that will auto-renew a certificate.",
///        "name": "AutoRenew",
///        "value": "AutoRenew"
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
pub enum CertificatePolicyAction {
    EmailContacts,
    AutoRenew,
}
impl ::std::convert::From<&Self> for CertificatePolicyAction {
    fn from(value: &CertificatePolicyAction) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for CertificatePolicyAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EmailContacts => f.write_str("EmailContacts"),
            Self::AutoRenew => f.write_str("AutoRenew"),
        }
    }
}
impl ::std::str::FromStr for CertificatePolicyAction {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "emailcontacts" => Ok(Self::EmailContacts),
            "autorenew" => Ok(Self::AutoRenew),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CertificatePolicyAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CertificatePolicyAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CertificatePolicyAction {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The certificate restore parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate restore parameters.",
///  "type": "object",
///  "required": [
///    "value"
///  ],
///  "properties": {
///    "value": {
///      "description": "The backup blob associated with a certificate bundle.",
///      "type": "string",
///      "format": "base64url",
///      "x-ms-client-name": "certificateBundleBackup"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct CertificateRestoreParameters {
    ///The backup blob associated with a certificate bundle.
    pub value: ::std::string::String,
}
impl ::std::convert::From<&CertificateRestoreParameters>
for CertificateRestoreParameters {
    fn from(value: &CertificateRestoreParameters) -> Self {
        value.clone()
    }
}
///The certificate update parameters.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The certificate update parameters.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/CertificatePolicy"
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
pub struct CertificateUpdateParameters {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policy: ::std::option::Option<CertificatePolicy>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
}
impl ::std::convert::From<&CertificateUpdateParameters> for CertificateUpdateParameters {
    fn from(value: &CertificateUpdateParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for CertificateUpdateParameters {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            policy: Default::default(),
            tags: Default::default(),
        }
    }
}
///The contact information for the vault certificates.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The contact information for the vault certificates.",
///  "type": "object",
///  "properties": {
///    "email": {
///      "description": "Email address.",
///      "type": "string",
///      "x-ms-client-name": "EmailAddress"
///    },
///    "name": {
///      "description": "Name.",
///      "type": "string"
///    },
///    "phone": {
///      "description": "Phone number.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Contact {
    ///Email address.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub email: ::std::option::Option<::std::string::String>,
    ///Name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
    ///Phone number.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub phone: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Contact> for Contact {
    fn from(value: &Contact) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Contact {
    fn default() -> Self {
        Self {
            email: Default::default(),
            name: Default::default(),
            phone: Default::default(),
        }
    }
}
///The contacts for the vault certificates.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The contacts for the vault certificates.",
///  "type": "object",
///  "properties": {
///    "contacts": {
///      "description": "The contact list for the vault certificates.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Contact"
///      },
///      "x-ms-client-name": "ContactList"
///    },
///    "id": {
///      "description": "Identifier for the contacts collection.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Contacts {
    ///The contact list for the vault certificates.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub contacts: ::std::vec::Vec<Contact>,
    ///Identifier for the contacts collection.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&Contacts> for Contacts {
    fn from(value: &Contacts) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Contacts {
    fn default() -> Self {
        Self {
            contacts: Default::default(),
            id: Default::default(),
        }
    }
}
///A Deleted Certificate consisting of its previous id, attributes and its tags, as well as information on when it will be purged.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A Deleted Certificate consisting of its previous id, attributes and its tags, as well as information on when it will be purged.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "cer": {
///      "description": "CER contents of x509 certificate.",
///      "type": "string",
///      "format": "byte"
///    },
///    "contentType": {
///      "description": "The content type of the secret. eg. 'application/x-pem-file' or 'application/x-pkcs12'.",
///      "type": "string"
///    },
///    "deletedDate": {
///      "description": "The time when the certificate was deleted, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "id": {
///      "description": "The certificate id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "kid": {
///      "description": "The key id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "policy": {
///      "$ref": "#/components/schemas/CertificatePolicy"
///    },
///    "preserveCertOrder": {
///      "description": "Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.",
///      "type": "boolean"
///    },
///    "recoveryId": {
///      "description": "The url of the recovery object, used to identify and recover the deleted certificate.",
///      "type": "string"
///    },
///    "scheduledPurgeDate": {
///      "description": "The time when the certificate is scheduled to be purged, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "sid": {
///      "description": "The secret id.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "tags": {
///      "description": "Application specific metadata in the form of key-value pairs.",
///      "type": "object",
///      "additionalProperties": {
///        "type": "string"
///      }
///    },
///    "x5t": {
///      "description": "Thumbprint of the certificate.",
///      "readOnly": true,
///      "type": "string",
///      "format": "base64url",
///      "x-ms-client-name": "X509Thumbprint"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedCertificateBundle {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    ///CER contents of x509 certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cer: ::std::option::Option<::std::string::String>,
    ///The content type of the secret. eg. 'application/x-pem-file' or 'application/x-pkcs12'.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub content_type: ::std::option::Option<::std::string::String>,
    ///The time when the certificate was deleted, in UTC
    #[serde(
        rename = "deletedDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deleted_date: ::std::option::Option<i64>,
    ///The certificate id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The key id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kid: ::std::option::Option<::std::string::String>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub policy: ::std::option::Option<CertificatePolicy>,
    ///Specifies whether the certificate chain preserves its original order. The default value is false, which sets the leaf certificate at index 0.
    #[serde(
        rename = "preserveCertOrder",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub preserve_cert_order: ::std::option::Option<bool>,
    ///The url of the recovery object, used to identify and recover the deleted certificate.
    #[serde(
        rename = "recoveryId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub recovery_id: ::std::option::Option<::std::string::String>,
    ///The time when the certificate is scheduled to be purged, in UTC
    #[serde(
        rename = "scheduledPurgeDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub scheduled_purge_date: ::std::option::Option<i64>,
    ///The secret id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sid: ::std::option::Option<::std::string::String>,
    ///Application specific metadata in the form of key-value pairs.
    #[serde(
        default,
        skip_serializing_if = ":: std :: collections :: HashMap::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub tags: ::std::collections::HashMap<::std::string::String, ::std::string::String>,
    ///Thumbprint of the certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub x5t: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedCertificateBundle> for DeletedCertificateBundle {
    fn from(value: &DeletedCertificateBundle) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedCertificateBundle {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            cer: Default::default(),
            content_type: Default::default(),
            deleted_date: Default::default(),
            id: Default::default(),
            kid: Default::default(),
            policy: Default::default(),
            preserve_cert_order: Default::default(),
            recovery_id: Default::default(),
            scheduled_purge_date: Default::default(),
            sid: Default::default(),
            tags: Default::default(),
            x5t: Default::default(),
        }
    }
}
///The deleted certificate item containing metadata about the deleted certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The deleted certificate item containing metadata about the deleted certificate.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/CertificateAttributes"
///    },
///    "deletedDate": {
///      "description": "The time when the certificate was deleted, in UTC",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "id": {
///      "description": "Certificate identifier.",
///      "type": "string"
///    },
///    "recoveryId": {
///      "description": "The url of the recovery object, used to identify and recover the deleted certificate.",
///      "type": "string"
///    },
///    "scheduledPurgeDate": {
///      "description": "The time when the certificate is scheduled to be purged, in UTC",
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
///    "x5t": {
///      "description": "Thumbprint of the certificate.",
///      "type": "string",
///      "format": "base64url",
///      "x-ms-client-name": "X509Thumbprint"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedCertificateItem {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<CertificateAttributes>,
    ///The time when the certificate was deleted, in UTC
    #[serde(
        rename = "deletedDate",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub deleted_date: ::std::option::Option<i64>,
    ///Certificate identifier.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The url of the recovery object, used to identify and recover the deleted certificate.
    #[serde(
        rename = "recoveryId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub recovery_id: ::std::option::Option<::std::string::String>,
    ///The time when the certificate is scheduled to be purged, in UTC
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
    ///Thumbprint of the certificate.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub x5t: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&DeletedCertificateItem> for DeletedCertificateItem {
    fn from(value: &DeletedCertificateItem) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedCertificateItem {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            deleted_date: Default::default(),
            id: Default::default(),
            recovery_id: Default::default(),
            scheduled_purge_date: Default::default(),
            tags: Default::default(),
            x5t: Default::default(),
        }
    }
}
///A list of certificates that have been deleted in this vault.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A list of certificates that have been deleted in this vault.",
///  "type": "object",
///  "properties": {
///    "nextLink": {
///      "description": "The URL to get the next set of deleted certificates.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "A response message containing a list of deleted certificates in the vault along with a link to the next page of deleted certificates.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/DeletedCertificateItem"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct DeletedCertificateListResult {
    ///The URL to get the next set of deleted certificates.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///A response message containing a list of deleted certificates in the vault along with a link to the next page of deleted certificates.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<DeletedCertificateItem>,
}
impl ::std::convert::From<&DeletedCertificateListResult>
for DeletedCertificateListResult {
    fn from(value: &DeletedCertificateListResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for DeletedCertificateListResult {
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
    PartialOrd
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
            Self::CustomizedRecoverablePurgeable => {
                f.write_str("CustomizedRecoverable+Purgeable")
            }
            Self::CustomizedRecoverable => f.write_str("CustomizedRecoverable"),
            Self::CustomizedRecoverableProtectedSubscription => {
                f.write_str("CustomizedRecoverable+ProtectedSubscription")
            }
        }
    }
}
impl ::std::str::FromStr for DeletionRecoveryLevel {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "purgeable" => Ok(Self::Purgeable),
            "recoverable+purgeable" => Ok(Self::RecoverablePurgeable),
            "recoverable" => Ok(Self::Recoverable),
            "recoverable+protectedsubscription" => {
                Ok(Self::RecoverableProtectedSubscription)
            }
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
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
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
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub innererror: ::std::boxed::Box<::std::option::Option<Error>>,
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
///The attributes of an issuer managed by the Key Vault service.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The attributes of an issuer managed by the Key Vault service.",
///  "type": "object",
///  "properties": {
///    "created": {
///      "description": "Creation time in UTC.",
///      "readOnly": true,
///      "type": "integer",
///      "format": "unixtime"
///    },
///    "enabled": {
///      "description": "Determines whether the issuer is enabled.",
///      "type": "boolean"
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
pub struct IssuerAttributes {
    ///Creation time in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub created: ::std::option::Option<i64>,
    ///Determines whether the issuer is enabled.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub enabled: ::std::option::Option<bool>,
    ///Last updated time in UTC.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub updated: ::std::option::Option<i64>,
}
impl ::std::convert::From<&IssuerAttributes> for IssuerAttributes {
    fn from(value: &IssuerAttributes) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IssuerAttributes {
    fn default() -> Self {
        Self {
            created: Default::default(),
            enabled: Default::default(),
            updated: Default::default(),
        }
    }
}
///The issuer for Key Vault certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The issuer for Key Vault certificate.",
///  "type": "object",
///  "properties": {
///    "attributes": {
///      "$ref": "#/components/schemas/IssuerAttributes"
///    },
///    "credentials": {
///      "$ref": "#/components/schemas/IssuerCredentials"
///    },
///    "id": {
///      "description": "Identifier for the issuer object.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "org_details": {
///      "$ref": "#/components/schemas/OrganizationDetails"
///    },
///    "provider": {
///      "description": "The issuer provider.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IssuerBundle {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub attributes: ::std::option::Option<IssuerAttributes>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub credentials: ::std::option::Option<IssuerCredentials>,
    ///Identifier for the issuer object.
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
    pub org_details: ::std::option::Option<OrganizationDetails>,
    ///The issuer provider.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IssuerBundle> for IssuerBundle {
    fn from(value: &IssuerBundle) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IssuerBundle {
    fn default() -> Self {
        Self {
            attributes: Default::default(),
            credentials: Default::default(),
            id: Default::default(),
            org_details: Default::default(),
            provider: Default::default(),
        }
    }
}
///The credentials to be used for the certificate issuer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The credentials to be used for the certificate issuer.",
///  "type": "object",
///  "properties": {
///    "account_id": {
///      "description": "The user name/account name/account id.",
///      "type": "string"
///    },
///    "pwd": {
///      "description": "The password/secret/account key.",
///      "type": "string",
///      "x-ms-client-name": "Password"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IssuerCredentials {
    ///The user name/account name/account id.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub account_id: ::std::option::Option<::std::string::String>,
    ///The password/secret/account key.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub pwd: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IssuerCredentials> for IssuerCredentials {
    fn from(value: &IssuerCredentials) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IssuerCredentials {
    fn default() -> Self {
        Self {
            account_id: Default::default(),
            pwd: Default::default(),
        }
    }
}
///Parameters for the issuer of the X509 component of a certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Parameters for the issuer of the X509 component of a certificate.",
///  "type": "object",
///  "properties": {
///    "cert_transparency": {
///      "description": "Indicates if the certificates generated under this policy should be published to certificate transparency logs.",
///      "type": "boolean",
///      "x-ms-client-name": "CertificateTransparency"
///    },
///    "cty": {
///      "description": "Certificate type as supported by the provider (optional); for example 'OV-SSL', 'EV-SSL'",
///      "type": "string",
///      "x-ms-client-name": "CertificateType"
///    },
///    "name": {
///      "description": "Name of the referenced issuer object or reserved names; for example, 'Self' or 'Unknown'.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct IssuerParameters {
    ///Indicates if the certificates generated under this policy should be published to certificate transparency logs.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cert_transparency: ::std::option::Option<bool>,
    ///Certificate type as supported by the provider (optional); for example 'OV-SSL', 'EV-SSL'
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub cty: ::std::option::Option<::std::string::String>,
    ///Name of the referenced issuer object or reserved names; for example, 'Self' or 'Unknown'.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&IssuerParameters> for IssuerParameters {
    fn from(value: &IssuerParameters) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for IssuerParameters {
    fn default() -> Self {
        Self {
            cert_transparency: Default::default(),
            cty: Default::default(),
            name: Default::default(),
        }
    }
}
///Elliptic curve name. For valid values, see JsonWebKeyCurveName.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Elliptic curve name. For valid values, see JsonWebKeyCurveName.",
///  "type": "string",
///  "enum": [
///    "P-256",
///    "P-384",
///    "P-521",
///    "P-256K"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "JsonWebKeyCurveName",
///    "values": [
///      {
///        "description": "The NIST P-256 elliptic curve, AKA SECG curve SECP256R1.",
///        "name": "P_256",
///        "value": "P-256"
///      },
///      {
///        "description": "The NIST P-384 elliptic curve, AKA SECG curve SECP384R1.",
///        "name": "P_384",
///        "value": "P-384"
///      },
///      {
///        "description": "The NIST P-521 elliptic curve, AKA SECG curve SECP521R1.",
///        "name": "P_521",
///        "value": "P-521"
///      },
///      {
///        "description": "The SECG SECP256K1 elliptic curve.",
///        "name": "P_256K",
///        "value": "P-256K"
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
pub enum JsonWebKeyCurveName {
    #[serde(rename = "P-256")]
    P256,
    #[serde(rename = "P-384")]
    P384,
    #[serde(rename = "P-521")]
    P521,
    #[serde(rename = "P-256K")]
    P256k,
}
impl ::std::convert::From<&Self> for JsonWebKeyCurveName {
    fn from(value: &JsonWebKeyCurveName) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for JsonWebKeyCurveName {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::P256 => f.write_str("P-256"),
            Self::P384 => f.write_str("P-384"),
            Self::P521 => f.write_str("P-521"),
            Self::P256k => f.write_str("P-256K"),
        }
    }
}
impl ::std::str::FromStr for JsonWebKeyCurveName {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "p-256" => Ok(Self::P256),
            "p-384" => Ok(Self::P384),
            "p-521" => Ok(Self::P521),
            "p-256k" => Ok(Self::P256k),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for JsonWebKeyCurveName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for JsonWebKeyCurveName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for JsonWebKeyCurveName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The type of key pair to be used for the certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The type of key pair to be used for the certificate.",
///  "type": "string",
///  "enum": [
///    "EC",
///    "EC-HSM",
///    "RSA",
///    "RSA-HSM",
///    "oct",
///    "oct-HSM"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "JsonWebKeyType",
///    "values": [
///      {
///        "description": "Elliptic Curve.",
///        "name": "EC",
///        "value": "EC"
///      },
///      {
///        "description": "Elliptic Curve with a private key which is not exportable from the HSM.",
///        "name": "EC_HSM",
///        "value": "EC-HSM"
///      },
///      {
///        "description": "RSA (https://tools.ietf.org/html/rfc3447).",
///        "name": "RSA",
///        "value": "RSA"
///      },
///      {
///        "description": "RSA with a private key which is not exportable from the HSM.",
///        "name": "RSA_HSM",
///        "value": "RSA-HSM"
///      },
///      {
///        "description": "Octet sequence (used to represent symmetric keys).",
///        "name": "oct",
///        "value": "oct"
///      },
///      {
///        "description": "Octet sequence with a private key which is not exportable from the HSM.",
///        "name": "oct_HSM",
///        "value": "oct-HSM"
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
pub enum JsonWebKeyType {
    #[serde(rename = "EC")]
    Ec,
    #[serde(rename = "EC-HSM")]
    EcHsm,
    #[serde(rename = "RSA")]
    Rsa,
    #[serde(rename = "RSA-HSM")]
    RsaHsm,
    #[serde(rename = "oct")]
    Oct,
    #[serde(rename = "oct-HSM")]
    OctHsm,
}
impl ::std::convert::From<&Self> for JsonWebKeyType {
    fn from(value: &JsonWebKeyType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for JsonWebKeyType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Ec => f.write_str("EC"),
            Self::EcHsm => f.write_str("EC-HSM"),
            Self::Rsa => f.write_str("RSA"),
            Self::RsaHsm => f.write_str("RSA-HSM"),
            Self::Oct => f.write_str("oct"),
            Self::OctHsm => f.write_str("oct-HSM"),
        }
    }
}
impl ::std::str::FromStr for JsonWebKeyType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "ec" => Ok(Self::Ec),
            "ec-hsm" => Ok(Self::EcHsm),
            "rsa" => Ok(Self::Rsa),
            "rsa-hsm" => Ok(Self::RsaHsm),
            "oct" => Ok(Self::Oct),
            "oct-hsm" => Ok(Self::OctHsm),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for JsonWebKeyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for JsonWebKeyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for JsonWebKeyType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///Properties of the key pair backing a certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the key pair backing a certificate.",
///  "type": "object",
///  "properties": {
///    "crv": {
///      "$ref": "#/components/schemas/JsonWebKeyCurveName"
///    },
///    "exportable": {
///      "description": "Indicates if the private key can be exported. Release policy must be provided when creating the first version of an exportable key.",
///      "type": "boolean"
///    },
///    "key_size": {
///      "description": "The key size in bits. For example: 2048, 3072, or 4096 for RSA.",
///      "type": "integer",
///      "format": "int32",
///      "x-ms-client-name": "keySize"
///    },
///    "kty": {
///      "$ref": "#/components/schemas/JsonWebKeyType"
///    },
///    "reuse_key": {
///      "description": "Indicates if the same key pair will be used on certificate renewal.",
///      "type": "boolean",
///      "x-ms-client-name": "reuseKey"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct KeyProperties {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub crv: ::std::option::Option<JsonWebKeyCurveName>,
    ///Indicates if the private key can be exported. Release policy must be provided when creating the first version of an exportable key.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub exportable: ::std::option::Option<bool>,
    ///The key size in bits. For example: 2048, 3072, or 4096 for RSA.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_size: ::std::option::Option<i32>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub kty: ::std::option::Option<JsonWebKeyType>,
    ///Indicates if the same key pair will be used on certificate renewal.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub reuse_key: ::std::option::Option<bool>,
}
impl ::std::convert::From<&KeyProperties> for KeyProperties {
    fn from(value: &KeyProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KeyProperties {
    fn default() -> Self {
        Self {
            crv: Default::default(),
            exportable: Default::default(),
            key_size: Default::default(),
            kty: Default::default(),
            reuse_key: Default::default(),
        }
    }
}
///Supported usages of a certificate key.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Supported usages of a certificate key.",
///  "type": "string",
///  "enum": [
///    "digitalSignature",
///    "nonRepudiation",
///    "keyEncipherment",
///    "dataEncipherment",
///    "keyAgreement",
///    "keyCertSign",
///    "cRLSign",
///    "encipherOnly",
///    "decipherOnly"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "KeyUsageType",
///    "values": [
///      {
///        "description": "Indicates that the certificate key can be used as a digital signature.",
///        "name": "digitalSignature",
///        "value": "digitalSignature"
///      },
///      {
///        "description": "Indicates that the certificate key can be used for authentication.",
///        "name": "nonRepudiation",
///        "value": "nonRepudiation"
///      },
///      {
///        "description": "Indicates that the certificate key can be used for key encryption.",
///        "name": "keyEncipherment",
///        "value": "keyEncipherment"
///      },
///      {
///        "description": "Indicates that the certificate key can be used for data encryption.",
///        "name": "dataEncipherment",
///        "value": "dataEncipherment"
///      },
///      {
///        "description": "Indicates that the certificate key can be used to determine key agreement, such as a key created using the Diffie-Hellman key agreement algorithm.",
///        "name": "keyAgreement",
///        "value": "keyAgreement"
///      },
///      {
///        "description": "Indicates that the certificate key can be used to sign certificates.",
///        "name": "keyCertSign",
///        "value": "keyCertSign"
///      },
///      {
///        "description": "Indicates that the certificate key can be used to sign a certificate revocation list.",
///        "name": "cRLSign",
///        "value": "cRLSign"
///      },
///      {
///        "description": "Indicates that the certificate key can be used for encryption only.",
///        "name": "encipherOnly",
///        "value": "encipherOnly"
///      },
///      {
///        "description": "Indicates that the certificate key can be used for decryption only.",
///        "name": "decipherOnly",
///        "value": "decipherOnly"
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
pub enum KeyUsageType {
    #[serde(rename = "digitalSignature")]
    DigitalSignature,
    #[serde(rename = "nonRepudiation")]
    NonRepudiation,
    #[serde(rename = "keyEncipherment")]
    KeyEncipherment,
    #[serde(rename = "dataEncipherment")]
    DataEncipherment,
    #[serde(rename = "keyAgreement")]
    KeyAgreement,
    #[serde(rename = "keyCertSign")]
    KeyCertSign,
    #[serde(rename = "cRLSign")]
    CRlSign,
    #[serde(rename = "encipherOnly")]
    EncipherOnly,
    #[serde(rename = "decipherOnly")]
    DecipherOnly,
}
impl ::std::convert::From<&Self> for KeyUsageType {
    fn from(value: &KeyUsageType) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for KeyUsageType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::DigitalSignature => f.write_str("digitalSignature"),
            Self::NonRepudiation => f.write_str("nonRepudiation"),
            Self::KeyEncipherment => f.write_str("keyEncipherment"),
            Self::DataEncipherment => f.write_str("dataEncipherment"),
            Self::KeyAgreement => f.write_str("keyAgreement"),
            Self::KeyCertSign => f.write_str("keyCertSign"),
            Self::CRlSign => f.write_str("cRLSign"),
            Self::EncipherOnly => f.write_str("encipherOnly"),
            Self::DecipherOnly => f.write_str("decipherOnly"),
        }
    }
}
impl ::std::str::FromStr for KeyUsageType {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "digitalsignature" => Ok(Self::DigitalSignature),
            "nonrepudiation" => Ok(Self::NonRepudiation),
            "keyencipherment" => Ok(Self::KeyEncipherment),
            "dataencipherment" => Ok(Self::DataEncipherment),
            "keyagreement" => Ok(Self::KeyAgreement),
            "keycertsign" => Ok(Self::KeyCertSign),
            "crlsign" => Ok(Self::CRlSign),
            "encipheronly" => Ok(Self::EncipherOnly),
            "decipheronly" => Ok(Self::DecipherOnly),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for KeyUsageType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for KeyUsageType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for KeyUsageType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
    pub error: ::std::option::Option<Error>,
}
impl ::std::convert::From<&KeyVaultError> for KeyVaultError {
    fn from(value: &KeyVaultError) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for KeyVaultError {
    fn default() -> Self {
        Self { error: Default::default() }
    }
}
///Action and its trigger that will be performed by Key Vault over the lifetime of a certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Action and its trigger that will be performed by Key Vault over the lifetime of a certificate.",
///  "type": "object",
///  "properties": {
///    "action": {
///      "$ref": "#/components/schemas/Action"
///    },
///    "trigger": {
///      "$ref": "#/components/schemas/Trigger"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct LifetimeAction {
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub action: ::std::option::Option<Action>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub trigger: ::std::option::Option<Trigger>,
}
impl ::std::convert::From<&LifetimeAction> for LifetimeAction {
    fn from(value: &LifetimeAction) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for LifetimeAction {
    fn default() -> Self {
        Self {
            action: Default::default(),
            trigger: Default::default(),
        }
    }
}
///Details of the organization of the certificate issuer.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Details of the organization of the certificate issuer.",
///  "type": "object",
///  "properties": {
///    "admin_details": {
///      "description": "Details of the organization administrator.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/AdministratorDetails"
///      }
///    },
///    "id": {
///      "description": "Id of the organization.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OrganizationDetails {
    ///Details of the organization administrator.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub admin_details: ::std::vec::Vec<AdministratorDetails>,
    ///Id of the organization.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&OrganizationDetails> for OrganizationDetails {
    fn from(value: &OrganizationDetails) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OrganizationDetails {
    fn default() -> Self {
        Self {
            admin_details: Default::default(),
            id: Default::default(),
        }
    }
}
///The pending certificate signing request result.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The pending certificate signing request result.",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "The pending certificate signing request as Base64 encoded string.",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct PendingCertificateSigningRequestResult {
    ///The pending certificate signing request as Base64 encoded string.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&PendingCertificateSigningRequestResult>
for PendingCertificateSigningRequestResult {
    fn from(value: &PendingCertificateSigningRequestResult) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for PendingCertificateSigningRequestResult {
    fn default() -> Self {
        Self { value: Default::default() }
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
///The Subject Alternative Names of a X509 object.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Subject Alternative Names of a X509 object.",
///  "type": "object",
///  "properties": {
///    "dns_names": {
///      "description": "Domain Names.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      },
///      "x-ms-client-name": "dnsNames"
///    },
///    "emails": {
///      "description": "Email addresses.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "upns": {
///      "description": "User Principal Names.",
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
pub struct SubjectAlternativeNames {
    ///Domain Names.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub dns_names: ::std::vec::Vec<::std::string::String>,
    ///Email addresses.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub emails: ::std::vec::Vec<::std::string::String>,
    ///User Principal Names.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub upns: ::std::vec::Vec<::std::string::String>,
}
impl ::std::convert::From<&SubjectAlternativeNames> for SubjectAlternativeNames {
    fn from(value: &SubjectAlternativeNames) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for SubjectAlternativeNames {
    fn default() -> Self {
        Self {
            dns_names: Default::default(),
            emails: Default::default(),
            upns: Default::default(),
        }
    }
}
///A condition to be satisfied for an action to be executed.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "A condition to be satisfied for an action to be executed.",
///  "type": "object",
///  "properties": {
///    "days_before_expiry": {
///      "description": "Days before expiry to attempt renewal. Value should be between 1 and validity_in_months multiplied by 27. If validity_in_months is 36, then value should be between 1 and 972 (36 * 27).",
///      "type": "integer",
///      "format": "int32",
///      "x-ms-client-name": "daysBeforeExpiry"
///    },
///    "lifetime_percentage": {
///      "description": "Percentage of lifetime at which to trigger. Value should be between 1 and 99.",
///      "type": "integer",
///      "format": "int32",
///      "maximum": 99.0,
///      "minimum": 1.0,
///      "x-ms-client-name": "lifetimePercentage"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Trigger {
    ///Days before expiry to attempt renewal. Value should be between 1 and validity_in_months multiplied by 27. If validity_in_months is 36, then value should be between 1 and 972 (36 * 27).
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub days_before_expiry: ::std::option::Option<i32>,
    ///Percentage of lifetime at which to trigger. Value should be between 1 and 99.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub lifetime_percentage: ::std::option::Option<::std::num::NonZeroU32>,
}
impl ::std::convert::From<&Trigger> for Trigger {
    fn from(value: &Trigger) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for Trigger {
    fn default() -> Self {
        Self {
            days_before_expiry: Default::default(),
            lifetime_percentage: Default::default(),
        }
    }
}
///Properties of the X509 component of a certificate.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Properties of the X509 component of a certificate.",
///  "type": "object",
///  "properties": {
///    "ekus": {
///      "description": "The enhanced key usage.",
///      "type": "array",
///      "items": {
///        "type": "string"
///      }
///    },
///    "key_usage": {
///      "description": "Defines how the certificate's key may be used.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/KeyUsageType"
///      },
///      "x-ms-client-name": "keyUsage"
///    },
///    "sans": {
///      "$ref": "#/components/schemas/SubjectAlternativeNames"
///    },
///    "subject": {
///      "description": "The subject name. Should be a valid X509 distinguished Name.",
///      "type": "string"
///    },
///    "validity_months": {
///      "description": "The duration that the certificate is valid in months.",
///      "type": "integer",
///      "format": "int32",
///      "minimum": 0.0,
///      "x-ms-client-name": "ValidityInMonths"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct X509CertificateProperties {
    ///The enhanced key usage.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub ekus: ::std::vec::Vec<::std::string::String>,
    ///Defines how the certificate's key may be used.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub key_usage: ::std::vec::Vec<KeyUsageType>,
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub sans: ::std::option::Option<SubjectAlternativeNames>,
    ///The subject name. Should be a valid X509 distinguished Name.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub subject: ::std::option::Option<::std::string::String>,
    ///The duration that the certificate is valid in months.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub validity_months: ::std::option::Option<i32>,
}
impl ::std::convert::From<&X509CertificateProperties> for X509CertificateProperties {
    fn from(value: &X509CertificateProperties) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for X509CertificateProperties {
    fn default() -> Self {
        Self {
            ekus: Default::default(),
            key_usage: Default::default(),
            sans: Default::default(),
            subject: Default::default(),
            validity_months: Default::default(),
        }
    }
}
