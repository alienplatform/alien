use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ContainerAppUserAssignedIdentityPolicy;

#[async_trait::async_trait]
impl azure_core_021::Policy for ContainerAppUserAssignedIdentityPolicy {
    async fn send(
        &self,
        ctx: &azure_core_021::Context,
        request: &mut azure_core_021::Request,
        next: &[Arc<dyn azure_core_021::Policy>],
    ) -> azure_core_021::PolicyResult {
        inject_user_assigned_identities(request)?;
        next[0].send(ctx, request, &next[1..]).await
    }
}

fn inject_user_assigned_identities(
    request: &mut azure_core_021::Request,
) -> azure_core_021::Result<()> {
    if !matches!(
        request.method(),
        &azure_core_021::Method::Put | &azure_core_021::Method::Patch
    ) || !request
        .url()
        .path()
        .contains("/providers/Microsoft.App/containerApps/")
    {
        return Ok(());
    }

    let azure_core_021::Body::Bytes(body) = request.body() else {
        return Ok(());
    };
    let mut body: serde_json::Value = serde_json::from_slice(body)?;
    let Some(identity_settings) = body
        .pointer("/properties/configuration/identitySettings")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(());
    };

    let mut user_assigned_identities = serde_json::Map::new();
    for identity in identity_settings.iter().filter_map(|identity_setting| {
        identity_setting
            .get("identity")
            .and_then(serde_json::Value::as_str)
            .filter(|identity| !identity.is_empty())
    }) {
        user_assigned_identities.insert(identity.to_string(), serde_json::json!({}));
    }

    if user_assigned_identities.is_empty() {
        return Ok(());
    }

    body["identity"] = serde_json::json!({
        "type": "UserAssigned",
        "userAssignedIdentities": user_assigned_identities,
    });
    let body = serde_json::to_vec(&body)?;
    request.insert_header(
        azure_core_021::headers::CONTENT_LENGTH,
        body.len().to_string(),
    );
    request.set_body(body);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_user_assigned_identity_map_from_generated_identity_settings() {
        let mut request = azure_core_021::Request::new(
            azure_core_021::Url::parse(
                "https://management.azure.com/subscriptions/sub/resourceGroups/rg/providers/Microsoft.App/containerApps/app?api-version=2024-08-02-preview",
            )
            .expect("test URL should parse"),
            azure_core_021::Method::Put,
        );
        request.set_body(
            serde_json::json!({
                "properties": {
                    "configuration": {
                        "identitySettings": [
                            {
                                "identity": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa",
                                "lifecycle": "All"
                            }
                        ]
                    }
                }
            })
            .to_string(),
        );

        inject_user_assigned_identities(&mut request).expect("identity injection should succeed");

        let azure_core_021::Body::Bytes(body) = request.body() else {
            panic!("test request should use a byte body");
        };
        let body: serde_json::Value =
            serde_json::from_slice(body).expect("mutated request body should be JSON");
        assert_eq!(body["identity"]["type"], "UserAssigned");
        assert_eq!(
            body["identity"]["userAssignedIdentities"]
                ["/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa"],
            serde_json::json!({})
        );
    }

    #[test]
    fn leaves_container_app_request_without_identity_settings_unchanged() {
        let body = serde_json::json!({
            "properties": {
                "configuration": {
                    "identitySettings": []
                }
            }
        })
        .to_string();
        let mut request = azure_core_021::Request::new(
            azure_core_021::Url::parse(
                "https://management.azure.com/subscriptions/sub/resourceGroups/rg/providers/Microsoft.App/containerApps/app?api-version=2024-08-02-preview",
            )
            .expect("test URL should parse"),
            azure_core_021::Method::Patch,
        );
        request.set_body(body.clone());

        inject_user_assigned_identities(&mut request).expect("identity injection should succeed");

        let azure_core_021::Body::Bytes(mutated_body) = request.body() else {
            panic!("test request should use a byte body");
        };
        assert_eq!(mutated_body.as_ref(), body.as_bytes());
    }
}
