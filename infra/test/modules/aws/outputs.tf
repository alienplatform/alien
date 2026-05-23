output "management_access_key_id" {
  value     = aws_iam_access_key.manager.id
  sensitive = true
}

output "management_secret_access_key" {
  value     = aws_iam_access_key.manager.secret
  sensitive = true
}

output "management_role_arn" {
  value     = aws_iam_role.management.arn
  sensitive = true
}

output "management_role_name" {
  value     = aws_iam_role.management.name
  sensitive = true
}

output "target_access_key_id" {
  value     = aws_iam_access_key.target.id
  sensitive = true
}

output "target_secret_access_key" {
  value     = aws_iam_access_key.target.secret
  sensitive = true
}

output "target_account_id" {
  value     = data.aws_caller_identity.target.account_id
  sensitive = true
}

output "e2e_vpc_id" {
  value     = aws_vpc.e2e.id
  sensitive = true
}

output "e2e_public_subnet_ids" {
  value     = aws_subnet.e2e_public[*].id
  sensitive = true
}

output "e2e_private_subnet_ids" {
  value     = aws_subnet.e2e_private[*].id
  sensitive = true
}

output "e2e_security_group_ids" {
  value     = [aws_security_group.e2e.id]
  sensitive = true
}

output "s3_bucket" {
  value     = aws_s3_bucket.test.bucket
  sensitive = true
}

output "lambda_image_uri" {
  value     = "${aws_ecr_repository.lambda_test.repository_url}:latest"
  sensitive = true
}

output "lambda_execution_role_arn" {
  value     = aws_iam_role.lambda_execution.arn
  sensitive = true
}

output "ecr_push_role_arn" {
  value     = aws_iam_role.ecr_push.arn
  sensitive = true
}

output "ecr_pull_role_arn" {
  value     = aws_iam_role.ecr_pull.arn
  sensitive = true
}

output "command_kv_table_name" {
  value     = aws_dynamodb_table.command_kv.name
  sensitive = true
}

output "command_kv_table_arn" {
  value     = aws_dynamodb_table.command_kv.arn
  sensitive = true
}

# E2E artifact registry (matches alien-infra controller pattern)
output "e2e_ar_push_role_arn" {
  value     = aws_iam_role.e2e_ar_push.arn
  sensitive = true
}

output "e2e_ar_pull_role_arn" {
  value     = aws_iam_role.e2e_ar_pull.arn
  sensitive = true
}

output "e2e_eks_cluster_name" {
  value     = aws_eks_cluster.e2e.name
  sensitive = true
}

output "e2e_eks_kube_context" {
  value     = aws_eks_cluster.e2e.name
  sensitive = true
}

output "e2e_eks_kubeconfig" {
  value = yamlencode({
    apiVersion = "v1"
    kind       = "Config"
    clusters = [{
      name = aws_eks_cluster.e2e.name
      cluster = {
        server                       = aws_eks_cluster.e2e.endpoint
        "certificate-authority-data" = aws_eks_cluster.e2e.certificate_authority[0].data
      }
    }]
    contexts = [{
      name = aws_eks_cluster.e2e.name
      context = {
        cluster = aws_eks_cluster.e2e.name
        user    = aws_eks_cluster.e2e.name
      }
    }]
    "current-context" = aws_eks_cluster.e2e.name
    users = [{
      name = aws_eks_cluster.e2e.name
      user = {
        exec = {
          apiVersion = "client.authentication.k8s.io/v1beta1"
          command    = "aws"
          args       = ["eks", "get-token", "--cluster-name", aws_eks_cluster.e2e.name, "--region", var.target_region]
          env = [
            { name = "AWS_ACCESS_KEY_ID", value = aws_iam_access_key.target.id },
            { name = "AWS_SECRET_ACCESS_KEY", value = aws_iam_access_key.target.secret },
          ]
        }
      }
    }]
  })
  sensitive = true
}

output "e2e_k8s_public_host_suffix" {
  value     = "${aws_eip.e2e_ingress[0].public_ip}.sslip.io"
  sensitive = true
}
