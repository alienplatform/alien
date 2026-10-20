//! Scratch (not committed): writes the Azure setup module for the compat-proof stack.
use super::helpers::render;
use alien_core::{
    permissions::PermissionProfile, ManagementPermissions, Platform, ResourceLifecycle, Stack,
    StackSettings, Vault, Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;

#[tokio::test]
async fn dump_compat_setup_module() {
    let Ok(dir) = std::env::var("DUMP_DIR") else { return };
    let stack = Stack::new("acme".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("app-secrets", ["vault/data-read"]),
        )
        .add(
            Vault::new("app-secrets".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image { image: "registry.example.com/api:1".to_string() })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .management(ManagementPermissions::Extend(
            PermissionProfile::new()
                .global(["vault/heartbeat"])
                .resource("app-secrets", ["vault/data-write"]),
        ))
        .build();
    let prepared =
        super::secrets_vault_management_tests::prepare_stack_for_dump(stack, Platform::Azure).await;
    std::fs::write(
        std::path::Path::new(&dir).join("prepared-stack.json"),
        serde_json::to_string_pretty(&prepared).unwrap(),
    )
    .ok();
    let module = render(&prepared, TerraformTarget::Azure, StackSettings::default());
    for (path, contents) in module.iter() {
        let target = std::path::Path::new(&dir).join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, contents).unwrap();
    }
}
