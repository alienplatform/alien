output "vpc_id" {
  value = aws_vpc.this.id
}

output "public_subnet_ids" {
  value = aws_subnet.public[*].id
}

output "private_subnet_ids" {
  value = aws_subnet.private[*].id
}

output "security_group_id" {
  value = aws_security_group.stack.id
}

output "internal_api_url" {
  description = "The connector's internalApiUrl input."
  value       = "http://${local.internal_api_ip}:8080"
}

output "scanner_url" {
  description = "The connector's scannerUrl input."
  value       = "http://${local.internal_api_ip}:8081"
}

# Everything `alien deploy` needs about this network, in the form its flags take: comma-separated
# subnet ids and the two inputs. Use it as `alien deploy ... $(terraform output -raw deploy_flags)`.
output "deploy_flags" {
  value = join(" ", [
    "--network byo",
    "--vpc-id ${aws_vpc.this.id}",
    "--public-subnet-ids ${join(",", aws_subnet.public[*].id)}",
    "--private-subnet-ids ${join(",", aws_subnet.private[*].id)}",
    "--security-group-ids ${aws_security_group.stack.id}",
    "--input internalApiUrl=http://${local.internal_api_ip}:8080",
    "--input scannerUrl=http://${local.internal_api_ip}:8081",
  ])
}
