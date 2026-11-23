output "management_role_arn" {
  value     = aws_iam_role.management.arn
  sensitive = true
}

output "management_role_name" {
  value     = aws_iam_role.management.name
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

output "e2e_terraform_state_bucket" {
  value     = local.e2e_terraform_state_name
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

output "e2e_k8s_public_host_suffix" {
  value     = "${aws_eip.e2e_ingress[0].public_ip}.sslip.io"
  sensitive = true
}

output "e2e_ingress_eip_allocation_ids" {
  value     = aws_eip.e2e_ingress[*].id
  sensitive = true
}

output "manager_ci_role_arn" {
  value     = aws_iam_role.manager_ci.arn
  sensitive = true
}

output "target_ci_role_arn" {
  value     = aws_iam_role.target_ci.arn
  sensitive = true
}
