//! What a management-side permission set must never grant: reading a customer's stored contents
//! (objects, items, secrets, messages, logs, build output) or reaching inside a sandbox session.
//!
//! Alien's own `provision`, `management` and `heartbeat` sets are held to these lists by the
//! sensitive-invariant tests. An extension's sets with those names go to the extension's
//! identity, whose credentials sign the cloud calls of the extension's controller, which Alien
//! has not reviewed.
//! [`extension_identity_refusals`] holds them to these lists and more: no predefined role, whose
//! contents change outside the stack, nothing that grants access or acts as another identity,
//! and the reads Alien's own reviewed sets need but a third party's do not.
//!
//! The extension lists are drawn from each provider's catalog of actions by the shape of their
//! names (a verb that runs, invokes, signs or decrypts; a noun that names a secret, key, password,
//! credential or token; a read of a store's contents), and the tests refuse each entry granted
//! alone.

use alien_core::permissions::PermissionSet;

use crate::registry::{iam_action_pattern_matches, AZURE_SANDBOX_DATA_PLANE_ROLE};
use crate::SENSITIVE_MICROVM_ACTIONS;

/// AWS actions that read stored contents.
pub const AWS_SENSITIVE_CONTENT_ACTIONS: &[&str] = &[
    "s3:GetObject",
    "s3:GetObjectVersion",
    "dynamodb:BatchGetItem",
    "dynamodb:GetItem",
    "dynamodb:Query",
    "dynamodb:Scan",
    "ssm:GetParameter",
    "ssm:GetParameters",
    "ssm:GetParametersByPath",
    "secretsmanager:GetSecretValue",
    "sqs:ReceiveMessage",
    "codebuild:BatchGetBuilds",
    "logs:GetLogEvents",
    "logs:FilterLogEvents",
    "logs:StartQuery",
    "logs:GetQueryResults",
    "dynamodb:PartiQLSelect",
    "dynamodb:GetRecords",
    "ssm:GetParameterHistory",
    "kinesis:GetRecords",
    "kinesis:SubscribeToShard",
    // Each copies contents somewhere the caller chooses, or streams them to the caller.
    "dynamodb:ExportTableToPointInTime",
    "logs:StartLiveTail",
    "logs:CreateExportTask",
    "logs:PutSubscriptionFilter",
    "logs:GetLogRecord",
    // The blocks of an EBS snapshot, which hold the volume's data.
    "ebs:GetSnapshotBlock",
    // A session that reads and writes a directory bucket's objects.
    "s3express:CreateSession",
    "dax:GetItem",
    "dax:BatchGetItem",
    "dax:Query",
    "dax:Scan",
    // Each signs in to the database or cache as one of its users.
    "rds-db:connect",
    "elasticache:Connect",
    "memorydb:Connect",
    "dsql:DbConnect",
    "dsql:DbConnectAdmin",
    "neptune-db:connect",
    // Each reads a domain's, cluster's, topic's, keyspace's or table's data.
    "es:ESHttpGet",
    "es:ESHttpPost",
    "neptune-db:ReadDataViaQuery",
    "kafka-cluster:ReadData",
    "cassandra:Select",
    "timestream:Select",
];

/// GCP permissions that read stored contents.
pub const GCP_SENSITIVE_CONTENT_PERMISSIONS: &[&str] = &[
    "storage.objects.get",
    "datastore.entities.get",
    "datastore.entities.list",
    "secretmanager.versions.access",
    "pubsub.subscriptions.consume",
    "artifactregistry.repositories.downloadArtifacts",
    "cloudbuild.builds.get",
    "cloudbuild.builds.list",
    "bigquery.tables.getData",
    "bigquery.tables.export",
    "logging.logEntries.list",
    "logging.privateLogEntries.list",
    "logging.views.access",
    "spanner.databases.read",
    "spanner.databases.select",
    "bigtable.tables.readRows",
    // Each copies contents somewhere the caller chooses.
    "cloudsql.instances.export",
    "datastore.databases.export",
    "logging.sinks.create",
    "redis.instances.export",
    "container.secrets.get",
    "container.secrets.list",
    // The password that reads the instance's keys.
    "redis.instances.getAuthString",
    "memorystore.authTokens.list",
    // Each signs in to the database or cache as one of its users.
    "memorystore.instances.connect",
    "redis.clusters.connect",
    "cloudsql.instances.login",
    "alloydb.users.login",
    // Runs code and reads files inside a live sandbox session; belongs to sandbox/execute alone.
    "aiplatform.sandboxEnvironments.execute",
];

/// GCP predefined roles that carry a permission on [`GCP_SENSITIVE_CONTENT_PERMISSIONS`].
pub const GCP_SENSITIVE_CONTENT_ROLES: &[&str] = &[
    "roles/storage.admin",
    "roles/storage.objectViewer",
    "roles/storage.objectUser",
    "roles/storage.objectAdmin",
    "roles/storage.legacyObjectReader",
    "roles/storage.legacyObjectOwner",
    // Each carries a database sign-in on the list above.
    "roles/cloudsql.instanceUser",
    "roles/alloydb.databaseUser",
    "roles/redis.dbConnectionUser",
    "roles/memorystore.dbConnectionUser",
    "roles/datastore.viewer",
    "roles/datastore.user",
    "roles/datastore.owner",
    "roles/datastore.admin",
    "roles/secretmanager.secretAccessor",
    "roles/secretmanager.admin",
    "roles/pubsub.subscriber",
    "roles/pubsub.editor",
    "roles/pubsub.admin",
    "roles/artifactregistry.reader",
    "roles/artifactregistry.writer",
    "roles/artifactregistry.repoAdmin",
    "roles/artifactregistry.admin",
    "roles/cloudbuild.builds.viewer",
    "roles/cloudbuild.builds.editor",
    "roles/cloudbuild.admin",
    "roles/bigquery.admin",
    "roles/bigquery.dataEditor",
    "roles/bigquery.dataOwner",
    "roles/bigquery.dataViewer",
    "roles/bigquery.studioAdmin",
    "roles/bigquery.routineAdmin",
    "roles/bigquery.routineDataEditor",
    "roles/bigquery.routineDataViewer",
    "roles/logging.admin",
    "roles/logging.privateLogViewer",
    "roles/logging.viewer",
    "roles/logging.configWriter",
    "roles/logging.viewAccessor",
    "roles/spanner.admin",
    "roles/spanner.editor",
    "roles/spanner.databaseAdmin",
    "roles/spanner.databaseReader",
    "roles/spanner.databaseReaderWithDataBoost",
    "roles/spanner.databaseUser",
    "roles/spanner.graphIntelligenceUser",
    "roles/bigtable.admin",
    "roles/bigtable.editor",
    "roles/bigtable.user",
    "roles/bigtable.reader",
    "roles/cloudsql.admin",
    "roles/cloudsql.editor",
    "roles/datastore.importExportAdmin",
    "roles/container.admin",
    "roles/container.developer",
    "roles/container.editor",
    "roles/redis.admin",
    "roles/owner",
    "roles/editor",
    "roles/viewer",
];

/// Azure control-plane actions that hand over a key or secret, or read stored contents.
pub const AZURE_SENSITIVE_CONTENT_ACTIONS: &[&str] = &[
    "Microsoft.Storage/storageAccounts/listKeys/action",
    // A SAS token reads whatever the account or service holds.
    "Microsoft.Storage/storageAccounts/ListAccountSas/action",
    "Microsoft.Storage/storageAccounts/listServiceSas/action",
    // The account API key, which is the whole inference surface. Azure spells it `listkeys` in
    // the role definition and `listKeys` in its docs, which is why every comparison is
    // case-insensitive.
    "Microsoft.CognitiveServices/accounts/listKeys/action",
    "Microsoft.App/containerApps/listSecrets/action",
    "Microsoft.App/managedEnvironments/listSecrets/action",
    // Runs a query over a workspace's or an Application Insights component's logs.
    "Microsoft.OperationalInsights/workspaces/query/read",
    "Microsoft.OperationalInsights/workspaces/search/action",
    "Microsoft.Insights/components/query/read",
    "Microsoft.OperationalInsights/workspaces/analytics/query/action",
];

/// Azure data actions that read stored contents or reach inside a sandbox session.
pub const AZURE_SENSITIVE_CONTENT_DATA_ACTIONS: &[&str] = &[
    // Reaching inside a live sandbox session: running a command, or moving its files. Session
    // lifecycle is separate on Azure and is not listed here.
    "Microsoft.App/sandboxGroups/sandboxes/executeCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/executeShellCommand/action",
    "Microsoft.App/sandboxGroups/sandboxes/exec/stream/action",
    "Microsoft.App/sandboxGroups/sandboxes/files/read",
    "Microsoft.App/sandboxGroups/sandboxes/files/write",
    "Microsoft.App/sandboxGroups/sandboxes/files/delete",
    "Microsoft.App/sandboxGroups/sandboxes/downloadContentPackage/action",
    // Each turns a live session's filesystem into an object outside it: a disk image another
    // sandbox can boot, or a snapshot.
    "Microsoft.App/sandboxGroups/sandboxes/commit/action",
    "Microsoft.App/sandboxGroups/sandboxes/snapshot/action",
    // The values of the secrets a sandbox group hands its sandboxes.
    "Microsoft.App/sandboxGroups/secrets/read",
    "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
    "Microsoft.Storage/storageAccounts/tableServices/tables/entities/read",
    "Microsoft.Storage/storageAccounts/queueServices/queues/messages/read",
    "Microsoft.KeyVault/vaults/secrets/read",
    "Microsoft.KeyVault/vaults/secrets/getSecret/action",
    "Microsoft.ServiceBus/namespaces/queues/messages/receive/action",
    "Microsoft.ServiceBus/namespaces/topics/subscriptions/messages/receive/action",
    "Microsoft.ServiceBus/namespaces/queues/messages/peek/action",
    "Microsoft.ServiceBus/namespaces/topics/subscriptions/messages/peek/action",
    "Microsoft.ServiceBus/namespaces/messages/receive/action",
    "Microsoft.Storage/storageAccounts/queueServices/queues/messages/process/action",
    "Microsoft.Storage/storageAccounts/fileServices/fileshares/files/read",
    "Microsoft.EventHub/namespaces/messages/receive/action",
    "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/items/read",
    "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/executeQuery",
    "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/readChangeFeed",
    // Runs a stored procedure, whose code reads and returns the container's items.
    "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/executeStoredProcedure",
    // Each reads every file in a share regardless of its ACLs.
    "Microsoft.Storage/storageAccounts/fileServices/readFileBackupSemantics/action",
    "Microsoft.FileShares/fileShares/readFileBackupSemantics/action",
    // Each reads a managed disk's or snapshot's bytes.
    "Microsoft.Compute/disks/download/action",
    "Microsoft.Compute/snapshots/download/action",
    // Pulls an image from a registry that authorizes by attributes.
    "Microsoft.ContainerRegistry/registries/repositories/content/read",
];

/// Azure built-in roles that carry an action on the two lists above. `Reader` and `Monitoring
/// Reader` carry the log query too, through `*/read`, but are not listed while Alien's own
/// heartbeat and management sets hold them at resource-group scope; narrowing those sets is a
/// separate change.
pub const AZURE_SENSITIVE_CONTENT_ROLES: &[&str] = &[
    "AcrPull",
    "AcrPush",
    // Carries `Microsoft.CognitiveServices/accounts/listkeys/action` and a
    // `Microsoft.CognitiveServices/*` dataAction, so it hands over the account key.
    "Cognitive Services User",
    // Carries the whole sandbox data plane, session contents included, so it belongs only to sets
    // meant to reach inside a session — `sandbox/execute` and `sandbox/remote-execute`. Named
    // from the shared constant so this list and the reach predicate cannot disagree.
    AZURE_SANDBOX_DATA_PLANE_ROLE,
    "Azure Service Bus Data Receiver",
    "Key Vault Secrets User",
    "Storage Blob Data Contributor",
    "Storage Blob Data Reader",
    "Storage Table Data Contributor",
    "Storage Table Data Reader",
    "Avere Contributor",
    "Reader and Data Access",
    "Storage Account Contributor",
    "Storage Connector Contributor",
    "Storage DataShare Contributor",
    "Storage Queue Data Contributor",
    "Storage Queue Data Message Processor",
    "Storage Queue Data Reader",
    "Defender for Storage Data Scanner",
    "Storage File Data Privileged Contributor",
    "Storage File Data Privileged Reader",
    "Storage File Data SMB Admin",
    "Storage File Data SMB MI Admin",
    "Storage File Data SMB Share Contributor",
    "Storage File Data SMB Share Elevated Contributor",
    "Storage File Data SMB Share Reader",
    "Azure Event Hubs Data Owner",
    "Azure Event Hubs Data Receiver",
    "Azure Service Bus Data Owner",
    "Key Vault Administrator",
    "Key Vault Certificate User",
    "Key Vault Secrets Officer",
    "Owner",
    "Contributor",
    "Storage Blob Data Owner",
    "Storage Account Key Operator Service Role",
    "Avere Operator",
];

/// AWS actions an extension's identity may not have, beyond [`AWS_SENSITIVE_CONTENT_ACTIONS`]:
/// reads Alien's own sets need (a function's code or settings, an image's layers, a topic's
/// messages), actions that grant access, share a resource or run code as another identity, ones
/// that return or mint a secret, key, password or token, and ones that decrypt or sign with a key,
/// read or query stored data or call the customer's code. Any action of a
/// service whose name the pattern covers as `iam` but a `Get` or `List`, and of one it covers as
/// `sts` but `GetCallerIdentity`, is refused as well.
pub const AWS_EXTENSION_IDENTITY_REFUSED_ACTIONS: &[&str] = &[
    "lambda:GetFunction",
    "lambda:GetFunctionConfiguration",
    "ecr:BatchGetImage",
    "ecr:GetDownloadUrlForLayer",
    "sns:Subscribe",
    // A resource policy, ACL or share opens the resource to another principal or account.
    "s3:PutBucketPolicy",
    "s3:PutBucketAcl",
    "s3:PutObjectAcl",
    "s3:PutObjectVersionAcl",
    "kms:PutKeyPolicy",
    "kms:CreateGrant",
    "secretsmanager:PutResourcePolicy",
    "sqs:AddPermission",
    "sqs:SetQueueAttributes",
    "lambda:AddPermission",
    "ecr:SetRepositoryPolicy",
    "dynamodb:PutResourcePolicy",
    "ec2:ModifySnapshotAttribute",
    "rds:ModifyDBSnapshotAttribute",
    "sns:AddPermission",
    "sns:SetTopicAttributes",
    "rds:ModifyDBClusterSnapshotAttribute",
    "ec2:ModifyImageAttribute",
    "ram:CreateResourceShare",
    "ram:AssociateResourceShare",
    "s3:PutAccessPointPolicy",
    "s3:PutMultiRegionAccessPointPolicy",
    "s3:PutBucketPublicAccessBlock",
    "s3:PutAccountPublicAccessBlock",
    "logs:PutResourcePolicy",
    "kinesis:PutResourcePolicy",
    "lambda:AddLayerVersionPermission",
    "lambda:CreateFunctionUrlConfig",
    "lambda:UpdateFunctionUrlConfig",
    "lakeformation:GrantPermissions",
    "elasticfilesystem:PutFileSystemPolicy",
    "glue:PutResourcePolicy",
    "backup:PutBackupVaultAccessPolicy",
    "redshift:AuthorizeSnapshotAccess",
    "redshift:AuthorizeDataShare",
    // Deleting a resource policy drops its explicit denies, and ownership controls turn ACLs back
    // on, so each opens access as a policy write does.
    "s3:DeleteBucketPolicy",
    "s3:DeleteAccessPointPolicy",
    "s3:PutBucketOwnershipControls",
    "secretsmanager:DeleteResourcePolicy",
    "ecr:DeleteRepositoryPolicy",
    "dynamodb:DeleteResourcePolicy",
    "logs:DeleteResourcePolicy",
    "kinesis:DeleteResourcePolicy",
    // Each runs code as the identity of the function, instance, task, build, job, session,
    // notebook, cluster, dev endpoint or environment, or signs in to an app that does.
    "lambda:UpdateFunctionCode",
    "lambda:UpdateFunctionConfiguration",
    "ec2:ModifyInstanceAttribute",
    "ssm:SendCommand",
    "ssm:StartSession",
    "ssm:CreateAssociation",
    "ssm:UpdateAssociation",
    "ssm:StartAutomationExecution",
    "ec2-instance-connect:SendSSHPublicKey",
    "ec2-instance-connect:SendSerialConsoleSSHPublicKey",
    "ecs:ExecuteCommand",
    "codebuild:StartBuild",
    "codebuild:StartBuildBatch",
    "codebuild:UpdateProject",
    "states:UpdateStateMachine",
    "ecs:RunTask",
    "ecs:StartTask",
    "codebuild:StartCommandExecution",
    "codebuild:StartSandboxConnection",
    "glue:StartJobRun",
    "glue:RunStatement",
    "athena:StartSession",
    "athena:StartCalculationExecution",
    "athena:CreatePresignedNotebookUrl",
    "sagemaker:CreatePresignedNotebookInstanceUrl",
    "sagemaker:CreatePresignedDomainUrl",
    "airflow:CreateCliToken",
    "airflow:CreateWebLoginToken",
    "airflow:InvokeRestApi",
    "bedrock-agentcore:InvokeAgentRuntimeCommand",
    "bedrock-agentcore:InvokeAgentRuntimeCommandShell",
    "bedrock-agentcore:InvokeCodeInterpreter",
    "elasticmapreduce:AddJobFlowSteps",
    "sagemaker:UpdateNotebookInstanceLifecycleConfig",
    "glue:UpdateDevEndpoint",
    "sagemaker:CreatePresignedMlflowTrackingServerUrl",
    "elasticmapreduce:CreateStudioPresignedUrl",
    "elasticmapreduce:GetOnClusterAppUIPresignedURL",
    // Each updates a stack, whose changes run as the stack's own service role.
    "cloudformation:ExecuteChangeSet",
    "cloudformation:UpdateStack",
    // Each grants a principal access to a cluster.
    "eks:CreateAccessEntry",
    "eks:UpdateAccessEntry",
    "eks:AssociateAccessPolicy",
    // Each mints a credential: a database or cache user and its password, an identity's sign-in
    // token or temporary credentials, or an API key, client secret or access token.
    "redshift:GetClusterCredentials",
    "redshift:GetClusterCredentialsWithIAM",
    "redshift-serverless:GetCredentials",
    "cognito-identity:GetOpenIdTokenForDeveloperIdentity",
    "elasticache:CreateUser",
    "elasticache:ModifyUser",
    "memorydb:CreateUser",
    "memorydb:UpdateUser",
    "datazone:GetEnvironmentCredentials",
    "datazone:GetDomainExecutionRoleCredentials",
    "elasticmapreduce:GetClusterSessionCredentials",
    "emr-containers:GetManagedEndpointSessionCredentials",
    "eks-auth:AssumeRoleForPodIdentity",
    "ssm:GetAccessToken",
    "connect:AdminGetEmergencyAccessToken",
    "grafana:CreateWorkspaceApiKey",
    "grafana:CreateWorkspaceServiceAccountToken",
    "lightsail:CreateBucketAccessKey",
    "appsync:CreateApiKey",
    "cognito-idp:AddUserPoolClientSecret",
    "bedrock-agentcore:GetResourceApiKey",
    "bedrock-agentcore:GetResourceOauth2Token",
    "bedrock-agentcore:GetResourcePaymentToken",
    "bedrock-agentcore:GetWorkloadAccessToken",
    "bedrock-agentcore:GetWorkloadAccessTokenForJWT",
    "bedrock-agentcore:GetWorkloadAccessTokenForUserId",
    // Each sets a user's password, so the caller can sign in as that user.
    "cognito-idp:AdminSetUserPassword",
    "ds:ResetUserPassword",
    "workmail:ResetPassword",
    // Each returns a stored secret, key, password or token.
    "secretsmanager:BatchGetSecretValue",
    "acm:ExportCertificate",
    "glue:GetConnection",
    "glue:GetConnections",
    "cognito-idp:DescribeUserPoolClient",
    "appsync:ListApiKeys",
    "events:RetrieveConnectionCredentials",
    "states:RevealSecrets",
    "wafv2:GetDecryptedAPIKey",
    "lightsail:GetRelationalDatabaseMasterUserPassword",
    "lightsail:DownloadDefaultKeyPair",
    "storagegateway:DescribeChapCredentials",
    "route53domains:RetrieveDomainAuthCode",
    "codeconnections:GetConnectionToken",
    "codeconnections:GetIndividualAccessToken",
    "codestar-connections:GetConnectionToken",
    "codestar-connections:GetIndividualAccessToken",
    "ivs:GetStreamKey",
    "ivs:BatchGetStreamKey",
    "payment-cryptography:ExportKey",
    "cognito-idp:ListUserPoolClientSecrets",
    "transfer:ExportHostKey",
    // Each changes which users may sign in to a cache.
    "elasticache:ModifyUserGroup",
    "memorydb:UpdateACL",
    // A database's or cache's ordinary update, such as `rds:ModifyDBInstance`,
    // `rds:ModifyDBCluster`, `elasticache:ModifyReplicationGroup`, `elasticache:ModifyCacheCluster`
    // or `redshift:ModifyCluster`, can set a master password or auth token too. AWS has no
    // condition key on that parameter, and refusing them would block managing any database, so
    // they are allowed.

    // Each widens what a role may do or who may take it on, mints a user's key, or acts as
    // another identity.
    "iam:PutRolePolicy",
    "iam:AttachRolePolicy",
    "iam:PassRole",
    "iam:CreatePolicyVersion",
    "iam:UpdateAssumeRolePolicy",
    "iam:CreateAccessKey",
    "sts:AssumeRole",
    // Each decrypts, signs, issues a certificate or derives a secret with a key. A new data key from
    // `kms:GenerateDataKey` decrypts nothing the key already protects, and a controller needs one
    // to write to a target the key encrypts, so that family is allowed.
    "kms:Decrypt",
    "kms:ReEncryptFrom",
    "kms:DeriveSharedSecret",
    "kms:Sign",
    "kms:GenerateMac",
    "payment-cryptography:DecryptData",
    "acm-pca:IssueCertificate",
    "signer:SignPayload",
    "signer:StartSigningJob",
    // Each runs a query over a database's or a data lake's contents, or returns its results.
    "rds-data:ExecuteStatement",
    "rds-data:BatchExecuteStatement",
    "rds-data:ExecuteSql",
    "athena:StartQueryExecution",
    "athena:GetQueryResults",
    "athena:GetQueryResultsStream",
    "redshift-data:ExecuteStatement",
    "redshift-data:BatchExecuteStatement",
    "redshift-data:GetStatementResult",
    "redshift:ExecuteQuery",
    "qldb:ExecuteStatement",
    "qldb:SendCommand",
    "sdb:Select",
    "cassandra:SelectMultiRegionResource",
    "iotsitewise:ExecuteQuery",
    "iottwinmaker:ExecuteQuery",
    "appsync:GraphQL",
    "opensearch:StartDirectQuery",
    "timestream:SelectValues",
    // Each returns an index's, a knowledge base's or an agent memory's contents.
    "bedrock:Retrieve",
    "bedrock:RetrieveAndGenerate",
    "bedrock-agentcore:RetrieveMemoryRecords",
    "kendra:Query",
    "kendra:Retrieve",
    "wisdom:Retrieve",
    // Each reads stored contents: an object, a media stream, a document, a repository's files, a
    // layer's code or a database's log.
    "s3:GetObjectVersionForReplication",
    "s3:GetObjectTorrent",
    "s3:GetObjectVersionTorrent",
    "s3-object-lambda:GetObject",
    "s3-object-lambda:GetObjectVersion",
    "s3-outposts:GetObject",
    "s3-outposts:GetObjectVersion",
    "s3-outposts:GetObjectVersionForReplication",
    "mediastore:GetObject",
    "mediapackagev2:GetObject",
    "kinesisvideo:GetMedia",
    "kinesisvideo:GetClip",
    "kinesisvideo:GetMediaForFragmentList",
    "workdocs:DownloadDocumentVersion",
    "codecommit:GitPull",
    "codecommit:GetBlob",
    "codecommit:GetFile",
    "lambda:GetLayerVersion",
    "rds:DownloadDBLogFilePortion",
    "rds:DownloadCompleteDBLogFile",
    // Each copies an instance's, image's, cache's, mail archive's or ledger's contents to a bucket.
    "ec2:ExportImage",
    "ec2:CreateInstanceExportTask",
    "ec2:CreateStoreImageTask",
    "elasticache:ExportServerlessCacheSnapshot",
    "elasticache:CopySnapshot",
    "memorydb:CopySnapshot",
    "ses:StartArchiveExport",
    "qldb:ExportJournalToS3",
    // An instance's administrator password.
    "ec2:GetPasswordData",
    // Every API Gateway read, an API key's value among them.
    "apigateway:GET",
    // Each runs the customer's code or workflow with input the caller chooses, and returns what
    // it returns.
    "lambda:InvokeFunction",
    "lambda:InvokeFunctionUrl",
    "lambda:InvokeAsync",
    "lambda:InvokeWebFunctionEndpoint",
    "execute-api:Invoke",
    "sagemaker:InvokeEndpoint",
    "sagemaker:InvokeEndpointAsync",
    "sagemaker:InvokeEndpointWithResponseStream",
    "states:StartExecution",
    "states:StartSyncExecution",
    "batch:SubmitJob",
    "vpc-lattice-svcs:Invoke",
    "vpc-lattice-svcs:Connect",
    "bedrock:InvokeAgent",
    "bedrock:InvokeInlineAgent",
    "bedrock:InvokeFlow",
    "bedrock-agentcore:InvokeAgentRuntime",
    "bedrock-agentcore:InvokeAgentRuntimeForUser",
    "bedrock-agentcore:InvokeAgentRuntimeWithWebSocketStream",
    "bedrock-agentcore:InvokeAgentRuntimeWithWebSocketStreamForUser",
    "bedrock-agentcore:InvokeGateway",
    // Each calls an HTTP endpoint with the credentials a customer's connection holds.
    "events:InvokeApiDestination",
    "states:InvokeHTTPEndpoint",
    // Each returns a workflow execution's input and output.
    "states:DescribeExecution",
    "states:GetExecutionHistory",
];

/// GCP permissions an extension's identity may not have, beyond
/// [`GCP_SENSITIVE_CONTENT_PERMISSIONS`]: acting as a service account, running code as one or
/// minting its credentials, changing who has access, decrypting or signing with a key, signing in
/// to an instance, returning a key, secret or token, querying stored data and calling the
/// customer's code. Any permission ending in `.setIamPolicy` is refused
/// as well.
pub const GCP_EXTENSION_IDENTITY_REFUSED_PERMISSIONS: &[&str] = &[
    // Reads a function's source code, as `lambda:GetFunction` does on AWS.
    "cloudfunctions.functions.sourceCodeGet",
    "iam.serviceAccounts.actAs",
    "iam.serviceAccounts.getAccessToken",
    "iam.serviceAccounts.getOpenIdToken",
    "iam.serviceAccounts.implicitDelegation",
    "iam.serviceAccounts.signBlob",
    "iam.serviceAccounts.signJwt",
    "iam.serviceAccountKeys.create",
    "iam.roles.create",
    "iam.roles.update",
    "resourcemanager.projects.setIamPolicy",
    // A key that signs as a service account.
    "storage.hmacKeys.create",
    // Deployment Manager creates resources as its own service agent.
    "deploymentmanager.deployments.create",
    // Alien's own queue sets need it; it subscribes to a topic's messages.
    "pubsub.topics.attachSubscription",
    "iam.serviceAccountKeys.upload",
    // Each lets an outside identity provider's tokens act as a service account.
    "iam.workloadIdentityPoolProviders.create",
    "iam.workloadIdentityPoolProviders.update",
    "iam.workloadIdentityPools.update",
    // Each creates a database user or sets one's password.
    "cloudsql.users.create",
    "cloudsql.users.update",
    // Each mints a credential for a cluster, or grants access or acts as another user inside one.
    "container.clusters.getCredentials",
    "container.serviceAccounts.createToken",
    "container.clusterRoleBindings.create",
    "container.clusterRoleBindings.update",
    "container.roleBindings.create",
    "container.roleBindings.update",
    "container.clusterRoles.bind",
    "container.clusterRoles.escalate",
    "container.roles.bind",
    "container.roles.escalate",
    "container.clusters.impersonate",
    "container.certificateSigningRequests.approve",
    "gkehub.gateway.generateCredentials",
    "gkemulticloud.awsClusters.generateAccessToken",
    "gkemulticloud.azureClusters.generateAccessToken",
    "edgecontainer.clusters.generateAccessToken",
    "edgecontainer.clusters.generateOfflineCredential",
    // Each returns or mints a key, secret, credential or token.
    "apikeys.keys.getKeyString",
    "iam.oauthClientCredentials.create",
    "iamconnectors.connectors.retrieveCredentials",
    "agentidentity.authProviders.retrieveCredentials",
    "agentidentity.authProviders.exchangeCredentials",
    "agentidentity.authorizations.retrieveCredentials",
    "agentidentity.authorizations.exchangeCredentials",
    "cloudbuild.repositories.accessReadToken",
    "cloudbuild.repositories.accessReadWriteToken",
    "developerconnect.gitRepositoryLinks.fetchReadToken",
    "developerconnect.gitRepositoryLinks.fetchReadWriteToken",
    "developerconnect.users.fetchAccessToken",
    "dataprocrm.nodes.mintOAuthToken",
    "recaptchaenterprise.keys.retrievelegacysecretkey",
    "vmwareengine.privateClouds.showNsxCredentials",
    "vmwareengine.privateClouds.showVcenterCredentials",
    "managedidentities.domains.resetpassword",
    "memorystore.authTokens.get",
    "composer.userworkloadssecrets.get",
    "apigee.keystores.export",
    // Each runs code as the identity of the instance, build, pod, job, cluster, workstation,
    // notebook or environment.
    "compute.instances.setMetadata",
    "compute.projects.setCommonInstanceMetadata",
    "cloudbuild.builds.create",
    "dataproc.jobs.create",
    "osconfig.patchJobs.exec",
    "osconfig.patchDeployments.execute",
    "osconfig.osPolicyAssignments.create",
    "osconfig.osPolicyAssignments.update",
    "composer.environments.executeAirflowCommand",
    "appengine.runtimes.actAsAdmin",
    "workstations.workstations.use",
    "notebooks.instances.use",
    "container.nodes.proxy",
    "container.pods.exec",
    "container.pods.attach",
    "container.pods.create",
    "container.pods.update",
    "container.deployments.create",
    "container.deployments.update",
    "container.jobs.create",
    "container.jobs.update",
    "container.cronJobs.create",
    "container.cronJobs.update",
    "container.daemonSets.create",
    "container.daemonSets.update",
    "container.statefulSets.create",
    "container.statefulSets.update",
    "container.replicaSets.create",
    "container.replicaSets.update",
    "container.replicationControllers.create",
    "container.replicationControllers.update",
    // A mutating webhook rewrites every pod the cluster admits, and either kind of webhook is
    // sent every object it admits, a secret's value among them.
    "container.mutatingWebhookConfigurations.create",
    "container.mutatingWebhookConfigurations.update",
    "container.validatingWebhookConfigurations.create",
    "container.validatingWebhookConfigurations.update",
    "run.jobs.runWithOverrides",
    // Each decrypts or signs with a key.
    "cloudkms.cryptoKeyVersions.useToDecrypt",
    "cloudkms.cryptoKeyVersions.useToSign",
    // Returns a parameter with the secrets it references filled in.
    "parametermanager.parameterVersions.render",
    // Each signs in to an instance, as a user or as its administrator.
    "compute.instances.osLogin",
    "compute.instances.osAdminLogin",
    // Each runs the customer's code or workflow with input the caller chooses, and returns what
    // it returns.
    "cloudfunctions.functions.call",
    "cloudfunctions.functions.invoke",
    "run.routes.invoke",
    "run.instances.invoke",
    "run.jobs.run",
    "workflows.executions.create",
    "composer.dags.execute",
    "cloudscheduler.jobs.run",
    "cloudtasks.tasks.run",
    "aiplatform.endpoints.predict",
    "aiplatform.endpoints.explain",
    "aiplatform.reasoningEngines.query",
    "aiplatform.extensions.execute",
    "ml.versions.predict",
    "ml.models.predict",
    "integrations.integrations.invoke",
    "integrations.integrationVersions.invoke",
    // Each returns a workflow execution's arguments and result.
    "workflows.executions.get",
    "workflows.executions.list",
    "integrations.executions.get",
    "integrations.executions.list",
    // Each runs a query over a database's contents, or reads its rows or source.
    "cloudsql.instances.executeSql",
    "cloudsql.instances.executeSqlReadOnly",
    "alloydb.instances.executeSql",
    "alloydb.instances.executeSqlReadOnly",
    "alloydb.instances.connect",
    "oracledatabase.databaseConnections.executeSql",
    "connectors.connections.executeSqlQuery",
    "bigtable.instances.executeQuery",
    "bigtable.authorizedViews.readRows",
    "bigtable.materializedViews.readRows",
    "bigtable.logicalViews.readRows",
    "bigquery.readsessions.getData",
    "bigquery.objectRefs.read",
    "biglake.tables.getData",
    "managedkafka.clusters.connect",
    "firebasedataconnect.connectors.impersonateQuery",
    "firebasedataconnect.connectors.impersonateMutation",
    "firebasedataconnect.services.executeGraphql",
    "firebasedataconnect.services.executeGraphqlRead",
    "aiplatform.ragCorpora.query",
    "aiplatform.memories.retrieve",
    "discoveryengine.servingConfigs.search",
    "discoveryengine.servingConfigs.answer",
    "discoveryengine.conversations.converse",
    "composer.dags.getSourceCode",
    "securesourcemanager.repositories.fetch",
    // Each copies a database's, cache's, disk's or volume's contents out of it.
    "cloudsql.backupRuns.export",
    "alloydb.clusters.export",
    "redis.backups.export",
    "memorystore.backups.export",
    "compute.instantSnapshots.export",
    "netapp.volumes.export",
];

/// Azure actions an extension's identity may not have, beyond [`AZURE_SENSITIVE_CONTENT_ACTIONS`]:
/// changing who has access, running code as another identity or the customer's code, reading an
/// image or a workflow run, sending logs elsewhere, and handing over or rewriting a key or secret.
/// Any action of a namespace the pattern covers as `Microsoft.Authorization` but a read is refused
/// as well, and so is any action the pattern covers as one of
/// [`AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_SUFFIXES`] or as a name of the shapes
/// [`AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_VERBS`] and
/// [`AZURE_EXTENSION_IDENTITY_REFUSED_NAME_ENDINGS`] describe, but the names
/// [`AZURE_EXTENSION_IDENTITY_SHAPE_EXCEPTIONS`] lists.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_ACTIONS: &[&str] = &[
    // Each changes who has access to the subscription's resources, a database, a cache or a
    // vault.
    "Microsoft.Authorization/roleAssignments/write",
    "Microsoft.Authorization/roleDefinitions/write",
    "Microsoft.DocumentDB/databaseAccounts/sqlRoleAssignments/write",
    "Microsoft.Cache/redis/accessPolicyAssignments/write",
    "Microsoft.KeyVault/vaults/accessPolicies/write",
    // Rewrites the vault's access policies.
    "Microsoft.KeyVault/vaults/write",
    // Each acts as a managed identity, or lets an outside identity provider's tokens act as one.
    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
    "Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials/write",
    // Each runs code on a machine, a cluster or a container, as its identity.
    "Microsoft.Compute/virtualMachines/runCommand/action",
    "Microsoft.Compute/virtualMachines/extensions/write",
    "Microsoft.Compute/virtualMachines/runCommands/write",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommand/action",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommands/write",
    "Microsoft.Compute/virtualMachineScaleSets/extensions/write",
    "Microsoft.ContainerService/managedClusters/runCommand/action",
    "Microsoft.ContainerInstance/containerGroups/containers/exec/action",
    "Microsoft.HybridCompute/machines/runcommands/write",
    "Microsoft.HybridCompute/machines/extensions/write",
    "Microsoft.SerialConsole/serialPorts/connect/action",
    "Microsoft.HDInsight/clusters/executeScriptActions/action",
    "Microsoft.NetworkCloud/bareMetalMachines/runCommand/action",
    "Microsoft.ContainerRegistry/registries/scheduleRun/action",
    // Signs in to a compute instance's applications, such as its terminal or Jupyter.
    "Microsoft.MachineLearningServices/workspaces/computes/applicationaccess/action",
    // Each deploys code that runs as the app's or the automation account's identity.
    "Microsoft.Web/sites/extensions/write",
    "Microsoft.Web/sites/slots/extensions/write",
    "Microsoft.Web/sites/publish/Action",
    "Microsoft.Web/sites/slots/publish/Action",
    "Microsoft.Automation/automationAccounts/runbooks/write",
    // `Microsoft.App/containerApps/write`, `Microsoft.App/jobs/write`, `Microsoft.Web/sites/write`
    // and `Microsoft.Web/sites/config/write` change an app's image along with its identity. They
    // are allowed because they are how an app is managed at all, as AWS allows a service's own
    // update.

    // Each is a token for a container app's or job's exec, log stream and port forward.
    "Microsoft.App/containerApps/getAuthToken/action",
    "Microsoft.App/containerApps/authtoken/action",
    "Microsoft.App/jobs/authtoken/action",
    // Each runs the customer's job, runbook, pipeline or workflow.
    "Microsoft.App/jobs/start/action",
    "Microsoft.Logic/workflows/triggers/run/action",
    "Microsoft.Logic/workflows/run/action",
    "Microsoft.App/logicApps/invoke/action",
    "Microsoft.Automation/automationAccounts/jobs/write",
    "Microsoft.DataFactory/factories/pipelines/createrun/action",
    "Microsoft.Web/sites/triggeredwebjobs/run/action",
    "Microsoft.Web/sites/slots/triggeredwebjobs/run/action",
    // Each returns a workflow run's inputs and outputs.
    "Microsoft.Logic/workflows/runs/read",
    "Microsoft.Logic/workflows/runs/actions/read",
    "Microsoft.Logic/workflows/runs/actions/repetitions/read",
    "Microsoft.Logic/workflows/runs/actions/scoperepetitions/read",
    "Microsoft.Logic/workflows/runs/actions/listExpressionTraces/action",
    "Microsoft.Logic/workflows/runs/actions/repetitions/listExpressionTraces/action",
    // Sends a resource's logs to a workspace, account or hub the caller chooses.
    "Microsoft.Insights/diagnosticSettings/write",
    // Each hands over or rewrites a key, connection string or credential.
    "Microsoft.Storage/storageAccounts/regeneratekey/action",
    "Microsoft.Network/connections/sharedkey/action",
    "Microsoft.ServiceBus/namespaces/authorizationRules/listKeys/action",
    "Microsoft.ServiceBus/namespaces/queues/authorizationRules/listKeys/action",
    "Microsoft.ServiceBus/namespaces/topics/authorizationRules/listKeys/action",
    "Microsoft.EventHub/namespaces/authorizationRules/listKeys/action",
    "Microsoft.EventHub/namespaces/eventhubs/authorizationRules/listKeys/action",
    "Microsoft.DocumentDB/databaseAccounts/listKeys/action",
    "Microsoft.DocumentDB/databaseAccounts/listConnectionStrings/action",
    "Microsoft.DocumentDB/databaseAccounts/readonlykeys/action",
    "Microsoft.Cache/redis/listKeys/action",
    "Microsoft.Cache/redisEnterprise/databases/listKeys/action",
    "Microsoft.ContainerRegistry/registries/listCredentials/action",
    "Microsoft.ContainerService/managedClusters/listClusterAdminCredential/action",
    "Microsoft.ContainerService/managedClusters/listClusterUserCredential/action",
    "Microsoft.ContainerService/managedClusters/accessProfiles/listCredential/action",
    "Microsoft.App/jobs/listSecrets/action",
    // A kubeconfig with a short-lived administrator certificate.
    "Microsoft.RedHatOpenShift/hcpOpenShiftClusters/requestadmincredential/action",
    // An app's or slot's settings, which hold its connection strings and keys.
    "Microsoft.Web/sites/config/list/action",
    "Microsoft.Web/sites/slots/config/list/action",
    // The app's or slot's deployment credentials.
    "Microsoft.Web/sites/publishxml/action",
    "Microsoft.Web/sites/slots/publishxml/action",
    // Each hands over a key that calls the app's functions: the host's keys, or one function's
    // keys or secrets.
    "Microsoft.Web/sites/host/listkeys/action",
    "Microsoft.Web/sites/slots/host/listkeys/action",
    "Microsoft.Web/sites/functions/listkeys/action",
    "Microsoft.Web/sites/slots/functions/listkeys/action",
    "Microsoft.Web/sites/functions/listsecrets/action",
    "Microsoft.Web/sites/slots/functions/listsecrets/action",
    "Microsoft.Web/sites/functions/masterkey/read",
    "Microsoft.Web/sites/hostruntime/functions/keys/read",
    // Each hands over a key, token or connection string through a read: a gateway connection's
    // shared key, a workspace's agent keys, an app's connection string or its functions' token, a
    // notebook's token, an Application Insights component's, web test's or live metrics' token,
    // or a task's settings with their secrets.
    "Microsoft.Network/connections/sharedKey/read",
    "Microsoft.Network/vpnGateways/vpnConnections/vpnLinkConnections/sharedKeys/read",
    "Microsoft.OperationalInsights/workspaces/sharedKeys/action",
    "Microsoft.OperationalInsights/workspaces/sharedKeys/read",
    "Microsoft.OperationalInsights/workspaces/listKeys/read",
    "Microsoft.Web/sites/config/web/connectionstrings/read",
    "Microsoft.Web/sites/slots/config/web/connectionstrings/read",
    "Microsoft.Web/sites/functions/token/read",
    "Microsoft.MachineLearningServices/workspaces/listNotebookAccessToken/read",
    "Microsoft.Insights/Components/GetToken/Read",
    "Microsoft.Insights/Webtests/GetToken/Read",
    "Microsoft.Insights/generateLiveToken/Read",
    "Microsoft.ContainerRegistry/registries/tasks/listDetails/action",
    "Microsoft.ContainerRegistry/registries/taskruns/listDetails/action",
    "Microsoft.ContainerRegistry/registries/runs/listLogSasUrl/action",
    // An image's layers.
    "Microsoft.ContainerRegistry/registries/pull/read",
    // A SAS that reads the disk's or snapshot's data. Alien's own container set needs the disk
    // one, since Azure checks it on the source disk when a snapshot is created.
    "Microsoft.Compute/disks/beginGetAccess/action",
    "Microsoft.Compute/snapshots/beginGetAccess/action",
];

/// Endings of the Azure actions that hand over a resource's keys, secrets, credentials or
/// connection strings, whichever provider defines them, lowercase. An action or data action
/// pattern whose end can match one of these endings is refused, so every pattern ending in `*`
/// is.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_SUFFIXES: &[&str] = &[
    "/listkeys/action",
    "/listsecrets/action",
    "/listcredentials/action",
    "/listconnectionstrings/action",
    "/regeneratekey/action",
    "/regeneratekeys/action",
    "/listadminkeys/action",
    "/listquerykeys/action",
    "/listclusteradmincredential/action",
    "/listclusterusercredential/action",
    "/listclustermonitoringusercredential/action",
    "/listcallbackurl/action",
    "/listvalue/action",
    "/generatecredentials/action",
    "/listappsettings/action",
    "/listconnectionkeys/action",
    "/config/list/action",
    "/listsas/action",
];

/// How the last name of an Azure action that hands over or rewrites a key, secret, credential or
/// token starts, lowercase. An action whose last name before `/action` starts with one of these
/// and ends with one of [`AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_NOUNS`], such as
/// `listAdminKeys/action` or `regeneratePrimaryKey/action`, is refused, whichever provider
/// defines it.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_VERBS: &[&str] = &[
    "list",
    "request",
    "regenerate",
    "generate",
    "create",
    "get",
    "reset",
    "rotate",
    "show",
    "fetch",
    "retrieve",
    "renew",
];

/// How the last name of such an action ends, lowercase. A bare `sas` is left out: it also ends
/// `getIkeSas`, which returns a VPN connection's security associations.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_NOUNS: &[&str] = &[
    "key",
    "keys",
    "secret",
    "secrets",
    "credential",
    "credentials",
    "token",
    "tokens",
    "password",
    "passwords",
    "connectionstring",
    "connectionstrings",
    "sastoken",
    "sastokens",
    "accountsas",
    "servicesas",
    "settings",
];

/// How the last name of an Azure action or read that hands over a key, secret, credential or
/// token ends, whatever comes before it in that name, lowercase: `authtoken/action`,
/// `masterkey/read`. A read pattern is refused when its last name spells the ending out, so
/// `functions/*/read` is not, while `functions/*keys/read` is.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_NAME_ENDINGS: &[&str] = &[
    "key",
    "keys",
    "secret",
    "secrets",
    "credential",
    "credentials",
    "token",
    "tokens",
];

/// Azure actions of the shapes above that hand nothing over, so an extension's identity may have
/// them. Only the name itself is allowed: a wildcard that covers one is refused as usual.
pub const AZURE_EXTENSION_IDENTITY_SHAPE_EXCEPTIONS: &[&str] = &[
    // Each reads a key's, secret's or credential's properties, the reference to a key in Key
    // Vault or a public key, never the secret part.
    "Microsoft.KeyVault/vaults/keys/read",
    "Microsoft.KeyVault/managedHSMs/keys/read",
    "Microsoft.KeyVault/vaults/secrets/read",
    "Microsoft.Sql/servers/keys/read",
    "Microsoft.Sql/managedInstances/keys/read",
    "Microsoft.DBforMySQL/servers/keys/read",
    "Microsoft.DBforMariaDB/servers/keys/read",
    "Microsoft.Synapse/workspaces/keys/read",
    "Microsoft.ContainerRegistry/registries/tokens/read",
    "Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials/read",
    "Microsoft.Automation/automationAccounts/credentials/read",
    "Microsoft.AwsConnector/kmsKeys/read",
    "Microsoft.Compute/sshPublicKeys/read",
    "Microsoft.DeviceOnboarding/discoveryServices/ownershipVoucherPublicKeys/read",
    "Microsoft.StorageSync/storageSyncServices/syncGroups/cloudEndpoints/afssharemetadatacertificatepublickeys/read",
    // Each revokes or invalidates keys, tokens or credentials, and returns none.
    "Microsoft.ManagedIdentity/userAssignedIdentities/revokeTokens/action",
    "Microsoft.Storage/storageAccounts/revokeUserDelegationKeys/action",
    "Microsoft.RedHatOpenShift/hcpOpenShiftClusters/revokecredentials/action",
    "Microsoft.Web/connections/revokeConnectionKeys/action",
    "Microsoft.ApiManagement/service/gateways/invalidateDebugCredentials/action",
    "Microsoft.Network/bastionHosts/deleteShareableLinksByToken/action",
    // Each checks a key or secret the caller gives, or has the service fetch or sync one itself.
    "Microsoft.Sql/managedInstances/validateAzureKeyVaultEncryptionKey/action",
    "Microsoft.Cdn/ValidateSecret/action",
    "Microsoft.ApiManagement/service/certificates/refreshSecret/action",
    "Microsoft.ApiManagement/service/namedValues/refreshSecret/action",
    "Microsoft.ApiManagement/service/workspaces/certificates/refreshSecret/action",
    "Microsoft.ApiManagement/service/workspaces/namedValues/refreshSecret/action",
    "Microsoft.Batch/batchAccounts/syncAutoStorageKeys/action",
    "Microsoft.DataFactory/factories/integrationruntimes/synccredentials/action",
    "Microsoft.Synapse/workspaces/integrationRuntimes/syncCredentials/action",
    "Microsoft.MachineLearningServices/workspaces/resynckeys/action",
    // A custom domain's validation token, which is published in its DNS.
    "Microsoft.Cdn/profiles/customdomains/RefreshValidationToken/action",
    "Microsoft.Cdn/operationresults/profileresults/customdomainresults/RefreshValidationToken/action",
    // A session host's token to join the host pool, which a virtual desktop controller hands its
    // hosts.
    "Microsoft.DesktopVirtualization/hostpools/retrieveRegistrationToken/action",
];

/// Azure data actions of the shapes above that hand nothing over, as
/// [`AZURE_EXTENSION_IDENTITY_SHAPE_EXCEPTIONS`] lists actions.
pub const AZURE_EXTENSION_IDENTITY_DATA_SHAPE_EXCEPTIONS: &[&str] = &[
    // Each reads a key's properties or public part.
    "Microsoft.KeyVault/vaults/keys/read",
    "Microsoft.CognitiveServices/accounts/ComputerVision/retrieval/publickey/read",
    // Each revokes every user delegation key, and returns none.
    "Microsoft.EventHub/namespaces/revokeUserDelegationKeys/action",
    "Microsoft.Relay/namespaces/revokeUserDelegationKeys/action",
    "Microsoft.ServiceBus/namespaces/revokeUserDelegationKeys/action",
];

/// Azure data actions an extension's identity may not have, beyond
/// [`AZURE_SENSITIVE_CONTENT_DATA_ACTIONS`]: using or releasing a Key Vault, Managed HSM or code
/// signing key or changing who may use one, signing in to a machine, running code in a session
/// or an app instance, issuing a token for an AI account, exporting a workspace's logs, and, on
/// an AKS, Arc, Fleet Manager or AI manager cluster, reading secrets, running code, granting
/// access, acting as another user, admission webhooks and node client certificates, as far as
/// that cluster kind's catalog names them. Data actions of the shapes the action list describes
/// are refused as well, but the names [`AZURE_EXTENSION_IDENTITY_DATA_SHAPE_EXCEPTIONS`] lists.
pub const AZURE_EXTENSION_IDENTITY_REFUSED_DATA_ACTIONS: &[&str] = &[
    "Microsoft.KeyVault/vaults/keys/decrypt/action",
    "Microsoft.KeyVault/vaults/keys/unwrap/action",
    "Microsoft.KeyVault/vaults/keys/sign/action",
    "Microsoft.KeyVault/vaults/keys/release/action",
    "Microsoft.KeyVault/vaults/certificates/release/action",
    "Microsoft.CodeSigning/certificateProfiles/Sign/action",
    "Microsoft.KeyVault/managedHsm/keys/decrypt/action",
    "Microsoft.KeyVault/managedHsm/keys/unwrap/action",
    "Microsoft.KeyVault/managedHsm/keys/sign/action",
    "Microsoft.KeyVault/managedHsm/keys/export/action",
    "Microsoft.KeyVault/managedHsm/keys/release/action",
    // The security domain decrypts every key the HSM holds.
    "Microsoft.KeyVault/managedHsm/securitydomain/download/action",
    // Each changes who may use the HSM's keys.
    "Microsoft.KeyVault/managedHsm/roleAssignments/write/action",
    "Microsoft.KeyVault/managedHsm/roleDefinitions/write/action",
    // Each signs in to a machine, as a user or as its administrator.
    "Microsoft.Compute/virtualMachines/login/action",
    "Microsoft.Compute/virtualMachines/loginAsAdmin/action",
    "Microsoft.HybridCompute/machines/login/action",
    "Microsoft.HybridCompute/machines/loginAsAdmin/action",
    "Microsoft.Compute/virtualMachines/WACloginAsAdmin/action",
    "Microsoft.HybridCompute/machines/WACloginAsAdmin/action",
    "Microsoft.AzureStackHCI/Clusters/WACloginAsAdmin/Action",
    "Microsoft.AzureStackHCI/VirtualMachineInstances/WACloginAsAdmin/Action",
    "Microsoft.AzureStackHCI/virtualMachines/WACloginAsAdmin/Action",
    "Microsoft.ConnectedVMwarevSphere/virtualmachines/WACloginAsAdmin/action",
    // Each runs code in a session or an app instance.
    "Microsoft.App/sessionPools/executions/action",
    "Microsoft.App/artifactApps/exec/action",
    "Microsoft.App/containerApps/exec/action",
    "Microsoft.App/jobs/exec/action",
    "Microsoft.CognitiveServices/accounts/AIServices/agents/session/exec/action",
    "Microsoft.AppPlatform/Spring/apps/deployments/connect/action",
    // Exports a workspace's logs.
    "Microsoft.OperationalInsights/workspaces/jobs/export/action",
    // Each issues a token that calls the account's models, as its keys do.
    "Microsoft.CognitiveServices/accounts/OpenAI/issuetoken/action",
    "Microsoft.CognitiveServices/accounts/OpenAI/issuescopedtoken/action",
    "Microsoft.CognitiveServices/accounts/SpeechServices/issuetoken/action",
    // Each reads a cluster's secrets, runs code in it, grants access or acts as another user
    // inside it, is sent every object it admits, or requests a node's client certificate, as far
    // as each cluster kind's catalog names these.
    "Microsoft.ContainerService/managedClusters/secrets/read",
    "Microsoft.ContainerService/managedClusters/pods/exec/action",
    "Microsoft.ContainerService/managedClusters/pods/write",
    "Microsoft.ContainerService/managedClusters/apps/deployments/write",
    "Microsoft.ContainerService/managedClusters/apps/daemonsets/write",
    "Microsoft.ContainerService/managedClusters/apps/statefulsets/write",
    "Microsoft.ContainerService/managedClusters/apps/replicasets/write",
    "Microsoft.ContainerService/managedClusters/batch/jobs/write",
    "Microsoft.ContainerService/managedClusters/batch/cronjobs/write",
    "Microsoft.ContainerService/managedClusters/replicationcontrollers/write",
    "Microsoft.ContainerService/managedClusters/admissionregistration.k8s.io/mutatingwebhookconfigurations/write",
    "Microsoft.ContainerService/managedClusters/admissionregistration.k8s.io/validatingwebhookconfigurations/write",
    "Microsoft.ContainerService/managedClusters/rbac.authorization.k8s.io/clusterrolebindings/write",
    "Microsoft.ContainerService/managedClusters/rbac.authorization.k8s.io/rolebindings/write",
    "Microsoft.ContainerService/managedClusters/rbac.authorization.k8s.io/clusterroles/bind/action",
    "Microsoft.ContainerService/managedClusters/rbac.authorization.k8s.io/clusterroles/escalate/action",
    "Microsoft.ContainerService/managedClusters/rbac.authorization.k8s.io/roles/bind/action",
    "Microsoft.ContainerService/managedClusters/rbac.authorization.k8s.io/roles/escalate/action",
    "Microsoft.ContainerService/managedClusters/serviceaccounts/impersonate/action",
    "Microsoft.ContainerService/managedClusters/users/impersonate/action",
    "Microsoft.ContainerService/managedClusters/groups/impersonate/action",
    "Microsoft.ContainerService/managedClusters/authentication.k8s.io/userextras/impersonate/action",
    "Microsoft.ContainerService/managedClusters/certificates.k8s.io/certificatesigningrequests/write",
    "Microsoft.ContainerService/managedClusters/certificates.k8s.io/certificatesigningrequests/nodeclient/action",
    "Microsoft.Kubernetes/connectedClusters/secrets/read",
    "Microsoft.Kubernetes/connectedClusters/pods/exec/action",
    "Microsoft.Kubernetes/connectedClusters/pods/write",
    "Microsoft.Kubernetes/connectedClusters/apps/deployments/write",
    "Microsoft.Kubernetes/connectedClusters/apps/daemonsets/write",
    "Microsoft.Kubernetes/connectedClusters/apps/statefulsets/write",
    "Microsoft.Kubernetes/connectedClusters/apps/replicasets/write",
    "Microsoft.Kubernetes/connectedClusters/batch/jobs/write",
    "Microsoft.Kubernetes/connectedClusters/batch/cronjobs/write",
    "Microsoft.Kubernetes/connectedClusters/replicationcontrollers/write",
    "Microsoft.Kubernetes/connectedClusters/admissionregistration.k8s.io/mutatingwebhookconfigurations/write",
    "Microsoft.Kubernetes/connectedClusters/admissionregistration.k8s.io/validatingwebhookconfigurations/write",
    "Microsoft.Kubernetes/connectedClusters/rbac.authorization.k8s.io/clusterrolebindings/write",
    "Microsoft.Kubernetes/connectedClusters/rbac.authorization.k8s.io/rolebindings/write",
    "Microsoft.Kubernetes/connectedClusters/rbac.authorization.k8s.io/clusterroles/bind/action",
    "Microsoft.Kubernetes/connectedClusters/rbac.authorization.k8s.io/clusterroles/escalate/action",
    "Microsoft.Kubernetes/connectedClusters/rbac.authorization.k8s.io/roles/bind/action",
    "Microsoft.Kubernetes/connectedClusters/rbac.authorization.k8s.io/roles/escalate/action",
    "Microsoft.Kubernetes/connectedClusters/serviceaccounts/impersonate/action",
    "Microsoft.Kubernetes/connectedClusters/users/impersonate/action",
    "Microsoft.Kubernetes/connectedClusters/groups/impersonate/action",
    "Microsoft.Kubernetes/connectedClusters/authentication.k8s.io/userextras/impersonate/action",
    "Microsoft.Kubernetes/connectedClusters/certificates.k8s.io/certificatesigningrequests/write",
    "Microsoft.ContainerService/fleets/secrets/read",
    "Microsoft.ContainerService/fleets/apps/deployments/write",
    "Microsoft.ContainerService/fleets/apps/daemonsets/write",
    "Microsoft.ContainerService/fleets/apps/statefulsets/write",
    "Microsoft.ContainerService/fleets/batch/jobs/write",
    "Microsoft.ContainerService/fleets/batch/cronjobs/write",
    "Microsoft.ContainerService/fleets/replicationcontrollers/write",
    "Microsoft.ContainerService/fleets/admissionregistration.k8s.io/mutatingwebhookconfigurations/write",
    "Microsoft.ContainerService/fleets/admissionregistration.k8s.io/validatingwebhookconfigurations/write",
    "Microsoft.ContainerService/fleets/rbac.authorization.k8s.io/clusterrolebindings/write",
    "Microsoft.ContainerService/fleets/rbac.authorization.k8s.io/rolebindings/write",
    "Microsoft.ContainerService/fleets/rbac.authorization.k8s.io/clusterroles/bind/action",
    "Microsoft.ContainerService/fleets/rbac.authorization.k8s.io/clusterroles/escalate/action",
    "Microsoft.ContainerService/fleets/rbac.authorization.k8s.io/roles/bind/action",
    "Microsoft.ContainerService/fleets/rbac.authorization.k8s.io/roles/escalate/action",
    "Microsoft.ContainerService/fleets/serviceaccounts/impersonate/action",
    "Microsoft.ContainerService/fleets/users/impersonate/action",
    "Microsoft.ContainerService/fleets/groups/impersonate/action",
    "Microsoft.ContainerService/fleets/authentication.k8s.io/userextras/impersonate/action",
    "Microsoft.ContainerService/fleets/certificates.k8s.io/certificatesigningrequests/write",
    "Microsoft.ContainerService/fleets/certificates.k8s.io/certificatesigningrequests/nodeclient/action",
    "Microsoft.ContainerService/fleets/members/secrets/read",
    "Microsoft.ContainerService/fleets/members/pods/exec/action",
    "Microsoft.ContainerService/fleets/members/pods/write",
    "Microsoft.ContainerService/fleets/members/apps/deployments/write",
    "Microsoft.ContainerService/fleets/members/apps/daemonsets/write",
    "Microsoft.ContainerService/fleets/members/apps/statefulsets/write",
    "Microsoft.ContainerService/fleets/members/apps/replicasets/write",
    "Microsoft.ContainerService/fleets/members/batch/jobs/write",
    "Microsoft.ContainerService/fleets/members/batch/cronjobs/write",
    "Microsoft.ContainerService/fleets/members/replicationcontrollers/write",
    "Microsoft.ContainerService/fleets/members/admissionregistration.k8s.io/mutatingwebhookconfigurations/write",
    "Microsoft.ContainerService/fleets/members/admissionregistration.k8s.io/validatingwebhookconfigurations/write",
    "Microsoft.ContainerService/fleets/members/rbac.authorization.k8s.io/clusterrolebindings/write",
    "Microsoft.ContainerService/fleets/members/rbac.authorization.k8s.io/rolebindings/write",
    "Microsoft.ContainerService/fleets/members/rbac.authorization.k8s.io/clusterroles/bind/action",
    "Microsoft.ContainerService/fleets/members/rbac.authorization.k8s.io/clusterroles/escalate/action",
    "Microsoft.ContainerService/fleets/members/rbac.authorization.k8s.io/roles/bind/action",
    "Microsoft.ContainerService/fleets/members/rbac.authorization.k8s.io/roles/escalate/action",
    "Microsoft.ContainerService/fleets/members/serviceaccounts/impersonate/action",
    "Microsoft.ContainerService/fleets/members/users/impersonate/action",
    "Microsoft.ContainerService/fleets/members/groups/impersonate/action",
    "Microsoft.ContainerService/fleets/members/authentication.k8s.io/userextras/impersonate/action",
    "Microsoft.ContainerService/fleets/members/certificates.k8s.io/certificatesigningrequests/write",
    "Microsoft.ContainerService/fleets/members/certificates.k8s.io/certificatesigningrequests/nodeclient/action",
    "Microsoft.ContainerService/aiManagers/secrets/read",
    "Microsoft.ContainerService/aiManagers/pods/exec/action",
    "Microsoft.ContainerService/aiManagers/pods/write",
    "Microsoft.ContainerService/aiManagers/apps/deployments/write",
    "Microsoft.ContainerService/aiManagers/apps/daemonsets/write",
    "Microsoft.ContainerService/aiManagers/apps/statefulsets/write",
    "Microsoft.ContainerService/aiManagers/apps/replicasets/write",
    "Microsoft.ContainerService/aiManagers/batch/jobs/write",
    "Microsoft.ContainerService/aiManagers/batch/cronjobs/write",
    "Microsoft.ContainerService/aiManagers/replicationcontrollers/write",
    "Microsoft.ContainerService/aiManagers/admissionregistration.k8s.io/mutatingwebhookconfigurations/write",
    "Microsoft.ContainerService/aiManagers/admissionregistration.k8s.io/validatingwebhookconfigurations/write",
    "Microsoft.ContainerService/aiManagers/rbac.authorization.k8s.io/clusterrolebindings/write",
    "Microsoft.ContainerService/aiManagers/rbac.authorization.k8s.io/rolebindings/write",
    "Microsoft.ContainerService/aiManagers/rbac.authorization.k8s.io/clusterroles/bind/action",
    "Microsoft.ContainerService/aiManagers/rbac.authorization.k8s.io/clusterroles/escalate/action",
    "Microsoft.ContainerService/aiManagers/rbac.authorization.k8s.io/roles/bind/action",
    "Microsoft.ContainerService/aiManagers/rbac.authorization.k8s.io/roles/escalate/action",
    "Microsoft.ContainerService/aiManagers/serviceaccounts/impersonate/action",
    "Microsoft.ContainerService/aiManagers/users/impersonate/action",
    "Microsoft.ContainerService/aiManagers/groups/impersonate/action",
    "Microsoft.ContainerService/aiManagers/authentication.k8s.io/userextras/impersonate/action",
    "Microsoft.ContainerService/aiManagers/certificates.k8s.io/certificatesigningrequests/write",
    "Microsoft.ContainerService/aiManagers/certificates.k8s.io/certificatesigningrequests/nodeclient/action",
];

/// Each grant of `permission_set` an extension's `provision`, `management` or `heartbeat` set
/// may not have, as "<cloud> <kind> <grant>": every [`sensitive_content_grants`] one, every
/// predefined role on GCP and Azure (the set lists the permissions it needs instead), and every
/// action on the extension lists above. A wildcard counts when it covers a listed action.
pub fn extension_identity_refusals(permission_set: &PermissionSet) -> Vec<String> {
    let mut found = sensitive_content_grants(permission_set);
    for entry in permission_set.platforms.aws.iter().flatten() {
        if !entry.effect.is_allow() {
            continue;
        }
        for pattern in entry.grant.actions.iter().flatten() {
            let lowered = pattern.to_ascii_lowercase();
            // A pattern that can match an `iam:` or `sts:` name, such as `*:PutUserPolicy`, is held
            // to their rules. A verb wildcard is read by what comes before it: `iam:Get*` reads,
            // `iam:Set*` may not.
            let verb = lowered.split_once(':').map_or("*", |(_, verb)| verb);
            let fixed = verb.split('*').next().unwrap_or_default();
            let escalates = (can_match_a_name_starting(&lowered, "iam:")
                && !(fixed.starts_with("get") || fixed.starts_with("list")))
                || (can_match_a_name_starting(&lowered, "sts:") && verb != "getcalleridentity");
            let listed = AWS_EXTENSION_IDENTITY_REFUSED_ACTIONS
                .iter()
                .any(|action| glob_covers(pattern, action));
            if (escalates || listed) && !found.contains(&format!("AWS action {pattern}")) {
                found.push(format!("AWS action {pattern}"));
            }
        }
    }
    for entry in permission_set.platforms.gcp.iter().flatten() {
        for role in entry.grant.predefined_roles.iter().flatten() {
            if !found.contains(&format!("GCP role {role}")) {
                found.push(format!("GCP role {role}"));
            }
        }
        let permissions = entry.grant.permissions.iter().flatten();
        let residual = entry.grant.residual_permissions.iter().flatten();
        for permission in permissions.chain(residual) {
            let refused = permission.to_ascii_lowercase().ends_with(".setiampolicy")
                || GCP_EXTENSION_IDENTITY_REFUSED_PERMISSIONS
                    .iter()
                    .any(|listed| glob_covers(permission, listed));
            if refused && !found.contains(&format!("GCP permission {permission}")) {
                found.push(format!("GCP permission {permission}"));
            }
        }
    }
    for entry in permission_set.platforms.azure.iter().flatten() {
        for role in entry.grant.predefined_roles.iter().flatten() {
            if !found.contains(&format!("Azure role {role}")) {
                found.push(format!("Azure role {role}"));
            }
        }
        let hands_over_a_key = |name: &str, exceptions: &[&str]| {
            can_match_a_key_handing_name(&name.to_ascii_lowercase())
                && !exceptions
                    .iter()
                    .any(|exception| exception.eq_ignore_ascii_case(name))
        };
        for action in entry.grant.actions.iter().flatten() {
            let lowered = action.to_ascii_lowercase();
            let authorization = can_match_a_name_starting(&lowered, "microsoft.authorization/")
                && !lowered.ends_with("/read");
            let refused = authorization
                || hands_over_a_key(action, AZURE_EXTENSION_IDENTITY_SHAPE_EXCEPTIONS)
                || AZURE_EXTENSION_IDENTITY_REFUSED_ACTIONS
                    .iter()
                    .any(|listed| glob_covers(action, listed));
            if refused && !found.contains(&format!("Azure action {action}")) {
                found.push(format!("Azure action {action}"));
            }
        }
        for data_action in entry.grant.data_actions.iter().flatten() {
            let refused =
                hands_over_a_key(data_action, AZURE_EXTENSION_IDENTITY_DATA_SHAPE_EXCEPTIONS)
                    || AZURE_EXTENSION_IDENTITY_REFUSED_DATA_ACTIONS
                        .iter()
                        .any(|listed| glob_covers(data_action, listed));
            if refused && !found.contains(&format!("Azure data action {data_action}")) {
                found.push(format!("Azure data action {data_action}"));
            }
        }
    }
    found
}

/// Each grant of `permission_set` that reaches a customer's stored contents, as
/// "<cloud> <kind> <grant>". A wildcard counts when it covers a listed action, so `s3:Get*` or
/// `*` is reported like `s3:GetObject`.
pub fn sensitive_content_grants(permission_set: &PermissionSet) -> Vec<String> {
    let mut found = Vec::new();
    let aws_listed = || {
        AWS_SENSITIVE_CONTENT_ACTIONS
            .iter()
            .chain(SENSITIVE_MICROVM_ACTIONS)
    };
    for entry in permission_set.platforms.aws.iter().flatten() {
        if !entry.effect.is_allow() {
            continue;
        }
        for pattern in entry.grant.actions.iter().flatten() {
            let lowered = pattern.to_ascii_lowercase();
            if aws_listed().any(|action| {
                iam_action_pattern_matches(
                    lowered.as_bytes(),
                    action.to_ascii_lowercase().as_bytes(),
                )
            }) {
                found.push(format!("AWS action {pattern}"));
            }
        }
    }
    for entry in permission_set.platforms.gcp.iter().flatten() {
        for role in entry.grant.predefined_roles.iter().flatten() {
            if GCP_SENSITIVE_CONTENT_ROLES.contains(&role.as_str()) {
                found.push(format!("GCP role {role}"));
            }
        }
        let permissions = entry.grant.permissions.iter().flatten();
        let residual = entry.grant.residual_permissions.iter().flatten();
        for permission in permissions.chain(residual) {
            if GCP_SENSITIVE_CONTENT_PERMISSIONS
                .iter()
                .any(|listed| glob_covers(permission, listed))
            {
                found.push(format!("GCP permission {permission}"));
            }
        }
    }
    for entry in permission_set.platforms.azure.iter().flatten() {
        for role in entry.grant.predefined_roles.iter().flatten() {
            if AZURE_SENSITIVE_CONTENT_ROLES
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(role))
            {
                found.push(format!("Azure role {role}"));
            }
        }
        for action in entry.grant.actions.iter().flatten() {
            if AZURE_SENSITIVE_CONTENT_ACTIONS
                .iter()
                .any(|listed| glob_covers(action, listed))
            {
                found.push(format!("Azure action {action}"));
            }
        }
        for data_action in entry.grant.data_actions.iter().flatten() {
            if AZURE_SENSITIVE_CONTENT_DATA_ACTIONS
                .iter()
                .any(|listed| glob_covers(data_action, listed))
            {
                found.push(format!("Azure data action {data_action}"));
            }
        }
    }
    found
}

/// Whether `pattern`, lowercase with `*` and `?` wildcards, matches some name that starts with
/// `prefix`.
fn can_match_a_name_starting(pattern: &str, prefix: &str) -> bool {
    fn reaches(pattern: &[u8], prefix: &[u8]) -> bool {
        match (pattern.first(), prefix.first()) {
            (_, None) | (Some(b'*'), _) => true,
            (Some(b'?'), Some(_)) => reaches(&pattern[1..], &prefix[1..]),
            (Some(expected), Some(actual)) if expected == actual => {
                reaches(&pattern[1..], &prefix[1..])
            }
            _ => false,
        }
    }
    reaches(pattern.as_bytes(), prefix.as_bytes())
}

/// Whether `pattern`, lowercase with `*` and `?` wildcards, matches some name that ends with
/// `suffix`.
fn can_match_a_name_ending(pattern: &str, suffix: &str) -> bool {
    let reversed = |text: &str| text.chars().rev().collect::<String>();
    can_match_a_name_starting(&reversed(pattern), &reversed(suffix))
}

/// Whether `pattern`, a lowercase Azure action or data action with `*` and `?` wildcards, can
/// match one that hands over a key: one with an ending of
/// [`AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_SUFFIXES`], or whose last name before `/action`
/// starts with one of [`AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_VERBS`] and ends with one of
/// [`AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_NOUNS`], or ends with one of
/// [`AZURE_EXTENSION_IDENTITY_REFUSED_NAME_ENDINGS`]; or a read whose last name spells out one
/// of those endings.
fn can_match_a_key_handing_name(pattern: &str) -> bool {
    if AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_SUFFIXES
        .iter()
        .any(|suffix| can_match_a_name_ending(pattern, suffix))
    {
        return true;
    }
    if let Some(read) = pattern.strip_suffix("/read") {
        let name = read.rsplit('/').next().unwrap_or(read);
        if AZURE_EXTENSION_IDENTITY_REFUSED_NAME_ENDINGS
            .iter()
            .any(|ending| name.ends_with(ending))
        {
            return true;
        }
    }
    let pattern: Vec<Glob> = pattern
        .bytes()
        .map(|byte| match byte {
            b'*' => Glob::Run,
            b'?' => Glob::AnyByte,
            byte => Glob::Byte(byte),
        })
        .collect();
    let action_named = |start: &str, end: &str| {
        let shape: Vec<Glob> = [Glob::Run, Glob::Byte(b'/')]
            .into_iter()
            .chain(start.bytes().map(Glob::Byte))
            .chain([Glob::RunInName])
            .chain(end.bytes().map(Glob::Byte))
            .chain("/action".bytes().map(Glob::Byte))
            .collect();
        globs_overlap(&pattern, &shape)
    };
    AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_VERBS
        .iter()
        .any(|verb| {
            AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_NOUNS
                .iter()
                .any(|noun| action_named(verb, noun))
        })
        || AZURE_EXTENSION_IDENTITY_REFUSED_NAME_ENDINGS
            .iter()
            .any(|ending| action_named("", ending))
}

#[derive(Clone, Copy)]
enum Glob {
    Byte(u8),
    AnyByte,
    /// Any run of bytes, `/` included.
    Run,
    /// Any run of bytes within one name, so without a `/`.
    RunInName,
}

/// Whether some text matches both `a` and `b`. Walks the pairs of positions, each at most once,
/// so a pattern of many wildcards is checked in time proportional to the two lengths' product.
fn globs_overlap(a: &[Glob], b: &[Glob]) -> bool {
    let is_run = |glob: Glob| matches!(glob, Glob::Run | Glob::RunInName);
    let share_a_byte = |x: Glob, y: Glob| match (x, y) {
        (Glob::Byte(p), Glob::Byte(q)) => p == q,
        (Glob::Byte(p), Glob::RunInName) | (Glob::RunInName, Glob::Byte(p)) => p != b'/',
        _ => true,
    };
    let width = b.len() + 1;
    let mut seen = vec![false; (a.len() + 1) * width];
    let mut pending = vec![(0, 0)];
    while let Some((i, j)) = pending.pop() {
        if std::mem::replace(&mut seen[i * width + j], true) {
            continue;
        }
        if i == a.len() && j == b.len() {
            return true;
        }
        // A run may match nothing.
        if i < a.len() && is_run(a[i]) {
            pending.push((i + 1, j));
        }
        if j < b.len() && is_run(b[j]) {
            pending.push((i, j + 1));
        }
        // Both match one more byte; a run stays where it is to match more.
        if i < a.len() && j < b.len() && share_a_byte(a[i], b[j]) {
            let next_i = if is_run(a[i]) { i } else { i + 1 };
            let next_j = if is_run(b[j]) { j } else { j + 1 };
            if (next_i, next_j) != (i, j) {
                pending.push((next_i, next_j));
            }
        }
    }
    false
}

/// Whether `pattern`, with `*` wildcards, covers `name`, ignoring case.
fn glob_covers(pattern: &str, name: &str) -> bool {
    iam_action_pattern_matches(
        pattern.to_ascii_lowercase().as_bytes(),
        name.to_ascii_lowercase().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(platforms: serde_json::Value) -> PermissionSet {
        serde_json::from_value(serde_json::json!({
            "id": "acme/dns-zone/provision", "description": "d", "platforms": platforms
        }))
        .unwrap()
    }

    /// One entry granting `grant` on `cloud`, bound to the whole stack.
    fn grant(cloud: &str, grant: serde_json::Value) -> serde_json::Value {
        let binding = match cloud {
            "aws" => serde_json::json!({ "resources": ["*"] }),
            _ => serde_json::json!({ "scope": "${stackPrefix}" }),
        };
        serde_json::json!([{ "grant": grant, "binding": { "stack": binding } }])
    }

    #[test]
    fn a_grant_that_reads_contents_is_found_on_each_cloud_wildcards_included() {
        for (platforms, expected) in [
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["s3:getobject"] })) }),
                "AWS action s3:getobject",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["s3:Get*"] })) }),
                "AWS action s3:Get*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["*"] })) }),
                "AWS action *",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/storage.objectViewer"] })) }),
                "GCP role roles/storage.objectViewer",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "permissions": ["secretmanager.versions.access"] })) }),
                "GCP permission secretmanager.versions.access",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "dataActions": ["Microsoft.Storage/storageAccounts/blobServices/*"] })) }),
                "Azure data action Microsoft.Storage/storageAccounts/blobServices/*",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Storage/storageAccounts/listkeys/action"] })) }),
                "Azure action Microsoft.Storage/storageAccounts/listkeys/action",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["storage blob data reader"] })) }),
                "Azure role storage blob data reader",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/container.developer"] })) }),
                "GCP role roles/container.developer",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/redis.admin"] })) }),
                "GCP role roles/redis.admin",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/logging.viewer"] })) }),
                "GCP role roles/logging.viewer",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Storage File Data Privileged Reader"] })) }),
                "Azure role Storage File Data Privileged Reader",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Storage Queue Data Reader"] })) }),
                "Azure role Storage Queue Data Reader",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/owner"] })) }),
                "GCP role roles/owner",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/editor"] })) }),
                "GCP role roles/editor",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/viewer"] })) }),
                "GCP role roles/viewer",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Owner"] })) }),
                "Azure role Owner",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Contributor"] })) }),
                "Azure role Contributor",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Storage Blob Data Owner"] })) }),
                "Azure role Storage Blob Data Owner",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Storage Account Key Operator Service Role"] })) }),
                "Azure role Storage Account Key Operator Service Role",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Avere Operator"] })) }),
                "Azure role Avere Operator",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/storage.legacyObjectReader"] })) }),
                "GCP role roles/storage.legacyObjectReader",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/storage.legacyObjectOwner"] })) }),
                "GCP role roles/storage.legacyObjectOwner",
            ),
        ] {
            assert_eq!(sensitive_content_grants(&set(platforms)), [expected]);
        }
    }

    #[test]
    fn an_extension_identity_gets_no_predefined_role_access_change_or_extra_read() {
        for (platforms, expected) in [
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "predefinedRoles": ["roles/dns.admin"] })) }),
                "GCP role roles/dns.admin",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["s3:DeleteBucketPolicy"] })) }),
                "AWS action s3:DeleteBucketPolicy",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["ecr:DeleteRepositoryPolicy"] })) }),
                "AWS action ecr:DeleteRepositoryPolicy",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "permissions": ["iam.serviceAccounts.actAs"] })) }),
                "GCP permission iam.serviceAccounts.actAs",
            ),
            (
                serde_json::json!({ "gcp": grant("gcp", serde_json::json!({ "permissions": ["storage.buckets.setIamPolicy"] })) }),
                "GCP permission storage.buckets.setIamPolicy",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "predefinedRoles": ["Key Vault Contributor"] })) }),
                "Azure role Key Vault Contributor",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/roleAssignments/write"] })) }),
                "Azure action Microsoft.Authorization/roleAssignments/write",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/*"] })) }),
                "Azure action Microsoft.Authorization/*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:PassRole"] })) }),
                "AWS action iam:PassRole",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:*"] })) }),
                "AWS action iam:*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:DeleteRolePermissionsBoundary"] })) }),
                "AWS action iam:DeleteRolePermissionsBoundary",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["sts:AssumeRole"] })) }),
                "AWS action sts:AssumeRole",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["iam:Set*"] })) }),
                "AWS action iam:Set*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["sts:Get*"] })) }),
                "AWS action sts:Get*",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/locks/*"] })) }),
                "Azure action Microsoft.Authorization/locks/*",
            ),
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["ecr:BatchGetImage"] })) }),
                "AWS action ecr:BatchGetImage",
            ),
        ] {
            assert_eq!(extension_identity_refusals(&set(platforms)), [expected]);
        }
        // Reads the shared lists gained, for an extension and for Alien's own sets alike.
        for (platforms, expected) in [
            (
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["dynamodb:PartiQLSelect"] })) }),
                "AWS action dynamodb:PartiQLSelect",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "dataActions": ["Microsoft.KeyVault/vaults/secrets/getSecret/action"] })) }),
                "Azure data action Microsoft.KeyVault/vaults/secrets/getSecret/action",
            ),
            (
                serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Storage/storageAccounts/ListAccountSas/action"] })) }),
                "Azure action Microsoft.Storage/storageAccounts/ListAccountSas/action",
            ),
        ] {
            assert_eq!(
                sensitive_content_grants(&set(platforms.clone())),
                [expected]
            );
            assert_eq!(extension_identity_refusals(&set(platforms)), [expected]);
        }
        // `*` is reported once, as a read.
        assert_eq!(
            extension_identity_refusals(&set(
                serde_json::json!({ "aws": grant("aws", serde_json::json!({ "actions": ["*"] })) })
            )),
            ["AWS action *"]
        );
    }

    /// `(cloud, kind, grant)` reported by [`extension_identity_refusals`] as one refusal.
    fn refused_alone(cloud: &str, kind: &str, name: &str) -> Vec<String> {
        let field = match kind {
            "action" => "actions",
            "permission" => "permissions",
            _ => "dataActions",
        };
        let platforms =
            serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
        extension_identity_refusals(&set(platforms))
    }

    fn label(cloud: &str, kind: &str, name: &str) -> String {
        let cloud = match cloud {
            "aws" => "AWS",
            "gcp" => "GCP",
            _ => "Azure",
        };
        let kind = if kind == "data" { "data action" } else { kind };
        format!("{cloud} {kind} {name}")
    }

    #[test]
    fn an_extension_identity_shares_no_resource_with_another_principal() {
        for action in [
            "s3:PutBucketPolicy",
            "s3:PutBucketAcl",
            "s3:PutObjectAcl",
            "kms:PutKeyPolicy",
            "kms:CreateGrant",
            "secretsmanager:PutResourcePolicy",
            "sqs:AddPermission",
            "sqs:SetQueueAttributes",
            "lambda:AddPermission",
            "ecr:SetRepositoryPolicy",
            "dynamodb:PutResourcePolicy",
            "ec2:ModifySnapshotAttribute",
            "rds:ModifyDBSnapshotAttribute",
            "s3:PutBucket*",
            "sns:AddPermission",
            "sns:SetTopicAttributes",
            "rds:ModifyDBClusterSnapshotAttribute",
            "ec2:ModifyImageAttribute",
            "ram:CreateResourceShare",
            "s3:PutAccessPointPolicy",
            "s3:PutMultiRegionAccessPointPolicy",
            "s3:PutBucketPublicAccessBlock",
            "logs:PutResourcePolicy",
            "kinesis:PutResourcePolicy",
            "lambda:AddLayerVersionPermission",
            "lambda:CreateFunctionUrlConfig",
            "lambda:UpdateFunctionUrlConfig",
            "ram:AssociateResourceShare",
            "s3:PutObjectVersionAcl",
            "s3:PutAccountPublicAccessBlock",
        ] {
            assert_eq!(
                refused_alone("aws", "action", action),
                [label("aws", "action", action)]
            );
        }
    }

    #[test]
    fn an_extension_identity_runs_no_code_as_another_identity() {
        for (cloud, kind, name) in [
            ("aws", "action", "lambda:UpdateFunctionCode"),
            ("aws", "action", "ssm:SendCommand"),
            ("aws", "action", "ssm:StartSession"),
            ("aws", "action", "ecs:ExecuteCommand"),
            ("aws", "action", "codebuild:StartBuild"),
            ("aws", "action", "codebuild:StartBuildBatch"),
            ("aws", "action", "codebuild:UpdateProject"),
            ("aws", "action", "lambda:UpdateFunctionConfiguration"),
            ("aws", "action", "ec2:ModifyInstanceAttribute"),
            ("aws", "action", "ssm:CreateAssociation"),
            ("aws", "action", "ssm:StartAutomationExecution"),
            ("aws", "action", "ec2-instance-connect:SendSSHPublicKey"),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachines/runCommand/action",
            ),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachines/extensions/write",
            ),
            ("gcp", "permission", "storage.hmacKeys.create"),
            ("gcp", "permission", "deploymentmanager.deployments.create"),
            ("gcp", "permission", "compute.instances.setMetadata"),
            (
                "gcp",
                "permission",
                "compute.projects.setCommonInstanceMetadata",
            ),
            ("gcp", "permission", "cloudbuild.builds.create"),
            ("gcp", "permission", "container.pods.exec"),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachines/runCommands/write",
            ),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommand/action",
            ),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachineScaleSets/extensions/write",
            ),
            (
                "azure",
                "action",
                "Microsoft.ContainerService/managedClusters/runCommand/action",
            ),
            (
                "aws",
                "action",
                "ec2-instance-connect:SendSerialConsoleSSHPublicKey",
            ),
            ("aws", "action", "ssm:UpdateAssociation"),
            (
                "azure",
                "action",
                "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommands/write",
            ),
            (
                "azure",
                "action",
                "Microsoft.ContainerInstance/containerGroups/containers/exec/action",
            ),
            ("gcp", "permission", "container.pods.attach"),
            ("gcp", "permission", "container.pods.create"),
            ("gcp", "permission", "run.jobs.runWithOverrides"),
            ("gcp", "permission", "container.deployments.create"),
            ("gcp", "permission", "container.deployments.update"),
            ("gcp", "permission", "container.jobs.create"),
            ("gcp", "permission", "container.cronJobs.create"),
            ("gcp", "permission", "container.daemonSets.create"),
            ("gcp", "permission", "container.statefulSets.create"),
            ("gcp", "permission", "container.replicaSets.create"),
            ("gcp", "permission", "container.pods.update"),
            (
                "azure",
                "action",
                "Microsoft.App/containerApps/getAuthToken/action",
            ),
            ("aws", "action", "states:UpdateStateMachine"),
            ("gcp", "permission", "container.jobs.update"),
            ("gcp", "permission", "container.cronJobs.update"),
            ("gcp", "permission", "container.daemonSets.update"),
            ("gcp", "permission", "container.statefulSets.update"),
            ("gcp", "permission", "container.replicaSets.update"),
            (
                "gcp",
                "permission",
                "container.replicationControllers.create",
            ),
            (
                "gcp",
                "permission",
                "container.replicationControllers.update",
            ),
            (
                "gcp",
                "permission",
                "container.mutatingWebhookConfigurations.create",
            ),
            (
                "gcp",
                "permission",
                "container.mutatingWebhookConfigurations.update",
            ),
            ("azure", "action", "Microsoft.Web/sites/extensions/write"),
            (
                "azure",
                "action",
                "Microsoft.Web/sites/slots/extensions/write",
            ),
            (
                "azure",
                "action",
                "Microsoft.Automation/automationAccounts/runbooks/write",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
    }

    #[test]
    fn an_extension_identity_grants_no_access_and_mints_no_credential() {
        for (cloud, kind, name) in [
            ("aws", "action", "eks:CreateAccessEntry"),
            ("aws", "action", "eks:AssociateAccessPolicy"),
            ("aws", "action", "eks:UpdateAccessEntry"),
            ("aws", "action", "redshift:GetClusterCredentials"),
            ("aws", "action", "redshift:GetClusterCredentialsWithIAM"),
            ("aws", "action", "redshift-serverless:GetCredentials"),
            (
                "aws",
                "action",
                "cognito-identity:GetOpenIdTokenForDeveloperIdentity",
            ),
            ("gcp", "permission", "iam.serviceAccountKeys.upload"),
            (
                "gcp",
                "permission",
                "iam.workloadIdentityPoolProviders.create",
            ),
            (
                "gcp",
                "permission",
                "iam.workloadIdentityPoolProviders.update",
            ),
            ("gcp", "permission", "iam.workloadIdentityPools.update"),
            ("gcp", "permission", "cloudsql.users.update"),
            ("gcp", "permission", "cloudsql.users.create"),
            ("gcp", "permission", "container.clusters.getCredentials"),
            ("gcp", "permission", "container.serviceAccounts.createToken"),
            ("gcp", "permission", "container.clusterRoleBindings.create"),
            ("gcp", "permission", "container.clusterRoleBindings.update"),
            ("gcp", "permission", "container.roleBindings.create"),
            ("gcp", "permission", "container.roleBindings.update"),
            ("gcp", "permission", "container.clusterRoles.bind"),
            ("gcp", "permission", "container.clusterRoles.escalate"),
            ("gcp", "permission", "container.roles.bind"),
            ("gcp", "permission", "container.roles.escalate"),
            ("aws", "action", "elasticache:CreateUser"),
            ("aws", "action", "elasticache:ModifyUser"),
            ("aws", "action", "memorydb:CreateUser"),
            ("aws", "action", "memorydb:UpdateUser"),
            ("aws", "action", "elasticache:ModifyUserGroup"),
            ("aws", "action", "memorydb:UpdateACL"),
            (
                "azure",
                "action",
                "Microsoft.DocumentDB/databaseAccounts/sqlRoleAssignments/write",
            ),
            (
                "azure",
                "action",
                "Microsoft.Cache/redis/accessPolicyAssignments/write",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
    }

    #[test]
    fn an_extension_identity_gets_no_azure_key_and_rewrites_no_vault() {
        for action in [
            "Microsoft.KeyVault/vaults/write",
            "Microsoft.Storage/storageAccounts/regenerateKey/action",
            "Microsoft.ServiceBus/namespaces/authorizationRules/listKeys/action",
            "Microsoft.ServiceBus/namespaces/queues/authorizationRules/listKeys/action",
            "Microsoft.EventHub/namespaces/authorizationRules/listKeys/action",
            "Microsoft.EventHub/namespaces/eventhubs/authorizationRules/listKeys/action",
            "Microsoft.DocumentDB/databaseAccounts/listKeys/action",
            "Microsoft.DocumentDB/databaseAccounts/listConnectionStrings/action",
            "Microsoft.Cache/redis/listKeys/action",
            "Microsoft.ContainerRegistry/registries/listCredentials/action",
            "Microsoft.Web/sites/config/list/action",
            "Microsoft.EventHub/*",
            "Microsoft.Web/sites/publishxml/action",
            "Microsoft.ContainerService/managedClusters/listClusterAdminCredential/action",
            "Microsoft.ContainerService/managedClusters/listClusterUserCredential/action",
            "Microsoft.App/jobs/listSecrets/action",
            "Microsoft.DocumentDB/databaseAccounts/readonlykeys/action",
            "Microsoft.Compute/disks/beginGetAccess/action",
            "Microsoft.Compute/snapshots/beginGetAccess/action",
            "Microsoft.Web/sites/slots/publishxml/action",
            "Microsoft.Web/sites/slots/config/list/action",
            "Microsoft.Web/sites/host/listkeys/action",
            "Microsoft.ContainerService/managedClusters/accessProfiles/listCredential/action",
            "Microsoft.Cache/redisEnterprise/databases/listKeys/action",
            "Microsoft.Web/sites/slots/host/listkeys/action",
            "Microsoft.Web/sites/functions/listkeys/action",
            "Microsoft.Web/sites/slots/functions/listkeys/action",
            "Microsoft.Web/sites/functions/listsecrets/action",
            "Microsoft.Web/sites/slots/functions/listsecrets/action",
        ] {
            assert_eq!(
                refused_alone("azure", "action", action),
                [label("azure", "action", action)]
            );
            // Alien's own sets may hold them.
            let platforms = serde_json::json!({ "azure": grant("azure", serde_json::json!({ "actions": [action] })) });
            assert!(
                sensitive_content_grants(&set(platforms)).is_empty(),
                "{action}"
            );
        }
    }

    #[test]
    fn content_reads_are_refused_for_every_management_set() {
        for (cloud, kind, name) in [
            ("aws", "action", "dynamodb:ExportTableToPointInTime"),
            ("aws", "action", "logs:StartLiveTail"),
            ("aws", "action", "logs:CreateExportTask"),
            ("aws", "action", "logs:PutSubscriptionFilter"),
            ("aws", "action", "kinesis:SubscribeToShard"),
            ("aws", "action", "logs:GetLogRecord"),
            ("aws", "action", "ebs:GetSnapshotBlock"),
            ("aws", "action", "s3express:CreateSession"),
            ("aws", "action", "dax:GetItem"),
            ("aws", "action", "dax:BatchGetItem"),
            ("aws", "action", "dax:Query"),
            ("aws", "action", "dax:Scan"),
            ("aws", "action", "rds-db:connect"),
            ("aws", "action", "elasticache:Connect"),
            ("aws", "action", "memorydb:Connect"),
            ("gcp", "permission", "spanner.databases.read"),
            ("gcp", "permission", "spanner.databases.select"),
            ("gcp", "permission", "bigtable.tables.readRows"),
            ("gcp", "permission", "cloudsql.instances.export"),
            ("gcp", "permission", "datastore.databases.export"),
            ("gcp", "permission", "logging.views.access"),
            ("gcp", "permission", "logging.sinks.create"),
            ("gcp", "permission", "container.secrets.get"),
            ("gcp", "permission", "container.secrets.list"),
            ("gcp", "permission", "redis.instances.getAuthString"),
            ("gcp", "permission", "redis.instances.export"),
            ("aws", "action", "dsql:DbConnect"),
            ("aws", "action", "dsql:DbConnectAdmin"),
            ("gcp", "permission", "memorystore.authTokens.list"),
            ("gcp", "permission", "memorystore.instances.connect"),
            ("gcp", "permission", "redis.clusters.connect"),
            ("gcp", "permission", "cloudsql.instances.login"),
            ("gcp", "permission", "alloydb.users.login"),
            ("aws", "action", "es:ESHttpGet"),
            ("aws", "action", "es:ESHttpPost"),
            ("aws", "action", "neptune-db:connect"),
            ("aws", "action", "neptune-db:ReadDataViaQuery"),
            ("aws", "action", "kafka-cluster:ReadData"),
            ("aws", "action", "cassandra:Select"),
            ("aws", "action", "timestream:Select"),
            (
                "azure",
                "action",
                "Microsoft.OperationalInsights/workspaces/query/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.ServiceBus/namespaces/messages/receive/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.Storage/storageAccounts/queueServices/queues/messages/process/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.Storage/storageAccounts/fileServices/fileshares/files/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.EventHub/namespaces/messages/receive/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/items/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/executeQuery",
            ),
            (
                "azure",
                "data",
                "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/readChangeFeed",
            ),
            (
                "azure",
                "data",
                "Microsoft.DocumentDB/databaseAccounts/sqlDatabases/containers/executeStoredProcedure",
            ),
            (
                "azure",
                "data",
                "Microsoft.Storage/storageAccounts/fileServices/readFileBackupSemantics/action",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
            let field = match kind {
                "action" => "actions",
                "permission" => "permissions",
                _ => "dataActions",
            };
            let platforms =
                serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
            assert_eq!(
                sensitive_content_grants(&set(platforms)),
                [label(cloud, kind, name)]
            );
        }
        // Reads Alien's own sets need are refused for an extension alone.
        for (cloud, kind, name) in [
            ("aws", "action", "sns:Subscribe"),
            ("aws", "action", "lambda:GetFunctionConfiguration"),
            ("gcp", "permission", "pubsub.topics.attachSubscription"),
            (
                "azure",
                "action",
                "Microsoft.ContainerRegistry/registries/pull/read",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
            let field = if kind == "action" {
                "actions"
            } else {
                "permissions"
            };
            let platforms =
                serde_json::json!({ cloud: grant(cloud, serde_json::json!({ field: [name] })) });
            assert!(
                sensitive_content_grants(&set(platforms)).is_empty(),
                "{name}"
            );
        }
    }

    /// The service or namespace is a pattern too: one that covers `iam`, `sts` or
    /// `Microsoft.Authorization` is held to their rules.
    #[test]
    fn a_wildcard_service_is_held_to_the_access_rules() {
        for (cloud, name) in [
            ("aws", "*:PutUserPolicy"),
            ("aws", "i?m:PutRolePolicy"),
            ("aws", "*:AssumeRole"),
            ("aws", "s?s:TagSession"),
            ("aws", "*PutUserPolicy"),
            ("azure", "*/locks/write"),
            ("azure", "*/policyAssignments/write"),
            ("azure", "Microsoft.Auth*ments/write"),
            // Covers a log query and an image pull.
            ("azure", "*/read"),
            // Covers a gateway connection's shared key.
            ("azure", "Microsoft.Network/*/read"),
        ] {
            assert_eq!(
                refused_alone(cloud, "action", name),
                [label(cloud, "action", name)]
            );
        }
        for (cloud, name) in [
            ("azure", "Microsoft.Network/dnszones/*/read"),
            ("aws", "route53:Change*"),
        ] {
            assert!(refused_alone(cloud, "action", name).is_empty(), "{name}");
        }
    }

    /// Alien's own sets may decrypt and invoke; an extension's identity may not use a key on the
    /// data it protects, query stored data, sign in to an instance, or call the customer's code.
    #[test]
    fn an_extension_identity_decrypts_nothing_and_calls_no_customer_code() {
        for (cloud, kind, name) in [
            ("aws", "action", "kms:Decrypt"),
            ("aws", "action", "kms:ReEncryptFrom"),
            ("aws", "action", "kms:DeriveSharedSecret"),
            ("aws", "action", "kms:Sign"),
            ("aws", "action", "kms:GenerateMac"),
            ("aws", "action", "redshift-data:ExecuteStatement"),
            ("aws", "action", "redshift-data:GetStatementResult"),
            ("aws", "action", "execute-api:Invoke"),
            ("aws", "action", "lambda:InvokeAsync"),
            ("aws", "action", "sagemaker:InvokeEndpoint"),
            ("aws", "action", "states:DescribeExecution"),
            ("aws", "action", "states:GetExecutionHistory"),
            ("gcp", "permission", "cloudkms.cryptoKeyVersions.useToSign"),
            ("gcp", "permission", "cloudfunctions.functions.invoke"),
            ("gcp", "permission", "workflows.executions.get"),
            (
                "gcp",
                "permission",
                "parametermanager.parameterVersions.render",
            ),
            ("azure", "action", "Microsoft.App/jobs/start/action"),
            (
                "azure",
                "action",
                "Microsoft.Logic/workflows/triggers/run/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/vaults/keys/release/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/managedHsm/keys/decrypt/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/managedHsm/keys/unwrap/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/managedHsm/keys/sign/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/managedHsm/keys/export/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/managedHsm/keys/release/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/secrets/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/pods/exec/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/serviceaccounts/impersonate/action",
            ),
            ("aws", "action", "rds-data:ExecuteStatement"),
            ("aws", "action", "rds-data:BatchExecuteStatement"),
            ("aws", "action", "athena:StartQueryExecution"),
            ("aws", "action", "athena:GetQueryResults"),
            ("aws", "action", "athena:*"),
            ("aws", "action", "ec2:GetPasswordData"),
            ("aws", "action", "apigateway:GET"),
            ("aws", "action", "lambda:InvokeFunction"),
            ("aws", "action", "lambda:InvokeFunctionUrl"),
            ("aws", "action", "states:StartExecution"),
            ("aws", "action", "states:StartSyncExecution"),
            ("aws", "action", "batch:SubmitJob"),
            (
                "gcp",
                "permission",
                "cloudkms.cryptoKeyVersions.useToDecrypt",
            ),
            ("gcp", "permission", "compute.instances.osLogin"),
            ("gcp", "permission", "compute.instances.osAdminLogin"),
            ("gcp", "permission", "cloudfunctions.functions.call"),
            ("gcp", "permission", "run.routes.invoke"),
            ("gcp", "permission", "workflows.executions.create"),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/vaults/keys/decrypt/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/vaults/keys/unwrap/action",
            ),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/vaults/keys/sign/action",
            ),
            ("azure", "data", "Microsoft.KeyVault/vaults/keys/*"),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/vaults/secrets/getSecret/action",
            ),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
        for (cloud, kind, name) in [
            ("aws", "action", "kms:Encrypt"),
            ("aws", "action", "kms:GenerateDataKey"),
            ("aws", "action", "kms:GenerateDataKeyWithoutPlaintext"),
            ("aws", "action", "kms:GenerateDataKeyPair"),
            ("aws", "action", "kms:GenerateDataKeyPairWithoutPlaintext"),
            ("aws", "action", "kms:DescribeKey"),
            (
                "azure",
                "data",
                "Microsoft.KeyVault/vaults/keys/encrypt/action",
            ),
        ] {
            assert!(refused_alone(cloud, kind, name).is_empty(), "{name}");
        }
    }

    /// Any provider's action that hands over keys, secrets, credentials or connection strings is
    /// refused, listed or not.
    #[test]
    fn an_extension_identity_lists_no_azure_key_of_any_provider() {
        for action in [
            "Microsoft.SignalRService/SignalR/listkeys/action",
            "Microsoft.MachineLearningServices/workspaces/listKeys/action",
            "Microsoft.Web/staticSites/listSecrets/action",
            "Microsoft.ContainerService/fleets/listCredentials/action",
            "Microsoft.DocumentDB/mongoClusters/listConnectionStrings/action",
            "Microsoft.EventGrid/topics/regenerateKey/action",
            "Microsoft.Maps/accounts/regenerateKeys/action",
            "Microsoft.Search/searchServices/listAdminKeys/action",
            "Microsoft.Search/searchServices/listQueryKeys/action",
            "Microsoft.Kubernetes/connectedClusters/listClusterAdminCredential/action",
            "Microsoft.Kubernetes/connectedClusters/listClusterUserCredential/action",
            "Microsoft.ContainerService/managedClusters/listClusterMonitoringUserCredential/action",
            "Microsoft.Logic/workflows/triggers/listCallbackUrl/action",
            "Microsoft.ApiManagement/service/namedValues/listValue/action",
            "Microsoft.ContainerRegistry/registries/generateCredentials/action",
            "Microsoft.Web/sites/slots/listAppSettings/action",
            "Microsoft.Web/connections/listConnectionKeys/action",
            "Microsoft.Web/staticSites/config/list/action",
            "*/listKeys/action",
            "Microsoft.SignalRService/*",
            "Microsoft.SignalRService/SignalR/list*",
            "Microsoft.Maps/accounts/listSas/action",
            // Each is on no list: its last name starts with a verb and ends with a noun.
            "Microsoft.Search/searchServices/regenerateAdminKey/action",
            "Microsoft.Search/searchServices/createQueryKey/action",
            "Microsoft.MachineLearningServices/workspaces/listStorageAccountKeys/action",
            "Microsoft.RedHatOpenShift/openShiftClusters/listAdminCredentials/action",
            "Microsoft.ApiManagement/service/gateways/generateToken/action",
            "Microsoft.ContainerRegistry/registries/regenerateCredential/action",
            "Microsoft.Kubernetes/connectedClusters/listClusterUserCredentials/action",
            "Microsoft.HDInsight/clusters/getGatewaySettings/action",
            "MICROSOFT.AVS/privateClouds/LISTADMINCREDENTIALS/ACTION",
            "Microsoft.Example/things/list*/action",
            "Microsoft.Example/things/regenerate?rimaryKey/action",
            "Microsoft.Example/*Token/action",
            // Each is on no list: its last name ends with a key, secret, credential or token
            // whatever comes before it.
            "Microsoft.Example/getThings/token/action",
            "Microsoft.Example/things/useSecrets/action",
            "Microsoft.Example/things/masterKey/read",
            "Microsoft.Example/things/*Keys/read",
            "Microsoft.Example/things/requestAdminCredential/action",
        ] {
            assert_eq!(
                refused_alone("azure", "action", action),
                [label("azure", "action", action)]
            );
        }
        for action in [
            "Microsoft.SignalRService/SignalR/read",
            "Microsoft.SignalRService/SignalR/write",
            "Microsoft.Network/dnszones/*/read",
            // A VPN connection's security associations, not a SAS.
            "Microsoft.Network/connections/getikesas/action",
            // The verb and the noun must sit in the same name.
            "Microsoft.Example/getThings/password/action",
            // A read is refused by its ending only when the pattern spells it out.
            "Microsoft.Example/listThings/*/read",
        ] {
            assert!(
                refused_alone("azure", "action", action).is_empty(),
                "{action}"
            );
        }
    }

    /// Every entry of the extension lists is refused granted alone, data actions as data actions.
    #[test]
    fn every_listed_grant_is_refused_alone() {
        let lists: [(&str, &str, &[&str]); 4] = [
            ("aws", "action", AWS_EXTENSION_IDENTITY_REFUSED_ACTIONS),
            (
                "gcp",
                "permission",
                GCP_EXTENSION_IDENTITY_REFUSED_PERMISSIONS,
            ),
            ("azure", "action", AZURE_EXTENSION_IDENTITY_REFUSED_ACTIONS),
            (
                "azure",
                "data",
                AZURE_EXTENSION_IDENTITY_REFUSED_DATA_ACTIONS,
            ),
        ];
        for (cloud, kind, names) in lists {
            for name in names {
                assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
            }
        }
        for suffix in AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_SUFFIXES {
            for kind in ["action", "data"] {
                let name = format!("Microsoft.Example/things{suffix}");
                assert_eq!(
                    refused_alone("azure", kind, &name),
                    [label("azure", kind, &name)]
                );
            }
        }
    }

    /// Every verb and noun of the shape make a refused action or data action, and every ending
    /// makes a refused action and read, whichever provider defines them.
    #[test]
    fn every_name_of_the_azure_key_shapes_is_refused() {
        let mut names = Vec::new();
        for verb in AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_VERBS {
            for noun in AZURE_EXTENSION_IDENTITY_REFUSED_ACTION_NOUNS {
                names.push(format!(
                    "Microsoft.Example/things/{verb}Primary{noun}/action"
                ));
            }
        }
        for ending in AZURE_EXTENSION_IDENTITY_REFUSED_NAME_ENDINGS {
            names.push(format!("Microsoft.Example/things/primary{ending}/action"));
            names.push(format!("Microsoft.Example/things/{ending}/read"));
        }
        for name in &names {
            for kind in ["action", "data"] {
                assert_eq!(
                    refused_alone("azure", kind, name),
                    [label("azure", kind, name)]
                );
            }
        }
    }

    /// A shape exception is allowed as its own name and on its own plane; a wildcard over it, or a
    /// name beside it, is still refused.
    #[test]
    fn a_shape_exception_allows_only_its_own_name() {
        for name in AZURE_EXTENSION_IDENTITY_SHAPE_EXCEPTIONS {
            assert!(refused_alone("azure", "action", name).is_empty(), "{name}");
        }
        for name in AZURE_EXTENSION_IDENTITY_DATA_SHAPE_EXCEPTIONS {
            assert!(refused_alone("azure", "data", name).is_empty(), "{name}");
        }
        for (kind, name) in [
            ("action", "Microsoft.DesktopVirtualization/hostpools/*"),
            (
                "action",
                "Microsoft.DesktopVirtualization/hostpools/retrieve*",
            ),
            (
                "action",
                "Microsoft.DesktopVirtualization/hostpools/listRegistrationTokens/action",
            ),
            ("action", "Microsoft.KeyVault/vaults/*Keys/read"),
            // A revocation is allowed as an action, not as a data action, and the data action
            // that sets a secret is refused.
            (
                "data",
                "Microsoft.Storage/storageAccounts/revokeUserDelegationKeys/action",
            ),
            ("data", "Microsoft.KeyVault/vaults/secrets/setSecret/action"),
        ] {
            assert_eq!(
                refused_alone("azure", kind, name),
                [label("azure", kind, name)]
            );
        }
    }

    /// A wildcard is refused when it covers a listed name, and only then.
    #[test]
    fn a_wildcard_is_refused_when_it_covers_a_listed_grant() {
        for (cloud, kind, name) in [
            ("aws", "action", "kms:*"),
            ("aws", "action", "kms:Generate*"),
            ("aws", "action", "states:Describe*"),
            ("aws", "action", "ecs:*Task"),
            ("aws", "action", "glue:GetConnection*"),
            ("gcp", "permission", "run.jobs.*"),
            ("azure", "action", "Microsoft.Web/sites/*"),
            ("azure", "action", "Microsoft.Logic/workflows/runs/*"),
            // Each covers a listed read: an app's connection strings or its functions' keys, a
            // workflow run's inputs and outputs, a notebook's token.
            ("azure", "action", "Microsoft.Web/sites/*/read"),
            ("azure", "action", "Microsoft.Web/sites/functions/*/read"),
            ("azure", "action", "Microsoft.Logic/workflows/*/read"),
            ("azure", "action", "Microsoft.Logic/workflows/runs/*/read"),
            (
                "azure",
                "action",
                "Microsoft.MachineLearningServices/workspaces/*/read",
            ),
            ("azure", "data", "Microsoft.KeyVault/managedHsm/*"),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/*",
            ),
            (
                "azure",
                "data",
                "Microsoft.Kubernetes/connectedClusters/*/write",
            ),
            ("azure", "data", "Microsoft.Compute/virtualMachines/login*"),
        ] {
            assert_eq!(refused_alone(cloud, kind, name), [label(cloud, kind, name)]);
        }
        for (cloud, kind, name) in [
            ("aws", "action", "kms:GenerateDataKey*"),
            ("aws", "action", "ecs:Describe*"),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/pods/read",
            ),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/apps/*/read",
            ),
        ] {
            assert!(refused_alone(cloud, kind, name).is_empty(), "{name}");
        }
    }

    /// The calls an ordinary controller makes to create and update its resources stay allowed.
    #[test]
    fn a_controller_still_creates_and_updates_its_resources() {
        for (cloud, kind, name) in [
            ("aws", "action", "s3:CreateBucket"),
            ("aws", "action", "ecs:CreateService"),
            ("aws", "action", "ecs:UpdateService"),
            ("aws", "action", "ecs:RegisterTaskDefinition"),
            ("aws", "action", "glue:CreateConnection"),
            ("aws", "action", "lambda:CreateFunction"),
            ("aws", "action", "secretsmanager:CreateSecret"),
            ("aws", "action", "secretsmanager:DescribeSecret"),
            ("gcp", "permission", "compute.instances.create"),
            ("gcp", "permission", "run.jobs.create"),
            ("gcp", "permission", "run.jobs.update"),
            ("gcp", "permission", "workflows.workflows.update"),
            ("azure", "action", "Microsoft.Storage/storageAccounts/write"),
            ("azure", "action", "Microsoft.Logic/workflows/write"),
            ("azure", "action", "Microsoft.Logic/workflows/read"),
            ("azure", "action", "Microsoft.Network/connections/write"),
            ("azure", "action", "Microsoft.KeyVault/vaults/secrets/write"),
            (
                "azure",
                "data",
                "Microsoft.ContainerService/managedClusters/namespaces/write",
            ),
        ] {
            assert!(refused_alone(cloud, kind, name).is_empty(), "{name}");
        }
    }

    /// A descriptor's pattern of many `*?` pairs that never matches is checked in bounded time.
    #[test]
    fn a_pattern_of_many_wildcards_is_checked_quickly() {
        let pattern = "*?".repeat(16) + "#";
        let platforms = serde_json::json!({
            "aws": grant("aws", serde_json::json!({ "actions": [pattern] })),
            "gcp": grant("gcp", serde_json::json!({ "permissions": [pattern] })),
            "azure": grant("azure", serde_json::json!({ "actions": [pattern], "dataActions": [pattern] })),
        });
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(extension_identity_refusals(&set(platforms)));
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("the refusals are found within a second");
    }

    #[test]
    fn a_grant_that_only_manages_is_not_found() {
        let platforms = serde_json::json!({
            "aws": grant("aws", serde_json::json!({ "actions": ["route53:CreateHostedZone", "s3:PutBucketTagging"] })),
            "gcp": grant("gcp", serde_json::json!({ "permissions": ["dns.managedZones.create"] })),
            "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Network/dnszones/write"] })),
        });
        assert!(sensitive_content_grants(&set(platforms.clone())).is_empty());
        assert!(extension_identity_refusals(&set(platforms)).is_empty());
        let reads_access = serde_json::json!({
            "aws": grant("aws", serde_json::json!({ "actions": ["iam:GetRole", "iam:List*", "sts:GetCallerIdentity"] })),
            "azure": grant("azure", serde_json::json!({ "actions": ["Microsoft.Authorization/roleAssignments/read", "Microsoft.Authorization/*/read"] })),
        });
        assert!(extension_identity_refusals(&set(reads_access)).is_empty());
        // Each can set a database's master password or auth token, or change an app's image with
        // its identity, and is allowed: managing a database or an app needs it.
        for (cloud, name) in [
            ("aws", "rds:ModifyDBInstance"),
            ("aws", "rds:ModifyDBCluster"),
            ("aws", "elasticache:ModifyReplicationGroup"),
            ("aws", "redshift:ModifyCluster"),
            ("azure", "Microsoft.App/containerApps/write"),
            ("azure", "Microsoft.App/jobs/write"),
            ("azure", "Microsoft.Web/sites/write"),
            ("azure", "Microsoft.Web/sites/config/write"),
        ] {
            assert!(refused_alone(cloud, "action", name).is_empty(), "{name}");
        }
    }
}
