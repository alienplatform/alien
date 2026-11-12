//! The interceptor hook through Alien's AWS clients. Every test sends to a local server that
//! records the bytes on the wire.

use crate::aws::credential_provider::AwsCredentialProvider;
use crate::aws::kms::{KmsApi, KmsClient};
use crate::aws::lambda::{InvokeRequest, LambdaApi, LambdaClient};
use crate::aws::rds::{CreateDbClusterRequest, ModifyDbClusterRequest, RdsApi, RdsClient};
use crate::aws::s3::{GetObjectRequest, HeadObjectRequest, PutObjectRequest, S3Api, S3Client};
use crate::aws::{AwsClientConfig, AwsCredentials, ServiceOverrides};
use alien_client_core::interceptor::{CallInterceptor, Cloud, InterceptorChain};
use alien_client_core::testing::capture::{self, Captured, Reply};
use alien_client_core::testing::TestInterceptor;
use alien_client_core::ErrorData;
use alien_error::AlienError;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use reqwest::header::HeaderValue;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const CREATE_CLUSTER_XML: &str = "<CreateDBClusterResponse><CreateDBClusterResult><DBCluster>\
<DBClusterIdentifier>stack-db</DBClusterIdentifier><Status>creating</Status>\
</DBCluster></CreateDBClusterResult></CreateDBClusterResponse>";

const DESCRIBE_CLUSTERS_XML: &str = "<DescribeDBClustersResponse><DescribeDBClustersResult>\
<DBClusters><DBCluster><DBClusterIdentifier>stack-db</DBClusterIdentifier>\
<Status>available</Status><Port>5432</Port></DBCluster></DBClusters>\
</DescribeDBClustersResult></DescribeDBClustersResponse>";

fn chain(interceptors: Vec<TestInterceptor>) -> InterceptorChain {
    InterceptorChain::new(
        interceptors
            .into_iter()
            .map(|interceptor| Arc::new(interceptor) as Arc<dyn CallInterceptor>)
            .collect(),
    )
}

fn credentials(
    service: &str,
    server: &capture::Server,
    chain: InterceptorChain,
) -> AwsCredentialProvider {
    AwsCredentialProvider::from_config_sync(AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test-access".into(),
            secret_access_key: "test-secret".into(),
            session_token: None,
        },
        service_overrides: Some(ServiceOverrides {
            endpoints: HashMap::from([(service.to_string(), server.base_url.clone())]),
        }),
    })
    .with_interceptors(chain)
}

fn rds(server: &capture::Server, chain: InterceptorChain) -> RdsClient {
    RdsClient::new(reqwest::Client::new(), credentials("rds", server, chain))
}

fn s3(server: &capture::Server, chain: InterceptorChain) -> S3Client {
    S3Client::new(reqwest::Client::new(), credentials("s3", server, chain))
}

/// Adds the IAM database authentication flag to every CreateDBCluster and ModifyDBCluster.
fn iam_auth_flag() -> TestInterceptor {
    TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|request| {
        let body = request
            .body
            .as_mut()
            .expect("a query API call has a form body");
        let action = body
            .split('&')
            .find_map(|pair| pair.strip_prefix("Action="));
        if matches!(action, Some("CreateDBCluster" | "ModifyDBCluster")) {
            body.push_str("&EnableIAMDatabaseAuthentication=true");
        }
        Ok(())
    })
}

fn create_cluster_request(identifier: &str) -> CreateDbClusterRequest {
    CreateDbClusterRequest {
        kms_key_id: None,
        identifier: identifier.into(),
        engine_version: "17.4".into(),
        master_username: "alien".into(),
        master_user_password: "secret".into(),
        database_name: "app".into(),
        db_subnet_group_name: "stack-subnets".into(),
        db_cluster_parameter_group_name: "stack-params".into(),
        vpc_security_group_ids: vec!["sg-1".into()],
        max_capacity: 4.0,
        backup_retention_days: 7,
        tags: HashMap::from([("alien-stack".to_string(), "stack".to_string())]),
    }
}

fn form(captured: &Captured) -> HashMap<String, String> {
    form_urlencoded::parse(&captured.body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Recomputes the SigV4 signature of a captured request over `body`, from the headers it signed.
fn recompute_signature(server: &capture::Server, captured: &Captured, body: &[u8]) -> String {
    let authorization = captured.header("authorization").expect("signed");
    let signed_headers = authorization
        .split("SignedHeaders=")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap();
    let headers: Vec<(String, String)> = signed_headers
        .split(';')
        .map(|name| (name.to_string(), captured.header(name).unwrap().to_string()))
        .collect();
    let seconds = chrono::NaiveDateTime::parse_from_str(
        captured.header("x-amz-date").unwrap(),
        "%Y%m%dT%H%M%SZ",
    )
    .unwrap()
    .and_utc()
    .timestamp();
    let identity =
        aws_credential_types::Credentials::new("test-access", "test-secret", None, None, "t")
            .into();
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region("us-east-1")
        .name("rds")
        .time(UNIX_EPOCH + Duration::from_secs(seconds as u64))
        .settings(SigningSettings::default())
        .build()
        .unwrap()
        .into();
    let url = format!("{}{}", server.base_url, captured.target);
    let signable = SignableRequest::new(
        &captured.method,
        &url,
        headers.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        SignableBody::Bytes(body),
    )
    .unwrap();
    sign(signable, &params).unwrap().into_parts().1
}

fn sent_signature(captured: &Captured) -> String {
    captured
        .header("authorization")
        .unwrap()
        .split("Signature=")
        .nth(1)
        .unwrap()
        .to_string()
}

fn assert_fails_naming(error: &AlienError<ErrorData>, extension: &str) {
    assert!(
        !error.retryable,
        "an interceptor failure is never retried: {error}"
    );
    assert!(
        matches!(&error.error, Some(ErrorData::InterceptorFailed { extension: name, .. }) if name == extension),
        "{error}"
    );
}

/// The interceptor's flag reaches CreateDBCluster under a valid signature: a recompute over the
/// sent body matches, a recompute over the body Alien built does not, and the payload digest
/// header describes the body that was sent.
#[tokio::test]
async fn rds_create_carries_the_flag_under_a_valid_signature() {
    let server = capture::start(|_| Reply::new(200, "text/xml", CREATE_CLUSTER_XML)).await;
    let client = rds(&server, chain(vec![iam_auth_flag()]));

    let cluster = client
        .create_db_cluster(create_cluster_request("stack-db"))
        .await
        .unwrap();
    assert_eq!(cluster.identifier, "stack-db");

    let sent = server.requests();
    assert_eq!(sent.len(), 1);
    let request = &sent[0];
    let params = form(request);
    assert_eq!(params["Action"], "CreateDBCluster");
    assert_eq!(params["EnableIAMDatabaseAuthentication"], "true");
    assert_eq!(params["DBClusterIdentifier"], "stack-db");
    assert_eq!(
        request.header("content-length").unwrap(),
        request.body.len().to_string()
    );
    assert_eq!(
        request.header("x-amz-content-sha256").unwrap(),
        hex::encode(Sha256::digest(&request.body))
    );

    let signature = sent_signature(request);
    assert_eq!(
        recompute_signature(&server, request, &request.body),
        signature
    );
    let built = request
        .body_text()
        .replace("&EnableIAMDatabaseAuthentication=true", "");
    assert_ne!(
        recompute_signature(&server, request, built.as_bytes()),
        signature
    );
}

/// The same interceptor reaches ModifyDBCluster, which Alien sends without reading a response.
#[tokio::test]
async fn rds_modify_carries_the_flag() {
    let server = capture::start(|_| {
        Reply::new(
            200,
            "text/xml",
            "<ModifyDBClusterResponse></ModifyDBClusterResponse>",
        )
    })
    .await;
    rds(&server, chain(vec![iam_auth_flag()]))
        .modify_db_cluster(ModifyDbClusterRequest {
            identifier: "stack-db".into(),
            engine_version: None,
            max_capacity: Some(8.0),
            master_user_password: None,
        })
        .await
        .unwrap();
    let params = form(&server.requests()[0]);
    assert_eq!(params["Action"], "ModifyDBCluster");
    assert_eq!(params["EnableIAMDatabaseAuthentication"], "true");
}

/// A change of host, port or scheme, or of a header that picks the destination or carries a
/// credential, fails the call before anything is sent, so neither server hears from Alien.
#[tokio::test]
async fn an_interceptor_cannot_move_the_request_or_touch_credentials() {
    let server = capture::start(|_| Reply::new(200, "text/xml", CREATE_CLUSTER_XML)).await;
    let elsewhere = capture::start(|_| Reply::new(200, "text/xml", CREATE_CLUSTER_XML)).await;
    let other = reqwest::Url::parse(&elsewhere.base_url).unwrap();
    let other_port = other.port();
    let other_authority = format!("127.0.0.1:{}", other_port.unwrap());
    let cases: Vec<(&str, TestInterceptor)> = vec![
        (
            "another host",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|r| {
                r.url.set_host(Some("localhost")).unwrap();
                Ok(())
            }),
        ),
        (
            "another port",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(move |r| {
                r.url.set_port(other_port).unwrap();
                Ok(())
            }),
        ),
        (
            "another scheme",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|r| {
                r.url.set_scheme("https").unwrap();
                Ok(())
            }),
        ),
        (
            "a Host header",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(move |r| {
                r.headers
                    .insert("host", HeaderValue::from_str(&other_authority).unwrap());
                Ok(())
            }),
        ),
        (
            "an Authorization header",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|r| {
                r.headers.insert(
                    "authorization",
                    HeaderValue::from_static("AWS4-HMAC-SHA256 forged"),
                );
                Ok(())
            }),
        ),
        (
            "a session token",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|r| {
                r.headers
                    .insert("x-amz-security-token", HeaderValue::from_static("stolen"));
                Ok(())
            }),
        ),
        (
            "the signing service",
            TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|r| {
                r.service = "ec2".into();
                Ok(())
            }),
        ),
    ];
    for (what, interceptor) in cases {
        let error = rds(&server, chain(vec![interceptor]))
            .create_db_cluster(create_cluster_request("stack-db"))
            .await
            .expect_err(what);
        assert_fails_naming(&error, "acme");
    }
    assert!(server.requests().is_empty());
    assert!(elsewhere.requests().is_empty());

    let client = rds(
        &server,
        chain(vec![TestInterceptor::new("acme", Cloud::Aws, "rds")
            .on_request(|r| {
                r.url.set_query(Some("extension=1"));
                r.headers
                    .insert("x-extension", HeaderValue::from_static("acme"));
                Ok(())
            })]),
    );
    client
        .create_db_cluster(create_cluster_request("stack-db"))
        .await
        .unwrap();
    let sent = server.requests();
    assert_eq!(sent[0].target, "/?extension=1");
    assert_eq!(sent[0].header("x-extension"), Some("acme"));
    assert_eq!(
        recompute_signature(&server, &sent[0], &sent[0].body),
        sent_signature(&sent[0])
    );
}

/// An interceptor that fails stops the call before anything is sent, and the error names its
/// extension and is not retried.
#[tokio::test]
async fn a_failing_interceptor_stops_the_call() {
    let server = capture::start(|_| Reply::new(200, "text/xml", CREATE_CLUSTER_XML)).await;
    let failing = TestInterceptor::new("acme", Cloud::Aws, "rds").on_request(|_| {
        Err(AlienError::new(ErrorData::GenericError {
            message: "no tag for this cluster".into(),
        }))
    });
    let error = rds(&server, chain(vec![failing]))
        .create_db_cluster(create_cluster_request("stack-db"))
        .await
        .unwrap_err();
    assert_fails_naming(&error, "acme");
    assert!(server.requests().is_empty());
}

/// A rewritten DescribeDBClusters response reaches what the RDS client returns, and the client
/// without interceptors reads the cloud's answer.
#[tokio::test]
async fn a_response_change_reaches_what_the_client_returns() {
    let server = capture::start(|_| Reply::new(200, "text/xml", DESCRIBE_CLUSTERS_XML)).await;
    let plain = rds(&server, InterceptorChain::default());
    assert_eq!(
        plain.describe_db_clusters("stack-db").await.unwrap()[0].status,
        "available"
    );

    let rewrite = TestInterceptor::new("acme", Cloud::Aws, "rds").on_response(|_, response| {
        response.body = response.body.replace(
            "<Status>available</Status>",
            "<Status>seen-by-extension</Status>",
        );
        Ok(())
    });
    let clusters = rds(&server, chain(vec![rewrite]))
        .describe_db_clusters("stack-db")
        .await
        .unwrap();
    assert_eq!(clusters[0].status, "seen-by-extension");
    assert_eq!(clusters[0].port, Some(5432));
}

/// Two deployments' clients in one process, sending at the same time: each request carries only
/// its own deployment's change under a valid signature, and a client with no interceptors sends
/// Alien's request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_deployment_sees_only_its_own_interceptors() {
    let server = capture::start(|_| Reply::new(200, "text/xml", CREATE_CLUSTER_XML)).await;
    let marker = |name: &'static str| {
        TestInterceptor::new(name, Cloud::Aws, "rds").on_request(move |r| {
            r.body.as_mut().unwrap().push_str(&format!(
                "&Tags.Tag.9.Key=deployment&Tags.Tag.9.Value={name}"
            ));
            Ok(())
        })
    };
    let a = Arc::new(rds(&server, chain(vec![marker("a")])));
    let b = Arc::new(rds(&server, chain(vec![marker("b")])));
    let none = Arc::new(rds(&server, InterceptorChain::default()));
    let mut tasks = Vec::new();
    for i in 0..30 {
        let client = [a.clone(), b.clone(), none.clone()][i % 3].clone();
        tasks.push(tokio::spawn(async move {
            client
                .create_db_cluster(create_cluster_request(&format!("db-{i}")))
                .await
                .unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let sent = server.requests();
    assert_eq!(sent.len(), 30);
    for request in &sent {
        let params = form(request);
        let i: usize = params["DBClusterIdentifier"][3..].parse().unwrap();
        let expected = ["a", "b", ""][i % 3];
        assert_eq!(
            params
                .get("Tags.Tag.9.Value")
                .map(String::as_str)
                .unwrap_or(""),
            expected
        );
        assert_eq!(
            recompute_signature(&server, request, &request.body),
            sent_signature(request)
        );
    }
}

/// An object's bytes are never offered to an interceptor, whatever their content type. With
/// no interceptor matching, PutObject sends them and GetObject returns them unchanged, even
/// when an S3 interceptor is present for other calls; when an interceptor matches, both calls
/// fail before anything is sent, naming the extension, and the failure is final.
#[tokio::test]
async fn object_bytes_pass_untouched_or_fail_the_call() {
    let object: Vec<u8> = br#"{"looks":"like json"}"#.to_vec();
    let stored = object.clone();
    let server = capture::start(move |request| {
        if request.method == "GET" {
            Reply::new(200, "application/json", stored.clone())
        } else {
            Reply::new(200, "application/xml", "").header("etag", "\"tag\"")
        }
    })
    .await;
    let put = || PutObjectRequest {
        bucket: "bucket".into(),
        key: "config.json".into(),
        body: object.clone(),
        content_type: Some("application/json".into()),
        content_md5: None,
        server_side_encryption: None,
        storage_class: None,
    };
    let get = GetObjectRequest {
        bucket: "bucket".into(),
        key: "config.json".into(),
        version_id: None,
        range: None,
    };

    let bucket_tagging = TestInterceptor::new("acme", Cloud::Aws, "s3")
        .when(|request| request.url.query() == Some("tagging"))
        .on_request(|_| panic!("only bucket tagging is matched"));
    let tagging_only = s3(&server, chain(vec![bucket_tagging]));
    tagging_only.put_object(&put()).await.unwrap();
    assert_eq!(server.requests()[0].body, object);
    assert_eq!(tagging_only.get_object(&get).await.unwrap().body, object);

    let every_call = || {
        TestInterceptor::new("acme", Cloud::Aws, "s3")
            .on_request(|_| panic!("an object's bytes are never offered"))
            .on_response(|_, _| panic!("an object's bytes are never offered"))
    };
    let intercepted = s3(&server, chain(vec![every_call()]));
    assert_fails_naming(&intercepted.put_object(&put()).await.unwrap_err(), "acme");
    assert_fails_naming(&intercepted.get_object(&get).await.unwrap_err(), "acme");
    assert_eq!(
        server.requests().len(),
        2,
        "neither intercepted call was sent"
    );
}

/// S3 wraps a failed send in an error marked retryable; an interceptor's failure underneath
/// stays final.
#[tokio::test]
async fn a_response_interceptor_failure_stays_final_through_the_s3_wrapper() {
    let server = capture::start(|_| {
        Reply::new(
            200,
            "application/xml",
            "<LifecycleConfiguration></LifecycleConfiguration>",
        )
    })
    .await;
    let failing = TestInterceptor::new("acme", Cloud::Aws, "s3").on_response(|_, _| {
        Err(AlienError::new(ErrorData::GenericError {
            message: "unexpected lifecycle rule".into(),
        }))
    });
    let error = s3(&server, chain(vec![failing]))
        .get_bucket_lifecycle_configuration("bucket")
        .await
        .unwrap_err();
    assert!(!error.retryable, "{error}");
    assert_eq!(server.requests().len(), 1, "sent once, never retried");
}

/// KMS wraps a failed call in an error marked retryable; a failing request interceptor
/// underneath stays final, and nothing is sent.
#[tokio::test]
async fn a_request_interceptor_failure_stays_final_through_the_kms_wrapper() {
    let server = capture::start(|_| Reply::new(200, "application/x-amz-json-1.1", "{}")).await;
    let failing = TestInterceptor::new("acme", Cloud::Aws, "kms").on_request(|_| {
        Err(AlienError::new(ErrorData::GenericError {
            message: "no key policy".into(),
        }))
    });
    let client = KmsClient::new(
        reqwest::Client::new(),
        credentials("kms", &server, chain(vec![failing])),
    );
    let error = client.describe_key("key").await.unwrap_err();
    assert!(!error.retryable, "{error}");
    assert!(server.requests().is_empty());
}

/// S3 wraps a body-less read in an error marked retryable; a failing response interceptor
/// underneath stays final, and the call is sent once.
#[tokio::test]
async fn a_response_interceptor_failure_stays_final_through_the_s3_text_read() {
    let server = capture::start(|_| {
        Reply::new(
            200,
            "application/xml",
            "<Tagging><TagSet></TagSet></Tagging>",
        )
    })
    .await;
    let failing = TestInterceptor::new("acme", Cloud::Aws, "s3").on_response(|_, _| {
        Err(AlienError::new(ErrorData::GenericError {
            message: "unexpected tag".into(),
        }))
    });
    let error = s3(&server, chain(vec![failing]))
        .get_bucket_tagging("bucket")
        .await
        .unwrap_err();
    assert!(!error.retryable, "{error}");
    assert_eq!(server.requests().len(), 1, "sent once, never retried");
}

/// A Lambda payload is the function's own bytes, JSON or not: an interceptor that does not
/// match leaves it as it was, and one that matches sees no body and fails the call before it is
/// sent.
#[tokio::test]
async fn a_lambda_payload_is_never_offered() {
    let server = capture::start(|_| Reply::new(200, "application/json", r#"{"ok":true}"#)).await;
    let invoke = || {
        InvokeRequest::builder()
            .function_name("f".into())
            .payload(br#"{"a":1}"#.to_vec())
            .build()
    };

    let not_invocations = TestInterceptor::new("acme", Cloud::Aws, "lambda")
        .when(|request| !request.url.path().ends_with("/invocations"))
        .on_request(|_| panic!("an invocation is not matched"));
    let client = LambdaClient::new(
        reqwest::Client::new(),
        credentials("lambda", &server, chain(vec![not_invocations])),
    );
    assert_eq!(
        client.invoke(invoke()).await.unwrap().payload,
        br#"{"ok":true}"#.to_vec()
    );
    assert_eq!(server.requests()[0].body, br#"{"a":1}"#.to_vec());

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let spy = seen.clone();
    let every_call = TestInterceptor::new("acme", Cloud::Aws, "lambda")
        .when(move |request| {
            spy.lock().unwrap().push(request.body.clone());
            true
        })
        .on_request(|_| panic!("a payload is never offered"));
    let client = LambdaClient::new(
        reqwest::Client::new(),
        credentials("lambda", &server, chain(vec![every_call])),
    );
    assert_fails_naming(&client.invoke(invoke()).await.unwrap_err(), "acme");
    assert_eq!(*seen.lock().unwrap(), vec![None]);
    assert_eq!(
        server.requests().len(),
        1,
        "the matched invocation was never sent"
    );
}

/// HeadObject reads an object's metadata and is an object call: an interceptor that does not
/// match leaves it alone, and one that matches fails it before it is sent.
#[tokio::test]
async fn head_object_is_an_object_call() {
    let server = capture::start(|_| Reply::new(200, "application/octet-stream", "")).await;
    let head = HeadObjectRequest {
        bucket: "bucket".into(),
        key: "image.png".into(),
        version_id: None,
    };

    let tagging_only = TestInterceptor::new("acme", Cloud::Aws, "s3")
        .when(|request| request.url.query() == Some("tagging"))
        .on_request(|_| panic!("only bucket tagging is matched"));
    s3(&server, chain(vec![tagging_only]))
        .head_object(&head)
        .await
        .unwrap();
    assert_eq!(server.requests().len(), 1);

    let every_call = TestInterceptor::new("acme", Cloud::Aws, "s3")
        .on_request(|_| panic!("an object call is never offered"))
        .on_response(|_, _| panic!("an object call is never offered"));
    let error = s3(&server, chain(vec![every_call]))
        .head_object(&head)
        .await
        .unwrap_err();
    assert_fails_naming(&error, "acme");
    assert_eq!(
        server.requests().len(),
        1,
        "the matched call was never sent"
    );
}
