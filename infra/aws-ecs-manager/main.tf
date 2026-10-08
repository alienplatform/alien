# The Alien manager on ECS Fargate: one task (the manager keeps a SQLite
# database, so there is a single writer), state on EFS, an Application Load
# Balancer in front, and the admin API key in Secrets Manager.

data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

locals {
  tags = merge(var.tags, {
    "alien:component" = "alien-manager"
    "alien:name"      = var.name
  })
  https    = var.certificate_arn != null
  host     = coalesce(var.domain_name, aws_lb.manager.dns_name)
  base_url = "${local.https ? "https" : "http"}://${local.host}"

  registry_config = var.create_artifact_repository ? join("\n", [
    "[artifact-registry.default]",
    "service = \"ecr\"",
    "repositoryPrefix = \"${aws_ecr_repository.releases[0].name}\"",
    "",
  ]) : ""
  config = join("\n", compact([local.registry_config, var.config]))
}

# --- Admin API key -----------------------------------------------------------

resource "random_password" "admin_token" {
  length  = 32
  special = false
  upper   = false
}

resource "aws_secretsmanager_secret" "admin_token" {
  name_prefix = "${var.name}-admin-token-"
  description = "Admin API key for the Alien manager"
  tags        = local.tags
}

resource "aws_secretsmanager_secret_version" "admin_token" {
  secret_id     = aws_secretsmanager_secret.admin_token.id
  secret_string = "ax_admin_${random_password.admin_token.result}"
}

# --- Release images ------------------------------------------------------------

resource "aws_ecr_repository" "releases" {
  count                = var.create_artifact_repository ? 1 : 0
  name                 = "${var.name}-releases"
  image_tag_mutability = "MUTABLE"
  force_delete         = false

  image_scanning_configuration {
    scan_on_push = true
  }
  encryption_configuration {
    encryption_type = "AES256"
  }
  tags = local.tags
}

# --- Network -------------------------------------------------------------------

resource "aws_security_group" "load_balancer" {
  name_prefix = "${var.name}-lb-"
  description = "Alien manager load balancer"
  vpc_id      = var.vpc_id
  tags        = local.tags

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_vpc_security_group_ingress_rule" "load_balancer" {
  for_each          = toset(flatten([for port in(local.https ? [80, 443] : [80]) : [for cidr in var.allowed_cidr_blocks : "${port}|${cidr}"]]))
  security_group_id = aws_security_group.load_balancer.id
  ip_protocol       = "tcp"
  from_port         = tonumber(split("|", each.value)[0])
  to_port           = tonumber(split("|", each.value)[0])
  cidr_ipv4         = split("|", each.value)[1]
}

resource "aws_vpc_security_group_egress_rule" "load_balancer_to_task" {
  security_group_id            = aws_security_group.load_balancer.id
  ip_protocol                  = "tcp"
  from_port                    = 8080
  to_port                      = 8080
  referenced_security_group_id = aws_security_group.task.id
}

resource "aws_security_group" "task" {
  name_prefix = "${var.name}-task-"
  description = "Alien manager task"
  vpc_id      = var.vpc_id
  tags        = local.tags

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_vpc_security_group_ingress_rule" "task_from_load_balancer" {
  security_group_id            = aws_security_group.task.id
  ip_protocol                  = "tcp"
  from_port                    = 8080
  to_port                      = 8080
  referenced_security_group_id = aws_security_group.load_balancer.id
}

resource "aws_vpc_security_group_egress_rule" "task_out" {
  security_group_id = aws_security_group.task.id
  ip_protocol       = "-1"
  cidr_ipv4         = "0.0.0.0/0"
}

resource "aws_security_group" "efs" {
  name_prefix = "${var.name}-efs-"
  description = "Alien manager file system"
  vpc_id      = var.vpc_id
  tags        = local.tags

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_vpc_security_group_ingress_rule" "efs_from_task" {
  security_group_id            = aws_security_group.efs.id
  ip_protocol                  = "tcp"
  from_port                    = 2049
  to_port                      = 2049
  referenced_security_group_id = aws_security_group.task.id
}

# --- State -----------------------------------------------------------------------

resource "aws_efs_file_system" "data" {
  creation_token = "${var.name}-data"
  encrypted      = true
  tags           = merge(local.tags, { Name = "${var.name}-data" })
}

resource "aws_efs_mount_target" "data" {
  for_each        = toset(var.task_subnet_ids)
  file_system_id  = aws_efs_file_system.data.id
  subnet_id       = each.value
  security_groups = [aws_security_group.efs.id]
}

# The image runs as uid/gid 65532 and keeps its state in /data.
resource "aws_efs_access_point" "data" {
  file_system_id = aws_efs_file_system.data.id
  posix_user {
    uid = 65532
    gid = 65532
  }
  root_directory {
    path = "/alien-manager"
    creation_info {
      owner_uid   = 65532
      owner_gid   = 65532
      permissions = "0700"
    }
  }
  tags = local.tags
}

# --- Load balancer ---------------------------------------------------------------

resource "aws_lb" "manager" {
  name_prefix        = substr(replace(var.name, "/[^a-zA-Z0-9]/", ""), 0, 6)
  load_balancer_type = "application"
  internal           = var.internal
  subnets            = var.load_balancer_subnet_ids
  security_groups    = [aws_security_group.load_balancer.id]
  # Tunnel connections and large image uploads stay open for a long time.
  idle_timeout = 3600
  tags         = local.tags
}

resource "aws_lb_target_group" "manager" {
  name_prefix          = substr(replace(var.name, "/[^a-zA-Z0-9]/", ""), 0, 6)
  port                 = 8080
  protocol             = "HTTP"
  target_type          = "ip"
  vpc_id               = var.vpc_id
  deregistration_delay = 30

  health_check {
    path                = "/health"
    matcher             = "200"
    interval            = 15
    healthy_threshold   = 2
    unhealthy_threshold = 3
  }
  tags = local.tags

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_lb_listener" "https" {
  count             = local.https ? 1 : 0
  load_balancer_arn = aws_lb.manager.arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS13-1-2-2021-06"
  certificate_arn   = var.certificate_arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.manager.arn
  }
}

resource "aws_lb_listener" "http" {
  load_balancer_arn = aws_lb.manager.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = local.https ? "redirect" : "forward"
    target_group_arn = local.https ? null : aws_lb_target_group.manager.arn

    dynamic "redirect" {
      for_each = local.https ? [1] : []
      content {
        port        = "443"
        protocol    = "HTTPS"
        status_code = "HTTP_301"
      }
    }
  }
}

# --- Task --------------------------------------------------------------------------

resource "aws_cloudwatch_log_group" "manager" {
  name              = "/alien/${var.name}"
  retention_in_days = var.log_retention_days
  tags              = local.tags
}

data "aws_iam_policy_document" "ecs_tasks_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "execution" {
  name_prefix        = "${var.name}-exec-"
  assume_role_policy = data.aws_iam_policy_document.ecs_tasks_assume.json
  tags               = local.tags
}

resource "aws_iam_role_policy_attachment" "execution" {
  role       = aws_iam_role.execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role_policy" "execution_secret" {
  name = "read-admin-token"
  role = aws_iam_role.execution.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = [aws_secretsmanager_secret.admin_token.arn]
    }]
  })
}

resource "aws_iam_role" "task" {
  name_prefix        = "${var.name}-task-"
  assume_role_policy = data.aws_iam_policy_document.ecs_tasks_assume.json
  tags               = local.tags
}

resource "aws_iam_role_policy" "task_efs" {
  name = "efs"
  role = aws_iam_role.task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["elasticfilesystem:ClientMount", "elasticfilesystem:ClientWrite"]
      Resource = [aws_efs_file_system.data.arn]
      Condition = {
        StringEquals = { "elasticfilesystem:AccessPointArn" = aws_efs_access_point.data.arn }
      }
    }]
  })
}

resource "aws_iam_role_policy" "task_ecr" {
  count = var.create_artifact_repository ? 1 : 0
  name  = "release-images"
  role  = aws_iam_role.task.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = ["ecr:GetAuthorizationToken"]
        Resource = ["*"]
      },
      {
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability", "ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer",
          "ecr:InitiateLayerUpload", "ecr:UploadLayerPart", "ecr:CompleteLayerUpload",
          "ecr:PutImage", "ecr:DescribeImages", "ecr:DescribeRepositories", "ecr:CreateRepository",
          "ecr:ListImages",
        ]
        Resource = ["arn:aws:ecr:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:repository/${var.name}-releases*"]
      },
    ]
  })
}

resource "aws_ecs_cluster" "manager" {
  name = var.name
  setting {
    name  = "containerInsights"
    value = "enabled"
  }
  tags = local.tags
}

resource "aws_ecs_task_definition" "manager" {
  family                   = var.name
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = var.cpu
  memory                   = var.memory
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.task.arn

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "ARM64"
  }

  volume {
    name = "data"
    efs_volume_configuration {
      file_system_id     = aws_efs_file_system.data.id
      transit_encryption = "ENABLED"
      authorization_config {
        access_point_id = aws_efs_access_point.data.id
        iam             = "ENABLED"
      }
    }
  }

  container_definitions = jsonencode([{
    name         = "manager"
    image        = var.image
    essential    = true
    portMappings = [{ containerPort = 8080, protocol = "tcp" }]
    environment = concat(
      [
        { name = "BASE_URL", value = local.base_url },
        { name = "PORT", value = "8080" },
        { name = "STATE_DIR", value = "/data" },
        { name = "AWS_REGION", value = data.aws_region.current.region },
      ],
      local.config == "" ? [] : [{ name = "ALIEN_MANAGER_CONFIG", value = local.config }],
      [for key, value in var.environment : { name = key, value = value }],
    )
    secrets = [{
      name      = "ALIEN_ADMIN_TOKEN"
      valueFrom = aws_secretsmanager_secret.admin_token.arn
    }]
    mountPoints            = [{ sourceVolume = "data", containerPath = "/data" }]
    readonlyRootFilesystem = false
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = aws_cloudwatch_log_group.manager.name
        awslogs-region        = data.aws_region.current.region
        awslogs-stream-prefix = "manager"
      }
    }
  }])
  tags = local.tags
}

resource "aws_ecs_service" "manager" {
  name            = var.name
  cluster         = aws_ecs_cluster.manager.id
  task_definition = aws_ecs_task_definition.manager.arn
  desired_count   = 1
  launch_type     = "FARGATE"

  # Stop the old task before starting the new one: one database writer.
  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100

  network_configuration {
    subnets          = var.task_subnet_ids
    security_groups  = [aws_security_group.task.id]
    assign_public_ip = var.assign_public_ip
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.manager.arn
    container_name   = "manager"
    container_port   = 8080
  }

  health_check_grace_period_seconds = 60
  depends_on                        = [aws_lb_listener.http, aws_efs_mount_target.data]
  tags                              = local.tags
}
