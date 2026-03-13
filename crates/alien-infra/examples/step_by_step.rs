use alien_aws_clients::AwsClientConfig;
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Function, FunctionCode,
    LifecycleRule, ResourceLifecycle, Stack, StackSettings, StackState, Storage,
};
use std::collections::HashMap;
use std::time::Duration;

use alien_infra::{ClientConfig, StackExecutor, StepResult};
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Step-by-Step Stack Example");

    // ----- Define Resources -----
    let uploads_bucket = Storage::new("my-step-uploads".to_owned())
        .versioning(true)
        .build();

    let processed_bucket = Storage::new("my-step-processed".to_owned())
        .lifecycle_rules(vec![LifecycleRule {
            days: 30,
            prefix: Some("temp/".to_string()),
        }])
        .build();

    let image_processor = Function::new("my-step-processor".to_string())
        .code(FunctionCode::Image {
            image: "your-repo/image-processor:latest".to_string(),
        })
        .permissions("execution".to_string())
        .memory_mb(512)
        .environment(HashMap::from([(
            "UPLOAD_BUCKET_NAME".to_string(),
            uploads_bucket.id().to_string(),
        )]))
        .link(&uploads_bucket)
        .build();

    // ----- Build Stack -----
    let stack = Stack::new("my-step-by-step-stack".to_owned())
        .add(uploads_bucket.clone(), ResourceLifecycle::Frozen)
        .add(processed_bucket.clone(), ResourceLifecycle::Frozen)
        .add(image_processor.clone(), ResourceLifecycle::Live)
        .build();

    // ----- AWS Configuration -----
    let account_id = std::env::var("AWS_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    if std::env::var("AWS_ACCOUNT_ID").is_err() || std::env::var("AWS_REGION").is_err() {
        println!("WARN: AWS_ACCOUNT_ID or AWS_REGION not set, using placeholders.");
    }
    let aws_platform_struct = AwsClientConfig {
        account_id: account_id.clone(),
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
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

    // ----- Deployment Phase (Step-by-Step) -----
    println!("\nStarting step-by-step deployment...");
    let mut step_count = 0;
    loop {
        step_count += 1;
        let plan = exec.plan(&current_state)?;
        if !plan.creates.is_empty() || !plan.updates.is_empty() || !plan.deletes.is_empty() {
            println!(
                "Deployment Step {}: Plan -> Creates: {:?}, Updates: {:?}, Deletes: {:?}",
                step_count,
                plan.creates,
                plan.updates.keys(),
                plan.deletes
            );
        } else if step_count == 1 {
            println!(
                "Deployment Step {}: Initial plan is empty, likely validating existing state.",
                step_count
            );
        }

        let step_result: StepResult = exec.step(current_state).await?;
        current_state = step_result.next_state;

        // Check if deployment is complete by computing status
        if let Ok(status) = current_state.compute_stack_status() {
            if status == alien_core::StackStatus::Running {
                println!("Stack deployment complete after {} steps.", step_count);
                break;
            }
        }

        if let Some(delay_ms) = step_result.suggested_delay_ms {
            sleep(Duration::from_millis(delay_ms)).await;
        } else {
            sleep(Duration::from_millis(50)).await;
        }
        if step_count > 50 {
            eprintln!("Max deployment steps reached. Aborting.");
            return Err("Max deployment steps reached".into());
        }
    }

    // ----- Deletion Phase (Step-by-Step) -----
    println!("\nPreparing for step-by-step stack deletion...");

    // Create a minimal deployment config for deletion
    let delete_config = alien_core::DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
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

    println!("\nStarting step-by-step deletion...");
    step_count = 0;
    loop {
        step_count += 1;
        let plan = delete_executor.plan(&current_state)?;
        if !plan.deletes.is_empty() || !plan.creates.is_empty() || !plan.updates.is_empty() {
            println!(
                "Deletion Step {}: Plan -> Deletes: {:?}, Creates: {:?}, Updates: {:?}",
                step_count,
                plan.deletes,
                plan.creates,
                plan.updates.keys()
            );
        } else if step_count == 1 {
            println!(
                "Deletion Step {}: Initial plan is empty, likely validating.",
                step_count
            );
        }

        let step_result: StepResult = delete_executor.step(current_state).await?;
        current_state = step_result.next_state;

        // Check if deletion is complete by computing status
        if let Ok(status) = current_state.compute_stack_status() {
            if status == alien_core::StackStatus::Deleted {
                println!("Stack deletion complete after {} steps.", step_count);
                break;
            }
        }

        if let Some(delay_ms) = step_result.suggested_delay_ms {
            sleep(Duration::from_millis(delay_ms)).await;
        } else {
            sleep(Duration::from_millis(50)).await;
        }
        if step_count > 50 {
            eprintln!("Max deletion steps reached. Aborting.");
            return Err("Max deletion steps reached".into());
        }
    }

    println!("\nStep-by-Step Stack Example Finished.");
    Ok(())
}
