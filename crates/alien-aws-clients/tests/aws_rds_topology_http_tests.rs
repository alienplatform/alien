use alien_aws_clients::{
    rds::{CreateDbInstanceRequest, RdsApi, RdsClient},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use alien_client_core::ErrorData;
use httpmock::prelude::*;
use std::collections::HashMap;

fn client(server: &MockServer) -> RdsClient {
    RdsClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".into(),
                secret_access_key: "test-secret-key".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("rds".into(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn topology_reads_actual_writer_zone_and_all_instance_pages() {
    let server = MockServer::start_async().await;
    let cluster = server.mock_async(|when, then| {
        when.method(POST).header_exists("authorization")
            .body_contains("Action=DescribeDBClusters").body_contains("DBClusterIdentifier=stack-db");
        then.status(200).body(r#"<DescribeDBClustersResponse><DescribeDBClustersResult><DBClusters><DBCluster>
            <DBClusterIdentifier>stack-db</DBClusterIdentifier><Status>available</Status>
            <AvailabilityZones><AvailabilityZone>us-east-1a</AvailabilityZone><AvailabilityZone>us-east-1b</AvailabilityZone></AvailabilityZones>
            <DBClusterMembers>
              <DBClusterMember><DBInstanceIdentifier>stack-db-1</DBInstanceIdentifier><IsClusterWriter>false</IsClusterWriter><PromotionTier>0</PromotionTier></DBClusterMember>
              <DBClusterMember><DBInstanceIdentifier>stack-db-2</DBInstanceIdentifier><IsClusterWriter>true</IsClusterWriter><PromotionTier>1</PromotionTier></DBClusterMember>
            </DBClusterMembers>
            </DBCluster></DBClusters></DescribeDBClustersResult></DescribeDBClustersResponse>"#);
    }).await;
    let first = server.mock_async(|when, then| {
        when.method(POST).header_exists("authorization").body_contains("Action=DescribeDBInstances")
            .body_contains("Filters.Filter.1.Name=db-cluster-id")
            .body_contains("Filters.Filter.1.Values.Value.1=stack-db")
            .matches(|request| !String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).contains("Marker="));
        then.status(200).body(r#"<DescribeDBInstancesResponse><DescribeDBInstancesResult><DBInstances><DBInstance>
          <DBInstanceIdentifier>stack-db-1</DBInstanceIdentifier><DBInstanceStatus>available</DBInstanceStatus>
          <DBClusterIdentifier>stack-db</DBClusterIdentifier><AvailabilityZone>us-east-1a</AvailabilityZone><PromotionTier>0</PromotionTier>
          </DBInstance></DBInstances><Marker>next-page</Marker></DescribeDBInstancesResult></DescribeDBInstancesResponse>"#);
    }).await;
    let second = server.mock_async(|when, then| {
        when.method(POST).header_exists("authorization").body_contains("Action=DescribeDBInstances")
            .body_contains("Filters.Filter.1.Values.Value.1=stack-db").body_contains("Marker=next-page");
        then.status(200).body(r#"<DescribeDBInstancesResponse><DescribeDBInstancesResult><DBInstances><DBInstance>
          <DBInstanceIdentifier>stack-db-2</DBInstanceIdentifier><DBInstanceStatus>available</DBInstanceStatus>
          <DBClusterIdentifier>stack-db</DBClusterIdentifier><AvailabilityZone>us-east-1b</AvailabilityZone><PromotionTier>1</PromotionTier>
          </DBInstance></DBInstances></DescribeDBInstancesResult></DescribeDBInstancesResponse>"#);
    }).await;
    let client = client(&server);
    let clusters = client.describe_db_clusters("stack-db").await.unwrap();
    assert_eq!(clusters.len(), 1);
    assert_eq!(
        clusters[0].availability_zones.members,
        ["us-east-1a", "us-east-1b"]
    );
    let members = &clusters[0].members.members;
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].is_cluster_writer, Some(false));
    assert_eq!(members[1].identifier, "stack-db-2");
    assert_eq!(members[1].is_cluster_writer, Some(true));
    let instances = client.describe_db_instances("stack-db").await.unwrap();
    assert_eq!(instances.len(), 2);
    for (instance, zone) in instances.iter().zip(["us-east-1a", "us-east-1b"]) {
        assert_eq!(instance.cluster_identifier.as_deref(), Some("stack-db"));
        assert_eq!(instance.availability_zone.as_deref(), Some(zone));
    }
    cluster.assert_async().await;
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn create_replica_and_request_exact_failover_use_signed_private_requests() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .header_exists("authorization")
                .body_contains("Action=CreateDBInstance")
                .body_contains("DBClusterIdentifier=stack-db")
                .body_contains("DBInstanceIdentifier=stack-db-2")
                .body_contains("AvailabilityZone=us-east-1b")
                .body_contains("PromotionTier=1")
                .body_contains("DBInstanceClass=db.serverless")
                .body_contains("PubliclyAccessible=false");
            then.status(200).body("<CreateDBInstanceResponse/>");
        })
        .await;
    let failover = server
        .mock_async(|when, then| {
            when.method(POST)
                .header_exists("authorization")
                .body_contains("Action=FailoverDBCluster")
                .body_contains("DBClusterIdentifier=stack-db")
                .body_contains("TargetDBInstanceIdentifier=stack-db-2");
            then.status(200).body("<FailoverDBClusterResponse/>");
        })
        .await;
    let client = client(&server);
    client
        .create_db_instance(CreateDbInstanceRequest {
            identifier: "stack-db-2".into(),
            cluster_identifier: "stack-db".into(),
            engine_version: "17.4".into(),
            performance_insights_kms_key_id: None,
            availability_zone: Some("us-east-1b".into()),
            promotion_tier: Some(1),
        })
        .await
        .unwrap();
    client
        .failover_db_cluster("stack-db", "stack-db-2")
        .await
        .unwrap();
    create.assert_async().await;
    failover.assert_async().await;
}

#[tokio::test]
async fn topology_read_failure_never_returns_partial_instances() {
    for repeated_marker in [false, true] {
        let server = MockServer::start_async().await;
        let first = server.mock_async(|when, then| {
            when.method(POST).body_contains("Action=DescribeDBInstances").matches(|request| !String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).contains("Marker="));
            then.status(200).body("<DescribeDBInstancesResponse><DescribeDBInstancesResult><DBInstances/><Marker>next</Marker></DescribeDBInstancesResult></DescribeDBInstancesResponse>");
        }).await;
        let second = server.mock_async(|when, then| {
            when.method(POST).body_contains("Marker=next");
            if repeated_marker {
                then.status(200).body("<DescribeDBInstancesResponse><DescribeDBInstancesResult><DBInstances/><Marker>next</Marker></DescribeDBInstancesResult></DescribeDBInstancesResponse>");
            } else {
                then.status(403).body("<ErrorResponse><Error><Code>AccessDenied</Code><Message>denied</Message></Error></ErrorResponse>");
            }
        }).await;
        let error = client(&server)
            .describe_db_instances("stack-db")
            .await
            .unwrap_err();
        assert!(
            matches!(
                error.error,
                Some(ErrorData::InvalidInput { .. }) | Some(ErrorData::RemoteAccessDenied { .. })
            ),
            "{error:?}"
        );
        first.assert_async().await;
        second.assert_async().await;
    }
}

#[tokio::test]
async fn failover_errors_are_single_attempt_and_keep_lifecycle_conflicts_retryable() {
    for (status, code) in [
        (400, "InvalidDBInstanceState"),
        (400, "InvalidDBClusterStateFault"),
        (403, "AccessDenied"),
        (404, "DBClusterNotFoundFault"),
    ] {
        let server = MockServer::start_async().await;
        let request = server.mock_async(|when, then| {
            when.method(POST).body_contains("Action=FailoverDBCluster");
            then.status(status).body(format!("<ErrorResponse><Error><Code>{code}</Code><Message>provider failure</Message></Error></ErrorResponse>"));
        }).await;
        let error = client(&server)
            .failover_db_cluster("stack-db", "stack-db-2")
            .await
            .unwrap_err();
        match status {
            400 => assert!(
                matches!(error.error, Some(ErrorData::RemoteResourceConflict { .. })),
                "{error:?}"
            ),
            403 => assert!(
                matches!(error.error, Some(ErrorData::RemoteAccessDenied { .. })),
                "{error:?}"
            ),
            404 => assert!(
                matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })),
                "{error:?}"
            ),
            _ => unreachable!(),
        }
        request.assert_async().await;
    }
}
