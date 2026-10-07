use alien_core::{
    BindingValue, ContainerAppsEnvironmentBinding, ExternalBinding, ExternalBindings,
    S3StorageBinding, SecretReference, StorageBinding,
};
use serde_json::json;

#[test]
fn generated_manager_sdk_preserves_core_external_binding_coordinates_and_credentials() {
    for credential in [
        BindingValue::value("example-secret".to_owned()),
        BindingValue::SecretRef {
            secret_ref: SecretReference {
                name: "storage-auth".into(),
                key: "signing-key".into(),
            },
        },
        BindingValue::expression(json!({"Fn::GetAtt": ["Storage", "SigningKey"]})),
    ] {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "container-environment",
            ExternalBinding::ContainerAppsEnvironment(
                ContainerAppsEnvironmentBinding::new(
                    "shared-environment",
                    "/subscriptions/00000000-0000-0000-0000-000000000001/resourceGroups/shared-group/providers/Microsoft.App/managedEnvironments/shared-environment",
                    "shared-group",
                    "apps.example.com",
                )
                .with_static_ip("192.0.2.10"),
            ),
        );
        bindings.insert(
            "archive",
            ExternalBinding::Storage(StorageBinding::S3(S3StorageBinding {
                bucket_name: "archive-bucket".into(),
                endpoint: Some("https://storage.example.com".into()),
                region: Some("us-east-1".into()),
                force_path_style: Some(true),
                access_key_id: Some("example-access-key".into()),
                secret_access_key: Some(credential),
            })),
        );
        let sdk: alien_manager_api::types::ExternalBindings =
            serde_json::from_value(serde_json::to_value(&bindings).unwrap()).unwrap();
        let restored: ExternalBindings =
            serde_json::from_value(serde_json::to_value(sdk).unwrap()).unwrap();
        assert_eq!(restored, bindings);
    }
}
