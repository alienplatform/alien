output "role_arn" {
  description = "ARN of the observe read role."
  value       = aws_iam_role.this.arn
}

output "role_name" {
  description = "Name of the observe read role."
  value       = aws_iam_role.this.name
}

output "policy_arn" {
  description = "ARN of the attached observe read policy."
  value       = aws_iam_policy.read.arn
}
