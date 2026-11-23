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

# The network settings for `alien deploy`, as JSON.
output "network" {
  value = jsonencode({
    type               = "byo-vpc-aws"
    vpc_id             = aws_vpc.this.id
    public_subnet_ids  = aws_subnet.public[*].id
    private_subnet_ids = aws_subnet.private[*].id
    security_group_ids = [aws_security_group.stack.id]
  })
}
