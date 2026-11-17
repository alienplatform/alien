resource "aws_security_group" "db" {
  name        = "${local.resource_prefix}-db-pg-sg"
  description = "Aurora Postgres security group"
  vpc_id      = var.network_mode == "create-new" ? aws_vpc.default_network[0].id : var.network_mode == "use-existing" ? var.vpc_id : data.aws_vpc.default_network_default[0].id

  egress {
    from_port = 0
    to_port   = 0
    protocol  = "-1"
    cidr_blocks = [
      "0.0.0.0/0"
    ]
  }

  tags = {
    "managed-by"    = "setup"
    deployment      = local.resource_prefix
    resource        = "db"
    "resource-type" = "postgres"
  }
}

resource "aws_security_group_rule" "db_ingress" {
  count                    = var.network_mode == "use-default" ? 0 : 1
  type                     = "ingress"
  protocol                 = "tcp"
  from_port                = 5432
  to_port                  = 5432
  security_group_id        = aws_security_group.db.id
  source_security_group_id = element(var.network_mode == "create-new" ? [aws_security_group.default_network_workload[0].id] : var.network_mode == "use-existing" ? compact([for id in var.security_group_ids : trimspace(id)]) : [], 0)
  description              = "Allow stack workloads to reach Postgres on 5432"
}

resource "aws_db_subnet_group" "db" {
  name       = "${local.resource_prefix}-db-subnets"
  subnet_ids = var.network_mode == "create-new" ? aws_subnet.default_network_private[*].id : var.network_mode == "use-existing" ? compact([for id in var.private_subnet_ids : trimspace(id)]) : data.aws_subnets.default_network_default[0].ids
  tags = {
    "managed-by"    = "setup"
    deployment      = local.resource_prefix
    resource        = "db"
    "resource-type" = "postgres"
  }
}

resource "random_password" "db" {
  length  = 60
  special = false
}

resource "aws_secretsmanager_secret" "db" {
  name        = "${local.resource_prefix}-db-credentials"
  description = "Aurora Postgres master password for 'db'"
  tags = {
    "managed-by"    = "setup"
    deployment      = local.resource_prefix
    resource        = "db"
    "resource-type" = "postgres"
  }
  recovery_window_in_days = 0
}

resource "aws_secretsmanager_secret_version" "db" {
  secret_id     = aws_secretsmanager_secret.db.id
  secret_string = random_password.db.result
}

resource "aws_rds_cluster_parameter_group" "db" {
  name_prefix = "${local.resource_prefix}-db-tls-"
  family      = "aurora-postgresql16"
  description = "Require TLS for Postgres connections"

  parameter {
    name         = "rds.force_ssl"
    value        = "1"
    apply_method = "pending-reboot"
  }

  lifecycle {
    create_before_destroy = true
  }

  tags = {
    "managed-by"    = "setup"
    deployment      = local.resource_prefix
    resource        = "db"
    "resource-type" = "postgres"
  }
}

resource "aws_rds_cluster" "db" {
  cluster_identifier              = "${local.resource_prefix}-db"
  db_cluster_parameter_group_name = aws_rds_cluster_parameter_group.db.name
  engine                          = "aurora-postgresql"
  engine_version                  = "16"
  master_username                 = "app_admin"
  master_password                 = random_password.db.result
  database_name                   = "app"
  db_subnet_group_name            = aws_db_subnet_group.db.name
  vpc_security_group_ids = [
    aws_security_group.db.id
  ]

  serverlessv2_scaling_configuration {
    min_capacity = 0
    max_capacity = 2
  }

  backup_retention_period = 7
  skip_final_snapshot     = true
  storage_encrypted       = true
  kms_key_id              = null
  apply_immediately       = true
  tags = {
    "managed-by"    = "setup"
    deployment      = local.resource_prefix
    resource        = "db"
    "resource-type" = "postgres"
  }
}

resource "aws_rds_cluster_instance" "db" {
  identifier                      = "${local.resource_prefix}-db-1"
  cluster_identifier              = aws_rds_cluster.db.id
  instance_class                  = "db.serverless"
  engine                          = "aurora-postgresql"
  publicly_accessible             = false
  performance_insights_enabled    = false
  performance_insights_kms_key_id = null
  tags = {
    "managed-by"    = "setup"
    deployment      = local.resource_prefix
    resource        = "db"
    "resource-type" = "postgres"
  }
}

resource "aws_iam_role_policy" "db_execution_sa_postgres-data-access_0" {
  name   = "db-postgres-data-access-0"
  role   = aws_iam_role.execution_sa.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "ReadConnectionSecret", Effect = "Allow", Action = ["secretsmanager:GetSecretValue"], Resource = ["arn:aws:secretsmanager:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:secret:${local.resource_prefix}-db-*"] }] })
}

resource "aws_iam_role_policy" "db_management_postgres-management_0" {
  name   = "db-postgres-management-0"
  role   = aws_iam_role.management.id
  policy = jsonencode({ Version = "2012-10-17", Statement = [{ Sid = "DescribeAndModifyAurora", Effect = "Allow", Action = ["rds:DescribeDBClusters", "rds:DescribeDBSubnetGroups", "rds:ModifyDBCluster", "rds:StartDBCluster", "rds:ModifyDBInstance", "rds:ListTagsForResource"], Resource = ["arn:aws:rds:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:cluster:${local.resource_prefix}-db", "arn:aws:rds:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:db:${local.resource_prefix}-db-*"] }, { Sid = "DescribeDatabaseInstances", Effect = "Allow", Action = ["rds:DescribeDBInstances"], Resource = ["*"] }] })
}
