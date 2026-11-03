use alien_core::{deployer_secret_location, deployer_secret_vault_key, DeployerSecretLocationContext, VaultBinding};
fn main() {
    let context = DeployerSecretLocationContext {
        aws_account_id: Some("123456789012".into()),
        aws_region: Some("us-east-1".into()),
        gcp_project_id: Some("example-project".into()),
        azure_subscription_id: Some("example-subscription".into()),
        secret_label: Some("Test key".into()),
        ..Default::default()
    };
    let result: Vec<_> = [
        ("aws", VaultBinding::parameter_store("shell-check-deployer-secrets")),
        ("gcp", VaultBinding::secret_manager("shell-check-deployer-secrets")),
        ("azure", VaultBinding::key_vault("shellcheck")),
        ("kubernetes", VaultBinding::kubernetes_secret("default", "shell-check-deployer-secrets")),
        ("local", VaultBinding::local("deployer-secrets", "/tmp/unused")),
    ].into_iter().map(|(store,binding)| serde_json::json!({"store":store,"location":deployer_secret_location(&binding,&deployer_secret_vault_key("key"),&context).unwrap()})).collect();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
