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
///The Azure Active Directory principal identifier and Azure built-in role that describes the access the principal will receive on the delegated resource in the managed tenant.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Azure Active Directory principal identifier and Azure built-in role that describes the access the principal will receive on the delegated resource in the managed tenant.",
///  "type": "object",
///  "required": [
///    "principalId",
///    "roleDefinitionId"
///  ],
///  "properties": {
///    "delegatedRoleDefinitionIds": {
///      "description": "The delegatedRoleDefinitionIds field is required when the roleDefinitionId refers to the User Access Administrator Role. It is the list of role definition ids which define all the permissions that the user in the authorization can assign to other principals.",
///      "type": "array",
///      "items": {
///        "type": "string",
///        "format": "uuid"
///      }
///    },
///    "principalId": {
///      "description": "The identifier of the Azure Active Directory principal.",
///      "type": "string"
///    },
///    "principalIdDisplayName": {
///      "description": "The display name of the Azure Active Directory principal.",
///      "type": "string"
///    },
///    "roleDefinitionId": {
///      "description": "The identifier of the Azure built-in role that defines the permissions that the Azure Active Directory principal will have on the projected scope.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Authorization {
    ///The delegatedRoleDefinitionIds field is required when the roleDefinitionId refers to the User Access Administrator Role. It is the list of role definition ids which define all the permissions that the user in the authorization can assign to other principals.
    #[serde(
        rename = "delegatedRoleDefinitionIds",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub delegated_role_definition_ids: ::std::vec::Vec<::uuid::Uuid>,
    ///The identifier of the Azure Active Directory principal.
    #[serde(rename = "principalId")]
    pub principal_id: ::std::string::String,
    ///The display name of the Azure Active Directory principal.
    #[serde(
        rename = "principalIdDisplayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id_display_name: ::std::option::Option<::std::string::String>,
    ///The identifier of the Azure built-in role that defines the permissions that the Azure Active Directory principal will have on the projected scope.
    #[serde(rename = "roleDefinitionId")]
    pub role_definition_id: ::std::string::String,
}
impl ::std::convert::From<&Authorization> for Authorization {
    fn from(value: &Authorization) -> Self {
        value.clone()
    }
}
///Defines the Azure Active Directory principal that can approve any just-in-time access requests by the principal defined in the EligibleAuthorization.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Defines the Azure Active Directory principal that can approve any just-in-time access requests by the principal defined in the EligibleAuthorization.",
///  "type": "object",
///  "required": [
///    "principalId"
///  ],
///  "properties": {
///    "principalId": {
///      "description": "The identifier of the Azure Active Directory principal.",
///      "type": "string"
///    },
///    "principalIdDisplayName": {
///      "description": "The display name of the Azure Active Directory principal.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EligibleApprover {
    ///The identifier of the Azure Active Directory principal.
    #[serde(rename = "principalId")]
    pub principal_id: ::std::string::String,
    ///The display name of the Azure Active Directory principal.
    #[serde(
        rename = "principalIdDisplayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id_display_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&EligibleApprover> for EligibleApprover {
    fn from(value: &EligibleApprover) -> Self {
        value.clone()
    }
}
///The Azure Active Directory principal identifier, Azure built-in role, and just-in-time access policy that describes the just-in-time access the principal will receive on the delegated resource in the managed tenant.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The Azure Active Directory principal identifier, Azure built-in role, and just-in-time access policy that describes the just-in-time access the principal will receive on the delegated resource in the managed tenant.",
///  "type": "object",
///  "required": [
///    "principalId",
///    "roleDefinitionId"
///  ],
///  "properties": {
///    "justInTimeAccessPolicy": {
///      "$ref": "#/components/schemas/JustInTimeAccessPolicy"
///    },
///    "principalId": {
///      "description": "The identifier of the Azure Active Directory principal.",
///      "type": "string"
///    },
///    "principalIdDisplayName": {
///      "description": "The display name of the Azure Active Directory principal.",
///      "type": "string"
///    },
///    "roleDefinitionId": {
///      "description": "The identifier of the Azure built-in role that defines the permissions that the Azure Active Directory principal will have on the projected scope.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct EligibleAuthorization {
    #[serde(
        rename = "justInTimeAccessPolicy",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub just_in_time_access_policy: ::std::option::Option<JustInTimeAccessPolicy>,
    ///The identifier of the Azure Active Directory principal.
    #[serde(rename = "principalId")]
    pub principal_id: ::std::string::String,
    ///The display name of the Azure Active Directory principal.
    #[serde(
        rename = "principalIdDisplayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub principal_id_display_name: ::std::option::Option<::std::string::String>,
    ///The identifier of the Azure built-in role that defines the permissions that the Azure Active Directory principal will have on the projected scope.
    #[serde(rename = "roleDefinitionId")]
    pub role_definition_id: ::std::string::String,
}
impl ::std::convert::From<&EligibleAuthorization> for EligibleAuthorization {
    fn from(value: &EligibleAuthorization) -> Self {
        value.clone()
    }
}
///The error response indicating why the incoming request wasn’t able to be processed
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The error response indicating why the incoming request wasn’t able to be processed",
///  "type": "object",
///  "required": [
///    "code",
///    "message"
///  ],
///  "properties": {
///    "code": {
///      "description": "The error code.",
///      "type": "string"
///    },
///    "details": {
///      "description": "The internal error details.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/ErrorDefinition"
///      }
///    },
///    "message": {
///      "description": "The error message indicating why the operation failed.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct ErrorDefinition {
    ///The error code.
    pub code: ::std::string::String,
    ///The internal error details.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub details: ::std::vec::Vec<ErrorDefinition>,
    ///The error message indicating why the operation failed.
    pub message: ::std::string::String,
}
impl ::std::convert::From<&ErrorDefinition> for ErrorDefinition {
    fn from(value: &ErrorDefinition) -> Self {
        value.clone()
    }
}
///Error response.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Error response.",
///  "properties": {
///    "error": {
///      "$ref": "#/components/schemas/ErrorDefinition"
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
    pub error: ::std::option::Option<ErrorDefinition>,
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
///Just-in-time access policy setting.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "Just-in-time access policy setting.",
///  "type": "object",
///  "required": [
///    "multiFactorAuthProvider"
///  ],
///  "properties": {
///    "managedByTenantApprovers": {
///      "description": "The list of managedByTenant approvers for the eligible authorization.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EligibleApprover"
///      }
///    },
///    "maximumActivationDuration": {
///      "description": "The maximum access duration in ISO 8601 format for just-in-time access requests.",
///      "default": "PT8H",
///      "type": "string",
///      "format": "duration"
///    },
///    "multiFactorAuthProvider": {
///      "description": "The multi-factor authorization provider to be used for just-in-time access requests.",
///      "default": "None",
///      "type": "string",
///      "enum": [
///        "Azure",
///        "None"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "MultiFactorAuthProvider"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct JustInTimeAccessPolicy {
    ///The list of managedByTenant approvers for the eligible authorization.
    #[serde(
        rename = "managedByTenantApprovers",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by_tenant_approvers: ::std::vec::Vec<EligibleApprover>,
    ///The maximum access duration in ISO 8601 format for just-in-time access requests.
    #[serde(
        rename = "maximumActivationDuration",
        default = "defaults::just_in_time_access_policy_maximum_activation_duration",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub maximum_activation_duration: ::std::string::String,
    ///The multi-factor authorization provider to be used for just-in-time access requests.
    #[serde(rename = "multiFactorAuthProvider")]
    pub multi_factor_auth_provider: JustInTimeAccessPolicyMultiFactorAuthProvider,
}
impl ::std::convert::From<&JustInTimeAccessPolicy> for JustInTimeAccessPolicy {
    fn from(value: &JustInTimeAccessPolicy) -> Self {
        value.clone()
    }
}
///The multi-factor authorization provider to be used for just-in-time access requests.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The multi-factor authorization provider to be used for just-in-time access requests.",
///  "default": "None",
///  "type": "string",
///  "enum": [
///    "Azure",
///    "None"
///  ],
///  "x-ms-enum": {
///    "modelAsString": true,
///    "name": "MultiFactorAuthProvider"
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
pub enum JustInTimeAccessPolicyMultiFactorAuthProvider {
    Azure,
    None,
}
impl ::std::convert::From<&Self> for JustInTimeAccessPolicyMultiFactorAuthProvider {
    fn from(value: &JustInTimeAccessPolicyMultiFactorAuthProvider) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for JustInTimeAccessPolicyMultiFactorAuthProvider {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Azure => f.write_str("Azure"),
            Self::None => f.write_str("None"),
        }
    }
}
impl ::std::str::FromStr for JustInTimeAccessPolicyMultiFactorAuthProvider {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "azure" => Ok(Self::Azure),
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for JustInTimeAccessPolicyMultiFactorAuthProvider {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for JustInTimeAccessPolicyMultiFactorAuthProvider {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for JustInTimeAccessPolicyMultiFactorAuthProvider {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::default::Default for JustInTimeAccessPolicyMultiFactorAuthProvider {
    fn default() -> Self {
        JustInTimeAccessPolicyMultiFactorAuthProvider::None
    }
}
///`MarketplaceRegistrationDefinition`
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The fully qualified path of the marketplace registration definition.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the marketplace registration definition.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "plan": {
///      "$ref": "#/components/schemas/Plan"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/MarketplaceRegistrationDefinitionProperties"
///    },
///    "type": {
///      "description": "The type of the Azure resource (Microsoft.ManagedServices/marketplaceRegistrationDefinitions).",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct MarketplaceRegistrationDefinition {
    ///The fully qualified path of the marketplace registration definition.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the marketplace registration definition.
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
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<MarketplaceRegistrationDefinitionProperties>,
    ///The type of the Azure resource (Microsoft.ManagedServices/marketplaceRegistrationDefinitions).
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&MarketplaceRegistrationDefinition>
for MarketplaceRegistrationDefinition {
    fn from(value: &MarketplaceRegistrationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for MarketplaceRegistrationDefinition {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            plan: Default::default(),
            properties: Default::default(),
            type_: Default::default(),
        }
    }
}
///The list of marketplace registration definitions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The list of marketplace registration definitions.",
///  "properties": {
///    "nextLink": {
///      "description": "The link to the next page of marketplace registration definitions.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of marketplace registration definitions.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/MarketplaceRegistrationDefinition"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct MarketplaceRegistrationDefinitionList {
    ///The link to the next page of marketplace registration definitions.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of marketplace registration definitions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<MarketplaceRegistrationDefinition>,
}
impl ::std::convert::From<&MarketplaceRegistrationDefinitionList>
for MarketplaceRegistrationDefinitionList {
    fn from(value: &MarketplaceRegistrationDefinitionList) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for MarketplaceRegistrationDefinitionList {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of the marketplace registration definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of the marketplace registration definition.",
///  "type": "object",
///  "required": [
///    "authorizations",
///    "managedByTenantId"
///  ],
///  "properties": {
///    "authorizations": {
///      "description": "The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Authorization"
///      }
///    },
///    "eligibleAuthorizations": {
///      "description": "The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EligibleAuthorization"
///      }
///    },
///    "managedByTenantId": {
///      "description": "The identifier of the managedBy tenant.",
///      "type": "string"
///    },
///    "offerDisplayName": {
///      "description": "The marketplace offer display name.",
///      "type": "string"
///    },
///    "planDisplayName": {
///      "description": "The marketplace plan display name.",
///      "type": "string"
///    },
///    "publisherDisplayName": {
///      "description": "The marketplace publisher display name.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct MarketplaceRegistrationDefinitionProperties {
    ///The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.
    pub authorizations: ::std::vec::Vec<Authorization>,
    ///The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.
    #[serde(
        rename = "eligibleAuthorizations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub eligible_authorizations: ::std::vec::Vec<EligibleAuthorization>,
    ///The identifier of the managedBy tenant.
    #[serde(rename = "managedByTenantId")]
    pub managed_by_tenant_id: ::std::string::String,
    ///The marketplace offer display name.
    #[serde(
        rename = "offerDisplayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub offer_display_name: ::std::option::Option<::std::string::String>,
    ///The marketplace plan display name.
    #[serde(
        rename = "planDisplayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub plan_display_name: ::std::option::Option<::std::string::String>,
    ///The marketplace publisher display name.
    #[serde(
        rename = "publisherDisplayName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub publisher_display_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&MarketplaceRegistrationDefinitionProperties>
for MarketplaceRegistrationDefinitionProperties {
    fn from(value: &MarketplaceRegistrationDefinitionProperties) -> Self {
        value.clone()
    }
}
///The object that describes a single Microsoft.ManagedServices operation.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The object that describes a single Microsoft.ManagedServices operation.",
///  "type": "object",
///  "properties": {
///    "display": {
///      "description": "The object that represents the operation.",
///      "readOnly": true,
///      "properties": {
///        "description": {
///          "description": "The description of the operation.",
///          "type": "string"
///        },
///        "operation": {
///          "description": "The operation type.",
///          "type": "string"
///        },
///        "provider": {
///          "description": "The service provider.",
///          "type": "string"
///        },
///        "resource": {
///          "description": "The resource on which the operation is performed.",
///          "type": "string"
///        }
///      }
///    },
///    "name": {
///      "description": "The operation name with the format: {provider}/{resource}/{operation}",
///      "readOnly": true,
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
    ///The operation name with the format: {provider}/{resource}/{operation}
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
///  "readOnly": true,
///  "properties": {
///    "description": {
///      "description": "The description of the operation.",
///      "type": "string"
///    },
///    "operation": {
///      "description": "The operation type.",
///      "type": "string"
///    },
///    "provider": {
///      "description": "The service provider.",
///      "type": "string"
///    },
///    "resource": {
///      "description": "The resource on which the operation is performed.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct OperationDisplay {
    ///The description of the operation.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The operation type.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub operation: ::std::option::Option<::std::string::String>,
    ///The service provider.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provider: ::std::option::Option<::std::string::String>,
    ///The resource on which the operation is performed.
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
///The list of the operations.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The list of the operations.",
///  "type": "object",
///  "properties": {
///    "value": {
///      "description": "The list of Microsoft.ManagedServices operations.",
///      "readOnly": true,
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
pub struct OperationList {
    ///The list of Microsoft.ManagedServices operations.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<Operation>,
}
impl ::std::convert::From<&OperationList> for OperationList {
    fn from(value: &OperationList) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for OperationList {
    fn default() -> Self {
        Self { value: Default::default() }
    }
}
///The details for the Managed Services offer’s plan in Azure Marketplace.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The details for the Managed Services offer’s plan in Azure Marketplace.",
///  "type": "object",
///  "required": [
///    "name",
///    "product",
///    "publisher",
///    "version"
///  ],
///  "properties": {
///    "name": {
///      "description": "Azure Marketplace plan name.",
///      "type": "string"
///    },
///    "product": {
///      "description": "Azure Marketplace product code.",
///      "type": "string"
///    },
///    "publisher": {
///      "description": "Azure Marketplace publisher ID.",
///      "type": "string"
///    },
///    "version": {
///      "description": "Azure Marketplace plan's version.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct Plan {
    ///Azure Marketplace plan name.
    pub name: ::std::string::String,
    ///Azure Marketplace product code.
    pub product: ::std::string::String,
    ///Azure Marketplace publisher ID.
    pub publisher: ::std::string::String,
    ///Azure Marketplace plan's version.
    pub version: ::std::string::String,
}
impl ::std::convert::From<&Plan> for Plan {
    fn from(value: &Plan) -> Self {
        value.clone()
    }
}
///The registration assignment.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The registration assignment.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The fully qualified path of the registration assignment.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the registration assignment.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RegistrationAssignmentProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    },
///    "type": {
///      "description": "The type of the Azure resource (Microsoft.ManagedServices/registrationAssignments).",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationAssignment {
    ///The fully qualified path of the registration assignment.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the registration assignment.
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
    pub properties: ::std::option::Option<RegistrationAssignmentProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the Azure resource (Microsoft.ManagedServices/registrationAssignments).
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistrationAssignment> for RegistrationAssignment {
    fn from(value: &RegistrationAssignment) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistrationAssignment {
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
///The list of registration assignments.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The list of registration assignments.",
///  "properties": {
///    "nextLink": {
///      "description": "The link to the next page of registration assignments.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of registration assignments.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RegistrationAssignment"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationAssignmentList {
    ///The link to the next page of registration assignments.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of registration assignments.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<RegistrationAssignment>,
}
impl ::std::convert::From<&RegistrationAssignmentList> for RegistrationAssignmentList {
    fn from(value: &RegistrationAssignmentList) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistrationAssignmentList {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of the registration assignment.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of the registration assignment.",
///  "type": "object",
///  "required": [
///    "registrationDefinitionId"
///  ],
///  "properties": {
///    "provisioningState": {
///      "description": "The current provisioning state of the registration assignment.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Accepted",
///        "Running",
///        "Ready",
///        "Creating",
///        "Created",
///        "Deleting",
///        "Deleted",
///        "Canceled",
///        "Failed",
///        "Succeeded",
///        "Updating"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "registrationDefinition": {
///      "description": "The registration definition associated with the registration assignment.",
///      "readOnly": true,
///      "type": "object",
///      "properties": {
///        "id": {
///          "description": "The fully qualified path of the registration definition.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "name": {
///          "description": "The name of the registration definition.",
///          "readOnly": true,
///          "type": "string"
///        },
///        "plan": {
///          "$ref": "#/components/schemas/Plan"
///        },
///        "properties": {
///          "description": "The properties of the registration definition associated with the registration assignment.",
///          "properties": {
///            "authorizations": {
///              "description": "The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///              "type": "array",
///              "items": {
///                "$ref": "#/components/schemas/Authorization"
///              }
///            },
///            "description": {
///              "description": "The description of the registration definition.",
///              "type": "string"
///            },
///            "eligibleAuthorizations": {
///              "description": "The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///              "type": "array",
///              "items": {
///                "$ref": "#/components/schemas/EligibleAuthorization"
///              }
///            },
///            "managedByTenantId": {
///              "description": "The identifier of the managedBy tenant.",
///              "type": "string"
///            },
///            "managedByTenantName": {
///              "description": "The name of the managedBy tenant.",
///              "type": "string"
///            },
///            "manageeTenantId": {
///              "description": "The identifier of the managed tenant.",
///              "type": "string"
///            },
///            "manageeTenantName": {
///              "description": "The name of the managed tenant.",
///              "type": "string"
///            },
///            "provisioningState": {
///              "description": "The current provisioning state of the registration definition.",
///              "type": "string",
///              "enum": [
///                "NotSpecified",
///                "Accepted",
///                "Running",
///                "Ready",
///                "Creating",
///                "Created",
///                "Deleting",
///                "Deleted",
///                "Canceled",
///                "Failed",
///                "Succeeded",
///                "Updating"
///              ],
///              "x-ms-enum": {
///                "modelAsString": true,
///                "name": "ProvisioningState"
///              }
///            },
///            "registrationDefinitionName": {
///              "description": "The name of the registration definition.",
///              "type": "string"
///            }
///          }
///        },
///        "systemData": {
///          "$ref": "#/components/schemas/systemData"
///        },
///        "type": {
///          "description": "The type of the Azure resource (Microsoft.ManagedServices/registrationDefinitions).",
///          "readOnly": true,
///          "type": "string"
///        }
///      }
///    },
///    "registrationDefinitionId": {
///      "description": "The fully qualified path of the registration definition.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationAssignmentProperties {
    ///The current provisioning state of the registration assignment.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        RegistrationAssignmentPropertiesProvisioningState,
    >,
    #[serde(
        rename = "registrationDefinition",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registration_definition: ::std::option::Option<
        RegistrationAssignmentPropertiesRegistrationDefinition,
    >,
    ///The fully qualified path of the registration definition.
    #[serde(rename = "registrationDefinitionId")]
    pub registration_definition_id: ::std::string::String,
}
impl ::std::convert::From<&RegistrationAssignmentProperties>
for RegistrationAssignmentProperties {
    fn from(value: &RegistrationAssignmentProperties) -> Self {
        value.clone()
    }
}
///The current provisioning state of the registration assignment.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The current provisioning state of the registration assignment.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Accepted",
///    "Running",
///    "Ready",
///    "Creating",
///    "Created",
///    "Deleting",
///    "Deleted",
///    "Canceled",
///    "Failed",
///    "Succeeded",
///    "Updating"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum RegistrationAssignmentPropertiesProvisioningState {
    NotSpecified,
    Accepted,
    Running,
    Ready,
    Creating,
    Created,
    Deleting,
    Deleted,
    Canceled,
    Failed,
    Succeeded,
    Updating,
}
impl ::std::convert::From<&Self> for RegistrationAssignmentPropertiesProvisioningState {
    fn from(value: &RegistrationAssignmentPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistrationAssignmentPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Accepted => f.write_str("Accepted"),
            Self::Running => f.write_str("Running"),
            Self::Ready => f.write_str("Ready"),
            Self::Creating => f.write_str("Creating"),
            Self::Created => f.write_str("Created"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Deleted => f.write_str("Deleted"),
            Self::Canceled => f.write_str("Canceled"),
            Self::Failed => f.write_str("Failed"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Updating => f.write_str("Updating"),
        }
    }
}
impl ::std::str::FromStr for RegistrationAssignmentPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "accepted" => Ok(Self::Accepted),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "creating" => Ok(Self::Creating),
            "created" => Ok(Self::Created),
            "deleting" => Ok(Self::Deleting),
            "deleted" => Ok(Self::Deleted),
            "canceled" => Ok(Self::Canceled),
            "failed" => Ok(Self::Failed),
            "succeeded" => Ok(Self::Succeeded),
            "updating" => Ok(Self::Updating),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
for RegistrationAssignmentPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for RegistrationAssignmentPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for RegistrationAssignmentPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The registration definition associated with the registration assignment.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The registration definition associated with the registration assignment.",
///  "readOnly": true,
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The fully qualified path of the registration definition.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the registration definition.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "plan": {
///      "$ref": "#/components/schemas/Plan"
///    },
///    "properties": {
///      "description": "The properties of the registration definition associated with the registration assignment.",
///      "properties": {
///        "authorizations": {
///          "description": "The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/Authorization"
///          }
///        },
///        "description": {
///          "description": "The description of the registration definition.",
///          "type": "string"
///        },
///        "eligibleAuthorizations": {
///          "description": "The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///          "type": "array",
///          "items": {
///            "$ref": "#/components/schemas/EligibleAuthorization"
///          }
///        },
///        "managedByTenantId": {
///          "description": "The identifier of the managedBy tenant.",
///          "type": "string"
///        },
///        "managedByTenantName": {
///          "description": "The name of the managedBy tenant.",
///          "type": "string"
///        },
///        "manageeTenantId": {
///          "description": "The identifier of the managed tenant.",
///          "type": "string"
///        },
///        "manageeTenantName": {
///          "description": "The name of the managed tenant.",
///          "type": "string"
///        },
///        "provisioningState": {
///          "description": "The current provisioning state of the registration definition.",
///          "type": "string",
///          "enum": [
///            "NotSpecified",
///            "Accepted",
///            "Running",
///            "Ready",
///            "Creating",
///            "Created",
///            "Deleting",
///            "Deleted",
///            "Canceled",
///            "Failed",
///            "Succeeded",
///            "Updating"
///          ],
///          "x-ms-enum": {
///            "modelAsString": true,
///            "name": "ProvisioningState"
///          }
///        },
///        "registrationDefinitionName": {
///          "description": "The name of the registration definition.",
///          "type": "string"
///        }
///      }
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    },
///    "type": {
///      "description": "The type of the Azure resource (Microsoft.ManagedServices/registrationDefinitions).",
///      "readOnly": true,
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationAssignmentPropertiesRegistrationDefinition {
    ///The fully qualified path of the registration definition.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the registration definition.
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
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<
        RegistrationAssignmentPropertiesRegistrationDefinitionProperties,
    >,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the Azure resource (Microsoft.ManagedServices/registrationDefinitions).
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistrationAssignmentPropertiesRegistrationDefinition>
for RegistrationAssignmentPropertiesRegistrationDefinition {
    fn from(value: &RegistrationAssignmentPropertiesRegistrationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistrationAssignmentPropertiesRegistrationDefinition {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            plan: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The properties of the registration definition associated with the registration assignment.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of the registration definition associated with the registration assignment.",
///  "properties": {
///    "authorizations": {
///      "description": "The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Authorization"
///      }
///    },
///    "description": {
///      "description": "The description of the registration definition.",
///      "type": "string"
///    },
///    "eligibleAuthorizations": {
///      "description": "The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EligibleAuthorization"
///      }
///    },
///    "managedByTenantId": {
///      "description": "The identifier of the managedBy tenant.",
///      "type": "string"
///    },
///    "managedByTenantName": {
///      "description": "The name of the managedBy tenant.",
///      "type": "string"
///    },
///    "manageeTenantId": {
///      "description": "The identifier of the managed tenant.",
///      "type": "string"
///    },
///    "manageeTenantName": {
///      "description": "The name of the managed tenant.",
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "The current provisioning state of the registration definition.",
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Accepted",
///        "Running",
///        "Ready",
///        "Creating",
///        "Created",
///        "Deleting",
///        "Deleted",
///        "Canceled",
///        "Failed",
///        "Succeeded",
///        "Updating"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "registrationDefinitionName": {
///      "description": "The name of the registration definition.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationAssignmentPropertiesRegistrationDefinitionProperties {
    ///The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub authorizations: ::std::vec::Vec<Authorization>,
    ///The description of the registration definition.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.
    #[serde(
        rename = "eligibleAuthorizations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub eligible_authorizations: ::std::vec::Vec<EligibleAuthorization>,
    ///The identifier of the managedBy tenant.
    #[serde(
        rename = "managedByTenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by_tenant_id: ::std::option::Option<::std::string::String>,
    ///The name of the managedBy tenant.
    #[serde(
        rename = "managedByTenantName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by_tenant_name: ::std::option::Option<::std::string::String>,
    ///The identifier of the managed tenant.
    #[serde(
        rename = "manageeTenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managee_tenant_id: ::std::option::Option<::std::string::String>,
    ///The name of the managed tenant.
    #[serde(
        rename = "manageeTenantName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managee_tenant_name: ::std::option::Option<::std::string::String>,
    ///The current provisioning state of the registration definition.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState,
    >,
    ///The name of the registration definition.
    #[serde(
        rename = "registrationDefinitionName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registration_definition_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<
    &RegistrationAssignmentPropertiesRegistrationDefinitionProperties,
> for RegistrationAssignmentPropertiesRegistrationDefinitionProperties {
    fn from(
        value: &RegistrationAssignmentPropertiesRegistrationDefinitionProperties,
    ) -> Self {
        value.clone()
    }
}
impl ::std::default::Default
for RegistrationAssignmentPropertiesRegistrationDefinitionProperties {
    fn default() -> Self {
        Self {
            authorizations: Default::default(),
            description: Default::default(),
            eligible_authorizations: Default::default(),
            managed_by_tenant_id: Default::default(),
            managed_by_tenant_name: Default::default(),
            managee_tenant_id: Default::default(),
            managee_tenant_name: Default::default(),
            provisioning_state: Default::default(),
            registration_definition_name: Default::default(),
        }
    }
}
///The current provisioning state of the registration definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The current provisioning state of the registration definition.",
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Accepted",
///    "Running",
///    "Ready",
///    "Creating",
///    "Created",
///    "Deleting",
///    "Deleted",
///    "Canceled",
///    "Failed",
///    "Succeeded",
///    "Updating"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    NotSpecified,
    Accepted,
    Running,
    Ready,
    Creating,
    Created,
    Deleting,
    Deleted,
    Canceled,
    Failed,
    Succeeded,
    Updating,
}
impl ::std::convert::From<&Self>
for RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    fn from(
        value: &RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState,
    ) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display
for RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Accepted => f.write_str("Accepted"),
            Self::Running => f.write_str("Running"),
            Self::Ready => f.write_str("Ready"),
            Self::Creating => f.write_str("Creating"),
            Self::Created => f.write_str("Created"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Deleted => f.write_str("Deleted"),
            Self::Canceled => f.write_str("Canceled"),
            Self::Failed => f.write_str("Failed"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Updating => f.write_str("Updating"),
        }
    }
}
impl ::std::str::FromStr
for RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "accepted" => Ok(Self::Accepted),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "creating" => Ok(Self::Creating),
            "created" => Ok(Self::Created),
            "deleting" => Ok(Self::Deleting),
            "deleted" => Ok(Self::Deleted),
            "canceled" => Ok(Self::Canceled),
            "failed" => Ok(Self::Failed),
            "succeeded" => Ok(Self::Succeeded),
            "updating" => Ok(Self::Updating),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
for RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for RegistrationAssignmentPropertiesRegistrationDefinitionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///The registration definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The registration definition.",
///  "type": "object",
///  "properties": {
///    "id": {
///      "description": "The fully qualified path of the registration definition.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "name": {
///      "description": "The name of the registration definition.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "plan": {
///      "$ref": "#/components/schemas/Plan"
///    },
///    "properties": {
///      "$ref": "#/components/schemas/RegistrationDefinitionProperties"
///    },
///    "systemData": {
///      "$ref": "#/components/schemas/systemData"
///    },
///    "type": {
///      "description": "The type of the Azure resource (Microsoft.ManagedServices/registrationDefinitions).",
///      "readOnly": true,
///      "type": "string"
///    }
///  },
///  "x-ms-azure-resource": true
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationDefinition {
    ///The fully qualified path of the registration definition.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub id: ::std::option::Option<::std::string::String>,
    ///The name of the registration definition.
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
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub properties: ::std::option::Option<RegistrationDefinitionProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub system_data: ::std::option::Option<SystemData>,
    ///The type of the Azure resource (Microsoft.ManagedServices/registrationDefinitions).
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub type_: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistrationDefinition> for RegistrationDefinition {
    fn from(value: &RegistrationDefinition) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistrationDefinition {
    fn default() -> Self {
        Self {
            id: Default::default(),
            name: Default::default(),
            plan: Default::default(),
            properties: Default::default(),
            system_data: Default::default(),
            type_: Default::default(),
        }
    }
}
///The list of registration definitions.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The list of registration definitions.",
///  "properties": {
///    "nextLink": {
///      "description": "The link to the next page of registration definitions.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "value": {
///      "description": "The list of registration definitions.",
///      "readOnly": true,
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/RegistrationDefinition"
///      }
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationDefinitionList {
    ///The link to the next page of registration definitions.
    #[serde(
        rename = "nextLink",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub next_link: ::std::option::Option<::std::string::String>,
    ///The list of registration definitions.
    #[serde(
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub value: ::std::vec::Vec<RegistrationDefinition>,
}
impl ::std::convert::From<&RegistrationDefinitionList> for RegistrationDefinitionList {
    fn from(value: &RegistrationDefinitionList) -> Self {
        value.clone()
    }
}
impl ::std::default::Default for RegistrationDefinitionList {
    fn default() -> Self {
        Self {
            next_link: Default::default(),
            value: Default::default(),
        }
    }
}
///The properties of a registration definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The properties of a registration definition.",
///  "type": "object",
///  "required": [
///    "authorizations",
///    "managedByTenantId"
///  ],
///  "properties": {
///    "authorizations": {
///      "description": "The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/Authorization"
///      }
///    },
///    "description": {
///      "description": "The description of the registration definition.",
///      "type": "string"
///    },
///    "eligibleAuthorizations": {
///      "description": "The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.",
///      "type": "array",
///      "items": {
///        "$ref": "#/components/schemas/EligibleAuthorization"
///      }
///    },
///    "managedByTenantId": {
///      "description": "The identifier of the managedBy tenant.",
///      "type": "string"
///    },
///    "managedByTenantName": {
///      "description": "The name of the managedBy tenant.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "manageeTenantId": {
///      "description": "The identifier of the managed tenant.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "manageeTenantName": {
///      "description": "The name of the managed tenant.",
///      "readOnly": true,
///      "type": "string"
///    },
///    "provisioningState": {
///      "description": "The current provisioning state of the registration definition.",
///      "readOnly": true,
///      "type": "string",
///      "enum": [
///        "NotSpecified",
///        "Accepted",
///        "Running",
///        "Ready",
///        "Creating",
///        "Created",
///        "Deleting",
///        "Deleted",
///        "Canceled",
///        "Failed",
///        "Succeeded",
///        "Updating"
///      ],
///      "x-ms-enum": {
///        "modelAsString": true,
///        "name": "ProvisioningState"
///      }
///    },
///    "registrationDefinitionName": {
///      "description": "The name of the registration definition.",
///      "type": "string"
///    }
///  }
///}
/// ```
/// </details>
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
pub struct RegistrationDefinitionProperties {
    ///The collection of authorization objects describing the access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.
    pub authorizations: ::std::vec::Vec<Authorization>,
    ///The description of the registration definition.
    #[serde(
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub description: ::std::option::Option<::std::string::String>,
    ///The collection of eligible authorization objects describing the just-in-time access Azure Active Directory principals in the managedBy tenant will receive on the delegated resource in the managed tenant.
    #[serde(
        rename = "eligibleAuthorizations",
        default,
        skip_serializing_if = "::std::vec::Vec::is_empty",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub eligible_authorizations: ::std::vec::Vec<EligibleAuthorization>,
    ///The identifier of the managedBy tenant.
    #[serde(rename = "managedByTenantId")]
    pub managed_by_tenant_id: ::std::string::String,
    ///The name of the managedBy tenant.
    #[serde(
        rename = "managedByTenantName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managed_by_tenant_name: ::std::option::Option<::std::string::String>,
    ///The identifier of the managed tenant.
    #[serde(
        rename = "manageeTenantId",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managee_tenant_id: ::std::option::Option<::std::string::String>,
    ///The name of the managed tenant.
    #[serde(
        rename = "manageeTenantName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub managee_tenant_name: ::std::option::Option<::std::string::String>,
    ///The current provisioning state of the registration definition.
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub provisioning_state: ::std::option::Option<
        RegistrationDefinitionPropertiesProvisioningState,
    >,
    ///The name of the registration definition.
    #[serde(
        rename = "registrationDefinitionName",
        default,
        skip_serializing_if = "::std::option::Option::is_none",
        deserialize_with = "serde_aux::field_attributes::deserialize_default_from_null"
    )]
    pub registration_definition_name: ::std::option::Option<::std::string::String>,
}
impl ::std::convert::From<&RegistrationDefinitionProperties>
for RegistrationDefinitionProperties {
    fn from(value: &RegistrationDefinitionProperties) -> Self {
        value.clone()
    }
}
///The current provisioning state of the registration definition.
///
/// <details><summary>JSON schema</summary>
///
/// ```json
///{
///  "description": "The current provisioning state of the registration definition.",
///  "readOnly": true,
///  "type": "string",
///  "enum": [
///    "NotSpecified",
///    "Accepted",
///    "Running",
///    "Ready",
///    "Creating",
///    "Created",
///    "Deleting",
///    "Deleted",
///    "Canceled",
///    "Failed",
///    "Succeeded",
///    "Updating"
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
    PartialOrd
)]
#[serde(try_from = "String")]
pub enum RegistrationDefinitionPropertiesProvisioningState {
    NotSpecified,
    Accepted,
    Running,
    Ready,
    Creating,
    Created,
    Deleting,
    Deleted,
    Canceled,
    Failed,
    Succeeded,
    Updating,
}
impl ::std::convert::From<&Self> for RegistrationDefinitionPropertiesProvisioningState {
    fn from(value: &RegistrationDefinitionPropertiesProvisioningState) -> Self {
        value.clone()
    }
}
impl ::std::fmt::Display for RegistrationDefinitionPropertiesProvisioningState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::NotSpecified => f.write_str("NotSpecified"),
            Self::Accepted => f.write_str("Accepted"),
            Self::Running => f.write_str("Running"),
            Self::Ready => f.write_str("Ready"),
            Self::Creating => f.write_str("Creating"),
            Self::Created => f.write_str("Created"),
            Self::Deleting => f.write_str("Deleting"),
            Self::Deleted => f.write_str("Deleted"),
            Self::Canceled => f.write_str("Canceled"),
            Self::Failed => f.write_str("Failed"),
            Self::Succeeded => f.write_str("Succeeded"),
            Self::Updating => f.write_str("Updating"),
        }
    }
}
impl ::std::str::FromStr for RegistrationDefinitionPropertiesProvisioningState {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value.to_ascii_lowercase().as_str() {
            "notspecified" => Ok(Self::NotSpecified),
            "accepted" => Ok(Self::Accepted),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "creating" => Ok(Self::Creating),
            "created" => Ok(Self::Created),
            "deleting" => Ok(Self::Deleting),
            "deleted" => Ok(Self::Deleted),
            "canceled" => Ok(Self::Canceled),
            "failed" => Ok(Self::Failed),
            "succeeded" => Ok(Self::Succeeded),
            "updating" => Ok(Self::Updating),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str>
for RegistrationDefinitionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String>
for RegistrationDefinitionPropertiesProvisioningState {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String>
for RegistrationDefinitionPropertiesProvisioningState {
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
/// Generation of default values for serde.
pub mod defaults {
    pub(super) fn just_in_time_access_policy_maximum_activation_duration() -> ::std::string::String {
        "PT8H".to_string()
    }
}
