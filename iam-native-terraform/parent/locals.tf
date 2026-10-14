locals {
  resource_prefix = var.resource_prefix == "" ? format("a%s", random_id.resource_prefix.hex) : var.resource_prefix
  deployment_name = var.name
  deployment_platform = "aws"
  deployment_target = "aws"
  deployment_region = data.aws_region.current.region
  deployment_management_config = var.deployment_model == "push" ? { platform = "aws", managingRoleArn = var.managing_role_arn } : null
  advanced_settings = merge(jsondecode(var.advanced_settings_json), jsondecode(var.advanced_settings_overlay_json))
  deployment_settings = merge(local.advanced_settings, { deploymentModel = var.deployment_model, updates = var.updates_mode, telemetry = var.telemetry_mode, heartbeats = var.heartbeats_mode, endpointAccess = var.endpoint_access, network = jsondecode(var.network_mode == "create-new" ? jsonencode({ type = "create", cidr = var.vpc_cidr == "" ? null : var.vpc_cidr, availabilityZones = var.availability_zones }) : var.network_mode == "use-existing" ? jsonencode({ type = "byo-vpc-aws", vpcId = var.vpc_id, publicSubnetIds = compact([for id in var.public_subnet_ids : trimspace(id)]), privateSubnetIds = compact([for id in var.private_subnet_ids : trimspace(id)]), securityGroupIds = compact([for id in var.security_group_ids : trimspace(id)]) }) : jsonencode({ type = "use-default" })) })
  deployment_resources = [
    {
      id = "execution-sa"
      type = "service-account"
      importData = {
        roleName = aws_iam_role.execution_sa.name
        roleArn = aws_iam_role.execution_sa.arn
        stackPermissionsApplied = true
      }
    },
    {
      id = "default-network"
      type = "network"
      importData = {
        vpcId = null
        cidrBlock = null
        internetGatewayId = null
        natGatewayId = null
        eipAllocationId = null
        publicSubnetIds = []
        privateSubnetIds = []
        publicRouteTableId = null
        privateRouteTableId = null
        securityGroupId = null
        availabilityZones = []
        isByoVpc = true
      }
    },
    {
      id = "management"
      type = "remote-stack-management"
      importData = {
        roleName = aws_iam_role.management.name
        roleArn = aws_iam_role.management.arn
        managementPermissionsApplied = true
      }
    }
  ]
}
