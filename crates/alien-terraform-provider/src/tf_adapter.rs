//! Terraform plugin adapter built on the Rust `tf-provider` crate.
//!
//! This module owns only Terraform protocol concerns. The manager-facing
//! lifecycle remains in [`crate::resource_alien_deployment`].

use std::borrow::Cow;
use std::collections::HashMap;

use alien_manager_api::types::{ImportedResource, ResourceType};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tf_provider::schema::{
    Attribute, AttributeConstraint, AttributeType, Block, Description, Schema,
};
use tf_provider::value::{ValueAny, ValueEmpty, ValueList, ValueString};
use tf_provider::{map, serve, Diagnostics, Provider, Resource};

use crate::resource_alien_deployment::{build_client, create, delete, read, AlienDeploymentInput};

/// Runtime defaults supplied by the binary embedding this provider.
#[derive(Debug, Clone)]
pub struct ProviderOptions {
    pub provider_name: String,
    pub resource_type: String,
    pub default_manager_url: Option<String>,
}

impl Default for ProviderOptions {
    fn default() -> Self {
        Self {
            provider_name: "alien".to_string(),
            resource_type: "deployment".to_string(),
            default_manager_url: None,
        }
    }
}

/// Serve the Terraform provider using the existing Rust `tf-provider` adapter.
pub async fn serve_terraform_provider(options: ProviderOptions) -> anyhow::Result<()> {
    let provider_name = options.provider_name.clone();
    let provider = AlienProvider { options };
    serve(provider_name, provider).await
}

#[derive(Debug, Clone)]
struct AlienProvider {
    options: ProviderOptions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AlienProviderConfig {}

#[async_trait]
impl Provider for AlienProvider {
    type Config<'a> = AlienProviderConfig;
    type MetaState<'a> = ValueEmpty;

    fn schema(&self, _diags: &mut Diagnostics) -> Option<Schema> {
        Some(Schema {
            version: 1,
            block: Block {
                version: 1,
                description: Description::plain("Alien deployment registration provider"),
                attributes: HashMap::default(),
                ..Default::default()
            },
        })
    }

    async fn configure<'a>(
        &self,
        _diags: &mut Diagnostics,
        _terraform_version: String,
        _config: Self::Config<'a>,
    ) -> Option<()> {
        Some(())
    }

    fn get_resources(
        &self,
        _diags: &mut Diagnostics,
    ) -> Option<HashMap<String, Box<dyn tf_provider::DynamicResource>>> {
        Some(map! {
            self.options.resource_type.as_str() => AlienDeploymentResource {
                default_manager_url: self.options.default_manager_url.clone(),
            },
        })
    }
}

#[derive(Debug, Clone)]
struct AlienDeploymentResource {
    default_manager_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerraformImportedResource<'a> {
    #[serde(borrow = "'a")]
    id: ValueString<'a>,
    #[serde(rename = "type", borrow = "'a")]
    type_: ValueString<'a>,
    import_data: ValueAny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerraformDeploymentState<'a> {
    #[serde(default, borrow = "'a")]
    manager_url: ValueString<'a>,
    #[serde(borrow = "'a")]
    deployment_group_token: ValueString<'a>,
    #[serde(borrow = "'a")]
    name: ValueString<'a>,
    #[serde(borrow = "'a")]
    platform: ValueString<'a>,
    #[serde(borrow = "'a")]
    region: ValueString<'a>,
    management_config: ValueAny,
    stack_settings: ValueAny,
    resources: ValueList<TerraformImportedResource<'a>>,
    #[serde(default, borrow = "'a")]
    deployment_id: ValueString<'a>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TerraformPrivateState {}

#[async_trait]
impl Resource for AlienDeploymentResource {
    type State<'a> = TerraformDeploymentState<'a>;
    type PrivateState<'a> = TerraformPrivateState;
    type ProviderMetaState<'a> = ValueEmpty;

    fn schema(&self, _diags: &mut Diagnostics) -> Option<Schema> {
        Some(Schema {
            version: 1,
            block: Block {
                version: 1,
                description: Description::plain("Registers resolved Alien stack import data"),
                attributes: map! {
                    "manager_url" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("Alien Manager URL. Optional when the provider binary has a baked-in default."),
                        constraint: AttributeConstraint::Optional,
                        ..Default::default()
                    },
                    "deployment_group_token" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("Deployment-group token used to authorize the import."),
                        constraint: AttributeConstraint::Required,
                        sensitive: true,
                        ..Default::default()
                    },
                    "name" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain(
                            "Deployment name. Required and unique within the deployment group — \
                             the manager returns 409 on collision.",
                        ),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "platform" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("Imported stack platform: aws, gcp, azure, kubernetes, local, or test."),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "region" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("Cloud region or location for the imported stack."),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "management_config" => Attribute {
                        attr_type: AttributeType::Any,
                        description: Description::plain("Typed management configuration emitted by the distribution artifact."),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "stack_settings" => Attribute {
                        attr_type: AttributeType::Any,
                        description: Description::plain("Stack settings emitted by the distribution artifact."),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "resources" => Attribute {
                        attr_type: AttributeType::List(Box::new(AttributeType::Object(map! {
                            "id" => AttributeType::String,
                            "type" => AttributeType::String,
                            "import_data" => AttributeType::Any,
                        }))),
                        description: Description::plain("Resolved imported resources emitted by the distribution artifact."),
                        constraint: AttributeConstraint::Required,
                        ..Default::default()
                    },
                    "deployment_id" => Attribute {
                        attr_type: AttributeType::String,
                        description: Description::plain("Manager deployment id assigned after import."),
                        constraint: AttributeConstraint::Computed,
                        ..Default::default()
                    },
                },
                ..Default::default()
            },
        })
    }

    async fn validate<'a>(&self, diags: &mut Diagnostics, config: Self::State<'a>) -> Option<()> {
        if let Err(err) = self.to_input(&config) {
            diags.root_error_short(err);
            return None;
        }
        Some(())
    }

    async fn read<'a>(
        &self,
        diags: &mut Diagnostics,
        state: Self::State<'a>,
        private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        let input = match self.to_input(&state) {
            Ok(input) => input,
            Err(err) => {
                diags.root_error_short(err);
                return Some((state, private_state));
            }
        };
        let client = match build_client(&input) {
            Ok(client) => client,
            Err(err) => {
                diags.root_error_short(err.to_string());
                return Some((state, private_state));
            }
        };
        match read(&client, &input).await {
            Ok(()) => Some((state, private_state)),
            Err(err) => {
                diags.root_error_short(err.to_string());
                Some((state, private_state))
            }
        }
    }

    async fn plan_create<'a>(
        &self,
        _diags: &mut Diagnostics,
        mut proposed_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        if proposed_state.manager_url.is_null() {
            if let Some(default_manager_url) = &self.default_manager_url {
                proposed_state.manager_url = value_string(default_manager_url.clone());
            }
        }
        proposed_state.deployment_id = ValueString::Unknown;
        Some((proposed_state, TerraformPrivateState::default()))
    }

    async fn plan_update<'a>(
        &self,
        _diags: &mut Diagnostics,
        _prior_state: Self::State<'a>,
        mut proposed_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        prior_private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(
        Self::State<'a>,
        Self::PrivateState<'a>,
        Vec<tf_provider::AttributePath>,
    )> {
        proposed_state.deployment_id = ValueString::Unknown;
        Some((proposed_state, prior_private_state, Vec::new()))
    }

    async fn plan_destroy<'a>(
        &self,
        _diags: &mut Diagnostics,
        _prior_state: Self::State<'a>,
        prior_private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<Self::PrivateState<'a>> {
        Some(prior_private_state)
    }

    async fn create<'a>(
        &self,
        diags: &mut Diagnostics,
        planned_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        self.import_stack_into_state(diags, planned_state, private_state)
            .await
    }

    async fn update<'a>(
        &self,
        diags: &mut Diagnostics,
        _prior_state: Self::State<'a>,
        planned_state: Self::State<'a>,
        _config_state: Self::State<'a>,
        private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<(Self::State<'a>, Self::PrivateState<'a>)> {
        self.import_stack_into_state(diags, planned_state, private_state)
            .await
    }

    async fn destroy<'a>(
        &self,
        diags: &mut Diagnostics,
        state: Self::State<'a>,
        _private_state: Self::PrivateState<'a>,
        _provider_meta_state: Self::ProviderMetaState<'a>,
    ) -> Option<()> {
        let input = match self.to_input(&state) {
            Ok(input) => input,
            Err(err) => {
                diags.root_error_short(err);
                return None;
            }
        };
        let deployment_id = match known_string(&state.deployment_id, "deployment_id") {
            Ok(id) => id,
            Err(err) => {
                diags.root_error_short(err);
                return None;
            }
        };
        let client = match build_client(&input) {
            Ok(client) => client,
            Err(err) => {
                diags.root_error_short(err.to_string());
                return None;
            }
        };
        match delete(&client, &deployment_id).await {
            Ok(()) => Some(()),
            Err(err) => {
                diags.root_error_short(err.to_string());
                None
            }
        }
    }
}

impl AlienDeploymentResource {
    async fn import_stack_into_state<'a>(
        &self,
        diags: &mut Diagnostics,
        mut planned_state: TerraformDeploymentState<'a>,
        private_state: TerraformPrivateState,
    ) -> Option<(TerraformDeploymentState<'a>, TerraformPrivateState)> {
        match self.import_stack(&planned_state).await {
            Ok(deployment_id) => {
                planned_state.deployment_id = value_string(deployment_id);
                Some((planned_state, private_state))
            }
            Err(err) => {
                diags.root_error_short(err);
                None
            }
        }
    }

    async fn import_stack<'a>(
        &self,
        state: &TerraformDeploymentState<'a>,
    ) -> Result<String, String> {
        let input = self.to_input(state)?;
        let client = build_client(&input).map_err(|err| err.to_string())?;
        let response = create(&client, &input)
            .await
            .map_err(|err| err.to_string())?;
        Ok(response.deployment_id)
    }

    fn to_input<'a>(
        &self,
        state: &TerraformDeploymentState<'a>,
    ) -> Result<AlienDeploymentInput, String> {
        let manager_url = match optional_string(&state.manager_url) {
            Some(url) => Some(url),
            None => self.default_manager_url.clone(),
        };

        Ok(AlienDeploymentInput {
            manager_url,
            deployment_group_token: known_string(
                &state.deployment_group_token,
                "deployment_group_token",
            )?,
            name: known_string(&state.name, "name")?,
            platform: parse_string(&state.platform, "platform")?,
            region: known_string(&state.region, "region")?,
            management_config: decode_value_any(&state.management_config, "management_config")?,
            stack_settings: decode_value_any(&state.stack_settings, "stack_settings")?,
            resources: decode_resources(&state.resources)?,
        })
    }
}

fn value_string(value: String) -> ValueString<'static> {
    ValueString::Value(Cow::Owned(value))
}

fn optional_string(value: &ValueString<'_>) -> Option<String> {
    match value {
        ValueString::Value(value) if !value.is_empty() => Some(value.to_string()),
        _ => None,
    }
}

fn known_string(value: &ValueString<'_>, field: &str) -> Result<String, String> {
    match value {
        ValueString::Value(value) if !value.is_empty() => Ok(value.to_string()),
        ValueString::Value(_) | ValueString::Null => Err(format!("{field} is required")),
        ValueString::Unknown => Err(format!("{field} is not known yet")),
    }
}

fn parse_string<T>(value: &ValueString<'_>, field: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let value = known_string(value, field)?;
    value
        .parse()
        .map_err(|err| format!("invalid {field} '{value}': {err}"))
}

fn decode_value_any<T>(value: &ValueAny, field: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    match value {
        ValueAny::Unknown => Err(format!("{field} is not known yet")),
        ValueAny::Null => Err(format!("{field} is required")),
        _ => serde_json::from_value(value_any_to_json(value))
            .map_err(|err| format!("invalid {field}: {err}; value: {}", value.json())),
    }
}

fn decode_resources(
    value: &ValueList<TerraformImportedResource<'_>>,
) -> Result<Vec<ImportedResource>, String> {
    let resources = match value {
        ValueList::Value(resources) => resources,
        ValueList::Null => return Err("resources is required".to_string()),
        ValueList::Unknown => return Err("resources is not known yet".to_string()),
    };
    if resources.is_empty() {
        return Err("resources must contain at least one entry".to_string());
    }

    resources
        .iter()
        .map(|resource| {
            let import_data = match value_any_to_json(&resource.import_data) {
                serde_json::Value::Object(map) => map,
                _ => return Err("resources.import_data must be an object".to_string()),
            };

            Ok(ImportedResource {
                id: known_string(&resource.id, "resources.id")?,
                type_: ResourceType::from(known_string(&resource.type_, "resources.type")?),
                import_data,
            })
        })
        .collect()
}

fn value_any_to_json(value: &ValueAny) -> serde_json::Value {
    match value {
        ValueAny::String(value) => serde_json::Value::String(value.clone()),
        ValueAny::Number(value) => serde_json::Value::Number((*value).into()),
        ValueAny::Bool(value) => serde_json::Value::Bool(*value),
        ValueAny::List(items) if is_cty_dynamic_value(items) => value_any_to_json(&items[1]),
        ValueAny::List(items) => {
            serde_json::Value::Array(items.iter().map(value_any_to_json).collect())
        }
        ValueAny::Map(items) => serde_json::Value::Object(
            items
                .iter()
                .map(|(key, value)| (key.clone(), value_any_to_json(value)))
                .collect(),
        ),
        ValueAny::Null | ValueAny::Unknown => serde_json::Value::Null,
    }
}

fn is_cty_dynamic_value(items: &[ValueAny]) -> bool {
    matches!(items, [ValueAny::String(type_expr), _] if type_expr.starts_with("[\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_manager_api::types::Platform;
    use std::collections::BTreeMap;

    fn any_map(entries: impl IntoIterator<Item = (&'static str, ValueAny)>) -> ValueAny {
        ValueAny::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    fn cty_object(entries: impl IntoIterator<Item = (&'static str, ValueAny)>) -> ValueAny {
        let map: BTreeMap<_, _> = entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        let type_expr = format!(
            "[\"object\",{{{}}}]",
            map.keys()
                .map(|key| format!("\"{key}\":\"string\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        ValueAny::List(vec![ValueAny::String(type_expr), ValueAny::Map(map)])
    }

    fn state_with_manager_url(
        manager_url: ValueString<'static>,
    ) -> TerraformDeploymentState<'static> {
        TerraformDeploymentState {
            manager_url,
            deployment_group_token: value_string("dg_test".to_string()),
            name: value_string("acme-prod".to_string()),
            platform: value_string("aws".to_string()),
            region: value_string("us-east-1".to_string()),
            management_config: any_map([
                ("platform", ValueAny::String("aws".to_string())),
                (
                    "managingRoleArn",
                    ValueAny::String("arn:aws:iam::000000000000:role/alien-manager".to_string()),
                ),
            ]),
            stack_settings: ValueAny::Map(BTreeMap::new()),
            resources: ValueList::Value(vec![TerraformImportedResource {
                id: value_string("data".to_string()),
                type_: value_string("storage".to_string()),
                import_data: any_map([
                    ("bucketName", ValueAny::String("acme-data".to_string())),
                    (
                        "bucketArn",
                        ValueAny::String("arn:aws:s3:::acme-data".to_string()),
                    ),
                ]),
            }]),
            deployment_id: ValueString::Null,
        }
    }

    #[test]
    fn to_input_uses_default_manager_url_when_resource_value_is_null() {
        let resource = AlienDeploymentResource {
            default_manager_url: Some("https://manager.example.com".to_string()),
        };

        let input = resource
            .to_input(&state_with_manager_url(ValueString::Null))
            .expect("terraform state should decode");

        assert_eq!(
            input.manager_url.as_deref(),
            Some("https://manager.example.com")
        );
        assert_eq!(input.deployment_group_token, "dg_test");
        assert_eq!(input.name, "acme-prod");
        assert_eq!(input.platform, Platform::Aws);
        assert_eq!(input.region, "us-east-1");
        assert_eq!(input.resources.len(), 1);
        assert_eq!(input.resources[0].id, "data");
        assert_eq!(input.resources[0].type_.to_string(), "storage");
        assert_eq!(
            input.resources[0]
                .import_data
                .get("bucketName")
                .and_then(|value| value.as_str()),
            Some("acme-data")
        );
    }

    #[test]
    fn to_input_prefers_explicit_manager_url_over_default() {
        let resource = AlienDeploymentResource {
            default_manager_url: Some("https://default.example.com".to_string()),
        };

        let input = resource
            .to_input(&state_with_manager_url(value_string(
                "https://explicit.example.com".to_string(),
            )))
            .expect("terraform state should decode");

        assert_eq!(
            input.manager_url.as_deref(),
            Some("https://explicit.example.com")
        );
    }

    #[test]
    fn to_input_rejects_unknown_values_before_create() {
        let resource = AlienDeploymentResource {
            default_manager_url: None,
        };
        let mut state =
            state_with_manager_url(value_string("https://manager.example.com".to_string()));
        state.resources = ValueList::Unknown;

        let error = resource
            .to_input(&state)
            .expect_err("unknown resources should fail validation");

        assert_eq!(error, "resources is not known yet");
    }

    #[test]
    fn provider_options_register_terraform_resource_suffix() {
        assert_eq!(ProviderOptions::default().provider_name, "alien");
        assert_eq!(ProviderOptions::default().resource_type, "deployment");
    }

    #[test]
    fn decode_value_any_unwraps_terraform_dynamic_object_envelope() {
        let config: alien_manager_api::types::ManagementConfig = decode_value_any(
            &cty_object([
                ("platform", ValueAny::String("aws".to_string())),
                (
                    "managingRoleArn",
                    ValueAny::String("arn:aws:iam::000000000000:role/alien-manager".to_string()),
                ),
            ]),
            "management_config",
        )
        .expect("cty object should decode");

        assert_eq!(
            serde_json::to_value(config)
                .expect("management config json")
                .get("platform")
                .and_then(|value| value.as_str()),
            Some("aws")
        );
    }
}
