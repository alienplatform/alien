use std::path::Path;

use alien_core::import::data::{
    AwsArtifactRegistryImportData, AwsKvImportData, AwsStorageImportData,
};

use super::*;

pub(super) fn recovery_workdir_suffix(workdir: Option<&Path>) -> String {
    workdir
        .map(|path| format!("; workdir retained at {}", path.display()))
        .unwrap_or_default()
}

pub(super) async fn cleanup_after_setup_error(
    cleanup: DistributionArtifactCleanup,
    setup_error: anyhow::Error,
) -> anyhow::Error {
    cleanup_after_ordered_setup_error(vec![cleanup], setup_error).await
}

pub(super) async fn cleanup_after_ordered_setup_error(
    cleanups: Vec<DistributionArtifactCleanup>,
    setup_error: anyhow::Error,
) -> anyhow::Error {
    let mut cleanups = cleanups.into_iter();
    while let Some(cleanup) = cleanups.next() {
        if let Err(cleanup_error) = cleanup.cleanup().await {
            let retained = cleanups
                .map(DistributionArtifactCleanup::preserve_for_recovery)
                .collect::<Vec<_>>()
                .join("\n");
            let recovery = if retained.is_empty() {
                String::new()
            } else {
                format!(
                    " Subsequent distribution artifacts were retained to preserve cleanup order:\n{retained}"
                )
            };
            return setup_error.context(format!(
                "distribution artifact cleanup also failed: {cleanup_error}.{recovery}"
            ));
        }
    }
    setup_error
}

pub(super) async fn cleanup_retained_cloudformation_resources(
    env: &[(String, String)],
    resources: &[ImportedResource],
) -> anyhow::Result<()> {
    for resource in resources {
        match resource.resource_type.as_ref() {
            "storage" => {
                let data: AwsStorageImportData =
                    serde_json::from_value(resource.import_data.clone()).with_context(|| {
                        format!(
                            "Failed to parse AWS storage import data for '{}'",
                            resource.id
                        )
                    })?;
                cleanup_retained_s3_bucket(env, &data.bucket_name).await?;
            }
            "kv" => {
                let data: AwsKvImportData = serde_json::from_value(resource.import_data.clone())
                    .with_context(|| {
                        format!("Failed to parse AWS KV import data for '{}'", resource.id)
                    })?;
                cleanup_retained_dynamodb_table(env, &data.table_name).await?;
            }
            "artifact-registry" => {
                let data: AwsArtifactRegistryImportData =
                    serde_json::from_value(resource.import_data.clone()).with_context(|| {
                        format!(
                            "Failed to parse AWS artifact registry import data for '{}'",
                            resource.id
                        )
                    })?;
                cleanup_retained_ecr_repository(env, &data.repository_prefix, &data.region).await?;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn cleanup_retained_s3_bucket(env: &[(String, String)], bucket: &str) -> anyhow::Result<()> {
    info!(%bucket, "deleting retained CloudFormation S3 bucket");
    let temp = TempDir::new().context("Failed to create retained S3 cleanup temp dir")?;

    let mut list = Command::new("aws");
    list.args([
        "s3api",
        "list-object-versions",
        "--bucket",
        bucket,
        "--output",
        "json",
    ]);
    apply_env(&mut list, env);
    let output = command_output(list, "aws s3api list-object-versions").await?;
    let versions: Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse S3 list-object-versions response")?;
    let mut objects = Vec::new();

    for field in ["Versions", "DeleteMarkers"] {
        let Some(entries) = versions.get(field).and_then(Value::as_array) else {
            continue;
        };

        for entry in entries {
            let Some(key) = entry.get("Key").and_then(Value::as_str) else {
                continue;
            };
            let mut object = serde_json::Map::new();
            object.insert("Key".to_string(), Value::String(key.to_string()));
            if let Some(version_id) = entry.get("VersionId").and_then(Value::as_str) {
                object.insert(
                    "VersionId".to_string(),
                    Value::String(version_id.to_string()),
                );
            }
            objects.push(Value::Object(object));
        }
    }

    for (index, chunk) in objects.chunks(1000).enumerate() {
        let delete_file = temp.path().join(format!("delete-{index}.json"));
        let payload = serde_json::json!({
            "Objects": chunk,
            "Quiet": true,
        });
        fs::write(
            &delete_file,
            serde_json::to_vec(&payload).context("Failed to serialize S3 delete payload")?,
        )
        .await
        .context("Failed to write S3 delete payload")?;

        let mut delete = Command::new("aws");
        delete.args([
            "s3api",
            "delete-objects",
            "--bucket",
            bucket,
            "--delete",
            &format!("file://{}", delete_file.display()),
        ]);
        apply_env(&mut delete, env);
        run_command(delete, "aws s3api delete-objects").await?;
    }

    let mut delete_bucket = Command::new("aws");
    delete_bucket.args(["s3api", "delete-bucket", "--bucket", bucket]);
    apply_env(&mut delete_bucket, env);
    run_command(delete_bucket, "aws s3api delete-bucket").await?;

    Ok(())
}

async fn cleanup_retained_ecr_repository(
    env: &[(String, String)],
    repository_name: &str,
    region: &str,
) -> anyhow::Result<()> {
    info!(%repository_name, %region, "deleting retained CloudFormation ECR repository");
    let mut delete = Command::new("aws");
    delete.args([
        "ecr",
        "delete-repository",
        "--repository-name",
        repository_name,
        "--region",
        region,
        "--force",
    ]);
    apply_env(&mut delete, env);
    run_command(delete, "aws ecr delete-repository").await?;

    Ok(())
}

async fn cleanup_retained_dynamodb_table(
    env: &[(String, String)],
    table_name: &str,
) -> anyhow::Result<()> {
    info!(%table_name, "deleting retained CloudFormation DynamoDB table");
    let mut delete = Command::new("aws");
    delete.args(["dynamodb", "delete-table", "--table-name", table_name]);
    apply_env(&mut delete, env);
    run_command(delete, "aws dynamodb delete-table").await?;

    let mut wait = Command::new("aws");
    wait.args([
        "dynamodb",
        "wait",
        "table-not-exists",
        "--table-name",
        table_name,
    ]);
    apply_env(&mut wait, env);
    run_command(wait, "aws dynamodb wait table-not-exists").await?;

    Ok(())
}
