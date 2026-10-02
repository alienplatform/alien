output "url" {
  description = "Manager URL"
  value       = local.base_url
}

output "admin_token_secret_arn" {
  description = "Secrets Manager secret holding the admin API key"
  value       = aws_secretsmanager_secret.admin_token.arn
}

output "login_command" {
  description = "Connect the CLI to this manager"
  value       = "alien login --manager ${local.base_url} --token \"$(aws secretsmanager get-secret-value --secret-id ${aws_secretsmanager_secret.admin_token.arn} --query SecretString --output text)\""
}

output "artifact_repository_url" {
  description = "ECR repository release images are stored in"
  value       = var.create_artifact_repository ? aws_ecr_repository.releases[0].repository_url : null
}

output "load_balancer_dns_name" {
  description = "Point your DNS name (domain_name) here"
  value       = aws_lb.manager.dns_name
}
