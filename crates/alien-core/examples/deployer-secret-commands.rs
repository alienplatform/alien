use alien_core::{DeployerSecretLocationContext, VaultBinding, deployer_secret_location};
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
        ("aws", VaultBinding::parameter_store("shell-check")),
        ("gcp", VaultBinding::secret_manager("shell-check")),
        ("azure", VaultBinding::key_vault("shellcheck")),
        ("kubernetes", VaultBinding::kubernetes_secret("default", "shell-check")),
        ("local", VaultBinding::local("secrets", "/tmp/unused")),
    ].into_iter().map(|(store,binding)| serde_json::json!({"store":store,"location":deployer_secret_location(&binding,"input-key",&context).unwrap()})).collect();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
