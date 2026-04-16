# NetworkSettingsByoVpcAws

Use an existing VPC (AWS).

Alien validates the references but creates no networking infrastructure.
The customer is responsible for routing and egress (NAT, proxy, VPN, etc.).

## Example Usage

```typescript
import { NetworkSettingsByoVpcAws } from "@alienplatform/manager-api/models";

let value: NetworkSettingsByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
  ],
  publicSubnetIds: [],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `privateSubnetIds`                                  | *string*[]                                          | :heavy_check_mark:                                  | IDs of private subnets                              |
| `publicSubnetIds`                                   | *string*[]                                          | :heavy_check_mark:                                  | IDs of public subnets (required for public ingress) |
| `securityGroupIds`                                  | *string*[]                                          | :heavy_minus_sign:                                  | Optional security group IDs to use                  |
| `type`                                              | *"byo-vpc-aws"*                                     | :heavy_check_mark:                                  | N/A                                                 |
| `vpcId`                                             | *string*                                            | :heavy_check_mark:                                  | The ID of the existing VPC                          |