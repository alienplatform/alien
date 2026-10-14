data "aws_vpc" "default_network_default" {
  default = true
}

data "aws_subnets" "default_network_default" {
  filter {
    name = "vpc-id"
    values = [
      data.aws_vpc.default_network_default.id
    ]
  }
}
