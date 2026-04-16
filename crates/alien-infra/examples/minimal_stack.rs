use alien_aws_clients::AwsClientConfig;
use alien_core::{ResourceLifecycle, Stack, StackState, Storage};
use alien_infra::{ClientConfig, StackExecutor};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Minimal Stack Example");

    // ----- Define a Simple Resource -----
    let simple_bucket = Storage::new("my-minimal-bucket".to_owned())
        .versioning(false)
        .build();

    // ----- Build Minimal Stack -----
    let stack = Stack::new("my-minimal-stack".to_owned())
        .add(simple_bucket.clone(), ResourceLifecycle::Frozen)
        .build();

    // ----- Choose the provider (AWS for this example) -----
    let account_id = std::env::var("AWS_ACCOUNT_ID").unwrap_or_else(|_| {
        println!("WARN: AWS_ACCOUNT_ID not set, using placeholder '123456789012'.");
        "123456789012".to_string()
    });
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| {
        println!("WARN: AWS_REGION not set, using placeholder 'us-east-1'.");
        "us-east-1".to_string()
    });
    let access_key_id = std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default();
    let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default();
    let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

    if access_key_id.is_empty() || secret_access_key.is_empty() {
        println!("WARN: AWS credentials not fully set. Operations might fail.");
    }

    let aws_platform_struct = AwsClientConfig {
        account_id: account_id.clone(),
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        },
        service_overrides: None,
    };
    let client_config = ClientConfig::Aws(Box::new(aws_platform_struct.clone()));

    // ----- Create Executor -----
    println!("\nInitializing Executor...");
    let exec = StackExecutor::new(&stack, client_config.clone(), None).map_err(|e| {
        eprintln!("Failed to initialize stack executor: {}", e);
        e
    })?;

    let mut current_state = StackState::new(client_config.platform());
    // println!("Initial State (before deployment):\n{}", serde_json::to_string_pretty(&current_state)?);

    // ----- Run 1: Initial creation -----
    println!("\nDeploying stack using run_until_synced...");
    match exec
        .run_until_synced(current_state.clone())
        .await
        .into_result()
    {
        Ok(new_state) => {
            current_state = new_state;
            println!("Stack deployment completed.");
            // println!("State after deployment:\n{}", serde_json::to_string_pretty(&current_state)?);
        }
        Err(e) => {
            eprintln!("Stack deployment failed: {}", e);
            return Err(e.into());
        }
    }

    // ----- Deletion Phase -----
    println!("\nPreparing for stack deletion...");

    // Create a minimal deployment config for deletion
    let delete_config = alien_core::DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();

    let delete_executor = StackExecutor::for_deletion(client_config, &delete_config, None)
        .map_err(|e| {
            eprintln!("Failed to create deletion executor: {}", e);
            e
        })?;

    println!("Deleting stack using run_until_synced...");
    match delete_executor
        .run_until_synced(current_state)
        .await
        .into_result()
    {
        Ok(_final_state) => {
            println!("Stack deletion successful.");
            // println!("Final State after deletion:\n{}", serde_json::to_string_pretty(&_final_state)?);
        }
        Err(e) => {
            eprintln!("Stack deletion failed: {}", e);
            return Err(e.into());
        }
    }

    println!("\nMinimal Stack Example Finished.");
    Ok(())
}
