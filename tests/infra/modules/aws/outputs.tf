output "management_access_key_id" {
  value     = aws_iam_access_key.manager.id
  sensitive = true
}

output "management_secret_access_key" {
  value     = aws_iam_access_key.manager.secret
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
  value = data.aws_caller_identity.target.account_id
}

output "s3_bucket" {
  value = aws_s3_bucket.test.bucket
}

output "lambda_image_uri" {
  value = "${aws_ecr_repository.lambda_test.repository_url}:latest"
}

output "lambda_execution_role_arn" {
  value = aws_iam_role.lambda_execution.arn
}
