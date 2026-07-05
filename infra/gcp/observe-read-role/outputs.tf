output "project_id" {
  description = "GCP project ID where observe read access was granted."
  value       = var.project_id
}

output "member" {
  description = "IAM member granted observe read access."
  value       = var.member
}

output "roles" {
  description = "Project IAM roles granted to the observe identity."
  value       = sort(var.roles)
}
