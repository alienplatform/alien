terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

locals {
  common_tags = merge(var.tags, {
    "alien:managed-by" = "terraform"
    "alien:component"  = "alien-manager"
    "alien:name"       = var.name
  })
}
