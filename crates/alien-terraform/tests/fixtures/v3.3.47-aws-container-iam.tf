# The IAM policy blocks v3.3.47 rendered for aws-container-e2e-stack.json, verbatim: the
# inline grants, managed policies and attachments an installed deployment holds before upgrading.

resource "aws_iam_role_policy" "alien_test_storage_execution_sa_storage-data-read_0" {
  name   = "alien-test-storage-storage-data-read-0"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ReadS3Objects", Effect = "Allow", Action = ["s3:ListBucket", "s3:GetObject", "s3:GetObjectVersion", "s3:GetObjectAcl"], Resource = ["arn:aws:s3:::${aws_s3_bucket.alien_test_storage.bucket}", "arn:aws:s3:::${aws_s3_bucket.alien_test_storage.bucket}/*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "alien_test_storage_execution_sa_storage-data-write_1" {
  name   = "alien-test-storage-storage-data-write-1"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "StorageDataWrite", Effect = "Allow", Action = ["s3:ListBucket", "s3:GetObject", "s3:GetObjectVersion", "s3:GetObjectAcl", "s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload", "s3:DeleteObjectVersion", "s3:PutObjectAcl", "s3:PutObjectTagging", "s3:RestoreObject"], Resource = ["arn:aws:s3:::${aws_s3_bucket.alien_test_storage.bucket}", "arn:aws:s3:::${aws_s3_bucket.alien_test_storage.bucket}/*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "alien_test_storage_management_storage-heartbeat_0" {
  name   = "alien-test-storage-storage-heartbeat-0"
  role   = aws_iam_role.management.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "CheckS3BucketHealth", Effect = "Allow", Action = ["s3:GetBucketLocation", "s3:GetBucketVersioning", "s3:GetLifecycleConfiguration", "s3:GetBucketPolicy", "s3:GetBucketAcl", "s3:GetEncryptionConfiguration", "s3:GetBucketPublicAccessBlock"], Resource = ["arn:aws:s3:::${aws_s3_bucket.alien_test_storage.bucket}"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "build_execution_sa_set_0" {
  name   = "build-logs-and-artifacts-0"
  role   = aws_iam_role.build_execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "BuildLogsAndArtifactsWriteRuntimeLogs", Effect = "Allow", Action = ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"], Resource = ["arn:aws:logs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:log-group:/aws/codebuild/${local.resource_prefix}-*:*"] }, { Sid = "BuildLogsAndArtifactsCodebuildCreateReportGroupPermissions", Effect = "Allow", Action = ["codebuild:CreateReportGroup", "codebuild:CreateReport", "codebuild:UpdateReport", "codebuild:BatchPutTestCases", "codebuild:BatchPutCodeCoverages"], Resource = ["arn:aws:codebuild:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:report-group/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "build_execution_sa_set_1" {
  name   = "build-execute-1"
  role   = aws_iam_role.build_execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "BuildExecute", Effect = "Allow", Action = ["codebuild:StartBuild", "codebuild:StartBuildBatch", "codebuild:StopBuild", "codebuild:StopBuildBatch", "codebuild:RetryBuild", "codebuild:RetryBuildBatch", "codebuild:BatchGetBuilds", "codebuild:BatchGetBuildBatches", "codebuild:ListBuildsForProject"], Resource = ["arn:aws:codebuild:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:project/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_0" {
  name   = "storage-data-read-0"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ReadS3Objects", Effect = "Allow", Action = ["s3:ListBucket", "s3:GetObject", "s3:GetObjectVersion", "s3:GetObjectAcl"], Resource = ["arn:aws:s3:::${local.resource_prefix}-*", "arn:aws:s3:::${local.resource_prefix}-*/*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_1" {
  name   = "storage-data-write-1"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "StorageDataWrite", Effect = "Allow", Action = ["s3:ListBucket", "s3:GetObject", "s3:GetObjectVersion", "s3:GetObjectAcl", "s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload", "s3:DeleteObjectVersion", "s3:PutObjectAcl", "s3:PutObjectTagging", "s3:RestoreObject"], Resource = ["arn:aws:s3:::${local.resource_prefix}-*", "arn:aws:s3:::${local.resource_prefix}-*/*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_2" {
  name   = "build-execute-2"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "BuildExecute", Effect = "Allow", Action = ["codebuild:StartBuild", "codebuild:StartBuildBatch", "codebuild:StopBuild", "codebuild:StopBuildBatch", "codebuild:RetryBuild", "codebuild:RetryBuildBatch", "codebuild:BatchGetBuilds", "codebuild:BatchGetBuildBatches", "codebuild:ListBuildsForProject"], Resource = ["arn:aws:codebuild:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:project/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_3" {
  name   = "artifact-registry-pull-3"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ArtifactRegistryPullEcrGetAuthorizationToken", Effect = "Allow", Action = ["ecr:GetAuthorizationToken"], Resource = ["*"] }, { Sid = "ArtifactRegistryPullReadEcrImages", Effect = "Allow", Action = ["ecr:BatchCheckLayerAvailability", "ecr:GetDownloadUrlForLayer", "ecr:BatchGetImage", "ecr:DescribeRepositories", "ecr:DescribeImages", "ecr:ListImages"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${local.resource_prefix}-*"] }, { Sid = "ArtifactRegistryPullStsAssumeRole", Effect = "Allow", Action = ["sts:AssumeRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*-pull"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_4" {
  name   = "artifact-registry-push-4"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ArtifactRegistryPushEcrGetAuthorizationToken", Effect = "Allow", Action = ["ecr:GetAuthorizationToken"], Resource = ["*"] }, { Sid = "ArtifactRegistryPushReadEcrImages", Effect = "Allow", Action = ["ecr:BatchCheckLayerAvailability", "ecr:GetDownloadUrlForLayer", "ecr:BatchGetImage", "ecr:DescribeRepositories", "ecr:DescribeImages", "ecr:ListImages", "ecr:CreateRepository", "ecr:DeleteRepository", "ecr:CompleteLayerUpload", "ecr:UploadLayerPart", "ecr:InitiateLayerUpload", "ecr:PutImage"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${local.resource_prefix}-*"] }, { Sid = "ArtifactRegistryPushStsAssumeRole", Effect = "Allow", Action = ["sts:AssumeRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*-push"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_5" {
  name   = "artifact-registry-provision-5"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ArtifactRegistryProvisionReadEcrImages", Effect = "Allow", Action = ["ecr:CreateRepository", "ecr:DeleteRepository", "ecr:SetRepositoryPolicy", "ecr:DeleteRepositoryPolicy", "ecr:TagResource", "ecr:PutLifecyclePolicy", "ecr:DeleteLifecyclePolicy", "ecr:PutImageScanningConfiguration", "ecr:PutImageTagMutability", "ecr:DescribeRepositories", "ecr:DescribeImages", "ecr:ListImages", "ecr:DescribeImageScanFindings", "ecr:GetRepositoryPolicy", "ecr:ListTagsForResource"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${local.resource_prefix}-*"] }, { Sid = "ArtifactRegistryProvisionEcrDescribeRegistry", Effect = "Allow", Action = ["ecr:DescribeRegistry"], Resource = ["*"] }, { Sid = "BoundedRoleCreate3", Effect = "Allow", Action = ["iam:CreateRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { "ForAllValues:StringEquals" = { "aws:TagKeys" = ["managed-by", "deployment", "resource", "resource-type", "Name"] }, StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "iam:PermissionsBoundary" = "${aws_iam_policy.management_permissions_boundary.arn}" } } }, { Sid = "BoundedRoleWrite3", Effect = "Allow", Action = ["iam:UpdateAssumeRolePolicy", "iam:PutRolePolicy", "iam:DeleteRolePolicy", "iam:DetachRolePolicy", "iam:PutRolePermissionsBoundary"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime", "iam:PermissionsBoundary" = "${aws_iam_policy.management_permissions_boundary.arn}" } } }, { Sid = "ArtifactRegistryProvisionIamGetRolePermissions", Effect = "Allow", Action = ["iam:GetRole", "iam:GetRolePolicy", "iam:ListRolePolicies", "iam:ListAttachedRolePolicies"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"] }, { Sid = "BoundedRoleTag4", Effect = "Allow", Action = ["iam:TagRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { "ForAllValues:StringEquals" = { "aws:TagKeys" = ["managed-by", "deployment", "resource", "resource-type", "Name"] }, StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "BoundedRoleDelete4", Effect = "Allow", Action = ["iam:DeleteRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_6" {
  name   = "vault-data-read-6"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "VaultDataRead", Effect = "Allow", Action = ["ssm:GetParameter"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_7" {
  name   = "vault-data-write-7"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "WriteSsmParameters", Effect = "Allow", Action = ["ssm:PutParameter", "ssm:DeleteParameter"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_8" {
  name   = "kv-data-read-8"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "KvDataRead", Effect = "Allow", Action = ["dynamodb:GetItem", "dynamodb:Query", "dynamodb:Scan", "dynamodb:DescribeTable"], Resource = ["arn:aws:dynamodb:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:table/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_9" {
  name   = "kv-data-write-9"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "KvDataWrite", Effect = "Allow", Action = ["dynamodb:GetItem", "dynamodb:Query", "dynamodb:Scan", "dynamodb:PutItem", "dynamodb:UpdateItem", "dynamodb:DeleteItem", "dynamodb:DescribeTable"], Resource = ["arn:aws:dynamodb:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:table/${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_10" {
  name   = "queue-data-read-10"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "QueueDataRead", Effect = "Allow", Action = ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:ChangeMessageVisibility", "sqs:GetQueueAttributes", "sqs:GetQueueUrl"], Resource = ["arn:aws:sqs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_11" {
  name   = "queue-data-write-11"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "SendSqsMessages", Effect = "Allow", Action = ["sqs:SendMessage", "sqs:DeleteMessage", "sqs:ChangeMessageVisibility", "sqs:GetQueueAttributes", "sqs:GetQueueUrl"], Resource = ["arn:aws:sqs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:${local.resource_prefix}-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_12" {
  name   = "worker-execute-12"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "WriteLambdaLogs", Effect = "Allow", Action = ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"], Resource = ["arn:aws:logs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:log-group:/aws/lambda/${local.resource_prefix}-*", "arn:aws:logs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:log-group:/aws/lambda/${local.resource_prefix}-*:*"] }, { Sid = "ReadLambdaFunctionUrlConfig", Effect = "Allow", Action = ["lambda:GetFunctionUrlConfig"], Resource = ["arn:aws:lambda:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:function:${local.resource_prefix}-*"] }, { Sid = "GetEcrLoginToken", Effect = "Allow", Action = ["ecr:GetAuthorizationToken"], Resource = ["*"] }, { Sid = "ReadDeploymentContainerImages", Effect = "Allow", Action = ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"], Resource = ["arn:aws:ecr:*:${var.managing_account_id}:repository/*"] }, { Sid = "ManageLambdaVpcNetworkInterfaces", Effect = "Allow", Action = ["ec2:CreateNetworkInterface", "ec2:DescribeNetworkInterfaces", "ec2:DeleteNetworkInterface", "ec2:AssignPrivateIpAddresses", "ec2:UnassignPrivateIpAddresses"], Resource = ["*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_set_13" {
  name   = "vault-protect-unmapped-deployer-secrets-13"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "VaultProtectUnmappedDeployerSecretsSsmPutParameterPermissions", Effect = "Deny", Action = ["ssm:PutParameter", "ssm:DeleteParameter*", "ssm:LabelParameterVersion", "ssm:UnlabelParameterVersion", "ssm:AddTagsToResource", "ssm:RemoveTagsFromResource", "ssm:PutResourcePolicy", "ssm:DeleteResourcePolicy"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-deployer-secrets-*"] }, { Sid = "VaultProtectUnmappedDeployerSecretsSsmGetParametersByPath", Effect = "Deny", Action = ["ssm:GetParametersByPath"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_policy" "management_permissions_boundary" {
  name   = "${local.resource_prefix}-deployment-management-boundary"
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Effect = "Allow", Resource = ["arn:aws:codebuild:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:project/${local.resource_prefix}-*", "arn:aws:codebuild:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:report-group/${local.resource_prefix}-*", "arn:aws:dynamodb:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:table/${local.resource_prefix}-*", "arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${local.resource_prefix}-*", "arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*", "arn:aws:lambda:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:function:${local.resource_prefix}-*", "arn:aws:lambda:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:microvm-image:${local.resource_prefix}-*", "arn:aws:logs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:log-group:/aws/codebuild/${local.resource_prefix}-*:*", "arn:aws:logs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:log-group:/aws/lambda/${local.resource_prefix}-*", "arn:aws:rds:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:cluster:${local.resource_prefix}-*", "arn:aws:s3:::${local.resource_prefix}-*", "arn:aws:secretsmanager:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:secret:${local.resource_prefix}-*", "arn:aws:ses:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:configuration-set/${local.resource_prefix}-*", "arn:aws:sqs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:${local.resource_prefix}-*", "arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-*"], NotAction = "iam:*" }, { Effect = "Allow", Resource = "*", Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}" } }, NotAction = "iam:*" }, { Effect = "Allow", Action = ["bedrock-mantle:CreateInference", "bedrock:InvokeModel", "bedrock:InvokeModelWithResponseStream"], Resource = ["arn:aws:bedrock-mantle:*:${data.aws_caller_identity.current.account_id}:project/default", "arn:aws:bedrock:*:${data.aws_caller_identity.current.account_id}:inference-profile/*", "arn:aws:bedrock:*::foundation-model/*"] }, { Effect = "Allow", Action = ["ec2:AssignPrivateIpAddresses", "ec2:CreateNetworkInterface", "ec2:DeleteNetworkInterface", "ec2:DescribeInstances", "ec2:DescribeNetworkInterfaces", "ec2:DescribeSnapshots", "ec2:DescribeTags", "ec2:DescribeVolumes", "ec2:UnassignPrivateIpAddresses", "ecr:GetAuthorizationToken", "elasticloadbalancing:DescribeTargetHealth", "ses:ListEmailIdentities"], Resource = "*" }, { Effect = "Allow", Action = ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"], Resource = "arn:aws:ecr:*:${var.managing_account_id}:repository/*" }, { Effect = "Allow", Action = "ec2:CreateSnapshot", Resource = "arn:aws:ec2:${data.aws_region.current.region}::snapshot/*", Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Effect = "Allow", Action = "ec2:CreateTags", Resource = "arn:aws:ec2:${data.aws_region.current.region}::snapshot/*", Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "ec2:CreateAction" = "CreateSnapshot" } } }, { Effect = "Allow", Action = ["ses:CreateEmailIdentity", "ses:DeleteEmailIdentity", "ses:GetEmailIdentity", "ses:PutEmailIdentityConfigurationSetAttributes", "ses:PutEmailIdentityDkimAttributes", "ses:PutEmailIdentityFeedbackAttributes", "ses:PutEmailIdentityMailFromAttributes", "ses:SendEmail", "ses:SendRawEmail"], Resource = "arn:aws:ses:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:identity/*" }, { Effect = "Allow", Action = "aoss:APIAccessAll", Resource = "arn:aws:aoss:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:collection/*" }, { Effect = "Allow", Action = "lambda:PassNetworkConnector", Resource = "arn:aws:lambda:${data.aws_region.current.region}:aws:network-connector:aws-network-connector:*" }, { Effect = "Deny", Action = ["ssm:AddTagsToResource", "ssm:DeleteParameter*", "ssm:DeleteResourcePolicy", "ssm:LabelParameterVersion", "ssm:PutParameter", "ssm:PutResourcePolicy", "ssm:RemoveTagsFromResource", "ssm:UnlabelParameterVersion"], Resource = "arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-deployer-secrets-*" }, { Effect = "Deny", Action = "ssm:GetParametersByPath", Resource = "arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/" }, { Effect = "Deny", Action = ["aoss:*AccessPolicy", "aoss:*SecurityPolicy", "backup:PutBackupVaultAccessPolicy", "codebuild:PutResourcePolicy", "dlm:*LifecyclePolicy", "dynamodb:PutResourcePolicy", "ec2:CopySnapshot", "ec2:CreateVpcPeeringConnection", "ec2:Modify*ImageAttribute", "ec2:ModifySnapshotAttribute", "ec2:ModifyVpcEndpointServicePermissions", "ecr:*RepositoryCreationTemplate", "ecr:PutRegistryPolicy", "ecr:PutReplicationConfiguration", "ecr:SetRepositoryPolicy", "elasticfilesystem:PutFileSystemPolicy", "es:Create*Domain", "es:Update*DomainConfig", "events:PutPermission", "glacier:SetVaultAccessPolicy", "glue:PutResourcePolicy", "kinesis:PutResourcePolicy", "kms:PutKeyPolicy", "lambda:Add*Permission", "lambda:CreateFunctionUrlConfig", "lambda:UpdateFunctionUrlConfig", "logs:PutDe*Policy", "logs:PutResourcePolicy", "logs:PutSubscriptionFilter", "ram:*", "rds:ModifyDB*SnapshotAttribute", "s3:*BucketPolicy", "s3:Put*PublicAccessBlock", "s3:PutAccessPointPolicy*", "s3:PutBucketAcl", "s3:PutBucketOwnershipControls", "s3:PutMultiRegionAccessPointPolicy", "s3:PutReplicationConfiguration", "secretsmanager:PutResourcePolicy", "ses:*IdentityPolicy", "sns:AddPermission", "sns:CreateTopic", "sns:SetTopicAttributes", "sqs:AddPermission", "sqs:CreateQueue", "sqs:SetQueueAttributes"], Resource = "*" }, { Effect = "Deny", Action = ["kms:CreateGrant"], Resource = "*", Condition = { BoolIfExists = { "kms:GrantIsForAWSResource" = "false" } } }] })
}

resource "aws_iam_policy" "management_managed_policy_0" {
  name   = length(format("%s-%s", local.resource_prefix, "deployment-management-0")) <= 128 ? format("%s-%s", local.resource_prefix, "deployment-management-0") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "deployment-management-0"), 0, 119), substr(sha1(format("%s-%s", local.resource_prefix, "deployment-management-0")), 0, 8))
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ArtifactRegistryHeartbeatEcrDescribeRepositories", Effect = "Allow", Action = ["ecr:DescribeRepositories"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/*"] }, { Sid = "ArtifactRegistryHeartbeatReadEcrImages", Effect = "Allow", Action = ["ecr:DescribeImages", "ecr:ListImages", "ecr:DescribeImageScanFindings", "ecr:GetRepositoryPolicy", "ecr:ListTagsForResource"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${local.resource_prefix}-*"] }, { Sid = "ArtifactRegistryManagementEcrDescribeRepositories", Effect = "Allow", Action = ["ecr:DescribeRepositories"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/*"] }, { Sid = "ArtifactRegistryManagementReadEcrImages", Effect = "Allow", Action = ["ecr:DescribeImages", "ecr:ListImages", "ecr:DescribeImageScanFindings", "ecr:GetRepositoryPolicy", "ecr:ListTagsForResource"], Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${local.resource_prefix}-*"] }, { Sid = "BoundedRoleWrite3", Effect = "Allow", Action = ["iam:UpdateAssumeRolePolicy", "iam:PutRolePolicy", "iam:DeleteRolePolicy", "iam:DetachRolePolicy", "iam:PutRolePermissionsBoundary"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime", "iam:PermissionsBoundary" = "${aws_iam_policy.management_permissions_boundary.arn}" } } }, { Sid = "ArtifactRegistryManagementIamGetRolePermissions", Effect = "Allow", Action = ["iam:GetRole", "iam:GetRolePolicy", "iam:ListRolePolicies", "iam:ListAttachedRolePolicies"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"] }, { Sid = "BoundedRoleDelete4", Effect = "Allow", Action = ["iam:DeleteRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "BuildHeartbeat", Effect = "Allow", Action = ["codebuild:BatchGetProjects", "codebuild:ListBuildsForProject", "codebuild:ListCuratedEnvironmentImages", "codebuild:GetResourcePolicy"], Resource = ["arn:aws:codebuild:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:project/${local.resource_prefix}-*"] }, { Sid = "ComputeClusterHeartbeat", Effect = "Allow", Action = ["autoscaling:DescribeAutoScalingGroups", "autoscaling:DescribeScalingActivities", "ec2:DescribeInstances", "ec2:DescribeInstanceStatus", "ec2:DescribeTags"], Resource = ["*"] }, { Sid = "ComputeClusterManagementServicequotasGetServiceQuotaPermissions", Effect = "Allow", Action = ["servicequotas:GetServiceQuota", "ec2:DescribeInstanceTypes"], Resource = ["*"] }, { Sid = "ComputeClusterManagementInspectCloudNetworking", Effect = "Allow", Action = ["ec2:DescribeLaunchTemplates", "ec2:DescribeSecurityGroups"], Resource = ["*"] }, { Sid = "ComputeClusterManagementEc2CreateLaunchTemplate", Effect = "Allow", Action = ["ec2:CreateLaunchTemplate"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:launch-template/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ComputeClusterManagementEc2CreateTags", Effect = "Allow", Action = ["ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:launch-template/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "ec2:CreateAction" = "CreateLaunchTemplate" } } }, { Sid = "ComputeClusterManagementInspectCloudNetworking2", Effect = "Allow", Action = ["ec2:CreateLaunchTemplateVersion", "ec2:DeleteLaunchTemplate", "ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:launch-template/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "UseRuntimeLaunchTemplates", Effect = "Allow", Action = ["ec2:RunInstances"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:launch-template/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "UseSetupComputeSecurityGroups", Effect = "Allow", Action = ["ec2:RunInstances"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:security-group/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "setup", "aws:ResourceTag/resource-type" = "compute-cluster" } } }] })
}

resource "aws_iam_role_policy_attachment" "management_managed_policy_0_attachment" {
  role       = aws_iam_role.management.name
  policy_arn = aws_iam_policy.management_managed_policy_0.arn
}

resource "aws_iam_policy" "management_managed_policy_1" {
  name   = length(format("%s-%s", local.resource_prefix, "deployment-management-1")) <= 128 ? format("%s-%s", local.resource_prefix, "deployment-management-1") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "deployment-management-1"), 0, 119), substr(sha1(format("%s-%s", local.resource_prefix, "deployment-management-1")), 0, 8))
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ComputeClusterManagementEc2RunInstances", Effect = "Allow", Action = ["ec2:RunInstances"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:instance/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ComputeClusterManagementEc2RunInstances2", Effect = "Allow", Action = ["ec2:RunInstances"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}::image/*", "arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:subnet/*", "arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:network-interface/*", "arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"] }, { Sid = "ComputeClusterManagementEc2CreateTags2", Effect = "Allow", Action = ["ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:instance/*", "arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:network-interface/*", "arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "ec2:CreateAction" = "RunInstances" } } }, { Sid = "ComputeClusterManagementIamCreateServiceLinkedRole", Effect = "Allow", Action = ["iam:CreateServiceLinkedRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/aws-service-role/autoscaling.amazonaws.com/AWSServiceRoleForAutoScaling"], Condition = { StringEquals = { "iam:AWSServiceName" = "autoscaling.amazonaws.com" } } }, { Sid = "BoundedRoleAsgCreate", Effect = "Allow", Action = ["autoscaling:CreateAutoScalingGroup"], Resource = ["arn:aws:autoscaling:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:autoScalingGroup:*:autoScalingGroupName/${local.resource_prefix}-*"], Condition = { Bool = { "autoscaling:LaunchTemplateVersionSpecified" = "true" }, Null = { "autoscaling:LaunchConfigurationName" = "true" }, StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "BoundedRoleAsgUpdate", Effect = "Allow", Action = ["autoscaling:UpdateAutoScalingGroup"], Resource = ["arn:aws:autoscaling:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:autoScalingGroup:*:autoScalingGroupName/${local.resource_prefix}-*"], Condition = { BoolIfExists = { "autoscaling:LaunchTemplateVersionSpecified" = "true" }, Null = { "autoscaling:LaunchConfigurationName" = "true" }, StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "BoundedRoleAsgManagement", Effect = "Allow", Action = ["autoscaling:DeleteAutoScalingGroup", "autoscaling:SetDesiredCapacity", "autoscaling:TerminateInstanceInAutoScalingGroup"], Resource = ["arn:aws:autoscaling:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:autoScalingGroup:*:autoScalingGroupName/${local.resource_prefix}-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ComputeClusterManagementElasticloadbalancingCreateTargetGroupPermissions", Effect = "Allow", Action = ["elasticloadbalancing:CreateTargetGroup", "elasticloadbalancing:AddTags"], Resource = ["arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:targetgroup/${local.resource_prefix}-*/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ComputeClusterManagementElasticloadbalancingModifyTargetGroupAttributes", Effect = "Allow", Action = ["elasticloadbalancing:ModifyTargetGroupAttributes"], Resource = ["arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:targetgroup/${local.resource_prefix}-*/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ComputeClusterManagementElasticloadbalancingDeleteTargetGroup", Effect = "Allow", Action = ["elasticloadbalancing:DeleteTargetGroup"], Resource = ["arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:targetgroup/${local.resource_prefix}-*/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ComputeClusterManagementIamGetInstanceProfile", Effect = "Allow", Action = ["iam:GetInstanceProfile"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:instance-profile/*"] }, { Sid = "ComputeClusterManagementIamGetRole", Effect = "Allow", Action = ["iam:GetRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"] }] })
}

resource "aws_iam_role_policy_attachment" "management_managed_policy_1_attachment" {
  role       = aws_iam_role.management.name
  policy_arn = aws_iam_policy.management_managed_policy_1.arn
}

resource "aws_iam_policy" "management_managed_policy_2" {
  name   = length(format("%s-%s", local.resource_prefix, "deployment-management-2")) <= 128 ? format("%s-%s", local.resource_prefix, "deployment-management-2") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "deployment-management-2"), 0, 119), substr(sha1(format("%s-%s", local.resource_prefix, "deployment-management-2")), 0, 8))
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "BoundedRolePass", Effect = "Allow", Action = ["iam:PassRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"], Condition = { StringEquals = { "iam:PassedToService" = "ec2.amazonaws.com" } } }, { Sid = "ComputeClusterManagementAutoscalingDescribeAutoScalingGroupsPermissions", Effect = "Allow", Action = ["autoscaling:DescribeAutoScalingGroups", "autoscaling:DescribeInstanceRefreshes", "autoscaling:DescribeScalingActivities", "ec2:DescribeInstanceTypeOfferings", "ec2:DescribeInstances", "ec2:DescribeInstanceStatus", "ec2:DescribeTags"], Resource = ["*"] }, { Sid = "ComputeClusterManagementEc2GetConsoleOutput", Effect = "Allow", Action = ["ec2:GetConsoleOutput"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:instance/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerHeartbeat", Effect = "Allow", Action = ["elasticloadbalancing:DescribeTargetHealth", "ec2:DescribeVolumes"], Resource = ["*"] }, { Sid = "ContainerProvisionElasticloadbalancingCreateLoadBalancerPermissions", Effect = "Allow", Action = ["elasticloadbalancing:CreateLoadBalancer", "elasticloadbalancing:CreateTargetGroup", "elasticloadbalancing:CreateListener"], Resource = ["*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionElasticloadbalancingAddTags", Effect = "Allow", Action = ["elasticloadbalancing:AddTags"], Resource = ["*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" }, StringLike = { "elasticloadbalancing:CreateAction" = "Create*" } } }, { Sid = "ContainerProvisionElasticloadbalancingModifyListenerPermissions", Effect = "Allow", Action = ["elasticloadbalancing:ModifyListener", "elasticloadbalancing:ModifyLoadBalancerAttributes", "elasticloadbalancing:ModifyTargetGroupAttributes", "elasticloadbalancing:DeleteLoadBalancer", "elasticloadbalancing:DeleteTargetGroup", "elasticloadbalancing:DeleteListener"], Resource = ["arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:loadbalancer/app/*/*", "arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:loadbalancer/net/*/*", "arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:targetgroup/*/*", "arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:listener/app/*/*/*", "arn:aws:elasticloadbalancing:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:listener/net/*/*/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2CreateVolume", Effect = "Allow", Action = ["ec2:CreateVolume"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2CreateTags", Effect = "Allow", Action = ["ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "ec2:CreateAction" = "CreateVolume" } } }, { Sid = "ContainerProvisionEc2CreateTags2", Effect = "Allow", Action = ["ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionInspectCloudNetworking", Effect = "Allow", Action = ["ec2:DeleteVolume", "ec2:ModifyVolume"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2DetachVolume", Effect = "Allow", Action = ["ec2:DetachVolume"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2DetachVolume2", Effect = "Allow", Action = ["ec2:DetachVolume"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:instance/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }] })
}

resource "aws_iam_role_policy_attachment" "management_managed_policy_2_attachment" {
  role       = aws_iam_role.management.name
  policy_arn = aws_iam_policy.management_managed_policy_2.arn
}

resource "aws_iam_policy" "management_managed_policy_3" {
  name   = length(format("%s-%s", local.resource_prefix, "deployment-management-3")) <= 128 ? format("%s-%s", local.resource_prefix, "deployment-management-3") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "deployment-management-3"), 0, 119), substr(sha1(format("%s-%s", local.resource_prefix, "deployment-management-3")), 0, 8))
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ContainerProvisionEc2CreateSnapshot", Effect = "Allow", Action = ["ec2:CreateSnapshot"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2CreateSnapshot2", Effect = "Allow", Action = ["ec2:CreateSnapshot"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}::snapshot/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2CreateTags3", Effect = "Allow", Action = ["ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}::snapshot/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "ec2:CreateAction" = "CreateSnapshot" } } }, { Sid = "ContainerProvisionEc2DeleteSnapshot", Effect = "Allow", Action = ["ec2:DeleteSnapshot"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}::snapshot/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2CreateVolume2", Effect = "Allow", Action = ["ec2:CreateVolume"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}::snapshot/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionEc2DeleteTags", Effect = "Allow", Action = ["ec2:DeleteTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:volume/*"], Condition = { "ForAllValues:StringEquals" = { "aws:TagKeys" = "deployment-restore-request" }, StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionDlmCreateLifecyclePolicy", Effect = "Allow", Action = ["dlm:CreateLifecyclePolicy"], Resource = ["*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionDlmTagResourcePermissions", Effect = "Allow", Action = ["dlm:TagResource", "dlm:GetLifecyclePolicy", "dlm:UpdateLifecyclePolicy", "dlm:DeleteLifecyclePolicy"], Resource = ["arn:aws:dlm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:policy/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionIamGetRole", Effect = "Allow", Action = ["iam:GetRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-volume-backups-*"] }, { Sid = "ContainerProvisionIamPassRole", Effect = "Allow", Action = ["iam:PassRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-volume-backups-*"], Condition = { StringEquals = { "iam:PassedToService" = "dlm.amazonaws.com" } } }, { Sid = "DenyPassingLegacyVolumeBackupRoles", Effect = "Deny", Action = ["iam:PassRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-dlm-*"] }, { Sid = "ContainerProvisionIamGetRole2", Effect = "Allow", Action = ["iam:GetRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-dlm-*"] }, { Sid = "BoundedRoleDelete24", Effect = "Allow", Action = ["iam:DeleteRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-dlm-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "BoundedRoleWrite25", Effect = "Allow", Action = ["iam:DetachRolePolicy"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-dlm-*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime", "iam:PolicyARN" = "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole" } } }, { Sid = "ContainerProvisionEc2DescribeVolumesPermissions", Effect = "Allow", Action = ["ec2:DescribeVolumes", "ec2:DescribeVolumesModifications", "ec2:DescribeSnapshots", "dlm:GetLifecyclePolicies", "elasticloadbalancing:DescribeLoadBalancers", "elasticloadbalancing:DescribeTargetGroups", "elasticloadbalancing:DescribeTargetHealth", "elasticloadbalancing:DescribeListeners", "elasticloadbalancing:DescribeTags"], Resource = ["*"] }, { Sid = "ContainerProvisionEc2CreateSecurityGroup", Effect = "Allow", Action = ["ec2:CreateSecurityGroup"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:security-group/*", "arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:vpc/*"] }] })
}

resource "aws_iam_role_policy_attachment" "management_managed_policy_3_attachment" {
  role       = aws_iam_role.management.name
  policy_arn = aws_iam_policy.management_managed_policy_3.arn
}

resource "aws_iam_policy" "management_managed_policy_4" {
  name   = length(format("%s-%s", local.resource_prefix, "deployment-management-4")) <= 128 ? format("%s-%s", local.resource_prefix, "deployment-management-4") : format("%s-%s", substr(format("%s-%s", local.resource_prefix, "deployment-management-4"), 0, 119), substr(sha1(format("%s-%s", local.resource_prefix, "deployment-management-4")), 0, 8))
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ContainerProvisionEc2CreateTags4", Effect = "Allow", Action = ["ec2:CreateTags"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:security-group/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime", "ec2:CreateAction" = "CreateSecurityGroup" } } }, { Sid = "ContainerProvisionInspectCloudNetworking2", Effect = "Allow", Action = ["ec2:DeleteSecurityGroup", "ec2:AuthorizeSecurityGroupIngress", "ec2:RevokeSecurityGroupIngress", "ec2:RevokeSecurityGroupEgress"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:security-group/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ManageSetupComputeSecurityGroupIngress", Effect = "Allow", Action = ["ec2:AuthorizeSecurityGroupIngress", "ec2:RevokeSecurityGroupIngress"], Resource = ["arn:aws:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:security-group/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "setup", "aws:ResourceTag/resource-type" = "compute-cluster" } } }, { Sid = "ContainerProvisionInspectCloudNetworking3", Effect = "Allow", Action = ["ec2:DescribeSecurityGroups", "ec2:DescribeVpcs"], Resource = ["*"] }, { Sid = "ContainerProvisionIamCreateServiceLinkedRole", Effect = "Allow", Action = ["iam:CreateServiceLinkedRole"], Resource = ["*"], Condition = { StringEquals = { "iam:AWSServiceName" = "elasticloadbalancing.amazonaws.com" } } }, { Sid = "ContainerProvisionAcmImportCertificate", Effect = "Allow", Action = ["acm:ImportCertificate"], Resource = ["arn:aws:acm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:certificate/*"], Condition = { StringEquals = { "aws:RequestTag/deployment" = "${local.resource_prefix}", "aws:RequestTag/managed-by" = "runtime" } } }, { Sid = "ContainerProvisionManageTlsCertificates", Effect = "Allow", Action = ["acm:AddTagsToCertificate", "acm:DeleteCertificate", "acm:DescribeCertificate", "acm:ListTagsForCertificate"], Resource = ["arn:aws:acm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:certificate/*"], Condition = { StringEquals = { "aws:ResourceTag/deployment" = "${local.resource_prefix}", "aws:ResourceTag/managed-by" = "runtime" } } }, { Sid = "ListTlsCertificates", Effect = "Allow", Action = ["acm:ListCertificates"], Resource = ["*"] }, { Sid = "CheckDynamodbTableHealth", Effect = "Allow", Action = ["dynamodb:DescribeTable", "dynamodb:ListTables", "dynamodb:DescribeTimeToLive", "dynamodb:ListTagsOfResource", "dynamodb:DescribeContinuousBackups"], Resource = ["arn:aws:dynamodb:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:table/${local.resource_prefix}-*"] }, { Sid = "NetworkHeartbeat", Effect = "Allow", Action = ["ec2:DescribeVpcs", "ec2:DescribeSubnets", "ec2:DescribeSecurityGroups", "ec2:DescribeRouteTables", "ec2:DescribeInternetGateways", "ec2:DescribeNatGateways", "ec2:DescribeAddresses", "ec2:DescribeAvailabilityZones"], Resource = ["*"] }, { Sid = "CheckSqsQueueHealth", Effect = "Allow", Action = ["sqs:GetQueueAttributes", "sqs:ListQueueTags"], Resource = ["arn:aws:sqs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:${local.resource_prefix}-*"] }, { Sid = "ServiceAccountHeartbeat", Effect = "Allow", Action = ["iam:GetRole", "iam:ListAttachedRolePolicies", "iam:ListRolePolicies"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"] }, { Sid = "CheckS3BucketHealth", Effect = "Allow", Action = ["s3:GetBucketLocation", "s3:GetBucketVersioning", "s3:GetLifecycleConfiguration", "s3:GetBucketPolicy", "s3:GetBucketAcl", "s3:GetEncryptionConfiguration", "s3:GetBucketPublicAccessBlock"], Resource = ["arn:aws:s3:::${local.resource_prefix}-*"] }, { Sid = "VaultHeartbeat", Effect = "Allow", Action = ["ssm:DescribeParameters"], Resource = ["*"] }, { Sid = "BoundedRoleProtectBoundary", Effect = "Deny", Action = ["iam:CreatePolicyVersion", "iam:SetDefaultPolicyVersion", "iam:DeletePolicyVersion", "iam:DeletePolicy", "iam:TagPolicy", "iam:UntagPolicy", "iam:DeleteRolePermissionsBoundary"], Resource = ["${aws_iam_policy.management_permissions_boundary.arn}", "arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-*"] }, { Sid = "DenyManagementRoleChanges", Effect = "Deny", Action = ["iam:DeleteRole", "iam:PutRolePolicy", "iam:DeleteRolePolicy", "iam:AttachRolePolicy", "iam:DetachRolePolicy", "iam:UpdateAssumeRolePolicy", "iam:PutRolePermissionsBoundary", "iam:DeleteRolePermissionsBoundary", "iam:UpdateRole", "iam:TagRole", "iam:UntagRole"], Resource = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${local.resource_prefix}-management"] }] })
}

resource "aws_iam_role_policy_attachment" "management_managed_policy_4_attachment" {
  role       = aws_iam_role.management.name
  policy_arn = aws_iam_policy.management_managed_policy_4.arn
}

resource "aws_iam_role_policy" "management_self_read" {
  name   = "deployment-management-self-read"
  role   = aws_iam_role.management.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ReadOwnManagementRole", Effect = "Allow", Action = "iam:GetRole", Resource = aws_iam_role.management.arn }] })
}

resource "aws_iam_role_policy" "registry_pull_policy" {
  name   = "ecr-pull"
  role   = aws_iam_role.registry_pull.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "GetAuthorizationToken", Effect = "Allow", Action = "ecr:GetAuthorizationToken", Resource = "*" }, { Sid = "RepositoryAccess", Effect = "Allow", Action = ["ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:DescribeImages", "ecr:DescribeRepositories", "ecr:GetDownloadUrlForLayer", "ecr:ListImages"], Resource = [aws_ecr_repository.registry.arn, format("%s-*", aws_ecr_repository.registry.arn)] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "registry_push_policy" {
  name   = "ecr-push-pull"
  role   = aws_iam_role.registry_push.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "GetAuthorizationToken", Effect = "Allow", Action = "ecr:GetAuthorizationToken", Resource = "*" }, { Sid = "RepositoryAccess", Effect = "Allow", Action = ["ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:DescribeImages", "ecr:DescribeRepositories", "ecr:GetDownloadUrlForLayer", "ecr:ListImages", "ecr:CompleteLayerUpload", "ecr:CreateRepository", "ecr:DeleteRepository", "ecr:InitiateLayerUpload", "ecr:PutImage", "ecr:UploadLayerPart"], Resource = [aws_ecr_repository.registry.arn, format("%s-*", aws_ecr_repository.registry.arn)] }, { Sid = "RegionalRepositoryPolicies", Effect = "Allow", Action = ["ecr:GetRepositoryPolicy", "ecr:DeleteRepositoryPolicy", "ecr:SetRepositoryPolicy", "ecr:DescribeRepositories"], Resource = [replace(aws_ecr_repository.registry.arn, format(":%s:", data.aws_region.current.region), ":*:"), format("%s-*", replace(aws_ecr_repository.registry.arn, format(":%s:", data.aws_region.current.region), ":*:"))] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_vault_test_vault_set_0" {
  name   = "access-test-vault-vault-data-read"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "VaultDataRead", Effect = "Allow", Action = ["ssm:GetParameter"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-test-vault-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_vault_test_vault_set_1" {
  name   = "access-test-vault-vault-data-write"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "WriteSsmParameters", Effect = "Allow", Action = ["ssm:PutParameter", "ssm:DeleteParameter"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-test-vault-*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "execution_sa_vault_test_vault_set_2" {
  name   = "access-test-vault-vault-protect-unmapped-deployer-secrets"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "VaultProtectUnmappedDeployerSecretsSsmPutParameterPermissions", Effect = "Deny", Action = ["ssm:PutParameter", "ssm:DeleteParameter*", "ssm:LabelParameterVersion", "ssm:UnlabelParameterVersion", "ssm:AddTagsToResource", "ssm:RemoveTagsFromResource", "ssm:PutResourcePolicy", "ssm:DeleteResourcePolicy"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-deployer-secrets-*"] }, { Sid = "VaultProtectUnmappedDeployerSecretsSsmGetParametersByPath", Effect = "Deny", Action = ["ssm:GetParametersByPath"], Resource = ["arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}

resource "aws_iam_role_policy" "management_vault_test_vault_set_0" {
  name   = "management-test-vault-vault-heartbeat"
  role   = aws_iam_role.management.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "VaultHeartbeat", Effect = "Allow", Action = ["ssm:DescribeParameters"], Resource = ["*"] }] })
  depends_on = [
    aws_iam_role.management
  ]
}
