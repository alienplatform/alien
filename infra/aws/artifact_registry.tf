# -----------------------------------------------------------------------------
# ECR Repository (conditional on enable_artifact_registry)
# -----------------------------------------------------------------------------

resource "aws_ecr_repository" "artifacts" {
  count = var.enable_artifact_registry ? 1 : 0

  name                 = "${var.name}-artifacts"
  image_tag_mutability = "IMMUTABLE"
  tags                 = local.common_tags

  image_scanning_configuration {
    scan_on_push = true
  }

  encryption_configuration {
    encryption_type = "AES256"
  }
}

resource "aws_ecr_lifecycle_policy" "artifacts" {
  count = var.enable_artifact_registry ? 1 : 0

  repository = aws_ecr_repository.artifacts[0].name

  policy = jsonencode({
    rules = [
      {
        rulePriority = 1
        description  = "Keep last 50 images"
        selection = {
          tagStatus   = "any"
          countType   = "imageCountMoreThan"
          countNumber = 50
        }
        action = {
          type = "expire"
        }
      }
    ]
  })
}
