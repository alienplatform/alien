//! What every built-in binding reads as, serializes to, and gives a workload, and how a malformed
//! one is refused, alone, in `ExternalBindings` and in `StackSettings`. The snapshot pins them, so a
//! change to how bindings read, such as an extension's binding type, leaves the built-ins as they
//! are.

use alien_core::{ExternalBinding, ExternalBindings, StackSettings};

const BINDINGS: &[&str] = &[
    r#"{"type":"storage","service":"s3","bucketName":"demo","endpoint":"https://s3.example.test","region":"us-east-1","forcePathStyle":true}"#,
    r#"{"type":"storage","service":"blob","accountName":"demo","containerName":"archive"}"#,
    r#"{"type":"storage","service":"gcs","bucketName":"demo"}"#,
    r#"{"type":"storage","service":"local-storage","storagePath":"/tmp/demo"}"#,
    r#"{"type":"storage","service":"s3","bucketName":{"secretRef":{"name":"demo","key":"bucket"}}}"#,
    r#"{"type":"queue","service":"sqs","queueUrl":"https://queue.example.test/demo"}"#,
    r#"{"type":"queue","service":"pubsub","topic":"demo","subscription":"reader"}"#,
    r#"{"type":"queue","service":"servicebus","namespace":"demo","queueName":"events"}"#,
    r#"{"type":"queue","service":"local-queue","queuePath":"/tmp/demo"}"#,
    r#"{"type":"kv","service":"dynamodb","tableName":"demo","region":"us-east-1","endpointUrl":"https://kv.example.test"}"#,
    r#"{"type":"kv","service":"firestore","projectId":"demo","databaseId":"demo","collectionName":"items"}"#,
    r#"{"type":"kv","service":"tablestorage","resourceGroupName":"demo","accountName":"demo","tableName":"items"}"#,
    r#"{"type":"kv","service":"redis","connectionUrl":"redis://localhost:6379","keyPrefix":"demo","database":2}"#,
    r#"{"type":"kv","service":"local-kv","dataDir":"/tmp/demo","keyPrefix":"demo"}"#,
    r#"{"type":"redis","service":"elasticache","host":"cache.example.test","port":6379,"username":"default","passwordSecretArn":"demo-secret"}"#,
    r#"{"type":"redis","service":"memorystore","host":"10.0.0.5","port":6379,"username":"default","serverCaCertificates":["demo-ca"],"instanceName":"demo"}"#,
    r#"{"type":"redis","service":"azure-managed-redis","host":"cache.example.test","port":10000,"username":"default","databaseResourceId":"demo-db"}"#,
    r#"{"type":"redis","service":"external","host":"cache.example.test","port":6379,"username":"default","password":"demo","tls":false}"#,
    r#"{"type":"redis","service":"local-redis","host":"127.0.0.1","port":6379,"username":"default","password":"demo"}"#,
    r#"{"type":"artifact_registry","service":"ecr","repositoryPrefix":"demo","pullRoleArn":"demo-pull","pushRoleArn":"demo-push"}"#,
    r#"{"type":"artifact_registry","service":"acr","registryName":"demo","resourceGroupName":"demo","repositoryPrefix":"demo"}"#,
    r#"{"type":"artifact_registry","service":"gar","repositoryName":"demo","pullServiceAccountEmail":"pull@example.test","pushServiceAccountEmail":"push@example.test"}"#,
    r#"{"type":"artifact_registry","service":"local","registryUrl":"localhost:5000","dataDir":null}"#,
    r#"{"type":"vault","service":"parameter-store","vaultPrefix":"demo"}"#,
    r#"{"type":"vault","service":"secret-manager","vaultPrefix":"demo"}"#,
    r#"{"type":"vault","service":"key-vault","vaultName":"demo"}"#,
    r#"{"type":"vault","service":"kubernetes-secret","namespace":"demo","vaultPrefix":"demo"}"#,
    r#"{"type":"vault","service":"local-vault","vaultName":"demo","dataDir":"/tmp/demo"}"#,
    r#"{"type":"azure_container_apps_environment","environmentName":"demo","resourceId":"demo","resourceGroupName":"demo","defaultDomain":"example.test","staticIp":"192.0.2.1"}"#,
    r#"{"type":"container_apps_environment","environmentName":"demo","resourceId":"demo","resourceGroupName":"demo","defaultDomain":"example.test","staticIp":"192.0.2.1"}"#,
    r#"{"type":"postgres","service":"aurora","clusterEndpoint":"db.example.test","port":5432,"database":"demo","username":"demo","passwordSecretArn":"demo-secret"}"#,
    r#"{"type":"postgres","service":"cloud-sql","host":"db.example.test","port":5432,"database":"demo","username":"demo","serverCaCertificates":["demo-ca"],"passwordSecretName":"demo-secret"}"#,
    r#"{"type":"postgres","service":"flexible-server","host":"db.example.test","port":5432,"database":"demo","username":"demo","passwordSecretUri":"https://vault.example.test/demo"}"#,
    r#"{"type":"postgres","service":"external","host":"db.example.test","port":5432,"database":"demo","username":"demo","password":"demo-only","sslMode":"verify-full"}"#,
    r#"{"type":"postgres","service":"external","host":"db.example.test","port":5432,"database":"demo","username":"demo","password":{"secretRef":{"name":"database-credentials","key":"password"}}}"#,
    r#"{"type":"postgres","service":"local-postgres","host":"db.example.test","port":5432,"database":"demo","username":"demo","password":"demo-only"}"#,
    r#"{"type":"ai","provider":"openai","apiKey":{"secretRef":{"name":"demo","key":"api-key"}}}"#,
    r#"{"type":"ai","provider":"anthropic","apiKey":"sk-x"}"#,
    // malformed
    r#"{"type":"unknown","service":"s3","bucketName":"demo"}"#,
    r#"{"type":"storage","service":"unknown","bucketName":"demo"}"#,
    r#"{"type":"storage","service":"s3"}"#,
    r#"{"type":"storage","service":"s3","bucketName":"demo","forcePathStyle":"true"}"#,
    r#"{"type":"ai","provider":42,"apiKey":"demo-only"}"#,
    r#"{"service":"s3","bucketName":"b"}"#,
    r#"{"type":7,"service":"s3","bucketName":"b"}"#,
    r#"[1,2]"#,
    r#""storage""#,
];

/// The value with every object's keys in sorted order. A JSON object's key order depends on
/// whether a crate in the build turns on `serde_json/preserve_order`, and the snapshot must not.
fn sorted_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: std::collections::BTreeMap<_, _> = map
                .iter()
                .map(|(k, v)| (k.clone(), sorted_keys(v)))
                .collect();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(sorted_keys).collect())
        }
        other => other.clone(),
    }
}

#[test]
fn built_in_bindings_read_and_serialize_as_they_always_have() {
    let mut out = String::new();
    for raw in BINDINGS {
        let one = serde_json::from_str::<ExternalBinding>(raw).map(|b| {
            (
                serde_json::to_string(&b).unwrap(),
                serde_json::to_string(&sorted_keys(
                    &serde_json::to_value(b.to_resource_outputs()).unwrap(),
                ))
                .unwrap(),
                b.to_env_binding_value()
                    .map(|v| sorted_keys(&v).to_string())
                    .map_err(|e| e.to_string()),
                b.binding_type().to_string(),
            )
        });
        out.push_str(&format!("ONE {raw}\n  => {one:?}\n"));
        let map = format!(r#"{{"r":{raw}}}"#);
        let many = serde_json::from_str::<ExternalBindings>(&map)
            .map(|b| serde_json::to_string(&b).unwrap());
        out.push_str(&format!("MAP => {many:?}\n"));
        let settings = format!(r#"{{"externalBindings":{map}}}"#);
        let s = serde_json::from_str::<StackSettings>(&settings)
            .map(|s| serde_json::to_string(&s).unwrap());
        out.push_str(&format!("SETTINGS => {s:?}\n"));
    }
    insta::assert_snapshot!(out);
}
