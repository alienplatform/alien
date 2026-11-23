# The customer's existing network: a VPC with private subnets behind one NAT gateway, and an
# internal API on a private address. The agent-sandboxes stack is deployed into this VPC with
# `byo-vpc-aws`; nothing here knows about it.

data "aws_availability_zones" "available" {
  state = "available"
}

locals {
  azs = slice(data.aws_availability_zones.available.names, 0, 2)
  # A fixed private address, so the README can show the sandbox being refused at it.
  internal_api_ip = cidrhost(cidrsubnet(var.vpc_cidr, 8, 10), 50)
  tags            = { Name = var.name_prefix }
}

resource "aws_vpc" "this" {
  cidr_block           = var.vpc_cidr
  enable_dns_support   = true
  enable_dns_hostnames = true
  tags                 = local.tags
}

resource "aws_internet_gateway" "this" {
  vpc_id = aws_vpc.this.id
  tags   = local.tags
}

resource "aws_subnet" "public" {
  count             = 2
  vpc_id            = aws_vpc.this.id
  availability_zone = local.azs[count.index]
  cidr_block        = cidrsubnet(var.vpc_cidr, 8, count.index)
  tags              = { Name = "${var.name_prefix}-public-${count.index}" }
}

resource "aws_subnet" "private" {
  count             = 2
  vpc_id            = aws_vpc.this.id
  availability_zone = local.azs[count.index]
  cidr_block        = cidrsubnet(var.vpc_cidr, 8, 10 + count.index)
  tags              = { Name = "${var.name_prefix}-private-${count.index}" }
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.this.id
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.this.id
  }
  tags = { Name = "${var.name_prefix}-public" }
}

resource "aws_route_table_association" "public" {
  count          = 2
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

# One NAT gateway (one Elastic IP) for both private subnets.
resource "aws_eip" "nat" {
  domain = "vpc"
  tags   = local.tags
}

resource "aws_nat_gateway" "this" {
  allocation_id = aws_eip.nat.id
  subnet_id     = aws_subnet.public[0].id
  tags          = local.tags
  depends_on    = [aws_internet_gateway.this]
}

resource "aws_route_table" "private" {
  vpc_id = aws_vpc.this.id
  route {
    cidr_block     = "0.0.0.0/0"
    nat_gateway_id = aws_nat_gateway.this.id
  }
  tags = { Name = "${var.name_prefix}-private" }
}

resource "aws_route_table_association" "private" {
  count          = 2
  subnet_id      = aws_subnet.private[count.index].id
  route_table_id = aws_route_table.private.id
}

# For the stack's workloads: all outbound, inbound only from itself.
resource "aws_security_group" "stack" {
  name        = "${var.name_prefix}-stack"
  description = "agent-sandboxes stack workloads"
  vpc_id      = aws_vpc.this.id
  ingress {
    from_port = 0
    to_port   = 0
    protocol  = "-1"
    self      = true
  }
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# The internal API: one small instance in a private subnet, no public address, reachable on
# 8080 (internal API) and 8081 (scanner) from inside the VPC only.
resource "aws_security_group" "internal_api" {
  name        = "${var.name_prefix}-internal-api"
  description = "internal API, VPC only"
  vpc_id      = aws_vpc.this.id
  ingress {
    from_port   = 8080
    to_port     = 8081
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr]
  }
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

data "aws_ssm_parameter" "al2023_arm64" {
  name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-arm64"
}

# The scanner's key lives in the customer's own secret store, not in the instance's user data.
# The instance reads it at boot with its own role.
resource "aws_ssm_parameter" "scanner_api_key" {
  name  = "/${var.name_prefix}/scanner-api-key"
  type  = "SecureString"
  value = var.scanner_api_key
}

data "aws_kms_alias" "ssm" {
  name = "alias/aws/ssm"
}

resource "aws_iam_role" "internal_api" {
  name = "${var.name_prefix}-internal-api"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "ec2.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy" "read_scanner_key" {
  name = "read-scanner-key"
  role = aws_iam_role.internal_api.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      { Effect = "Allow", Action = "ssm:GetParameter", Resource = aws_ssm_parameter.scanner_api_key.arn },
      { Effect = "Allow", Action = "kms:Decrypt", Resource = data.aws_kms_alias.ssm.target_key_arn },
    ]
  })
}

resource "aws_iam_instance_profile" "internal_api" {
  name = "${var.name_prefix}-internal-api"
  role = aws_iam_role.internal_api.name
}

resource "aws_instance" "internal_api" {
  ami                         = data.aws_ssm_parameter.al2023_arm64.value
  instance_type               = "t4g.nano"
  subnet_id                   = aws_subnet.private[0].id
  private_ip                  = local.internal_api_ip
  associate_public_ip_address = false
  iam_instance_profile        = aws_iam_instance_profile.internal_api.name
  vpc_security_group_ids      = [aws_security_group.internal_api.id]
  metadata_options {
    http_tokens = "required"
  }
  user_data_replace_on_change = true
  user_data                   = <<-EOT
    #!/bin/bash
    set -euo pipefail
    dnf install -y nodejs
    mkdir -p /opt/internal-api
    echo '${base64encode(file("${path.module}/../internal-api/server.mjs"))}' | base64 -d > /opt/internal-api/server.mjs
    cat > /etc/systemd/system/internal-api.service <<'UNIT'
    [Unit]
    Description=internal API
    After=network-online.target
    [Service]
    ExecStart=/usr/bin/node /opt/internal-api/server.mjs
    Restart=always
    DynamicUser=yes
    [Install]
    WantedBy=multi-user.target
    UNIT
    mkdir -p /opt/scanner
    echo '${base64encode(file("${path.module}/../scanner/server.mjs"))}' | base64 -d > /opt/scanner/server.mjs
    # The scanner's key, from the customer's secret store, into a file only root (systemd) reads.
    umask 077
    printf 'SCANNER_API_KEY=%s\n' "$(aws ssm get-parameter --region ${var.region} --name ${aws_ssm_parameter.scanner_api_key.name} --with-decryption --query Parameter.Value --output text)" > /etc/scanner.env
    cat > /etc/systemd/system/scanner.service <<'UNIT'
    [Unit]
    Description=scanner
    After=network-online.target
    [Service]
    ExecStart=/usr/bin/node /opt/scanner/server.mjs
    EnvironmentFile=/etc/scanner.env
    Environment=PORT=8081
    Restart=always
    DynamicUser=yes
    [Install]
    WantedBy=multi-user.target
    UNIT
    systemctl daemon-reload
    systemctl enable --now internal-api scanner
  EOT
  tags                        = { Name = "${var.name_prefix}-internal-api" }
  depends_on                  = [aws_route_table_association.private, aws_iam_role_policy.read_scanner_key]
}
