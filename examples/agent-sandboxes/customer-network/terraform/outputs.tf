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

output "sandbox_subnet_ids" {
  value = aws_subnet.sandbox[*].id
}

output "sandbox_gateway_subnet_ids" {
  value = aws_subnet.sandbox_gateway[*].id
}

output "sandbox_gateway_subnet_cidrs" {
  value = aws_subnet.sandbox_gateway[*].cidr_block
}

# The network values the stack's Terraform setup asks for: the variables of the rendered module.
output "terraform_setup_values" {
  value = {
    network_mode               = "use-existing"
    vpc_id                     = aws_vpc.this.id
    public_subnet_ids          = aws_subnet.public[*].id
    private_subnet_ids         = aws_subnet.private[*].id
    security_group_ids         = [aws_security_group.stack.id]
    sandbox_subnet_ids         = aws_subnet.sandbox[*].id
    sandbox_gateway_subnet_ids = aws_subnet.sandbox_gateway[*].id
  }
}

# The same for the CloudFormation setup, as `--parameter-overrides` arguments. Lists are
# comma-separated.
output "cloudformation_parameters" {
  value = join(" ", [
    "NetworkMode=use-existing",
    "VpcId=${aws_vpc.this.id}",
    "PublicSubnetIds=${join(",", aws_subnet.public[*].id)}",
    "PrivateSubnetIds=${join(",", aws_subnet.private[*].id)}",
    "SecurityGroupIds=${aws_security_group.stack.id}",
    "SandboxSubnetIds=${join(",", aws_subnet.sandbox[*].id)}",
    "SandboxGatewaySubnetIds=${join(",", aws_subnet.sandbox_gateway[*].id)}",
    "SandboxGatewaySubnetCidrs=${join(",", aws_subnet.sandbox_gateway[*].cidr_block)}",
  ])
}
